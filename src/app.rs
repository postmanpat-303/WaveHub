//! The window: a card per app, plus Community and About pages.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use egui::{vec2, Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, Ui};

use crate::github;
use crate::install::{self, Phase, Plan};
use crate::manifest::{self, AppSpec, Manifest};
use crate::store::{Installed, Kind, Paths, Settings};

const BUY_ME_A_COFFEE: &str = "https://buymeacoffee.com/postmanpat";
const PAYPAL: &str = "https://paypal.me/derekh100";

/// Every colour the window uses, for one theme.
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub line: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub info: Color32,
    pub bad: Color32,
    pub soft: Color32,
    pub shadow: Color32,
}

// Dark: deep night-sea blues and engraved-ink white.
const DARK_PALETTE: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(11, 14, 26),
    panel: Color32::from_rgb(16, 20, 36),
    card: Color32::from_rgb(21, 27, 48),
    line: Color32::from_rgb(38, 47, 78),
    text: Color32::from_rgb(236, 239, 248),
    muted: Color32::from_rgb(140, 150, 182),
    good: Color32::from_rgb(70, 214, 140),
    warn: Color32::from_rgb(255, 190, 70),
    info: Color32::from_rgb(110, 170, 255),
    bad: Color32::from_rgb(255, 105, 105),
    soft: Color32::from_rgb(46, 56, 92),
    shadow: Color32::from_black_alpha(0),
};

// Light: warm paper white, soft cards, friendly dark ink.
const LIGHT_PALETTE: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(250, 247, 242),
    panel: Color32::from_rgb(255, 255, 255),
    card: Color32::from_rgb(255, 255, 255),
    line: Color32::from_rgb(236, 230, 220),
    text: Color32::from_rgb(34, 36, 52),
    muted: Color32::from_rgb(110, 112, 132),
    good: Color32::from_rgb(22, 150, 84),
    warn: Color32::from_rgb(201, 112, 0),
    info: Color32::from_rgb(37, 99, 235),
    bad: Color32::from_rgb(214, 52, 52),
    soft: Color32::from_rgb(241, 236, 228),
    shadow: Color32::from_rgba_premultiplied(16, 11, 6, 26),
};

static DARK_MODE: AtomicBool = AtomicBool::new(false);

fn p() -> &'static Palette {
    if DARK_MODE.load(Ordering::Relaxed) { &DARK_PALETTE } else { &LIGHT_PALETTE }
}

/// A rounded card with a soft shadow in light mode, a fine outline in dark mode.
fn card_frame(radius: u8, margin: i8) -> egui::Frame {
    let pal = p();
    let f = egui::Frame::new().fill(pal.card).corner_radius(radius).inner_margin(Margin::same(margin));
    if pal.dark {
        f.stroke(Stroke::new(1.0, pal.line))
    } else {
        f.stroke(Stroke::new(1.0, pal.line)).shadow(egui::Shadow { offset: [0, 6], blur: 22, spread: 0, color: pal.shadow })
    }
}

/// A pale wash of `c` for chips and highlights.
fn tint(c: Color32, amount: f32) -> Color32 {
    let base = p().card;
    let mix = |a: u8, b: u8| (a as f32 * amount + b as f32 * (1.0 - amount)) as u8;
    Color32::from_rgb(mix(c.r(), base.r()), mix(c.g(), base.g()), mix(c.b(), base.b()))
}

const HUB_PINK: Color32 = Color32::from_rgb(251, 37, 134);
const INK: Color32 = Color32::from_rgb(22, 24, 36);
const WAVE_GREEN: Color32 = Color32::from_rgb(52, 196, 22);
const WAVE_BLUE: Color32 = Color32::from_rgb(20, 92, 255);
const WAVE_ORANGE: Color32 = Color32::from_rgb(255, 112, 10);

/// Readable text colour on top of `fill`.
fn on(fill: Color32) -> Color32 {
    let lum = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    if lum > 150.0 { INK } else { Color32::WHITE }
}

fn icon_bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "bigwave_radio" => include_bytes!("../assets/icons/bigwave_radio_128.png"),
        "chipwave_tracker" => include_bytes!("../assets/icons/chipwave_tracker_128.png"),
        "streamwave" => include_bytes!("../assets/icons/streamwave_128.png"),
        "wavehub" => include_bytes!("../assets/icons/wavehub_128.png"),
        _ => return None,
    })
}

#[derive(Clone, Debug)]
enum Latest {
    Checking,
    Found(String),
    NoReleases,
    Error(String),
}

