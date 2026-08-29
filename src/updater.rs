//! Manual "check for updates" against GitHub Releases, and self-update via the `self_replace`
//! crate — it handles the Windows locked-executable dance correctly (you cannot overwrite a
//! running `.exe` directly; hand-rolling that swap is a good way to brick a technician's
//! install mid-job). Nothing here runs automatically: both steps are only ever triggered by an
//! explicit user action (the `u` key in the TUI / "Check for Updates" button on the web UI).

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

const REPO: &str = "duncanunderwood/netdx";
#[derive(Clone)]
pub struct UpdateInfo {
    pub latest_version: String,
    pub release_url: String,
    asset_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// The exact target-triple naming the release workflow packages assets under
/// (`netdx-<target>.tar.gz` / `.zip`) for the platform this binary is currently running on.
fn current_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// Parses a `vX.Y.Z` release tag into a comparable tuple. Anything that doesn't match that
/// shape parses as `None`, which callers treat as "not newer" — a malformed tag must never be
/// able to wrongly claim to be an available update.
fn parse_semver(tag: &str) -> Option<(u64, u64, u64)> {
    let s = tag.strip_prefix('v').unwrap_or(tag);
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

pub struct CheckResult {
    /// GitHub's actual latest release tag, whatever it is — even when it's not newer than the
    /// running build (e.g. this is a local/CI build ahead of the last release).
    pub latest_version: String,
    /// Only set when that release is actually newer and has a matching asset for this platform.
    pub update: Option<UpdateInfo>,
}

/// Fetches GitHub's latest release and reports it, regardless of whether it's newer than the
/// running build — callers decide what to do with `CheckResult::update` being `None` vs `Some`.
/// `Err` only on an outright lookup failure (network, unexpected response shape, unsupported
/// platform, or a newer release with no asset for this platform).
pub async fn check_for_update() -> Result<CheckResult, String> {
    let target = current_target().ok_or_else(|| "unsupported platform for auto-update".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("netdx/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .send()
        .await
        .map_err(|e| format!("couldn't reach GitHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub returned HTTP {}", resp.status()));
    }
    let release: Release = resp.json().await.map_err(|e| format!("unexpected response from GitHub: {e}"))?;

    let current = parse_semver(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0));
    let latest = parse_semver(&release.tag_name);
    if latest.map(|l| l <= current).unwrap_or(true) {
        return Ok(CheckResult { latest_version: release.tag_name, update: None });
    }

    let ext = if target.contains("windows") { "zip" } else { "tar.gz" };
    let asset_name = format!("netdx-{target}.{ext}");
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| format!("release {} has no asset for this platform ({asset_name})", release.tag_name))?;

    Ok(CheckResult {
        latest_version: release.tag_name.clone(),
        update: Some(UpdateInfo {
            latest_version: release.tag_name,
            release_url: release.html_url,
            asset_url: asset.browser_download_url.clone(),
        }),
    })
}

/// Downloads, extracts, swaps the new binary in over the running one, and relaunches. Never
/// returns on success — the new process takes over and this one exits. Returns `Err` if
/// anything before the point of no return (the `self_replace` call) fails, leaving the current
/// install completely untouched.
pub async fn install_and_relaunch(info: &UpdateInfo) -> Result<(), String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(120)).build().map_err(|e| e.to_string())?;
    let bytes = client
        .get(&info.asset_url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("download failed: {e}"))?;

    let tmp_dir = std::env::temp_dir().join(format!("netdx-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let new_binary_path = extract_binary(&bytes, &tmp_dir)?;

    // Point of no return: on success the file at `current_exe()` now contains the new binary.
    self_replace::self_replace(&new_binary_path).map_err(|e| format!("couldn't install update: {e}"))?;
    let _ = std::fs::remove_file(&new_binary_path);
    let _ = std::fs::remove_dir(&tmp_dir);

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::Command::new(&exe)
        .args(&args)
        .spawn()
        .map_err(|e| format!("update installed, but couldn't relaunch automatically — start netdx again: {e}"))?;

    std::process::exit(0);
}

#[cfg(windows)]
fn extract_binary(bytes: &[u8], tmp_dir: &std::path::Path) -> Result<PathBuf, String> {
    let out_path = tmp_dir.join("netdx.exe");
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| format!("bad update archive: {e}"))?;
    let mut file = zip.by_name("netdx.exe").map_err(|e| format!("update archive missing netdx.exe: {e}"))?;
    let mut out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
    std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
    Ok(out_path)
}

#[cfg(unix)]
fn extract_binary(bytes: &[u8], tmp_dir: &std::path::Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    let out_path = tmp_dir.join("netdx");
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut ar = tar::Archive::new(gz);
    ar.unpack(tmp_dir).map_err(|e| format!("bad update archive: {e}"))?;

    let mut perms = std::fs::metadata(&out_path).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&out_path, perms).map_err(|e| e.to_string())?;
    Ok(out_path)
}
