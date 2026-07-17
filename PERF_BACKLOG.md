# Performance backlog

Long-tail ideas for reducing frame cost while keeping visual parity with the
xscreensaver originals. Sourced from a 5-agent code sweep (2026-07-16) plus
follow-up ideas. Quick wins from that sweep were applied directly and are not
listed here; this file tracks the **structural** work — real refactors that
need care, profiling, or design.

Format per item: problem → fix sketch → impact / risk / effort.
Status: `todo` | `in-progress` | `done` | `wontfix (reason)`.

## From the sweep

### 1. petri: incremental render instead of full grid rescan — `done`
`src/animation/modes/petri.rs:270-305`, `clears_each_frame() == true`.
`tick()` already maintains a doubly-linked active-cell list (`head`/`next`/
`prev`) of exactly the cells changing, but `render()` ignores it and rescans
the whole `arr_width × arr_height` grid every frame after a full clear, even
though most of the colony is settled and unchanging.
**Fix:** track dirty cells (append on `newcell`/`killcell`/settle), switch to
`clears_each_frame() == false` + incremental draws — the pattern
`lissie.rs`/`maze.rs` already use.
Impact: high (scales with screen area / cell_size²). Risk: low. Effort: medium.

### 2. polyominoes: incremental render instead of full board rescan — `done` (bitmap style; plain style kept full redraw, see commit)
`src/animation/modes/polyominoes.rs:1328-1447`, `clears_each_frame() == true`.
`tick()` attaches at most one piece per call, but render walks the whole
`bw × bh` board (with 8-neighbor lookups per cell in the bitmap path) after a
full clear.
**Fix:** redraw only newly-changed cells and their neighbors (bitmap edge
detection depends on neighbors), drop the full clear.
Impact: high for small box_size. Risk: low. Effort: medium.

### 3. vermiculate: persistent BGRA canvas — `done`
`src/animation/modes/vermiculate.rs:1166-1188`.
`render()` scans the entire `width × height` pixel-index buffer per frame
(read + branch + palette lookup + `put_pixel` per pixel) though a tick writes
only a handful of pixels via `sp()`.
**Fix:** keep a persistent BGRA canvas mirroring `point[]`, write into it in
`sp()`, make `render()` a `copy_from_slice` — the pattern `squiral.rs`,
`whirlwindwarp.rs`, `xrayswarm.rs`, `worm.rs` already use. Palette repaint
sites (autopal / `clearscreen()` / `reset_p`) must rebuild the canvas.
Impact: high. Risk: low. Effort: medium.

### 4. binaryring: store BGRA instead of packed u32 — `done`
`src/animation/modes/binaryring.rs:342-357`.
Internal buffer is packed `0x00RRGGBB` u32; `render()` converts the full
screen to BGRA every call. Sibling `binaryhorizon.rs` (same author/algorithm)
stores BGRA `u8` directly and renders with a plain `copy_from_slice`.
**Fix:** port binaryhorizon's buffer format; `draw_point`/`dla_plot` write
BGRA directly; render becomes memcpy.
Impact: high (mode runs at 100 fps uncapped). Risk: none. Effort: small-medium.

### 5. Dirty-rect damage tracking — `todo`
`src/wayland.rs:204` submits `damage_buffer(0, 0, pw, ph)` on every commit,
even when only the small indicator region changed (animation clock didn't
elapse). Forces full-surface recomposite on every ~16 ms indicator frame
while typing.
**Fix:** damage the union of previous ∪ current indicator rect (bbox already
computed in `render_indicator` — `src/render/indicator/mod.rs:211-212`,
`303-306`) when the animation buffer wasn't touched; full damage when the
animation ticked or on first frame/resize.
Impact: medium (compositor/iGPU bandwidth, battery). **Risk: medium — stale
prev-rect bookkeeping leaves ghosting.** Effort: medium (rect plumbing from
`render_to_surface` up to `draw()`).

### 6. moire: blit only the modified row range — `done`
`src/animation/modes/moire.rs:183-189`.
`tick()` touches only `CHUNK_SIZE = 20` rows, but `render()` copies the whole
buffer every frame.
**Fix:** track last-modified row range, copy only those rows.
Impact: medium. Risk: none. Effort: small (kept here because it needs a
one-frame full-copy path after resize/reset to stay correct).

