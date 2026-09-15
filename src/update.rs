//! # Update Checking
//!
//! Checks the GitHub Releases API for a newer Portal version and (on the
//! user's approval) downloads + reveals the matching installer. All network
//! I/O is blocking (`ureq`) and must be run off the UI thread — the caller
//! spawns a `std::thread`. Version comparison, JSON parsing, and asset
//! selection are pure and unit-tested without touching the network.

use std::cmp::Ordering;
use std::io::{Read, Write};

pub const GITHUB_API_LATEST: &str = "https://api.github.com/repos/Firstnsnd/portal/releases/latest";

/// One downloadable asset from a GitHub release.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

/// The `releases/latest` response, trimmed to what we consume.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LatestRelease {
    pub tag_name: String,
    pub assets: Vec<ReleaseAsset>,
}

/// App-level update lifecycle state, driven from the UI and advanced by
/// background threads through a shared handle.
#[derive(Debug, Clone, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    Available {
        version: String,
        url: String,
        asset_name: String,
    },
    Downloading {
        version: String,
        asset_name: String,
        received: u64,
        total: Option<u64>,
    },
    Ready {
        version: String,
        path: std::path::PathBuf,
    },
    Error(String),
}

// ────────────────────────────────────────────────────────────────────────
// Pure helpers (unit-tested, no I/O)
// ────────────────────────────────────────────────────────────────────────

/// Parse a `releases/latest` JSON body. `None` on malformed input.
pub fn parse_release_json(body: &str) -> Option<LatestRelease> {
    serde_json::from_str::<LatestRelease>(body).ok()
}

/// Strip the optional leading `v` from a tag. `"v0.14.10"` → `Some("0.14.10")`,
/// `"0.14.10"` → `Some("0.14.10")`, `""`/`"v"` → `None`.
pub fn normalize_tag(tag: &str) -> Option<String> {
    let t = tag.strip_prefix('v').unwrap_or(tag);
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Numeric dotted-segment version comparison. Missing segments are 0,
/// non-numeric trailing segments (e.g. `rc1`) are ignored, and empty /
/// non-numeric versions collapse to `[0]`. Never lexicographic, so
/// `"10.0" > "9.0"`.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let segs = |v: &str| -> Vec<u64> {
        v.split('.')
            // Drop a trailing prerelease like "10-rc1" → "10".
            .map(|s| s.split('-').next().unwrap_or(s))
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let a = segs(a);
    let b = segs(b);
    let len = a.len().max(b.len());
    for i in 0..len {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        match av.cmp(&bv) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// True when `latest` is strictly newer than `current`, comparing each after
/// `normalize_tag`. Prereleases compare equal to their base version and so
/// are never treated as upgrades.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (normalize_tag(current), normalize_tag(latest)) {
        (Some(c), Some(l)) => compare_versions(&l, &c) == Ordering::Greater,
        _ => false,
    }
}

/// Select the installer asset that matches the given OS/arch strings.
/// Mirrors `.github/workflows/release.yml` naming. Pure so it is testable on
/// any host regardless of `cfg!`.
pub fn select_asset_for<'a>(
    os: &str,
    arch: &str,
    assets: &'a [ReleaseAsset],
) -> Option<&'a ReleaseAsset> {
    match os {
        "macos" => {
            // Rust's consts::ARCH reports Apple Silicon as "aarch64", but the
            // release assets are labelled "arm64" (matrix name in release.yml).
            let asset_arch = if arch == "aarch64" { "arm64" } else { arch };
            let want = format!("Portal-macos-{asset_arch}.dmg");
            assets.iter().find(|a| a.name == want)
        }
        "linux" => {
            let prefix = format!("Portal-linux-{arch}-v");
            assets
                .iter()
                .find(|a| a.name.starts_with(&prefix) && a.name.ends_with(".tar.gz"))
        }
        _ => None,
    }
}

