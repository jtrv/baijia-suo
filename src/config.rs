//! Configuration for baijia-suo.
//!
//! Configuration comes from two places, merged in [`Config::from_args`]:
//!
//! 1. A TOML config file, loaded from `-C <path>` if given, else
//!    `$XDG_CONFIG_HOME/baijia-suo/config.toml` (default
//!    `~/.config/baijia-suo/config.toml`), else `/etc/baijia-suo/config.toml`.
//! 2. CLI flags, which override file values.
//!
//! Schema (all keys optional):
//!
//! ```toml
//! color = "1a2b3c"          # background, RRGGBB(AA)
//! animation = ["ripple"]    # one mode, a list to cycle, or "random"
//! cycle = 60                # seconds per mode when cycling
//! daemonize = false
//!
//! [indicator]
//! mode = "fade"
//! opacity = 1.0
//! color = "3c3c3c"
//! ```
//!
//! Values feed the same validation as the CLI flags of the same name.
//! Unknown keys are rejected by name.

use crate::animation::AnimConfig;
use crate::args::Args;
use std::path::PathBuf;
use std::time::Duration;

/// Color represented as RGBA components (0.0 - 1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Default for Color {
    /// Opaque black.
    fn default() -> Self {
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }
}

impl Color {
    /// Parse a color from hex string (RRGGBB or RRGGBBAA format).
    pub fn from_hex(s: &str) -> Result<Self, String> {
        let s = s.trim_start_matches('#');
        let hex = u32::from_str_radix(s, 16).map_err(|_| format!("invalid hex color: {}", s))?;

        match s.len() {
            6 => Ok(Color {
                r: ((hex >> 16) & 0xFF) as f64 / 255.0,
                g: ((hex >> 8) & 0xFF) as f64 / 255.0,
                b: (hex & 0xFF) as f64 / 255.0,
                a: 1.0,
            }),
            8 => Ok(Color {
                r: ((hex >> 24) & 0xFF) as f64 / 255.0,
                g: ((hex >> 16) & 0xFF) as f64 / 255.0,
                b: ((hex >> 8) & 0xFF) as f64 / 255.0,
                a: (hex & 0xFF) as f64 / 255.0,
            }),
            _ => Err(format!("invalid color format: {}", s)),
        }
    }
}

/// Animation configuration.
#[derive(Debug, Clone)]
pub struct AnimationConfig {
    /// Animation modes to play. Empty means a solid background; more than one
    /// is cycled through (shuffled) with `cycle` airtime each.
    pub modes: Vec<String>,
    /// How long each mode plays before cycling to the next.
    pub cycle: Duration,
    /// Animation parameters.
    pub params: AnimConfig,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig {
            modes: Vec::new(),
            cycle: Duration::from_secs(60),
            // count/size/delay_us of 0 mean "use the mode's own default"
            // (see e.g. modes::worm::new), matching xscreensaver behavior.
            // width/height are placeholders; the player resizes to the
            // real output dimensions on the first frame.
            params: AnimConfig {
                width: 1920,
                height: 1080,
                count: 0,
                cycles: 0,
                size: 0,
                ncolors: 64,
                delay_us: 0,
                max_fps: 0,
            },
        }
    }
}

/// Indicator configuration.
#[derive(Debug, Clone)]
pub struct IndicatorConfig {
    /// Position X (0.0 = left, 1.0 = right, 0.5 = center).
    pub x_position: f64,
    /// Position Y (0.0 = top, 1.0 = bottom, 0.5 = center).
    pub y_position: f64,
    /// Radius of the indicator ring.
    pub radius: f64,
    /// Which typing-indicator animation to draw.
    pub mode: crate::render::indicator::IndicatorMode,
    /// Overall opacity of the indicator chrome (the inner disk), 0.0-1.0.
    pub opacity: f64,
    /// Override for the idle/typing disk color. Semantic state colors
    /// (success green, invalid red, caps amber) are unaffected.
    pub color: Option<Color>,
}

