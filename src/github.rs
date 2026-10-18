//! Talking to GitHub without the API (and so without its hourly limit).
//!
//! `https://github.com/<repo>/releases/latest` redirects to `.../releases/tag/<tag>`, which tells
//! us the newest release. Download links are then built from the file names in `apps.json`:
//! `https://github.com/<repo>/releases/download/<tag>/<file>`.
//!
//! Development builds can point all of this at a local mock server with
//! `WAVEHUB_GITHUB_BASE=http://127.0.0.1:8000`.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// `https://github.com`, or the mock server set for tests / development builds.
pub fn base() -> String {
    if let Some(b) = BASE_OVERRIDE.get() {
        return b.clone();
    }
    #[cfg(debug_assertions)]
    if let Ok(b) = std::env::var("WAVEHUB_GITHUB_BASE") {
        return b.trim_end_matches('/').to_string();
    }
    "https://github.com".to_string()
}

static BASE_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[cfg(test)]
pub fn set_base_for_tests(base: &str) {
    let _ = BASE_OVERRIDE.set(base.to_string());
}

const USER_AGENT: &str = concat!("WaveHub/", env!("CARGO_PKG_VERSION"));

fn agent(follow_redirects: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .max_redirects(if follow_redirects { 10 } else { 0 })
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .build()
        .new_agent()
}

/// The newest stable release's tag, or `None` when the repo has no releases (or doesn't exist).
pub fn latest_tag(repo: &str) -> Result<Option<String>, String> {
    let url = format!("{}/{repo}/releases/latest", base());
    let (status, location) = call(false, &url)?;
    if status == 404 {
        return Ok(None);
    }
    if !(300..400).contains(&status) {
        return Err(format!("GitHub answered {status}"));
    }
    Ok(tag_from_location(&location))
}

/// One request that doesn't follow the redirect to a release's page or file, but does follow
/// GitHub's own "moved permanently" for a repo that was renamed or moved
/// (`github.com/owner/OldName/...` -> `github.com/owner/NewName/...`), so an app keeps working
/// in Wave Hub after its repo is renamed. Gives the status and the absolute `Location`.
fn call(head: bool, url: &str) -> Result<(u16, String), String> {
    let mut url = url.to_string();
    for _ in 0..4 {
        let a = agent(false);
        let resp = if head { a.head(&url).call() } else { a.get(&url).call() }.map_err(|e| format!("couldn't reach GitHub ({e})"))?;
        let status = resp.status().as_u16();
        let location = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default();
        // GitHub sends an absolute URL; accept a path-only one too.
        let location = if location.starts_with('/') { format!("https://github.com{location}") } else { location.replacen(&base(), "https://github.com", 1) };
        if !(matches!(status, 301 | 308) && location.starts_with("https://github.com/")) {
            return Ok((status, location));
        }
        url = location.replacen("https://github.com", &base(), 1);
    }
    Err("GitHub sent too many redirects".into())
}

/// `https://github.com/<owner>/<repo>/releases/tag/v1.2.3` -> `v1.2.3`, for any repo (a renamed
/// one answers with its new name). Anything else (GitHub sends repos without releases to the
/// release list) -> `None`.
pub fn tag_from_location(location: &str) -> Option<String> {
    let rest = location.strip_prefix("https://github.com/")?;
    let mut parts = rest.splitn(5, '/');
    let (owner, name) = (parts.next()?, parts.next()?);
    if owner.is_empty() || name.is_empty() || parts.next()? != "releases" || parts.next()? != "tag" {
        return None;
    }
    let tag = parts.next()?;
    let ok = !tag.is_empty() && tag.len() < 64 && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b));
    ok.then(|| tag.to_string())
}

pub fn download_url(repo: &str, tag: &str, file: &str) -> String {
    format!("{}/{repo}/releases/download/{tag}/{file}", base())
}

/// The release page that a person reads (always the real GitHub).
pub fn release_page(repo: &str, tag: &str) -> String {
    format!("https://github.com/{repo}/releases/tag/{tag}")
}

