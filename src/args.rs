// CLI argument definitions.
//
// Self-contained (depends only on `clap`) so `build.rs` can `include!` it to
// generate shell completions from the same `Command` the binary parses with —
// one source of truth. Kept as `//` (not `//!`) comments so `include!` into
// build.rs stays valid.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "baijia-suo", version, about = "A secure Wayland screen locker")]
pub(crate) struct Args {
    /// Path to config file
    #[arg(short = 'C', long)]
    pub config: Option<String>,

    /// Background color in RRGGBB or RRGGBBAA format
    #[arg(short, long)]
    pub color: Option<String>,

    /// Enable debug logging
    #[arg(long)]
    pub debug: bool,

    /// Detach from terminal after locking
    #[arg(short, long)]
    pub daemonize: bool,

    /// File descriptor to send readiness notification
    #[arg(short = 'R', long)]
    pub ready_fd: Option<i32>,

    /// Animation mode(s). Repeat or comma-separate to cycle through several
    /// (e.g. `-A spiral,flame`); "random" or "all" uses every mode. Omit for
    /// a solid background.
    #[arg(short = 'A', long, value_delimiter = ',')]
    pub animation: Vec<String>,

    /// Seconds each animation plays before cycling to the next; only applies
    /// with multiple modes. Default 60.
    #[arg(long, value_name = "SECONDS")]
    pub cycle: Option<u64>,

    /// Cap animation frame rates (fps). Unset = uncapped, each mode's own
    /// clock (some run at 100+ fps).
    #[arg(long, value_name = "FPS")]
    pub max_fps: Option<u32>,

    /// Suspend animations (solid background) when the battery is
    /// discharging at or below this percent. Unset = never.
    #[arg(long, value_name = "PERCENT")]
    pub low_battery_percent: Option<u32>,

    /// List available animation modes and exit
    #[arg(long)]
    pub list_animations: bool,

    /// Typing indicator style: fade (default), pin-tumbler, comet, breath,
    /// dots, scope, or ripple. All but pin-tumbler avoid revealing the
    /// password length on screen.
    #[arg(long)]
    pub indicator_mode: Option<String>,

    /// Indicator opacity, 0.0-1.0 (default 1.0 = fully opaque)
    #[arg(long)]
    pub indicator_opacity: Option<String>,

    /// Indicator disk color for idle/typing states, RRGGBB
    #[arg(long)]
    pub indicator_color: Option<String>,

    /// Authentication backend to use
    #[arg(long, default_value = "pam")]
    pub auth_backend: Option<String>,

    /// Username for authentication
    #[arg(long)]
    pub username: Option<String>,

    /// Test authentication only (does not lock screen)
    #[arg(long)]
    pub auth_test: bool,
}
