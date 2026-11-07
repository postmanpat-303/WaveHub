//! Which of Wave Hub's own release files this computer should get, and whether to ask about one.
//!
//! A newer *stable* release is noticed in the background. The window then asks, showing the
//! installed version and the one that's available. Later remembers that version. Nothing is
//! downloaded, installed or restarted until the user says Update, and even then Wave Hub only
//! downloads the file and checks it against `SHA256SUMS.txt` — the same check the app installs
//! use. Package-manager, Flatpak, setup, portable, AppImage and Mac copies are left where they
//! are; the message says how that copy is meant to be updated.
//!
//! From 0.2.0 Wave Hub ships in the same formats, with the same names, as BigWave Radio:
//!
//! | how it's installed      | file                                         |
//! |-------------------------|----------------------------------------------|
//! | Windows setup           | `WaveHub-<v>-setup-x64.exe`                  |
//! | Windows portable        | `WaveHub-<v>-windows-x64.zip`                |
//! | Linux AppImage          | `WaveHub-<v>-linux-<arch>.AppImage`          |
//! | Debian / Ubuntu         | `WaveHub-<v>-linux-<arch>.deb`               |
//! | Fedora / openSUSE       | `WaveHub-<v>-linux-<arch>.rpm`               |
//! | Arch / CachyOS          | `WaveHub-<v>-linux-<arch>.pkg.tar.zst`       |
//! | Linux tar.gz            | `WaveHub-<v>-linux-<arch>.tar.gz`            |
//! | macOS                   | `WaveHub-<v>-macos-universal.dmg` (or .zip)  |
//!
//! 0.1.0 shipped bare programs, `WaveHub.exe` and `WaveHub-linux-x86_64`. Each list below ends
//! with those, so a release that only has the old names still gets an answer, and Wave Hub
//! picks the first name the release really has.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use crate::github;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    WindowsSetup,
    WindowsPortable,
    AppImage,
    Deb,
    Rpm,
    Arch,
    Tarball,
    /// A bare program file (how 0.1.0 shipped).
    Bare,
    MacOs,
    /// Running inside Flatpak (`FLATPAK_ID` is set). Updated with Flatpak, not by replacing files.
    Flatpak,
}

impl InstallKind {
    pub fn label(self) -> &'static str {
        match self {
            InstallKind::WindowsSetup => "Windows setup",
            InstallKind::WindowsPortable => "Windows portable",
            InstallKind::AppImage => "AppImage",
            InstallKind::Deb => "Debian/Ubuntu package",
            InstallKind::Rpm => "RPM package",
            InstallKind::Arch => "Arch package",
            InstallKind::Tarball => "tar.gz",
            InstallKind::Bare => "single program file",
            InstallKind::MacOs => "macOS app",
            InstallKind::Flatpak => "Flatpak",
        }
    }
}

/// How the running Wave Hub was installed, from where it runs.
pub fn detect() -> InstallKind {
    // Flatpak sets this for the app it launched. It wins over the path guess below.
    if std::env::var_os("FLATPAK_ID").is_some() {
        return InstallKind::Flatpak;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(windows) {
        detect_windows(&exe)
    } else if cfg!(target_os = "macos") {
        InstallKind::MacOs
    } else {
        detect_linux(&exe, std::env::var_os("APPIMAGE").is_some(), Path::new("/"))
    }
}

fn detect_windows(exe: &Path) -> InstallKind {
    // The setup puts an uninstaller next to the program; the zip doesn't.
    let dir = exe.parent().unwrap_or(Path::new("."));
    if dir.join("Uninstall.exe").is_file() || dir.join("Uninstall Wave Hub.exe").is_file() {
        InstallKind::WindowsSetup
    } else {
        InstallKind::WindowsPortable
    }
}

/// `root` is `/` except in tests.
fn detect_linux(exe: &Path, in_appimage: bool, root: &Path) -> InstallKind {
    if in_appimage {
        return InstallKind::AppImage;
    }
    let s = exe.to_string_lossy();
    if s.starts_with("/usr/") || s.starts_with("/opt/") {
        if root.join("var/lib/dpkg/info/wave-hub.list").is_file() {
            return InstallKind::Deb;
        }
        let pacman = root.join("var/lib/pacman/local");
        if std::fs::read_dir(&pacman).is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("wave-hub"))) {
            return InstallKind::Arch;
        }
        if root.join("var/lib/rpm").is_dir() || root.join("usr/lib/sysimage/rpm").is_dir() {
            return InstallKind::Rpm;
        }
        // Installed by hand under /usr: guess from the distribution.
        let os = std::fs::read_to_string(root.join("etc/os-release")).unwrap_or_default().to_ascii_lowercase();
        let like = os.lines().filter(|l| l.starts_with("id=") || l.starts_with("id_like=")).collect::<Vec<_>>().join(" ");
        return if like.contains("arch") {
            InstallKind::Arch
        } else if like.contains("fedora") || like.contains("rhel") || like.contains("suse") {
            InstallKind::Rpm
        } else if like.contains("debian") || like.contains("ubuntu") {
            InstallKind::Deb
        } else {
            InstallKind::Tarball
        };
    }
    // The tar.gz has install.sh next to the program, and install.sh copies it to ~/.local/bin.
    let dir = exe.parent().unwrap_or(Path::new("."));
    let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if dir.join("install.sh").is_file() || (name == "wave-hub" && s.contains("/.local/bin/")) {
        InstallKind::Tarball
    } else {
        InstallKind::Bare
    }
}

