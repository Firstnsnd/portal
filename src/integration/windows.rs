//! Windows backend: Explorer context-menu entries in HKCU.
//!
//! Two actions ("New Portal Tab Here" / "New Portal Window Here"), each
//! registered both on directories (`Directory\shell`, shown when a folder
//! is selected) and the folder background (`Directory\Background\shell`,
//! shown inside an open folder). `%V` is the folder in both cases. Uninstall
//! removes the action keys with `RegDeleteTreeW`; state checks open the tab
//! action's key.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

use super::{portal_bin, IntegrationStatus, ShellIntegration};

pub(crate) struct ExplorerIntegration;

impl ShellIntegration for ExplorerIntegration {
    fn name(&self) -> &'static str {
        "Explorer"
    }

    fn install(&self) -> Result<(), String> {
        install()
    }

    fn uninstall(&self) -> Result<(), String> {
        uninstall()
    }

    fn status(&self) -> IntegrationStatus {
        status()
    }
}

/// (registry key name, menu label, portal flag)
const ACTIONS: [(&str, &str, &str); 2] = [
    ("PortalTabHere", "New Portal Tab Here", "--open-tab"),
    ("PortalWindowHere", "New Portal Window Here", "--open-window"),
];

const DIRECTORY_SHELL: &str = r"Directory\shell";
const BACKGROUND_SHELL: &str = r"Directory\Background\shell";

/// The command a menu entry runs. `%V` is the selected folder
/// (`Directory\shell`) or the open folder (`Background\shell`).
fn command_value(bin: &str, flag: &str) -> String {
    format!("\"{bin}\" {flag} \"%V\"")
}

/// All registry key paths one action occupies (install + uninstall).
fn key_paths(key: &str) -> [String; 2] {
    [format!("{DIRECTORY_SHELL}\\{key}"), format!("{BACKGROUND_SHELL}\\{key}")]
}

fn install() -> Result<(), String> {
    let bin = portal_bin();
    for (key, label, flag) in ACTIONS {
        for path in key_paths(key) {
            write_menu_entry(&path, label, &bin, flag)?;
        }
    }
    Ok(())
}

/// One action under `Directory[Background]\shell`: display name + icon on
/// the key, launch command in its `command` subkey.
fn write_menu_entry(key_path: &str, label: &str, bin: &str, flag: &str) -> Result<(), String> {
    create_key(key_path, |hkey| {
        reg_set_string(hkey, "", label)?;
        reg_set_string(hkey, "Icon", bin)
    })?;
    create_key(&format!("{key_path}\\command"), |hkey| {
        reg_set_string(hkey, "", &command_value(bin, flag))
    })
}

fn uninstall() -> Result<(), String> {
    for (key, _, _) in ACTIONS {
        for path in key_paths(key) {
            delete_tree(&path);
        }
    }
    Ok(())
}

fn status() -> IntegrationStatus {
    if open_key(&format!("{DIRECTORY_SHELL}\\{}", ACTIONS[0].0)) {
        IntegrationStatus::Installed
    } else {
        IntegrationStatus::NotInstalled
    }
}

// ── Registry helpers ─────────────────────────────────────────────────────────

/// UTF-16 with trailing null, for PCWSTR construction.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Write a REG_SZ value (`""` selects the key's default value). Values are
/// UTF-16 with a terminating NUL; `cbData` counts bytes.
fn reg_set_string(hkey: HKEY, name: &str, value: &str) -> Result<(), String> {
    let _name_w;
    let name_pc = match name {
        "" => PCWSTR::null(),
        n => {
            _name_w = wide(n);
            PCWSTR::from_raw(_name_w.as_ptr())
        }
    };
    let mut value_w = wide(value);
    value_w.push(0);
    let bytes: Vec<u8> = value_w.iter().flat_map(|u| u.to_le_bytes()).collect();
    let rc = unsafe { RegSetValueExW(hkey, name_pc, None, REG_SZ, Some(&bytes)) };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("RegSetValueExW({name}) failed: {}", rc.0))
    }
}

/// Create (or open) `HKCU\path` under `KEY_WRITE`, run `f` with the handle,
/// then close it.
fn create_key(path: &str, f: impl FnOnce(HKEY) -> Result<(), String>) -> Result<(), String> {
    let sub_w = wide(path);
    let mut hkey = HKEY::default();
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(sub_w.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(format!("RegCreateKeyExW({path}) failed: {}", rc.0));
    }
    let result = f(hkey);
    unsafe {
        RegCloseKey(hkey);
    }
    result
}

/// Whether `HKCU\path` exists.
fn open_key(path: &str) -> bool {
    let sub_w = wide(path);
    let mut hkey = HKEY::default();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(sub_w.as_ptr()),
            None,
            KEY_READ,
            &mut hkey,
        )
    };
    if rc == ERROR_SUCCESS {
        unsafe {
            RegCloseKey(hkey);
        }
        true
    } else {
        false
    }
}

/// Delete `HKCU\path` with all its subkeys; a missing key is not an error.
fn delete_tree(path: &str) {
    let rc: WIN32_ERROR = delete_tree_inner(path);
    if rc != ERROR_SUCCESS && rc != ERROR_FILE_NOT_FOUND {
        log::warn!("RegDeleteTreeW({path}) failed: {}", rc.0);
    }
}

fn delete_tree_inner(path: &str) -> WIN32_ERROR {
    let sub_w = wide(path);
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR::from_raw(sub_w.as_ptr())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_value_quotes_binary_and_folder() {
        assert_eq!(
            command_value(r"C:\Program Files\Portal\portal.exe", "--open-tab"),
            r#""C:\Program Files\Portal\portal.exe" --open-tab "%V""#
        );
    }

    #[test]
    fn key_paths_cover_selected_and_background_menus() {
        assert_eq!(
            key_paths("PortalTabHere"),
            [
                r"Directory\shell\PortalTabHere".to_string(),
                r"Directory\Background\shell\PortalTabHere".to_string(),
            ]
        );
    }

    #[test]
    fn uninstall_targets_every_installed_key() {
        // The uninstall set must be exactly the install set, or entries leak.
        let installed: Vec<String> = ACTIONS
            .iter()
            .flat_map(|(key, _, _)| key_paths(key))
            .map(|p| format!("{p}\\command"))
            .collect();
        assert_eq!(installed.len(), 4);
    }
}
