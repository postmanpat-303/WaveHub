//! The window: a card per app, plus Community and About pages.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use egui::{vec2, Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, Ui};

use crate::github;
use crate::install::{self, Phase, Plan};
use crate::manifest::{self, AppSpec, Manifest};
use crate::store::{Installed, Kind, Paths, Record, Settings};

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
}

// Dark: printer's ink — a near-black page, paper-white type, hairlines.
const DARK_PALETTE: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(11, 12, 16),
    panel: Color32::from_rgb(14, 15, 20),
    card: Color32::from_rgb(18, 20, 25),
    line: Color32::from_rgba_premultiplied(36, 36, 36, 38),
    text: Color32::from_rgb(242, 241, 238),
    muted: Color32::from_rgba_premultiplied(142, 142, 140, 150),
    good: Color32::from_rgb(63, 214, 140),
    warn: Color32::from_rgb(255, 193, 77),
    info: Color32::from_rgb(116, 170, 255),
    bad: Color32::from_rgb(255, 106, 94),
    soft: Color32::from_rgb(26, 28, 34),
};

// Light: warm paper, black ink, the same hairlines.
const LIGHT_PALETTE: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(242, 241, 238),
    panel: Color32::from_rgb(249, 248, 245),
    card: Color32::from_rgb(255, 255, 255),
    line: Color32::from_rgba_premultiplied(2, 2, 3, 38),
    text: Color32::from_rgb(16, 16, 20),
    muted: Color32::from_rgba_premultiplied(9, 9, 12, 150),
    good: Color32::from_rgb(10, 117, 99),
    warn: Color32::from_rgb(138, 88, 0),
    info: Color32::from_rgb(22, 89, 196),
    bad: Color32::from_rgb(179, 38, 30),
    soft: Color32::from_rgb(233, 232, 228),
};

static DARK_MODE: AtomicBool = AtomicBool::new(false);

fn p() -> &'static Palette {
    if DARK_MODE.load(Ordering::Relaxed) {
        &DARK_PALETTE
    } else {
        &LIGHT_PALETTE
    }
}

/// Paper and ink swapped: the primary button, the selected filter, the number blocks.
fn invert() -> (Color32, Color32) {
    if p().dark {
        (Color32::from_rgb(242, 241, 238), Color32::from_rgb(11, 11, 12))
    } else {
        (Color32::from_rgb(16, 16, 20), Color32::from_rgb(242, 241, 238))
    }
}

fn hairline() -> Stroke {
    Stroke::new(1.0, p().line)
}

/// Monospaced, letter-spaced small caps: the labels, numbers and status lines.
fn hud_job(text: &str, size: f32, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(text, 0.0, egui::TextFormat { font_id: FontId::new(size, egui::FontFamily::Monospace), color, extra_letter_spacing: 0.6, ..Default::default() });
    job
}

fn hud(ui: &mut Ui, text: &str, size: f32, color: Color32) -> egui::Response {
    ui.add(egui::Label::new(hud_job(text, size, color)).selectable(false))
}

/// A hairline across the available width.
fn rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, hairline());
}

/// A small boxed label: "LAUNCHER", platform chips, status chips.
fn boxed(ui: &mut Ui, text: &str, fg: Color32, border: Color32) -> egui::Response {
    egui::Frame::new().stroke(Stroke::new(1.0, border)).corner_radius(CornerRadius::ZERO).inner_margin(Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.add(egui::Label::new(hud_job(text, 9.5, fg)).selectable(false))
    })
    .response
}

