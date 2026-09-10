use libc::{c_void, mlock, munlock};
use std::alloc::{self, Layout};
use std::ptr::{self, NonNull};
use zeroize::Zeroize;

/// A page-aligned, mlock'd heap buffer that is zeroized on drop.
///
/// This is used for all sensitive data (passwords, key material) to prevent
/// the data from being swapped to disk and to ensure it is zeroed on free.
pub struct SecureBuffer {
    ptr: NonNull<u8>,
    layout: Layout,
    len: usize,
    /// Logical capacity requested by user.
    logical_capacity: usize,
    /// Actual allocated capacity (page-aligned).
    allocated_capacity: usize,
    mlocked: bool,
}

// SAFETY: SecureBuffer owns its memory exclusively and is not aliased.
unsafe impl Send for SecureBuffer {}

impl SecureBuffer {
    /// Create a new SecureBuffer with the given capacity.
    ///
    /// The buffer is page-aligned and mlock'd. Returns an error if
    /// allocation or mlock fails.
    pub fn new(capacity: usize) -> Result<Self, SecureBufferError> {
        if capacity == 0 {
            return Err(SecureBufferError::ZeroCapacity);
        }

        let page_size = page_size();
        // Align to page size, round capacity up to page boundary
        let aligned_capacity = (capacity + page_size - 1) & !(page_size - 1);
        let layout = Layout::from_size_align(aligned_capacity, page_size)
            .map_err(|_| SecureBufferError::LayoutError)?;

        // SAFETY: layout is non-zero (capacity > 0, page_size > 0)
        let ptr = unsafe { alloc::alloc_zeroed(layout) };
        let ptr = NonNull::new(ptr).ok_or(SecureBufferError::AllocFailed)?;

        let mut buf = SecureBuffer {
            ptr,
            layout,
            len: 0,
            logical_capacity: capacity,
            allocated_capacity: aligned_capacity,
            mlocked: false,
        };

        buf.lock_memory()?;
        Ok(buf)
    }

    /// Attempt to push bytes into the buffer. Returns an error if the buffer
    /// would overflow.
    pub fn try_push(&mut self, data: &[u8]) -> Result<(), SecureBufferError> {
        if self.len + data.len() > self.logical_capacity {
            return Err(SecureBufferError::Overflow {
                requested: data.len(),
                available: self.logical_capacity.saturating_sub(self.len),
            });
        }

        // SAFETY: we've verified there's enough logical capacity
        // Data is written to the actual memory (which is larger due to page alignment)
        unsafe {
            let dst = self.ptr.as_ptr().add(self.len);
            ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
        self.len += data.len();
        Ok(())
    }

    /// Set the buffer contents to all zeros and reset the length.
    pub fn clear(&mut self) {
        self.wipe_range(0, self.len);
        self.len = 0;
    }

    /// Truncate the buffer to the given length, zeroing the removed bytes.
    pub fn truncate(&mut self, new_len: usize) {
        if new_len >= self.len {
            return;
        }
        self.wipe_range(new_len, self.len - new_len);
        self.len = new_len;
    }

    /// Return the current length (number of bytes written).
    pub fn len(&self) -> usize {
        self.len
    }

    /// Return true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Return the total capacity.
    pub fn capacity(&self) -> usize {
        self.logical_capacity
    }

    #[cfg(test)]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// View the buffer contents as a byte slice.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr is valid for len bytes
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// View the buffer contents as a UTF-8 string.
    ///
    /// Returns None if the contents are not valid UTF-8.
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(self.as_slice()).ok()
    }

    fn lock_memory(&mut self) -> Result<(), SecureBufferError> {
        // SAFETY: ptr is valid for allocated_capacity bytes
        let ret = unsafe { mlock(self.ptr.as_ptr() as *const c_void, self.allocated_capacity) };
        if ret == 0 {
            self.mlocked = true;
            Ok(())
        } else {
            let err = std::io::Error::last_os_error();
            match err.raw_os_error() {
                Some(libc::EPERM) => {
                    // Insufficient RLIMIT_MEMLOCK — log warning but continue
                    log::warn!("mlock failed (EPERM): password may be swappable");
                    Ok(())
                }
                Some(libc::ENOMEM) => {
                    log::warn!("mlock failed (ENOMEM): insufficient memory to lock");
                    Ok(())
                }
                _ => Err(SecureBufferError::MlockFailed(err)),
            }
        }
    }

    fn unlock_memory(&mut self) {
        if self.mlocked {
            // SAFETY: ptr is valid for allocated_capacity bytes
            unsafe {
                munlock(self.ptr.as_ptr() as *const c_void, self.allocated_capacity);
            }
            self.mlocked = false;
        }
    }

