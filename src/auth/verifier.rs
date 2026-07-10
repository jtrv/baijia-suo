//! ForkedVerifier: one out-of-process, pipe-based password verifier.
//!
//! Forks a child process that loops: read a password over a pipe, verify
//! it, write back a yes/no reply. The loop (rather than exiting after one
//! attempt) is what lets the locker re-prompt after a wrong password
//! without re-forking. Backend-specific behavior (privilege drop, which PAM
//! service, how verification actually happens) is data — a `VerifierSpec`
//! — not a trait implementation, since every backend shares the same
//! transport.

use crate::auth::ipc;
use crate::secure::SecureBuffer;
use libc::pid_t;
use std::os::unix::io::RawFd;
use std::sync::mpsc;
use zeroize::Zeroize;

/// The outcome of one verification: whether it succeeded, plus any message the
/// backend surfaced (e.g. a PAM faillock lockout notice) to relay for display.
pub type AuthReply = (bool, Option<String>);

/// What a forked child does before entering its request loop, and how it
/// turns a password into a yes/no. Two real specs exist — `Pam` and
/// `UnprivilegedPam` (see `auth::spec_for`) — plus an in-memory spec used
/// only in tests, which is what justifies this being a seam at all.
#[derive(Clone, Copy)]
pub struct VerifierSpec {
    /// Run once in the child, before the request loop (e.g. privilege drop).
    pub on_child_start: Option<fn()>,
    /// Verify a password. Called in the child for every attempt. Returns
    /// whether it succeeded and any message the backend surfaced (e.g. a PAM
    /// faillock lockout notice) to relay to the parent for display.
    pub verify: fn(username: &str, password: &str) -> AuthReply,
}

/// A live, forked verifier. Long-lived: spawned once per lock session,
/// `start`/`poll` per authentication attempt.
pub struct ForkedVerifier {
    pid: Option<pid_t>,
    read_fd: RawFd,
    write_fd: RawFd,
    pending: Option<mpsc::Receiver<Result<AuthReply, VerifierError>>>,
}

impl ForkedVerifier {
    /// Fork a child running `spec` for `username`.
    ///
    /// Refuses to run if the binary is setuid — fail closed rather than
    /// authenticate with unexpected privilege. (Deployments that need
    /// elevated access, e.g. to `/etc/shadow`, install the binary setgid
    /// instead; that leaves `getuid`/`geteuid` unaffected.)
    pub fn spawn(spec: VerifierSpec, username: String) -> Result<Self, VerifierError> {
        let uid = unsafe { libc::getuid() };
        let euid = unsafe { libc::geteuid() };
        if uid != euid {
            return Err(VerifierError::SetupRefused(
                "refusing to run as setuid binary".into(),
            ));
        }

        let (child_read_fd, parent_write_fd) = make_pipe()?;
        let (parent_read_fd, child_write_fd) = make_pipe()?;

        let pid = unsafe { libc::fork() };
        match pid {
            0 => unsafe {
                libc::close(parent_read_fd);
                libc::close(parent_write_fd);
                // Pin the whole child in RAM: PAM and libc make heap copies
                // of the password we can't individually mlock. The child's
                // footprint is a few MB, so MCL_FUTURE is safe here (unlike
                // in the parent, whose multi-MB frame buffers could blow
                // RLIMIT_MEMLOCK and fail rendering). Best-effort: EPERM
                // under a tight rlimit just means swappable, not broken.
                libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE);
                if let Some(f) = spec.on_child_start {
                    f();
                }
                run_child_loop(spec, &username, child_read_fd, child_write_fd);
            },
            pid if pid > 0 => {
                unsafe {
                    libc::close(child_read_fd);
                    libc::close(child_write_fd);
                }
                Ok(ForkedVerifier {
                    pid: Some(pid),
                    read_fd: parent_read_fd,
                    write_fd: parent_write_fd,
                    pending: None,
                })
            }
            _ => Err(VerifierError::ForkFailed),
        }
    }

    /// Begin verifying `password` on a background thread — the pipe round
    /// trip would otherwise block the Wayland event loop. Call `poll()` to
    /// check for completion.
    pub fn start(&mut self, password: SecureBuffer) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let write_fd = self.write_fd;
        let read_fd = self.read_fd;
        std::thread::spawn(move || {
            let result = ipc::write_request(write_fd, password)
                .and_then(|_| ipc::read_reply(read_fd))
                .map_err(VerifierError::from);
            let _ = tx.send(result);
        });
    }

    /// Non-blocking check for the attempt started with `start`. Returns
    /// `None` until the child has replied.
    pub fn poll(&mut self) -> Option<Result<AuthReply, VerifierError>> {
        let rx = self.pending.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.pending = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Err(VerifierError::IoError(std::io::Error::other(
                    "verifier thread died",
                ))))
            }
        }
    }

    /// Verify `password` synchronously on the calling thread. For one-shot
    /// callers (e.g. `--auth-test`) that don't need to keep an event loop
    /// responsive while waiting.
    pub fn verify_blocking(&mut self, password: SecureBuffer) -> Result<AuthReply, VerifierError> {
        ipc::write_request(self.write_fd, password)?;
        Ok(ipc::read_reply(self.read_fd)?)
    }

    fn shutdown(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
        if let Some(pid) = self.pid.take() {
            let mut status: libc::c_int = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
        }
    }
}

impl Drop for ForkedVerifier {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn make_pipe() -> Result<(RawFd, RawFd), VerifierError> {
    let mut fds = [0 as RawFd, 0 as RawFd];
    let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(VerifierError::IoError(std::io::Error::last_os_error()));
    }
    Ok((fds[0], fds[1]))
}