struct Job {
    phase: Phase,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Shared {
    latest: HashMap<String, Latest>,
    jobs: HashMap<String, Job>,
    errors: HashMap<String, String>,
    installed: Installed,
    checked_at: Option<Instant>,
    checking: bool,
    hub_latest: Option<Latest>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Apps,
    Community,
    About,
}

pub struct WaveHub {
    manifest: Manifest,
    paths: Paths,
    settings: Settings,
    shared: Arc<Mutex<Shared>>,
    tab: Tab,
    /// Height of the tallest card in each row last frame, so a row's cards line up.
    row_heights: Vec<f32>,
}

fn lock(s: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

impl WaveHub {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let paths = Paths::new();
        let manifest = manifest::load(&paths.data);
        let mut settings = Settings::load(&paths);
        if let Ok(theme) = std::env::var("WAVEHUB_THEME") {
            settings.theme = theme;
        }
        DARK_MODE.store(settings.theme == "dark", Ordering::Relaxed);
        install_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);
        let mut installed = Installed::load(&paths);
        // Forget installs whose program has gone (deleted by hand, or uninstalled in Windows).
        installed.apps.retain(|_, r| r.path.exists());
        let shared = Arc::new(Mutex::new(Shared { installed, ..Default::default() }));
        let tab = match std::env::var("WAVEHUB_START_TAB").as_deref() {
            Ok("community") => Tab::Community,
            Ok("about") => Tab::About,
            _ => Tab::Apps,
        };
        let hub = Self { manifest, paths, settings, shared, tab, row_heights: Vec::new() };
        hub.check_now(&cc.egui_ctx);
        hub
    }

