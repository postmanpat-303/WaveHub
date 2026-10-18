//! The list of apps Wave Hub knows about.
//!
//! The built-in list is `apps.json` at the root of the project, compiled into the program. To try
//! a new app without rebuilding, put an `apps.json` in Wave Hub's data folder; it replaces the
//! built-in list.

use serde::Deserialize;

const BUILT_IN: &str = include_str!("../apps.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    /// Where Wave Hub's own releases live (for "Check for updates").
    pub launcher_repo: String,
    pub apps: Vec<AppSpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AppSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    /// `owner/name` on GitHub.
    pub repo: String,
    /// Name of a built-in icon (see `ui::icon_bytes`).
    #[serde(default)]
    pub icon: String,
    /// Card accent colour.
    #[serde(default = "default_accent")]
    pub accent: [u8; 3],
    /// Checksum list published with each release, if any.
    #[serde(default)]
    pub sums: Option<String>,
    #[serde(default)]
    pub windows: Option<WindowsSpec>,
    #[serde(default)]
    pub linux: Option<LinuxSpec>,
    #[serde(default)]
    pub macos: Option<MacSpec>,
}

fn default_accent() -> [u8; 3] {
    [236, 32, 130]
}

/// Release file names may contain `{version}` (the tag without a leading `v`) and `{tag}`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct WindowsSpec {
    /// Portable zip with the program at its root.
    pub portable: Option<String>,
    /// The program inside the portable zip.
    pub exe: Option<String>,
    /// The official setup program.
    pub installer: Option<String>,
    /// Where the official installer puts the program. `{localappdata}` and `{programfiles}` are
    /// filled in.
    #[serde(default)]
    pub installed_exe: Vec<String>,
    /// Arguments that make the official installer run without questions (NSIS: `/S`).
    #[serde(default)]
    pub silent_args: Vec<String>,
    /// The installer's key under `...\CurrentVersion\Uninstall`, used to find the program
    /// if it isn't in one of the `installed_exe` places.
    #[serde(default)]
    pub uninstall_key: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LinuxSpec {
    /// May contain `{arch}` (`x86_64` or `aarch64`).
    pub appimage: Option<String>,
    /// Processors the AppImage is built for.
    #[serde(default = "default_arches")]
    pub arches: Vec<String>,
    /// Where the app's own .deb / .rpm / Arch package puts the program, so Wave Hub can
    /// show it as installed (and remove it with the package manager).
    #[serde(default)]
    pub system_exe: Vec<String>,
    /// Shown instead of an Install button when there is no Linux download.
    pub unavailable: Option<String>,
}

fn default_arches() -> Vec<String> {
    vec!["x86_64".into()]
}

/// A zip with the `.app` at its root (as `ditto -c -k --keepParent` makes it).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct MacSpec {
    pub zip: Option<String>,
    /// The program inside the zip, e.g. `BigWave Radio.app/Contents/MacOS/BigWaveRadio`.
    pub exe: Option<String>,
    pub unavailable: Option<String>,
    /// Where the app's own .dmg puts it, e.g. `/Applications/BigWave Radio.app`; `~` is the
    /// home folder. Lets Wave Hub show a copy installed by hand.
    #[serde(default)]
    pub installed_app: Vec<String>,
}

pub fn fill(template: &str, tag: &str) -> String {
    template.replace("{version}", tag.trim_start_matches('v')).replace("{tag}", tag).replace("{arch}", std::env::consts::ARCH)
}

pub fn load(data_dir: &std::path::Path) -> Manifest {
    let custom = data_dir.join("apps.json");
    if let Ok(text) = std::fs::read_to_string(&custom) {
        match serde_json::from_str::<Manifest>(&text) {
            Ok(m) => return m,
            Err(e) => eprintln!("ignoring {}: {e}", custom.display()),
        }
    }
    serde_json::from_str(BUILT_IN).expect("built-in apps.json is valid")
}

#[cfg(test)]
mod tests {
    #[test]
    fn built_in_manifest_parses() {
        let m: super::Manifest = serde_json::from_str(super::BUILT_IN).unwrap();
        assert_eq!(m.apps.len(), 3);
        assert_eq!(super::fill("A-{version}-x.zip", "v0.9.6"), "A-0.9.6-x.zip");
        assert_eq!(super::fill("A-{version}-linux-{arch}.AppImage", "v1"), format!("A-1-linux-{}.AppImage", std::env::consts::ARCH));
        assert!(m.apps[0].linux.as_ref().unwrap().arches.contains(&"aarch64".to_string()));
        assert_eq!(m.apps[2].linux.as_ref().unwrap().arches, ["x86_64"]);
    }
}