### 7. piecewise: incremental arc rasterization — `todo`
`src/animation/modes/piecewise.rs:796-811`.
`draw_arc` calls `.cos()`/`.sin()` at every arc step; steps scale with radius,
across ~32 circles per frame — thousands of trig pairs per frame.
**Fix:** compute `(cos Δθ, sin Δθ)` once per arc, advance points with the 2D
rotation recurrence. Verify visually — recurrence drift is negligible at these
step counts but this is a numeric-behavior change.
Impact: medium-high. Risk: low. Effort: small-medium.

## Additional candidates (not from the sweep — unverified, profile first)

### 8. Frame-callback-driven rendering — `done`
Animation timing is timer-driven (`tick_timers` + `next_wake`); the surface
commits regardless of visibility. Driving redraws from `wl_surface::frame`
callbacks would let the compositor throttle us for free when the output is
off (DPMS), occluded, or on an idle VT — likely the single biggest battery
win for a lock screen, which spends most of its life with the screen off.
Needs care: PAM/auth timers must keep running independently of frame
callbacks. Impact: potentially very high. Risk: medium. Effort: large.

### 9. Skip render work entirely while all outputs are off — `done` (falls out of #8: withheld callbacks queue-and-disarm the whole render path)
Related to #8 but cheaper: if the compositor signals output power-off (or no
frame callback returns for N seconds), stop the animation clock (`next_wake =
None`) and re-arm on the next input/output event. Impact: high on battery.
Risk: low. Effort: small-medium.

### 10. Render at logical resolution on HiDPI via wp_viewport — `todo`
On scale-2 outputs the animation renders at 4× the pixels of the logical
size. xscreensaver-era hacks were designed for ~1080p or less; rendering the
animation layer at logical resolution and letting the compositor scale via
`wp_viewporter` would quarter the per-frame work with little perceptible
difference for most modes (the indicator should stay native-res). Opt-in
knob, since some fine-detail modes (matrix, mandelbrot) may visibly soften.
Impact: high on HiDPI. Risk: medium (visual parity). Effort: medium.

### 11. Share one player across same-sized outputs — `todo`
Verify whether multi-monitor setups run one animation+buffer per output; if
so, same-sized outputs could share a single rendered frame and blit twice.
Impact: high for multi-monitor. Risk: low. Effort: medium (check current
behavior first — may already be shared).

### 12. Adaptive cap on battery — `done` (as low_battery_percent power saver: animations suspend below threshold)
Read `/sys/class/power_supply/*/status` at start (and on a slow poll);
when discharging, apply a default `max_fps` (e.g. 30-60) unless the user set
one explicitly. Pure policy on top of the existing knob. Impact: medium.
Risk: none. Effort: small.

### 13. u32-batched pixel writes in primitives — `todo`
`put_pixel` writes 4 individual `u8`s with per-pixel bounds checks; horizontal
runs (`fill_rect`, scanline fills, `clear_buffer` already does this) could
write `u32`s via `align_to_mut` and clip once per run instead of per pixel.
The sweep rated broad `put_pixel` changes risky for parity (rasterization
edge cases) — restrict to run-based fills, keep Bresenham paths untouched,
and only with a profiler showing it matters. Impact: low-medium. Risk: low
if fill-only. Effort: medium.

### 14. Shared sin/cos LUT in primitives — `todo`
Several modes tabulate their own trig (`worm.rs` via `OnceLock`) while others
call `sin`/`cos` in hot loops (piecewise, whirlwindwarp, crystal — the hot
call sites got hoisted in the quick-wins pass, but new modes keep
reinventing this). A shared degree- or fixed-point-indexed LUT in
`primitives` would make the cheap path the default one. Impact: low-medium.
Risk: none (LUT resolution ≥ what modes use today). Effort: small.

### 15. Render directly into wl_shm, skip blit_into — `todo`
Every frame pays a full-buffer copy from the player's persistent canvas into
the wl_shm buffer. Non-clearing modes need the persistent canvas (shm buffers
rotate), but clearing modes (`clears_each_frame() == true`) rebuild the frame
from scratch anyway and could render straight into the shm buffer, saving an
~8 MB copy per frame at 1080p. Impact: medium. Risk: low. Effort: medium
(per-output sizing lives in the player today).
