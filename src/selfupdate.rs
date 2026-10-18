//! Which of Wave Hub's own release files this computer should get.
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

use std::path::Path;

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
        }
    }
}

/// How the running Wave Hub was installed, from where it runs.
pub fn detect() -> InstallKind {
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
    };
    // 0.1.0's names.
    match kind {
        InstallKind::WindowsSetup | InstallKind::WindowsPortable => out.push("WaveHub.exe".into()),
        InstallKind::MacOs => {}
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
}
