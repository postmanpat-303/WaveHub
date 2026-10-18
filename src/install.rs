//! Installing, updating, opening and removing an app.
//!
//! Default install is the portable zip on Windows and the AppImage on Linux, into Wave Hub's own
//! per-user folder (no admin rights needed). On Windows the official setup program can be chosen
//! instead in Settings. An app is never replaced while it is running.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use crate::github;
use crate::manifest::{fill, AppSpec};
use crate::store::{now, Kind, Paths, Record};

/// What Wave Hub would do to install `spec` on this computer.
#[derive(Clone, Debug)]
pub enum Plan {
    AppImage { file: String },
    Portable { file: String, exe: String },
    Installer { file: String, exe_candidates: Vec<String>, silent_args: Vec<String>, uninstall_key: Option<String> },
    Unavailable(String),
}

pub fn os_label() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "this system"
    }
}

pub fn plan(spec: &AppSpec, prefer_installer: bool) -> Plan {
    if cfg!(windows) {
        let Some(w) = &spec.windows else {
            return Plan::Unavailable("No Windows download yet.".into());
        };
        let installer = w.installer.clone().map(|file| Plan::Installer { file, exe_candidates: w.installed_exe.clone(), silent_args: w.silent_args.clone(), uninstall_key: w.uninstall_key.clone() });
        let portable = match (&w.portable, &w.exe) {
            (Some(file), Some(exe)) => Some(Plan::Portable { file: file.clone(), exe: exe.clone() }),
            _ => None,
        };
        let pick = if prefer_installer { installer.or(portable) } else { portable.or(installer) };
        pick.unwrap_or_else(|| Plan::Unavailable("No Windows download yet.".into()))
    } else if cfg!(target_os = "linux") {
        let l = spec.linux.clone().unwrap_or_default();
        match l.appimage {
            Some(file) if l.arches.iter().any(|a| a == std::env::consts::ARCH) => Plan::AppImage { file },
            Some(_) => Plan::Unavailable("No download for this processor yet.".into()),
            None => Plan::Unavailable(l.unavailable.unwrap_or_else(|| "No Linux download yet.".into())),
        }
    } else if cfg!(target_os = "macos") {
        let m = spec.macos.clone().unwrap_or_default();
        match (m.zip, m.exe) {
            (Some(file), Some(exe)) => Plan::Portable { file, exe },
            _ => Plan::Unavailable(m.unavailable.unwrap_or_else(|| "No macOS download yet.".into())),
        }
    } else {
        Plan::Unavailable("Wave Hub supports Windows, Linux and macOS.".into())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Waiting its turn behind another app (Update all).
    Queued,
    Checking,
    Downloading {
        done: u64,
        total: u64,
    },
    Verifying,
    Installing,
    /// The official setup program is running (silently where it can).
    RunningInstaller,
    /// Being uninstalled.
    Removing,
}

/// Downloads, checks and installs `tag` of `spec`. `previous` is what's installed now, if anything.
pub fn install(spec: &AppSpec, tag: &str, plan: &Plan, paths: &Paths, previous: Option<&Record>, cancel: &AtomicBool, report: &mut dyn FnMut(Phase)) -> Result<Record, String> {
    if let Some(prev) = previous {
        if is_running(&prev.path) {
            return Err(format!("{} is open. Close it first, then try again.", spec.name));
        }
    }
    let template = match plan {
        Plan::AppImage { file } | Plan::Portable { file, .. } | Plan::Installer { file, .. } => file,
        Plan::Unavailable(why) => return Err(why.clone()),
    };
    let file = fill(template, tag);
    report(Phase::Checking);
    let expected = match &spec.sums {
        Some(sums) => {
            let text = github::get_text(&github::download_url(&spec.repo, tag, sums))?;
            match text {
                Some(text) => {
                    Some(github::parse_sums(&text).into_iter().find(|(_, name)| *name == file).map(|(hash, _)| hash).ok_or_else(|| format!("{sums} doesn't list {file}, so it can't be checked."))?)
                }
                // Older releases may not have one; install unchecked rather than not at all.
                None => None,
            }
        }
        None => None,
    };
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Cancelled.".into());
    }

    let part = paths.downloads().join(format!("{file}.part"));
    let url = github::download_url(&spec.repo, tag, &file);
    let hash = github::download(&url, &part, cancel, &mut |done, total| report(Phase::Downloading { done, total }))?;
    report(Phase::Verifying);
    if let Some(expected) = expected {
        if expected != hash {
            let _ = std::fs::remove_file(&part);
            return Err("The download doesn't match its published checksum, so it wasn't installed. Please try again.".into());
        }
    }
    // Check again: the app may have been opened while we were downloading.
    if let Some(prev) = previous {
        if is_running(&prev.path) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{} is open. Close it first, then try again.", spec.name));
        }
    }

    report(Phase::Installing);
    let version = tag.trim_start_matches('v').to_string();
    let record = |kind, path| Record { version: version.clone(), tag: tag.to_string(), kind, path, installed_at: now(), external: false };
    let result = match plan {
        Plan::AppImage { .. } => install_appimage(spec, &part, paths).map(|p| record(Kind::AppImage, p)),
        Plan::Portable { exe, .. } => install_portable(spec, &part, exe, paths).map(|p| record(Kind::PortableZip, p)),
        Plan::Installer { exe_candidates, silent_args, uninstall_key, .. } => {
            report(Phase::RunningInstaller);
            run_installer(spec, &part, &file, silent_args, exe_candidates, uninstall_key.as_deref()).map(|p| record(Kind::Installer, p))
        }
        Plan::Unavailable(_) => unreachable!(),
    };
    let _ = std::fs::remove_file(&part);
    if result.is_ok() {
        if let Some(prev) = previous {
            // Moving between AppImage/portable and the installer leaves the old copy behind;
            // tidy it if it was ours.
            if prev.kind != Kind::Installer && result.as_ref().is_ok_and(|r| r.kind == Kind::Installer) {
                let _ = std::fs::remove_dir_all(paths.apps().join(&spec.id));
            }
        }
    }
    result
}