/// Select the asset for the current platform.
pub fn select_asset(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset> {
    select_asset_for(std::env::consts::OS, std::env::consts::ARCH, assets)
}

// ────────────────────────────────────────────────────────────────────────
// Network / I/O (blocking; call from a background thread)
// ────────────────────────────────────────────────────────────────────────

fn request(url: &str) -> Result<ureq::Response, String> {
    ureq::get(url)
        .set("User-Agent", concat!("Portal/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("request failed: {e}"))
}

/// Fetch the latest release and, relative to `current_version`, produce the
/// next state: `Available` when a newer version has a matching artifact,
/// `Idle` when up-to-date or no artifact matches, `Error` on any failure.
pub fn check_latest(current_version: &str) -> UpdateState {
    let body = match request(GITHUB_API_LATEST).and_then(|r| r.into_string().map_err(|e| e.to_string())) {
        Ok(b) => b,
        Err(e) => return UpdateState::Error(e),
    };
    let release = match parse_release_json(&body) {
        Some(r) => r,
        None => return UpdateState::Error("malformed release JSON".to_string()),
    };
    let version = match normalize_tag(&release.tag_name) {
        Some(v) => v,
        None => return UpdateState::Idle,
    };
    if !is_newer(current_version, &version) {
        return UpdateState::Idle;
    }
    match select_asset(&release.assets) {
        Some(asset) => UpdateState::Available {
            version,
            url: asset.url.clone(),
            asset_name: asset.name.clone(),
        },
        None => UpdateState::Idle,
    }
}

/// Stream `url` to `dest` on disk, invoking `on_progress(received, total)` as
/// chunks arrive. `total` comes from the `Content-Length` header when present.
pub fn download_to_path(
    url: &str,
    dest: &std::path::Path,
    on_progress: impl Fn(u64, Option<u64>),
) -> Result<(), String> {
    let resp = request(url)?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::new(file);

    let mut reader = resp.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        on_progress(received, total);
    }
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// Reveal a downloaded installer. macOS: open the `.dmg`. Linux: extract the
/// `.tar.gz` and open the resulting directory. Returns the revealed path.
#[cfg(target_os = "macos")]
pub fn reveal_downloaded(
    _asset_name: &str,
    archive_path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    std::process::Command::new("open")
        .arg(archive_path)
        .status()
        .map_err(|e| format!("failed to open installer: {e}"))?;
    Ok(archive_path.to_path_buf())
}

/// See `reveal_downloaded` on macOS above.
#[cfg(not(target_os = "macos"))]
pub fn reveal_downloaded(
    asset_name: &str,
    archive_path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let downloads_dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    let dir_name = asset_name.trim_end_matches(".tar.gz").to_string();
    let extracted = downloads_dir.join(&dir_name);

    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(&downloads_dir)
        .status()
        .map_err(|e| format!("failed to extract archive: {e}"))?;
    if !status.success() {
        return Err("archive extraction failed".to_string());
    }

    let target = if extracted.is_dir() {
        extracted
    } else {
        downloads_dir
    };
    let status = std::process::Command::new("xdg-open")
        .arg(&target)
        .status()
        .map_err(|e| format!("failed to open folder: {e}"))?;
    if !status.success() {
        return Err("failed to open folder".to_string());
    }
    Ok(target)
}

/// Open an already-revealed path again (used by the "Open" button after a
/// download completes). Returns the path on success.
pub fn open_revealed(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(not(target_os = "macos"))]
    let mut cmd = std::process::Command::new("xdg-open");
    let status = cmd.arg(path).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("failed to open folder".to_string());
    }
    Ok(path.to_path_buf())
}

/// Download `url` (for `version`) into the Downloads dir and reveal it.
pub fn download_and_reveal(
    version: &str,
    asset_name: &str,
    url: &str,
    on_progress: impl Fn(u64, Option<u64>),
) -> Result<std::path::PathBuf, String> {
    let downloads_dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    let dest = downloads_dir.join(asset_name);
    download_to_path(url, &dest, on_progress)?;
    let path = reveal_downloaded(asset_name, &dest)?;
    let _ = version; // documented for callers; not used in naming
    Ok(path)
}

// ────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_v_and_rejects_empty() {
        assert_eq!(normalize_tag("v0.14.10").as_deref(), Some("0.14.10"));
        assert_eq!(normalize_tag("0.14.10").as_deref(), Some("0.14.10"));
        assert_eq!(normalize_tag(""), None);
        assert_eq!(normalize_tag("v"), None);
    }

    #[test]
    fn compare_versions_is_numeric() {
        assert_eq!(compare_versions("0.14.10", "0.14.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.9.9", "0.10.0"), Ordering::Less);
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("10.0", "9.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.14.10-rc1", "0.14.10"), Ordering::Equal);
        assert_eq!(compare_versions("abc", "1.0.0"), Ordering::Less);
        assert_eq!(compare_versions("", "1.0.0"), Ordering::Less);
    }

    #[test]
    fn is_newer_compares_correctly() {
        assert!(is_newer("0.14.10", "0.15.0"));
        assert!(!is_newer("0.14.10", "0.14.10"));
        assert!(!is_newer("0.14.10", "0.14.9"));
        assert!(!is_newer("0.14.10", "0.14.10-rc1"));
        // leading v's on either side
        assert!(is_newer("v0.14.10", "v0.15.0"));
    }

    #[test]
    fn parse_release_json_roundtrips() {
        let body = r#"{"tag_name":"v0.15.0","assets":[{"name":"Portal-macos-arm64.dmg","browser_download_url":"https://example.com/a.dmg"}]}"#;
        let release = parse_release_json(body).expect("valid JSON");
        assert_eq!(release.tag_name, "v0.15.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "Portal-macos-arm64.dmg");
        assert_eq!(release.assets[0].url, "https://example.com/a.dmg");
        assert!(parse_release_json("{").is_none());
    }

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset { name: name.to_string(), url: format!("https://example.com/{name}") }
    }

    fn sample_assets() -> Vec<ReleaseAsset> {
        vec![
            asset("Portal-macos-x86_64.dmg"),
            asset("Portal-macos-arm64.dmg"),
            asset("Portal-linux-x86_64-v0.15.0.tar.gz"),
        ]
    }

    #[test]
    fn select_asset_for_picks_exact_platform() {
        let assets = sample_assets();
        assert_eq!(select_asset_for("macos", "aarch64", &assets).map(|a| a.name.as_str()), Some("Portal-macos-arm64.dmg"));
        assert_eq!(select_asset_for("macos", "x86_64", &assets).map(|a| a.name.as_str()), Some("Portal-macos-x86_64.dmg"));
        assert_eq!(select_asset_for("linux", "x86_64", &assets).map(|a| a.name.as_str()), Some("Portal-linux-x86_64-v0.15.0.tar.gz"));
        assert!(select_asset_for("windows", "x86_64", &assets).is_none());
        // missing platform asset: only a Linux tarball present → no macOS match
        assert!(select_asset_for("macos", "x86_64", &assets[2..3]).is_none());
    }
}
