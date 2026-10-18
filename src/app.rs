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

// The wave palette: deep night-sea blues, engraved-ink white, and each app's own colour.
const BG: Color32 = Color32::from_rgb(11, 14, 26);
const PANEL: Color32 = Color32::from_rgb(16, 20, 36);
const CARD: Color32 = Color32::from_rgb(21, 27, 48);
const LINE: Color32 = Color32::from_rgb(38, 47, 78);
const TEXT: Color32 = Color32::from_rgb(236, 239, 248);
const MUTED: Color32 = Color32::from_rgb(140, 150, 182);
const HUB_PINK: Color32 = Color32::from_rgb(251, 37, 134);
const GOOD: Color32 = Color32::from_rgb(70, 214, 140);
const WARN: Color32 = Color32::from_rgb(255, 190, 70);
const INFO: Color32 = Color32::from_rgb(110, 170, 255);
const BAD: Color32 = Color32::from_rgb(255, 105, 105);

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
}

fn lock(s: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

impl WaveHub {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);
        let paths = Paths::new();
        let manifest = manifest::load(&paths.data);
        let settings = Settings::load(&paths);
        let mut installed = Installed::load(&paths);
        // Forget installs whose program has gone (deleted by hand, or uninstalled in Windows).
        installed.apps.retain(|_, r| r.path.exists());
        let shared = Arc::new(Mutex::new(Shared { installed, ..Default::default() }));
        let tab = match std::env::var("WAVEHUB_START_TAB").as_deref() {
            Ok("community") => Tab::Community,
            Ok("about") => Tab::About,
            _ => Tab::Apps,
        };
        let hub = Self { manifest, paths, settings, shared, tab };
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
        ui.horizontal(|ui| {
            if let Some(bytes) = icon_bytes("wavehub") {
                ui.add(egui::Image::from_bytes("bytes://wavehub.png", bytes).fit_to_exact_size(vec2(44.0, 44.0)));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new("Wave Hub").font(FontId::proportional(24.0)).strong().color(TEXT));
                ui.label(RichText::new("Install, update and open the Wave apps").color(MUTED));
            });
            wave_arcs(ui, HUB_PINK);
            ui.add_space(16.0);
            for (tab, label) in [(Tab::Apps, "Apps"), (Tab::Community, "Community"), (Tab::About, "About")] {
                let selected = self.tab == tab;
                let text = RichText::new(label).size(15.0).color(if selected { BG } else { TEXT });
                let button = egui::Button::new(text).fill(if selected { TEXT } else { Color32::TRANSPARENT }).corner_radius(16.0).min_size(vec2(0.0, 30.0));
                if ui.add(button).clicked() {
                    self.tab = tab;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let updates = self.updatable();
                let label = if updates.is_empty() { "Update all".to_string() } else { format!("Update all ({})", updates.len()) };
                let button = egui::Button::new(RichText::new(label).strong().color(if updates.is_empty() { MUTED } else { BG }))
                    .fill(if updates.is_empty() { CARD } else { HUB_PINK })
                    .corner_radius(8.0)
                    .min_size(vec2(0.0, 32.0));
                if ui.add_enabled(!updates.is_empty(), button).on_hover_text("Update every app that has a newer version").clicked() {
                    self.start_installs(&ctx, updates);
                }
                let checking = lock(&self.shared).checking;
                let refresh = egui::Button::new(RichText::new(if checking { "Checking…" } else { "Check now" }).color(TEXT)).fill(CARD).corner_radius(8.0).min_size(vec2(0.0, 32.0));
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
        let gap = 16.0;
        let columns = ((width + gap) / (320.0 + gap)).floor().clamp(1.0, 3.0) as usize;
        let card_w = (width - gap * (columns as f32 - 1.0)) / columns as f32;
        let apps = self.manifest.apps.clone();
        for row in apps.chunks(columns) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for spec in row {
                    ui.allocate_ui_with_layout(vec2(card_w, 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(card_w);
                        self.card(ui, spec);
                    });
                }
            });
            ui.add_space(gap);
        }
    }

    fn card(&mut self, ui: &mut Ui, spec: &AppSpec) {
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
        let response = egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0, LINE)).corner_radius(14.0).inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_min_height(268.0);
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(bytes) = icon_bytes(&spec.icon) {
                    ui.add(egui::Image::from_bytes(format!("bytes://{}.png", spec.icon), bytes).fit_to_exact_size(vec2(64.0, 64.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(&spec.name).font(FontId::proportional(20.0)).strong().color(TEXT));
                    ui.add(egui::Label::new(RichText::new(&spec.description).color(MUTED)).wrap());
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
                    INFO,
                )
            } else if let Plan::Unavailable(_) = &plan {
                if installed_v.is_some() { ("INSTALLED", GOOD) } else { (if cfg!(windows) { "NOT AVAILABLE YET" } else { "WINDOWS ONLY FOR NOW" }, MUTED) }
            } else if update {
                ("UPDATE AVAILABLE", WARN)
            } else if installed_v.is_some() {
                ("INSTALLED · UP TO DATE", GOOD)
            } else {
                match &latest {
                    Latest::Checking => ("CHECKING…", MUTED),
                    Latest::Found(_) => ("READY TO INSTALL", INFO),
                    Latest::NoReleases => ("NO RELEASES YET", MUTED),
                    Latest::Error(_) => ("OFFLINE", BAD),
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
                ui.label(RichText::new("Installed").color(MUTED));
                ui.label(RichText::new(installed_v.clone().unwrap_or_else(|| "—".into())).color(TEXT).strong());
                ui.label(RichText::new("   Latest").color(MUTED));
                ui.label(RichText::new(latest_text).color(TEXT).strong());
                if let Some(r) = &record {
                    ui.label(RichText::new(format!("({})", r.kind.label())).color(MUTED).small());
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
                ui.label(RichText::new(text).color(TEXT));
                ui.horizontal(|ui| {
                    let bar_w = ui.available_width() - 80.0;
                    ui.add(egui::ProgressBar::new(frac).fill(accent).desired_width(bar_w).desired_height(10.0).corner_radius(5.0));
                    let can_cancel = matches!(phase, Phase::Downloading { .. } | Phase::Checking);
                    if ui.add_enabled(can_cancel, egui::Button::new("Cancel").corner_radius(6.0)).clicked() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                });
            } else if let Plan::Unavailable(why) = &plan {
                ui.add(egui::Label::new(RichText::new(why).color(MUTED)).wrap());
                if let Some(r) = &record {
                    if main_button(ui, "OPEN", accent, full, true).clicked() {
                        self.open(spec, &r.path);
                    }
                } else if main_button(ui, "BUILD FROM SOURCE", Color32::from_rgb(46, 56, 92), full, true).clicked() {
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
                ui.add(egui::Label::new(RichText::new(e).color(BAD)).wrap());
            }
            if let Latest::Error(e) = &latest {
                ui.add(egui::Label::new(RichText::new(e).color(MUTED).small()).wrap());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                link(ui, "GitHub", &format!("https://github.com/{}", spec.repo));
                link(ui, "Release notes", &format!("https://github.com/{}/releases/latest", spec.repo));
                link(ui, "Discussions", &format!("https://github.com/{}/discussions", spec.repo));
                if let Some(r) = record.clone() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.menu_button(RichText::new("•••").color(MUTED), |ui| {
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
        let strip = egui::Rect::from_min_size(rect.min + vec2(16.0, 0.0), vec2(56.0, 3.0));
        ui.painter().rect_filled(strip, CornerRadius::same(2), accent);
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
        ui.label(RichText::new("Join the community").font(FontId::proportional(22.0)).strong().color(TEXT));
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new("Every Wave app is free. Ask questions, share ideas and show what you've made in each app's Discussions on GitHub.").color(MUTED)).wrap());
        ui.add_space(12.0);
        for spec in self.manifest.apps.clone() {
            let accent = Color32::from_rgb(spec.accent[0], spec.accent[1], spec.accent[2]);
            egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0, LINE)).corner_radius(12.0).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if let Some(bytes) = icon_bytes(&spec.icon) {
                        ui.add(egui::Image::from_bytes(format!("bytes://{}.png", spec.icon), bytes).fit_to_exact_size(vec2(40.0, 40.0)));
                    }
                    ui.label(RichText::new(&spec.name).size(17.0).strong().color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let b = egui::Button::new(RichText::new("Discussions").strong().color(BG)).fill(accent).corner_radius(8.0).min_size(vec2(0.0, 30.0));
                        if ui.add(b).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://github.com/{}/discussions", spec.repo)));
                        }
                        link(ui, "Report a problem", &format!("https://github.com/{}/issues", spec.repo));
                        link(ui, "Releases", &format!("https://github.com/{}/releases", spec.repo));
                    });
                });
            });
            ui.add_space(8.0);
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
                ui.label(RichText::new("Wave Hub").font(FontId::proportional(26.0)).strong().color(TEXT));
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(MUTED));
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new("Wave Hub is free and always will be. No account, no ads, no tracking. It installs and updates BigWave Radio, ChipWave Tracker and StreamWave on Windows and Linux, straight from their official GitHub releases.").color(TEXT)).wrap());
            });
        });
        ui.add_space(14.0);
        section(ui, "Updates", |ui| {
            let hub = lock(&self.shared).hub_latest.clone();
            let current = env!("CARGO_PKG_VERSION");
            let (text, color) = match &hub {
                None | Some(Latest::Checking) => ("Checking for a newer Wave Hub…".to_string(), MUTED),
                Some(Latest::Found(tag)) if github::is_newer(tag, current) => (format!("Wave Hub {} is available.", tag.trim_start_matches('v')), WARN),
                Some(Latest::Found(_)) => ("You have the newest Wave Hub.".to_string(), GOOD),
                Some(Latest::NoReleases) => ("No Wave Hub releases have been published yet, so this is the newest.".to_string(), MUTED),
                Some(Latest::Error(e)) => (format!("Couldn't check: {e}"), BAD),
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
            if cfg!(windows) {
                let mut v = self.settings.prefer_installer;
                if ui.checkbox(&mut v, "Use each app's official installer instead of the portable version").changed() {
                    self.settings.prefer_installer = v;
                    let _ = self.settings.save(&self.paths);
                }
                ui.label(RichText::new("The portable version needs no admin rights and lives in Wave Hub's folder. StreamWave always uses its installer.").color(MUTED).small());
            } else {
                ui.label(RichText::new("On Linux, apps are installed as AppImages in your home folder. No password needed.").color(MUTED));
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Apps folder:").color(MUTED));
                ui.label(RichText::new(self.paths.apps().display().to_string()).monospace().color(TEXT));
                if ui.button("Open folder").clicked() {
                    install::reveal(&self.paths.apps());
                }
            });
        });
        support_box(ui, true);
        ui.add_space(10.0);
        ui.label(RichText::new("MIT License · © 2026 Derek Heasley").color(MUTED).small());
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
            ui.label(RichText::new(format!("{checked} · Installing for {} · Free and open source", install::os_label())).color(MUTED).small());
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
        egui::Panel::top("header").frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(20, 12))).show(ui, |ui| self.header(ui));
        egui::Panel::bottom("footer").frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(20, 8))).show(ui, |ui| self.footer(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(20))).show(ui, |ui| {
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

fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(8, 10, 20);
    v.faint_bg_color = CARD;
    v.hyperlink_color = INFO;
    v.selection.bg_fill = HUB_PINK.gamma_multiply(0.6);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(32, 40, 68);
    v.widgets.inactive.bg_fill = Color32::from_rgb(32, 40, 68);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 54, 90);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(54, 66, 108);
    ctx.set_visuals(v);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(12.0, 6.0);
        style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
    });
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