pub(crate) fn app_dir(spec: &AppSpec, paths: &Paths) -> PathBuf {
    paths.apps().join(&spec.id)
}

fn install_appimage(spec: &AppSpec, part: &Path, paths: &Paths) -> Result<PathBuf, String> {
    let dir = app_dir(spec, paths);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name: String = spec.name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let target = dir.join(format!("{name}.AppImage"));
    let staged = dir.join(format!("{name}.AppImage.new"));
    // Copy rather than rename: downloads and apps may one day be on different drives.
    std::fs::copy(part, &staged).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&staged, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

fn install_portable(spec: &AppSpec, zip_path: &Path, exe: &str, paths: &Paths) -> Result<PathBuf, String> {
    let dir = app_dir(spec, paths);
    let staging = paths.apps().join(format!("{}.new", spec.id));
    let old = paths.apps().join(format!("{}.old", spec.id));
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let unpack = || -> Result<(), String> {
        let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("the zip can't be opened ({e})"))?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            // `enclosed_name` refuses paths that would escape the folder.
            let Some(rel) = entry.enclosed_name() else {
                return Err("the zip contains an unsafe path".into());
            };
            let out = staging.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
            // Keep the "can run" bit (a macOS .app's program needs it).
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777)).map_err(|e| e.to_string())?;
            }
        }
        if !staging.join(exe).is_file() {
            return Err(format!("the zip doesn't contain {exe}"));
        }
        Ok(())
    };
    if let Err(e) = unpack() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    // Swap folders: old -> .old, new -> in place, then delete .old. Settings live elsewhere, so
    // nothing of the user's is lost.
    if dir.exists() {
        std::fs::rename(&dir, &old).map_err(|e| format!("couldn't move the old version aside ({e}). Is {} open?", spec.name))?;
    }
    if let Err(e) = std::fs::rename(&staging, &dir) {
        let _ = std::fs::rename(&old, &dir);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(dir.join(exe))
}