    /// Looks up the newest release of every app (and of Wave Hub) in the background.
    fn check_now(&self, ctx: &egui::Context) {
        {
            let mut s = lock(&self.shared);
            if s.checking {
                return;
            }
            s.checking = true;
            for app in &self.manifest.apps {
                s.latest.insert(app.id.clone(), Latest::Checking);
            }
            s.hub_latest = Some(Latest::Checking);
        }
        let shared = self.shared.clone();
        let apps = self.manifest.apps.clone();
        let hub_repo = self.manifest.launcher_repo.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for app in apps {
                let latest = match github::latest_tag(&app.repo) {
                    Ok(Some(tag)) => Latest::Found(tag),
                    Ok(None) => Latest::NoReleases,
                    Err(e) => Latest::Error(e),
                };
                lock(&shared).latest.insert(app.id.clone(), latest);
                ctx.request_repaint();
            }
            let hub = match github::latest_tag(&hub_repo) {
                Ok(Some(tag)) => Latest::Found(tag),
                Ok(None) => Latest::NoReleases,
                Err(e) => Latest::Error(e),
            };
            let mut s = lock(&shared);
            s.hub_latest = Some(hub);
            s.checking = false;
            s.checked_at = Some(Instant::now());
            ctx.request_repaint();
        });
    }

    fn check_hub_only(&self, ctx: &egui::Context) {
        lock(&self.shared).hub_latest = Some(Latest::Checking);
        let shared = self.shared.clone();
        let repo = self.manifest.launcher_repo.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let hub = match github::latest_tag(&repo) {
                Ok(Some(tag)) => Latest::Found(tag),
                Ok(None) => Latest::NoReleases,
                Err(e) => Latest::Error(e),
            };
            lock(&shared).hub_latest = Some(hub);
            ctx.request_repaint();
        });
    }

    /// Installs or updates these apps one after another in the background.
    fn start_installs(&self, ctx: &egui::Context, ids: Vec<String>) {
        let mut queued = Vec::new();
        {
            let mut s = lock(&self.shared);
            for id in ids {
                let Some(spec) = self.manifest.apps.iter().find(|a| a.id == id) else { continue };
                let Some(Latest::Found(tag)) = s.latest.get(&id).cloned() else { continue };
                if s.jobs.contains_key(&id) {
                    continue;
                }
                let cancel = Arc::new(AtomicBool::new(false));
                s.jobs.insert(id.clone(), Job { phase: Phase::Checking, cancel: cancel.clone() });
                s.errors.remove(&id);
                queued.push((spec.clone(), tag, cancel));
            }
        }
        if queued.is_empty() {
            return;
        }
        let shared = self.shared.clone();
        let paths = self.paths.clone();
        let prefer_installer = self.settings.prefer_installer;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for (spec, tag, cancel) in queued {
                run_one(&shared, &paths, &spec, &tag, prefer_installer, &cancel, &ctx);
            }
        });
    }

    fn header(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        header_decor(ui);
        ui.horizontal(|ui| {
            if let Some(bytes) = icon_bytes("wavehub") {
                ui.add(egui::Image::from_bytes("bytes://wavehub.png", bytes).fit_to_exact_size(vec2(44.0, 44.0)));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(title_job("Wave Hub", 26.0));
                ui.label(RichText::new("Install, update and open the Wave apps").color(p().muted));
            });
            wave_arcs(ui, HUB_PINK);
            ui.add_space(16.0);
            for (tab, label) in [(Tab::Apps, "Apps"), (Tab::Community, "Community"), (Tab::About, "About")] {
                let selected = self.tab == tab;
                let pill = if p().dark { p().text } else { HUB_PINK };
                let text = RichText::new(label).size(15.0).color(if selected { on(pill) } else { p().text });
                let button = egui::Button::new(text).fill(if selected { pill } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(18.0).min_size(vec2(0.0, 32.0));
                if ui.add(button).clicked() {
                    self.tab = tab;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let updates = self.updatable();
                let label = if updates.is_empty() { "Update all".to_string() } else { format!("Update all ({})", updates.len()) };
                let button = egui::Button::new(RichText::new(label).family(bold()).color(if updates.is_empty() { p().muted } else { Color32::WHITE }))
                    .fill(if updates.is_empty() { p().soft } else { HUB_PINK })
                    .stroke(Stroke::NONE)
                    .corner_radius(18.0)
                    .min_size(vec2(0.0, 34.0));
                if ui.add_enabled(!updates.is_empty(), button).on_hover_text("Update every app that has a newer version").clicked() {
                    self.start_installs(&ctx, updates);
                }
                let checking = lock(&self.shared).checking;
                let refresh = egui::Button::new(RichText::new(if checking { "Checking…" } else { "Check now" }).color(p().text)).fill(p().soft).stroke(Stroke::NONE).corner_radius(18.0).min_size(vec2(0.0, 34.0));
                if ui.add_enabled(!checking, refresh).clicked() {
                    self.check_now(&ctx);
                }
            });
        });
    }

    /// Apps with an update that can be installed here and aren't busy.
    fn updatable(&self) -> Vec<String> {
        let s = lock(&self.shared);
        self.manifest
            .apps
            .iter()
            .filter(|a| !matches!(install::plan(a, self.settings.prefer_installer), Plan::Unavailable(_)))
            .filter(|a| !s.jobs.contains_key(&a.id))
            .filter(|a| match (s.installed.apps.get(&a.id), s.latest.get(&a.id)) {
                (Some(rec), Some(Latest::Found(tag))) => github::is_newer(tag, &rec.version),
                _ => false,
            })
            .map(|a| a.id.clone())
            .collect()
    }

    fn apps_page(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let gap = if p().dark { 16.0 } else { 22.0 };
        let columns = ((width + gap) / (320.0 + gap)).floor().clamp(1.0, 3.0) as usize;
        let card_w = (width - gap * (columns as f32 - 1.0)) / columns as f32;
        let apps = self.manifest.apps.clone();
        let mut heights = Vec::new();
        for (row_index, row) in apps.chunks(columns).enumerate() {
            let row_min = self.row_heights.get(row_index).copied().unwrap_or(0.0);
            // Lay each card out in its own fixed-width column, then move down by the tallest.
            let top_left = ui.cursor().min;
            let mut tallest: f32 = 0.0;
            for (i, spec) in row.iter().enumerate() {
                let min = top_left + vec2(i as f32 * (card_w + gap), 0.0);
                let rect = egui::Rect::from_min_size(min, vec2(card_w, 2000.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
                self.card(&mut child, spec, card_w, row_min);
                tallest = tallest.max(child.min_rect().height());
            }
            heights.push(tallest);
            ui.allocate_space(vec2(width, tallest));
            ui.add_space(gap);
        }
        if heights != self.row_heights {
            self.row_heights = heights;
            ui.ctx().request_repaint();
        }
        ui.add_space(4.0);
        self.welcome_strip(ui);
    }

    /// A friendly nudge under the cards towards the community page.
    fn welcome_strip(&mut self, ui: &mut Ui) {
        let fill = if p().dark { p().card } else { tint(HUB_PINK, 0.08) };
        egui::Frame::new().fill(fill).corner_radius(18).inner_margin(Margin::symmetric(20, 14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(bytes) = icon_bytes("wavehub") {
                    ui.add(egui::Image::from_bytes("bytes://wavehub.png", bytes).fit_to_exact_size(vec2(36.0, 36.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Free audio apps, built with the community").family(bold()).size(15.0).color(p().text));
                    ui.label(RichText::new("Every Wave app is free. Say hi, ask questions and share ideas in Discussions.").color(p().muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let b = egui::Button::new(RichText::new("Join the community").family(bold()).color(Color32::WHITE)).fill(HUB_PINK).stroke(Stroke::NONE).corner_radius(18.0).min_size(vec2(0.0, 36.0));
                    if ui.add(b).clicked() {
                        self.tab = Tab::Community;
                    }
                });
            });
        });
    }

    fn card(&mut self, ui: &mut Ui, spec: &AppSpec, width: f32, row_min: f32) {
        let ctx = ui.ctx().clone();
        let accent = Color32::from_rgb(spec.accent[0], spec.accent[1], spec.accent[2]);
        let plan = install::plan(spec, self.settings.prefer_installer);
        let (latest, record, job, error) = {
            let s = lock(&self.shared);
            (
                s.latest.get(&spec.id).cloned().unwrap_or(Latest::Checking),
                s.installed.apps.get(&spec.id).cloned(),
                s.jobs.get(&spec.id).map(|j| (j.phase.clone(), j.cancel.clone())),
                s.errors.get(&spec.id).cloned(),
            )
        };
        let light = !p().dark;
        let radius: u8 = if light { 20 } else { 14 };
        let margin: i8 = if light { 20 } else { 16 };
        let response = card_frame(radius, margin).show(ui, |ui| {
            ui.set_width(width - 2.0 * margin as f32 - 2.0);
            ui.set_min_height((row_min - 2.0 * margin as f32 - 2.0).max(if light { 284.0 } else { 268.0 }));
            if light {
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                if let Some(bytes) = icon_bytes(&spec.icon) {
                    ui.add(egui::Image::from_bytes(format!("bytes://{}.png", spec.icon), bytes).fit_to_exact_size(vec2(64.0, 64.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(&spec.name).font(FontId::proportional(20.0)).family(bold()).color(p().text));
                    ui.add(egui::Label::new(RichText::new(&spec.description).color(p().muted)).wrap());
                });
            });
            ui.add_space(10.0);

            // Status chip and version line.
            let installed_v = record.as_ref().map(|r| r.version.clone());
            let latest_tag = match &latest {
                Latest::Found(t) => Some(t.clone()),
                _ => None,
            };
            let latest_v = latest_tag.as_deref().map(|t| t.trim_start_matches('v').to_string());
            let update = matches!((&installed_v, &latest_v), (Some(i), Some(l)) if github::is_newer(l, i));
            let (chip, chip_color) = if let Some((phase, _)) = &job {
                (
                    match phase {
                        Phase::Checking => "PREPARING…",
                        Phase::Downloading { .. } => "DOWNLOADING…",
                        Phase::Verifying => "CHECKING DOWNLOAD…",
                        Phase::Installing => "INSTALLING…",
                        Phase::RunningInstaller => "INSTALLER OPEN…",
                    },
                    p().info,
                )
            } else if let Plan::Unavailable(_) = &plan {
                if installed_v.is_some() { ("INSTALLED", p().good) } else { (if cfg!(windows) { "NOT AVAILABLE YET" } else { "WINDOWS ONLY FOR NOW" }, p().muted) }
            } else if update {
                ("UPDATE AVAILABLE", p().warn)
            } else if installed_v.is_some() {
                ("INSTALLED · UP TO DATE", p().good)
            } else {
                match &latest {
                    Latest::Checking => ("CHECKING…", p().muted),
                    Latest::Found(_) => ("READY TO INSTALL", p().info),
                    Latest::NoReleases => ("NO RELEASES YET", p().muted),
                    Latest::Error(_) => ("OFFLINE", p().bad),
                }
            };
            status_chip(ui, chip, chip_color);
            ui.add_space(6.0);
            let latest_text = match &latest {
                Latest::Checking => "checking…".to_string(),
                Latest::Found(_) => latest_v.clone().unwrap_or_default(),
                Latest::NoReleases => "none yet".into(),
                Latest::Error(_) => "couldn't check".into(),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("Installed").color(p().muted));
                ui.label(RichText::new(installed_v.clone().unwrap_or_else(|| "—".into())).color(p().text).family(bold()));
                ui.label(RichText::new("   Latest").color(p().muted));
                ui.label(RichText::new(latest_text).color(p().text).family(bold()));
                if let Some(r) = &record {
                    ui.label(RichText::new(format!("({})", r.kind.label())).color(p().muted).small());
                }
            });
            ui.add_space(10.0);

            // The one main button, or progress.
            let full = vec2(ui.available_width(), 38.0);
            if let Some((phase, cancel)) = &job {
                let (frac, text) = match phase {
                    Phase::Downloading { done, total } if *total > 0 => {
                        (*done as f32 / *total as f32, format!("Downloading {} of {} · {:.0}%", mb(*done), mb(*total), *done as f32 * 100.0 / *total as f32))
                    }
                    Phase::Downloading { done, .. } => (0.0, format!("Downloading {}", mb(*done))),
                    Phase::Checking => (0.0, "Getting ready…".into()),
                    Phase::Verifying => (1.0, "Checking the download is genuine…".into()),
                    Phase::Installing => (1.0, "Installing…".into()),
                    Phase::RunningInstaller => (1.0, "Finish the setup in the installer window".into()),
                };
                ui.label(RichText::new(text).color(p().text));
                ui.horizontal(|ui| {
                    let bar_w = ui.available_width() - 80.0;
                    ui.add(egui::ProgressBar::new(frac).fill(accent).desired_width(bar_w).desired_height(12.0).corner_radius(6.0));
                    let can_cancel = matches!(phase, Phase::Downloading { .. } | Phase::Checking);
                    if ui.add_enabled(can_cancel, egui::Button::new("Cancel").corner_radius(14.0)).clicked() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                });
            } else if let Plan::Unavailable(why) = &plan {
                ui.add(egui::Label::new(RichText::new(why).color(p().muted)).wrap());
                if let Some(r) = &record {
                    if main_button(ui, "OPEN", accent, full, true).clicked() {
                        self.open(spec, &r.path);
                    }
                } else if main_button(ui, "BUILD FROM SOURCE", p().soft, full, true).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(format!("https://github.com/{}#readme", spec.repo)));
                }
            } else if update {
                let label = format!("UPDATE TO {}", latest_v.clone().unwrap_or_default());
                if main_button(ui, &label, accent, full, true).clicked() {
                    self.start_installs(&ctx, vec![spec.id.clone()]);
                }
            } else if let Some(r) = &record {
                if main_button(ui, "OPEN", accent, full, true).clicked() {
                    self.open(spec, &r.path);
                }
            } else {
                let ready = latest_tag.is_some();
                if main_button(ui, "INSTALL", accent, full, ready).clicked() {
                    self.start_installs(&ctx, vec![spec.id.clone()]);
                }
            }
            if let Some(e) = &error {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(e).color(p().bad)).wrap());
            }
            if let Latest::Error(e) = &latest {
                ui.add(egui::Label::new(RichText::new(e).color(p().muted).small()).wrap());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                link(ui, "GitHub", &format!("https://github.com/{}", spec.repo));
                link(ui, "Release notes", &format!("https://github.com/{}/releases/latest", spec.repo));
                link(ui, "Discussions", &format!("https://github.com/{}/discussions", spec.repo));
                if let Some(r) = record.clone() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.menu_button(RichText::new("•••").color(p().muted), |ui| {
                            if update && ui.button(format!("Open {} (current version)", r.version)).clicked() {
                                self.open(spec, &r.path);
                            }
                            if ui.button("Show folder").clicked() {
                                if let Some(dir) = r.path.parent() {
                                    install::reveal(dir);
                                }
                            }
                            if r.kind != Kind::Installer && ui.button(format!("Remove {}", spec.name)).clicked() {
                                self.remove(spec);
                            }
                        });
                    });
                }
            });
        });
        // Accent strip along the top edge, like the coloured app tiles.
        let rect = response.response.rect;
        if light {
            let strip = egui::Rect::from_min_size(rect.min, vec2(rect.width(), 8.0));
            let corners = CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 };
            ui.painter().rect_filled(strip, corners, accent);
        } else {
            let strip = egui::Rect::from_min_size(rect.min + vec2(16.0, 0.0), vec2(56.0, 3.0));
            ui.painter().rect_filled(strip, CornerRadius::same(2), accent);
        }
    }

    fn open(&self, spec: &AppSpec, path: &std::path::Path) {
        let result = install::open(path);
        let mut s = lock(&self.shared);
        match result {
            Ok(()) => {
                s.errors.remove(&spec.id);
            }
            Err(e) => {
                s.errors.insert(spec.id.clone(), format!("Couldn't open {}: {e}", spec.name));
            }
        }
    }

    fn remove(&self, spec: &AppSpec) {
        let mut s = lock(&self.shared);
        let Some(record) = s.installed.apps.get(&spec.id).cloned() else { return };
        match install::remove(spec, &record, &self.paths) {
            Ok(()) => {
                s.installed.apps.remove(&spec.id);
                let _ = s.installed.save(&self.paths);
                s.errors.remove(&spec.id);
            }
            Err(e) => {
                s.errors.insert(spec.id.clone(), e);
            }
        }
    }

    fn community_page(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Join the community").font(FontId::proportional(22.0)).family(bold()).color(p().text));
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new("Every Wave app is free. Ask questions, share ideas and show what you've made in each app's Discussions on GitHub.").color(p().muted)).wrap());
        ui.add_space(12.0);
        for spec in self.manifest.apps.clone() {
            let accent = Color32::from_rgb(spec.accent[0], spec.accent[1], spec.accent[2]);
            card_frame(16, 14).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if let Some(bytes) = icon_bytes(&spec.icon) {
                        ui.add(egui::Image::from_bytes(format!("bytes://{}.png", spec.icon), bytes).fit_to_exact_size(vec2(40.0, 40.0)));
                    }
                    ui.label(RichText::new(&spec.name).size(17.0).family(bold()).color(p().text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let b = egui::Button::new(RichText::new("Discussions").family(bold()).color(on(accent))).fill(accent).stroke(Stroke::NONE).corner_radius(16.0).min_size(vec2(0.0, 32.0));
                        if ui.add(b).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://github.com/{}/discussions", spec.repo)));
                        }
                        link(ui, "Report a problem", &format!("https://github.com/{}/issues", spec.repo));
                        link(ui, "Releases", &format!("https://github.com/{}/releases", spec.repo));
                    });
                });
            });
            ui.add_space(if p().dark { 8.0 } else { 14.0 });
        }
        ui.add_space(8.0);
        support_box(ui, false);
    }

    fn about_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if let Some(bytes) = icon_bytes("wavehub") {
                ui.add(egui::Image::from_bytes("bytes://wavehub.png", bytes).fit_to_exact_size(vec2(96.0, 96.0)));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("Wave Hub").font(FontId::proportional(26.0)).family(bold()).color(p().text));
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(p().muted));
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new("Wave Hub is free and always will be. No account, no ads, no tracking. It installs and updates BigWave Radio, ChipWave Tracker and StreamWave on Windows and Linux, straight from their official GitHub releases.").color(p().text)).wrap());
            });
        });
        ui.add_space(14.0);
        section(ui, "Updates", |ui| {
            let hub = lock(&self.shared).hub_latest.clone();
            let current = env!("CARGO_PKG_VERSION");
            let (text, color) = match &hub {
                None | Some(Latest::Checking) => ("Checking for a newer Wave Hub…".to_string(), p().muted),
                Some(Latest::Found(tag)) if github::is_newer(tag, current) => (format!("Wave Hub {} is available.", tag.trim_start_matches('v')), p().warn),
                Some(Latest::Found(_)) => ("You have the newest Wave Hub.".to_string(), p().good),
                Some(Latest::NoReleases) => ("No Wave Hub releases have been published yet, so this is the newest.".to_string(), p().muted),
                Some(Latest::Error(e)) => (format!("Couldn't check: {e}"), p().bad),
            };
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Check for updates").corner_radius(8.0)).clicked() {
                    self.check_hub_only(&ctx);
                }
                ui.label(RichText::new(text).color(color));
                if matches!(&hub, Some(Latest::Found(tag)) if github::is_newer(tag, current)) {
                    link(ui, "Download", &format!("https://github.com/{}/releases/latest", self.manifest.launcher_repo));
                }
            });
        });
        section(ui, "Settings", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Appearance").color(p().text));
                let mut theme = self.settings.theme.clone();
                let mut changed = false;
                changed |= ui.selectable_value(&mut theme, "light".to_string(), "  Light  ").changed();
                changed |= ui.selectable_value(&mut theme, "dark".to_string(), "  Dark  ").changed();
                if changed && theme != self.settings.theme {
                    self.settings.theme = theme;
                    let _ = self.settings.save(&self.paths);
                    DARK_MODE.store(self.settings.theme == "dark", Ordering::Relaxed);
                    apply_theme(&ctx);
                }
            });
            ui.add_space(4.0);
            if cfg!(windows) {
                let mut v = self.settings.prefer_installer;
                if ui.checkbox(&mut v, "Use each app's official installer instead of the portable version").changed() {
                    self.settings.prefer_installer = v;
                    let _ = self.settings.save(&self.paths);
                }
                ui.label(RichText::new("The portable version needs no admin rights and lives in Wave Hub's folder. StreamWave always uses its installer.").color(p().muted).small());
            } else {
                ui.label(RichText::new("On Linux, apps are installed as AppImages in your home folder. No password needed.").color(p().muted));
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Apps folder:").color(p().muted));
                ui.label(RichText::new(self.paths.apps().display().to_string()).monospace().color(p().text));
                if ui.button("Open folder").clicked() {
                    install::reveal(&self.paths.apps());
                }
            });
        });
        support_box(ui, true);
        ui.add_space(10.0);
        ui.label(RichText::new("MIT License · © 2026 Derek Heasley").color(p().muted).small());
    }

    fn footer(&self, ui: &mut Ui) {
        let s = lock(&self.shared);
        let checked = match s.checked_at {
            _ if s.checking => "Checking GitHub…".to_string(),
            Some(t) => {
                let mins = t.elapsed().as_secs() / 60;
                if mins == 0 { "Checked just now".into() } else { format!("Checked {mins} min ago") }
            }
            None => "Not checked yet".into(),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{checked} · Installing for {} · Free and open source", install::os_label())).color(p().muted).small());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                link(ui, "Buy Me a Coffee", BUY_ME_A_COFFEE);
            });
        });
    }
}

