//! # Update Checking
//!
//! Checks the GitHub Releases API for a newer Portal version and (on the
//! user's approval) downloads + reveals the matching installer. All network
//! I/O is blocking (`ureq`) and must be run off the UI thread — the caller
//! spawns a `std::thread`. Version comparison, JSON parsing, and asset
//! selection are pure and unit-tested without touching the network.

use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::io::{Read, Write};

pub const GITHUB_API_LATEST: &str = "https://api.github.com/repos/Firstnsnd/portal/releases/latest";

/// One downloadable asset from a GitHub release.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    /// GitHub's SHA-256 digest for the uploaded asset (`sha256:<hex>`).
    /// Updates are rejected when GitHub does not provide this value.
    #[serde(default)]
    pub digest: Option<String>,
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
        digest: String,
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
        "windows" => {
            // consts::ARCH reports ARM64 Windows as "aarch64" and the release
            // assets use that spelling directly — no remap like the macos
            // "arm64". The portable zip exists too; updates point at the
            // installer so a stock install gets the full upgrade path.
            let prefix = format!("Portal-windows-{arch}-v");
            assets
                .iter()
                .find(|a| a.name.starts_with(&prefix) && a.name.ends_with("-setup.exe"))
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
    let body =
        match request(GITHUB_API_LATEST).and_then(|r| r.into_string().map_err(|e| e.to_string())) {
            Ok(b) => b,
            Err(e) => return UpdateState::Error(e),
        };
    let release = match parse_release_json(&body) {
        Some(r) => r,
        None => return UpdateState::Error("malformed release JSON".to_string()),
    };
    update_state_from_release(current_version, &release)
}

/// Pure, fetch-free update decision for a parsed release: `Available` only
/// when a strictly newer version exists AND its installer asset carries
/// GitHub's SHA-256 digest. Integrity verification is mandatory — an asset
/// without a digest is never offered, never downloaded, never revealed.
fn update_state_from_release(current_version: &str, release: &LatestRelease) -> UpdateState {
    let Some(version) = normalize_tag(&release.tag_name) else {
        return UpdateState::Idle;
    };
    if !is_newer(current_version, &version) {
        return UpdateState::Idle;
    }
    let Some(asset) = select_asset(&release.assets) else {
        return UpdateState::Idle;
    };
    let Some(digest) = asset
        .digest
        .as_deref()
        // Must parse as `sha256:<hex>` — anything malformed counts as
        // missing, so verification can never be skipped downstream.
        .filter(|d| parse_sha256_digest(d).is_ok())
    else {
        return UpdateState::Idle;
    };
    UpdateState::Available {
        version,
        url: asset.url.clone(),
        asset_name: asset.name.clone(),
        digest: digest.to_string(),
    }
}

/// Stream `url` to `dest` on disk, invoking `on_progress(received, total)` as
/// chunks arrive. `total` comes from the `Content-Length` header when present.
pub fn download_to_path(
    url: &str,
    dest: &std::path::Path,
    expected_digest: &str,
    on_progress: impl Fn(u64, Option<u64>),
) -> Result<(), String> {
    // Fail fast on a malformed digest — no point downloading an artifact we
    // could never verify. (The authoritative compare happens in
    // `verify_digest` once the bytes are in hand.)
    parse_sha256_digest(expected_digest)?;
    let resp = request(url)?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = dest.with_extension(format!("download-{}.tmp", uuid::Uuid::new_v4()));
    let file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::new(file);
    let mut hasher = Sha256::new();

    let mut reader = resp.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        received += n as u64;
        on_progress(received, total);
    }
    writer.flush().map_err(|e| e.to_string())?;
    let actual = hasher.finalize();
    if let Err(e) = verify_digest(expected_digest, actual.as_slice()) {
        // Never install (let alone reveal) an artifact that doesn't match
        // the release's published digest.
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    replace_file_atomically(&temp, dest)?;
    Ok(())
}

/// Compare the streamed artifact's SHA-256 against the release asset's
/// published `sha256:<hex>` digest. `Ok` only on an exact match.
fn verify_digest(expected_digest: &str, actual: &[u8]) -> Result<(), String> {
    let expected = parse_sha256_digest(expected_digest)?;
    if expected.as_slice() != actual {
        return Err("downloaded update failed SHA-256 verification".to_string());
    }
    Ok(())
}

