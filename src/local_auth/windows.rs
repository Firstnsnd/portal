//! Windows backend: `LogonUserW` (advapi32; in windows 0.62 it sits in
//! `Win32::Security` directly — covered by the existing `Win32_Security`
//! feature, no separate Logon feature exists).
//!
//! Verifies the caller-collected password by performing an interactive logon
//! as the current user (`USERNAME`/`USERDOMAIN`); since XP this needs no
//! special privilege for `LOGON32_LOGON_INTERACTIVE`. `ERROR_LOGON_FAILURE`
//! (wrong user/password pair) maps to `Ok(false)`; every other failure is a
//! real error whose message the dialog shows. There is no biometric owner
//! prompt on Windows here — the account password is the check.

use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_LOGON_FAILURE, HANDLE};
use windows::Win32::Security::{LogonUserW, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT};

pub struct Win32Logon;

impl super::OwnerAuth for Win32Logon {
    fn name(&self) -> &'static str {
        "Windows LogonUserW"
    }

    fn verify_password(&self, password: &str) -> Result<bool, String> {
        let user = std::env::var("USERNAME").map_err(|_| "USERNAME not set".to_string())?;
        // USERDOMAIN is the machine name for local accounts; "." also targets
        // the local SAM, and serves when the variable is missing.
        let domain = std::env::var("USERDOMAIN").unwrap_or_else(|_| ".".to_string());

        let user_w = wide(&user);
        let domain_w = wide(&domain);
        let password_w = wide(password);

        let mut token = HANDLE::default();
        let result = unsafe {
            LogonUserW(
                PCWSTR::from_raw(user_w.as_ptr()),
                PCWSTR::from_raw(domain_w.as_ptr()),
                PCWSTR::from_raw(password_w.as_ptr()),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
        };
        match result {
            Ok(()) => {
                // We only needed the verdict, not the token.
                unsafe {
                    let _ = CloseHandle(token);
                };
                Ok(true)
            }
            Err(e) if e.code() == HRESULT::from_win32(ERROR_LOGON_FAILURE.0) => Ok(false),
            Err(e) => Err(format!("LogonUserW failed: {e}")),
        }
    }

    fn verify_system_prompt(&self, _reason: &str, _reply: super::OwnerReply) -> Result<(), String> {
        Err("Windows has no system owner prompt; use verify_password".to_string())
    }
}

/// UTF-16 with a trailing null, for `PCWSTR` construction.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