fn run_installer(spec: &AppSpec, part: &Path, file: &str, args: &[String], candidates: &[String], key: Option<&str>) -> Result<PathBuf, String> {
    let setup = part.with_file_name(file);
    std::fs::rename(part, &setup).map_err(|e| e.to_string())?;
    let status = installer_command(&setup, args).status();
    let _ = std::fs::remove_file(&setup);
    let status = status.map_err(|e| format!("couldn't start the installer ({e})"))?;
    let found = find_installed(candidates).or_else(|| key.and_then(registry_exe));
    if !status.success() && found.is_none() {
        let code = status.code().map(|c| format!(" (code {c})")).unwrap_or_default();
        return Err(format!("{}'s installer stopped before finishing{code}. Try again, or run it yourself from the release page.", spec.name));
    }
    found.ok_or_else(|| format!("The installer finished, but {} wasn't found afterwards. If Windows asked for permission and you said no, that's fine: try again.", spec.name))
}

/// Where the official installer put the program, if it did.
pub fn find_installed(candidates: &[String]) -> Option<PathBuf> {
    let local = dirs::data_local_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let program_files = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    candidates.iter().map(|c| PathBuf::from(c.replace("{localappdata}", &local).replace("{programfiles}", &program_files))).find(|p| p.is_file())
}

/// The program named by an installer's `DisplayIcon` (per-user first, then per-machine).
#[cfg(windows)]
pub(crate) fn registry_exe(key: &str) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    for root in ["HKCU", "HKLM", r"HKLM\SOFTWARE\WOW6432Node"] {
        let path = if root.contains('\\') { format!(r"{root}\Microsoft\Windows\CurrentVersion\Uninstall\{key}") } else { format!(r"{root}\Software\Microsoft\Windows\CurrentVersion\Uninstall\{key}") };
        let Ok(out) = Command::new("reg").args(["query", &path, "/v", "DisplayIcon"]).creation_flags(0x0800_0000).output() else {
            continue;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(exe) = parse_display_icon(&text) {
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

#[cfg(not(windows))]
pub(crate) fn registry_exe(_key: &str) -> Option<PathBuf> {
    None
}

/// `    DisplayIcon    REG_SZ    "C:\x\App.exe",0` -> `C:\x\App.exe`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_display_icon(reg_output: &str) -> Option<PathBuf> {
    let line = reg_output.lines().find(|l| l.trim_start().starts_with("DisplayIcon"))?;
    let value = line.split("REG_SZ").nth(1)?.trim();
    let value = value.trim_matches('"');
    let value = match value.rsplit_once(',') {
        Some((path, idx)) if idx.trim().parse::<i32>().is_ok() => path.trim_matches('"'),
        _ => value,
    };
    (!value.is_empty()).then(|| PathBuf::from(value))
}

#[cfg(windows)]
pub(crate) fn installer_command(setup: &Path, args: &[String]) -> Command {
    use std::os::windows::process::CommandExt;
    // `start /wait` goes through the shell, so an installer that needs admin rights gets the
    // usual Windows prompt, and we still wait for it to finish. `/S` keeps NSIS quiet.
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg("start").arg("").arg("/wait").arg(setup).args(args);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(not(windows))]
pub(crate) fn installer_command(setup: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(setup);
    cmd.args(args);
    cmd
}

#[cfg(test)]
mod tests {
    /// Unpacks a real macOS app zip the way Wave Hub does on a Mac and checks the program
    /// keeps its "can run" bit. Run with
    /// `WAVEHUB_TEST_MAC_ZIP=BigWaveRadio-0.9.6-macos-universal.zip cargo test -- --ignored`.
    #[test]
    #[ignore]
    #[cfg(unix)]
    fn unpacks_a_mac_app_zip() {
        use std::os::unix::fs::PermissionsExt;
        let zip = std::env::var("WAVEHUB_TEST_MAC_ZIP").expect("set WAVEHUB_TEST_MAC_ZIP");
        let m: crate::manifest::Manifest = serde_json::from_str(include_str!("../apps.json")).unwrap();
        let spec = m.apps.iter().find(|a| a.id == "bigwave-radio").unwrap();
        let exe = spec.macos.as_ref().unwrap().exe.clone().unwrap();
        let paths = crate::store::Paths { data: std::env::temp_dir().join(format!("wavehub-mac-{}", std::process::id())) };
        let path = super::install_portable(spec, std::path::Path::new(&zip), &exe, &paths).unwrap();
        assert!(path.ends_with("BigWave Radio.app/Contents/MacOS/BigWaveRadio"));
        assert!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o111 != 0);
        assert!(path.parent().unwrap().parent().unwrap().join("Info.plist").is_file());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn display_icon() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\...\\StreamWave\r\n    DisplayIcon    REG_SZ    \"C:\\Users\\d\\AppData\\Local\\StreamWave\\StreamWave.exe\"\r\n";
        assert_eq!(super::parse_display_icon(out).unwrap().to_string_lossy(), r"C:\Users\d\AppData\Local\StreamWave\StreamWave.exe");
        let out = "    DisplayIcon    REG_SZ    C:\\Program Files\\BigWave Radio\\BigWaveRadio.exe,0";
        assert_eq!(super::parse_display_icon(out).unwrap().to_string_lossy(), r"C:\Program Files\BigWave Radio\BigWaveRadio.exe");
    }
}

/// Starts the app on its own; it keeps running if Wave Hub closes.
pub fn open(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        let hint = if cfg!(windows) { " If Windows Security removed it, see Windows Security > Virus & threat protection > Protection history." } else { "" };
        return Err(format!("{} is missing. Try installing again.{hint}", path.display()));
    }
    let mut cmd = Command::new(path);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // Don't hand our own AppImage environment to another AppImage.
    for var in ["APPIMAGE", "APPDIR", "OWD", "ARGV0"] {
        cmd.env_remove(var);
    }
    let mut child = cmd.spawn().map_err(|e| start_error(path, &e))?;
    // Reap it when it exits so it doesn't linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A friendly message for a program that wouldn't start.
fn start_error(path: &Path, e: &std::io::Error) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match e.raw_os_error() {
        // ERROR_VIRUS_INFECTED / ERROR_VIRUS_DELETED: Microsoft Defender stopped it.
        Some(225) | Some(226) if cfg!(windows) => format!(
            "Windows Security blocked {name}. New apps that aren't code-signed yet are sometimes \
             flagged by mistake. If you trust it: open Windows Security > Virus & threat protection > \
             Protection history, choose the {name} entry, then Actions > Allow on device (or Restore). \
             Then click Open again, or Reinstall if it was removed."
        ),
        // ERROR_ACCESS_DENIED, often Smart App Control or a company policy.
        Some(5) if cfg!(windows) => format!(
            "Windows didn't allow {name} to start (access denied). Smart App Control or a work or \
             school policy may be blocking apps that aren't code-signed yet."
        ),
        _ => format!("couldn't start it ({e})"),
    }
}