fn parse_sha256_digest(value: &str) -> Result<Vec<u8>, String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("release asset has no valid SHA-256 digest".to_string());
    }
    (0..64)
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| "invalid SHA-256 digest".to_string())
        })
        .collect()
}

fn replace_file_atomically(temp: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if std::fs::rename(temp, dest).is_err() {
        // Windows cannot replace an existing file with rename. Remove only the
        // exact destination, then retry; Unix keeps the atomic fast path above.
        if dest.exists() {
            std::fs::remove_file(dest).map_err(|e| e.to_string())?;
            std::fs::rename(temp, dest).map_err(|e| e.to_string())
        } else {
            Err(format!(
                "failed to install downloaded update: {}",
                dest.display()
            ))
        }
    } else {
        Ok(())
    }
}

/// Reveal a downloaded asset. Archives (`.tar.gz`) are extracted next to the
/// download and the resulting directory is opened; single-file installers
/// (`.dmg`, `setup.exe`) are handed straight to the OS default handler —
/// the NSIS installer's own UAC prompt covers elevation on Windows.
/// Returns the revealed path.
pub fn reveal_downloaded(
    asset_name: &str,
    archive_path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    if asset_name.ends_with(".tar.gz") {
        let downloads_dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
        let dir_name = asset_name.trim_end_matches(".tar.gz").to_string();
        let extracted = downloads_dir.join(&dir_name);

        let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive
            .entries()
            .map_err(|e| format!("failed to read archive: {e}"))?
        {
            let mut entry = entry.map_err(|e| format!("failed to read archive entry: {e}"))?;
            let entry_path = entry
                .path()
                .map_err(|e| format!("invalid archive path: {e}"))?
                .into_owned();
            if entry_path.is_absolute()
                || entry_path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err("update archive contains an unsafe path".to_string());
            }
            if entry.header().entry_type().is_symlink()
                || entry.header().entry_type().is_hard_link()
            {
                return Err("update archive contains a link entry".to_string());
            }
            let target = downloads_dir.join(&entry_path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            entry
                .unpack(&target)
                .map_err(|e| format!("failed to extract archive: {e}"))?;
        }

        let target = if extracted.is_dir() {
            extracted
        } else {
            downloads_dir
        };
        crate::opener::open_folder(&target)?;
        Ok(target)
    } else {
        crate::opener::open_path(archive_path)?;
        Ok(archive_path.to_path_buf())
    }
}

/// Open an already-revealed path again (used by the "Open" button after a
/// download completes). Returns the path on success.
pub fn open_revealed(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    crate::opener::open_path(path)?;
    Ok(path.to_path_buf())
}

