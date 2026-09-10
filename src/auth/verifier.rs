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
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use zeroize::Zeroize;

// PAM modules can contact remote services; fail this attempt rather than leave
// the locked UI unresponsive forever.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE: Duration = Duration::from_millis(100);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const IO_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    worker: Option<JoinHandle<()>>,
    cancellation: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    spec: VerifierSpec,
    username: String,
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

        let pid = fork_child();
        match pid {
            0 => unsafe {
                drop(parent_read_fd);
                drop(parent_write_fd);
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
                run_child_loop(
                    spec,
                    &username,
                    child_read_fd.into_raw_fd(),
                    child_write_fd.into_raw_fd(),
                );
            },
            pid if pid > 0 => {
                drop(child_read_fd);
                drop(child_write_fd);
                Ok(ForkedVerifier {
                    pid: Some(pid),
                    read_fd: parent_read_fd.into_raw_fd(),
                    write_fd: parent_write_fd.into_raw_fd(),
                    pending: None,
                    worker: None,
                    cancellation: None,
                    deadline: None,
                    spec,
                    username,
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
        self.deadline = Some(
            Instant::now()
                .checked_add(ATTEMPT_TIMEOUT)
                .unwrap_or_else(Instant::now),
        );
        if self.pid.is_none() {
            if let Err(error) = self.replace_helper() {
                let _ = tx.send(Err(error));
                return;
            }
        }
        let write_fd = unsafe { libc::dup(self.write_fd) };
        let read_fd = unsafe { libc::dup(self.read_fd) };
        if write_fd < 0 || read_fd < 0 {
            if write_fd >= 0 {
                unsafe { libc::close(write_fd) };
            }
            if read_fd >= 0 {
                unsafe { libc::close(read_fd) };
            }
            let _ = tx.send(Err(VerifierError::IoError(std::io::Error::last_os_error())));
            return;
        }
        if let Err(error) = set_nonblocking(write_fd).and_then(|_| set_nonblocking(read_fd)) {
            unsafe {
                libc::close(write_fd);
                libc::close(read_fd);
            }
            let _ = tx.send(Err(error.into()));
            return;
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation);
        self.cancellation = Some(cancellation);
        self.worker = Some(std::thread::spawn(move || {
            let result = write_request_cancellable(write_fd, password, &worker_cancellation)
                .and_then(|_| read_reply_cancellable(read_fd, &worker_cancellation))
                .map_err(VerifierError::from);
            unsafe {
                libc::close(write_fd);
                libc::close(read_fd);
            }
            let _ = tx.send(result);
        }));
    }

    /// Non-blocking check for the attempt started with `start`. Returns
    /// `None` until the child has replied.
    pub fn poll(&mut self) -> Option<Result<AuthReply, VerifierError>> {
        let rx = self.pending.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.pending = None;
                self.deadline = None;
                self.join_worker();
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                self.deadline = None;
                self.join_worker();
                Some(Err(VerifierError::IoError(std::io::Error::other(
                    "verifier thread died",
                ))))
            }
        }
        .or_else(|| {
            if self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                self.pending = None;
                self.deadline = None;
                Some(self.restart().and(Err(VerifierError::TimedOut)))
            } else {
                None
            }
        })
    }

    /// Verify `password` synchronously on the calling thread. For one-shot
    /// callers (e.g. `--auth-test`) that don't need to keep an event loop
    /// responsive while waiting.
    pub fn verify_blocking(&mut self, password: SecureBuffer) -> Result<AuthReply, VerifierError> {
        self.start(password);
        loop {
            if let Some(result) = self.poll() {
                return result;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn join_worker(&mut self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.store(true, Ordering::Release);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.cancellation = None;
    }

    fn close_parent_fds(&mut self) {
        unsafe {
            if self.read_fd >= 0 {
                libc::close(self.read_fd);
                self.read_fd = -1;
            }
            if self.write_fd >= 0 {
                libc::close(self.write_fd);
                self.write_fd = -1;
            }
        }
    }

    fn wait_for_child(pid: pid_t, grace: Duration) -> bool {
        let deadline = Instant::now()
            .checked_add(grace)
            .unwrap_or_else(Instant::now);
        let mut status: libc::c_int = 0;
        loop {
            let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if result == pid
                || (result < 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD))
            {
                return true;
            }
            if result < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(WAIT_POLL_INTERVAL);
        }
    }

    fn reap_killed_child(pid: pid_t) {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        let _ = Self::wait_for_child(pid, SHUTDOWN_GRACE);
    }

    fn shutdown(&mut self) {
        self.pending = None;
        self.deadline = None;
        if let Some(pid) = self.pid.take() {
            if self.worker.is_some() {
                Self::reap_killed_child(pid);
                self.join_worker();
                self.close_parent_fds();
            } else {
                self.close_parent_fds();
                if !Self::wait_for_child(pid, SHUTDOWN_GRACE) {
                    Self::reap_killed_child(pid);
                }
            }
        } else {
            self.join_worker();
            self.close_parent_fds();
        }
    }

    fn restart(&mut self) -> Result<(), VerifierError> {
        self.pending = None;
        self.deadline = None;
        if let Some(pid) = self.pid.take() {
            Self::reap_killed_child(pid);
        }
        self.join_worker();
        self.close_parent_fds();

        self.replace_helper()
    }

    fn replace_helper(&mut self) -> Result<(), VerifierError> {
        // A PAM grandchild can hold the pipe open, so EOF is not a reliable wakeup.
        // Keep fork single-threaded: a worker must always be joined before re-forking.
        debug_assert!(self.worker.is_none());
        let mut replacement = Self::spawn(self.spec, self.username.clone())?;
        self.pid = replacement.pid.take();
        self.read_fd = replacement.read_fd;
        replacement.read_fd = -1;
        self.write_fd = replacement.write_fd;
        replacement.write_fd = -1;
        Ok(())
    }
}

#[cfg(not(test))]
fn fork_child() -> pid_t {
    unsafe { libc::fork() }
}

#[cfg(test)]
static FORCE_FORK_FAILURE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
fn fork_child() -> pid_t {
    if FORCE_FORK_FAILURE.load(Ordering::Acquire) {
        -1
    } else {
        unsafe { libc::fork() }
    }
}

impl Drop for ForkedVerifier {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn make_pipe() -> Result<(OwnedFd, OwnedFd), VerifierError> {
    let mut fds = [0 as RawFd, 0 as RawFd];
    // PAM modules may execute helpers; neither end may leak into them.
    let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if ret != 0 {
        return Err(VerifierError::IoError(std::io::Error::last_os_error()));
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn set_nonblocking(fd: RawFd) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn poll_cancellable(
    fd: RawFd,
    events: libc::c_short,
    cancellation: &AtomicBool,
) -> std::io::Result<()> {
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "authentication cancelled",
            ));
        }
        let mut pollfd = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let timeout = IO_POLL_INTERVAL
            .as_millis()
            .try_into()
            .unwrap_or(libc::c_int::MAX);
        let result = unsafe { libc::poll(&mut pollfd, 1, timeout) };
        if result > 0 {
            return Ok(());
        }
        if result == 0 {
            continue;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn write_full_cancellable(
    fd: RawFd,
    bytes: &[u8],
    cancellation: &AtomicBool,
) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        poll_cancellable(fd, libc::POLLOUT, cancellation)?;
        let result = unsafe {
            libc::write(
                fd,
                bytes[offset..].as_ptr() as *const libc::c_void,
                bytes.len() - offset,
            )
        };
        match result {
            n if n > 0 => offset += n as usize,
            0 => return Err(std::io::Error::from(std::io::ErrorKind::WriteZero)),
            _ => {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::WouldBlock
                    && error.kind() != std::io::ErrorKind::Interrupted
                {
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

fn read_full_cancellable(
    fd: RawFd,
    bytes: &mut [u8],
    cancellation: &AtomicBool,
) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        poll_cancellable(fd, libc::POLLIN, cancellation)?;
        let result = unsafe {
            libc::read(
                fd,
                bytes[offset..].as_mut_ptr() as *mut libc::c_void,
                bytes.len() - offset,
            )
        };
        match result {
            n if n > 0 => offset += n as usize,
            0 => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "unexpected EOF",
                ))
            }
            _ => {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::WouldBlock
                    && error.kind() != std::io::ErrorKind::Interrupted
                {
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

fn write_request_cancellable(
    fd: RawFd,
    mut password: SecureBuffer,
    cancellation: &AtomicBool,
) -> std::io::Result<()> {
    let len = (password.len() as u32).to_be_bytes();
    let result = write_full_cancellable(fd, &len, cancellation)
        .and_then(|_| write_full_cancellable(fd, password.as_slice(), cancellation));
    password.zeroize();
    result
}

fn read_reply_cancellable(fd: RawFd, cancellation: &AtomicBool) -> std::io::Result<AuthReply> {
    let mut status = [0_u8; 1];
    read_full_cancellable(fd, &mut status, cancellation)?;
    let success = match status[0] {
        0 => false,
        1 => true,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid reply status",
            ))
        }
    };
    let mut length = [0_u8; 2];
    read_full_cancellable(fd, &mut length, cancellation)?;
    let length = u16::from_be_bytes(length) as usize;
    if length > 4096 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "reply message too long",
        ));
    }
    if length == 0 {
        return Ok((success, None));
    }
    let mut message = vec![0; length];
    read_full_cancellable(fd, &mut message, cancellation)?;
    String::from_utf8(message)
        .map(|message| (success, Some(message)))
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "reply message is not UTF-8",
            )
        })
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
    TimedOut,
}