fn main_button(ui: &mut Ui, label: &str, fill: Color32, size: egui::Vec2, enabled: bool) -> egui::Response {
    // Painted by hand so the label is centred and the colours stay ours.
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let lum = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    let mut text_color = if lum > 140.0 { BG } else { Color32::WHITE };
    let mut bg = fill;
    if !enabled {
        bg = Color32::from_rgb(40, 46, 70);
        text_color = MUTED;
    } else if response.is_pointer_button_down_on() {
        bg = fill.linear_multiply(0.8);
    } else if response.hovered() {
        bg = Color32::from_rgb(fill.r().saturating_add(18), fill.g().saturating_add(18), fill.b().saturating_add(18));
    }
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(15.0), text_color);
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn status_chip(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new().stroke(Stroke::new(1.0, color)).corner_radius(4.0).inner_margin(Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).size(11.0).strong().color(color));
    });
}

fn link(ui: &mut Ui, text: &str, url: &str) {
    let r = ui.add(egui::Label::new(RichText::new(text).color(MUTED).size(12.5).underline()).sense(Sense::click())).on_hover_text(url).on_hover_cursor(egui::CursorIcon::PointingHand);
    if r.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0, LINE)).corner_radius(12.0).inner_margin(Margin::same(14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(16.0).strong().color(TEXT));
        ui.add_space(6.0);
        body(ui);
    });
    ui.add_space(10.0);
}

fn support_box(ui: &mut Ui, with_paypal: bool) {
    section(ui, "Support Del's work", |ui| {
        ui.add(egui::Label::new(RichText::new("All Wave apps are free. If they make your day better, a coffee keeps the updates coming. Totally optional.").color(MUTED)).wrap());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let coffee = egui::Button::new(RichText::new("Buy Me a Coffee").strong().color(BG)).fill(Color32::from_rgb(255, 221, 0)).corner_radius(8.0).min_size(vec2(0.0, 32.0));
            if ui.add(coffee).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(BUY_ME_A_COFFEE));
            }
            if with_paypal {
                let paypal = egui::Button::new(RichText::new("PayPal").strong().color(Color32::WHITE)).fill(Color32::from_rgb(0, 112, 186)).corner_radius(8.0).min_size(vec2(0.0, 32.0));
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