/// A page tab: small caps, with a pink rule under the one you're on.
fn nav_tab(ui: &mut Ui, label: &str, active: bool) -> bool {
    let color = if active { p().text } else { p().muted };
    let galley = ui.painter().layout_job(hud_job(label, 10.5, color));
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 10.0, 26.0), Sense::click());
    ui.painter().galley(rect.min + vec2(5.0, rect.height() / 2.0 - galley.size().y / 2.0), galley, color);
    if active {
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(2.0, HUB_PINK));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// One segment of the ALL / INSTALLED / UPDATES / NOT INSTALLED switch.
fn filter_tab(ui: &mut Ui, label: &str, count: usize, active: bool) -> bool {
    let (fill, fg) = if active { invert() } else { (Color32::TRANSPARENT, p().muted) };
    let text = format!("{label} {count}");
    let galley = ui.painter().layout_job(hud_job(&text, 9.5, fg));
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, 28.0), Sense::click());
    let border = if active { fill } else { p().line };
    ui.painter().rect(rect, CornerRadius::ZERO, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A small square button: filled (paper/ink) for the main action, outlined for the rest.
fn action_button(ui: &mut Ui, label: &str, enabled: bool, filled: bool) -> egui::Response {
    let (fill, fg) = if !enabled {
        (Color32::TRANSPARENT, p().muted.gamma_multiply(0.8))
    } else if filled {
        invert()
    } else {
        (Color32::TRANSPARENT, p().text)
    };
    let galley = ui.painter().layout_job(hud_job(label, 9.5, fg));
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 22.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let border = if filled && enabled { fill } else { p().line };
    ui.painter().rect(rect, CornerRadius::ZERO, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A small caps link with an arrow, as on the cards' top strip.
fn hud_link(ui: &mut Ui, text: &str, url: &str) {
    let galley = ui.painter().layout_job(hud_job(text, 9.0, p().muted));
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 2.0, 18.0), Sense::click());
    let color = if response.hovered() { p().text } else { p().muted };
    let galley = ui.painter().layout_job(hud_job(text, 9.0, color));
    ui.painter().galley(egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, color);
    if response.clicked() {
        crate::links::open(url);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
}

/// WINDOWS / LINUX / MAC: lit in the app's colour when it has a download there.
fn platform_chip(ui: &mut Ui, label: &str, available: bool, accent: Color32) {
    let (fg, border, fill) = if available {
        (accent, accent, tint(accent, if p().dark { 0.16 } else { 0.10 }))
    } else {
        (p().muted.gamma_multiply(0.75), p().line, Color32::TRANSPARENT)
    };
    let galley = ui.painter().layout_job(hud_job(label, 8.5, fg));
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 14.0, 20.0), Sense::hover());
    ui.painter().rect(rect, CornerRadius::ZERO, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
}

/// The app's website, from its GitHub name (all the Wave apps publish one there).
fn site_url(spec: &AppSpec) -> String {
    let name = spec.repo.rsplit('/').next().unwrap_or_default();
    format!("https://postmanpat-303.github.io/{name}/")
}

/// A pale wash of `c` for chips and highlights.
fn tint(c: Color32, amount: f32) -> Color32 {
    let base = p().card;
    let mix = |a: u8, b: u8| (a as f32 * amount + b as f32 * (1.0 - amount)) as u8;
    Color32::from_rgb(mix(c.r(), base.r()), mix(c.g(), base.g()), mix(c.b(), base.b()))
}

const HUB_PINK: Color32 = Color32::from_rgb(251, 37, 134);
const INK: Color32 = Color32::from_rgb(22, 24, 36);

/// Readable text colour on top of `fill`.
fn on(fill: Color32) -> Color32 {
    let lum = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    if lum > 150.0 {
        INK
    } else {
        Color32::WHITE
    }
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
    /// Bytes per second, smoothed.
    speed: f64,
    /// Whether this replaces an installed version (for the wording at the end).
    update: bool,
    /// Reinstalling the same version (Repair / Reinstall).
    repair: bool,
}

/// Something that went wrong, shown on the card until it's retried or dismissed.
#[derive(Clone)]
struct Failure {
    message: String,
    /// Offer "Retry" (an install or update that failed), not just "Dismiss".
    retry: bool,
}

#[derive(Default)]
struct Shared {
    latest: HashMap<String, Latest>,
    jobs: HashMap<String, Job>,
    errors: HashMap<String, Failure>,
    /// Just finished: when, and the message to celebrate with.
    finished: HashMap<String, (Instant, String)>,
    /// A short note, such as "Download cancelled.", that fades after a few seconds.
    notes: HashMap<String, (Instant, String)>,
    installed: Installed,
    checked_at: Option<Instant>,
    checking: bool,
    hub_latest: Option<Latest>,
    /// The release file this computer should get when a newer Wave Hub is out
    /// (`Ok(None)`: the release has nothing for it).
    hub_asset: Option<Result<Option<String>, String>>,
    /// For each app: the file names its plan tries, and which one the latest release has
    /// (`None`: none of them, e.g. StreamWave before its first Mac build).
    assets: HashMap<String, (Vec<String>, Option<String>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Apps,
    Community,
    About,
}

/// Which cards the toolbar is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Installed,
    Updates,
    NotInstalled,
}

pub struct WaveHub {
    manifest: Manifest,
    paths: Paths,
    settings: Settings,
    shared: Arc<Mutex<Shared>>,
    tab: Tab,
    filter: Filter,
    /// Height of the tallest card in each row last frame, so a row's cards line up.
    row_heights: Vec<f32>,
    /// The progress each card's bar shows, easing towards the real value.
    shown_progress: HashMap<String, f32>,
    /// The app whose "Remove …?" box is open.
    confirm_uninstall: Option<String>,
}

/// What a card says when the latest release has no file for this computer.
fn no_download_text(spec: &AppSpec) -> String {
    let custom = if cfg!(target_os = "macos") {
        spec.macos.as_ref().and_then(|m| m.unavailable.clone())
    } else if cfg!(target_os = "linux") {
        spec.linux.as_ref().and_then(|l| l.unavailable.clone())
    } else {
        None
    };
    custom.unwrap_or_else(|| format!("The latest {} release has no download for {} yet.", spec.name, install::os_label()))
}

/// Whether Wave Hub can update this install itself. Package-manager and hand-installed Mac
/// copies are updated the way they were installed.
fn hub_can_update(r: &Record) -> bool {
    !matches!(r.kind, Kind::SystemPackage | Kind::MacApp)
}

/// The install plan for this app, keeping to the setup program when that's how it's installed.
fn plan_for(spec: &AppSpec, prefer_installer: bool, record: Option<&Record>) -> Plan {
    install::plan(spec, prefer_installer || record.is_some_and(|r| r.kind == Kind::Installer))
}

/// Adds apps found on this computer that Wave Hub doesn't know about (installed with their own
/// setup, a package manager or by hand), and forgets ones that have gone.
fn find_external(shared: &Arc<Mutex<Shared>>, paths: &Paths, apps: &[AppSpec]) {
    // Wave Hub's own installs are known; ones found before are looked up again, in case
    // they've been updated or removed outside Wave Hub since.
    let known: Vec<String> = lock(shared).installed.apps.iter().filter(|(_, r)| !r.external).map(|(id, _)| id.clone()).collect();
    let found: Vec<(String, Record)> = apps.iter().filter(|a| !known.contains(&a.id)).filter_map(|a| crate::uninstall::find_external(a).map(|r| (a.id.clone(), r))).collect();
    let mut s = lock(shared);
    let before = s.installed.apps.len();
    let busy: Vec<String> = s.jobs.keys().cloned().collect();
    // Keep installs whose program file went missing (shown as FILES MISSING, with Repair);
    // forget ones that were uninstalled or deleted outside Wave Hub.
    s.installed.apps.retain(|id, r| busy.contains(id) || apps.iter().find(|a| a.id == *id).is_some_and(|a| crate::uninstall::check_files(a, r, paths) != crate::uninstall::Files::Gone));
    let mut changed = before != s.installed.apps.len();
    for (id, r) in found {
        let replace = s.installed.apps.get(&id).is_none_or(|old| old.external && (old.version != r.version || old.path != r.path || old.kind != r.kind));
        if !s.jobs.contains_key(&id) && replace {
            s.installed.apps.insert(id, r);
            changed = true;
        }
    }
    if changed {
        let _ = s.installed.save(paths);
    }
}

/// The newest Wave Hub and, when it's newer than this one, the file to download.
fn check_hub(repo: &str) -> (Latest, Option<Result<Option<String>, String>>) {
    match github::latest_tag(repo) {
        Ok(Some(tag)) => {
            let asset = github::is_newer(&tag, env!("CARGO_PKG_VERSION")).then(|| crate::selfupdate::asset_for_this_computer(repo, &tag));
            (Latest::Found(tag), asset)
        }
        Ok(None) => (Latest::NoReleases, None),
        Err(e) => (Latest::Error(e), None),
    }
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
        // Forget installs that were uninstalled or deleted outside Wave Hub; keep ones whose
        // program file alone went missing, so the card can offer Repair.
        installed.apps.retain(|id, r| manifest.apps.iter().find(|a| a.id == *id).is_some_and(|a| crate::uninstall::check_files(a, r, &paths) != crate::uninstall::Files::Gone));
        let shared = Arc::new(Mutex::new(Shared { installed, ..Default::default() }));
        let tab = match std::env::var("WAVEHUB_START_TAB").as_deref() {
            Ok("community") => Tab::Community,
            Ok("about") => Tab::About,
            _ => Tab::Apps,
        };
        let confirm_uninstall = std::env::var("WAVEHUB_CONFIRM_UNINSTALL").ok();
        let hub = Self { manifest, paths, settings, shared, tab, filter: Filter::All, row_heights: Vec::new(), shown_progress: HashMap::new(), confirm_uninstall };
        hub.check_now(&cc.egui_ctx);
        hub.repair_shortcuts();
        hub
    }

    /// Windows: portable installs from before Wave Hub made shortcuts get them, once, in the
    /// background (PowerShell takes a moment).
    fn repair_shortcuts(&self) {
        if !cfg!(windows) || !self.settings.shortcuts {
            return;
        }
        let shared = self.shared.clone();
        let paths = self.paths.clone();
        let apps = self.manifest.apps.clone();
        std::thread::spawn(move || {
            let mut todo = lock(&shared).installed.clone();
            if !install::repair_shortcuts(&apps, &mut todo) {
                return;
            }
            let mut s = lock(&shared);
            let mut changed = false;
            for (id, done) in todo.apps.iter().filter(|(_, r)| r.shortcuts_done) {
                // Only if it's still the same install (an update may have finished meanwhile).
                if let Some(r) = s.installed.apps.get_mut(id).filter(|r| r.path == done.path && !r.shortcuts_done) {
                    r.shortcuts_done = true;
                    changed = true;
                }
            }
            if changed {
                let _ = s.installed.save(&paths);
            }
        });
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
        let paths = self.paths.clone();
        let prefer_installer = self.settings.prefer_installer;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            find_external(&shared, &paths, &apps);
            ctx.request_repaint();
            for app in apps {
                let latest = match github::latest_tag(&app.repo) {
                    Ok(Some(tag)) => Latest::Found(tag),
                    Ok(None) => Latest::NoReleases,
                    Err(e) => Latest::Error(e),
                };
                // Which of the plan's files the release really has, so a card switches on by
                // itself when a release adds a download (and off when it has none).
                if let Latest::Found(tag) = &latest {
                    let record = lock(&shared).installed.apps.get(&app.id).cloned();
                    let plan = plan_for(&app, prefer_installer, record.as_ref());
                    if !plan.files().is_empty() {
                        if let Ok(found) = install::resolve_file(&app.repo, tag, plan.files()) {
                            lock(&shared).assets.insert(app.id.clone(), (plan.files().to_vec(), found));
                        }
                    }
                }
                lock(&shared).latest.insert(app.id.clone(), latest);
                ctx.request_repaint();
            }
            let (hub, asset) = check_hub(&hub_repo);
            let mut s = lock(&shared);
            s.hub_latest = Some(hub);
            s.hub_asset = asset;
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
            let (hub, asset) = check_hub(&repo);
            let mut s = lock(&shared);
            s.hub_latest = Some(hub);
            s.hub_asset = asset;
            ctx.request_repaint();
        });
    }

    /// Installs or updates these apps one after another in the background.
    fn start_installs(&self, ctx: &egui::Context, ids: Vec<String>) {
        self.start_jobs(ctx, ids, false);
    }

    /// Downloads and installs the installed version again (Repair, Reinstall). Falls back to
    /// the latest when Wave Hub doesn't know which version it is.
    fn start_reinstall(&self, ctx: &egui::Context, id: &str) {
        self.start_jobs(ctx, vec![id.to_string()], true);
    }

    fn start_jobs(&self, ctx: &egui::Context, ids: Vec<String>, repair: bool) {
        let mut queued = Vec::new();
        {
            let mut s = lock(&self.shared);
            for id in ids {
                let Some(spec) = self.manifest.apps.iter().find(|a| a.id == id) else {
                    continue;
                };
                let same = s.installed.apps.get(&id).filter(|_| repair).map(|r| if !r.tag.is_empty() { r.tag.clone() } else { format!("v{}", r.version) }).filter(|t| t.len() > 1);
                let tag = match (same, s.latest.get(&id).cloned()) {
                    (Some(t), _) => t,
                    (None, Some(Latest::Found(t))) => t,
                    _ => continue,
                };
                if s.jobs.contains_key(&id) {
                    continue;
                }
                let cancel = Arc::new(AtomicBool::new(false));
                let update = s.installed.apps.contains_key(&id);
                let phase = if queued.is_empty() { Phase::Checking } else { Phase::Queued };
                s.jobs.insert(id.clone(), Job { phase, cancel: cancel.clone(), speed: 0.0, update, repair });
                s.errors.remove(&id);
                s.notes.remove(&id);
                s.finished.remove(&id);
                queued.push((spec.clone(), tag, cancel));
            }
        }
        if queued.is_empty() {
            return;
        }
        let shared = self.shared.clone();
        let paths = self.paths.clone();
        let prefer_installer = self.settings.prefer_installer;
        let shortcuts = self.settings.shortcuts;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for (spec, tag, cancel) in queued {
                if cancel.load(Ordering::Relaxed) {
                    let mut s = lock(&shared);
                    s.jobs.remove(&spec.id);
                    s.notes.insert(spec.id.clone(), (Instant::now(), "Cancelled.".into()));
                    continue;
                }
                run_one(&shared, &paths, &spec, &tag, prefer_installer, shortcuts, &cancel, &ctx);
            }
        });
    }

    fn header(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if let Some(bytes) = icon_bytes("wavehub") {
                ui.add(egui::Image::from_bytes("bytes://wavehub.png", bytes).fit_to_exact_size(vec2(26.0, 26.0)));
            }
            ui.add(egui::Label::new(hud_job("WAVE HUB", 15.5, p().text)).selectable(false));
            boxed(ui, "LAUNCHER", p().muted, p().line);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let label = if p().dark { "LIGHT" } else { "DARK" };
                let theme_button = egui::Button::new(RichText::new(label).size(9.5).family(bold()).color(p().text))
                    .fill(Color32::TRANSPARENT)
                    .stroke(hairline())
                    .corner_radius(CornerRadius::ZERO)
                    .min_size(vec2(48.0, 25.0));
                if ui.add(theme_button).on_hover_text(if p().dark { "Light theme" } else { "Dark theme" }).clicked() {
                    self.settings.theme = if p().dark { "light".into() } else { "dark".into() };
                    let _ = self.settings.save(&self.paths);
                    DARK_MODE.store(self.settings.theme == "dark", Ordering::Relaxed);
                    apply_theme(&ctx);
                }
                ui.add_space(10.0);
                for (tab, label) in [(Tab::About, "ABOUT"), (Tab::Community, "COMMUNITY"), (Tab::Apps, "APPS")] {
                    if nav_tab(ui, label, self.tab == tab) {
                        self.tab = tab;
                    }
                }
            });
        });
        rule(ui);
    }

    /// Apps with an update that can be installed here and aren't busy.
    fn updatable(&self) -> Vec<String> {
        let s = lock(&self.shared);
        self.manifest
            .apps
            .iter()
            .filter(|a| !matches!(plan_for(a, self.settings.prefer_installer, s.installed.apps.get(&a.id)), Plan::Unavailable(_)))
            .filter(|a| !s.jobs.contains_key(&a.id))
            .filter(|a| match (s.installed.apps.get(&a.id), s.latest.get(&a.id)) {
                (Some(rec), Some(Latest::Found(tag))) => hub_can_update(rec) && github::is_newer(tag, &rec.version),
                _ => false,
            })
            .map(|a| a.id.clone())
            .collect()
    }

    fn apps_page(&mut self, ui: &mut Ui) {
        let apps = self.manifest.apps.clone();
        let updates = self.updatable();
        let installed = {
            let s = lock(&self.shared);
            apps.iter().filter(|a| s.installed.apps.contains_key(&a.id)).count()
        };

        ui.horizontal(|ui| {
            hud(ui, "01 \u{2014} THE WAVE APPS", 10.0, p().muted);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                hud(ui, &format!("{} APPS \u{00b7} {} INSTALLED", apps.len(), installed), 10.0, p().muted);
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let mut want = self.filter;
            if filter_tab(ui, "ALL", apps.len(), self.filter == Filter::All) {
                want = Filter::All;
            }
            if filter_tab(ui, "INSTALLED", installed, self.filter == Filter::Installed) {
                want = Filter::Installed;
            }
            if filter_tab(ui, "UPDATES", updates.len(), self.filter == Filter::Updates) {
                want = Filter::Updates;
            }
            if filter_tab(ui, "NOT INSTALLED", apps.len() - installed, self.filter == Filter::NotInstalled) {
                want = Filter::NotInstalled;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if updates.is_empty() { "UPDATE ALL".to_string() } else { format!("UPDATE ALL ({})", updates.len()) };
                if action_button(ui, &label, !updates.is_empty(), true).clicked() {
                    self.start_installs(ui.ctx(), updates.clone());
                }
                let checking = lock(&self.shared).checking;
                let label = if checking { "CHECKING\u{2026}".to_string() } else { "CHECK NOW".to_string() };
                if action_button(ui, &label, !checking, false).clicked() {
                    self.check_now(ui.ctx());
                }
            });
            if want != self.filter {
                self.filter = want;
                self.row_heights.clear();
                ui.ctx().request_repaint();
            }
        });
        ui.add_space(14.0);

        let visible: Vec<AppSpec> = {
            let s = lock(&self.shared);
            apps.iter()
                .filter(|a| match self.filter {
                    Filter::All => true,
                    Filter::Installed => s.installed.apps.contains_key(&a.id),
                    Filter::Updates => updates.iter().any(|id| id == &a.id),
                    Filter::NotInstalled => !s.installed.apps.contains_key(&a.id),
                })
                .cloned()
                .collect()
        };
        if visible.is_empty() {
            egui::Frame::new().stroke(hairline()).inner_margin(Margin::symmetric(16, 20)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                hud(ui, "NOTHING IN THIS VIEW", 10.5, p().text);
                ui.label(RichText::new("Try another filter, or check that Wave Hub can reach GitHub.").size(12.5).color(p().muted));
            });
            ui.add_space(16.0);
            self.welcome_strip(ui);
            return;
        }

        let width = ui.available_width();
        let gap = 14.0;
        let columns = ((width + gap) / (300.0 + gap)).floor().clamp(1.0, 3.0) as usize;
        let card_w = (width - gap * (columns as f32 - 1.0)) / columns as f32;
        let mut heights = Vec::new();
        for (row_index, row) in visible.chunks(columns).enumerate() {
            let row_min = self.row_heights.get(row_index).copied().unwrap_or(0.0);
            // Lay each card out in its own fixed-width column, then move down by the tallest.
            let top_left = ui.cursor().min;
            let mut tallest: f32 = 0.0;
            let mut painted: f32 = 0.0;
            for (i, spec) in row.iter().enumerate() {
                let min = top_left + vec2(i as f32 * (card_w + gap), 0.0);
                let rect = egui::Rect::from_min_size(min, vec2(card_w, 2600.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
                let number = self.manifest.apps.iter().position(|a| a.id == spec.id).unwrap_or(0) + 1;
                let natural = self.card(&mut child, spec, card_w, row_min, number);
                tallest = tallest.max(natural);
                painted = painted.max(child.min_rect().height());
            }
            heights.push(tallest);
            ui.allocate_space(vec2(width, tallest.max(row_min).max(painted)));
            ui.add_space(gap);
        }
        if heights != self.row_heights {
            self.row_heights = heights;
            ui.ctx().request_repaint();
        }
        ui.add_space(2.0);
        self.welcome_strip(ui);
    }

    /// The line under the cards: one plain sentence and a way into Community.
    fn welcome_strip(&mut self, ui: &mut Ui) {
        egui::Frame::new().stroke(hairline()).inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                boxed(ui, "FREE", p().good, p().line);
                ui.add_space(2.0);
                ui.label(RichText::new("Every Wave app is free. Say hi and share what you make in Discussions.").size(12.5).color(p().muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if action_button(ui, "COMMUNITY", true, false).clicked() {
                        self.tab = Tab::Community;
                    }
                });
            });
        });
    }

    /// Draws one app card; returns its height without any padding.
    fn card(&mut self, ui: &mut Ui, spec: &AppSpec, width: f32, row_min: f32, number: usize) -> f32 {
        let ctx = ui.ctx().clone();
        let accent = Color32::from_rgb(spec.accent[0], spec.accent[1], spec.accent[2]);
        // `waiting`: Wave Hub knows this app's files for this computer, but the latest release
        // has none of them yet (the card switches on by itself once one appears).
        let (plan, waiting) = {
            let s = lock(&self.shared);
            let plan = plan_for(spec, self.settings.prefer_installer, s.installed.apps.get(&spec.id));
            match s.assets.get(&spec.id) {
                Some((files, None)) if !files.is_empty() && files.as_slice() == plan.files() => (Plan::Unavailable(no_download_text(spec)), true),
                _ => (plan, false),
            }
        };
        let (latest, record, job, error, finished, note) = {
            let mut s = lock(&self.shared);
            // Celebrations and notes fade after a few seconds.
            s.finished.retain(|_, (t, _)| t.elapsed().as_secs_f32() < 6.0);
            s.notes.retain(|_, (t, _)| t.elapsed().as_secs_f32() < 4.0);
            (
                s.latest.get(&spec.id).cloned().unwrap_or(Latest::Checking),
                s.installed.apps.get(&spec.id).cloned(),
                s.jobs.get(&spec.id).map(|j| (j.phase.clone(), j.cancel.clone(), j.speed)),
                s.errors.get(&spec.id).cloned(),
                s.finished.get(&spec.id).cloned(),
                s.notes.get(&spec.id).map(|(_, n)| n.clone()),
            )
        };
        if job.is_none() {
            self.shown_progress.remove(&spec.id);
        }

        let latest_tag = match &latest {
            Latest::Found(t) => Some(t.clone()),
            _ => None,
        };
        let latest_v = latest_tag.as_deref().map(|t| t.trim_start_matches('v').to_string());
        let installed_v = record.as_ref().map(|r| r.version.clone());
        // The program file has gone (Windows Security quarantine, a half-deleted folder).
        let missing = record.as_ref().is_some_and(|r| !r.path.is_file());
        let update = !missing && record.as_ref().is_some_and(hub_can_update) && matches!((&installed_v, &latest_v), (Some(i), Some(l)) if github::is_newer(l, i));

        let response = egui::Frame::new()
            .fill(p().card)
            .stroke(hairline())
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::same(0))
            .show(ui, |ui| {
                ui.set_width(width - 2.0);

                // The strip along the top: the number in the app's colour, the version, the links.
                let strip_h = 36.0;
                let strip = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), strip_h));
                ui.allocate_rect(strip, Sense::hover());
                let painter = ui.painter().with_clip_rect(ui.clip_rect());
                let num_rect = egui::Rect::from_min_size(strip.min, vec2(46.0, strip_h));
                painter.rect_filled(num_rect, CornerRadius::ZERO, accent);
                painter.text(num_rect.center(), egui::Align2::CENTER_CENTER, format!("{number:02}"), FontId::new(11.0, egui::FontFamily::Monospace), on(accent));
                painter.hline(strip.x_range(), strip.bottom(), hairline());
                let version = latest_tag.clone().unwrap_or_else(|| "\u{2014}".into());
                painter.text(egui::pos2(num_rect.right() + 12.0, strip.center().y), egui::Align2::LEFT_CENTER, version, FontId::new(9.5, egui::FontFamily::Monospace), p().muted);
                let links_rect = egui::Rect::from_min_max(egui::pos2(strip.right() - 220.0, strip.top()), strip.max);
                let mut links = ui.new_child(egui::UiBuilder::new().max_rect(links_rect).layout(Layout::right_to_left(Align::Center)));
                links.spacing_mut().item_spacing.x = 14.0;
                hud_link(&mut links, "GITHUB \u{2197}", &format!("https://github.com/{}", spec.repo));
                hud_link(&mut links, "WEBSITE \u{2197}", &site_url(spec));

                egui::Frame::new().inner_margin(Margin { left: 16, right: 16, top: 14, bottom: 16 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        if let Some(bytes) = icon_bytes(&spec.icon) {
                            ui.add(egui::Image::from_bytes(format!("bytes://{}.png", spec.icon), bytes).fit_to_exact_size(vec2(54.0, 54.0)));
                        }
                        ui.vertical(|ui| {
                            ui.add_space(2.0);
                            ui.label(RichText::new(&spec.name).font(FontId::proportional(21.0)).family(bold()).color(p().text));
                            ui.add(egui::Label::new(RichText::new(&spec.description).size(12.5).color(p().muted)).wrap());
                        });
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        platform_chip(ui, "WINDOWS", spec.windows.as_ref().is_some_and(|w| !w.installer.is_empty() || !w.portable.is_empty()), accent);
                        platform_chip(ui, "LINUX", spec.linux.as_ref().is_some_and(|l| !l.appimage.is_empty()), accent);
                        platform_chip(ui, "MAC", spec.macos.as_ref().is_some_and(|m| !m.zip.is_empty()), accent);
                    });
                    ui.add_space(10.0);

                    // Status chip and version line.
                    let (chip, chip_color) = if let Some((phase, _, _)) = &job {
                        (
                            match phase {
                                Phase::Queued => "WAITING\u{2026}",
                                Phase::Checking => "PREPARING\u{2026}",
                                Phase::Downloading { .. } => "DOWNLOADING\u{2026}",
                                Phase::Verifying => "CHECKING DOWNLOAD\u{2026}",
                                Phase::Installing => "INSTALLING\u{2026}",
                                Phase::RunningInstaller => "INSTALLING\u{2026}",
                                Phase::Removing => "REMOVING\u{2026}",
                            },
                            p().info,
                        )
                    } else if finished.is_some() {
                        (
                            match finished.as_ref().map(|(_, m)| m.as_str()) {
                                Some(m) if m.starts_with("Updated") => "UPDATED!",
                                Some(m) if m.starts_with("Removed") => "REMOVED",
                                _ => "INSTALLED!",
                            },
                            p().good,
                        )
                    } else if missing {
                        ("FILES MISSING", p().bad)
                    } else if error.as_ref().is_some_and(|e| e.retry) {
                        ("SOMETHING WENT WRONG", p().bad)
                    } else if let Plan::Unavailable(_) = &plan {
                        if installed_v.is_some() { ("INSTALLED", p().good) } else { (if cfg!(target_os = "macos") { "NOT ON MAC YET" } else if cfg!(windows) || waiting { "NOT AVAILABLE YET" } else { "WINDOWS ONLY FOR NOW" }, p().muted) }
                    } else if update {
                        ("UPDATE AVAILABLE", p().warn)
                    } else if installed_v.is_some() {
                        ("INSTALLED \u{00b7} UP TO DATE", p().good)
                    } else {
                        match &latest {
                            Latest::Checking => ("CHECKING\u{2026}", p().muted),
                            Latest::Found(_) => ("READY TO INSTALL", p().info),
                            Latest::NoReleases => ("NO RELEASES YET", p().muted),
                            Latest::Error(_) => ("OFFLINE", p().bad),
                        }
                    };
                    ui.horizontal(|ui| {
                        status_chip(ui, chip, chip_color);
                        ui.add_space(4.0);
                        let versions = match (&installed_v, &latest_v) {
                            (Some(i), Some(l)) if i == l => i.clone(),
                            (Some(i), Some(l)) => format!("{} \u{2192} {}", i, l),
                            (Some(i), None) => i.clone(),
                            (None, Some(l)) => format!("LATEST {}", l),
                            (None, None) => String::new(),
                        };
                        if !versions.is_empty() {
                            hud(ui, &versions, 9.5, p().muted);
                        }
                        if let Some(r) = &record {
                            hud(ui, &r.kind.label().to_uppercase(), 9.5, p().muted);
                        }
                    });
                    ui.add_space(10.0);

                    // The announcement strip, if the app has one (apps.json `notice`).
                    if let Some(notice) = spec.notice.as_deref().filter(|n| !n.trim().is_empty()) {
                        notice_note(ui, notice, accent);
                        ui.add_space(8.0);
                    }

                    // The one main button, or progress.
                    let full = vec2(ui.available_width(), 40.0);
                    if let Some((phase, cancel, speed)) = &job {
                        // One bar for the whole job: download fills most of it, checking and
                        // installing the rest.
                        let (target, stage, detail) = match phase {
                            Phase::Queued => (0.0, "Waiting for the app before it\u{2026}".to_string(), String::new()),
                            Phase::Checking => (0.02, "Starting download\u{2026}".to_string(), String::new()),
                            Phase::Downloading { done, total } if *total > 0 => {
                                let f = *done as f32 / *total as f32;
                                let mut detail = format!("{} of {}", mb(*done), mb(*total));
                                if *speed > 0.0 {
                                    let left = (*total - *done) as f64 / speed;
                                    detail = format!("{detail} \u{00b7} {} \u{00b7} {}", speed_text(*speed), eta_text(left));
                                }
                                (0.02 + 0.86 * f, format!("Downloading \u{00b7} {:.0}%", f * 100.0), detail)
                            }
                            Phase::Downloading { done, .. } => (0.3, format!("Downloading {}", mb(*done)), speed_text(*speed)),
                            Phase::Verifying => (0.91, "Checking it's genuine\u{2026}".to_string(), String::new()),
                            Phase::Installing => (0.96, "Installing\u{2026}".to_string(), String::new()),
                            Phase::RunningInstaller => (0.96, "Installing\u{2026} (this can take a minute)".to_string(), String::new()),
                            Phase::Removing => {
                                let how = match record.as_ref().map(|r| r.kind) {
                                    Some(Kind::Installer) => format!("Running {}'s uninstaller\u{2026}", spec.name),
                                    Some(Kind::SystemPackage) => "Waiting for the package manager\u{2026}".to_string(),
                                    _ => format!("Removing {}\u{2026}", spec.name),
                                };
                                (0.6, how, "Your settings stay.".to_string())
                            }
                        };
                        let shown = self.shown_progress.entry(spec.id.clone()).or_insert(0.0);
                        let dt = ui.input(|i| i.stable_dt).min(0.1);
                        *shown += (target - *shown) * (1.0 - (-dt * 6.0).exp());
                        let shown = *shown;
                        ui.label(RichText::new(stage).color(p().text).family(bold()).size(13.0));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let cancellable = matches!(phase, Phase::Queued | Phase::Checking | Phase::Downloading { .. });
                            let bar_w = ui.available_width() - if cancellable { 86.0 } else { 0.0 };
                            progress_bar(ui, shown, accent, bar_w, ui.input(|i| i.time));
                            if cancellable {
                                let b = egui::Button::new(RichText::new("CANCEL").color(p().text).family(bold()).size(11.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(hairline())
                                    .corner_radius(CornerRadius::ZERO)
                                    .min_size(vec2(78.0, 28.0));
                                if ui.add(b).clicked() {
                                    cancel.store(true, Ordering::Relaxed);
                                }
                            }
                        });
                        if !detail.is_empty() {
                            ui.add_space(3.0);
                            hud(ui, &detail, 10.0, p().muted);
                        }
                        ui.add_space(8.0);
                        if !matches!(phase, Phase::Removing) {
                            stage_dots(ui, phase, accent);
                        }
                    } else if let Some(r) = record.as_ref().filter(|_| missing) {
                        let name = r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let why = if cfg!(windows) {
                            format!("{name} isn't there any more. Windows Security may have removed it: open Windows Security > Virus & threat protection > Protection history, find {name}, and choose Actions > Restore. Or click Repair to download a fresh copy.")
                        } else {
                            format!("{name} isn't there any more. Click Repair to download a fresh copy.")
                        };
                        ui.add(egui::Label::new(RichText::new(why).color(p().muted).size(12.0)).wrap());
                        ui.add_space(8.0);
                        let can = hub_can_update(r) && !matches!(plan, Plan::Unavailable(_));
                        if can {
                            let v = if r.version.is_empty() { String::new() } else { format!(" {}", r.version) };
                            if main_button(ui, &format!("REPAIR{v}"), accent, full, true).clicked() {
                                self.start_reinstall(&ctx, &spec.id);
                            }
                        } else if main_button(ui, "UNINSTALL", p().soft, full, true).clicked() {
                            self.confirm_uninstall = Some(spec.id.clone());
                        }
                    } else if let Plan::Unavailable(why) = &plan {
                        ui.add(egui::Label::new(RichText::new(why).color(p().muted).size(12.0)).wrap());
                        ui.add_space(8.0);
                        if let Some(r) = &record {
                            if main_button(ui, "OPEN", accent, full, true).clicked() {
                                self.open(spec, &r.path);
                            }
                        } else {
                            // Linux can build ChipWave from source; on a Mac there's nothing to build yet,
                            // so just point to the project.
                            let label = if cfg!(target_os = "macos") || waiting { "VIEW ON GITHUB" } else { "BUILD FROM SOURCE" };
                            if main_button(ui, label, p().soft, full, true).clicked() {
                                crate::links::open(&if waiting { format!("https://github.com/{}/releases/latest", spec.repo) } else { format!("https://github.com/{}#readme", spec.repo) });
                            }
                        }
                    } else if update {
                        let label = format!("UPDATE TO {}", latest_v.clone().unwrap_or_default());
                        let label = if error.as_ref().is_some_and(|e| e.retry) { format!("RETRY UPDATE TO {}", latest_v.clone().unwrap_or_default()) } else { label };
                        if main_button(ui, &label, accent, full, true).clicked() {
                            self.start_installs(&ctx, vec![spec.id.clone()]);
                        }
                    } else if let Some(r) = &record {
                        if finished.is_some() {
                            // A gentle pulse so the eye finds the new Open button.
                            let t = ui.input(|i| i.time) as f32;
                            let grow = 1.5 + 1.5 * (t * 4.0).sin().abs();
                            let (rect, _) = (ui.cursor().min, ());
                            let glow = egui::Rect::from_min_size(rect, full).expand(grow);
                            ui.painter().rect_filled(glow, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 50));
                        }
                        if main_button(ui, "OPEN", accent, full, true).clicked() {
                            self.open(spec, &r.path);
                        }
                    } else {
                        let ready = latest_tag.is_some();
                        let label = if error.as_ref().is_some_and(|e| e.retry) { "RETRY INSTALL" } else { "INSTALL" };
                        if main_button(ui, label, accent, full, ready).clicked() {
                            self.start_installs(&ctx, vec![spec.id.clone()]);
                        }
                    }

                    if let Some((when, message)) = &finished {
                        // Fade in quickly, hold, then fade out.
                        let t = when.elapsed().as_secs_f32();
                        let alpha = (t * 4.0).min(1.0) * ((6.0 - t) / 1.0).clamp(0.0, 1.0);
                        ui.add_space(8.0);
                        let fill = tint(p().good, 0.10 * alpha);
                        egui::Frame::new().fill(fill).corner_radius(CornerRadius::ZERO).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(message).color(p().good.gamma_multiply(alpha.max(0.05))).size(12.0));
                        });
                    }
                    if let Some(note) = &note {
                        ui.add_space(4.0);
                        hud(ui, note, 10.0, p().muted);
                    }
                    if let Some(e) = &error {
                        ui.add_space(8.0);
                        let mut retry = false;
                        let mut dismiss = false;
                        egui::Frame::new().fill(tint(p().bad, 0.08)).corner_radius(CornerRadius::ZERO).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(RichText::new(&e.message).color(p().bad).size(12.0)).wrap());
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if e.retry {
                                    let b = egui::Button::new(RichText::new("RETRY").family(bold()).size(11.0).color(on(p().bad))).fill(p().bad).stroke(Stroke::NONE).corner_radius(CornerRadius::ZERO).min_size(vec2(70.0, 26.0));
                                    retry = ui.add(b).clicked();
                                }
                                let d = egui::Button::new(RichText::new("DISMISS").size(11.0).color(p().text)).fill(Color32::TRANSPARENT).stroke(hairline()).corner_radius(CornerRadius::ZERO).min_size(vec2(0.0, 26.0));
                                dismiss = ui.add(d).clicked();
                            });
                        });
                        if retry {
                            self.start_installs(&ctx, vec![spec.id.clone()]);
                        } else if dismiss {
                            lock(&self.shared).errors.remove(&spec.id);
                        }
                    }
                    if let Latest::Error(e) = &latest {
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(RichText::new(e).color(p().muted).size(11.0)).wrap());
                    }

                    // Footer: the app's links, and the rest under the dot-dot-dot.
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        hud_link(ui, "RELEASE NOTES \u{2197}", &format!("https://github.com/{}/releases/latest", spec.repo));
                        ui.add_space(12.0);
                        hud_link(ui, "DISCUSSIONS \u{2197}", &format!("https://github.com/{}/discussions", spec.repo));
                        if let Some(r) = record.clone().filter(|_| job.is_none()) {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let dots = egui::Button::new(RichText::new("\u{00b7}\u{00b7}\u{00b7}").color(p().text).size(17.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(hairline())
                                    .corner_radius(CornerRadius::ZERO)
                                    .min_size(vec2(40.0, 30.0));
                                egui::containers::menu::MenuButton::from_button(dots).ui(ui, |ui| {
                                    ui.set_min_width(230.0);
                                    if update && ui.button(format!("Open {} (current version)", r.version)).clicked() {
                                        self.open(spec, &r.path);
                                    }
                                    if ui.button("Show folder").clicked() {
                                        if let Some(dir) = r.path.parent() {
                                            install::reveal(dir);
                                        }
                                    }
                                    if hub_can_update(&r) && !matches!(plan, Plan::Unavailable(_)) {
                                        let v = if r.version.is_empty() { String::new() } else { format!(" {}", r.version) };
                                        if ui.button(format!("Reinstall{v}")).clicked() {
                                            self.start_reinstall(&ctx, &spec.id);
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    let b = egui::Button::new(RichText::new(format!("Uninstall {}\u{2026}", spec.name)).color(p().bad));
                                    if ui.add(b).clicked() {
                                        self.confirm_uninstall = Some(spec.id.clone());
                                        ui.close();
                                    }
                                });
                            });
                        }
                    });

                    // Pad to the row's tallest card so a row lines up; report our own natural height.
                    let natural = ui.min_rect().height();
                    let target = (row_min - 38.0).max(0.0);
                    let pad = target - natural - ui.spacing().item_spacing.y;
                    if pad > 0.0 {
                        ui.allocate_space(vec2(1.0, pad));
                    }
                    natural
                })
                .inner
            });
        let natural = response.inner;
        38.0 + natural
    }


    fn open(&self, spec: &AppSpec, path: &std::path::Path) {
        let result = install::open(path);
        let mut s = lock(&self.shared);
        match result {
            Ok(()) => {
                s.errors.remove(&spec.id);
            }
            Err(e) => {
                let message = if e.starts_with("Windows ") { e } else { format!("Couldn't open {}: {e}", spec.name) };
                s.errors.insert(spec.id.clone(), Failure { message, retry: false });
            }
        }
    }

    /// Uninstalls the app in the background; the card shows progress, then "Removed".
    fn start_uninstall(&self, ctx: &egui::Context, spec: &AppSpec) {
        let record = {
            let mut s = lock(&self.shared);
            let Some(record) = s.installed.apps.get(&spec.id).cloned() else {
                return;
            };
            if s.jobs.contains_key(&spec.id) {
                return;
            }
            if install::is_running(&record.path) {
                s.errors.insert(spec.id.clone(), Failure { message: format!("{} is open. Close it first, then try again.", spec.name), retry: false });
                return;
            }
            s.errors.remove(&spec.id);
            s.finished.remove(&spec.id);
            s.notes.remove(&spec.id);
            s.jobs.insert(spec.id.clone(), Job { phase: Phase::Removing, cancel: Arc::new(AtomicBool::new(false)), speed: 0.0, update: false, repair: false });
            record
        };
        let (shared, paths, spec, ctx) = (self.shared.clone(), self.paths.clone(), spec.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = crate::uninstall::uninstall(&spec, &record, &paths);
            let mut s = lock(&shared);
            s.jobs.remove(&spec.id);
            match result {
                Ok(()) => {
                    s.installed.apps.remove(&spec.id);
                    let _ = s.installed.save(&paths);
                    s.finished.insert(spec.id.clone(), (Instant::now(), format!("Removed {}. Your settings stay.", spec.name)));
                }
                Err(e) => {
                    s.errors.insert(spec.id.clone(), Failure { message: e, retry: false });
                }
            }
            ctx.request_repaint();
        });
    }

    /// The "Remove StreamWave?" box.
    fn uninstall_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_uninstall.clone() else {
            return;
        };
        let Some(spec) = self.manifest.apps.iter().find(|a| a.id == id).cloned() else {
            self.confirm_uninstall = None;
            return;
        };
        let Some(record) = lock(&self.shared).installed.apps.get(&id).cloned() else {
            self.confirm_uninstall = None;
            return;
        };
        let accent = p().bad;
        let mut close = false;
        let mut go = false;
        let modal = egui::Modal::new(egui::Id::unique("uninstall-confirm"))
            .frame(egui::Frame::new().fill(p().panel).corner_radius(CornerRadius::ZERO).inner_margin(Margin::same(24)).stroke(Stroke::new(1.0, p().line)))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    if let Some(bytes) = icon_bytes(&spec.icon) {
                        ui.add(egui::Image::from_bytes(format!("bytes://{}.png", spec.icon), bytes).fit_to_exact_size(vec2(44.0, 44.0)));
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("Remove {}?", spec.name)).font(FontId::proportional(20.0)).family(bold()).color(p().text));
                        ui.label(RichText::new("Your settings stay.").color(p().muted));
                    });
                });
                ui.add_space(10.0);
                let version = if record.version.is_empty() { String::new() } else { format!(" {}", record.version) };
                let found = if record.external { ", found on this computer" } else { "" };
                ui.label(RichText::new(format!("{}{} ({}{found})", spec.name, version, record.kind.label())).color(p().text));
                ui.add(egui::Label::new(RichText::new(crate::uninstall::what_happens(&spec, &record)).color(p().muted)).wrap());
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let b = egui::Button::new(RichText::new("UNINSTALL").family(bold()).size(11.0).color(on(accent))).fill(accent).stroke(Stroke::NONE).corner_radius(CornerRadius::ZERO).min_size(vec2(112.0, 34.0));
                        go = ui.add(b).clicked();
                        let c = egui::Button::new(RichText::new("CANCEL").size(11.0).color(p().text)).fill(p().soft).stroke(Stroke::NONE).corner_radius(CornerRadius::ZERO).min_size(vec2(90.0, 34.0));
                        close = ui.add(c).clicked();
                    });
                });
            });
        if go {
            self.confirm_uninstall = None;
            self.start_uninstall(ctx, &spec);
        } else if close || modal.should_close() {
            self.confirm_uninstall = None;
        }
    }

    fn community_page(&mut self, ui: &mut Ui) {
        hud(ui, "02 \u{2014} COMMUNITY", 10.0, p().muted);
        ui.add_space(6.0);
        ui.label(RichText::new("Join the community").font(FontId::proportional(28.0)).family(bold()).color(p().text));
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new("Every Wave app is free. Ask questions, share ideas and show what you've made in each app's Discussions on GitHub.").size(13.0).color(p().muted)).wrap());
        ui.add_space(16.0);
        for (i, spec) in self.manifest.apps.clone().iter().enumerate() {
            let accent = Color32::from_rgb(spec.accent[0], spec.accent[1], spec.accent[2]);
            egui::Frame::new().stroke(hairline()).corner_radius(CornerRadius::ZERO).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (num, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                    ui.painter().rect_filled(num, CornerRadius::ZERO, accent);
                    ui.painter().text(num.center(), egui::Align2::CENTER_CENTER, format!("{:02}", i + 1), FontId::new(10.0, egui::FontFamily::Monospace), on(accent));
                    if let Some(bytes) = icon_bytes(&spec.icon) {
                        ui.add(egui::Image::from_bytes(format!("bytes://{}.png", spec.icon), bytes).fit_to_exact_size(vec2(30.0, 30.0)));
                    }
                    ui.label(RichText::new(&spec.name).size(16.0).family(bold()).color(p().text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let b = egui::Button::new(RichText::new("DISCUSSIONS").family(bold()).size(10.0).color(on(accent))).fill(accent).stroke(Stroke::NONE).corner_radius(CornerRadius::ZERO).min_size(vec2(0.0, 28.0));
                        if ui.add(b).clicked() {
                            crate::links::open(&format!("https://github.com/{}/discussions", spec.repo));
                        }
                        ui.add_space(12.0);
                        link(ui, "ISSUES \u{2197}", &format!("https://github.com/{}/issues", spec.repo));
                        ui.add_space(12.0);
                        link(ui, "RELEASES \u{2197}", &format!("https://github.com/{}/releases", spec.repo));
                    });
                });
            });
            ui.add_space(8.0);
        }
        ui.add_space(10.0);
        support_box(ui, false);
    }

    fn about_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        hud(ui, "03 \u{2014} ABOUT", 10.0, p().muted);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if let Some(bytes) = icon_bytes("wavehub") {
                ui.add(egui::Image::from_bytes("bytes://wavehub.png", bytes).fit_to_exact_size(vec2(72.0, 72.0)));
            }
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new("WAVE HUB").font(FontId::proportional(30.0)).family(bold()).color(p().text));
                hud(ui, &format!("VERSION {} \u{00b7} MIT LICENCE \u{00b7} \u{00a9} 2026 DEL (POSTMANPAT)", env!("CARGO_PKG_VERSION")), 9.0, p().muted);
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new("Wave Hub is free and always will be. No account, no ads, no tracking. It installs and updates BigWave Radio, ChipWave Studio and StreamWave on Windows, Linux and Mac, straight from their official GitHub releases.").size(12.5).color(p().text)).wrap());
            });
        });
        ui.add_space(16.0);
        let (left_ui, right_ui) = {
            let mut cols: Vec<Ui> = Vec::new();
            let full = ui.available_rect_before_wrap();
            let gap = 18.0;
            let w = (full.width() - gap) / 2.0;
            for i in 0..2 {
                let rect = egui::Rect::from_min_size(full.min + vec2(i as f32 * (w + gap), 0.0), vec2(w, full.height().max(10.0)));
                cols.push(ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min))));
            }
            let right = cols.pop().unwrap();
            (cols.pop().unwrap(), right)
        };
        let (mut left_ui, mut right_ui) = (left_ui, right_ui);
        self.about_left(&mut left_ui, &ctx);
        support_box(&mut right_ui, true);
        credits_box(&mut right_ui);
        let used = left_ui.min_rect().height().max(right_ui.min_rect().height());
        ui.allocate_space(vec2(ui.available_width(), used));
    }

    fn about_left(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let ctx = ctx.clone();
        section(ui, "Updates", |ui| {
            let hub = lock(&self.shared).hub_latest.clone();
            let current = env!("CARGO_PKG_VERSION");
            let (text, color) = match &hub {
                None | Some(Latest::Checking) => ("Checking for a newer Wave Hub…".to_string(), p().muted),
                Some(Latest::Found(tag)) if github::is_newer(tag, current) => (format!("Wave Hub {} is available.", tag.trim_start_matches('v')), p().warn),
                Some(Latest::Found(_)) => ("You have the newest Wave Hub.".to_string(), p().good),
                Some(Latest::NoReleases) => ("No newer version. You're up to date.".to_string(), p().good),
                Some(Latest::Error(e)) => (format!("Couldn't check: {e}"), p().bad),
            };
            ui.horizontal_wrapped(|ui| {
                if ui.add(egui::Button::new(RichText::new("CHECK FOR UPDATES").size(10.0).family(bold()).color(p().text)).corner_radius(CornerRadius::ZERO)).clicked() {
                    self.check_hub_only(&ctx);
                }
                ui.label(RichText::new(text).color(color));
                if let Some(Latest::Found(tag)) = hub.as_ref().filter(|h| matches!(h, Latest::Found(t) if github::is_newer(t, current))) {
                    let repo = &self.manifest.launcher_repo;
                    // Straight to the file that matches how this copy was installed.
                    let asset = lock(&self.shared).hub_asset.clone();
                    match asset {
                        Some(Ok(Some(file))) => link(ui, &format!("Download {file}"), &github::download_url(repo, tag, &file)),
                        None => {}
                        Some(_) => link(ui, "Download", &github::release_page(repo, tag)),
                    }
                    link(ui, "What's new", &github::release_page(repo, tag));
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
                if ui.checkbox(&mut v, "Use official installers instead of portable apps").changed() {
                    self.settings.prefer_installer = v;
                    let _ = self.settings.save(&self.paths);
                }
                ui.add(
                    egui::Label::new(RichText::new("The portable version needs no admin rights. StreamWave always uses its own installer, run quietly in the background.").color(p().muted).small())
                        .wrap(),
                );
                ui.add_space(4.0);
                let mut v = self.settings.shortcuts;
                if ui.checkbox(&mut v, "Create Start Menu and Desktop shortcuts").changed() {
                    self.settings.shortcuts = v;
                    let _ = self.settings.save(&self.paths);
                    if v {
                        self.repair_shortcuts();
                    }
                }
                ui.add(egui::Label::new(RichText::new("For portable apps. The official installers make their own.").color(p().muted).small()).wrap());
            } else {
                ui.add(egui::Label::new(RichText::new("Apps are installed as AppImages in your home folder. No password needed.").color(p().muted)).wrap());
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Apps folder:").color(p().muted));
                ui.label(RichText::new(self.paths.apps().display().to_string()).monospace().color(p().text));
                if ui.button("Open folder").clicked() {
                    install::reveal(&self.paths.apps());
                }
            });
        });
    }

    fn footer(&self, ui: &mut Ui) {
        let s = lock(&self.shared);
        let checked = match s.checked_at {
            _ if s.checking => "CHECKING GITHUB\u{2026}".to_string(),
            Some(t) => {
                let mins = t.elapsed().as_secs() / 60;
                if mins == 0 {
                    "CHECKED JUST NOW".into()
                } else {
                    format!("CHECKED {mins} MIN AGO")
                }
            }
            None => "NOT CHECKED YET".into(),
        };
        ui.horizontal(|ui| {
            hud(ui, &format!("WAVE HUB {} \u{00b7} {} \u{00b7} {}", env!("CARGO_PKG_VERSION"), checked, install::os_label().to_uppercase()), 9.0, p().muted);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                link(ui, "BUY ME A COFFEE \u{2197}", BUY_ME_A_COFFEE);
                ui.add_space(12.0);
                link(ui, "DISCUSSIONS \u{2197}", "https://github.com/postmanpat-303/WaveHub/discussions");
                ui.add_space(12.0);
                link(ui, "GITHUB \u{2197}", "https://github.com/postmanpat-303/WaveHub");
            });
        });
    }
}