/// Release files to try, best first, for `version` (without the `v`).
pub fn candidates(kind: InstallKind, version: &str, arch: &str) -> Vec<String> {
    let v = version.trim_start_matches('v');
    let linux = |ext: &str| format!("WaveHub-{v}-linux-{arch}.{ext}");
    let mut out: Vec<String> = match kind {
        InstallKind::WindowsSetup => vec![format!("WaveHub-{v}-setup-x64.exe"), format!("WaveHub-{v}-windows-x64.zip")],
        InstallKind::WindowsPortable => vec![format!("WaveHub-{v}-windows-x64.zip"), format!("WaveHub-{v}-setup-x64.exe")],
        InstallKind::MacOs => vec![format!("WaveHub-{v}-macos-universal.dmg"), format!("WaveHub-{v}-macos-universal.zip")],
        InstallKind::AppImage | InstallKind::Bare => vec![linux("AppImage"), linux("tar.gz")],
        InstallKind::Deb => vec![linux("deb"), linux("AppImage")],
        InstallKind::Rpm => vec![linux("rpm"), linux("AppImage")],
        InstallKind::Arch => vec![linux("pkg.tar.zst"), linux("AppImage")],
        InstallKind::Tarball => vec![linux("tar.gz"), linux("AppImage")],
        InstallKind::Flatpak => vec![format!("WaveHub-{v}-linux-{arch}.flatpak")],
    };
    // 0.1.0's names. A Flatpak build was never published under those, so it isn't offered one.
    match kind {
        InstallKind::WindowsSetup | InstallKind::WindowsPortable => out.push("WaveHub.exe".into()),
        InstallKind::MacOs | InstallKind::Flatpak => {}
        _ if arch == "x86_64" => out.push("WaveHub-linux-x86_64".into()),
        _ => {}
    }
    out
}

/// The first of `candidates` that release `tag` of `repo` has. `Ok(None)`: none of them.
pub fn pick(repo: &str, tag: &str, candidates: &[String]) -> Result<Option<String>, String> {
    for name in candidates {
        if github::asset_exists(repo, tag, name)? {
            return Ok(Some(name.clone()));
        }
    }
    Ok(None)
}

/// The file this computer should download for release `tag`.
pub fn asset_for_this_computer(repo: &str, tag: &str) -> Result<Option<String>, String> {
    pick(repo, tag, &candidates(detect(), tag, std::env::consts::ARCH))
}

/// `v1.2.3` -> `1.2.3` for the sentences in the window.
pub fn display_version(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

/// How the newest tag compares with the version compiled into this copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StableCompare {
    /// A stable release, and its numbers are higher.
    Newer,
    /// A stable release we can read, and it isn't higher. Includes the same version.
    NotNewer,
    /// A pre-release, a nightly, or a tag that isn't a version. Never offered.
    NotStable,
}

/// `latest` is a newer stable release than `installed`.
pub fn is_newer_stable(latest: &str, installed: &str) -> bool {
    compare_stable(latest, installed) == StableCompare::Newer
}

pub fn compare_stable(latest: &str, installed: &str) -> StableCompare {
    let Some(latest_n) = numeric_prefix(latest) else {
        return StableCompare::NotStable;
    };
    let Some(installed_n) = numeric_prefix(installed) else {
        return StableCompare::NotNewer;
    };
    match cmp_nums(&latest_n, &installed_n) {
        Ordering::Greater => StableCompare::Newer,
        _ => StableCompare::NotNewer,
    }
}

/// Whether the window should ask about `latest`.
///
/// `dismissed` is the tag Last chosen with Later (empty if none). A Later hides that version
/// and anything older; a higher stable version asks again. `manual` is Check for updates,
/// which asks again for the same version.
pub fn should_ask(latest: &str, installed: &str, dismissed: &str, manual: bool) -> bool {
    if !is_newer_stable(latest, installed) {
        return false;
    }
    if manual || dismissed.trim().is_empty() {
        return true;
    }
    let Some(latest_n) = numeric_prefix(latest) else {
        return false;
    };
    match numeric_prefix(dismissed) {
        Some(d) => cmp_nums(&latest_n, &d) == Ordering::Greater,
        // A stored value we can't read doesn't block a real update.
        None => true,
    }
}

