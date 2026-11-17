//! Windows backend: Credential Manager generic credentials, via
//! CredWriteW/CredReadW/CredDeleteW (advapi32, feature `Win32_Security_Credentials`).
//!
//! Credential Manager keys entries by a single TargetName, so the
//! `(service, account)` pair is folded into one name (`{service}:{account}`);
//! UserName still carries the account so entries stay readable in the
//! Control Panel UI. Secrets are stored as raw UTF-8 bytes in the credential
//! blob (no null terminator), symmetric with how they are read back.

use std::ptr;

use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    CREDENTIALW,
};

pub struct CredentialManager;

impl super::SecretStore for CredentialManager {
    fn name(&self) -> &'static str {
        "Windows Credential Manager"
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        let name_w = wide(&target_name(service, account));
        let account_w = wide(account);
        let blob = secret.as_bytes();

        let mut cred = CREDENTIALW::default();
        cred.Type = CRED_TYPE_GENERIC;
        cred.TargetName = PWSTR(name_w.as_mut_ptr());
        cred.Comment = PWSTR::null();
        cred.CredentialBlobSize = blob.len() as u32;
        cred.CredentialBlob = blob.as_ptr() as *mut u8;
        cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
        cred.UserName = PWSTR(account_w.as_mut_ptr());

        // CredWriteW atomically replaces an existing entry — no delete first.
        unsafe { CredWriteW(&cred, 0) }.map_err(|e| format!("CredWriteW failed: {e}"))
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        let name_w = wide(&target_name(service, account));
        let mut cred_ptr: *mut CREDENTIALW = ptr::null_mut();

        match unsafe { CredReadW(PCWSTR::from_raw(name_w.as_ptr()), CRED_TYPE_GENERIC, None, &mut cred_ptr) } {
            Err(e) if e.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(None),
            Err(e) => Err(format!("CredReadW failed: {e}")),
            Ok(()) => {
                // SAFETY: on success cred_ptr is a CREDENTIALW allocated by
                // CredReadW; CredentialBlob is CredentialBlobSize bytes of the
                // UTF-8 we wrote. CredFree releases both struct and blob.
                let cred = unsafe { &*cred_ptr };
                let bytes =
                    unsafe { std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize) };
                let secret = String::from_utf8_lossy(bytes).into_owned();
                unsafe { CredFree(cred_ptr as *const core::ffi::c_void) };
                Ok(Some(secret))
            }
        }
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        let name_w = wide(&target_name(service, account));
        match unsafe { CredDeleteW(PCWSTR::from_raw(name_w.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Err(e) if e.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
            other => other.map_err(|e| format!("CredDeleteW failed: {e}")),
        }
    }
}

/// Credential Manager lookup key: the (service, account) pair folded into one.
fn target_name(service: &str, account: &str) -> String {
    format!("{service}:{account}")
}

/// UTF-16 with a trailing null, for `PCWSTR`/`PWSTR` construction.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
