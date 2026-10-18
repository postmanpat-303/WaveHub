//! Wave Hub: one place to install, update and open Del's Wave apps.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod github;
mod install;
mod links;
mod manifest;
mod selfupdate;
mod store;
mod uninstall;

fn main() -> eframe::Result<()> {
    // Small command-line helpers, used by the packaging smoke tests.
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Written with `writeln!` and errors ignored, so `wave-hub --version | head -c1` doesn't
    // panic with "Broken pipe".
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        let _ = writeln!(out, "Wave Hub {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--update-info") {
        // How this copy was installed, and which release files a newer version would come from.
        let kind = selfupdate::detect();
        let version = args.iter().skip_while(|a| *a != "--update-info").nth(1).cloned().unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
        let _ = writeln!(out, "Wave Hub {} ({}, {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
        let _ = writeln!(out, "installed as: {}", kind.label());
        let _ = writeln!(out, "would update from (best first): {}", selfupdate::candidates(kind, &version, std::env::consts::ARCH).join(", "));
        return Ok(());
    }
    if let Some(url) = args.iter().skip_while(|a| *a != "--open-url").nth(1) {
        // Opens a link the same way the window's links do (used by the Windows CI check).
        match links::open_url(url) {
            Ok(()) => {
                let _ = writeln!(out, "opened {url}");
                return Ok(());
            }
            Err(e) => {
                links::log(&format!("couldn't open {url}: {e}"));
                let _ = writeln!(out, "couldn't open {url}: {e}");
                std::process::exit(1);
            }
        }
    }
    if args.iter().any(|a| a == "--list-apps" || a == "--uninstall") {
        // What's installed (by Wave Hub or found on this computer), and removing one.
        let paths = store::Paths::new();
        let manifest = manifest::load(&paths.data);
        let mut installed = store::Installed::load(&paths);
        installed.apps.retain(|id, r| manifest.apps.iter().find(|a| a.id == *id).is_some_and(|a| uninstall::check_files(a, r, &paths) != uninstall::Files::Gone));
        for spec in &manifest.apps {
            if !installed.apps.contains_key(&spec.id) {
                if let Some(r) = uninstall::find_external(spec) {
                    installed.apps.insert(spec.id.clone(), r);
                }
            }
        }
        if let Some(id) = args.iter().skip_while(|a| *a != "--uninstall").nth(1) {
            let Some(spec) = manifest.apps.iter().find(|a| a.id == *id) else {
                let _ = writeln!(out, "No app called {id}.");
                std::process::exit(2);
            };
            let Some(record) = installed.apps.get(id).cloned() else {
                let _ = writeln!(out, "{} isn't installed.", spec.name);
                std::process::exit(1);
            };
            let _ = writeln!(out, "Removing {} {} ({})…", spec.name, record.version, record.kind.label());
            match uninstall::uninstall(spec, &record, &paths) {
                Ok(()) => {
                    installed.apps.remove(id);
                    let _ = installed.save(&paths);
                    let _ = writeln!(out, "Removed {}. Your settings stay.", spec.name);
                }
                Err(e) => {
                    let _ = writeln!(out, "{e}");
                    std::process::exit(1);
                }
            }
        } else {
            for spec in &manifest.apps {
                match installed.apps.get(&spec.id) {
                    Some(r) => {
                        let found = if r.external { ", found on this computer" } else { "" };
                        let missing = if r.path.is_file() { "" } else { " [FILES MISSING]" };
                        let _ = writeln!(out, "{}: {} ({}{found}){missing} {}", spec.id, if r.version.is_empty() { "?" } else { &r.version }, r.kind.label(), r.path.display());
                    }
                    None => {
                        let _ = writeln!(out, "{}: not installed", spec.id);
                    }
                }
            }
        }
        return Ok(());
    }
    drop(out);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/wavehub_256.png")).ok();
    let mut viewport = egui::ViewportBuilder::default().with_title("Wave Hub").with_app_id("wave-hub").with_inner_size([1100.0, 720.0]).with_min_inner_size([440.0, 420.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Wave Hub", options, Box::new(|cc| Ok(Box::new(app::WaveHub::new(cc)))))
}
