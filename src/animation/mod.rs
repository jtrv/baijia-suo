#![forbid(unsafe_code)]

pub mod modes;
mod player;
mod playlist;
pub mod primitives;

pub use player::AnimationPlayer;
pub use playlist::Playlist;

/// Configuration for an animation mode.
#[derive(Debug, Clone)]
pub struct AnimConfig {
    pub width: u32,
    pub height: u32,
    pub count: i32,
    pub cycles: i32,
    pub size: i32,
    pub ncolors: i32,
    pub delay_us: u64,
    /// Frame-rate cap; 0 = uncapped (each mode's own clock, the original
    /// behavior). Only read by the player, never by modes.
    pub max_fps: u32,
}

impl Default for AnimConfig {
    fn default() -> Self {
        AnimConfig {
            width: 800,
            height: 600,
            count: 0,
            cycles: 0,
            size: 1,
            ncolors: 64,
            delay_us: 20_000, // 50fps
            max_fps: 0,
        }
    }
}

/// How a mode's `render()` relates to its `tick()`s — the player's
/// scheduling contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderPolicy {
    /// `render()` consumes per-tick state (op queues, dirty lists): tick and
    /// render must stay paired 1:1. The safe default.
    Incremental,
    /// `render()` redraws the complete current state onto a cleared buffer:
    /// ticks may be batched, then the player clears and renders once.
    ClearThenRender,
    /// `render()` overwrites the full frame from a self-contained canvas:
    /// ticks may be batched and no clear is needed — batching skips
    /// intermediate full-canvas copies that could never be presented.
    CompleteFrame,
}

/// The core animation trait.
pub trait Animation: Send {
    /// Create a new animation instance.
    fn new(config: &AnimConfig) -> Self
    where
        Self: Sized;

    /// Advance the animation by one frame.
    fn tick(&mut self);

    /// Render the current state to the pixel buffer.
    fn render(&self, buffer: &mut [u8], width: u32, height: u32);

    /// Requires a non-incremental render policy and interpolatable previous/current states.
    fn interpolates(&self) -> bool {
        false
    }

    /// Render between the previous and current tick (one tick of latency).
    fn render_interpolated(&self, buffer: &mut [u8], width: u32, height: u32, _fraction: f64) {
        self.render(buffer, width, height);
    }

    /// Reset the animation state.
    fn reset(&mut self, config: &AnimConfig);

    /// The tick/render scheduling contract for this mode.
    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::Incremental
    }

    /// The target delay between frames in microseconds.
    fn frame_delay_us(&self) -> u64;
}

type AnimConstructor = fn(&AnimConfig) -> Box<dyn Animation>;