/// Result of one child-loop iteration.
enum LoopSignal {
    Continue,
    /// Parent closed the request pipe, or went away before a reply could be
    /// sent: nothing more to do.
    CleanShutdown,
    /// Unrecoverable I/O error.
    ErrorShutdown,
}

/// Read one password, verify it, write back the reply.
///
/// Split out from `run_child_loop` so it can run on a plain thread in
/// tests — `run_child_loop`'s `_exit()` calls would otherwise tear down
/// the whole test process if called outside a real forked child.
fn child_loop_once(
    spec: &VerifierSpec,
    username: &str,
    read_fd: RawFd,
    write_fd: RawFd,
) -> LoopSignal {
    let mut password = match ipc::read_request(read_fd) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return LoopSignal::CleanShutdown
        }
        Err(e) => {
            eprintln!("auth child: read password failed: {}", e);
            return LoopSignal::ErrorShutdown;
        }
    };

    let (success, message) = match password.as_str() {
        // Borrow straight from the mlocked buffer — no intermediate copy.
        Some(s) => (spec.verify)(username, s),
        None => {
            eprintln!("auth child: invalid UTF-8 in password");
            (false, None)
        }
    };
    password.zeroize();

    // Rate-limit: slow brute-force attempts after any failed attempt,
    // regardless of backend.
    if !success {
        unsafe { libc::sleep(2) };
    }

    if ipc::write_reply(write_fd, success, message.as_deref()).is_err() {
        return LoopSignal::CleanShutdown;
    }

    LoopSignal::Continue
}

/// Runs forever inside the forked child, exiting the process on shutdown.
unsafe fn run_child_loop(spec: VerifierSpec, username: &str, read_fd: RawFd, write_fd: RawFd) -> ! {
    loop {
        match child_loop_once(&spec, username, read_fd, write_fd) {
            LoopSignal::Continue => {}
            LoopSignal::CleanShutdown => {
                libc::close(read_fd);
                libc::close(write_fd);
                libc::_exit(0);
            }
            LoopSignal::ErrorShutdown => {
                libc::close(read_fd);
                libc::close(write_fd);
                libc::_exit(1);
            }
        }
    }
}

#[derive(Debug)]
pub enum VerifierError {
    SetupRefused(String),
    ForkFailed,
    IoError(std::io::Error),
}

impl std::fmt::Display for VerifierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifierError::SetupRefused(s) => write!(f, "setup refused: {}", s),
            VerifierError::ForkFailed => write!(f, "fork failed"),
            VerifierError::IoError(s) => write!(f, "I/O error: {}", s),
        }
    }
}

impl std::error::Error for VerifierError {}

impl From<std::io::Error> for VerifierError {
    fn from(e: std::io::Error) -> Self {
        VerifierError::IoError(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn create_pipe() -> (RawFd, RawFd) {
        let mut fds = [0 as RawFd, 0 as RawFd];
        unsafe {
            libc::pipe(fds.as_mut_ptr());
        }
        (fds[0], fds[1])
    }

    /// Exercises the child loop's framing, verify call, and clean-shutdown
    /// detection without forking or touching PAM — the seam that
    /// `VerifierSpec.verify` being plain data (not a trait impl) makes
    /// possible. Uses a real thread + real pipes, matching the style of
    /// `ipc`'s own tests.
    #[test]
    fn child_loop_verifies_and_detects_shutdown() {
        let (req_read, req_write) = create_pipe();
        let (reply_read, reply_write) = create_pipe();

        let spec = VerifierSpec {
            on_child_start: None,
            verify: |_, pw| (pw == "hunter2", None),
        };

        let handle = thread::spawn(move || {
            assert!(matches!(
                child_loop_once(&spec, "tester", req_read, reply_write),
                LoopSignal::Continue
            ));
            assert!(matches!(
                child_loop_once(&spec, "tester", req_read, reply_write),
                LoopSignal::CleanShutdown
            ));
            unsafe {
                libc::close(req_read);
                libc::close(reply_write);
            }
        });

        let mut pw = SecureBuffer::new(16).unwrap();
        pw.try_push(b"hunter2").unwrap();
        ipc::write_request(req_write, pw).unwrap();
        assert!(ipc::read_reply(reply_read).unwrap().0);

        // Close the request pipe: next read hits EOF -> CleanShutdown.
        unsafe { libc::close(req_write) };
        handle.join().unwrap();
        unsafe { libc::close(reply_read) };
    }

    /// A wrong password must still produce a reply, not silently hang.
    #[test]
    fn child_loop_rejects_wrong_password() {
        let (req_read, req_write) = create_pipe();
        let (reply_read, reply_write) = create_pipe();

        let spec = VerifierSpec {
            on_child_start: None,
            verify: |_, pw| (pw == "hunter2", None),
        };

        let handle = thread::spawn(move || {
            let signal = child_loop_once(&spec, "tester", req_read, reply_write);
            unsafe {
                libc::close(req_read);
                libc::close(reply_write);
            }
            signal
        });

        let mut pw = SecureBuffer::new(16).unwrap();
        pw.try_push(b"wrong").unwrap();
        ipc::write_request(req_write, pw).unwrap();
        assert!(!ipc::read_reply(reply_read).unwrap().0);

        assert!(matches!(handle.join().unwrap(), LoopSignal::Continue));
        unsafe {
            libc::close(req_write);
            libc::close(reply_read);
        }
    }
}
