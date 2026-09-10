//! CLI entry point: argument parsing, process hardening, and the
//! `run()` consumed by the `baijia-suo` binary shim.

use crate::app::App;
use crate::args::Args;
use crate::config::Config;
use crate::secure::SecureBuffer;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use zeroize::Zeroizing;

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

struct TermiosRestore {
    fd: libc::c_int,
    termios: libc::termios,
    sigint: libc::sigaction,
    sigterm: libc::sigaction,
    sigquit: libc::sigaction,
}

impl Drop for TermiosRestore {
    fn drop(&mut self) {
        unsafe {
            while libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.termios) != 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
            {}
            libc::sigaction(libc::SIGINT, &self.sigint, std::ptr::null_mut());
            libc::sigaction(libc::SIGTERM, &self.sigterm, std::ptr::null_mut());
            libc::sigaction(libc::SIGQUIT, &self.sigquit, std::ptr::null_mut());
        }
    }
}

extern "C" fn termination_noop(_: libc::c_int) {}

/// Read from the controlling terminal so redirected stdin cannot provide a password.
fn read_password() -> std::io::Result<SecureBuffer> {
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    let fd = tty.as_raw_fd();
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let original = unsafe { original.assume_init() };
    let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
    sa.sa_sigaction = termination_noop as *const () as libc::sighandler_t;
    // Deliberately no SA_RESTART: ^C must make the read fail with EINTR so
    // the guard below restores echo instead of the process dying with it off.
    sa.sa_flags = 0;
    unsafe { libc::sigemptyset(&mut sa.sa_mask) };
    let install_signal = |signal| {
        let mut previous = std::mem::MaybeUninit::<libc::sigaction>::uninit();
        if unsafe { libc::sigaction(signal, &sa, previous.as_mut_ptr()) } != 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(unsafe { previous.assume_init() })
        }
    };
    let prev_sigint = install_signal(libc::SIGINT)?;
    let prev_sigterm = match install_signal(libc::SIGTERM) {
        Ok(previous) => previous,
        Err(error) => {
            unsafe { libc::sigaction(libc::SIGINT, &prev_sigint, std::ptr::null_mut()) };
            return Err(error);
        }
    };
    let prev_sigquit = match install_signal(libc::SIGQUIT) {
        Ok(previous) => previous,
        Err(error) => {
            unsafe {
                libc::sigaction(libc::SIGINT, &prev_sigint, std::ptr::null_mut());
                libc::sigaction(libc::SIGTERM, &prev_sigterm, std::ptr::null_mut());
            }
            return Err(error);
        }
    };
    let _restore = TermiosRestore {
        fd,
        termios: original,
        sigint: prev_sigint,
        sigterm: prev_sigterm,
        sigquit: prev_sigquit,
    };
    let mut no_echo = original;
    no_echo.c_lflag &= !libc::ECHO;
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &no_echo) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut bytes =
        SecureBuffer::new(256).map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut byte = Zeroizing::new([0_u8; 1]);
    loop {
        match tty.read(&mut byte[..])? {
            0 => break,
            _ if byte[0] == b'\n' => break,
            _ => bytes.try_push(&byte[..]).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "password exceeds 256 bytes",
                )
            })?,
        }
    }
    if bytes.as_slice().last() == Some(&b'\r') {
        bytes.truncate(bytes.len() - 1);
    }
    bytes.as_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "password is not valid UTF-8",
        )
    })?;
    Ok(bytes)
}

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
        std::process::exit(run_auth_test(&args));
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
fn run_auth_test(args: &Args) -> i32 {
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
            return 1;
        }
    };

    println!("Using backend: {}", backend_kind);
    println!("Authenticating as: {}\n", username);

    println!("Enter password to test (Ctrl+C to cancel):\n");

    print!("Password: ");
    let _ = std::io::stdout().flush();

    // Read from /dev/tty with echo disabled, so redirected stdin cannot submit a password.
    let password_input = match read_password() {
        Ok(password) => password,
        Err(e) => {
            eprintln!("Failed to read password: {e}");
            return 1;
        }
    };

    let mut password_buf = match crate::password::Password::new(256) {
        Ok(buf) => buf,
        Err(e) => {
            eprintln!("Failed to create password buffer: {}", e);
            return 1;
        }
    };

    for c in password_input.as_str().unwrap_or_default().chars() {
        if password_buf.append_char(c as u32).is_err() {
            eprintln!("Password too long");
            password_buf.clear();
            return 1;
        }
    }

    let mut secure_pw = match crate::secure::SecureBuffer::new(256) {
        Ok(buf) => buf,
        Err(e) => {
            eprintln!("Failed to create secure buffer: {}", e);
            password_buf.clear();
            return 1;
        }
    };

    let _ = secure_pw.try_push(password_buf.as_bytes());
    password_buf.clear();
    drop(password_buf);
    drop(password_input);

    let mut verifier = match crate::auth::create_verifier(backend_kind, &username) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to initialize authentication verifier:");
            eprintln!("  {}", e);
            return 1;
        }
    };

    println!("Authentication verifier initialized successfully.");

    println!("\nTesting authentication...");

    // Authenticate
    match verifier.verify_blocking(secure_pw) {
        Ok((true, _)) => {
            println!("\n✓ Authentication SUCCESSFUL");
            0
        }
        Ok((false, message)) => {
            println!("\n✗ Authentication FAILED");
            if let Some(msg) = message {
                println!("  PAM: {msg}");
            }
            1
        }
        Err(e) => {
            eprintln!("\n✗ Authentication error: {}", e);
            1
        }
    }
}
