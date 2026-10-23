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
/// `files` are release file names to try in order (see [`resolve_file`]).
#[derive(Clone, Debug)]
pub enum Plan {
    AppImage {
        files: Vec<String>,
    },
    /// A zip: Windows portable (`exe` required, first one found wins) or a macOS `.app`
    /// (`exe` optional).
    Portable {
        files: Vec<String>,
        exe: Vec<String>,
    },
    Installer {
        files: Vec<String>,
        exe_candidates: Vec<String>,
        silent_args: Vec<String>,
        uninstall_key: Vec<String>,
    },
    Unavailable(String),
}

impl Plan {
    pub fn files(&self) -> &[String] {
        match self {
            Plan::AppImage { files } | Plan::Portable { files, .. } | Plan::Installer { files, .. } => files,
            Plan::Unavailable(_) => &[],
        }
    }
}

/// The first of `files` that the release `tag` actually has, by asking GitHub about each name
/// (a HEAD request, no API). `Ok(None)`: none of them is there (yet).
pub fn resolve_file(repo: &str, tag: &str, files: &[String]) -> Result<Option<String>, String> {
    for f in files {
        let name = fill(f, tag);
        if github::asset_exists(repo, tag, &name)? {
            return Ok(Some(name));
        }
    }
    Ok(None)
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
        let installer = (!w.installer.is_empty()).then(|| Plan::Installer {
            files: w.installer.clone(),
            exe_candidates: w.installed_exe.clone(),
            silent_args: w.silent_args.clone(),
            uninstall_key: w.uninstall_key.clone(),
        });
        let portable = (!w.portable.is_empty() && !w.exe.is_empty()).then(|| Plan::Portable { files: w.portable.clone(), exe: w.exe.clone() });
        let pick = if prefer_installer { installer.or(portable) } else { portable.or(installer) };
        pick.unwrap_or_else(|| Plan::Unavailable("No Windows download yet.".into()))
    } else if cfg!(target_os = "linux") {
        let l = spec.linux.clone().unwrap_or_default();
        if l.appimage.is_empty() {
            Plan::Unavailable(l.unavailable.unwrap_or_else(|| "No Linux download yet.".into()))
        } else if l.arches.iter().any(|a| a == std::env::consts::ARCH) {
            Plan::AppImage { files: l.appimage }
        } else {
            Plan::Unavailable("No download for this processor yet.".into())
        }
    } else if cfg!(target_os = "macos") {
        let m = spec.macos.clone().unwrap_or_default();
        if m.zip.is_empty() {
            Plan::Unavailable(m.unavailable.unwrap_or_else(|| "No macOS download yet.".into()))
        } else {
            Plan::Portable { files: m.zip, exe: m.exe.into_iter().collect() }
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
/// `shortcuts`: make Start menu and Desktop shortcuts for a Windows portable install (Settings).
#[allow(clippy::too_many_arguments)]
pub fn install(spec: &AppSpec, tag: &str, plan: &Plan, paths: &Paths, previous: Option<&Record>, shortcuts: bool, cancel: &AtomicBool, report: &mut dyn FnMut(Phase)) -> Result<Record, String> {
    if let Some(prev) = previous {
        if is_running(&prev.path) {
            return Err(format!("{} is open. Close it first, then try again.", spec.name));
        }
    }
    if let Plan::Unavailable(why) = plan {
        return Err(why.clone());
    }
    report(Phase::Checking);
    // Ask first even for a single name, so a missing file says so (not "can't be checked").
    let file = resolve_file(&spec.repo, tag, plan.files())?.ok_or_else(|| format!("This {} release has no download for {} yet.", spec.name, os_label()))?;
    // The first checksum list the release has. Older releases may not have one; install
    // unchecked rather than not at all.
    let mut expected = None;
    for sums in &spec.sums {
        if let Some(text) = github::get_text(&github::download_url(&spec.repo, tag, sums))? {
            let hash = github::parse_sums(&text).into_iter().find(|(_, name)| *name == file).map(|(hash, _)| hash);
            expected = Some(hash.ok_or_else(|| format!("{sums} doesn't list {file}, so it can't be checked."))?);
            break;
        }
    }
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
    let record = |kind, path| Record { version: version.clone(), tag: tag.to_string(), kind, path, installed_at: now(), external: false, shortcuts_done: false };
    let result = match plan {
        Plan::AppImage { .. } => install_appimage(spec, &part, paths).map(|p| record(Kind::AppImage, p)),
        Plan::Portable { exe, .. } => install_portable(spec, &part, exe, paths).map(|p| record(Kind::PortableZip, p)),
        Plan::Installer { exe_candidates, silent_args, uninstall_key, .. } => {
            report(Phase::RunningInstaller);
            run_installer(spec, &part, &file, silent_args, exe_candidates, uninstall_key).map(|p| record(Kind::Installer, p))
        }
        Plan::Unavailable(_) => unreachable!(),
    };
    let _ = std::fs::remove_file(&part);
    let mut result = result;
    if let Ok(new) = result.as_mut() {
        if cfg!(windows) && new.kind == Kind::PortableZip {
            let had = previous.is_some_and(|p| p.kind == Kind::PortableZip && p.shortcuts_done);
            if had {
                // Already made once: keep the ones still there pointing at the program, but
                // don't bring back one the user deleted.
                new.shortcuts_done = true;
                if let Err(e) = create_shortcuts(spec, &new.path, true) {
                    crate::links::log(&format!("couldn't update {}'s shortcuts: {e}", spec.name));
                }
            } else if shortcuts {
                match create_shortcuts(spec, &new.path, false) {
                    Ok(()) => new.shortcuts_done = true,
                    Err(e) => crate::links::log(&format!("couldn't make {}'s shortcuts: {e}", spec.name)),
                }
            }
        }
    }
    if result.is_ok() {
        if let Some(prev) = previous {
            // Moving between AppImage/portable and the installer leaves the old copy behind;
            // tidy it if it was ours.
            if prev.kind != Kind::Installer && result.as_ref().is_ok_and(|r| r.kind == Kind::Installer) {
                let _ = std::fs::remove_dir_all(paths.apps().join(&spec.id));
                // The setup makes its own shortcuts; ours would now point at nothing.
                if prev.kind == Kind::PortableZip {
                    remove_shortcuts(spec, paths);
                }
            }
            // The new version's .app has a new name (e.g. after an app is renamed): remove the
            // old bundle Wave Hub put in Applications, so there aren't two copies.
            if cfg!(target_os = "macos") && !prev.external {
                let bundle = |p: &Path| p.ancestors().find(|a| a.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf);
                if let (Some(old_app), Some(new_app)) = (bundle(&prev.path), result.as_ref().ok().and_then(|r| bundle(&r.path))) {
                    if old_app != new_app && old_app.starts_with(mac_apps_dir()) && old_app.is_dir() {
                        let _ = std::fs::remove_dir_all(&old_app);
                    }
                }
            }
            // A Mac app that 0.2.2 or older kept in Wave Hub's own folder now lives in
            // ~/Applications: remove the old copy.
            if cfg!(target_os = "macos") && result.as_ref().is_ok_and(|r| !r.path.starts_with(paths.apps())) && prev.path.starts_with(paths.apps()) {
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

fn install_portable(spec: &AppSpec, zip_path: &Path, exe_names: &[String], paths: &Paths) -> Result<PathBuf, String> {
    let dir = app_dir(spec, paths);
    let mut exe: Option<String> = None;
    let staging = paths.apps().join(format!("{}.new", spec.id));
    let old = paths.apps().join(format!("{}.old", spec.id));
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let mut unpack = || -> Result<(), String> {
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
        // A Mac zip: find the .app and its program if the manifest doesn't say, or if it
        // has moved.
        exe = exe_names.iter().find(|e| staging.join(e).is_file()).or(exe_names.first()).cloned();
        if exe.as_ref().is_none_or(|e| !staging.join(e).is_file()) {
            if let Some(found) = find_app_program(&staging) {
                exe = Some(found);
            }
        }
        match &exe {
            Some(e) if staging.join(e).is_file() => Ok(()),
            Some(e) => Err(format!("the zip doesn't contain {e}")),
            None => Err("the zip doesn't contain a program Wave Hub can start".into()),
        }
    };
    if let Err(e) = unpack() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let exe = exe.unwrap_or_default();
    if cfg!(target_os = "macos") {
        return install_mac_app(spec, &staging, &exe, paths, &mac_apps_dir());
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

// --- Windows: Start menu and Desktop shortcuts for portable installs -------------------------
//
// Made with Windows' own WScript.Shell through PowerShell (no extra crates). Per-user places
// only: Start menu = %APPDATA%\Microsoft\Windows\Start Menu\Programs, Desktop = wherever the
// user's Desktop really is (it may be in OneDrive), so no admin rights are needed.

/// `ChipWave Studio.lnk` (characters Windows doesn't allow in file names are dropped).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn shortcut_file_name(spec: &AppSpec) -> String {
    let name: String = spec.name.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()).collect();
    format!("{}.lnk", name.trim().trim_end_matches('.'))
}

/// A PowerShell single-quoted string: `'` is written twice.
#[cfg_attr(not(windows), allow(dead_code))]
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg_attr(not(windows), allow(dead_code))]
const SHORTCUT_PLACES: &str = "$places = @([Environment]::GetFolderPath('Programs'), [Environment]::GetFolderPath('Desktop')) | Where-Object { $_ }";

/// The PowerShell that makes (or, with `only_existing`, just refreshes) both shortcuts.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn create_shortcuts_script(spec: &AppSpec, exe: &Path, only_existing: bool) -> String {
    let exe_s = exe.to_string_lossy();
    let dir = exe.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let desc = format!("{} (installed by Wave Hub)", spec.name);
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $exe = {exe}; $dir = {dir}; $file = {file}; $desc = {desc}; $onlyExisting = ${only}\n\
         {places}\n\
         $sh = New-Object -ComObject WScript.Shell\n\
         foreach ($p in $places) {{\n\
           $lnk = Join-Path $p $file\n\
           if ($onlyExisting -and -not (Test-Path -LiteralPath $lnk)) {{ continue }}\n\
           if (-not (Test-Path -LiteralPath $p)) {{ New-Item -ItemType Directory -Force -Path $p | Out-Null }}\n\
           $s = $sh.CreateShortcut($lnk)\n\
           $s.TargetPath = $exe; $s.WorkingDirectory = $dir; $s.IconLocation = $exe + ',0'; $s.Description = $desc\n\
           $s.Save()\n\
         }}\n",
        exe = ps_quote(&exe_s),
        dir = ps_quote(&dir),
        file = ps_quote(&shortcut_file_name(spec)),
        desc = ps_quote(&desc),
        only = if only_existing { "true" } else { "false" },
        places = SHORTCUT_PLACES,
    )
}

/// The PowerShell that deletes both shortcuts, but only if they point into Wave Hub's apps folder
/// (a shortcut the app's own setup made, or the user's, is left alone).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn remove_shortcuts_script(spec: &AppSpec, paths: &Paths) -> String {
    let apps = paths.apps().to_string_lossy().trim_end_matches(['\\', '/']).to_string() + "\\";
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $file = {file}; $apps = {apps}\n\
         {places}\n\
         $sh = New-Object -ComObject WScript.Shell\n\
         foreach ($p in $places) {{\n\
           $lnk = Join-Path $p $file\n\
           if (-not (Test-Path -LiteralPath $lnk)) {{ continue }}\n\
           $t = $sh.CreateShortcut($lnk).TargetPath\n\
           if ($t -and $t.StartsWith($apps, [StringComparison]::OrdinalIgnoreCase)) {{ Remove-Item -LiteralPath $lnk -Force }}\n\
         }}\n",
        file = ps_quote(&shortcut_file_name(spec)),
        apps = ps_quote(&apps),
        places = SHORTCUT_PLACES,
    )
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .stdin(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| format!("couldn't start PowerShell ({e})"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Makes `<name>.lnk` in the user's Start menu and on their Desktop, starting `exe` in its own
/// folder with its own icon. `only_existing`: just point the ones still there at `exe`.
#[cfg(windows)]
pub fn create_shortcuts(spec: &AppSpec, exe: &Path, only_existing: bool) -> Result<(), String> {
    run_powershell(&create_shortcuts_script(spec, exe, only_existing))
}

#[cfg(not(windows))]
pub fn create_shortcuts(_spec: &AppSpec, _exe: &Path, _only_existing: bool) -> Result<(), String> {
    Ok(())
}

/// Removes the shortcuts [`create_shortcuts`] made. Failures are only logged.
#[cfg(windows)]
pub fn remove_shortcuts(spec: &AppSpec, paths: &Paths) {
    if let Err(e) = run_powershell(&remove_shortcuts_script(spec, paths)) {
        crate::links::log(&format!("couldn't remove {}'s shortcuts: {e}", spec.name));
    }
}

#[cfg(not(windows))]
pub fn remove_shortcuts(_spec: &AppSpec, _paths: &Paths) {}

/// Portable installs made before Wave Hub made shortcuts (0.2.6 and older): make them once.
/// Returns whether any record changed (save `installed.json` then).
pub fn repair_shortcuts(apps: &[AppSpec], installed: &mut crate::store::Installed) -> bool {
    let mut changed = false;
    if !cfg!(windows) {
        return false;
    }
    for spec in apps {
        let Some(r) = installed.apps.get_mut(&spec.id) else {
            continue;
        };
        if r.kind != Kind::PortableZip || r.external || r.shortcuts_done || !r.path.is_file() {
            continue;
        }
        match create_shortcuts(spec, &r.path, false) {
            Ok(()) => {
                r.shortcuts_done = true;
                changed = true;
            }
            Err(e) => crate::links::log(&format!("couldn't make {}'s shortcuts: {e}", spec.name)),
        }
    }
    changed
}

/// `X.app/Contents/MacOS/X` for the first `.app` in `dir` (or one folder down), from its
/// Info.plist's CFBundleExecutable, or the only program in `Contents/MacOS`.
pub(crate) fn find_app_program(dir: &Path) -> Option<String> {
    let mut apps: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "app") && p.is_dir() {
            apps.push(p);
        } else if p.is_dir() {
            for sub in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                if sub.path().extension().is_some_and(|e| e == "app") && sub.path().is_dir() {
                    apps.push(sub.path());
                }
            }
        }
    }
    apps.sort();
    let program = app_program(&apps.into_iter().next()?)?;
    Some(program.strip_prefix(dir).ok()?.to_string_lossy().into_owned())
}

/// The program inside a `.app`: Info.plist's CFBundleExecutable, or the only file in
/// `Contents/MacOS`.
pub(crate) fn app_program(app: &Path) -> Option<PathBuf> {
    let macos = app.join("Contents/MacOS");
    let from_plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok().and_then(|t| {
        let after = t.split("<key>CFBundleExecutable</key>").nth(1)?;
        Some(after.split("<string>").nth(1)?.split("</string>").next()?.trim().to_string())
    });
    let program = from_plist.filter(|n| macos.join(n).is_file()).or_else(|| {
        let files: Vec<_> = std::fs::read_dir(&macos).ok()?.flatten().filter(|e| e.path().is_file()).collect();
        (files.len() == 1).then(|| files[0].file_name().to_string_lossy().into_owned())
    })?;
    Some(macos.join(program))
}

/// Where Mac apps go: `~/Applications`, the per-user Applications folder (no admin needed).
/// `WAVEHUB_MAC_APPS` overrides it for tests.
pub(crate) fn mac_apps_dir() -> PathBuf {
    std::env::var_os("WAVEHUB_MAC_APPS").map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Applications"))
}

/// Moves the unpacked `.app` from `staging` into `~/Applications`, replacing an older copy.
fn install_mac_app(spec: &AppSpec, staging: &Path, exe: &str, paths: &Paths, apps: &Path) -> Result<PathBuf, String> {
    let rel = Path::new(exe);
    let parts: Vec<_> = rel.components().collect();
    let i = parts.iter().position(|c| c.as_os_str().to_string_lossy().ends_with(".app")).ok_or("the zip has no .app")?;
    let app_rel: PathBuf = parts[..=i].iter().collect();
    let app_name = app_rel.file_name().ok_or("the zip has no .app")?.to_owned();
    let inner = rel.strip_prefix(&app_rel).map_err(|_| "the zip has no .app".to_string())?.to_path_buf();
    std::fs::create_dir_all(apps).map_err(|e| format!("couldn't make {} ({e})", apps.display()))?;
    let target = apps.join(&app_name);
    let old = paths.apps().join(format!("{}.old.app", spec.id));
    let _ = std::fs::remove_dir_all(&old);
    let cleanup = || {
        let _ = std::fs::remove_dir_all(staging);
    };
    if target.exists() {
        // Move the old copy aside (into Wave Hub's folder, so it's out of Applications).
        if std::fs::rename(&target, &old).is_err() {
            // Different disk: copy-free fallback is to delete it.
            if let Err(e) = std::fs::remove_dir_all(&target) {
                cleanup();
                return Err(format!("couldn't replace {} ({e}). Is {} open?", target.display(), spec.name));
            }
        }
    }
    if let Err(e) = std::fs::rename(staging.join(&app_rel), &target) {
        let _ = std::fs::rename(&old, &target);
        cleanup();
        return Err(format!("couldn't move {} into {} ({e})", app_name.to_string_lossy(), apps.display()));
    }
    cleanup();
    let _ = std::fs::remove_dir_all(&old);
    Ok(target.join(inner))
}

fn run_installer(spec: &AppSpec, part: &Path, file: &str, args: &[String], candidates: &[String], keys: &[String]) -> Result<PathBuf, String> {
    let setup = part.with_file_name(file);
    std::fs::rename(part, &setup).map_err(|e| e.to_string())?;
    let status = installer_command(&setup, args).status();
    let _ = std::fs::remove_file(&setup);
    let status = status.map_err(|e| format!("couldn't start the installer ({e})"))?;
    let found = find_installed(candidates).or_else(|| keys.iter().find_map(|k| registry_exe(k)));
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
        let exe = spec.macos.as_ref().unwrap().exe.clone().unwrap_or_else(|| "ChipWave.app/Contents/MacOS/ChipWave".into());
        let paths = crate::store::Paths { data: std::env::temp_dir().join(format!("wavehub-mac-{}", std::process::id())) };
        let path = super::install_portable(spec, std::path::Path::new(&zip), std::slice::from_ref(&exe), &paths).unwrap();
        assert!(path.ends_with("BigWave Radio.app/Contents/MacOS/BigWaveRadio"));
        assert!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o111 != 0);
        assert!(path.parent().unwrap().parent().unwrap().join("Info.plist").is_file());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    /// A real ChipWave Tracker Mac zip (WAVEHUB_TEST_CW_ZIP): the program is found from its
    /// Info.plist without being named.
    #[test]
    #[ignore]
    #[cfg(unix)]
    fn finds_chipwave_program() {
        let zip = std::env::var("WAVEHUB_TEST_CW_ZIP").expect("set WAVEHUB_TEST_CW_ZIP");
        let m: crate::manifest::Manifest = serde_json::from_str(include_str!("../apps.json")).unwrap();
        let spec = m.apps.iter().find(|a| a.id == "chipwave-tracker").unwrap();
        let paths = crate::store::Paths { data: std::env::temp_dir().join(format!("wavehub-cw-{}", std::process::id())) };
        let path = super::install_portable(spec, std::path::Path::new(&zip), &[], &paths).unwrap();
        assert!(path.ends_with("ChipWave.app/Contents/MacOS/ChipWave"), "{}", path.display());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    /// The Mac install steps, run on any OS: find the .app's program from Info.plist, then
    /// move the .app into an Applications folder, replacing an older copy.
    #[test]
    #[cfg(unix)]
    fn mac_app_into_applications() {
        let m: crate::manifest::Manifest = serde_json::from_str(include_str!("../apps.json")).unwrap();
        let spec = m.apps.iter().find(|a| a.id == "streamwave").unwrap();
        let root = std::env::temp_dir().join(format!("wavehub-macapp-{}", std::process::id()));
        let paths = crate::store::Paths { data: root.join("data") };
        let apps = root.join("Applications");
        for round in 0..2 {
            let staging = paths.apps().join("streamwave.new");
            let macos = staging.join("StreamWave.app/Contents/MacOS");
            std::fs::create_dir_all(&macos).unwrap();
            std::fs::write(macos.join("stream-wave"), format!("v{round}")).unwrap();
            std::fs::write(staging.join("StreamWave.app/Contents/Info.plist"), "<dict>\n<key>CFBundleExecutable</key>\n<string>stream-wave</string>\n</dict>").unwrap();
            let exe = super::find_app_program(&staging).unwrap();
            assert_eq!(exe, "StreamWave.app/Contents/MacOS/stream-wave");
            let path = super::install_mac_app(spec, &staging, &exe, &paths, &apps).unwrap();
            assert_eq!(path, apps.join("StreamWave.app/Contents/MacOS/stream-wave"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("v{round}"));
            assert!(!staging.exists());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shortcut_scripts() {
        let mut m: crate::manifest::Manifest = serde_json::from_str(include_str!("../apps.json")).unwrap();
        let spec = m.apps.iter_mut().find(|a| a.id == "chipwave-tracker").unwrap();
        assert_eq!(super::shortcut_file_name(spec), "ChipWave Studio.lnk");
        spec.name = "Del's: App?".into();
        assert_eq!(super::shortcut_file_name(spec), "Del's App.lnk");
        let exe = std::path::Path::new("C:/Users/O'Brien/AppData/Local/WaveHub/apps/chipwave-tracker/ChipWave.exe");
        let s = super::create_shortcuts_script(spec, exe, false);
        assert!(s.contains("$exe = 'C:/Users/O''Brien/AppData/Local/WaveHub/apps/chipwave-tracker/ChipWave.exe'"), "{s}");
        assert!(s.contains("$dir = 'C:/Users/O''Brien/AppData/Local/WaveHub/apps/chipwave-tracker'"), "{s}");
        assert!(s.contains("$file = 'Del''s App.lnk'"), "{s}");
        assert!(s.contains("$onlyExisting = $false"));
        assert!(s.contains("GetFolderPath('Desktop')") && s.contains("GetFolderPath('Programs')"));
        assert!(!s.contains('"'), "no double quotes, so the command line needs no escaping");
        assert!(super::create_shortcuts_script(spec, exe, true).contains("$onlyExisting = $true"));
        let paths = crate::store::Paths { data: std::path::PathBuf::from("C:/x/WaveHub") };
        let r = super::remove_shortcuts_script(spec, &paths);
        assert!(r.contains("$apps = 'C:/x/WaveHub/apps\\'"), "{r}");
        assert!(!r.contains('"'));
    }

    #[test]
    fn display_icon() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\...\\StreamWave\r\n    DisplayIcon    REG_SZ    \"C:\\Users\\d\\AppData\\Local\\StreamWave\\StreamWave.exe\"\r\n";
        assert_eq!(super::parse_display_icon(out).unwrap().to_string_lossy(), r"C:\Users\d\AppData\Local\StreamWave\StreamWave.exe");
        let out = "    DisplayIcon    REG_SZ    C:\\Program Files\\BigWave Radio\\BigWaveRadio.exe,0";
        assert_eq!(super::parse_display_icon(out).unwrap().to_string_lossy(), r"C:\Program Files\BigWave Radio\BigWaveRadio.exe");
    }
}
