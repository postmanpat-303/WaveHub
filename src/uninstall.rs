//! Removing apps, and finding apps that were installed without Wave Hub.
//!
//! | how it's installed                    | how it's removed                                   |
//! |---------------------------------------|----------------------------------------------------|
//! | AppImage / portable zip (Wave Hub's)  | delete its folder in Wave Hub's apps folder        |
//! | Windows setup (Wave Hub's or not)     | the app's own uninstaller, quietly (NSIS `/S`,     |
//! |                                       | Inno `/VERYSILENT`, MSI `/qn`), then check it's gone |
//! | Linux .deb / .rpm / Arch package      | the package manager, through `pkexec`              |
//! | macOS `.app` in Applications          | delete the `.app`                                  |
//!
//! Settings and data that the apps keep in their own folders are never touched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::install;
use crate::manifest::AppSpec;
use crate::store::{now, Kind, Paths, Record};

/// What the confirm box says Wave Hub is about to do.
pub fn what_happens(spec: &AppSpec, record: &Record) -> String {
    match record.kind {
        Kind::AppImage | Kind::PortableZip if cfg!(target_os = "macos") => format!("Wave Hub deletes {} from your Applications folder.", spec.name),
        Kind::AppImage | Kind::PortableZip => format!("Wave Hub deletes {} from its apps folder.", spec.name),
        Kind::Installer => format!("Wave Hub runs {}'s own uninstaller. Windows may ask for permission.", spec.name),
        Kind::SystemPackage => "Your system's package manager removes it, and will ask for your password.".to_string(),
        Kind::MacApp => format!("Wave Hub deletes {} from Applications.", spec.name),
    }
}

/// Removes the app. Blocks until it's done (run it on a background thread).
pub fn uninstall(spec: &AppSpec, record: &Record, paths: &Paths) -> Result<(), String> {
    if install::is_running(&record.path) {
        return Err(format!("{} is open. Close it first, then try again.", spec.name));
    }
    match record.kind {
        Kind::AppImage | Kind::PortableZip => {
            // A Mac app Wave Hub put in ~/Applications.
            if let Some(app) = app_bundle(&record.path).filter(|a| !a.starts_with(paths.apps())) {
                remove_folder(&app, spec)?;
            }
            remove_folder(&install::app_dir(spec, paths), spec)
        }
        Kind::Installer => run_uninstaller(spec, record),
        Kind::SystemPackage => remove_package(spec, &record.path),
        Kind::MacApp => match app_bundle(&record.path) {
            Some(app) => remove_folder(&app, spec),
            None => Err(format!("Couldn't find the {}.app folder.", spec.name)),
        },
    }
}

fn remove_folder(dir: &Path, spec: &AppSpec) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("Couldn't remove {} ({e}). Is it open?", spec.name))?;
    }
    Ok(())
}

/// `.../BigWave Radio.app/Contents/MacOS/BigWaveRadio` -> `.../BigWave Radio.app`.
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

// --- Windows: the app's own uninstaller -------------------------------------------------------

/// The uninstaller's command, as the setup program registered it.
#[derive(Debug, PartialEq)]
pub struct UninstallCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Picks the quiet way to run an uninstaller from the registry values of its Uninstall key.
pub fn quiet_command(values: &HashMap<String, String>) -> Option<UninstallCommand> {
    if let Some(q) = values.get("QuietUninstallString").filter(|q| !q.trim().is_empty()) {
        return split_command(q);
    }
    let mut cmd = split_command(values.get("UninstallString")?)?;
    // (Split by hand: these are Windows paths, which Linux's Path wouldn't split on `\`.)
    let program = cmd.program.to_string_lossy().to_ascii_lowercase();
    let name = program.rsplit(['\\', '/']).next().unwrap_or_default().to_string();
    let has = |args: &[String], flag: &str| args.iter().any(|a| a.eq_ignore_ascii_case(flag));
    if name == "msiexec.exe" || name == "msiexec" {
        // `MsiExec.exe /I{GUID}` opens the repair screen; /X removes.
        cmd.args = cmd.args.iter().map(|a| if a.len() > 2 && a[..2].eq_ignore_ascii_case("/i") { format!("/X{}", &a[2..]) } else { a.clone() }).collect();
        if !has(&cmd.args, "/qn") {
            cmd.args.push("/qn".into());
        }
    } else if name.strip_prefix("unins").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit())) {
        // Inno Setup: unins000.exe
        for flag in ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"] {
            if !has(&cmd.args, flag) {
                cmd.args.push(flag.into());
            }
        }
    } else if !has(&cmd.args, "/S") {
        // NSIS (and anything else that follows its convention)
        cmd.args.push("/S".into());
    }
    Some(cmd)
}