impl Default for IndicatorConfig {
    fn default() -> Self {
        IndicatorConfig {
            x_position: 0.5,
            y_position: 0.5,
            radius: 100.0,
            mode: crate::render::indicator::IndicatorMode::default(),
            opacity: 1.0,
            color: None,
        }
    }
}

/// Main configuration for baijia-suo.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Suspend animations (solid background) when the battery is
    /// discharging at/below this percent. 0 = disabled.
    pub low_battery_percent: u32,
    /// Background color.
    pub background_color: Color,
    /// Animation configuration.
    pub animation: AnimationConfig,
    /// Indicator configuration.
    pub indicator: IndicatorConfig,
    /// Daemonize mode.
    pub daemonize: bool,
    /// Log per-frame timing aggregates (CLI-only debug flag).
    pub debug_timing: bool,
    /// Readiness file descriptor.
    pub ready_fd: Option<i32>,
}

/// Options read from the TOML config. `deny_unknown_fields` rejects any key
/// (top-level or in `[indicator]`) the code doesn't handle, keeping the
/// accepted surface small and auditable. Values are validated the same way as
/// the matching CLI flag in [`Config::build`].
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    color: Option<String>,
    /// A single mode (`animation = "spiral"`) or a list
    /// (`animation = ["spiral", "flame"]`).
    #[serde(default, deserialize_with = "de_string_or_seq")]
    animation: Option<Vec<String>>,
    /// Seconds each mode plays before cycling (with multiple modes).
    cycle: Option<u64>,
    /// Frame-rate cap for animations; unset = uncapped (each mode's own clock).
    max_fps: Option<u32>,
    /// Suspend animations below this battery percent; unset = never.
    low_battery_percent: Option<u32>,
    daemonize: Option<bool>,
    #[serde(default)]
    indicator: IndicatorFile,
}

/// Deserialize the `animation` key as either a string or a list of strings.
/// Only invoked when the key is present (missing → `None` via `default`).
fn de_string_or_seq<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a mode name or a list of mode names")
        }
        fn visit_str<E>(self, s: &str) -> Result<Vec<String>, E> {
            Ok(vec![s.to_string()])
        }
        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<String>, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                out.push(s);
            }
            Ok(out)
        }
    }
    deserializer.deserialize_any(V).map(Some)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IndicatorFile {
    mode: Option<String>,
    /// A TOML float, e.g. `opacity = 0.85`; range-checked in `build`.
    opacity: Option<f64>,
    color: Option<String>,
}

impl FileConfig {
    fn parse(text: &str) -> Result<Self, String> {
        basic_toml::from_str(text).map_err(|e| format!("invalid config: {e}"))
    }
}

/// First existing default config file:
/// `$XDG_CONFIG_HOME/baijia-suo/config.toml`, else
/// `~/.config/baijia-suo/config.toml`, else `/etc/baijia-suo/config.toml`.
fn default_config_path() -> Option<PathBuf> {
    let user_base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(base) = user_base {
        let path = base.join("baijia-suo/config.toml");
        if path.exists() {
            return Some(path);
        }
    }
    let etc = PathBuf::from("/etc/baijia-suo/config.toml");
    etc.exists().then_some(etc)
}