    fn wipe_range(&mut self, start: usize, len: usize) {
        // Volatile writes prevent the compiler from eliding sensitive-data erasure.
        unsafe {
            let ptr = self.ptr.as_ptr().add(start);
            for i in 0..len {
                ptr::write_volatile(ptr.add(i), 0);
            }
        }
    }
}

impl Drop for SecureBuffer {
    fn drop(&mut self) {
        // Zero the entire allocated region
        self.zeroize();
        self.unlock_memory();
        // SAFETY: ptr was allocated with self.layout, and we're in drop
        unsafe {
            alloc::dealloc(self.ptr.as_ptr(), self.layout);
        }
    }
}

impl Zeroize for SecureBuffer {
    fn zeroize(&mut self) {
        self.wipe_range(0, self.allocated_capacity);
        self.len = 0;
    }
}

fn page_size() -> usize {
    // Cache the page size — it never changes at runtime
    use std::sync::OnceLock;
    static PAGE_SIZE: OnceLock<usize> = OnceLock::new();
    *PAGE_SIZE.get_or_init(|| {
        // SAFETY: sysconf is always safe to call
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if size > 0 {
            size as usize
        } else {
            4096
        }
    })
}

#[derive(Debug)]
pub enum SecureBufferError {
    ZeroCapacity,
    LayoutError,
    AllocFailed,
    MlockFailed(std::io::Error),
    Overflow { requested: usize, available: usize },
}

impl std::fmt::Display for SecureBufferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecureBufferError::ZeroCapacity => write!(f, "capacity must be non-zero"),
            SecureBufferError::LayoutError => write!(f, "invalid memory layout"),
            SecureBufferError::AllocFailed => write!(f, "allocation failed"),
            SecureBufferError::MlockFailed(e) => write!(f, "mlock failed: {}", e),
            SecureBufferError::Overflow {
                requested,
                available,
            } => {
                write!(
                    f,
                    "buffer overflow: {} bytes requested, {} available",
                    requested, available
                )
            }
        }
    }
}

impl std::error::Error for SecureBufferError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_is_page_aligned() {
        let mut buf = SecureBuffer::new(1024).unwrap();
        let ptr = buf.as_mut_ptr() as usize;
        let ps = page_size();
        assert_eq!(
            ptr % ps,
            0,
            "buffer pointer {:#x} not aligned to page size {}",
            ptr,
            ps
        );
    }

    #[test]
    fn buffer_capacity_rounded_to_page() {
        let buf = SecureBuffer::new(100).unwrap();
        // Logical capacity should be exactly what was requested
        assert_eq!(buf.capacity(), 100);
        // Test that we can use the full logical capacity
        let mut buf = SecureBuffer::new(100).unwrap();
        assert!(buf.try_push(&[0xAB; 100]).is_ok());
        assert_eq!(buf.len(), 100);
    }

    #[test]
    fn buffer_starts_empty() {
        let buf = SecureBuffer::new(256).unwrap();
        assert_eq!(buf.len(), 0);
        assert!(buf.as_slice().is_empty());
    }

    #[test]
    fn try_push_increases_len() {
        let mut buf = SecureBuffer::new(256).unwrap();
        buf.try_push(b"hello").unwrap();
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.as_slice(), b"hello");

        buf.try_push(b" world").unwrap();
        assert_eq!(buf.len(), 11);
        assert_eq!(buf.as_slice(), b"hello world");
    }

    #[test]
    fn try_push_overflow_rejected() {
        // Use capacity of 256 (which won't be aligned to a huge page)
        let mut buf = SecureBuffer::new(256).unwrap();
        // Fill the buffer
        assert!(buf.try_push(&[0xAA; 256]).is_ok());
        // One more byte should fail
        assert!(buf.try_push(b"x").is_err());
    }

    #[test]
    fn try_push_exact_capacity() {
        let mut buf = SecureBuffer::new(256).unwrap();
        let cap = 256; // Use the logical capacity we requested
        let data = vec![0xAA; cap];
        assert!(buf.try_push(&data).is_ok());
        assert_eq!(buf.len(), cap);
        // One more byte should fail
        assert!(buf.try_push(&[0xFF]).is_err());
    }

    #[test]
    fn clear_zeroes_and_resets_len() {
        let mut buf = SecureBuffer::new(256).unwrap();
        buf.try_push(b"secret data").unwrap();
        let ptr = buf.as_mut_ptr() as *const u8;

        buf.clear();
        assert_eq!(buf.len(), 0);

        // Verify the memory was actually zeroed
        let slice = unsafe { std::slice::from_raw_parts(ptr, 11) };
        assert!(
            slice.iter().all(|&b| b == 0),
            "buffer not zeroed after clear"
        );
    }

    #[test]
    fn zero_capacity_rejected() {
        assert!(matches!(
            SecureBuffer::new(0),
            Err(SecureBufferError::ZeroCapacity)
        ));
    }
}
