//! macOS: the `open` tool handles URLs, files and folders; `-R` reveals and
//! selects in Finder. TCC authorization panes deep-link via the
//! `x-apple.systempreferences:` URL scheme. `open` exits non-zero when the
//! target can't be handled, so statuses are checked.

use std::path::Path;

use super::Opener;

pub struct MacosOpener;

impl Opener for MacosOpener {
    fn name(&self) -> &'static str {
        "macos-open"
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        run(&["open", url])
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        run_with(&["open"], path)
    }

    fn open_folder(&self, dir: &Path) -> Result<(), String> {
        run_with(&["open"], dir)
    }

    fn reveal_path(&self, path: &Path) -> Result<(), String> {
        run_with(&["open", "-R"], path)
    }

    fn has_privacy_settings(&self) -> bool {
        true
    }

    fn open_privacy_settings(&self) -> Result<(), String> {
        // Full Disk Access pane — the pane the cwd-permission dialog points at.
        run(&[
            "open",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
        ])
    }
}

fn run(args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new(args[0])
        .args(&args[1..])
        .status()
        .map_err(|e| format!("failed to launch {}: {e}", args[0]))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} exited with {status}", args[0]))
    }
}

fn run_with(args: &[&str], path: &Path) -> Result<(), String> {
    let status = std::process::Command::new(args[0])
        .args(&args[1..])
        .arg(path)
        .status()
        .map_err(|e| format!("failed to launch {}: {e}", args[0]))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} exited with {status}", args[0]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_and_open_use_the_open_tool() {
        let it = MacosOpener;
        assert_eq!(it.name(), "macos-open");
        // Contract: macOS deep-links the TCC pane.
        assert!(it.has_privacy_settings());
    }
}
