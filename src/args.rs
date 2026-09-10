#![forbid(unsafe_code)]

// CLI argument definitions.

use std::ffi::OsString;

pub(crate) struct Args {
    /// Path to config file
    pub config: Option<String>,
    /// Background color in RRGGBB or RRGGBBAA format
    pub color: Option<String>,
    /// Enable debug logging
    pub debug: bool,
    /// Detach from terminal after locking
    pub daemonize: bool,
    /// Stay in the foreground even if the config file sets daemonize.
    /// Lets a supervisor (systemd-inhibit, systemd-run --wait, shell `wait`)
    /// hold the locker's lifetime per-invocation.
    pub no_daemonize: bool,
    /// File descriptor to send readiness notification
    pub ready_fd: Option<i32>,
    /// Animation mode(s). Repeat or comma-separate to cycle through several
    /// (e.g. `-A spiral,flame`); "random" or "all" uses every mode. Omit for
    /// a solid background.
    pub animation: Vec<String>,
    /// Seconds each animation plays before cycling to the next; only applies
    /// with multiple modes. Default 60.
    pub cycle: Option<u64>,
    /// Cap animation frame rates (fps). Unset = uncapped, each mode's own
    /// clock (some run at 100+ fps).
    pub max_fps: Option<u32>,
    /// Suspend animations (solid background) when the battery is
    /// discharging at or below this percent. Unset = never.
    pub low_battery_percent: Option<u32>,
    /// Log per-frame timing (animation advance vs present cost, 1/s
    /// aggregate) for performance work.
    pub debug_timing: bool,
    /// List available animation modes and exit
    pub list_animations: bool,
    /// Typing indicator style: fade (default), pin-tumbler, comet, breath,
    /// dots, scope, or ripple. All but pin-tumbler avoid revealing the
    /// password length on screen.
    pub indicator_mode: Option<String>,
    /// Indicator opacity, 0.0-1.0 (default 1.0 = fully opaque)
    pub indicator_opacity: Option<String>,
    /// Indicator disk color for idle/typing states, RRGGBB
    pub indicator_color: Option<String>,
    /// Authentication backend to use
    pub auth_backend: Option<String>,
    /// Username for authentication
    pub username: Option<String>,
    /// Test authentication only (does not lock screen)
    pub auth_test: bool,
}

struct Flag {
    short: Option<char>,
    long: &'static str,
    value_name: Option<&'static str>,
    doc: &'static str,
}

const FLAGS: &[Flag] = &[
    Flag { short: Some('C'), long: "config", value_name: Some("CONFIG"), doc: "Path to config file" }, Flag { short: Some('c'), long: "color", value_name: Some("COLOR"), doc: "Background color in RRGGBB or RRGGBBAA format" }, Flag { short: None, long: "debug", value_name: None, doc: "Enable debug logging" }, Flag { short: Some('d'), long: "daemonize", value_name: None, doc: "Detach from terminal after locking" }, Flag { short: None, long: "no-daemonize", value_name: None, doc: "Stay in the foreground even if the config file sets daemonize" }, Flag { short: Some('R'), long: "ready-fd", value_name: Some("READY_FD"), doc: "File descriptor to send readiness notification" }, Flag { short: Some('A'), long: "animation", value_name: Some("ANIMATION"), doc: "Animation mode(s). Repeat or comma-separate to cycle through several" }, Flag { short: None, long: "cycle", value_name: Some("SECONDS"), doc: "Seconds each animation plays before cycling to the next; only applies with multiple modes. Default 60." }, Flag { short: None, long: "max-fps", value_name: Some("FPS"), doc: "Cap animation frame rates (fps). Unset = uncapped, each mode's own clock (some run at 100+ fps)." }, Flag { short: None, long: "low-battery-percent", value_name: Some("PERCENT"), doc: "Suspend animations (solid background) when the battery is discharging at or below this percent. Unset = never." }, Flag { short: None, long: "debug-timing", value_name: None, doc: "Log per-frame timing (animation advance vs present cost, 1/s aggregate) for performance work." }, Flag { short: None, long: "list-animations", value_name: None, doc: "List available animation modes and exit" }, Flag { short: None, long: "indicator-mode", value_name: Some("INDICATOR_MODE"), doc: "Typing indicator style: fade (default), pin-tumbler, comet, breath, dots, scope, or ripple." }, Flag { short: None, long: "indicator-opacity", value_name: Some("INDICATOR_OPACITY"), doc: "Indicator opacity, 0.0-1.0 (default 1.0 = fully opaque)" }, Flag { short: None, long: "indicator-color", value_name: Some("INDICATOR_COLOR"), doc: "Indicator disk color for idle/typing states, RRGGBB" }, Flag { short: None, long: "auth-backend", value_name: Some("AUTH_BACKEND"), doc: "Authentication backend to use (default: pam)" }, Flag { short: None, long: "username", value_name: Some("USERNAME"), doc: "Username for authentication" }, Flag { short: None, long: "auth-test", value_name: None, doc: "Test authentication only (does not lock screen)" }, Flag { short: Some('h'), long: "help", value_name: None, doc: "Print help" }, Flag { short: Some('V'), long: "version", value_name: None, doc: "Print version" },
];