/// `"C:\x y\uninstall.exe" /S` -> (`C:\x y\uninstall.exe`, [`/S`]). Unquoted paths with spaces
/// are matched up to `.exe`.
pub fn split_command(s: &str) -> Option<UninstallCommand> {
    let s = s.trim();
    let (program, rest) = if let Some(stripped) = s.strip_prefix('"') {
        let end = stripped.find('"')?;
        (stripped[..end].to_string(), stripped[end + 1..].to_string())
    } else if let Some(i) = s.to_ascii_lowercase().find(".exe") {
        (s[..i + 4].to_string(), s[i + 4..].to_string())
    } else {
        let mut parts = s.splitn(2, ' ');
        (parts.next()?.to_string(), parts.next().unwrap_or("").to_string())
    };
    if program.is_empty() {
        return None;
    }
    Some(UninstallCommand { program: PathBuf::from(program), args: rest.split_whitespace().map(String::from).collect() })
}

/// `reg query` output -> value name -> data (REG_SZ / REG_EXPAND_SZ / REG_DWORD as text).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_reg_values(output: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in output.lines() {
        for ty in ["REG_SZ", "REG_EXPAND_SZ", "REG_DWORD"] {
            let marker = format!("    {ty}    ");
            if let Some(i) = line.find(&marker) {
                let name = line[..i].trim().to_string();
                let value = line[i + marker.len()..].trim().to_string();
                if !name.is_empty() {
                    out.insert(name, value);
                }
                break;
            }
        }
    }
    out
}

