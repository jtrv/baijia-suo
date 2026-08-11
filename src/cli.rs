//! CLI entry point: argument parsing, process hardening, and the
//! `run()` consumed by the `baijia-suo` binary shim.

use crate::app::App;
use crate::args::Args;
use crate::config::Config;
use clap::Parser;
use std::io::Write;

/// Minimal stderr logger: writes `[LEVEL] message` lines. Level is fixed at
/// init (Warn, or Debug with --debug).
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: StderrLogger = StderrLogger;

/// Process-wide hardening, done before anything touches a password.
fn harden_process() {
    unsafe {
        // No core dumps and no non-root ptrace attach: a crash while a
        // password copy is live must not land in a coredump on disk.
        // Inherited by the forked auth child.
        if libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0 {
            log::warn!(
                "PR_SET_DUMPABLE failed: {}",
                std::io::Error::last_os_error()
            );
        }
        // A dead auth child would otherwise turn the next pipe write into
        // a fatal SIGPIPE; with SIG_IGN the write returns EPIPE and is
        // handled as a failed attempt.
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

/// Resolve the user to authenticate as: `--username`, else the owner of the
/// real uid, else `$USER`.
fn resolve_username(flag: Option<&str>) -> Option<String> {
    if let Some(u) = flag {
        return Some(u.to_string());
    }
    // Single-threaded at startup, so getpwuid's static buffer is fine.
    let pw = unsafe { libc::getpwuid(libc::getuid()) };
    if !pw.is_null() {
        let name = unsafe { std::ffi::CStr::from_ptr((*pw).pw_name) };
        if let Ok(name) = name.to_str() {
            return Some(name.to_string());
        }
    }
    std::env::var("USER").ok()
}

/// Parse arguments, lock the screen, and block until unlocked.
pub fn run() {
    let args = Args::parse();

    let level = if args.debug {
        log::LevelFilter::Debug
    } else if args.debug_timing {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Warn
    };
    log::set_logger(&LOGGER).expect("logger already set");
    log::set_max_level(level);

    log::info!("baijia-suo starting");

    harden_process();

    if args.list_animations {
        let registry = crate::animation::AnimRegistry::new();
        let mut modes = registry.available_modes();
        modes.sort();
        for mode in modes {
            println!("{}", mode);
        }
        return;
    }

    if args.auth_test {
        run_auth_test(&args);
        return;
    }

    let cfg = match Config::from_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    log::debug!("Configuration loaded: {:?}", cfg);

    let mut app = App::new(cfg);

    // Initialize the authentication backend. Fail closed: if authentication
    // cannot be set up, refuse to lock rather than locking with no way back in.
    let username = match resolve_username(args.username.as_deref()) {
        Some(u) => u,
        None => {
            eprintln!("Error: could not determine username (try --username)");
            std::process::exit(1);
        }
    };

    let backend_kind = match args
        .auth_backend
        .as_deref()
        .unwrap_or("pam")
        .parse::<crate::auth::AuthBackendKind>()
    {
        Ok(kind) => kind,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = app.init_auth(&username, backend_kind) {
        eprintln!("Error: failed to initialize authentication backend: {}", e);
        eprintln!("Refusing to lock the screen without a working authentication backend.");
        std::process::exit(1);
    }

    // Phase 2: Wayland session lock
    if let Err(e) = crate::wayland::run_wayland(app) {
        log::error!("Wayland error: {}", e);
        std::process::exit(1);
    }
}

/// Run authentication test mode.
fn run_auth_test(args: &Args) {
    println!("=== baijia-suo Authentication Test ===\n");

    use crate::auth::AuthBackendKind;

    let backend_kind = args
        .auth_backend
        .as_ref()
        .and_then(|s| s.parse::<AuthBackendKind>().ok())
        .unwrap_or(AuthBackendKind::Pam);

    let username = match resolve_username(args.username.as_deref()) {
        Some(u) => u,
        None => {
            eprintln!("Error: could not determine username (try --username)");
            std::process::exit(1);
        }
    };

    println!("Using backend: {}", backend_kind);
    println!("Authenticating as: {}\n", username);

    let mut verifier = match crate::auth::create_verifier(backend_kind, &username) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to initialize authentication verifier:");
            eprintln!("  {}", e);
            std::process::exit(1);
        }
    };

    println!("Authentication verifier initialized successfully.");
    println!("Enter password to test (Ctrl+C to cancel):\n");

    print!("Password: ");
    let _ = std::io::stdout().flush();

    // Wrap so the plaintext heap copy is wiped on drop, matching the lock
    // path's "no un-zeroized transient copy" guarantee. rpassword handles
    // echo-off and restores the terminal on interrupt.
    let password_input = zeroize::Zeroizing::new(rpassword::read_password().unwrap_or_default());

    let mut password_buf = match crate::password::Password::new(256) {
        Ok(buf) => buf,
        Err(e) => {
            eprintln!("Failed to create password buffer: {}", e);
            std::process::exit(1);
        }
    };

    for c in password_input.chars() {
        if password_buf.append_char(c as u32).is_err() {
            eprintln!("Password too long");
            password_buf.clear();
            std::process::exit(1);
        }
    }

    println!("\nTesting authentication...");

    let mut secure_pw = match crate::secure::SecureBuffer::new(256) {
        Ok(buf) => buf,
        Err(e) => {
            eprintln!("Failed to create secure buffer: {}", e);
            password_buf.clear();
            std::process::exit(1);
        }
    };

    let _ = secure_pw.try_push(password_buf.as_bytes());
    password_buf.clear();

    // Authenticate
    match verifier.verify_blocking(secure_pw) {
        Ok((true, _)) => {
            println!("\n✓ Authentication SUCCESSFUL");
            std::process::exit(0);
        }
        Ok((false, message)) => {
            println!("\n✗ Authentication FAILED");
            if let Some(msg) = message {
                println!("  PAM: {msg}");
            }
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("\n✗ Authentication error: {}", e);
            std::process::exit(1);
        }
    }
}
