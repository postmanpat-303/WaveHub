//! Wave Hub: one place to install, update and open Del's Wave apps.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod github;
mod install;
mod manifest;
mod store;

fn main() -> eframe::Result<()> {
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