fn help() -> String {
    use std::fmt::Write;
    let mut output =
        String::from("A secure Wayland screen locker\n\nUsage: baijia-suo [OPTIONS]\n\nOptions:\n");
    for flag in FLAGS {
        let short = flag.short.map_or(String::new(), |c| format!("-{c}, "));
        let value = flag
            .value_name
            .map_or(String::new(), |name| format!(" <{name}>"));
        writeln!(output, "  {short}--{}{value:<24} {}", flag.long, flag.doc).unwrap();
    }
    output
}

impl Args {
    pub fn parse() -> Args {
        Self::parse_or_exit(std::env::args_os())
    }
    #[allow(dead_code)]
    pub fn parse_from<I, T>(iter: I) -> Args
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Self::parse_or_exit(iter)
    }
    fn parse_or_exit<I, T>(iter: I) -> Args
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        match Self::try_parse_from(iter) {
            Ok(args) => args,
            Err(error) if error == "help" => {
                print!("{}", help());
                std::process::exit(0);
            }
            Err(error) if error == "version" => {
                println!("baijia-suo {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("error: {error}\n\nUsage: baijia-suo [OPTIONS]");
                std::process::exit(2);
            }
        }
    }
    pub fn try_parse_from<I, T>(iter: I) -> Result<Args, String>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let values: Vec<String> = iter
            .into_iter()
            .map(|v| {
                v.into()
                    .into_string()
                    .map_err(|v| format!("argument is not valid UTF-8: {v:?}"))
            })
            .collect::<Result<_, _>>()?;
        let mut args = Self::defaults();
        let mut index = 1;
        let mut options = true;
        while index < values.len() {
            let arg = &values[index];
            if options && arg == "--" {
                options = false;
                index += 1;
                continue;
            }
            if options && (arg == "-h" || arg == "--help") {
                return Err("help".into());
            }
            if options && (arg == "-V" || arg == "--version") {
                return Err("version".into());
            }
            if options && arg.starts_with("--") {
                let (name, inline) = arg[2..]
                    .split_once('=')
                    .map_or((&arg[2..], None), |(n, v)| (n, Some(v)));
                match name {
                    "debug" => args.debug = flag(inline, name)?,
                    "daemonize" => {
                        args.daemonize = flag(inline, name)?;
                        args.no_daemonize = false;
                    }
                    "no-daemonize" => {
                        args.no_daemonize = flag(inline, name)?;
                        args.daemonize = false;
                    }
                    "debug-timing" => args.debug_timing = flag(inline, name)?,
                    "list-animations" => args.list_animations = flag(inline, name)?,
                    "auth-test" => args.auth_test = flag(inline, name)?,
                    "config" => args.config = Some(value(&values, &mut index, inline, name)?),
                    "color" => args.color = Some(value(&values, &mut index, inline, name)?),
                    "ready-fd" => {
                        args.ready_fd =
                            Some(number(value(&values, &mut index, inline, name)?, name)?)
                    }
                    "animation" => args.animation.extend(
                        value(&values, &mut index, inline, name)?
                            .split(',')
                            .map(str::to_owned),
                    ),
                    "cycle" => {
                        args.cycle = Some(number(value(&values, &mut index, inline, name)?, name)?)
                    }
                    "max-fps" => {
                        args.max_fps =
                            Some(number(value(&values, &mut index, inline, name)?, name)?)
                    }
                    "low-battery-percent" => {
                        args.low_battery_percent =
                            Some(number(value(&values, &mut index, inline, name)?, name)?)
                    }
                    "indicator-mode" => {
                        args.indicator_mode = Some(value(&values, &mut index, inline, name)?)
                    }
                    "indicator-opacity" => {
                        args.indicator_opacity = Some(value(&values, &mut index, inline, name)?)
                    }
                    "indicator-color" => {
                        args.indicator_color = Some(value(&values, &mut index, inline, name)?)
                    }
                    "auth-backend" => {
                        args.auth_backend = Some(value(&values, &mut index, inline, name)?)
                    }
                    "username" => args.username = Some(value(&values, &mut index, inline, name)?),
                    _ => return Err(format!("unexpected argument '--{name}' found")),
                }
            } else if options && arg.starts_with('-') && arg != "-" {
                let mut chars = arg[1..].chars();
                let short = chars.next().unwrap();
                let rest = chars.as_str();
                let inline =
                    rest.strip_prefix('=')
                        .or(if rest.is_empty() { None } else { Some(rest) });
                match short {
                    'C' => args.config = Some(value(&values, &mut index, inline, "config")?),
                    'c' => args.color = Some(value(&values, &mut index, inline, "color")?),
                    'd' => {
                        args.daemonize = flag(inline, "daemonize")?;
                        args.no_daemonize = false;
                    }
                    'R' => {
                        args.ready_fd = Some(number(
                            value(&values, &mut index, inline, "ready-fd")?,
                            "ready-fd",
                        )?)
                    }
                    'A' => args.animation.extend(
                        value(&values, &mut index, inline, "animation")?
                            .split(',')
                            .map(str::to_owned),
                    ),
                    _ => return Err(format!("unexpected argument '-{short}' found")),
                }
            } else {
                return Err(format!("unexpected argument '{arg}' found"));
            }
            index += 1;
        }
        Ok(args)
    }
    fn defaults() -> Args {
        Args {
            config: None,
            color: None,
            debug: false,
            daemonize: false,
            no_daemonize: false,
            ready_fd: None,
            animation: Vec::new(),
            cycle: None,
            max_fps: None,
            low_battery_percent: None,
            debug_timing: false,
            list_animations: false,
            indicator_mode: None,
            indicator_opacity: None,
            indicator_color: None,
            auth_backend: Some("pam".into()),
            username: None,
            auth_test: false,
        }
    }
}
fn flag(inline: Option<&str>, name: &str) -> Result<bool, String> {
    if inline.is_some() {
        Err(format!("argument '--{name}' does not take a value"))
    } else {
        Ok(true)
    }
}
fn value(
    values: &[String],
    index: &mut usize,
    inline: Option<&str>,
    name: &str,
) -> Result<String, String> {
    match inline {
        Some(value) if !value.is_empty() => Ok(value.into()),
        Some(_) => Err(format!(
            "a value is required for '--{name} <VALUE>' but none was supplied"
        )),
        None => {
            *index += 1;
            match values.get(*index) {
                Some(value) if !value.starts_with('-') => Ok(value.clone()),
                _ => Err(format!(
                    "a value is required for '--{name} <VALUE>' but none was supplied"
                )),
            }
        }
    }
}
fn number<T: std::str::FromStr>(value: String, name: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value '{value}' for '--{name} <VALUE>': invalid number"))
}
#[cfg(test)]
mod tests {
    use super::{Args, FLAGS};
    #[test]
    fn every_documented_flag_is_accepted() {
        for flag in FLAGS {
            let mut argv = vec!["x".to_owned(), format!("--{}", flag.long)];
            if flag.value_name.is_some() {
                argv.push("1".to_owned());
            }
            let result = Args::try_parse_from(argv);
            assert!(
                result.is_ok()
                    || matches!(
                        result.as_ref().map_err(String::as_str),
                        Err("help" | "version")
                    ),
                "--{} has no parser arm",
                flag.long
            );
        }
    }
    #[test]
    fn animation_delimiter_and_repeats() {
        assert_eq!(
            Args::try_parse_from(["x", "-A", "a,b"]).unwrap().animation,
            ["a", "b"]
        );
        assert_eq!(
            Args::try_parse_from(["x", "-A", "a", "-A", "b"])
                .unwrap()
                .animation,
            ["a", "b"]
        );
    }
    #[test]
    fn daemonize_last_wins() {
        let a = Args::try_parse_from(["x", "-d", "--no-daemonize"]).unwrap();
        assert!(!a.daemonize && a.no_daemonize);
        let a = Args::try_parse_from(["x", "--no-daemonize", "-d"]).unwrap();
        assert!(a.daemonize && !a.no_daemonize);
    }
    #[test]
    fn auth_backend_defaults_to_pam() {
        assert_eq!(
            Args::try_parse_from(["x"]).unwrap().auth_backend.as_deref(),
            Some("pam")
        );
    }
    #[test]
    fn rejects_invalid_number_and_unknown_flag() {
        assert!(Args::try_parse_from(["x", "--max-fps=abc"]).is_err());
        assert!(Args::try_parse_from(["x", "--bogus"]).is_err());
    }
    #[test]
    fn double_dash_stops_options() {
        assert!(Args::try_parse_from(["x", "--", "--bogus"])
            .err()
            .unwrap()
            .contains("unexpected argument '--bogus'"));
    }
    #[test]
    fn username_and_short_equals_forms_parse() {
        assert_eq!(
            Args::try_parse_from(["x", "--username", "alice"])
                .unwrap()
                .username
                .as_deref(),
            Some("alice")
        );
        assert_eq!(
            Args::try_parse_from(["x", "--username=alice"])
                .unwrap()
                .username
                .as_deref(),
            Some("alice")
        );
        assert_eq!(
            Args::try_parse_from(["x", "-R=3"]).unwrap().ready_fd,
            Some(3)
        );
    }
    #[test]
    fn invalid_short_and_option_value_are_errors() {
        assert!(Args::try_parse_from(["x", "-é"]).is_err());
        assert!(Args::try_parse_from(["x", "--config", "--help"]).is_err());
    }
}