impl Config {
    /// Build the runtime configuration: load the config file (explicit `-C`
    /// path, else the first existing default location), then apply CLI
    /// flags on top. CLI flags override file values.
    pub(crate) fn from_args(args: &Args) -> Result<Self, String> {
        let path = args
            .config
            .clone()
            .map(PathBuf::from)
            .or_else(default_config_path);
        let file = match path {
            Some(path) => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read config file {}: {}", path.display(), e))?;
                FileConfig::parse(&text).map_err(|e| format!("{}: {}", path.display(), e))?
            }
            None => FileConfig::default(),
        };
        Self::build(args, file)
    }

    /// Merge CLI arguments over file options and validate the result.
    fn build(args: &Args, file: FileConfig) -> Result<Self, String> {
        fn pick(cli: &Option<String>, file: Option<String>) -> Option<String> {
            cli.clone().or(file)
        }

        let mut indicator = IndicatorConfig::default();
        if let Some(m) = pick(&args.indicator_mode, file.indicator.mode) {
            indicator.mode = m.parse()?;
        }
        // Opacity comes as a string from the CLI, an f64 from the file; the CLI
        // wins. Validate the resolved value against the same range either way.
        let opacity = match &args.indicator_opacity {
            Some(s) => Some(s.parse::<f64>().map_err(|_| {
                format!("invalid indicator opacity: '{s}' (expected a number in 0.0-1.0)")
            })?),
            None => file.indicator.opacity,
        };
        if let Some(v) = opacity {
            if !(0.0..=1.0).contains(&v) {
                return Err(format!(
                    "indicator opacity out of range: {v} (expected 0.0-1.0)"
                ));
            }
            indicator.opacity = v;
        }
        if let Some(c) = pick(&args.indicator_color, file.indicator.color) {
            indicator.color = Some(Color::from_hex(&c)?);
        }

        Ok(Config {
            // 0 = disabled.
            low_battery_percent: args
                .low_battery_percent
                .or(file.low_battery_percent)
                .unwrap_or(0),
            daemonize: args.daemonize || file.daemonize.unwrap_or(false),
            debug_timing: args.debug_timing,
            ready_fd: args.ready_fd,
            background_color: pick(&args.color, file.color)
                .map(|c| Color::from_hex(&c))
                .transpose()?
                .unwrap_or_default(),
            animation: AnimationConfig {
                // CLI list wins if given, else the file's, else none.
                modes: expand_modes(if args.animation.is_empty() {
                    file.animation.unwrap_or_default()
                } else {
                    args.animation.clone()
                }),
                // At least 1 s, so cycling can't switch every frame.
                cycle: Duration::from_secs(args.cycle.or(file.cycle).unwrap_or(60).max(1)),
                params: AnimConfig {
                    // 0 = uncapped, the original per-mode clocks.
                    max_fps: args.max_fps.or(file.max_fps).unwrap_or(0),
                    ..AnimationConfig::default().params
                },
            },
            indicator,
        })
    }
}

