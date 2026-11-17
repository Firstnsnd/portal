//! macOS backend: the `security` command-line tool.
//!
//! Deliberately not the `security-framework`/`keyring` crates: shelling out
//! avoids the macOS Keychain ACL issue where recompiling the app changes its
//! code signature and triggers a password prompt on every read.
//! - Store:  `security add-generic-password -A` (allow all apps, no prompt on rebuild)
//! - Read:   `security find-generic-password -g` (always accessible, no ACL check)
//! - Delete: `security delete-generic-password`

use std::process::Command;

pub struct KeychainCli;

impl super::SecretStore for KeychainCli {
    fn name(&self) -> &'static str {
        "macOS Keychain"
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        // Delete existing entry first (-U can't change ACL on existing items)
        let _ = Command::new("security")
            .args(["delete-generic-password", "-a", account, "-s", service])
            .output();

        // Add with -A (allow all applications to access without prompt)
        match Command::new("security")
            .args(["add-generic-password", "-a", account, "-s", service, "-w", secret, "-A"])
            .output()
        {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!(
                "security add-generic-password failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            )),
            Err(e) => Err(format!("failed to run security: {e}")),
        }
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        let output = match Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", account, "-g"])
            .output()
        {
            Ok(o) => o,
            Err(e) => return Err(format!("failed to run security: {e}")),
        };

        if !output.status.success() {
            return Ok(None);
        }

        // Password is on stderr in format:
        //   password: "the password"          (text)
        //   password: 0x2d2d2d2d2d42454...   (hex, for binary/multi-line content)
        //   On some macOS versions the hex is followed by the quoted string on the same line.
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("password: ") {
                if let Some(hex_str) = rest.strip_prefix("0x") {
                    // Hex-encoded binary content — take only hex chars (stop at first space/non-hex)
                    let hex_only: String = hex_str.chars()
                        .take_while(|c| c.is_ascii_hexdigit())
                        .collect();
                    if !hex_only.is_empty() && hex_only.len().is_multiple_of(2) {
                        let bytes: Vec<u8> = (0..hex_only.len())
                            .step_by(2)
                            .filter_map(|i| hex_only.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
                            .collect();
                        if bytes.len() == hex_only.len() / 2 {
                            return String::from_utf8(bytes)
                                .map(Some)
                                .map_err(|_| "keychain item is not valid UTF-8".to_string());
                        }
                    }
                } else if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
                    // Quoted string
                    return Ok(Some(rest[1..rest.len() - 1].to_string()));
                }
            }
        }
        Ok(None)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        // `security` exits non-zero when the entry is absent; that is success for us.
        let _ = Command::new("security")
            .args(["delete-generic-password", "-a", account, "-s", service])
            .output();
        Ok(())
    }
}
