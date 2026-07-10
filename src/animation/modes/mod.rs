//! The animation modes: 1:1 Rust ports of xscreensaver / xlockmore hacks.
//!
//! Port-inherent clippy noise is allowed for every mode via `ported!` below;
//! everything else must pass `cargo clippy -- -D warnings`.

// The three allowed lints are inherent to faithful C ports and would demand
// hundreds of fidelity-losing rewrites:
//   - excessive_precision: float literals copied verbatim from the C sources
//   - too_many_arguments:  helper fns mirror the upstream C signatures
//   - needless_range_loop: C-style indexed loops kept 1:1 with the originals
macro_rules! ported {
    ($($m:ident),* $(,)?) => {
        $(
            #[allow(
                clippy::excessive_precision,
                clippy::too_many_arguments,
                clippy::needless_range_loop
            )]
            pub mod $m;
        )*
    };
}

ported!(
    abstractile,
    anemone,
    apollonian,
    attraction,
    ball,
    binaryhorizon,
    binaryring,
    blaster,
    blot,
    bouboule,
    bounce,
    boxfit,
    braid,
    bubble,
    ccurve,
    coral,
    crystal,
    cynosure,
    discrete,
    drift,
    euler2d,
    fiberlamp,
    flame,
    flow,
    fluidballs,
    forest,
    galaxy,
    goop,
    grav,
    helix,
    hop,
    ico,
    ifs,
    intermomentary,
    julia,
    life3d,
    lightning,
    lisa,
    lissie,
    mandelbrot,
    matrix,
    maze,
    moire,
    molecule,
    mountain,
    noof,
    petri,
    piecewise,
    polyominoes,
    popsquares,
    pyro,
    pyro2,
    qix,
    rain,
    scooter,
    space,
    spiral,
    squiral,
    starfish,
    vermiculate,
    vines,
    whirlwindwarp,
    worm,
    wormhole,
    xrayswarm,
);