/// Leading dotted numbers of a stable tag. `None` for a pre-release or anything else.
///
/// `1.2.3`, `v1.2.3` and `1.2.3+build` are stable. `1.2.3-rc.1`, `1.2.3-beta`, `rc1` and
/// `nightly` are not. A Debian-style `-1` suffix is ignored, so `1.2.3-1` compares as `1.2.3`
/// and is not treated as newer than `1.2.3`.
fn numeric_prefix(tag: &str) -> Option<Vec<u64>> {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    let prerelease = lower.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| {
        let w = w.trim_matches(|c: char| c.is_ascii_digit());
        matches!(w, "alpha" | "beta" | "rc" | "pre" | "preview" | "nightly" | "dev" | "snapshot" | "test")
    });
    if prerelease {
        return None;
    }
    let head = t.split(|c: char| c != '.' && !c.is_ascii_digit()).next().unwrap_or("");
    if head.is_empty() || head.starts_with('.') || head.ends_with('.') || head.contains("..") {
        return None;
    }
    head.split('.').map(|p| p.parse().ok()).collect()
}

fn cmp_nums(a: &[u64], b: &[u64]) -> Ordering {
    let n = a.len().max(b.len());
    for i in 0..n {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    Ordering::Equal
}

/// What the background check found, before the window decides whether it may change the prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    Offer(String),
    /// Up to date, dismissed, pre-release, or no releases. No prompt.
    Quiet,
    /// Offline or GitHub answered badly. The About page says so; no prompt.
    Unreachable,
}

/// Where the self-update question is. Repeated checks leave a download, a finished file and an
/// open question alone, so the window doesn't ask again on every check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Hidden,
    Asking { tag: String },
    Working { tag: String, file: String, stage: WorkStage, done: u64, total: u64 },
    Ready { tag: String, file: String, path: PathBuf, note: String },
    Failed { tag: String, message: String },
}