impl std::fmt::Display for VerifierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifierError::SetupRefused(s) => write!(f, "setup refused: {}", s),
            VerifierError::ForkFailed => write!(f, "fork failed"),
            VerifierError::IoError(s) => write!(f, "I/O error: {}", s),
            VerifierError::TimedOut => write!(f, "authentication timed out"),
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
            libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC);
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

    fn unavailable_verifier(spec: VerifierSpec) -> ForkedVerifier {
        ForkedVerifier {
            pid: None,
            read_fd: -1,
            write_fd: -1,
            pending: None,
            worker: None,
            cancellation: None,
            deadline: None,
            spec,
            username: "tester".into(),
        }
    }

    fn password(bytes: &[u8]) -> SecureBuffer {
        let mut password = SecureBuffer::new(16).unwrap();
        password.try_push(bytes).unwrap();
        password
    }

    #[test]
    fn unavailable_helper_retries_on_later_attempt() {
        let spec = VerifierSpec {
            on_child_start: None,
            verify: |_, password| (password == "hunter2", None),
        };
        let mut verifier = unavailable_verifier(spec);
        FORCE_FORK_FAILURE.store(true, Ordering::Release);
        verifier.start(password(b"hunter2"));
        assert!(matches!(
            verifier.poll(),
            Some(Err(VerifierError::ForkFailed))
        ));
        FORCE_FORK_FAILURE.store(false, Ordering::Release);
        assert!(verifier.verify_blocking(password(b"hunter2")).unwrap().0);
    }

    #[test]
    fn cancellation_does_not_depend_on_reply_pipe_eof() {
        let (read_fd, write_fd) = create_pipe();
        let holder = unsafe { libc::fork() };
        if holder == 0 {
            unsafe {
                libc::close(read_fd);
                libc::pause();
                libc::_exit(0);
            }
        }
        assert!(holder > 0);
        unsafe { libc::close(write_fd) };
        set_nonblocking(read_fd).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation);
        let worker = thread::spawn(move || read_reply_cancellable(read_fd, &worker_cancellation));
        std::thread::sleep(IO_POLL_INTERVAL * 2);
        let started = Instant::now();
        cancellation.store(true, Ordering::Release);
        assert!(worker.join().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        unsafe { libc::kill(holder, libc::SIGKILL) };
        assert!(ForkedVerifier::wait_for_child(
            holder,
            Duration::from_secs(1)
        ));
    }
}