/// Runs one install on the current (background) thread and records the outcome.
#[allow(clippy::too_many_arguments)]
fn run_one(shared: &Arc<Mutex<Shared>>, paths: &Paths, spec: &AppSpec, tag: &str, prefer_installer: bool, shortcuts: bool, cancel: &AtomicBool, ctx: &egui::Context) {
    let previous = lock(shared).installed.apps.get(&spec.id).cloned();
    let plan = plan_for(spec, prefer_installer, previous.as_ref());
    // Download speed: sampled every half second, smoothed so the ETA doesn't jump about.
    let mut sample: Option<(Instant, u64)> = None;
    let result = install::install(spec, tag, &plan, paths, previous.as_ref(), shortcuts, cancel, &mut |phase| {
        let mut s = lock(shared);
        let Some(job) = s.jobs.get_mut(&spec.id) else {
            return;
        };
        if let Phase::Downloading { done, .. } = phase {
            match sample {
                None => sample = Some((Instant::now(), done)),
                Some((t, d)) if t.elapsed().as_secs_f64() >= 0.5 => {
                    let rate = (done.saturating_sub(d)) as f64 / t.elapsed().as_secs_f64();
                    job.speed = if job.speed == 0.0 { rate } else { job.speed * 0.6 + rate * 0.4 };
                    sample = Some((Instant::now(), done));
                }
                _ => {}
            }
        }
        job.phase = phase;
        // The window repaints itself every frame while a job runs, so no need to ask here.
    });
    let mut s = lock(shared);
    let job = s.jobs.remove(&spec.id);
    let was_update = job.as_ref().is_some_and(|j| j.update);
    let was_repair = job.as_ref().is_some_and(|j| j.repair);
    match result {
        Ok(record) => {
            let message = if was_repair {
                format!("Repaired! {} {} is ready.", spec.name, record.version)
            } else if was_update {
                format!("Updated to {}!", record.version)
            } else {
                "Installed! Ready when you are.".to_string()
            };
            s.finished.insert(spec.id.clone(), (Instant::now(), message));
            s.installed.apps.insert(spec.id.clone(), record);
            if let Err(e) = s.installed.save(paths) {
                s.errors.insert(spec.id.clone(), Failure { message: format!("Installed, but couldn't save the list of installed apps ({e})."), retry: false });
            }
        }
        Err(e) if e == "Cancelled." => {
            s.notes.insert(
                spec.id.clone(),
                (
                    Instant::now(),
                    if was_repair {
                        "Repair cancelled.".into()
                    } else if was_update {
                        "Update cancelled.".into()
                    } else {
                        "Download cancelled.".into()
                    },
                ),
            );
        }
        Err(e) => {
            s.errors.insert(spec.id.clone(), Failure { message: e, retry: true });
        }
    }
    ctx.request_repaint();
}

