//! macOS backend: LocalAuthentication's `LAContext`.
//!
//! `evaluatePolicy:localizedReason:reply:` with
//! `kLAPolicyDeviceOwnerAuthentication` (= 2) shows the system sheet — Touch
//! ID first, account-password fallback built in — so no app-drawn password
//! path is needed on macOS. The API is asynchronous: the reply block fires on
//! an arbitrary queue, which is why
//! [`super::OwnerAuth::verify_system_prompt`] hands the result to a callback
//! instead of returning it. Raw values are verified against the SDK header
//! (`LAPublicDefines.h`), not invented: policy 2 = device-owner auth.
//!
//! Uses `objc` 0.2 (already a dependency for the terminal view) plus the
//! `block` crate it pairs with for reply-block construction.

use objc::msg_send;
use objc::runtime::{Class, Object};
use objc::sel;
use objc::sel_impl;

use super::OwnerReply;

pub struct LocalAuthentication;

/// `kLAPolicyDeviceOwnerAuthentication` (LAPublicDefines.h).
const POLICY_DEVICE_OWNER_AUTHENTICATION: usize = 2;

impl super::OwnerAuth for LocalAuthentication {
    fn name(&self) -> &'static str {
        "macOS LocalAuthentication"
    }

    fn verify_password(&self, _password: &str) -> Result<bool, String> {
        // macOS collects the secret itself (Touch ID / system password sheet);
        // there is no app-drawn password path to verify against.
        Err("macOS verifies via the system prompt, not a typed password".to_string())
    }

    fn verify_system_prompt(&self, reason: &str, reply: OwnerReply) -> Result<(), String> {
        unsafe {
            let ctx_cls = Class::get("LAContext").ok_or("LAContext class not found")?;
            let ctx: *mut Object = msg_send![ctx_cls, new];

            // The real selector is canEvaluatePolicy:error: — the two-part
            // form. Calling the one-part canEvaluatePolicy: is an
            // unrecognized selector → NSInvalidArgumentException → foreign
            // exception → abort (verified by probe, 2026-11-21). Null
            // NSError** out-param: we don't want the error object.
            let can: bool = msg_send![
                ctx,
                canEvaluatePolicy: POLICY_DEVICE_OWNER_AUTHENTICATION
                error: std::ptr::null_mut::<*mut Object>()
            ];
            if !can {
                // DeviceOwnerAuthentication always has the password fallback,
                // so this should never fire; log and let evaluatePolicy err.
                log::warn!("owner auth: canEvaluatePolicy(DeviceOwnerAuthentication) = false");
            }

            let ns_reason = nsstring(reason)?;

            // ConcreteBlock + copy() gives a heap (Rc) block; LAContext copies
            // it again, so it survives both this frame and our RcBlock's drop.
            // Blocks must be Fn, so the FnOnce reply rides in a RefCell<Option>
            // and is taken on the (single) invocation.
            let reply_cell = std::cell::RefCell::new(Some(reply));
            let blk =
                block::ConcreteBlock::new(move |success: libc::c_schar, _error: *mut Object| {
                    if let Some(reply) = reply_cell.borrow_mut().take() {
                        reply(success != 0);
                    }
                })
                .copy();

            // NOT the `options:` variant: that one demands the reason inside
            // the options dictionary (LAContextLocalizedReason) and raises
            // NSInvalidArgumentException when it is missing — which surfaces
            // in Rust as a foreign exception and aborts the process (crash
            // log: ___rust_foreign_exception). The classic localizedReason
            // variant takes the string directly and is not deprecated.
            let () = msg_send![
                ctx,
                evaluatePolicy: POLICY_DEVICE_OWNER_AUTHENTICATION
                localizedReason: ns_reason
                reply: &*blk
            ];

            let _: () = msg_send![ctx, release];
            let _: () = msg_send![ns_reason, release];
            Ok(())
        }
    }
}

/// `NSString` from UTF-8, caller-owned (released by the constructor above).
unsafe fn nsstring(s: &str) -> Result<*mut Object, String> {
    let cls = Class::get("NSString").ok_or("NSString class not found")?;
    let alloc: *mut Object = msg_send![cls, alloc];
    let obj: *mut Object = msg_send![
        alloc,
        initWithUTF8String: s.as_ptr() as *const libc::c_char
    ];
    if obj.is_null() {
        Err("initWithUTF8String returned nil".to_string())
    } else {
        Ok(obj)
    }
}
