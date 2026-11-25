//! Linux: everything goes through `xdg-open`, the default handler for URLs,
//! files and folders alike. `xdg-open`'s exit status is unreliable across
//! desktop environments (notably non-zero on success over ssh), so the
//! implementations fire it and only surface spawn failures.

use std::path::Path;

use super::Opener;

pub struct LinuxOpener;

impl Opener for LinuxOpener {
    fn name(&self) -> &'static str {
        "xdg-open"
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        spawn("xdg-open", &[url])
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        spawn_path("xdg-open", path)
    }

    fn open_folder(&self, dir: &Path) -> Result<(), String> {
        spawn_path("xdg-open", dir)
    }

    /// xdg-open has no "select" mode — reveal degrades to opening the parent
    /// directory (the file shows up unselected).
    fn reveal_path(&self, path: &Path) -> Result<(), String> {
        match reveal_target(path) {
            Some(parent) => spawn_path("xdg-open", parent),
            None => Err(format!(
                "no parent directory to open for {}",
                path.display()
            )),
        }
    }

    fn has_privacy_settings(&self) -> bool {
        false
    }

    fn open_privacy_settings(&self) -> Result<(), String> {
        Err("no authorization settings pane on this platform".to_string())
    }
}

/// The directory to open for a reveal: the file's parent (root has none).
fn reveal_target(path: &Path) -> Option<&Path> {
    path.parent().filter(|p| !p.as_os_str().is_empty())
}

fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {program}: {e}"))
}

fn spawn_path(program: &str, path: &Path) -> Result<(), String> {
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {program}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_targets_the_parent_directory() {
        assert_eq!(
            reveal_target(Path::new("/home/u/rec.cast")),
            Some(Path::new("/home/u"))
        );
        // Root and bare names have no meaningful parent to open.
        assert_eq!(reveal_target(Path::new("/")), None);
        assert_eq!(reveal_target(Path::new("rec.cast")), None);
        // Contract: no TCC-style pane on Linux.
        assert!(!LinuxOpener.has_privacy_settings());
    }
}