/// Runs one install on the current (background) thread and records the outcome.
fn run_one(shared: &Arc<Mutex<Shared>>, paths: &Paths, spec: &AppSpec, tag: &str, prefer_installer: bool, cancel: &AtomicBool, ctx: &egui::Context) {
    let plan = install::plan(spec, prefer_installer);
    let previous = lock(shared).installed.apps.get(&spec.id).cloned();
    let mut last_paint = Instant::now();
    let result = install::install(spec, tag, &plan, paths, previous.as_ref(), cancel, &mut |phase| {
        let repaint = !matches!(phase, Phase::Downloading { .. }) || last_paint.elapsed().as_millis() > 50;
        if let Some(job) = lock(shared).jobs.get_mut(&spec.id) {
            job.phase = phase;
        }
        if repaint {
            last_paint = Instant::now();
            ctx.request_repaint();
        }
    });
    let mut s = lock(shared);
    s.jobs.remove(&spec.id);
    match result {
        Ok(record) => {
            s.installed.apps.insert(spec.id.clone(), record);
            if let Err(e) = s.installed.save(paths) {
                s.errors.insert(spec.id.clone(), format!("Installed, but couldn't save the list of installed apps ({e})."));
            }
        }
        Err(e) if e == "Cancelled." => {}
        Err(e) => {
            s.errors.insert(spec.id.clone(), e);
        }
    }
    ctx.request_repaint();
}

