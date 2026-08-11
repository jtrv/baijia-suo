use libc::{
    c_char, c_void, ftruncate, memfd_create, mmap, munmap, MAP_SHARED, MFD_CLOEXEC, PROT_READ,
    PROT_WRITE,
};
use std::ffi::CString;
use std::os::unix::io::BorrowedFd;
use std::ptr;
use std::sync::{Arc, Mutex};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::{Dispatch, QueueHandle};

use crate::render::BufferContent;

/// A shared memory buffer for Wayland rendering.
pub struct PoolBuffer {
    buffer: WlBuffer,
    busy: Arc<Mutex<bool>>,
    mmap_ptr: *mut c_void,
    size: usize,
    width: i32,
    height: i32,
    content: BufferContent,
}

// Ensure PoolBuffer is Send + Sync
unsafe impl Send for PoolBuffer {}
unsafe impl Sync for PoolBuffer {}

impl PoolBuffer {
    /// Creates a new PoolBuffer.
    pub fn new<D>(shm: &WlShm, width: i32, height: i32, qh: &QueueHandle<D>) -> Result<Self, String>
    where
        D: Dispatch<WlBuffer, Arc<Mutex<bool>>> + Dispatch<WlShmPool, ()> + 'static,
    {
        let stride = width * 4;
        let size = (stride * height) as usize;

        let name = CString::new("baijia-suo-shm").unwrap();
        let fd = unsafe { memfd_create(name.as_ptr() as *const c_char, MFD_CLOEXEC) };
        if fd < 0 {
            return Err("Failed to create memfd".into());
        }

        // Truncate to size
        let ret = unsafe { ftruncate(fd, size as libc::off_t) };
        if ret < 0 {
            unsafe { libc::close(fd) };
            return Err("Failed to truncate memfd".into());
        }

        // Mmap
        let mmap_ptr = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                fd,
                0,
            )
        };
        if mmap_ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err("Failed to mmap memfd".into());
        }

        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let pool = shm.create_pool(borrowed_fd, size as i32, qh, ());
        // Create the busy flag first; pass the same Arc as wl_buffer user data so the
        // Release event handler (Dispatch<WlBuffer, Arc<Mutex<bool>>>) can clear it.
        let busy = Arc::new(Mutex::new(false));
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            stride,
            wl_shm::Format::Argb8888,
            qh,
            busy.clone(),
        );

        // We can destroy the pool, the buffer will still be valid
        pool.destroy();
        unsafe { libc::close(fd) };

        // Initialize every pixel to opaque black. The mmap starts zero-filled
        // (BGRA = 0,0,0,0 = transparent), which lets the compositor bleed the
        // desktop through on lock surfaces that don't paint every pixel.
        let pixel_slice = unsafe { std::slice::from_raw_parts_mut(mmap_ptr as *mut u32, size / 4) };
        pixel_slice.fill(0xFF00_0000u32);

        Ok(PoolBuffer {
            buffer,
            busy,
            mmap_ptr,
            size,
            width,
            height,
            content: BufferContent::default(),
        })
    }

    pub fn buffer(&self) -> &WlBuffer {
        &self.buffer
    }

    /// The raw mmap'd `wl_shm` pixels as a mutable BGRA (Argb8888) slice.
    /// Stride is exactly `width * 4` (no padding), so callers can treat it as
    /// a tightly packed `height` × `width` BGRA image.
    pub fn data_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.mmap_ptr as *mut u8, self.size) }
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn is_busy(&self) -> bool {
        *self.busy.lock().unwrap()
    }

    pub fn set_busy(&self, busy: bool) {
        *self.busy.lock().unwrap() = busy;
    }

    pub(crate) fn content(&self) -> BufferContent {
        self.content
    }

    pub(crate) fn set_content(&mut self, content: BufferContent) {
        self.content = content;
    }
}

impl Drop for PoolBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        unsafe {
            munmap(self.mmap_ptr, self.size);
        }
    }
}

/// A double-buffered pool for a single output.
pub struct DoublePool {
    buffers: Vec<PoolBuffer>,
}

impl Default for DoublePool {
    fn default() -> Self {
        Self::new()
    }
}

impl DoublePool {
    pub fn new() -> Self {
        DoublePool {
            buffers: Vec::new(),
        }
    }

    pub fn get_buffer<D>(
        &mut self,
        shm: &WlShm,
        width: i32,
        height: i32,
        qh: &QueueHandle<D>,
    ) -> Result<&mut PoolBuffer, String>
    where
        D: Dispatch<WlBuffer, Arc<Mutex<bool>>> + Dispatch<WlShmPool, ()> + 'static,
    {
        // First check if we have a free buffer of the correct size
        if let Some(index) = self
            .buffers
            .iter()
            .position(|b| !b.is_busy() && b.width() == width && b.height() == height)
        {
            return Ok(&mut self.buffers[index]);
        }

        // If not, we might need to create one. Let's prune buffers that are not the right size and not busy.
        self.buffers
            .retain(|b| b.is_busy() || (b.width() == width && b.height() == height));

        // Bound the pool: frame callbacks can arrive before the compositor
        // releases its wl_buffer, so pacing alone doesn't keep this at two
        // entries — and a 4K BGRA buffer is ~33 MB. Three matching buffers
        // (one on screen, one queued, one being drawn) is the ceiling any
        // sane compositor needs; past that, refuse and let the caller skip
        // this presentation (the next frame callback retries).
        const MAX_MATCHING: usize = 3;
        let matching = self
            .buffers
            .iter()
            .filter(|b| b.width() == width && b.height() == height)
            .count();
        if matching >= MAX_MATCHING {
            return Err(format!(
                "all {matching} pool buffers busy for {width}x{height}; skipping frame"
            ));
        }

        let buffer = PoolBuffer::new(shm, width, height, qh)?;
        self.buffers.push(buffer);

        Ok(self.buffers.last_mut().unwrap())
    }
}
