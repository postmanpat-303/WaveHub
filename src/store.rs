//! Wave Hub's folders and the small JSON files it keeps there.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Paths {
    /// `~/.local/share/WaveHub` or `%LOCALAPPDATA%\WaveHub`.
    pub data: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
        Self { data: base.join("WaveHub") }
    }
    /// Where portable apps and AppImages go: one folder per app.
    pub fn apps(&self) -> PathBuf {
        self.data.join("apps")
    }
    pub fn downloads(&self) -> PathBuf {
        self.data.join("downloads")
    }
    fn installed_file(&self) -> PathBuf {
        self.data.join("installed.json")
    }
    fn settings_file(&self) -> PathBuf {
        self.data.join("settings.json")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    AppImage,
    PortableZip,
    Installer,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::AppImage => "AppImage",
            Kind::PortableZip => "portable",
            Kind::Installer => "installer",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub version: String,
    pub tag: String,
    pub kind: Kind,
    /// The program to start.
    pub path: PathBuf,
    /// Unix seconds.
    pub installed_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Installed {
    pub apps: BTreeMap<String, Record>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Windows: use the official setup program instead of the portable zip when both exist.
    pub prefer_installer: bool,
    /// "light" (the default) or "dark".
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { prefer_installer: false, theme: "light".into() }
    }
}

fn read<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Write to a temporary file and rename, so a crash never leaves half a file.
fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

impl Installed {
    pub fn load(paths: &Paths) -> Self {
        read(&paths.installed_file())
    }
    pub fn save(&self, paths: &Paths) -> Result<(), String> {
        write(&paths.installed_file(), self)
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        read(&paths.settings_file())
    }
    pub fn save(&self, paths: &Paths) -> Result<(), String> {
        write(&paths.settings_file(), self)
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