/// Whether a program started from `path` is running.
#[cfg(target_os = "linux")]
pub fn is_running(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    let target = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let target_str = target.to_string_lossy().into_owned();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let proc_dir = entry.path();
        if std::fs::read_link(proc_dir.join("exe")).is_ok_and(|exe| exe == target) {
            return true;
        }
        // An AppImage runs from a mount; its runtime keeps the AppImage path in its arguments
        // or the APPIMAGE variable.
        if let Ok(cmdline) = std::fs::read(proc_dir.join("cmdline")) {
            if cmdline.split(|b| *b == 0).next().is_some_and(|arg0| arg0 == target_str.as_bytes()) {
                return true;
            }
        }
        // Only count programs running from the AppImage's own mount (or extracted copy) in
        // the temp folder: system helpers it started, such as a session dbus-daemon, inherit
        // APPIMAGE too and can outlive the app.
        let from_mount = std::fs::read_link(proc_dir.join("exe")).is_ok_and(|exe| exe.starts_with(std::env::temp_dir()));
        if !from_mount {
            continue;
        }
        if let Ok(environ) = std::fs::read(proc_dir.join("environ")) {
            let needle = format!("APPIMAGE={target_str}");
            if environ.split(|b| *b == 0).any(|kv| kv == needle.as_bytes()) {
                return true;
            }
        }
    }
    false
}

#[cfg(windows)]
pub fn is_running(path: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let out = Command::new("tasklist").args(["/FI", &format!("IMAGENAME eq {name}"), "/NH", "/FO", "CSV"]).creation_flags(0x0800_0000).output();
    out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(&name.to_ascii_lowercase()))
}

#[cfg(target_os = "macos")]
pub fn is_running(path: &Path) -> bool {
    // `pgrep -f` matches the full command line, which starts with the program's path.
    Command::new("pgrep").arg("-f").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn is_running(_path: &Path) -> bool {
    false
}

/// Opens a folder in the file manager.
pub fn reveal(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(program).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}