static MODES: &[(&str, AnimConstructor)] = &[
    ("abstractile", |cfg| {
        Box::new(modes::abstractile::Abstractile::new(cfg))
    }),
    ("anemone", |cfg| Box::new(modes::anemone::Anemone::new(cfg))),
    ("apollonian", |cfg| {
        Box::new(modes::apollonian::Apollonian::new(cfg))
    }),
    ("attraction", |cfg| {
        Box::new(modes::attraction::Attraction::new(cfg))
    }),
    ("ball", |cfg| Box::new(modes::ball::Ball::new(cfg))),
    ("binaryhorizon", |cfg| {
        Box::new(modes::binaryhorizon::BinaryHorizon::new(cfg))
    }),
    ("binaryring", |cfg| {
        Box::new(modes::binaryring::BinaryRing::new(cfg))
    }),
    ("blaster", |cfg| Box::new(modes::blaster::Blaster::new(cfg))),
    ("blot", |cfg| Box::new(modes::blot::Blot::new(cfg))),
    ("bouboule", |cfg| {
        Box::new(modes::bouboule::Bouboule::new(cfg))
    }),
    ("bounce", |cfg| Box::new(modes::bounce::Bounce::new(cfg))),
    ("boxfit", |cfg| Box::new(modes::boxfit::BoxFit::new(cfg))),
    ("braid", |cfg| Box::new(modes::braid::Braid::new(cfg))),
    ("bubble", |cfg| Box::new(modes::bubble::Bubble::new(cfg))),
    ("ccurve", |cfg| Box::new(modes::ccurve::CCurve::new(cfg))),
    ("coral", |cfg| Box::new(modes::coral::Coral::new(cfg))),
    ("crystal", |cfg| Box::new(modes::crystal::Crystal::new(cfg))),
    ("cynosure", |cfg| {
        Box::new(modes::cynosure::Cynosure::new(cfg))
    }),
    ("discrete", |cfg| {
        Box::new(modes::discrete::Discrete::new(cfg))
    }),
    ("drift", |cfg| Box::new(modes::drift::Drift::new(cfg))),
    ("euler2d", |cfg| Box::new(modes::euler2d::Euler2D::new(cfg))),
    ("fiberlamp", |cfg| {
        Box::new(modes::fiberlamp::Fiberlamp::new(cfg))
    }),
    ("flame", |cfg| Box::new(modes::flame::Flame::new(cfg))),
    ("flow", |cfg| Box::new(modes::flow::Flow::new(cfg))),
    ("fluidballs", |cfg| {
        Box::new(modes::fluidballs::FluidBalls::new(cfg))
    }),
    ("forest", |cfg| Box::new(modes::forest::Forest::new(cfg))),
    ("galaxy", |cfg| Box::new(modes::galaxy::Galaxy::new(cfg))),
    ("goop", |cfg| Box::new(modes::goop::Goop::new(cfg))),
    ("grav", |cfg| Box::new(modes::grav::Grav::new(cfg))),
    ("helix", |cfg| Box::new(modes::helix::Helix::new(cfg))),
    ("hop", |cfg| Box::new(modes::hop::Hop::new(cfg))),
    ("ico", |cfg| Box::new(modes::ico::Ico::new(cfg))),
    ("ifs", |cfg| Box::new(modes::ifs::Ifs::new(cfg))),
    ("intermomentary", |cfg| {
        Box::new(modes::intermomentary::Intermomentary::new(cfg))
    }),
    ("julia", |cfg| Box::new(modes::julia::Julia::new(cfg))),
    ("life3d", |cfg| Box::new(modes::life3d::Life3D::new(cfg))),
    ("lightning", |cfg| {
        Box::new(modes::lightning::Lightning::new(cfg))
    }),
    ("lisa", |cfg| Box::new(modes::lisa::Lisa::new(cfg))),
    ("lissie", |cfg| Box::new(modes::lissie::Lissie::new(cfg))),
    ("mandelbrot", |cfg| {
        Box::new(modes::mandelbrot::Mandelbrot::new(cfg))
    }),
    ("matrix", |cfg| Box::new(modes::matrix::Matrix::new(cfg))),
    ("maze", |cfg| Box::new(modes::maze::Maze::new(cfg))),
    ("moire", |cfg| Box::new(modes::moire::Moire::new(cfg))),
    ("molecule", |cfg| {
        Box::new(modes::molecule::Molecule::new(cfg))
    }),
    ("mountain", |cfg| {
        Box::new(modes::mountain::Mountain::new(cfg))
    }),
    ("noof", |cfg| Box::new(modes::noof::Noof::new(cfg))),
    ("petri", |cfg| Box::new(modes::petri::Petri::new(cfg))),
    ("piecewise", |cfg| {
        Box::new(modes::piecewise::Piecewise::new(cfg))
    }),
    ("polyominoes", |cfg| {
        Box::new(modes::polyominoes::Polyominoes::new(cfg))
    }),
    ("popsquares", |cfg| {
        Box::new(modes::popsquares::PopSquares::new(cfg))
    }),
    ("pyro2", |cfg| Box::new(modes::pyro2::Pyro2::new(cfg))),
    ("pyro", |cfg| Box::new(modes::pyro::Pyro::new(cfg))),
    ("qix", |cfg| Box::new(modes::qix::Qix::new(cfg))),
    ("rain", |cfg| Box::new(modes::rain::Rain::new(cfg))),
    ("scooter", |cfg| Box::new(modes::scooter::Scooter::new(cfg))),
    ("space", |cfg| Box::new(modes::space::Space::new(cfg))),
    ("spiral", |cfg| Box::new(modes::spiral::Spiral::new(cfg))),
    ("squiral", |cfg| Box::new(modes::squiral::Squiral::new(cfg))),
    ("starfish", |cfg| {
        Box::new(modes::starfish::Starfish::new(cfg))
    }),
    ("vermiculate", |cfg| {
        Box::new(modes::vermiculate::Vermiculate::new(cfg))
    }),
    ("vines", |cfg| Box::new(modes::vines::Vines::new(cfg))),
    ("whirlwindwarp", |cfg| {
        Box::new(modes::whirlwindwarp::WhirlwindWarp::new(cfg))
    }),
    ("wormhole", |cfg| {
        Box::new(modes::wormhole::Wormhole::new(cfg))
    }),
    ("worm", |cfg| Box::new(modes::worm::Worm::new(cfg))),
    ("xrayswarm", |cfg| {
        Box::new(modes::xrayswarm::XRaySwarm::new(cfg))
    }),
];

/// Registry of available animations.
pub struct AnimRegistry;

impl AnimRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, name: &str, config: &AnimConfig) -> Option<Box<dyn Animation>> {
        MODES
            .iter()
            .find(|(mode, _)| *mode == name)
            .map(|(_, constructor)| constructor(config))
    }

    pub fn available_modes(&self) -> Vec<String> {
        MODES.iter().map(|(name, _)| (*name).to_string()).collect()
    }
}

impl Default for AnimRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registered mode must survive create / tick / render / reset
    /// at a normal and a tiny size without panicking.
    #[test]
    fn all_modes_smoke() {
        let registry = AnimRegistry::new();
        for name in registry.available_modes() {
            for (w, h) in [(800u32, 600u32), (100, 80)] {
                let config = AnimConfig {
                    width: w,
                    height: h,
                    ..AnimConfig::default()
                };
                let mut anim = registry.create(&name, &config).unwrap();
                let mut buffer = vec![0u8; (w * h * 4) as usize];
                for _ in 0..50 {
                    anim.tick();
                    anim.render(&mut buffer, w, h);
                }
                anim.reset(&config);
                anim.tick();
                anim.render(&mut buffer, w, h);
            }
        }
    }
}
