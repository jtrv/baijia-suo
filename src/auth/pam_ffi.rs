//! Raw PAM (Pluggable Authentication Modules) FFI bindings.
//!
//! Only ever called from inside the forked, unprivileged auth child (see
//! `verifier`) — never from the parent process.

use std::ffi::CString;
use std::ptr;
use zeroize::Zeroize;

#[link(name = "pam")]
#[link(name = "pam_misc")]
extern "C" {
    fn pam_start(
        service_name: *const libc::c_char,
        user: *const libc::c_char,
        conv: *mut libc::c_void,
        pamh: *mut *mut libc::c_void,
    ) -> libc::c_int;

    fn pam_authenticate(pamh: *mut libc::c_void, flags: libc::c_int) -> libc::c_int;

    // Account phase: enforces expiry, locking, and time/access restrictions
    // that pam_authenticate does not check. Run after a successful auth.
    fn pam_acct_mgmt(pamh: *mut libc::c_void, flags: libc::c_int) -> libc::c_int;

    // Refresh credentials (e.g. Kerberos ticket lifetime) on unlock.
    fn pam_setcred(pamh: *mut libc::c_void, flags: libc::c_int) -> libc::c_int;

    fn pam_end(pamh: *mut libc::c_void, pam_status: libc::c_int) -> libc::c_int;
}

const PAM_SUCCESS: libc::c_int = 0;
const PAM_AUTH_ERR: libc::c_int = 4;
/// `pam_setcred` flag: refresh existing credentials without re-establishing.
const PAM_REFRESH_CRED: libc::c_int = 0x0010;
/// Bound retained PAM conversation text to the IPC payload limit.
const MAX_PAM_MESSAGE_BYTES: usize = 4096;

/// PAM message structure matching C struct.
#[repr(C)]
struct PamMessage {
    msg_style: libc::c_int,
    msg: *const libc::c_char,
}

/// PAM response structure matching C struct.
#[repr(C)]
struct PamResponse {
    resp: *mut libc::c_char,
    resp_retcode: libc::c_int,
}

/// PAM conversation structure matching C struct.
#[repr(C)]
struct PamConv {
    conv: Option<
        unsafe extern "C" fn(
            num_msg: libc::c_int,
            msg: *const *mut PamMessage,
            resp: *mut *mut PamResponse,
            appdata_ptr: *mut libc::c_void,
        ) -> libc::c_int,
    >,
    appdata_ptr: *mut libc::c_void,
}

/// State passed to PAM conversation callback. Holds the single in-child
/// copy of the password; `wipe_cstring` erases it wherever it ends up.
struct PamConvState {
    password: Option<CString>,
    /// Informational/error messages PAM emitted during the conversation
    /// (e.g. a faillock lockout notice), collected to relay to the parent.
    messages: Vec<String>,
    message_bytes: usize,
}

impl PamConvState {
    /// None for an empty password or one with an interior NUL — both make
    /// the conversation fail closed at the prompt.
    fn new(password: &str) -> Self {
        let password = if password.is_empty() {
            None
        } else {
            CString::new(password).ok()
        };
        PamConvState {
            password,
            messages: Vec::new(),
            message_bytes: 0,
        }
    }

    /// Consume the password for a single prompt.
    /// Returns None if already consumed or never set.
    fn take_password(&mut self) -> Option<CString> {
        self.password.take()
    }

    fn push_message(&mut self, message: &std::ffi::CStr) {
        let separator = usize::from(!self.messages.is_empty());
        let remaining = MAX_PAM_MESSAGE_BYTES
            .saturating_sub(self.message_bytes)
            .saturating_sub(separator);
        if remaining == 0 {
            return;
        }
        let mut text =
            String::from_utf8_lossy(&message.to_bytes()[..message.to_bytes().len().min(remaining)])
                .trim()
                .to_string();
        while text.len() > remaining {
            text.pop();
        }
        if !text.is_empty() {
            self.message_bytes += separator + text.len();
            self.messages.push(text);
        }
    }
}

impl Drop for PamConvState {
    fn drop(&mut self) {
        if let Some(pw) = self.password.take() {
            wipe_cstring(pw);
        }
    }
}

/// Zero a CString's bytes before the allocation is freed.
fn wipe_cstring(s: CString) {
    s.into_bytes().zeroize();
}

unsafe fn wipe_and_free_response(response: *mut libc::c_char) {
    let len = libc::strlen(response);
    for i in 0..=len {
        ptr::write_volatile(response.add(i), 0);
    }
    libc::free(response as *mut libc::c_void);
}