impl eframe::App for WaveHub {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("header").frame(egui::Frame::new().fill(p().bg).inner_margin(Margin::symmetric(22, 10))).show(ui, |ui| self.header(ui));
        egui::Panel::bottom("footer").frame(egui::Frame::new().fill(p().bg).inner_margin(Margin::symmetric(22, 8))).show(ui, |ui| self.footer(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p().bg).inner_margin(Margin::symmetric(22, 16))).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Apps => self.apps_page(ui),
                Tab::Community => self.community_page(ui),
                Tab::About => self.about_page(ui),
            });
        });
        let ctx = ui.ctx().clone();
        self.uninstall_dialog(&ctx);
        link_failed_dialog(&ctx);
        // Smooth animation while anything is moving; otherwise just keep "Checked n min ago" fresh.
        let busy = {
            let s = lock(&self.shared);
            !s.jobs.is_empty() || !s.finished.is_empty() || !s.notes.is_empty()
        };
        if busy {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(30));
        }
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
    v.window_fill = pal.card;
    v.faint_bg_color = pal.soft;
    v.extreme_bg_color = pal.soft;
    v.hyperlink_color = pal.info;
    v.selection.bg_fill = if pal.dark { HUB_PINK.gamma_multiply(0.7) } else { tint(HUB_PINK, 0.20) };
    v.selection.stroke = Stroke::new(1.0, pal.text);
    v.window_stroke = hairline();
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    v.widgets.noninteractive.bg_stroke = hairline();
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
    v.widgets.inactive.corner_radius = CornerRadius::ZERO;
    v.widgets.inactive.bg_fill = pal.soft;
    v.widgets.inactive.weak_bg_fill = pal.soft;
    v.widgets.inactive.bg_stroke = hairline();
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text);
    v.widgets.hovered.corner_radius = CornerRadius::ZERO;
    v.widgets.hovered.bg_fill = HUB_PINK;
    v.widgets.hovered.weak_bg_fill = HUB_PINK;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, HUB_PINK);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, on(HUB_PINK));
    v.widgets.active.corner_radius = CornerRadius::ZERO;
    v.widgets.active.bg_fill = HUB_PINK;
    v.widgets.active.weak_bg_fill = HUB_PINK;
    v.widgets.active.bg_stroke = Stroke::new(1.0, HUB_PINK);
    v.widgets.active.fg_stroke = Stroke::new(1.0, on(HUB_PINK));
    v.widgets.open.corner_radius = CornerRadius::ZERO;
    v.widgets.open.bg_stroke = hairline();
    ctx.set_visuals(v);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = vec2(9.0, 8.0);
        style.spacing.button_padding = vec2(10.0, 5.0);
        // Scroll bars float over the content, so the cards never change width when one appears.
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.0));
        style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(11.5));
        style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.0));
    });
}

