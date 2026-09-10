#!/usr/bin/env bash
#MISE description="Visual QA: cycle every animation in a nested niri"
# Visual QA harness: run the locker inside a nested niri so you can eyeball it
# without relocking your real session by hand. Every qa task goes through here,
# so your real session is never locked. Build first (`mise run release`) — the
# harness only builds when the binary is missing, so it always shows the last
# build you made.
#
# Two shapes:
#   rotate (default) — play every animation in turn, hands-off.
#   static (-s)      — one run you can interact with: type to exercise the
#                      indicator, read --debug-timing output, drive a config.
#
# How it works:
#   - A generated niri config spawns baijia-suo at startup. niri, launched
#     inside your existing Wayland session, runs nested (windowed) and its
#     spawned child locks THAT nested instance — your host session is never
#     touched.
#   - rotate mode: watchexec -r watches the config while a background rotator
#     rewrites it every INTERVAL seconds, so niri restarts into each mode in
#     sequence.
#   - static mode: niri runs once, no rotator, no watchexec.
#
# Usage:
#   mise run qa [-i SECONDS] [animation ...]
#   mise run qa -- -s [-- LOCKER_ARG ...]
#     -i SECONDS   seconds per animation (default 10, rotate mode only)
#     -s           static: one session, no rotation — for interactive tests
#     animation... explicit list to cycle (default: every registered mode)
#     -- ARG...    extra args passed straight to baijia-suo
#
# Examples:
#   mise run qa ico rain      # eyeball two modes in turn
#   mise run qa -- -s -- -C .config/mise/qa/playlist.toml
#   mise run qa -- -s -- --debug-timing -A petri
#   mise run qa -- -s -- --indicator-mode ripple   # then type
#
#   The qa-playlist / qa-capped / qa-timing / qa-indicator tasks are those
#   invocations under their own names; extra args are appended, so
#   `mise run qa-indicator ripple` picks the ripple style.
#
#   Ctrl-C stops everything and removes the generated config.
set -euo pipefail

repo="$MISE_PROJECT_ROOT"
bin="$repo/target/release/baijia-suo"
config="$repo/.visual-test.kdl"
interval=10
static=0

while getopts "i:sh" opt; do
    case "$opt" in
        i) interval="$OPTARG" ;;
        s) static=1 ;;
        h) awk 'NR>2 && /^#/ {print; next} NR>2 {exit}' "$0"; exit 0 ;;
        *) exit 2 ;;
    esac
done
shift $((OPTIND - 1))

# watchexec only drives the rotate loop; static mode runs niri once.
tools=(niri)
[ "$static" -eq 1 ] || tools+=(watchexec)
for tool in "${tools[@]}"; do
    command -v "$tool" >/dev/null || { echo "error: $tool not found" >&2; exit 1; }
done

# Build the release binary if it is missing. It is NOT rebuilt when present —
# rebuild yourself (cargo build --release) after code changes so the harness
# tests what you just wrote.
if [ ! -x "$bin" ]; then
    echo "[visual-test] building release binary..."
    (cd "$repo" && cargo build --release)
fi

cleanup_config() { rm -f "$config"; }

# Static: one nested session with whatever args you passed, no rotation. The
# locker keeps running until you unlock or Ctrl-C, so you can type at the
# indicator or watch --debug-timing scroll.
if [ "$static" -eq 1 ]; then
    trap cleanup_config EXIT INT TERM
    # %q-quote each arg so paths with spaces survive the KDL string and the
    # shell that spawn-sh-at-startup runs it through.
    printf 'spawn-sh-at-startup "%s' "$bin" > "$config"
    for arg in "$@"; do printf ' %s' "$(printf '%q' "$arg")" >> "$config"; done
    printf '"\n' >> "$config"
    echo "[visual-test] static session: $bin $*"
    exec niri -c "$config"
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
