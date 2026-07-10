//! Pipe-based IPC protocol for secure password transmission between parent and auth child.
//!
//! Uses length-prefixed messages with proper EINTR handling for signal safety.

use crate::secure::SecureBuffer;
use std::io::{self};
use std::os::unix::io::RawFd;
use zeroize::Zeroize;

/// Read exactly `len` bytes from the file descriptor, handling EINTR.
fn read_full(fd: RawFd, buf: &mut [u8]) -> io::Result<()> {
    let mut pos = 0;
    while pos < buf.len() {
        match unsafe {
            libc::read(
                fd,
                buf.as_mut_ptr().add(pos) as *mut libc::c_void,
                buf.len() - pos,
            )
        } {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF",
                ))
            }
            n if n < 0 => {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            n => pos += n as usize,
        }
    }
    Ok(())
}

/// Write exactly `len` bytes to the file descriptor, handling EINTR.
fn write_full(fd: RawFd, buf: &[u8]) -> io::Result<()> {
    let mut pos = 0;
    while pos < buf.len() {
        match unsafe {
            libc::write(
                fd,
                buf.as_ptr().add(pos) as *const libc::c_void,
                buf.len() - pos,
            )
        } {
            // write() returning 0 makes no progress; loop forever otherwise.
            0 => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            n if n < 0 => {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            n => pos += n as usize,
        }
    }
    Ok(())
}

/// Write a password request through the IPC channel.
///
/// Format: [4-byte length][password bytes]
/// After sending, the password buffer is zeroized.
pub fn write_request(fd: RawFd, mut password: SecureBuffer) -> io::Result<()> {
    let len = password.len() as u32;
    let len_bytes = len.to_be_bytes();

    write_full(fd, &len_bytes)?;
    if len > 0 {
        write_full(fd, password.as_slice())?;
    }
    password.zeroize();
    Ok(())
}

/// Read a password request from the IPC channel.
pub fn read_request(fd: RawFd) -> io::Result<SecureBuffer> {
    let mut len_bytes = [0u8; 4];
    read_full(fd, &mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes) as usize;

    // Sanity check - reject absurdly large requests
    const MAX_PASSWORD_LEN: usize = 64 * 1024;
    if len > MAX_PASSWORD_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "password length {} exceeds maximum {}",
                len, MAX_PASSWORD_LEN
            ),
        ));
    }

    // Read password data into a new SecureBuffer. SecureBuffer rejects a
    // zero capacity, so allocate at least one byte: an empty password must
    // still yield a valid (empty) buffer rather than an error.
    let mut buf = SecureBuffer::new(len.max(1))
        .map_err(|e| io::Error::other(format!("SecureBuffer allocation failed: {}", e)))?;

    if len > 0 {
        // Zeroizing wipes on every exit path, including the error returns.
        let mut temp_buf = zeroize::Zeroizing::new(vec![0u8; len]);
        read_full(fd, &mut temp_buf)?;
        buf.try_push(&temp_buf)
            .map_err(|e| io::Error::other(format!("Failed to push password: {}", e)))?;
    }

    Ok(buf)
}

/// Longest PAM message the child will relay to the parent.
const MAX_MSG_LEN: usize = 4096;

/// Write an authentication reply through the IPC channel.
///
/// Format: `[1 byte: 0=failure, 1=success][2-byte BE message length][message
/// bytes]`. The message is any text PAM surfaced (e.g. a faillock lockout
/// notice); empty when there is none.
pub fn write_reply(fd: RawFd, success: bool, message: Option<&str>) -> io::Result<()> {
    write_full(fd, &[if success { 1u8 } else { 0u8 }])?;
    let msg = message.unwrap_or("").as_bytes();
    let msg = &msg[..msg.len().min(MAX_MSG_LEN)];
    write_full(fd, &(msg.len() as u16).to_be_bytes())?;
    if !msg.is_empty() {
        write_full(fd, msg)?;
    }
    Ok(())
}

