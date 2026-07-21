#!/usr/bin/env bash
# Visual QA harness: play every animation in turn inside a nested niri so you
# can eyeball each one without relocking your real session by hand.
#
# How it works:
#   - A generated niri config spawns `baijia-suo -A <animation>` at startup.
#     niri, launched inside your existing Wayland session, runs nested
#     (windowed) and its spawned child locks THAT nested instance — your host
#     session is never touched.
#   - watchexec -r watches the config and restarts niri whenever it changes.
#   - A background rotator rewrites the config every INTERVAL seconds to the
#     next animation, so niri restarts into each mode in sequence.
#
# Usage:
#   scripts/visual-test.sh [-i SECONDS] [animation ...]
#     -i SECONDS   seconds per animation (default 10)
#     animation... explicit list to cycle (default: every registered mode)
#
#   Ctrl-C stops the rotator, watchexec, and the nested niri, and removes the
#   generated config.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
bin="$repo/target/release/baijia-suo"
config="$repo/.visual-test.kdl"
interval=10

while getopts "i:h" opt; do
    case "$opt" in
        i) interval="$OPTARG" ;;
        h) sed -n '2,20p' "$0"; exit 0 ;;
        *) exit 2 ;;
    esac
done
shift $((OPTIND - 1))

for tool in niri watchexec; do
    command -v "$tool" >/dev/null || { echo "error: $tool not found" >&2; exit 1; }
done

# Build the release binary if it is missing. It is NOT rebuilt when present —
# rebuild yourself (cargo build --release) after code changes so the harness
# tests what you just wrote.
if [ ! -x "$bin" ]; then
    echo "[visual-test] building release binary..."
    (cd "$repo" && cargo build --release)
fi

# Animation list: explicit args, else every registered mode.
if [ "$#" -gt 0 ]; then
    anims=("$@")
else
    mapfile -t anims < <("$bin" --list-animations)
fi
[ "${#anims[@]}" -gt 0 ] || { echo "error: no animations to show" >&2; exit 1; }

write_config() {
    # Minimal nested-niri config: just spawn the locker with one animation.
    # niri fills in every other default.
    printf 'spawn-sh-at-startup "%s -A %s"\n' "$bin" "$1" > "$config"
}

rotator_pid=""
cleanup() {
    [ -n "$rotator_pid" ] && kill "$rotator_pid" 2>/dev/null || true
    rm -f "$config"
}
trap cleanup EXIT INT TERM

# Show the first mode, then rotate. The rotator sleeps first so each mode
# (including the first) gets a full interval before the switch.
write_config "${anims[0]}"
echo "[visual-test] showing ${anims[0]} (1/${#anims[@]})"
(
    i=0
    n=${#anims[@]}
    while true; do
        sleep "$interval"
        i=$(( (i + 1) % n ))
        write_config "${anims[i]}"
        echo "[visual-test] showing ${anims[i]} ($((i + 1))/$n)"
    done
) &
rotator_pid=$!

# watchexec restarts the nested niri each time the rotator rewrites the config.
exec watchexec -r -w "$config" -- niri -c "$config"