impl eframe::App for WaveHub {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("header").frame(egui::Frame::new().fill(p().panel).inner_margin(Margin::symmetric(24, if p().dark { 12 } else { 16 }))).show(ui, |ui| self.header(ui));
        egui::Panel::bottom("footer").frame(egui::Frame::new().fill(p().panel).inner_margin(Margin::symmetric(20, 8))).show(ui, |ui| self.footer(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p().bg).inner_margin(Margin::same(if p().dark { 20 } else { 26 }))).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Apps => self.apps_page(ui),
                Tab::Community => self.community_page(ui),
                Tab::About => self.about_page(ui),
            });
        });
        // Keep "Checked n min ago" fresh.
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(30));
    }
}

fn bold() -> egui::FontFamily {
    egui::FontFamily::Name("bold".into())
}

/// Poppins (as on the Wave Family banner), with egui's own fonts behind it for symbols.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("poppins".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Poppins-Regular.ttf"))));
    fonts.font_data.insert("poppins-semibold".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Poppins-SemiBold.ttf"))));
    let fallbacks = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "poppins".into());
    let mut b = vec!["poppins-semibold".to_string()];
    b.extend(fallbacks);
    fonts.families.insert(bold(), b);
    ctx.set_fonts(fonts);
}

fn apply_theme(ctx: &egui::Context) {
    let pal = p();
    let mut v = if pal.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = pal.bg;
    v.window_fill = pal.panel;
    v.faint_bg_color = pal.card;
    v.hyperlink_color = pal.info;
    v.selection.bg_fill = if pal.dark { HUB_PINK.gamma_multiply(0.6) } else { tint(HUB_PINK, 0.25) };
    v.selection.stroke = Stroke::new(1.0, pal.text);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.line);
    v.override_text_color = None;
    if pal.dark {
        v.extreme_bg_color = Color32::from_rgb(8, 10, 20);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(32, 40, 68);
        v.widgets.inactive.bg_fill = Color32::from_rgb(32, 40, 68);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 54, 90);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(54, 66, 108);
    } else {
        v.extreme_bg_color = Color32::from_rgb(240, 235, 227);
        v.widgets.inactive.weak_bg_fill = pal.soft;
        v.widgets.inactive.bg_fill = pal.soft;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(232, 225, 214);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(220, 210, 196));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.text);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(222, 213, 200);
        v.widgets.active.fg_stroke = Stroke::new(1.0, pal.text);
        v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: pal.shadow };
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|style| {
        let light = !pal.dark;
        style.spacing.item_spacing = if light { vec2(10.0, 7.0) } else { vec2(8.0, 6.0) };
        style.spacing.button_padding = vec2(14.0, 6.0);
        style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
    });
}