/// Read an authentication reply from the IPC channel: the success flag and any
/// PAM message the child relayed (`None` if empty).
pub fn read_reply(fd: RawFd) -> io::Result<(bool, Option<String>)> {
    let mut byte = [0u8];
    read_full(fd, &mut byte)?;
    let success = byte[0] != 0;

    let mut len_bytes = [0u8; 2];
    read_full(fd, &mut len_bytes)?;
    let len = u16::from_be_bytes(len_bytes) as usize;
    if len == 0 {
        return Ok((success, None));
    }
    let mut buf = vec![0u8; len];
    read_full(fd, &mut buf)?;
    Ok((success, Some(String::from_utf8_lossy(&buf).into_owned())))
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

    /// A request pipe (parent -> child) and a reply pipe (child -> parent).
    ///
    /// A single pipe cannot be shared for both directions: the parent's
    /// `read_reply` and the child's `read_request` would both read from the
    /// same fd and race to consume each other's bytes, deadlocking.
    struct Channel {
        req_read: RawFd,
        req_write: RawFd,
        reply_read: RawFd,
        reply_write: RawFd,
    }

    impl Channel {
        fn new() -> Self {
            let (req_read, req_write) = create_pipe();
            let (reply_read, reply_write) = create_pipe();
            Channel {
                req_read,
                req_write,
                reply_read,
                reply_write,
            }
        }
    }

    impl Drop for Channel {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.req_read);
                libc::close(self.req_write);
                libc::close(self.reply_read);
                libc::close(self.reply_write);
            }
        }
    }

    /// Run a request/reply roundtrip: send `payload`, have the child compare it
    /// against `expected` and reply with the result.
    fn roundtrip(payload: &[u8], expected: &'static [u8]) -> bool {
        let chan = Channel::new();
        let (req_read, reply_write) = (chan.req_read, chan.reply_write);

        let handle = thread::spawn(move || {
            let mut buf = read_request(req_read).unwrap();
            let success = buf.as_slice() == expected;
            write_reply(reply_write, success, None).unwrap();
            buf.zeroize();
        });

        let mut password = SecureBuffer::new(256).unwrap();
        if !payload.is_empty() {
            password.try_push(payload).unwrap();
        }
        write_request(chan.req_write, password).unwrap();

        let (success, _msg) = read_reply(chan.reply_read).unwrap();
        handle.join().unwrap();
        success
    }

    #[test]
    fn reply_relays_pam_message() {
        let (rfd, wfd) = create_pipe();
        let handle = thread::spawn(move || {
            write_reply(wfd, false, Some("Account locked (9 min left)")).unwrap();
        });
        let (success, msg) = read_reply(rfd).unwrap();
        handle.join().unwrap();
        assert!(!success);
        assert_eq!(msg.as_deref(), Some("Account locked (9 min left)"));
        unsafe {
            libc::close(rfd);
            libc::close(wfd);
        }
    }

    #[test]
    fn roundtrip_empty_password() {
        assert!(roundtrip(b"", b""));
    }

    #[test]
    fn roundtrip_ascii_password() {
        assert!(roundtrip(b"test_password_123", b"test_password_123"));
    }

    #[test]
    fn roundtrip_utf8_preserved() {
        // 🔒 is 4 bytes in UTF-8: F0 9F 94 92
        assert!(roundtrip(
            &[0xF0, 0x9F, 0x94, 0x92],
            &[0xF0, 0x9F, 0x94, 0x92]
        ));
    }

    #[test]
    fn reply_success_false() {
        let (rfd, wfd) = create_pipe();

        let handle = thread::spawn(move || {
            write_reply(wfd, false, None).unwrap();
        });

        let (success, _) = read_reply(rfd).unwrap();
        handle.join().unwrap();

        assert!(!success);

        unsafe {
            libc::close(rfd);
            libc::close(wfd);
        }
    }

    #[test]
    fn reply_success_true() {
        let (rfd, wfd) = create_pipe();

        let handle = thread::spawn(move || {
            write_reply(wfd, true, None).unwrap();
        });

        let (success, _) = read_reply(rfd).unwrap();
        handle.join().unwrap();

        assert!(success);

        unsafe {
            libc::close(rfd);
            libc::close(wfd);
        }
    }
}
