//! Password check over Linux-PAM, the way `swaylock` and `hyprlock` do
//! it: `pam_start` on a service, a conversation that answers the
//! password to every hidden prompt, `pam_authenticate`, `pam_acct_mgmt`.
//! A direct binding to `libpam` (as `libc::statvfs` is used elsewhere),
//! the six calls the locker needs and nothing more.
//!
//! The service is `[locker] pam_service`, or by default `aria-shell`
//! when `/etc/pam.d/aria-shell` exists (`assets/pam.d/aria-shell` is
//! the file to install), else `login`, present on every distribution
//! and what `swaylock`'s own file includes; `other` denies everything,
//! so a made-up name does too (the UI scenario counts on it: a real
//! service would record the failure in `pam_faillock`'s tally and lock
//! the developer's account for ten minutes after three runs).
//! `pam_unix` reads the shadow file through the setuid `unix_chkpwd`
//! helper, so this works from the user's own process.
//!
//! Blocking: a failed attempt sleeps for `pam_faildelay` (~2s). Call it
//! off the UI thread.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: Option<
        unsafe extern "C" fn(
            c_int,
            *mut *const PamMessage,
            *mut *mut PamResponse,
            *mut c_void,
        ) -> c_int,
    >,
    appdata_ptr: *mut c_void,
}

/// Opaque `pam_handle_t`.
#[repr(C)]
struct PamHandle {
    _private: [u8; 0],
}

const PAM_SUCCESS: c_int = 0;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        handle: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(handle: *mut PamHandle, status: c_int) -> c_int;
    fn pam_strerror(handle: *mut PamHandle, errnum: c_int) -> *const c_char;
}

/// What the conversation works with: the password to answer, and the
/// texts PAM sent (`pam_faillock` says why an account is locked there,
/// as a `PAM_TEXT_INFO` and only without `PAM_SILENT`; `pam_strerror`
/// only says "Authentication failure").
struct Conversation {
    password: CString,
    messages: Vec<String>,
}

/// The conversation: every hidden prompt gets a copy of the password.
/// The responses are freed by PAM, so they come from `calloc`/`strdup`.
/// A visible prompt (a second factor we can't ask for) fails the
/// conversation.
unsafe extern "C" fn converse(
    count: c_int,
    messages: *mut *const PamMessage,
    responses: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    if count <= 0 || messages.is_null() || responses.is_null() || appdata.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: PAM hands `count` valid message pointers; `appdata` is the
    // `Conversation` alive for the whole `authenticate` call, and PAM
    // calls this from that very thread.
    unsafe {
        let conversation = &mut *(appdata as *mut Conversation);
        let password = conversation.password.as_ptr();
        let out = libc::calloc(count as usize, size_of::<PamResponse>()) as *mut PamResponse;
        if out.is_null() {
            return PAM_CONV_ERR;
        }
        for i in 0..count as usize {
            let message = &**messages.add(i);
            let text = if message.msg.is_null() {
                String::new()
            } else {
                CStr::from_ptr(message.msg).to_string_lossy().into_owned()
            };
            match message.msg_style {
                PAM_PROMPT_ECHO_OFF => {
                    (*out.add(i)).resp = libc::strdup(password);
                }
                PAM_TEXT_INFO | PAM_ERROR_MSG => {
                    log::info!("pam: {}", text.trim_end());
                    conversation.messages.push(text.trim_end().to_owned());
                }
                // PAM_PROMPT_ECHO_ON, or a style we don't know.
                _ => {
                    log::warn!("pam: unsupported prompt {:?}", text.trim_end());
                    for j in 0..i {
                        libc::free((*out.add(j)).resp as *mut c_void);
                    }
                    libc::free(out as *mut c_void);
                    return PAM_CONV_ERR;
                }
            }
        }
        *responses = out;
    }
    PAM_SUCCESS
}

/// The PAM service to start when the config doesn't say: ours when its
/// file is installed, else `login`.
pub fn default_service() -> &'static str {
    if Path::new("/etc/pam.d/aria-shell").exists() {
        "aria-shell"
    } else {
        "login"
    }
}

/// Check `password` for `user` with `service`; `Err` carries what PAM
/// said: its error messages when it sent some ("The account is locked
/// due to 3 failed logins"), else `pam_strerror`'s ("Authentication
/// failure"). The password's memory is zeroed after the call.
pub fn authenticate(service: &str, user: &str, password: String) -> Result<(), String> {
    let c_service = CString::new(service).map_err(|e| e.to_string())?;
    let c_user = CString::new(user).map_err(|e| e.to_string())?;
    let password = CString::new(password).map_err(|_| "password with a NUL byte".to_owned())?;
    let mut conversation = Conversation {
        password,
        messages: Vec::new(),
    };
    let conv = PamConv {
        conv: Some(converse),
        appdata_ptr: &mut conversation as *mut Conversation as *mut c_void,
    };
    let mut handle: *mut PamHandle = ptr::null_mut();
    // SAFETY: plain calls into libpam with valid C strings, the handle
    // checked before use and ended once.
    let result = unsafe {
        let started = pam_start(c_service.as_ptr(), c_user.as_ptr(), &conv, &mut handle);
        if started != PAM_SUCCESS || handle.is_null() {
            Err(format!("pam_start({service}) failed: {started}"))
        } else {
            // Not PAM_SILENT: that would keep `pam_faillock` from
            // saying the account is locked.
            let mut status = pam_authenticate(handle, 0);
            if status == PAM_SUCCESS {
                status = pam_acct_mgmt(handle, 0);
            }
            let outcome = if status == PAM_SUCCESS {
                Ok(())
            } else {
                Err(CStr::from_ptr(pam_strerror(handle, status))
                    .to_string_lossy()
                    .into_owned())
            };
            pam_end(handle, status);
            outcome
        }
    };
    let result = match result {
        Err(e) if !conversation.messages.is_empty() => {
            log::warn!("pam ({service}): {e}");
            Err(conversation.messages.join(" "))
        }
        other => other,
    };
    let mut bytes = conversation.password.into_bytes_with_nul();
    bytes.fill(0);
    // Keep the write: the buffer is dropped right after.
    std::hint::black_box(&bytes);
    log::debug!("pam ({service}): {result:?}");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the system's PAM: `cargo test -- --ignored pam`. On a
    /// service that doesn't exist, so `other` (deny) answers: a real
    /// one would count the failure against the developer's account
    /// (`pam_faillock`: three of them lock it for ten minutes).
    #[test]
    #[ignore]
    fn wrong_password_is_refused() {
        let user = std::env::var("USER").unwrap();
        let result = authenticate(
            "aria-shell-no-such-service",
            &user,
            "not-the-password".to_owned(),
        );
        assert!(result.is_err(), "{result:?}");
    }
}
