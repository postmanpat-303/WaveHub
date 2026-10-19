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
    /// A short announcement shown on the app's card, e.g. "New name! Formerly X. Same app,
    /// nothing to do." Text up to the first "! " is shown in bold.
    #[serde(default)]
    pub notice: Option<String>,
    /// Card accent colour.
    #[serde(default = "default_accent")]
    pub accent: [u8; 3],
    /// Checksum lists to look for in each release, first found wins. Downloads are checked
    /// against it when the release has one.
    #[serde(default = "default_sums", deserialize_with = "names")]
    pub sums: Vec<String>,
    #[serde(default)]
    pub windows: Option<WindowsSpec>,
    #[serde(default)]
    pub linux: Option<LinuxSpec>,
    #[serde(default)]
    pub macos: Option<MacSpec>,
}

fn default_sums() -> Vec<String> {
    vec!["SHA256SUMS.txt".into(), "SHA256SUMS".into()]
}

/// A release file name, or a list of names to try in order (the first one the release has
/// is used), so a release that renames its files or adds a new one doesn't need a new
/// Wave Hub.
fn names<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
        None(()),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
        OneOrMany::None(()) => Vec::new(),
    })
}

fn default_accent() -> [u8; 3] {
    [236, 32, 130]
}

/// Release file names may contain `{version}` (the tag without a leading `v`) and `{tag}`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct WindowsSpec {
    /// Portable zip with the program at its root.
    #[serde(default, deserialize_with = "names")]
    pub portable: Vec<String>,
    /// The program inside the portable zip, or names to try in order.
    #[serde(default, deserialize_with = "names")]
    pub exe: Vec<String>,
    /// The official setup program.
    #[serde(default, deserialize_with = "names")]
    pub installer: Vec<String>,
    /// Where the official installer puts the program. `{localappdata}` and `{programfiles}` are
    /// filled in.
    #[serde(default)]
    pub installed_exe: Vec<String>,
    /// Arguments that make the official installer run without questions (NSIS: `/S`).
    #[serde(default)]
    pub silent_args: Vec<String>,
    /// The installer's key under `...\CurrentVersion\Uninstall`, used to find the program
    /// if it isn't in one of the `installed_exe` places. A list when the app's installer has
    /// used more than one (e.g. before and after a rename); the first one registered wins.
    #[serde(default, deserialize_with = "names")]
    pub uninstall_key: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LinuxSpec {
    /// May contain `{arch}` (`x86_64` or `aarch64`).
    #[serde(default, deserialize_with = "names")]
    pub appimage: Vec<String>,
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

/// A zip with the `.app` at its root (as `ditto -c -k --keepParent` makes it). Wave Hub
/// installs the `.app` into `~/Applications` (just for this user, no admin password).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct MacSpec {
    #[serde(default, deserialize_with = "names")]
    pub zip: Vec<String>,
    /// The program inside the zip, e.g. `BigWave Radio.app/Contents/MacOS/BigWaveRadio`.
    /// Optional: without it, the zip's `.app` is found and its Info.plist says which program
    /// to start.
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
    fn names_one_or_many() {
        let m: super::Manifest = serde_json::from_str(r#"{"launcher_repo":"a/b","community_url":"x","apps":[{"id":"x","name":"X","description":"","repo":"a/x","macos":{"zip":["X-{version}.zip","X.zip"]},"windows":{"portable":"X-{version}.zip","exe":"X.exe"},"linux":{"appimage":null}}]}"#).unwrap();
        let a = &m.apps[0];
        assert_eq!(a.macos.as_ref().unwrap().zip, ["X-{version}.zip", "X.zip"]);
        assert_eq!(a.windows.as_ref().unwrap().portable, ["X-{version}.zip"]);
        assert!(a.linux.as_ref().unwrap().appimage.is_empty());
        assert_eq!(a.sums, ["SHA256SUMS.txt", "SHA256SUMS"]);
    }

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