/// Expand the `random`/`all` keyword (used alone) to every registered mode.
/// Any other list is returned as-is; `Playlist` validates and shuffles it.
fn expand_modes(list: Vec<String>) -> Vec<String> {
    if list.len() == 1 && matches!(list[0].to_ascii_lowercase().as_str(), "random" | "all") {
        let mut all = crate::animation::AnimRegistry::new().available_modes();
        all.sort();
        all
    } else {
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args(argv: &[&str]) -> Args {
        Args::parse_from(std::iter::once("baijia-suo").chain(argv.iter().copied()))
    }

    fn from_cli(argv: &[&str]) -> Result<Config, String> {
        Config::build(&args(argv), FileConfig::default())
    }

    #[test]
    fn color_from_hex_rgb() {
        let c = Color::from_hex("#FF0000").unwrap();
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
        assert_eq!(c.b, 0.0);
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn color_from_hex_rgba() {
        let c = Color::from_hex("#FF000080").unwrap();
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
        assert_eq!(c.b, 0.0);
        assert!((c.a - 128.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn color_from_hex_shorthand() {
        let c = Color::from_hex("FFFFFF").unwrap();
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 1.0);
        assert_eq!(c.b, 1.0);
    }

    #[test]
    fn color_invalid_hex() {
        assert!(Color::from_hex("notacolor").is_err());
        assert!(Color::from_hex("#GGG").is_err());
    }

    #[test]
    fn background_defaults_to_opaque_black() {
        let cfg = from_cli(&[]).unwrap();
        assert_eq!(
            cfg.background_color,
            Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0
            }
        );
        assert!(!cfg.daemonize);
    }

    #[test]
    fn indicator_opacity_validated() {
        // File path: a TOML float, range-checked in `build`.
        let file = |toml: &str| Config::build(&args(&[]), FileConfig::parse(toml).unwrap());
        assert_eq!(
            file("[indicator]\nopacity = 0.8")
                .unwrap()
                .indicator
                .opacity,
            0.8
        );
        assert!(file("[indicator]\nopacity = 1.5").is_err());
        assert!(file("[indicator]\nopacity = -0.1").is_err());
        assert!(file("[indicator]\nopacity = nan").is_err());

        // CLI path: a string, parsed then range-checked (shares the range).
        let cli = |s: &str| from_cli(&["--indicator-opacity", s]);
        assert_eq!(cli("1.0").unwrap().indicator.opacity, 1.0);
        assert!(cli("1.5").is_err());
        assert!(cli("garbage").is_err());

        // default is fully opaque
        let cfg = from_cli(&[]).unwrap();
        assert_eq!(cfg.indicator.opacity, 1.0);
        assert_eq!(cfg.indicator.color, None);
    }

    #[test]
    fn indicator_color_from_args() {
        let cfg = from_cli(&["--indicator-color", "FF8800"]).unwrap();
        let c = cfg.indicator.color.unwrap();
        assert_eq!(c.r, 1.0);
        assert!((c.g - 136.0 / 255.0).abs() < 0.01);
        assert_eq!(c.b, 0.0);
        assert!(from_cli(&["--indicator-color", "nothex"]).is_err());
    }

    #[test]
    fn anim_params_default_to_mode_defaults() {
        // 0 = "use the mode's own default"; the same params must come out
        // of both Default and the CLI path (F8: one source of truth).
        let d = AnimationConfig::default().params;
        let cfg = from_cli(&["--animation", "worm"]).unwrap();
        assert_eq!(cfg.animation.modes, vec!["worm".to_string()]);
        assert_eq!(cfg.animation.params.count, 0);
        assert_eq!(cfg.animation.params.size, 0);
        assert_eq!(cfg.animation.params.count, d.count);
        assert_eq!(cfg.animation.params.size, d.size);
    }

    #[test]
    fn file_parse_happy_path() {
        let f = FileConfig::parse(
            "# a comment\n\
             color = \"1a2b3c\"\n\
             animation = \"spiral\"\n\
             daemonize = true\n\
             \n\
             [indicator]\n\
             mode = \"dots\"\n\
             opacity = 0.5\n\
             color = \"ff8800\"\n",
        )
        .unwrap();
        assert_eq!(f.color.as_deref(), Some("1a2b3c"));
        assert_eq!(f.animation, Some(vec!["spiral".to_string()]));
        assert_eq!(f.indicator.mode.as_deref(), Some("dots"));
        assert_eq!(f.indicator.opacity, Some(0.5));
        assert_eq!(f.indicator.color.as_deref(), Some("ff8800"));
        assert_eq!(f.daemonize, Some(true));
    }

    #[test]
    fn file_parse_comments_and_empty() {
        let f = FileConfig::parse("# just comments\n\n").unwrap();
        assert!(f.color.is_none());
        assert_eq!(f.daemonize, None);
    }

    #[test]
    fn file_parse_unknown_key_named() {
        let err =
            FileConfig::parse("color = \"000000\"\nbackground-image = \"/x.png\"\n").unwrap_err();
        assert!(
            err.contains("background-image"),
            "error should name the key: {err}"
        );
        let nested = FileConfig::parse("[indicator]\nsize = 5\n").unwrap_err();
        assert!(
            nested.contains("size"),
            "should name the nested key: {nested}"
        );
    }

    #[test]
    fn file_parse_wrong_type_rejected() {
        assert!(FileConfig::parse("color = 5\n").is_err());
        assert!(FileConfig::parse("daemonize = \"yes\"\n").is_err());
        assert!(FileConfig::parse("[indicator]\nopacity = \"high\"\n").is_err());
        assert!(FileConfig::parse("not valid toml =").is_err());
    }

    #[test]
    fn cli_overrides_file() {
        let file =
            FileConfig::parse("color = \"ff0000\"\nanimation = \"spiral\"\ndaemonize = true\n")
                .unwrap();
        let cfg = Config::build(&args(&["--color", "00ff00"]), file).unwrap();
        // CLI wins where given...
        assert_eq!(cfg.background_color, Color::from_hex("00ff00").unwrap());
        // ...file value survives where the CLI is silent.
        assert_eq!(cfg.animation.modes, vec!["spiral".to_string()]);
        assert!(cfg.daemonize);
    }

    #[test]
    fn file_animation_accepts_string_or_list() {
        let one = FileConfig::parse("animation = \"spiral\"\n").unwrap();
        assert_eq!(one.animation, Some(vec!["spiral".to_string()]));
        let many = FileConfig::parse("animation = [\"spiral\", \"flame\"]\n").unwrap();
        assert_eq!(
            many.animation,
            Some(vec!["spiral".to_string(), "flame".to_string()])
        );
    }

    #[test]
    fn cli_animation_list_and_cycle() {
        let cfg = from_cli(&["-A", "spiral,flame,worm", "--cycle", "30"]).unwrap();
        assert_eq!(cfg.animation.modes, vec!["spiral", "flame", "worm"]);
        assert_eq!(cfg.animation.cycle, Duration::from_secs(30));
        // Repeated flags accumulate too.
        let cfg = from_cli(&["-A", "spiral", "-A", "flame"]).unwrap();
        assert_eq!(cfg.animation.modes, vec!["spiral", "flame"]);
    }

    #[test]
    fn cli_overrides_file_animation_list() {
        let file = FileConfig::parse("animation = [\"spiral\", \"flame\"]\n").unwrap();
        let cfg = Config::build(&args(&["-A", "worm"]), file).unwrap();
        assert_eq!(cfg.animation.modes, vec!["worm".to_string()]);
    }

    #[test]
    fn random_keyword_expands_to_all_modes() {
        let all = crate::animation::AnimRegistry::new().available_modes();
        let cfg = from_cli(&["-A", "random"]).unwrap();
        assert_eq!(cfg.animation.modes.len(), all.len());
        assert!(cfg.animation.modes.len() > 1);
        // "all" is an alias.
        let cfg = from_cli(&["-A", "all"]).unwrap();
        assert_eq!(cfg.animation.modes.len(), all.len());
    }

    #[test]
    fn cycle_defaults_and_floors_at_one_second() {
        assert_eq!(
            from_cli(&["-A", "spiral"]).unwrap().animation.cycle,
            Duration::from_secs(60)
        );
        assert_eq!(
            from_cli(&["-A", "spiral", "--cycle", "0"])
                .unwrap()
                .animation
                .cycle,
            Duration::from_secs(1)
        );
    }

    #[test]
    fn file_values_are_validated() {
        let file = FileConfig::parse("[indicator]\nopacity = 7.0\n").unwrap();
        assert!(Config::build(&args(&[]), file).is_err());
    }

    #[test]
    fn from_args_reads_explicit_config_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        writeln!(
            std::fs::File::create(&path).unwrap(),
            "# test config\ncolor = \"112233\"\n[indicator]\nmode = \"fade\""
        )
        .unwrap();

        let cfg = Config::from_args(&args(&["-C", path.to_str().unwrap()])).unwrap();
        assert_eq!(cfg.background_color, Color::from_hex("112233").unwrap());

        // Explicit -C pointing at a missing file is an error, not a silent no-op.
        let missing = dir.path().join("nope");
        assert!(Config::from_args(&args(&["-C", missing.to_str().unwrap()])).is_err());
    }
}