fn speed_text(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0)
    } else {
        format!("{:.0} KB/s", bytes_per_sec / 1000.0)
    }
}

fn eta_text(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    match s {
        0..=1 => "almost done".into(),
        2..=59 => format!("{s} s left"),
        60..=3599 => format!("{} min {} s left", s / 60, s % 60),
        _ => format!("{} h {} min left", s / 3600, (s % 3600) / 60),
    }
}

/// A squared bar: a paper-ink track, the app's colour fill, a pale band sliding over it.
fn progress_bar(ui: &mut Ui, fraction: f32, color: Color32, width: f32, time: f64) {
    let (rect, _) = ui.allocate_exact_size(vec2(width.max(40.0), 8.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect_filled(rect, CornerRadius::ZERO, p().soft);
    let w = (rect.width() * fraction.clamp(0.0, 1.0)).max(6.0);
    let fill = egui::Rect::from_min_size(rect.min, vec2(w, rect.height()));
    painter.rect_filled(fill, CornerRadius::ZERO, color);
    let band = 60.0;
    let x = fill.left() - band + ((time as f32 * 120.0) % (fill.width() + band * 2.0));
    let sheen = egui::Rect::from_min_max(egui::pos2(x.max(fill.left()), fill.top()), egui::pos2((x + band).min(fill.right()), fill.bottom()));
    if sheen.width() > 0.0 {
        painter.with_clip_rect(fill).rect_filled(sheen, CornerRadius::ZERO, Color32::from_white_alpha(40));
    }
    ui.painter().rect_stroke(rect, CornerRadius::ZERO, hairline(), egui::StrokeKind::Inside);
}

/// Download · Check · Install, with the current step lit.
fn stage_dots(ui: &mut Ui, phase: &Phase, accent: Color32) {
    let step = match phase {
        Phase::Queued | Phase::Checking | Phase::Downloading { .. } => 0,
        Phase::Verifying => 1,
        Phase::Installing | Phase::RunningInstaller | Phase::Removing => 2,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        for (i, label) in ["DOWNLOAD", "CHECK", "INSTALL"].iter().enumerate() {
            let (dot, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
            let c = if i <= step { accent } else { p().line };
            ui.painter().rect_filled(dot, CornerRadius::ZERO, c);
            hud(ui, label, 8.5, if i == step { p().text } else { p().muted });
            ui.add_space(9.0);
        }
    });
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

fn main_button(ui: &mut Ui, label: &str, accent: Color32, size: egui::Vec2, enabled: bool) -> egui::Response {
    // Paper on ink (or ink on paper); the app's colour takes over under the pointer.
    let (bg, fg) = if enabled { invert() } else { (Color32::TRANSPARENT, p().muted.gamma_multiply(0.8)) };
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let mut fill = bg;
    let mut text_color = fg;
    let mut border = if enabled { bg } else { p().line };
    if enabled && response.is_pointer_button_down_on() {
        fill = accent.gamma_multiply(0.85);
        text_color = on(fill);
        border = fill;
    } else if enabled && response.hovered() {
        fill = accent;
        text_color = on(accent);
        border = accent;
    }
    if ui.is_rect_visible(rect) {
        ui.painter().rect(rect, CornerRadius::ZERO, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
        let galley = ui.painter().layout_job(hud_job(label, 10.5, text_color));
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, text_color);
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A small announcement on an app's card (apps.json `notice`): a hairline strip in the
/// app's colour, with a bold lead-in.
fn notice_note(ui: &mut Ui, notice: &str, accent: Color32) {
    let (head, rest) = match notice.split_once("! ") {
        Some((h, r)) if h.len() <= 24 => (format!("{h}!"), r.trim().to_string()),
        _ => (String::new(), notice.trim().to_string()),
    };
    let fill = tint(accent, if p().dark { 0.14 } else { 0.08 });
    egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, accent)).corner_radius(CornerRadius::ZERO).inner_margin(Margin::symmetric(10, 7)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut job = egui::text::LayoutJob::default();
        if !head.is_empty() {
            job.append(&head, 0.0, egui::TextFormat { font_id: FontId::new(12.0, bold()), color: accent, ..Default::default() });
            job.append(" ", 0.0, egui::TextFormat { font_id: FontId::new(12.0, egui::FontFamily::Proportional), color: p().text, ..Default::default() });
        }
        job.append(&rest, 0.0, egui::TextFormat { font_id: FontId::new(12.0, egui::FontFamily::Proportional), color: p().text, ..Default::default() });
        ui.add(egui::Label::new(job).wrap());
    });
}

fn status_chip(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_job(hud_job(text, 8.5, color));
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 22.0), Sense::hover());
    let fill = tint(color, if p().dark { 0.12 } else { 0.08 });
    ui.painter().rect(rect, CornerRadius::ZERO, fill, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}

fn link(ui: &mut Ui, text: &str, url: &str) {
    let galley = ui.painter().layout_job(hud_job(text, 9.0, p().muted));
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 2.0, 18.0), Sense::click());
    let color = if response.hovered() { p().text } else { p().muted };
    let galley = ui.painter().layout_job(hud_job(text, 9.0, color));
    ui.painter().galley(egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, color);
    if response.clicked() {
        crate::links::open(url);
    }
    response.on_hover_text(url).on_hover_cursor(egui::CursorIcon::PointingHand);
}

fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new().stroke(hairline()).corner_radius(CornerRadius::ZERO).inner_margin(Margin::symmetric(16, 14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        hud(ui, &title.to_uppercase(), 9.5, p().muted);
        ui.add_space(8.0);
        body(ui);
    });
    ui.add_space(12.0);
}

fn credits_box(ui: &mut Ui) {
    section(ui, "Design credit", |ui| {
        ui.add(
            egui::Label::new(
                RichText::new(
                    "The look of this window \u{2014} the hard edges, hairline rules, numbered cards and small-caps labels \u{2014} is modelled on ArtCraft Launcher by storytold. Their one-click install and update flow showed the way too. Wave Hub's code and artwork are its own.",
                )
                .size(12.5)
                .color(p().muted),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            link(ui, "ArtCraft Launcher", "https://github.com/storytold/craft-launcher");
            link(ui, "storytold", "https://github.com/storytold");
            link(ui, "getartcraft.com", "https://getartcraft.com/");
        });
    });
}

/// "Couldn't open your browser", with the address to copy by hand.
fn link_failed_dialog(ctx: &egui::Context) {
    let Some((url, why)) = crate::links::FAILED.lock().unwrap_or_else(|p| p.into_inner()).clone() else { return };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("link-failed")).frame(egui::Frame::new().fill(p().panel).corner_radius(CornerRadius::ZERO).inner_margin(Margin::same(24)).stroke(Stroke::new(1.0, p().line))).show(
        ctx,
        |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new("Couldn't open your browser").font(FontId::proportional(19.0)).family(bold()).color(p().text));
            ui.add(egui::Label::new(RichText::new(format!("{why}. You can copy the address into your browser instead:")).color(p().muted)).wrap());
            ui.add_space(6.0);
            let mut text = url.clone();
            ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY));
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = ui.add(egui::Button::new(RichText::new("OK").color(p().text)).fill(p().soft).stroke(Stroke::NONE).corner_radius(CornerRadius::ZERO).min_size(vec2(80.0, 32.0))).clicked();
            });
        },
    );
    if close || modal.should_close() {
        *crate::links::FAILED.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

fn support_box(ui: &mut Ui, with_paypal: bool) {
    section(ui, "Support Del's work", |ui| {
        ui.add(egui::Label::new(RichText::new("All Wave apps are free. If they make your day better, a coffee keeps the updates coming. Totally optional.").color(p().muted)).wrap());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let coffee =
                egui::Button::new(RichText::new("BUY ME A COFFEE").family(bold()).size(10.0).color(INK)).fill(Color32::from_rgb(255, 221, 0)).stroke(Stroke::NONE).corner_radius(CornerRadius::ZERO).min_size(vec2(0.0, 32.0));
            if ui.add(coffee).clicked() {
                crate::links::open(BUY_ME_A_COFFEE);
            }
            if with_paypal {
                let paypal = egui::Button::new(RichText::new("PAYPAL").family(bold()).size(10.0).color(Color32::WHITE))
                    .fill(Color32::from_rgb(0, 112, 186))
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::ZERO)
                    .min_size(vec2(0.0, 30.0));
                if ui.add(paypal).clicked() {
                    crate::links::open(PAYPAL);
                }
            }
        });
    });
}