/// The app's Uninstall key: (where it was found, its values). Per-user first.
#[cfg(windows)]
fn uninstall_key(key: &str) -> Option<(String, HashMap<String, String>)> {
    use std::os::windows::process::CommandExt;
    for root in [r"HKCU\Software", r"HKLM\Software", r"HKLM\SOFTWARE\WOW6432Node"] {
        let path = format!(r"{root}\Microsoft\Windows\CurrentVersion\Uninstall\{key}");
        let Ok(out) = Command::new("reg").args(["query", &path]).creation_flags(0x0800_0000).output() else {
            continue;
        };
        if out.status.success() {
            let values = parse_reg_values(&String::from_utf8_lossy(&out.stdout));
            if !values.is_empty() {
                return Some((path, values));
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn uninstall_key(_key: &str) -> Option<(String, HashMap<String, String>)> {
    None
}

fn run_uninstaller(spec: &AppSpec, record: &Record) -> Result<(), String> {
    let keys = spec.windows.as_ref().map(|w| w.uninstall_key.clone()).unwrap_or_default();
    let registered = keys.iter().find_map(|k| uninstall_key(k));
    let cmd = registered.as_ref().and_then(|(_, v)| quiet_command(v)).or_else(|| {
        // No registry entry: look next to the program.
        let dir = record.path.parent()?;
        ["Uninstall.exe", "uninstall.exe", &format!("Uninstall {}.exe", spec.name), "unins000.exe"]
            .iter()
            .map(|n| dir.join(n))
            .find(|p| p.is_file())
            .and_then(|p| quiet_command(&HashMap::from([("UninstallString".to_string(), format!("\"{}\"", p.display()))])))
    });
    let Some(cmd) = cmd else {
        return Err(format!("{}'s uninstaller wasn't found. Remove it from Windows Settings > Apps > Installed apps.", spec.name));
    };
    let status = install::installer_command(&cmd.program, &cmd.args).status().map_err(|e| format!("Couldn't start {}'s uninstaller ({e}).", spec.name))?;
    // NSIS uninstallers copy themselves to a temporary folder and return straight away, so
    // wait for the program and its registry entry to really go.
    let gone = || !record.path.exists() && keys.iter().all(|k| uninstall_key(k).is_none());
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if gone() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if !status.success() {
        let code = status.code().map(|c| format!(" (code {c})")).unwrap_or_default();
        return Err(format!("{}'s uninstaller stopped before finishing{code}. If Windows asked for permission and you said no, that's fine: try again.", spec.name));
    }
    Err(format!("{}'s uninstaller finished, but {} is still there. Remove it from Windows Settings > Apps > Installed apps.", spec.name, record.path.display()))
}

// --- Linux: system packages ---------------------------------------------------------------------

/// The package manager and package that own `path`, e.g. ("dpkg", "bigwave-radio", "0.9.6-1").
fn package_of(path: &Path) -> Option<(&'static str, String, String)> {
    let p = path.to_string_lossy().into_owned();
    let run = |prog: &str, args: &[&str]| -> Option<String> {
        let out = Command::new(prog).args(args).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if let Some(out) = run("dpkg-query", &["-S", &p]) {
        let name = out.lines().next()?.split(':').next()?.trim().to_string();
        let version = run("dpkg-query", &["-W", "-f=${Version}", &name]).unwrap_or_default();
        return Some(("dpkg", name, version));
    }
    if let Some(out) = run("rpm", &["-qf", "--qf", "%{NAME} %{VERSION}\n", &p]) {
        let mut it = out.lines().next()?.split_whitespace();
        return Some(("rpm", it.next()?.to_string(), it.next().unwrap_or_default().to_string()));
    }
    if let Some(out) = run("pacman", &["-Qo", &p]) {
        // "/usr/bin/x is owned by name 1.2-1"
        let tail = out.lines().next()?.split(" is owned by ").nth(1)?;
        let mut it = tail.split_whitespace();
        return Some(("pacman", it.next()?.to_string(), it.next().unwrap_or_default().to_string()));
    }
    None
}

/// The command that removes `name` with `manager`.
pub fn removal_command(manager: &str, name: &str) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match manager {
        "dpkg" => v(&["apt-get", "remove", "-y", name]),
        "rpm" => {
            if Path::new("/usr/bin/dnf").exists() {
                v(&["dnf", "remove", "-y", name])
            } else if Path::new("/usr/bin/zypper").exists() {
                v(&["zypper", "--non-interactive", "remove", name])
            } else {
                v(&["rpm", "-e", name])
            }
        }
        _ => v(&["pacman", "-R", "--noconfirm", name]),
    }
}

fn remove_package(spec: &AppSpec, path: &Path) -> Result<(), String> {
    let Some((manager, name, _)) = package_of(path) else {
        return Err(format!("{} is in {}, but no package manager says it installed it. Remove it the way you installed it.", spec.name, path.display()));
    };
    let cmd = removal_command(manager, &name);
    // pkexec shows the system's own password box. Without it, say what to type.
    let pkexec = ["/usr/bin/pkexec", "/bin/pkexec"].into_iter().find(|p| Path::new(p).exists());
    let Some(pkexec) = pkexec else {
        return Err(format!("{} was installed with your system's package manager. To remove it, run in a terminal:  sudo {}", spec.name, cmd.join(" ")));
    };
    let status = Command::new(pkexec).args(&cmd).stdin(Stdio::null()).status().map_err(|e| format!("Couldn't ask for permission ({e})."))?;
    if path.exists() {
        return Err(if status.code() == Some(126) || status.code() == Some(127) {
            "Removing it needs your password. Nothing was changed.".to_string()
        } else {
            format!("The package manager didn't remove {}. To do it yourself: sudo {}", spec.name, cmd.join(" "))
        });
    }
    Ok(())
}

// --- Is it still there? -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Files {
    Ok,
    /// The program file is gone but the install isn't (Windows Security quarantine, a
    /// half-deleted folder): offer Repair.
    Missing,
    /// Uninstalled or deleted outside Wave Hub: forget it.
    Gone,
}

/// Checks that the app's program file is really there.
pub fn check_files(spec: &AppSpec, record: &Record, paths: &Paths) -> Files {
    if record.path.is_file() {
        return Files::Ok;
    }
    let still_installed = match record.kind {
        // Wave Hub's own folder is still there, only the program went.
        Kind::AppImage | Kind::PortableZip => install::app_dir(spec, paths).is_dir() || app_bundle(&record.path).is_some_and(|a| a.is_dir()),
        // The setup's Uninstall entry is still registered, or its folder is still there.
        Kind::Installer => spec.windows.as_ref().is_some_and(|w| w.uninstall_key.iter().any(|k| uninstall_key(k).is_some())) || record.path.parent().is_some_and(Path::is_dir),
        Kind::SystemPackage | Kind::MacApp => false,
    };
    if still_installed && !(record.external && record.kind != Kind::Installer) {
        Files::Missing
    } else {
        Files::Gone
    }
}

// --- Finding apps installed without Wave Hub ----------------------------------------------------

/// A copy of the app on this computer that Wave Hub didn't install, if there is one.
pub fn find_external(spec: &AppSpec) -> Option<Record> {
    let record = |kind, path: PathBuf, version: String| Record { version, tag: String::new(), kind, path, installed_at: now(), external: true };
    if cfg!(windows) {
        let w = spec.windows.as_ref()?;
        let key = w.uninstall_key.iter().find_map(|k| uninstall_key(k));
        // Still registered but the program file has gone (Windows Security quarantine): use
        // the registered icon path, so the card shows FILES MISSING with Repair.
        let registered = || key.as_ref().and_then(|(_, v)| v.get("DisplayIcon")).map(|icon| PathBuf::from(display_icon_path(icon)));
        let path = install::find_installed(&w.installed_exe).or_else(|| w.uninstall_key.iter().find_map(|k| install::registry_exe(k))).or_else(registered)?;
        let version = key.as_ref().and_then(|(_, v)| v.get("DisplayVersion").cloned()).unwrap_or_default();
        return Some(record(Kind::Installer, path, version));
    }
    if cfg!(target_os = "linux") {
        let l = spec.linux.as_ref()?;
        let path = l.system_exe.iter().map(PathBuf::from).find(|p| p.is_file())?;
        // "0.9.6-1" -> "0.9.6": the package's own revision isn't the app's version.
        let version = package_of(&path).map(|(_, _, v)| v.split('-').next().unwrap_or_default().to_string()).unwrap_or_default();
        return Some(record(Kind::SystemPackage, path, version));
    }
    if cfg!(target_os = "macos") {
        let m = spec.macos.as_ref()?;
        let home = dirs::home_dir().unwrap_or_default();
        let exe_in_app = m.exe.as_deref().and_then(|e| e.split_once(".app/")).map(|(_, rest)| rest.to_string());
        for app in &m.installed_app {
            let app = PathBuf::from(app.replacen('~', &home.to_string_lossy(), 1));
            let exe = exe_in_app.as_ref().map(|r| app.join(r)).filter(|p| p.is_file()).or_else(|| install::app_program(&app));
            if let Some(exe) = exe {
                let version = plist_version(&app.join("Contents/Info.plist")).unwrap_or_default();
                return Some(record(Kind::MacApp, exe, version));
            }
        }
    }
    None
}

/// `"C:\\x\\App.exe",0` -> `C:\\x\\App.exe`.
#[cfg_attr(not(windows), allow(dead_code))]
fn display_icon_path(icon: &str) -> String {
    let icon = icon.trim();
    let icon = match icon.rsplit_once(',') {
        Some((p, n)) if n.trim().parse::<i32>().is_ok() => p,
        _ => icon,
    };
    icon.trim().trim_matches('"').to_string()
}

/// CFBundleShortVersionString from an XML Info.plist.
fn plist_version(plist: &Path) -> Option<String> {
    let text = std::fs::read_to_string(plist).ok()?;
    let after = text.split("<key>CFBundleShortVersionString</key>").nth(1)?;
    let v = after.split("<string>").nth(1)?.split("</string>").next()?;
    Some(v.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninstall_commands() {
        let reg = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\StreamWave\r\n    DisplayName    REG_SZ    StreamWave\r\n    DisplayVersion    REG_SZ    0.6.0\r\n    UninstallString    REG_SZ    \"C:\\Users\\d\\AppData\\Local\\StreamWave\\uninstall.exe\"\r\n";
        let v = parse_reg_values(reg);
        assert_eq!(v["DisplayVersion"], "0.6.0");
        let c = quiet_command(&v).unwrap();
        assert_eq!(c.program, PathBuf::from(r"C:\Users\d\AppData\Local\StreamWave\uninstall.exe"));
        assert_eq!(c.args, ["/S"]);
        // Quiet string wins.
        let v = HashMap::from([
            ("UninstallString".into(), "\"C:\\Program Files\\BigWave Radio\\Uninstall.exe\"".into()),
            ("QuietUninstallString".into(), "\"C:\\Program Files\\BigWave Radio\\Uninstall.exe\" /S".into()),
        ]);
        assert_eq!(quiet_command(&v).unwrap().args, ["/S"]);
        // Inno Setup and MSI.
        let v = HashMap::from([("UninstallString".to_string(), "\"C:\\Apps\\X\\unins000.exe\"".to_string())]);
        assert_eq!(quiet_command(&v).unwrap().args, ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]);
        let v = HashMap::from([("UninstallString".to_string(), "MsiExec.exe /I{1234-ABCD}".to_string())]);
        assert_eq!(quiet_command(&v).unwrap().args, ["/X{1234-ABCD}", "/qn"]);
        // Unquoted path with spaces.
        let c = split_command(r"C:\Program Files\App\uninstall.exe /S").unwrap();
        assert_eq!(c.program, PathBuf::from(r"C:\Program Files\App\uninstall.exe"));
    }

    #[test]
    fn bundles_and_packages() {
        assert_eq!(app_bundle(Path::new("/Applications/BigWave Radio.app/Contents/MacOS/BigWaveRadio")).unwrap(), PathBuf::from("/Applications/BigWave Radio.app"));
        assert_eq!(removal_command("dpkg", "bigwave-radio"), ["apt-get", "remove", "-y", "bigwave-radio"]);
        assert_eq!(removal_command("pacman", "bigwave-radio-bin")[0], "pacman");
        assert_eq!(display_icon_path("\"C:\\Program Files\\BigWave Radio\\BigWaveRadio.exe\",0"), r"C:\Program Files\BigWave Radio\BigWaveRadio.exe");
        assert_eq!(display_icon_path(r"C:\Apps\StreamWave.exe"), r"C:\Apps\StreamWave.exe");
    }

    #[test]
    fn missing_files() {
        let m = crate::manifest::load(Path::new("/nonexistent"));
        let spec = m.apps.iter().find(|a| a.id == "streamwave").unwrap();
        let data = std::env::temp_dir().join(format!("wavehub-missing-{}", std::process::id()));
        let paths = Paths { data: data.clone() };
        let dir = install::app_dir(spec, &paths);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("StreamWave.AppImage");
        let rec = Record { version: "0.6.0".into(), tag: "v0.6.0".into(), kind: Kind::AppImage, path: exe.clone(), installed_at: 0, external: false };
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(check_files(spec, &rec, &paths), Files::Ok);
        // Quarantined: the file went, its folder didn't.
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(check_files(spec, &rec, &paths), Files::Missing);
        // Deleted by hand: forget it.
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(check_files(spec, &rec, &paths), Files::Gone);
        // A package-manager install that's gone was uninstalled.
        let sys = Record { kind: Kind::SystemPackage, external: true, ..rec };
        assert_eq!(check_files(spec, &sys, &paths), Files::Gone);
        let _ = std::fs::remove_dir_all(&data);
    }
}