impl Default for UpdateState {
    fn default() -> Self {
        UpdateState::Hidden
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkStage {
    /// Fetching `SHA256SUMS.txt` before the file itself.
    CheckingSums,
    Downloading,
    Verifying,
}

/// The release file this computer would get, once we know the tag is a stable update.
#[derive(Clone, Debug)]
pub struct HubOffer {
    pub tag: String,
    pub kind: InstallKind,
    pub asset: Result<Option<String>, String>,
}

pub fn fold_check(current: UpdateState, outcome: CheckOutcome, manual: bool) -> UpdateState {
    // A download in progress is the user's current work. A check must not restart the question.
    if matches!(current, UpdateState::Working { .. }) {
        return current;
    }
    if matches!(current, UpdateState::Ready { .. }) && !manual {
        return current;
    }
    match outcome {
        CheckOutcome::Offer(tag) => match &current {
            // The same failure, noticed again by the startup check, stays a failure.
            UpdateState::Failed { tag: shown, .. } if shown == &tag && !manual => current,
            // The file is already downloaded. Checking again doesn't throw that away.
            UpdateState::Ready { .. } => current,
            _ => UpdateState::Asking { tag },
        },
        CheckOutcome::Quiet => match current {
            UpdateState::Failed { .. } if !manual => current,
            UpdateState::Ready { .. } => current,
            _ => UpdateState::Hidden,
        },
        // A failed lookup doesn't dismiss a question that's already on screen.
        CheckOutcome::Unreachable => match current {
            UpdateState::Asking { .. } | UpdateState::Failed { .. } | UpdateState::Ready { .. } => current,
            _ => UpdateState::Hidden,
        },
    }
}

pub fn tag_of(state: &UpdateState) -> Option<&str> {
    match state {
        UpdateState::Hidden => None,
        UpdateState::Asking { tag } | UpdateState::Working { tag, .. } | UpdateState::Ready { tag, .. } | UpdateState::Failed { tag, .. } => Some(tag),
    }
}

/// The tag Later should remember, when the question can be dismissed. A download keeps going.
pub fn dismiss_tag(state: &UpdateState) -> Option<String> {
    match state {
        UpdateState::Asking { tag } | UpdateState::Ready { tag, .. } | UpdateState::Failed { tag, .. } => Some(tag.clone()),
        UpdateState::Hidden | UpdateState::Working { .. } => None,
    }
}

pub fn begin_download(state: UpdateState, file: &str) -> UpdateState {
    match state {
        UpdateState::Asking { tag } | UpdateState::Failed { tag, .. } => UpdateState::Working { tag, file: file.to_string(), stage: WorkStage::CheckingSums, done: 0, total: 0 },
        other => other,
    }
}

pub fn note_progress(state: UpdateState, done: u64, total: u64) -> UpdateState {
    match state {
        UpdateState::Working { tag, file, stage: WorkStage::CheckingSums | WorkStage::Downloading, .. } => UpdateState::Working { tag, file, stage: WorkStage::Downloading, done, total },
        other => other,
    }
}

pub fn mark_verifying(state: UpdateState) -> UpdateState {
    match state {
        UpdateState::Working { tag, file, done, total, .. } => UpdateState::Working { tag, file, stage: WorkStage::Verifying, done, total },
        other => other,
    }
}

pub fn mark_ready(state: UpdateState, path: PathBuf, note: String) -> UpdateState {
    match state {
        UpdateState::Working { tag, file, .. } => UpdateState::Ready { tag, file, path, note },
        other => other,
    }
}

pub fn mark_failed(state: UpdateState, message: String) -> UpdateState {
    let Some(tag) = tag_of(&state).map(str::to_string) else {
        return state;
    };
    UpdateState::Failed { tag, message }
}

/// Stop a download and return to the question. The version is not dismissed.
pub fn cancel_download(state: UpdateState) -> UpdateState {
    match state {
        UpdateState::Working { tag, .. } => UpdateState::Asking { tag },
        other => other,
    }
}

/// What Update will do, said before anything is downloaded.
pub fn before_you_update(kind: InstallKind) -> &'static str {
    match kind {
        InstallKind::WindowsSetup => "This copy was installed with the Windows setup. Update downloads that setup and checks its checksum. Wave Hub stays open; open the file yourself when you want to install it. Windows may ask for an administrator password.",
        InstallKind::WindowsPortable => "This copy is the portable zip. Update downloads the new zip and checks its checksum. Windows keeps this program locked while it's open, so close Wave Hub when you're finished, then unzip the file over this folder. Your settings stay.",
        InstallKind::AppImage => "This copy is an AppImage. Update downloads the new AppImage and checks its checksum. Swap the file in after you close Wave Hub. Your settings stay.",
        InstallKind::Deb => "This copy was installed with a Debian or Ubuntu package. Update can download a new .deb and check it. Change the installed package with apt.",
        InstallKind::Rpm => "This copy was installed with an RPM. Update can download a new .rpm and check it. Change the installed package with your package manager.",
        InstallKind::Arch => "This copy was installed with an Arch package. Update can download a new package and check it. Change the installed package with pacman.",
        InstallKind::Tarball => "This copy was installed from the tar.gz. Update downloads a new archive and checks it. Unpack it and run install.sh when you're ready. Your settings stay.",
        InstallKind::Bare => "This copy is a single program file. Update downloads a replacement and checks it. Close Wave Hub when you're finished, then put the new file in its place. Your settings stay.",
        InstallKind::MacOs => "This copy is the macOS app. Update downloads the disk image or zip and checks it. Open it and drag Wave Hub into Applications. Wave Hub stays open until you quit.",
        InstallKind::Flatpak => "This copy is a Flatpak. Update it with Flatpak. Wave Hub can still download the Flatpak file and check it, and leaves the installed copy alone.",
    }
}

/// What to do with a file that has already been checked. Wave Hub does not launch it.
pub fn handoff(kind: InstallKind, file: &str) -> String {
    let text: &str = match kind {
        InstallKind::WindowsSetup => "Downloaded and checked. Open the setup when you want to install it. Windows may ask for an administrator password. Wave Hub stays open, and your settings stay.",
        InstallKind::WindowsPortable => "Downloaded and checked. Close Wave Hub when you're finished, then unzip this over the folder you run it from. Your settings stay.",
        InstallKind::AppImage => "Downloaded and checked. Close Wave Hub when you're finished, then replace your AppImage with this file. Your settings stay.",
        InstallKind::Deb => "Downloaded and checked. This copy stays as it is. Install the .deb with apt, or your usual package installer, when you want to update.",
        InstallKind::Rpm => "Downloaded and checked. This copy stays as it is. Install the .rpm with your package manager when you want to update.",
        InstallKind::Arch => "Downloaded and checked. This copy stays as it is. Install it with pacman when you want to update.",
        InstallKind::Tarball => "Downloaded and checked. Unpack it and run install.sh when you're ready. Your settings stay.",
        InstallKind::Bare => "Downloaded and checked. Close Wave Hub when you're finished, then replace the program file with this one. Your settings stay.",
        InstallKind::MacOs if file.to_ascii_lowercase().ends_with(".zip") => "Downloaded and checked. Open the zip and move Wave Hub into Applications. This copy stays until you do.",
        InstallKind::MacOs => "Downloaded and checked. Open the disk image and drag Wave Hub into Applications. This copy stays until you do.",
        InstallKind::Flatpak => "Downloaded and checked. This Flatpak copy stays as it is. Update it with Flatpak, or install this file yourself.",
    };
    text.to_string()
}

pub fn no_file_message(kind: InstallKind) -> String {
    format!("This release has no download for a {} install. Nothing was downloaded.", kind.label())
}

/// The hash `SHA256SUMS.txt` (or `SHA256SUMS`) publishes for `file`.
/// Missing list or a missing line is an error: Wave Hub's own update is never installed unchecked.
pub fn expected_hash(sums_text: Option<&str>, file: &str) -> Result<String, String> {
    let Some(text) = sums_text else {
        return Err("This release has no SHA256SUMS file, so Wave Hub won't install it.".into());
    };
    github::parse_sums(text).into_iter().find(|(_, name)| name == file).map(|(hash, _)| hash).ok_or_else(|| format!("The checksum list doesn't include {file}, so it can't be checked. Nothing was installed."))
}

fn plain_file_name(file: &str) -> Result<&str, String> {
    if file.is_empty() || file.contains(['/', '\\', '\0']) || file.contains("..") {
        return Err("The release file name isn't one Wave Hub can save.".into());
    }
    Ok(file)
}

/// Progress from [`fetch_verified`], in order: checksum list, the file, then the comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchEvent {
    CheckingSums,
    Progress { done: u64, total: u64 },
    Verifying,
}

