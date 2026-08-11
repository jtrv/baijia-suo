#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
baseline=local
criterion_dir=target/criterion

# Refuse to measure on a busy machine. Background load moves these numbers by
# ~10% — twice the regressed threshold — so a run under load reports verdicts
# that say nothing about the code. Measured: identical code back to back
# scored -11% to +8% at loadavg 3.6, and stayed within ~1% when idle.
# BENCH_MAX_LOAD tunes the ceiling; BENCH_FORCE=1 measures anyway.
max_load=${BENCH_MAX_LOAD:-1.0}
load=$(awk '{print $1}' /proc/loadavg)
if [ -z "${BENCH_FORCE:-}" ] && [ "$(awk -v l="$load" -v m="$max_load" 'BEGIN{print (l>m)}')" = 1 ]; then
    echo "bench-verdict: loadavg $load exceeds $max_load — refusing to measure." >&2
    echo "  Wait for the machine to settle, or re-run with BENCH_FORCE=1 to" >&2
    echo "  measure anyway (the verdict will be noise, not a result)." >&2
    exit 2
fi

if find "$criterion_dir" -path "*/$baseline/estimates.json" -print -quit 2>/dev/null | grep -q .; then
    cargo +stable bench --bench animations -- --baseline "$baseline"
    estimates=change
else
    cargo +stable bench --bench animations -- --save-baseline "$baseline"
    estimates=$baseline
fi

python3 - "$criterion_dir" "$estimates" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
estimate_set = sys.argv[2]
thresholds = {}
for line in pathlib.Path("benches/thresholds.toml").read_text().splitlines():
    line = line.split("#", 1)[0].strip()
    if "=" in line:
        key, value = line.split("=", 1)
        thresholds[key.strip()] = float(value)

improved = thresholds["improved_percent"]
regressed = thresholds["regressed_percent"]
verdicts = []
for path in sorted(root.glob(f"**/{estimate_set}/estimates.json")):
    case = "/".join(path.relative_to(root).parts[:-2])
    if estimate_set == "change":
        percent = json.loads(path.read_text())["mean"]["point_estimate"] * 100
    else:
        percent = 0.0
    verdict = "REGRESSED" if percent >= regressed else "IMPROVED" if percent <= improved else "UNCHANGED"
    verdicts.append(verdict)
    print(f"{case}: {verdict} {percent:+.2f}%")

if not verdicts:
    raise SystemExit("no Criterion estimates found")
worst = "REGRESSED" if "REGRESSED" in verdicts else "UNCHANGED" if "UNCHANGED" in verdicts else "IMPROVED"
print(f"VERDICT: {worst} (improved <= {improved:+g}%, regressed >= {regressed:+g}%)")
PY