/// "Wave Hub" in the Wave Family gradient (green → blue → orange → pink), as on the banner.
fn title_job(text: &str, size: f32) -> egui::text::LayoutJob {
    let stops = [WAVE_GREEN, Color32::from_rgb(20, 160, 200), WAVE_BLUE, WAVE_ORANGE, HUB_PINK];
    let n = text.chars().count().max(2) - 1;
    let mut job = egui::text::LayoutJob::default();
    for (i, ch) in text.chars().enumerate() {
        let t = i as f32 / n as f32 * (stops.len() - 1) as f32;
        let k = (t.floor() as usize).min(stops.len() - 2);
        let f = t - k as f32;
        let (a, b) = (stops[k], stops[k + 1]);
        let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f) as u8;
        let color = if p().dark { Color32::from_rgb(mix(a.r(), b.r()).saturating_add(30), mix(a.g(), b.g()).saturating_add(30), mix(a.b(), b.b()).saturating_add(30)) } else { Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b())) };
        job.append(&ch.to_string(), 0.0, egui::TextFormat { font_id: FontId::new(size, bold()), color, ..Default::default() });
    }
    job
}

/// Behind the header: faint sound-wave rings and three gentle waves along the bottom edge,
/// in the apps' colours, like the Wave Family banner.
fn header_decor(ui: &mut Ui) {
    let rect = ui.max_rect().expand2(vec2(24.0, 16.0));
    let painter = ui.painter().with_clip_rect(rect);
    let strength = if p().dark { 0.35 } else { 1.0 };
    // Rings radiating from the middle of the header.
    let center = egui::pos2(rect.center().x + 40.0, rect.center().y);
    let ring_colors = [WAVE_GREEN, WAVE_BLUE, WAVE_ORANGE, HUB_PINK];
    for i in 0..8 {
        let r = 40.0 + i as f32 * 26.0;
        let c = ring_colors[i % 4];
        let alpha = ((46.0 - i as f32 * 4.5) * strength) as u8;
        for side in [-1.0f32, 1.0] {
            let pts: Vec<egui::Pos2> = (0..=24)
                .map(|k| {
                    let a = -0.55 + 1.1 * k as f32 / 24.0;
                    center + vec2(side * r * a.cos(), r * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha))));
        }
    }
    // Waves along the bottom.
    for (j, c) in [WAVE_GREEN, WAVE_BLUE, WAVE_ORANGE].iter().enumerate() {
        let phase = j as f32 * 2.1;
        let pts: Vec<egui::Pos2> = (0..=120)
            .map(|k| {
                let x = rect.left() + rect.width() * k as f32 / 120.0;
                let y = rect.bottom() - 7.0 + 4.0 * ((x / 90.0) + phase).sin() + 2.0 * ((x / 37.0) - phase).sin();
                egui::pos2(x, y)
            })
            .collect();
        let alpha = (110.0 * strength) as u8;
        painter.add(egui::Shape::line(pts, Stroke::new(1.6, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha))));
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

