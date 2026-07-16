use std::collections::HashMap;

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

    /// Reset the animation state.
    fn reset(&mut self, config: &AnimConfig);

    /// Returns true if the animation clears the screen each frame.
    fn clears_each_frame(&self) -> bool {
        false
    }

    /// The target delay between frames in microseconds.
    fn frame_delay_us(&self) -> u64;
}

type AnimConstructor = fn(&AnimConfig) -> Box<dyn Animation>;

/// Registry of available animations.
pub struct AnimRegistry {
    registry: HashMap<String, AnimConstructor>,
}

impl AnimRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            registry: HashMap::new(),
        };
        registry.register_tier1();
        registry
    }

    // One aligned line per mode reads as a table; keep it verbatim.
    #[rustfmt::skip]
    fn register_tier1(&mut self) {
        // We will register animation modes here as they are ported
        self.registry.insert("abstractile".to_string(), |cfg| Box::new(modes::abstractile::Abstractile::new(cfg)));
        self.registry.insert("anemone".to_string(), |cfg| Box::new(modes::anemone::Anemone::new(cfg)));
        self.registry.insert("apollonian".to_string(), |cfg| Box::new(modes::apollonian::Apollonian::new(cfg)));
        self.registry.insert("attraction".to_string(), |cfg| Box::new(modes::attraction::Attraction::new(cfg)));
        self.registry.insert("ball".to_string(), |cfg| Box::new(modes::ball::Ball::new(cfg)));
        self.registry.insert("binaryhorizon".to_string(), |cfg| Box::new(modes::binaryhorizon::BinaryHorizon::new(cfg)));
        self.registry.insert("binaryring".to_string(), |cfg| Box::new(modes::binaryring::BinaryRing::new(cfg)));
        self.registry.insert("blaster".to_string(), |cfg| Box::new(modes::blaster::Blaster::new(cfg)));
        self.registry.insert("blot".to_string(), |cfg| Box::new(modes::blot::Blot::new(cfg)));
        self.registry.insert("bouboule".to_string(), |cfg| Box::new(modes::bouboule::Bouboule::new(cfg)));
        self.registry.insert("bounce".to_string(), |cfg| Box::new(modes::bounce::Bounce::new(cfg)));
        self.registry.insert("boxfit".to_string(), |cfg| Box::new(modes::boxfit::BoxFit::new(cfg)));
        self.registry.insert("braid".to_string(), |cfg| Box::new(modes::braid::Braid::new(cfg)));
        self.registry.insert("bubble".to_string(), |cfg| Box::new(modes::bubble::Bubble::new(cfg)));
        self.registry.insert("ccurve".to_string(), |cfg| Box::new(modes::ccurve::CCurve::new(cfg)));
        self.registry.insert("coral".to_string(), |cfg| Box::new(modes::coral::Coral::new(cfg)));
        self.registry.insert("crystal".to_string(), |cfg| Box::new(modes::crystal::Crystal::new(cfg)));
        self.registry.insert("cynosure".to_string(), |cfg| Box::new(modes::cynosure::Cynosure::new(cfg)));
        self.registry.insert("discrete".to_string(), |cfg| Box::new(modes::discrete::Discrete::new(cfg)));
        self.registry.insert("drift".to_string(), |cfg| Box::new(modes::drift::Drift::new(cfg)));
        self.registry.insert("euler2d".to_string(), |cfg| Box::new(modes::euler2d::Euler2D::new(cfg)));
        self.registry.insert("fiberlamp".to_string(), |cfg| Box::new(modes::fiberlamp::Fiberlamp::new(cfg)));
        self.registry.insert("flame".to_string(), |cfg| { Box::new(modes::flame::Flame::new(cfg)) });
        self.registry.insert("flow".to_string(), |cfg| Box::new(modes::flow::Flow::new(cfg)));
        self.registry.insert("fluidballs".to_string(), |cfg| Box::new(modes::fluidballs::FluidBalls::new(cfg)));
        self.registry.insert("forest".to_string(), |cfg| Box::new(modes::forest::Forest::new(cfg)));
        self.registry.insert("galaxy".to_string(), |cfg| Box::new(modes::galaxy::Galaxy::new(cfg)));
        self.registry.insert("goop".to_string(), |cfg| Box::new(modes::goop::Goop::new(cfg)));
        self.registry.insert("grav".to_string(), |cfg| Box::new(modes::grav::Grav::new(cfg)));
        self.registry.insert("helix".to_string(), |cfg| { Box::new(modes::helix::Helix::new(cfg)) });
        self.registry.insert("hop".to_string(), |cfg| Box::new(modes::hop::Hop::new(cfg)));
        self.registry.insert("ico".to_string(), |cfg| Box::new(modes::ico::Ico::new(cfg)));
        self.registry.insert("ifs".to_string(), |cfg| Box::new(modes::ifs::Ifs::new(cfg)));
        self.registry.insert("intermomentary".to_string(), |cfg| Box::new(modes::intermomentary::Intermomentary::new(cfg)));
        self.registry.insert("julia".to_string(), |cfg| Box::new(modes::julia::Julia::new(cfg)));
        self.registry.insert("life3d".to_string(), |cfg| Box::new(modes::life3d::Life3D::new(cfg)));
        self.registry.insert("lightning".to_string(), |cfg| Box::new(modes::lightning::Lightning::new(cfg)));
        self.registry.insert("lisa".to_string(), |cfg| Box::new(modes::lisa::Lisa::new(cfg)));
        self.registry.insert("lissie".to_string(), |cfg| { Box::new(modes::lissie::Lissie::new(cfg)) });
        self.registry.insert("mandelbrot".to_string(), |cfg| Box::new(modes::mandelbrot::Mandelbrot::new(cfg)));
        self.registry.insert("matrix".to_string(), |cfg| Box::new(modes::matrix::Matrix::new(cfg)));
        self.registry.insert("maze".to_string(), |cfg| Box::new(modes::maze::Maze::new(cfg)));
        self.registry.insert("moire".to_string(), |cfg| Box::new(modes::moire::Moire::new(cfg)));
        self.registry.insert("molecule".to_string(), |cfg| Box::new(modes::molecule::Molecule::new(cfg)));
        self.registry.insert("mountain".to_string(), |cfg| Box::new(modes::mountain::Mountain::new(cfg)));
        self.registry.insert("noof".to_string(), |cfg| Box::new(modes::noof::Noof::new(cfg)));
        self.registry.insert("petri".to_string(), |cfg| Box::new(modes::petri::Petri::new(cfg)));
        self.registry.insert("piecewise".to_string(), |cfg| Box::new(modes::piecewise::Piecewise::new(cfg)));
        self.registry.insert("polyominoes".to_string(), |cfg| Box::new(modes::polyominoes::Polyominoes::new(cfg)));
        self.registry.insert("popsquares".to_string(), |cfg| Box::new(modes::popsquares::PopSquares::new(cfg)));
        self.registry.insert("pyro2".to_string(), |cfg| Box::new(modes::pyro2::Pyro2::new(cfg)));
        self.registry.insert("pyro".to_string(), |cfg| Box::new(modes::pyro::Pyro::new(cfg)));
        self.registry .insert("qix".to_string(), |cfg| Box::new(modes::qix::Qix::new(cfg)));
        self.registry.insert("rain".to_string(), |cfg| Box::new(modes::rain::Rain::new(cfg)));
        self.registry.insert("scooter".to_string(), |cfg| Box::new(modes::scooter::Scooter::new(cfg)));
        self.registry.insert("space".to_string(), |cfg| Box::new(modes::space::Space::new(cfg)));
        self.registry.insert("spiral".to_string(), |cfg| { Box::new(modes::spiral::Spiral::new(cfg)) });
        self.registry.insert("squiral".to_string(), |cfg| Box::new(modes::squiral::Squiral::new(cfg)));
        self.registry.insert("starfish".to_string(), |cfg| Box::new(modes::starfish::Starfish::new(cfg)));
        self.registry.insert("vermiculate".to_string(), |cfg| Box::new(modes::vermiculate::Vermiculate::new(cfg)));
        self.registry.insert("vines".to_string(), |cfg| Box::new(modes::vines::Vines::new(cfg)));
        self.registry.insert("whirlwindwarp".to_string(), |cfg| Box::new(modes::whirlwindwarp::WhirlwindWarp::new(cfg)));
        self.registry.insert("wormhole".to_string(), |cfg| Box::new(modes::wormhole::Wormhole::new(cfg)));
        self.registry.insert("worm".to_string(), |cfg| Box::new(modes::worm::Worm::new(cfg)));
        self.registry.insert("xrayswarm".to_string(), |cfg| Box::new(modes::xrayswarm::XRaySwarm::new(cfg)));
    }

    pub fn create(&self, name: &str, config: &AnimConfig) -> Option<Box<dyn Animation>> {
        self.registry
            .get(name)
            .map(|constructor| constructor(config))
    }

    pub fn available_modes(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
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
