//! Opening web links in the default browser.
//!
//! eframe only opens links itself with its "links" feature, which Wave Hub 0.1–0.2.1 didn't
//! turn on, so every link silently did nothing. Wave Hub now opens them itself:
//! Windows `ShellExecuteW("open")` (no `cmd /C start`, whose `&` handling breaks URLs),
//! Linux `xdg-open` (then `gio open`), macOS `open`. Failures go to `wavehub.log` in Wave Hub's
//! data folder and are shown in the window with the address to copy.

use std::sync::Mutex;

/// The last link that couldn't be opened, and why; the window shows it.
pub static FAILED: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Opens `url` in the default browser, recording a failure for the window to show.
pub fn open(url: &str) {
    if let Err(e) = open_url(url) {
        log(&format!("couldn't open {url}: {e}"));
        *FAILED.lock().unwrap_or_else(|p| p.into_inner()) = Some((url.to_string(), e));
    }
}

/// Appends a line to `wavehub.log` (and stderr).
pub fn log(line: &str) {
    use std::io::Write;
    eprintln!("{line}");
    let path = crate::store::Paths::new().data.join("wavehub.log");
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {line}", crate::store::now());
    }
}

/// Only web and mail links: never hand an arbitrary string to the shell.
fn allowed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https://", "http://", "mailto:"].iter().any(|p| lower.starts_with(p)) || (cfg!(debug_assertions) || std::env::var_os("WAVEHUB_TEST_LINKS").is_some()) && lower.starts_with("wavehub-test:")
}

#[cfg(windows)]
pub fn open_url(url: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    if !allowed(url) {
        return Err("not a web address".into());
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(hwnd: isize, op: *const u16, file: *const u16, params: *const u16, dir: *const u16, show: i32) -> isize;
    }
    let wide = |s: &str| std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let (op, file) = (wide("open"), wide(url));
    // SW_SHOWNORMAL = 1. Returns a value > 32 on success.
    let r = unsafe { ShellExecuteW(0, op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    if r > 32 {
        Ok(())
    } else {
        let why = match r {
            2 | 3 => "no program is set to open web links",
            5 => "access denied",
            31 => "no browser is set as the default for web links",
            _ => "Windows couldn't open it",
        };
        Err(format!("{why} (ShellExecute code {r})"))
    }
}

#[cfg(not(windows))]
pub fn open_url(url: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};
    if !allowed(url) {
        return Err("not a web address".into());
    }
    let tries: &[(&str, &[&str])] = if cfg!(target_os = "macos") { &[("open", &[])] } else { &[("xdg-open", &[]), ("gio", &["open"])] };
    let mut errors = Vec::new();
    for (prog, pre) in tries {
        // The URL is one argument, never parsed by a shell. Not waited for: the browser may
        // keep running.
        match Command::new(prog).args(*pre).arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(mut child) => {
                // xdg-open returns quickly; report if it said it couldn't.
                std::thread::spawn(move || {
                    if let Ok(status) = child.wait() {
                        if !status.success() {
                            log(&format!("{prog} exited with {status}"));
                        }
                    }
                });
                return Ok(());
            }
            Err(e) => errors.push(format!("{prog}: {e}")),
        }
    }
    Err(format!("couldn't start a browser ({})", errors.join("; ")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_web_links() {
        assert!(super::allowed("https://github.com/postmanpat-303/WaveHub/discussions?a=1&b=2"));
        assert!(super::allowed("HTTPS://buymeacoffee.com/postmanpat"));
        assert!(!super::allowed("file:///etc/passwd"));
        assert!(!super::allowed("calc.exe"));
    }
}