fn main_button(ui: &mut Ui, label: &str, fill: Color32, size: egui::Vec2, enabled: bool) -> egui::Response {
    // Painted by hand so the label is centred and the colours stay ours.
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let mut text_color = if fill == p().soft { p().text } else { on(fill) };
    let mut bg = fill;
    if !enabled {
        bg = p().soft;
        text_color = p().muted;
    } else if response.is_pointer_button_down_on() {
        bg = fill.linear_multiply(0.8);
    } else if response.hovered() {
        bg = Color32::from_rgb(fill.r().saturating_add(18), fill.g().saturating_add(18), fill.b().saturating_add(18));
    }
    if ui.is_rect_visible(rect) {
        let radius = if p().dark { 10 } else { (size.y / 2.0) as u8 };
        if !p().dark && enabled {
            // A soft coloured glow under cheerful buttons.
            let glow = egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 70) };
            ui.painter().add(glow.as_shape(rect, CornerRadius::same(radius)));
        }
        ui.painter().rect_filled(rect, CornerRadius::same(radius), bg);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::new(15.0, bold()), text_color);
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn status_chip(ui: &mut Ui, text: &str, color: Color32) {
    let frame = if p().dark {
        egui::Frame::new().stroke(Stroke::new(1.0, color)).corner_radius(4.0).inner_margin(Margin::symmetric(6, 2))
    } else {
        egui::Frame::new().fill(tint(color, 0.13)).corner_radius(10.0).inner_margin(Margin::symmetric(9, 3))
    };
    frame.show(ui, |ui| {
        ui.label(RichText::new(text).size(11.0).family(bold()).color(color));
    });
}

