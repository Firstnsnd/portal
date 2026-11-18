//! Windows: URLs and files go through `cmd /C start` (the empty argument is
//! the window title, not the target — without it a path with spaces breaks);
//! CREATE_NO_WINDOW keeps the cmd shim console from flashing. Folders and
//! reveal use `explorer`. `explorer` exits non-zero even on success, so its
//! calls are fire-and-forget: only spawn failures surface.

use std::os::windows::process::CommandExt;
use std::path::Path;

use super::Opener;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct WindowsOpener;

impl Opener for WindowsOpener {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        start(url)
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        start(&path.to_string_lossy())
    }

    fn open_folder(&self, dir: &Path) -> Result<(), String> {
        spawn("explorer", &dir.to_string_lossy())
    }

    /// Explorer's `/select,` verb opens the parent and selects the file.
    fn reveal_path(&self, path: &Path) -> Result<(), String> {
        spawn("explorer", &format!("/select,{}", path.display()))
    }

    /// Windows has no TCC-style per-folder authorization pane.
    fn has_privacy_settings(&self) -> bool {
        false
    }

    fn open_privacy_settings(&self) -> Result<(), String> {
        Err("no authorization settings pane on this platform".to_string())
    }
}

/// `cmd /C start "" <target>` — wait for the shim and check its status.
fn start(target: &str) -> Result<(), String> {
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", target])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("failed to launch: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("start exited with {status}"))
    }
}

fn spawn(program: &str, arg: &str) -> Result<(), String> {
    std::process::Command::new(program)
        .arg(arg)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {program}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_and_privacy_contract() {
        let it = WindowsOpener;
        assert_eq!(it.name(), "shell");
        // Contract: no TCC-style pane on Windows.
        assert!(!it.has_privacy_settings());
    }
}