/// PAM conversation callback.
///
/// Called by PAM during authentication to obtain credentials.
/// Handles both password (echo_off) and username (echo_on) prompts.
///
/// The PAM API requires the callback to allocate the response array with
/// malloc/calloc and write its address into *resp. PAM frees it afterwards.
/// *resp is NOT pre-allocated by PAM; accessing it before writing is UB.
unsafe extern "C" fn pam_conv_callback(
    num_msg: libc::c_int,
    msg: *const *mut PamMessage,
    resp: *mut *mut PamResponse,
    appdata_ptr: *mut libc::c_void,
) -> libc::c_int {
    if num_msg <= 0 || msg.is_null() || resp.is_null() || appdata_ptr.is_null() {
        return PAM_AUTH_ERR;
    }

    let n = num_msg as usize;
    let state = &mut *(appdata_ptr as *mut PamConvState);
    let messages = std::slice::from_raw_parts(msg, n);

    // Allocate the response array; PAM takes ownership and will free it.
    let replies = libc::calloc(n, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
    if replies.is_null() {
        return PAM_AUTH_ERR;
    }

    for (i, msg_ptr) in messages.iter().enumerate() {
        let message = &**msg_ptr;
        let reply = &mut *replies.add(i);

        match message.msg_style {
            1 => {
                // PAM_PROMPT_ECHO_OFF — password prompt
                if let Some(pw) = state.take_password() {
                    // PAM takes ownership of the strdup'd copy and frees it.
                    reply.resp = libc::strdup(pw.as_ptr());
                    reply.resp_retcode = 0;
                    wipe_cstring(pw);
                } else {
                    // Free already-filled replies before returning
                    for j in 0..i {
                        let r = &mut *replies.add(j);
                        if !r.resp.is_null() {
                            wipe_and_free_response(r.resp);
                        }
                    }
                    libc::free(replies as *mut libc::c_void);
                    return PAM_AUTH_ERR;
                }
            }
            2 => {
                // PAM_PROMPT_ECHO_ON — username prompt (already set in pam_start)
                reply.resp = ptr::null_mut();
                reply.resp_retcode = 0;
            }
            3 | 4 => {
                // PAM_ERROR_MSG / PAM_TEXT_INFO — capture the text to relay,
                // then acknowledge with an empty response.
                if !message.msg.is_null() {
                    state.push_message(std::ffi::CStr::from_ptr(message.msg));
                }
                reply.resp = ptr::null_mut();
                reply.resp_retcode = 0;
            }
            _ => {
                for j in 0..=i {
                    let r = &mut *replies.add(j);
                    if !r.resp.is_null() {
                        wipe_and_free_response(r.resp);
                    }
                }
                libc::free(replies as *mut libc::c_void);
                return PAM_AUTH_ERR;
            }
        }
    }

    *resp = replies;
    PAM_SUCCESS
}

/// Authenticate `password` for `username` against the named PAM service.
///
/// Matches `VerifierSpec::verify`'s signature so it can be used directly as
/// a backend's verification function.
pub(crate) fn run_pam_auth(
    service_name: &str,
    username: &str,
    password: &str,
) -> (bool, Option<String>) {
    // SAFETY: pam_start/pam_authenticate/pam_end are standard PAM FFI calls;
    // conv_state outlives the pam_* calls that reference it via appdata_ptr.
    unsafe {
        let mut conv_state = PamConvState::new(password);

        let conv = PamConv {
            conv: Some(pam_conv_callback),
            appdata_ptr: &mut conv_state as *mut _ as *mut libc::c_void,
        };
        let mut handle: *mut libc::c_void = ptr::null_mut();

        let service_cstr = match CString::new(service_name) {
            Ok(s) => s,
            Err(_) => return (false, None),
        };
        let user_cstr = match CString::new(username) {
            Ok(s) => s,
            Err(_) => return (false, None),
        };

        let ret = pam_start(
            service_cstr.as_ptr(),
            user_cstr.as_ptr(),
            &conv as *const PamConv as *mut libc::c_void,
            &mut handle,
        );

        let success = if ret == PAM_SUCCESS {
            // Both phases must pass: pam_authenticate proves the password,
            // pam_acct_mgmt enforces account validity (expiry, lock, access
            // restrictions). Fail closed if either rejects.
            if pam_authenticate(handle, 0) == PAM_SUCCESS && pam_acct_mgmt(handle, 0) == PAM_SUCCESS
            {
                // Best-effort credential refresh; never gate the unlock on it.
                pam_setcred(handle, PAM_REFRESH_CRED);
                true
            } else {
                false
            }
        } else {
            false
        };

        pam_end(handle, 0);

        let message = if conv_state.messages.is_empty() {
            None
        } else {
            Some(conv_state.messages.join(" "))
        };
        (success, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pam_conv_state_take_password() {
        let mut state = PamConvState::new("secret");
        let pw = state.take_password();
        assert!(pw.is_some());
        assert_eq!(pw.unwrap().to_str().unwrap(), "secret");

        // Second call should return None
        assert!(state.take_password().is_none());
    }

    /// Empty and interior-NUL passwords must fail closed (no prompt reply).
    #[test]
    fn pam_conv_state_rejects_empty_and_nul() {
        assert!(PamConvState::new("").take_password().is_none());
        assert!(PamConvState::new("a\0b").take_password().is_none());
    }

    #[test]
    fn pam_messages_stay_within_cumulative_budget() {
        let mut state = PamConvState::new("secret");
        let first = std::ffi::CString::new("x".repeat(MAX_PAM_MESSAGE_BYTES)).unwrap();
        let second = std::ffi::CString::new("later").unwrap();
        state.push_message(&first);
        state.push_message(&second);
        assert_eq!(state.messages.join(" ").len(), MAX_PAM_MESSAGE_BYTES);
    }
}