/// Reads the checksum list, downloads `file` into `dest_dir`, and keeps it only when the hash matches.
pub fn fetch_verified(repo: &str, tag: &str, file: &str, dest_dir: &Path, cancel: &AtomicBool, report: &mut dyn FnMut(FetchEvent)) -> Result<PathBuf, String> {
    let file = plain_file_name(file)?;
    if cancel.load(AtomicOrdering::Relaxed) {
        return Err("Cancelled.".into());
    }
    report(FetchEvent::CheckingSums);
    let expected = checksum_for(repo, tag, file)?;
    if cancel.load(AtomicOrdering::Relaxed) {
        return Err("Cancelled.".into());
    }
    let part = dest_dir.join(format!("{file}.part"));
    let actual = github::download(&github::download_url(repo, tag, file), &part, cancel, &mut |done, total| report(FetchEvent::Progress { done, total }))?;
    report(FetchEvent::Verifying);
    accept_download(&part, &dest_dir.join(file), &expected, &actual)
}

fn checksum_for(repo: &str, tag: &str, file: &str) -> Result<String, String> {
    for sums in ["SHA256SUMS.txt", "SHA256SUMS"] {
        if let Some(text) = github::get_text(&github::download_url(repo, tag, sums))? {
            return expected_hash(Some(&text), file);
        }
    }
    expected_hash(None, file)
}

