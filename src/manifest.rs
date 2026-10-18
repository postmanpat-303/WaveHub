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
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LinuxSpec {
    pub appimage: Option<String>,
    /// Shown instead of an Install button when there is no Linux download.
    pub unavailable: Option<String>,
}

pub fn fill(template: &str, tag: &str) -> String {
    template.replace("{version}", tag.trim_start_matches('v')).replace("{tag}", tag)
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
    }
}