/// Download `url` (for `version`) into the Downloads dir and reveal it.
pub fn download_and_reveal(
    version: &str,
    asset_name: &str,
    url: &str,
    digest: &str,
    on_progress: impl Fn(u64, Option<u64>),
) -> Result<std::path::PathBuf, String> {
    let downloads_dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    let dest = downloads_dir.join(asset_name);
    download_to_path(url, &dest, digest, on_progress)?;
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
        ReleaseAsset {
            name: name.to_string(),
            url: format!("https://example.com/{name}"),
            digest: Some(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
            ),
        }
    }

    fn sample_assets() -> Vec<ReleaseAsset> {
        vec![
            asset("Portal-macos-x86_64.dmg"),
            asset("Portal-macos-arm64.dmg"),
            asset("Portal-linux-x86_64-v0.15.0.tar.gz"),
            asset("Portal-windows-x86_64-v0.15.0.zip"),
            asset("Portal-windows-x86_64-v0.15.0-setup.exe"),
            asset("Portal-windows-aarch64-v0.15.0-setup.exe"),
        ]
    }

    #[test]
    fn select_asset_for_picks_exact_platform() {
        let assets = sample_assets();
        assert_eq!(
            select_asset_for("macos", "aarch64", &assets).map(|a| a.name.as_str()),
            Some("Portal-macos-arm64.dmg")
        );
        assert_eq!(
            select_asset_for("macos", "x86_64", &assets).map(|a| a.name.as_str()),
            Some("Portal-macos-x86_64.dmg")
        );
        assert_eq!(
            select_asset_for("linux", "x86_64", &assets).map(|a| a.name.as_str()),
            Some("Portal-linux-x86_64-v0.15.0.tar.gz")
        );
        // Windows: the setup.exe over the portable zip, per arch.
        assert_eq!(
            select_asset_for("windows", "x86_64", &assets).map(|a| a.name.as_str()),
            Some("Portal-windows-x86_64-v0.15.0-setup.exe")
        );
        assert_eq!(
            select_asset_for("windows", "aarch64", &assets).map(|a| a.name.as_str()),
            Some("Portal-windows-aarch64-v0.15.0-setup.exe")
        );
        // No setup.exe for this arch (zip alone doesn't count).
        assert!(select_asset_for("windows", "x86_64", &assets[3..4]).is_none());
        // missing platform asset: only a Linux tarball present → no macOS match
        assert!(select_asset_for("macos", "x86_64", &assets[2..3]).is_none());
    }

    fn release_with_digest(digest: Option<&str>) -> LatestRelease {
        let mut a = asset("Portal-macos-arm64.dmg");
        a.digest = digest.map(str::to_string);
        LatestRelease {
            tag_name: "v0.20.0".to_string(),
            assets: vec![a],
        }
    }

    #[test]
    fn update_requires_digest_on_the_asset() {
        // Integrity verification is mandatory: without GitHub's digest the
        // newer release is NOT offered (Idle), never downloaded.
        let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let with = release_with_digest(Some(&format!("sha256:{hex}")));
        match update_state_from_release("0.19.8", &with) {
            UpdateState::Available { digest, .. } => {
                assert_eq!(digest, format!("sha256:{hex}"))
            }
            other => panic!("expected Available, got {other:?}"),
        }
        for digest in [None, Some(""), Some("not-a-digest")] {
            let state = update_state_from_release("0.19.8", &release_with_digest(digest));
            assert!(
                matches!(state, UpdateState::Idle),
                "digest {digest:?} must not produce Available"
            );
        }
        // Not newer / malformed tag → Idle regardless of digest.
        assert!(matches!(
            update_state_from_release("0.20.0", &release_with_digest(Some("sha256:ab"))),
            UpdateState::Idle
        ));
        // A valid digest on an up-to-date version stays Idle too.
        assert!(matches!(
            update_state_from_release(
                "0.20.0",
                &release_with_digest(Some(&format!("sha256:{hex}")))
            ),
            UpdateState::Idle
        ));
    }

    #[test]
    fn parse_sha256_digest_accepts_only_valid_hex() {
        let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(parse_sha256_digest(hex).is_ok());
        assert!(parse_sha256_digest(&format!("sha256:{hex}")).is_ok());
        // Wrong length, non-hex, empty.
        assert!(parse_sha256_digest("abcd").is_err());
        assert!(parse_sha256_digest(&hex[..63]).is_err());
        assert!(parse_sha256_digest(&format!("sha256:{}", "g".repeat(64))).is_err());
        assert!(parse_sha256_digest("").is_err());
    }

    #[test]
    fn verify_digest_rejects_any_mismatch() {
        // `actual` is the streamed artifact's digest (32 raw bytes).
        let empty_hash: [u8; 32] = Sha256::digest(b"").into();
        let empty_hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(verify_digest(empty_hex, &empty_hash), Ok(()));
        assert_eq!(
            verify_digest(&format!("sha256:{empty_hex}"), &empty_hash),
            Ok(())
        );
        // One byte off → rejected with the verification error.
        let mut bad = empty_hash;
        bad[0] ^= 0xff;
        let err = verify_digest(empty_hex, &bad).unwrap_err();
        assert!(err.contains("SHA-256"));
        // Unparseable expected digest → rejected (nothing passes).
        assert!(verify_digest("", &empty_hash).is_err());
    }

    #[test]
    fn replace_file_atomically_installs_or_keeps_old() {
        let dir = std::env::temp_dir().join(format!("portal-update-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("installer.dmg");
        // Fresh install.
        let temp = dir.join("t1");
        std::fs::write(&temp, b"new").unwrap();
        replace_file_atomically(&temp, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!temp.exists());
        // Replacement over an existing destination.
        let temp = dir.join("t2");
        std::fs::write(&temp, b"newer").unwrap();
        replace_file_atomically(&temp, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"newer");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