fn link(ui: &mut Ui, text: &str, url: &str) {
    let r = ui.add(egui::Label::new(RichText::new(text).color(p().muted).size(12.5).underline()).sense(Sense::click())).on_hover_text(url).on_hover_cursor(egui::CursorIcon::PointingHand);
    if r.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    card_frame(16, if p().dark { 14 } else { 18 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(16.0).family(bold()).color(p().text));
        ui.add_space(6.0);
        body(ui);
    });
    ui.add_space(if p().dark { 10.0 } else { 16.0 });
}

fn support_box(ui: &mut Ui, with_paypal: bool) {
    section(ui, "Support Del's work", |ui| {
        ui.add(egui::Label::new(RichText::new("All Wave apps are free. If they make your day better, a coffee keeps the updates coming. Totally optional.").color(p().muted)).wrap());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let coffee = egui::Button::new(RichText::new("Buy Me a Coffee").family(bold()).color(INK)).fill(Color32::from_rgb(255, 221, 0)).stroke(Stroke::NONE).corner_radius(16.0).min_size(vec2(0.0, 32.0));
            if ui.add(coffee).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(BUY_ME_A_COFFEE));
            }
            if with_paypal {
                let paypal = egui::Button::new(RichText::new("PayPal").family(bold()).color(Color32::WHITE)).fill(Color32::from_rgb(0, 112, 186)).stroke(Stroke::NONE).corner_radius(16.0).min_size(vec2(0.0, 32.0));
                if ui.add(paypal).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(PAYPAL));
                }
            }
        });
    });
}

/// Three sound-wave arcs, as on the app icons.
fn wave_arcs(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 44.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.left_center();
    for (i, r) in [9.0f32, 16.0, 23.0].iter().enumerate() {
        let points: Vec<egui::Pos2> = (0..=16)
            .map(|k| {
                let a = -0.8 + 1.6 * k as f32 / 16.0;
                center + vec2(r * a.cos(), r * a.sin())
            })
            .collect();
        let alpha = 1.0 - i as f32 * 0.25;
        painter.add(egui::Shape::line(points, Stroke::new(2.5, color.gamma_multiply(alpha))));
    }
}
