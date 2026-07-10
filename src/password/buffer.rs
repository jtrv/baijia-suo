use crate::secure::SecureBuffer;

/// A secure password buffer that handles UTF-8 encoding.
///
/// The underlying memory is page-aligned, mlock'd, and zeroized on drop.
pub struct Password {
    buf: SecureBuffer,
}

impl Password {
    /// Create a new password buffer with the given capacity in bytes.
    pub fn new(capacity: usize) -> Result<Self, PasswordError> {
        let buf = SecureBuffer::new(capacity).map_err(PasswordError::Buffer)?;
        Ok(Password { buf })
    }

    /// Append a Unicode codepoint as UTF-8. Rejects invalid codepoints
    /// (surrogates, > U+10FFFF) — this is what keeps `as_str`'s
    /// `from_utf8_unchecked` sound. Returns an error if the buffer would
    /// overflow (accounting for a trailing NUL byte).
    pub fn append_char(&mut self, codepoint: u32) -> Result<(), PasswordError> {
        let ch = char::from_u32(codepoint).ok_or(PasswordError::InvalidCodepoint)?;
        // Need room for the bytes + a trailing NUL
        if self.buf.len() + ch.len_utf8() + 1 > self.buf.capacity() {
            return Err(PasswordError::Overflow);
        }

        let mut utf8_bytes = [0u8; 4];
        let encoded = ch.encode_utf8(&mut utf8_bytes).as_bytes();
        self.buf.try_push(encoded).map_err(PasswordError::Buffer)?;
        Ok(())
    }

    /// Remove the last UTF-8 character from the buffer. Returns true if a
    /// character was removed, false if the buffer was empty.
    pub fn backspace(&mut self) -> bool {
        if self.buf.is_empty() {
            return false;
        }

        // Walk backwards to find the start of the last UTF-8 character
        let slice = self.buf.as_slice();
        let mut pos = slice.len();
        while pos > 0 {
            pos -= 1;
            if !is_continuation_byte(slice[pos]) {
                break;
            }
        }

        // Zero the removed bytes and truncate len
        let removed = slice.len() - pos;
        unsafe {
            let ptr = self.buf.as_mut_ptr().add(pos);
            std::ptr::write_bytes(ptr, 0, removed);
        }
        // We need to update the length — access via as_mut_slice and truncate
        self.buf.truncate(pos);
        true
    }

    /// Clear (zeroize) the entire password buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Return true if the password is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.len() == 0
    }

    /// View the password as a byte slice. Guaranteed valid UTF-8, since
    /// bytes only ever enter via `append_char`.
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.as_slice()
    }
}

/// Return true if the byte is a UTF-8 continuation byte (10xxxxxx).
fn is_continuation_byte(b: u8) -> bool {
    (b & 0xC0) == 0x80
}

#[derive(Debug)]
pub enum PasswordError {
    Buffer(crate::secure::SecureBufferError),
    Overflow,
    InvalidCodepoint,
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasswordError::Buffer(e) => write!(f, "password buffer error: {}", e),
            PasswordError::Overflow => write!(f, "password buffer overflow"),
            PasswordError::InvalidCodepoint => write!(f, "invalid Unicode codepoint"),
        }
    }
}

impl std::error::Error for PasswordError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_back() {
        let mut pw = Password::new(256).unwrap();
        pw.append_char('a' as u32).unwrap();
        pw.append_char('b' as u32).unwrap();
        pw.append_char('c' as u32).unwrap();
        assert_eq!(pw.as_bytes(), b"abc");
        assert!(!pw.is_empty());
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut pw = Password::new(256).unwrap();
        pw.append_char('a' as u32).unwrap();
        pw.append_char('b' as u32).unwrap();
        pw.append_char('c' as u32).unwrap();
        assert_eq!(pw.as_bytes().len(), 3);

        let removed = pw.backspace();
        assert!(removed);
        assert_eq!(pw.as_bytes(), b"ab");
    }

    #[test]
    fn backspace_empty_returns_false() {
        let mut pw = Password::new(256).unwrap();
        assert!(!pw.backspace());
    }

    #[test]
    fn utf8_multibyte_backspace() {
        let mut pw = Password::new(256).unwrap();
        // é is 2 bytes (U+00E9)
        pw.append_char(0x00E9).unwrap();
        // € is 3 bytes (U+20AC)
        pw.append_char(0x20AC).unwrap();
        assert_eq!(pw.as_bytes().len(), 5);

        pw.backspace(); // removes € (3 bytes)
        assert_eq!(pw.as_bytes(), "é".as_bytes());

        pw.backspace(); // removes é (2 bytes)
        assert!(pw.is_empty());
    }

    #[test]
    fn utf8_emoji() {
        let mut pw = Password::new(256).unwrap();
        // 🔒 is U+1F512, 4 bytes in UTF-8
        pw.append_char(0x1F512).unwrap();
        assert_eq!(pw.as_bytes(), "🔒".as_bytes());
    }

    #[test]
    fn clear_zeroes_buffer() {
        let mut pw = Password::new(256).unwrap();
        pw.append_char('s' as u32).unwrap();
        pw.append_char('e' as u32).unwrap();
        pw.append_char('c' as u32).unwrap();
        pw.clear();
        assert_eq!(pw.as_bytes(), b"");
        assert!(pw.is_empty());
    }

    #[test]
    fn backspace_all_chars() {
        let mut pw = Password::new(256).unwrap();
        pw.append_char('x' as u32).unwrap();
        pw.append_char('y' as u32).unwrap();
        assert!(pw.backspace());
        assert!(pw.backspace());
        assert!(!pw.backspace());
        assert!(pw.is_empty());
    }

    #[test]
    fn mixed_ascii_and_utf8() {
        let mut pw = Password::new(256).unwrap();
        pw.append_char('a' as u32).unwrap(); // 1 byte
        pw.append_char(0x00E9).unwrap(); // 2 bytes (é)
        pw.append_char('b' as u32).unwrap(); // 1 byte
        pw.append_char(0x1F512).unwrap(); // 4 bytes (🔒)
        assert_eq!(pw.as_bytes().len(), 1 + 2 + 1 + 4); // 8 bytes

        pw.backspace(); // remove 🔒
        assert_eq!(pw.as_bytes(), "aéb".as_bytes());
    }

    #[test]
    fn as_bytes_returns_raw_utf8() {
        let mut pw = Password::new(256).unwrap();
        pw.append_char('A' as u32).unwrap();
        assert_eq!(pw.as_bytes(), &[0x41]);
    }
}
