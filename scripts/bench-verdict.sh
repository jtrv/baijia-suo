#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
baseline=local
criterion_dir=target/criterion

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
