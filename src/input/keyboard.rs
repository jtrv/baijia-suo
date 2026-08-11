use std::os::unix::io::RawFd;
use xkbcommon::xkb::{self, Context, Keymap, State};

pub struct KeyboardHandler {
    context: Context,
    state: Option<State>,
    pub caps_lock: bool,
    pub ctrl: bool,
}

impl Default for KeyboardHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardHandler {
    pub fn new() -> Self {
        Self {
            context: Context::new(xkb::CONTEXT_NO_FLAGS),
            state: None,
            caps_lock: false,
            ctrl: false,
        }
    }

    /// Update the keymap from a Wayland keymap event.
    pub fn update_keymap(&mut self, fd: RawFd, size: usize) -> Result<(), String> {
        let mmap_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                fd,
                0,
            )
        };

        if mmap_ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err("Failed to mmap keymap".into());
        }

        let keymap_str = {
            let slice = unsafe { std::slice::from_raw_parts(mmap_ptr as *const u8, size) };
            // The string might be null-terminated, find the first null byte
            let len = slice.iter().position(|&b| b == 0).unwrap_or(size);
            // Lossy: the compositor should send valid UTF-8, but don't
            // build a &str on unchecked bytes (UB if it ever doesn't).
            String::from_utf8_lossy(&slice[..len]).into_owned()
        };

        let keymap = Keymap::new_from_string(
            &self.context,
            keymap_str,
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or("Failed to create keymap from string")?;

        self.state = Some(State::new(&keymap));

        unsafe {
            libc::munmap(mmap_ptr, size);
            libc::close(fd);
        }

        Ok(())
    }

    /// Process a key event and return the codepoint if any.
    pub fn process_key(&mut self, keycode: u32, pressed: bool) -> Option<u32> {
        // Wayland keycodes are +8 from xkb keycodes
        let xkb_keycode = keycode + 8;

        if !pressed {
            return None;
        }

        if let Some(ref mut state) = self.state {
            let keysym = state.key_get_one_sym(xkb_keycode.into());
            let codepoint = state.key_get_utf32(xkb_keycode.into());

            // Handle some specific control codes that xkb might not give us as utf32
            // like Enter, Backspace, Escape
            match keysym.into() {
                xkb::keysyms::KEY_Return | xkb::keysyms::KEY_KP_Enter => return Some(0x0D),
                xkb::keysyms::KEY_BackSpace => return Some(0x08),
                xkb::keysyms::KEY_Escape => return Some(0x1B),
                _ => {}
            }

            // If Ctrl is down, map some common keys
            if self.ctrl {
                match keysym.into() {
                    xkb::keysyms::KEY_u | xkb::keysyms::KEY_U => return Some(0x15), // Ctrl+U
                    xkb::keysyms::KEY_c | xkb::keysyms::KEY_C => return Some(0x03), // Ctrl+C
                    _ => {}
                }
            }

            if codepoint != 0 {
                return Some(codepoint);
            }
        }

        None
    }

    /// Update modifiers from Wayland event.
    pub fn update_modifiers(
        &mut self,
        mods_depressed: u32,
        mods_latched: u32,
        mods_locked: u32,
        group: u32,
    ) {
        if let Some(ref mut state) = self.state {
            state.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);

            self.caps_lock =
                state.mod_name_is_active(&xkb::MOD_NAME_CAPS, xkb::STATE_MODS_EFFECTIVE);
            self.ctrl = state.mod_name_is_active(&xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        }
    }
}
