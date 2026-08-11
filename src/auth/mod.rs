//! Authentication subsystem for baijia-suo.
//!
//! One out-of-process transport — `ForkedVerifier` — forks a child that
//! authenticates and replies over a pipe. `VerifierSpec` selects backend
//! behavior (privilege drop, which PAM service) as data; there is no
//! backend trait, since every backend shares the same transport.

mod ipc;
mod pam_ffi;
mod verifier;

pub use verifier::{ForkedVerifier, VerifierError, VerifierSpec};

use std::fmt;

/// Authentication state, owned here with the rest of the auth domain.
///
/// `app` drives the transitions; `render::indicator` and `wayland` only read
/// the current value. Defined outside `app` so the render layer never
/// depends on `app` (keeps the module graph acyclic).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AuthState {
    Idle,
    Typing,
    Verifying,
    Success,
    Invalid,
}

/// Authentication backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthBackendKind {
    #[default]
    Pam,
    /// Alias for `Pam`, kept for CLI/config compatibility ("shadow").
    ///
    /// Both variants now behave identically: the forked child drops any
    /// inherited elevated gid/uid before authenticating, then authenticates
    /// via PAM — it does not read `/etc/shadow` or use `crypt(3)` directly.
    UnprivilegedPam,
}

impl fmt::Display for AuthBackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthBackendKind::Pam => write!(f, "pam"),
            AuthBackendKind::UnprivilegedPam => write!(f, "shadow"),
        }
    }
}

impl std::str::FromStr for AuthBackendKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pam" => Ok(AuthBackendKind::Pam),
            "shadow" | "crypt" => Ok(AuthBackendKind::UnprivilegedPam),
            _ => Err(format!("unknown auth backend: '{}'", s)),
        }
    }
}

/// Drop to the real (non-effective) uid/gid. Run once in the forked child,
/// before it starts handling requests. Fails closed: any partial drop
/// kills the child rather than leaving it half-privileged.
fn drop_privileges() {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };

    unsafe {
        // Supplementary groups: only root may call setgroups; for the
        // setgid install the child never has them elevated.
        if libc::geteuid() == 0 && libc::setgroups(0, std::ptr::null()) != 0 {
            eprintln!("auth child: setgroups failed");
            libc::_exit(1);
        }
        // setres* also clears the *saved* ids — plain setgid() from a
        // setgid binary leaves the saved gid regainable. Group first,
        // while we still have the privilege to change it.
        if libc::setresgid(gid, gid, gid) != 0 || libc::setresuid(uid, uid, uid) != 0 {
            eprintln!("auth child: privilege drop failed");
            libc::_exit(1);
        }
    }

    // Verify we can't regain root.
    let check_uid = unsafe { libc::getuid() };
    if check_uid == 0 {
        eprintln!("auth child: still root after privilege drop!");
        unsafe { libc::_exit(1) };
    }
}

/// Authenticate against the dedicated PAM service if the admin installed
/// one, else fall back to "login" (per README). Checked per attempt, in the
/// child — a config installed mid-session is picked up.
fn pam_verify(username: &str, password: &str) -> (bool, Option<String>) {
    let service = if std::path::Path::new("/etc/pam.d/baijia-suo").exists() {
        "baijia-suo"
    } else {
        "login"
    };
    pam_ffi::run_pam_auth(service, username, password)
}

fn spec_for(kind: AuthBackendKind) -> VerifierSpec {
    // Both backends drop any inherited elevated gid/uid in the child before
    // authenticating: pam_unix reaches /etc/shadow through the setuid-root
    // helper `unix_chkpwd`, so the PAM stack never needs the setgid-shadow
    // privilege the documented install grants. Dropping is a no-op for a
    // plain (non-setgid) install, so it is unconditional — no PAM stack ever
    // runs elevated. The `kind` distinction is retained only for the CLI
    // alias; the two specs are now identical.
    let _ = kind;
    VerifierSpec {
        on_child_start: Some(drop_privileges),
        verify: pam_verify,
    }
}

/// Spawn a verifier for the given backend and username.
///
/// Forks a child process immediately; fails closed if the fork or the
/// setuid check fails.
pub fn create_verifier(
    kind: AuthBackendKind,
    username: &str,
) -> Result<ForkedVerifier, VerifierError> {
    ForkedVerifier::spawn(spec_for(kind), username.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_backend_kind_from_str() {
        assert_eq!(
            "pam".parse::<AuthBackendKind>().unwrap(),
            AuthBackendKind::Pam
        );
        assert_eq!(
            "shadow".parse::<AuthBackendKind>().unwrap(),
            AuthBackendKind::UnprivilegedPam
        );
        assert_eq!(
            "crypt".parse::<AuthBackendKind>().unwrap(),
            AuthBackendKind::UnprivilegedPam
        );
        assert!("invalid".parse::<AuthBackendKind>().is_err());
    }
}