/// Moves a finished download into place, or deletes it when the hash doesn't match.
pub fn accept_download(part: &Path, dest: &Path, expected: &str, actual: &str) -> Result<PathBuf, String> {
    if !expected.eq_ignore_ascii_case(actual) {
        let _ = std::fs::remove_file(part);
        return Err("The download doesn't match its published checksum, so it wasn't kept. Please try again.".into());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        std::fs::remove_file(dest).map_err(|e| format!("couldn't replace the previous download ({e})"))?;
    }
    match std::fs::rename(part, dest) {
        Ok(()) => Ok(dest.to_path_buf()),
        Err(_) => {
            std::fs::copy(part, dest).map_err(|e| format!("couldn't save the download ({e})"))?;
            let _ = std::fs::remove_file(part);
            Ok(dest.to_path_buf())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    #[test]
    fn names() {
        assert_eq!(candidates(InstallKind::WindowsSetup, "v0.2.0", "x86_64"), ["WaveHub-0.2.0-setup-x64.exe", "WaveHub-0.2.0-windows-x64.zip", "WaveHub.exe"]);
        assert_eq!(candidates(InstallKind::Deb, "0.2.0", "x86_64")[0], "WaveHub-0.2.0-linux-x86_64.deb");
        assert_eq!(candidates(InstallKind::Arch, "0.2.0", "x86_64")[0], "WaveHub-0.2.0-linux-x86_64.pkg.tar.zst");
        assert_eq!(candidates(InstallKind::AppImage, "0.2.0", "aarch64"), ["WaveHub-0.2.0-linux-aarch64.AppImage", "WaveHub-0.2.0-linux-aarch64.tar.gz"]);
        assert_eq!(candidates(InstallKind::Bare, "0.2.0", "x86_64").last().unwrap(), "WaveHub-linux-x86_64");
        assert_eq!(candidates(InstallKind::MacOs, "0.2.0", "aarch64")[0], "WaveHub-0.2.0-macos-universal.dmg");
    }

    #[test]
    fn detects_linux_installs() {
        let root = std::env::temp_dir().join(format!("wavehub-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("etc")).unwrap();
        let usr = Path::new("/usr/bin/wave-hub");
        assert_eq!(detect_linux(Path::new("/home/d/Apps/WaveHub.AppImage"), true, &root), InstallKind::AppImage);
        assert_eq!(detect_linux(Path::new("/home/d/Downloads/WaveHub-linux-x86_64"), false, &root), InstallKind::Bare);
        assert_eq!(detect_linux(Path::new("/home/d/.local/bin/wave-hub"), false, &root), InstallKind::Tarball);
        std::fs::write(root.join("etc/os-release"), "ID=cachyos\nID_LIKE=arch\n").unwrap();
        assert_eq!(detect_linux(usr, false, &root), InstallKind::Arch);
        std::fs::create_dir_all(root.join("var/lib/rpm")).unwrap();
        assert_eq!(detect_linux(usr, false, &root), InstallKind::Rpm);
        std::fs::create_dir_all(root.join("var/lib/pacman/local/wave-hub-bin-0.2.0-1")).unwrap();
        assert_eq!(detect_linux(usr, false, &root), InstallKind::Arch);
        std::fs::create_dir_all(root.join("var/lib/dpkg/info")).unwrap();
        std::fs::write(root.join("var/lib/dpkg/info/wave-hub.list"), "/usr/bin/wave-hub\n").unwrap();
        assert_eq!(detect_linux(usr, false, &root), InstallKind::Deb);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A tiny stand-in for github.com: `releases/latest` redirects to the newest tag, and a
    /// release "has" exactly the files listed for it.
    fn mock_github(releases: &'static [(&'static str, &'static [&'static str])]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let me = base.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let repo = "/postmanpat-303/WaveHub";
                let found = if path == format!("{repo}/releases/latest") {
                    Some(format!("{me}{repo}/releases/tag/{}", releases[0].0))
                } else {
                    releases.iter().find_map(|(tag, files)| {
                        let file = path.strip_prefix(&format!("{repo}/releases/download/{tag}/"))?;
                        files.contains(&file).then(|| format!("https://objects.example/{file}"))
                    })
                };
                // A renamed repo: GitHub answers 301 with the new name for every path.
                let moved = path.strip_prefix("/postmanpat-303/OldWaveHub/").map(|rest| format!("{me}{repo}/{rest}"));
                let resp = match (moved, found) {
                    (Some(loc), _) => format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                    (None, Some(loc)) => format!("HTTP/1.1 302 Found\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                    (None, None) => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                };
                let mut s = stream;
                let _ = s.write_all(resp.as_bytes());
            }
        });
        base
    }

    #[test]
    fn picks_from_a_mock_github() {
        static RELEASES: &[(&str, &[&str])] = &[
            (
                "v0.2.0",
                &["WaveHub-0.2.0-setup-x64.exe", "WaveHub-0.2.0-windows-x64.zip", "WaveHub-0.2.0-linux-x86_64.deb", "WaveHub-0.2.0-linux-x86_64.AppImage", "WaveHub.exe", "WaveHub-linux-x86_64"],
            ),
            ("v0.1.0", &["WaveHub.exe", "WaveHub-linux-x86_64", "SHA256SUMS.txt"]),
        ];
        github::set_base_for_tests(&mock_github(RELEASES));
        let repo = "postmanpat-303/WaveHub";
        assert_eq!(github::latest_tag(repo).unwrap().as_deref(), Some("v0.2.0"));
        let pick_for = |kind, tag: &str| pick(repo, tag, &candidates(kind, tag, "x86_64")).unwrap();
        // New names.
        assert_eq!(pick_for(InstallKind::WindowsSetup, "v0.2.0").as_deref(), Some("WaveHub-0.2.0-setup-x64.exe"));
        assert_eq!(pick_for(InstallKind::WindowsPortable, "v0.2.0").as_deref(), Some("WaveHub-0.2.0-windows-x64.zip"));
        assert_eq!(pick_for(InstallKind::Deb, "v0.2.0").as_deref(), Some("WaveHub-0.2.0-linux-x86_64.deb"));
        // No .rpm in this release: falls back to the AppImage.
        assert_eq!(pick_for(InstallKind::Rpm, "v0.2.0").as_deref(), Some("WaveHub-0.2.0-linux-x86_64.AppImage"));
        // A release with only 0.1.0's names.
        assert_eq!(pick_for(InstallKind::WindowsSetup, "v0.1.0").as_deref(), Some("WaveHub.exe"));
        assert_eq!(pick_for(InstallKind::AppImage, "v0.1.0").as_deref(), Some("WaveHub-linux-x86_64"));
        // The old name of a renamed repo still finds its releases and files.
        assert_eq!(github::latest_tag("postmanpat-303/OldWaveHub").unwrap().as_deref(), Some("v0.2.0"));
        assert!(github::asset_exists("postmanpat-303/OldWaveHub", "v0.2.0", "WaveHub.exe").unwrap());
        assert!(!github::asset_exists("postmanpat-303/OldWaveHub", "v0.2.0", "Nope.exe").unwrap());
        // Nothing for this computer.
        assert_eq!(pick(repo, "v0.1.0", &candidates(InstallKind::MacOs, "v0.1.0", "aarch64")).unwrap(), None);
    }

    #[test]
    fn stable_versions_and_no_surprises() {
        assert_eq!(compare_stable("v0.2.9", "0.2.8"), StableCompare::Newer);
        assert_eq!(compare_stable("0.2.8", "v0.2.8"), StableCompare::NotNewer);
        assert_eq!(compare_stable("v0.2.7", "0.2.8"), StableCompare::NotNewer);
        assert_eq!(compare_stable("v0.10.0", "0.9.9"), StableCompare::Newer);
        assert_eq!(compare_stable("v1.2.0", "1.2"), StableCompare::NotNewer);
        assert_eq!(compare_stable("v0.2.3.5", "0.2.3"), StableCompare::Newer);
        assert_eq!(compare_stable("v1.2.3+build.4", "1.2.2"), StableCompare::Newer);
        assert_eq!(compare_stable("v1.2.3+build.4", "1.2.3"), StableCompare::NotNewer);
        // A rebuild suffix is not a newer version, and a pre-release is not offered.
        assert_eq!(compare_stable("v1.2.3-1", "1.2.3"), StableCompare::NotNewer);
        assert_eq!(compare_stable("v1.2.3-1", "1.2.2"), StableCompare::Newer);
        for tag in ["v0.2.9-rc.1", "v0.2.9-RC1", "v0.2.9-beta.2", "v0.2.9-alpha", "v1.0.0-preview.1", "nightly", "v0.3.0-dev", "v9.9.9-test", "", "v"] {
            assert_eq!(compare_stable(tag, "0.2.8"), StableCompare::NotStable, "{tag}");
            assert!(!should_ask(tag, "0.2.8", "", true), "{tag}");
        }
        // The running version is ahead of the published stable release: no downgrade.
        assert!(!should_ask("v0.2.7", "0.2.8", "", true));
    }

    #[test]
    fn later_hides_one_version_and_a_manual_check_asks_again() {
        assert!(should_ask("v1.2.3", "1.2.0", "", false));
        assert!(!should_ask("v1.2.3", "1.2.0", "v1.2.3", false));
        assert!(!should_ask("v1.2.3", "1.2.0", "1.2.3", false));
        // An older dismissal doesn't hide a newer release. A newer dismissal hides this one.
        assert!(should_ask("v1.2.4", "1.2.0", "v1.2.3", false));
        assert!(!should_ask("v1.2.3", "1.0.0", "v2.0.0", false));
        // Check for updates asks again. A value we can't read doesn't block.
        assert!(should_ask("v1.2.3", "1.2.0", "v1.2.3", true));
        assert!(should_ask("v1.2.3", "1.2.0", "later-please", false));
        assert!(!should_ask("v1.2.3", "", "", true));
    }

    #[test]
    fn checks_do_not_nag_or_clobber_the_prompt() {
        let offer = CheckOutcome::Offer("v1.2.3".into());
        let once = fold_check(UpdateState::Hidden, offer.clone(), false);
        let twice = fold_check(once.clone(), offer.clone(), false);
        assert_eq!(once, twice);
        assert_eq!(twice, UpdateState::Asking { tag: "v1.2.3".into() });

        // Offline at startup: no banner. Offline while the question is up: leave it there.
        assert_eq!(fold_check(UpdateState::Hidden, CheckOutcome::Unreachable, false), UpdateState::Hidden);
        assert_eq!(fold_check(UpdateState::Hidden, CheckOutcome::Quiet, false), UpdateState::Hidden);
        assert_eq!(fold_check(once.clone(), CheckOutcome::Unreachable, false), once);

        let working = UpdateState::Working { tag: "v1.2.3".into(), file: "WaveHub.zip".into(), stage: WorkStage::Downloading, done: 10, total: 40 };
        assert_eq!(fold_check(working.clone(), CheckOutcome::Offer("v9.0.0".into()), true), working);
        assert_eq!(dismiss_tag(&working), None);

        let failed = UpdateState::Failed { tag: "v1.2.3".into(), message: "offline".into() };
        assert_eq!(fold_check(failed.clone(), offer, false), failed);
        assert_eq!(fold_check(failed, CheckOutcome::Offer("v1.2.3".into()), true), UpdateState::Asking { tag: "v1.2.3".into() });

        let ready = UpdateState::Ready { tag: "v1.2.3".into(), file: "a.zip".into(), path: PathBuf::from("a.zip"), note: "ready".into() };
        assert_eq!(fold_check(ready.clone(), CheckOutcome::Offer("v1.2.3".into()), true), ready);
        assert_eq!(fold_check(ready.clone(), CheckOutcome::Quiet, false), ready);

        // A later stable release replaces a failure. Later remembers the tag it dismissed.
        let failed = UpdateState::Failed { tag: "v1.2.3".into(), message: "no".into() };
        assert_eq!(fold_check(failed, CheckOutcome::Offer("v1.2.4".into()), false), UpdateState::Asking { tag: "v1.2.4".into() });
        assert_eq!(dismiss_tag(&UpdateState::Asking { tag: "v1.4.0".into() }).as_deref(), Some("v1.4.0"));
    }

    #[test]
    fn download_state_cancel_returns_to_the_question() {
        let asking = UpdateState::Asking { tag: "v1.2.3".into() };
        assert_eq!(begin_download(UpdateState::Hidden, "a.zip"), UpdateState::Hidden);
        let started = begin_download(asking, "WaveHub-1.2.3-setup-x64.exe");
        assert_eq!(
            started,
            UpdateState::Working { tag: "v1.2.3".into(), file: "WaveHub-1.2.3-setup-x64.exe".into(), stage: WorkStage::CheckingSums, done: 0, total: 0 }
        );
        let halfway = note_progress(started, 5, 10);
        assert!(matches!(halfway, UpdateState::Working { stage: WorkStage::Downloading, done: 5, total: 10, .. }));
        let checking = mark_verifying(halfway);
        assert!(matches!(checking, UpdateState::Working { stage: WorkStage::Verifying, .. }));
        // Cancelling does not dismiss the version.
        assert_eq!(cancel_download(checking), UpdateState::Asking { tag: "v1.2.3".into() });

        let working = begin_download(UpdateState::Failed { tag: "v1.2.3".into(), message: "try again".into() }, "a.zip");
        let failed = mark_failed(working, "The download doesn't match its published checksum, so it wasn't kept. Please try again.".into());
        assert!(matches!(failed, UpdateState::Failed { tag, .. } if tag == "v1.2.3"));
        // A progress tick after the question is back must not reopen the download.
        assert_eq!(note_progress(UpdateState::Asking { tag: "v1.2.3".into() }, 1, 2), UpdateState::Asking { tag: "v1.2.3".into() });
    }

    #[test]
    fn checksum_keeps_a_match_and_drops_a_miss() {
        let hash = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let text = format!("{hash}  WaveHub-1.2.3-setup-x64.exe\n");
        assert_eq!(expected_hash(Some(&text), "WaveHub-1.2.3-setup-x64.exe").unwrap(), hash);
        assert!(expected_hash(Some(&text), "other.zip").unwrap_err().contains("doesn't include"));
        assert!(expected_hash(None, "a.zip").unwrap_err().contains("no SHA256SUMS"));

        let dir = std::env::temp_dir().join(format!("wavehub-sums-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("ok.bin.part");
        std::fs::write(&part, b"hello").unwrap();
        let dest = dir.join("ok.bin");
        assert_eq!(accept_download(&part, &dest, hash, hash).unwrap(), dest);
        assert!(dest.is_file());
        assert!(!part.exists());

        let bad = dir.join("bad.bin.part");
        std::fs::write(&bad, b"nope").unwrap();
        let err = accept_download(&bad, &dir.join("bad.bin"), hash, "00").unwrap_err();
        assert!(err.contains("doesn't match"));
        assert!(!bad.exists());
        assert!(!dir.join("bad.bin").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handoff_says_what_this_computer_can_do() {
        let kinds = [
            InstallKind::WindowsSetup,
            InstallKind::WindowsPortable,
            InstallKind::AppImage,
            InstallKind::Deb,
            InstallKind::Rpm,
            InstallKind::Arch,
            InstallKind::Tarball,
            InstallKind::Bare,
            InstallKind::MacOs,
            InstallKind::Flatpak,
        ];
        for kind in kinds {
            let before = before_you_update(kind).to_ascii_lowercase();
            let after = handoff(kind, "WaveHub-1.2.3-macos-universal.dmg").to_ascii_lowercase();
            assert!(!before.contains("restart"), "{kind:?}");
            assert!(!after.contains("restart"), "{kind:?}");
            assert!(before.len() > 40 && after.len() > 40, "{kind:?}");
        }
        assert!(handoff(InstallKind::MacOs, "WaveHub-1.2.3-macos-universal.zip").contains("zip"));
        assert!(handoff(InstallKind::MacOs, "WaveHub-1.2.3-macos-universal.dmg").contains("disk image"));
        assert!(handoff(InstallKind::Flatpak, "WaveHub-1.2.3-linux-x86_64.flatpak").contains("Flatpak"));
        assert!(handoff(InstallKind::Deb, "WaveHub-1.2.3-linux-x86_64.deb").contains("apt"));
        assert!(before_you_update(InstallKind::WindowsSetup).contains("administrator password"));
        assert_eq!(candidates(InstallKind::Flatpak, "0.2.8", "x86_64"), ["WaveHub-0.2.8-linux-x86_64.flatpak"]);
        assert!(no_file_message(InstallKind::Deb).contains("Nothing was downloaded"));
        assert!(plain_name_rejected());
    }

    fn plain_name_rejected() -> bool {
        // A name that climbs out of the downloads folder is refused before any write.
        fetch_verified("postmanpat-303/WaveHub", "v0.2.8", "../WaveHub.exe", Path::new("."), &AtomicBool::new(false), &mut |_| ()).is_err()
    }
}
