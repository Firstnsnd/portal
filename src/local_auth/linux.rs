//! Linux backend: PAM (`libpam`), service "login".
//!
//! The conversation callback hands the caller-collected password to PAM via
//! `appdata_ptr` (a `CString` owned by the enclosing `verify_password` call);
//! everything else in the "login" auth stack is `pam_unix`-style password
//! checking — account/session phases (nologin, securetty) are not part of
//! `pam_authenticate`. Hand-written FFI over libc: no `pam` crate dependency,
//! and `libpam` is present on every mainstream distribution.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

pub struct PamAuth;

const PAM_SUCCESS: c_int = 0;
const PAM_AUTH_ERR: c_int = 7;
const PAM_CRED_INSUFFICIENT: c_int = 8;
const PAM_AUTHINFO_UNAVAIL: c_int = 9;
const PAM_USER_UNKNOWN: c_int = 10;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_SILENT: c_int = 0x8000;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *mut c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: extern "C" fn(
        num_msg: c_int,
        msg: *mut *mut PamMessage,
        resp: *mut *mut PamResponse,
        appdata_ptr: *mut c_void,
    ) -> c_int,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
extern "C" {
    fn pam_start(
        service_name: *const c_char,
        user: *const c_char,
        conversation: *const PamConv,
        pamh: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, pam_status: c_int) -> c_int;
}

impl super::OwnerAuth for PamAuth {
    fn name(&self) -> &'static str {
        "Linux PAM (login)"
    }

    fn verify_password(&self, password: &str) -> Result<bool, String> {
        let user = current_username()?;
        let service = CString::new("login").expect("no interior NUL");
        let secret = CString::new(password.to_owned())
            .map_err(|_| "password contains interior NUL".to_string())?;

        let conv = PamConv {
            conv: password_conversation,
            appdata_ptr: secret.as_ptr() as *mut c_void,
        };

        let mut pamh: *mut c_void = ptr::null_mut();
        let rc = unsafe { pam_start(service.as_ptr(), user.as_ptr(), &conv, &mut pamh) };
        if rc != PAM_SUCCESS {
            return Err(format!("pam_start failed with code {rc}"));
        }

        let rc = unsafe { pam_authenticate(pamh, PAM_SILENT) };
        // pam_end must run whatever the outcome; pass rc back per convention.
        unsafe { pam_end(pamh, rc) };

        match rc {
            PAM_SUCCESS => Ok(true),
            // Authentication refused for identity/credential reasons: report
            // "wrong password" rather than a backend error.
            PAM_AUTH_ERR | PAM_CRED_INSUFFICIENT | PAM_AUTHINFO_UNAVAIL | PAM_USER_UNKNOWN => {
                Ok(false)
            }
            other => Err(format!("pam_authenticate failed with code {other}")),
        }
    }

    fn verify_system_prompt(&self, _reason: &str, _reply: super::OwnerReply) -> Result<(), String> {
        Err("Linux has no system owner prompt; use verify_password".to_string())
    }
}

/// PAM conversation: answer every hidden prompt with the password from
/// `appdata_ptr`; ignore informational messages (empty response slot).
extern "C" fn password_conversation(
    num_msg: c_int,
    msg: *mut *mut PamMessage,
    resp: *mut *mut PamResponse,
    appdata_ptr: *mut c_void,
) -> c_int {
    unsafe {
        if num_msg <= 0 || msg.is_null() || resp.is_null() {
            return PAM_CONV_ERR;
        }
        let n = num_msg as usize;
        let responses =
            libc::calloc(n, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
        if responses.is_null() {
            return PAM_CONV_ERR;
        }
        let secret = appdata_ptr as *const c_char;
        for i in 0..n {
            if (**msg.add(i)).msg_style == PAM_PROMPT_ECHO_OFF {
                let dup = libc::strdup(secret);
                if dup.is_null() {
                    free_responses(responses, n);
                    return PAM_CONV_ERR;
                }
                (*responses.add(i)).resp = dup;
            }
        }
        *resp = responses;
        PAM_SUCCESS
    }
}

/// Free a half-built response array the same way PAM frees it (per-string
/// `free` on non-null entries, then the array itself).
unsafe fn free_responses(responses: *mut PamResponse, n: usize) {
    for i in 0..n {
        let r = responses.add(i);
        if !(*r).resp.is_null() {
            libc::free((*r).resp as *mut c_void);
        }
    }
    libc::free(responses as *mut c_void);
}

/// Real login name of the current UID (getenv can be spoofed).
fn current_username() -> Result<CString, String> {
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0u8; 4096];
        let mut result: *mut libc::passwd = ptr::null_mut();
        let rc = libc::getpwuid_r(
            libc::getuid(),
            &mut pwd,
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() {
            return Err("failed to resolve current username".to_string());
        }
        Ok(CString::from(CStr::from_ptr(pwd.pw_name)))
    }
}