/// Whether a release has a file with this name, without downloading it. GitHub answers a
/// redirect to the file's storage for files that exist and 404 for ones that don't.
pub fn asset_exists(repo: &str, tag: &str, file: &str) -> Result<bool, String> {
    let url = download_url(repo, tag, file);
    match call(true, &url)?.0 {
        200..=399 => Ok(true),
        404 => Ok(false),
        s => Err(format!("GitHub answered {s} for {file}")),
    }
}

/// A small text file from a release (e.g. `SHA256SUMS.txt`). `None` when it isn't there.
pub fn get_text(url: &str) -> Result<Option<String>, String> {
    let mut resp = agent(true).get(url).call().map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    match resp.status().as_u16() {
        200 => resp.body_mut().with_config().limit(1024 * 1024).read_to_string().map(Some).map_err(|e| e.to_string()),
        404 => Ok(None),
        s => Err(format!("GitHub answered {s} for {url}")),
    }
}

/// `sha256  name` lines -> (hash, name).
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?.to_ascii_lowercase();
            let name = parts.next()?.trim_start_matches('*').to_string();
            (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then_some((hash, name))
        })
        .collect()
}

/// Downloads `url` into `dest`, calling `progress(done, total)` as it goes. Returns the file's
/// SHA-256 (lowercase hex).
pub fn download(url: &str, dest: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<String, String> {
    let mut resp = agent(true).get(url).call().map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    match resp.status().as_u16() {
        200 => {}
        404 => return Err(format!("This release has no file called {}.", url.rsplit('/').next().unwrap_or(url))),
        s => return Err(format!("GitHub answered {s} for the download")),
    }
    let total = resp.body().content_length().unwrap_or(0);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(dest).map_err(|e| format!("couldn't create {} ({e})", dest.display()))?;
    let mut reader = resp.body_mut().with_config().limit(4 * 1024 * 1024 * 1024).reader();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    // Development builds only: WAVEHUB_SLOW_KBPS=2000 caps the speed, to watch the progress UI.
    #[cfg(debug_assertions)]
    let slow: Option<f64> = std::env::var("WAVEHUB_SLOW_KBPS").ok().and_then(|v| v.parse().ok());
    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(dest);
            return Err("Cancelled.".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("download interrupted ({e})"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| format!("couldn't write the download ({e})"))?;
        done += n as u64;
        #[cfg(debug_assertions)]
        if let Some(kbps) = slow {
            let due = done as f64 / (kbps * 1000.0);
            let ahead = due - started.elapsed().as_secs_f64();
            if ahead > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(ahead));
            }
        }
        progress(done, total);
    }
    file.sync_all().map_err(|e| e.to_string())?;
    if total > 0 && done != total {
        return Err("The download was cut short. Please try again.".into());
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares dotted version numbers (`v0.2.3.5`, `0.9.6`). Non-numeric parts compare as 0.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse().unwrap_or(0)).collect()
    }
    let (a, b) = (parts(latest), parts(installed));
    let n = a.len().max(b.len());
    for i in 0..n {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_versions() {
        assert_eq!(tag_from_location("https://github.com/postmanpat-303/BigWaveRadio/releases/tag/v0.9.6").as_deref(), Some("v0.9.6"));
        assert_eq!(tag_from_location("https://github.com/postmanpat-303/BigWaveRadio/releases"), None);
        // After a repo rename GitHub answers with the new name.
        assert_eq!(tag_from_location("https://github.com/postmanpat-303/ChipWaveStudio/releases/tag/v0.3.0").as_deref(), Some("v0.3.0"));
        assert_eq!(tag_from_location("https://github.com/postmanpat-303/ChipWaveStudio/releases/latest"), None);
        assert!(is_newer("v0.2.3.5", "0.2.3"));
        assert!(!is_newer("0.9.6", "v0.9.6"));
        assert!(is_newer("0.10.0", "0.9.9"));
        let sums = parse_sums("ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789  a.zip\nbad line\n");
        assert_eq!(sums.len(), 1);
        assert_eq!(sums[0].1, "a.zip");
    }
}
