//! Wave Hub: one place to install, update and open Del's Wave apps.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod github;
mod install;
mod manifest;
mod selfupdate;
mod store;

fn main() -> eframe::Result<()> {
    // Small command-line helpers, used by the packaging smoke tests.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Wave Hub {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--update-info") {
        // How this copy was installed, and which release files a newer version would come from.
        let kind = selfupdate::detect();
        let version = args.iter().skip_while(|a| *a != "--update-info").nth(1).cloned().unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
        println!("Wave Hub {} ({}, {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
        println!("installed as: {}", kind.label());
        println!("would update from (best first): {}", selfupdate::candidates(kind, &version, std::env::consts::ARCH).join(", "));
        return Ok(());
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/wavehub_256.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Wave Hub")
        .with_app_id("wave-hub")
        .with_inner_size([1100.0, 720.0])
        .with_min_inner_size([440.0, 420.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Wave Hub", options, Box::new(|cc| Ok(Box::new(app::WaveHub::new(cc)))))
}
