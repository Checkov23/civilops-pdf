//! Help ▸ Check for updates (issue #28): ask for the latest release and offer its download page.
//!
//! The desktop app supplies how to ask ([`PrintCraftApp::update_source`]), so this crate has no
//! network code; without a source (the web build, tests) the command opens the releases page.
//! PrintCraft never downloads or installs anything itself: the user downloads the new version.
//! It asks only when the user does: there is no check at start (the owner's decision).
//!
//! CivilOps PDF updates itself, like CivilOps CAD: its source downloads a newer version, and
//! [`PrintCraftApp::update_apply`] installs it after the app closes and starts the new one ("Restart
//! now", only with nothing unsaved; otherwise it installs at the next start). With
//! [`PrintCraftApp::update_check_on_launch`] the source is asked once at launch, quietly: the dialog
//! opens only when an update is ready.

use std::sync::Arc;

use egui::{Align, Layout};

use crate::{PrintCraftApp, theme, widgets};

/// Where every PrintCraft release is listed.
pub const RELEASES_PAGE: &str = "https://civilops.de";

/// The latest published release.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    /// Its version tag, such as `v0.2.0`.
    pub version: String,
    /// Its page on [`RELEASES_PAGE`], where the downloads are.
    pub url: String,
    /// What's new in it (Markdown, may be empty).
    pub notes: String,
}

/// Asks for the latest release (blocking; it runs on its own thread).
pub type UpdateSource = Arc<dyn Fn() -> Result<Release, String> + Send + Sync>;

/// Installs the update the source downloaded once the app has closed, then starts the new version.
/// Called on the UI thread; the app closes right after it succeeds.
pub type UpdateApply = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

/// The version this build calls itself (About, Check for updates).
pub fn current_version() -> &'static str {
    printcraft_engine::links::VERSION
}

/// Whether release `latest` (a tag such as `v0.2.0`) is newer than version `current` (`0.1.1`).
/// Pre-release and build suffixes are ignored; a version that doesn't parse is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = |required: bool| match parts.next() {
        Some(p) => p.parse::<u64>().ok(),
        None if required => None,
        None => Some(0),
    };
    let version = (next(true)?, next(false)?, next(false)?);
    parts.next().is_none().then_some(version)
}

/// Where a check is.
#[derive(Default)]
pub(crate) enum Check {
    #[default]
    Idle,
    #[cfg(not(target_arch = "wasm32"))]
    Running(std::sync::mpsc::Receiver<Result<Release, String>>),
    Done(Result<Release, String>),
}

#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // the web build never checks at launch
pub(crate) struct Updates {
    pub(crate) check: Check,
    /// The Updates dialog is showing.
    pub(crate) open: bool,
    /// The running check is the quiet one from launch: it opens the dialog only for an update.
    pub(crate) quiet: bool,
    /// The check at launch has been started.
    pub(crate) launched: bool,
    /// Why installing the downloaded update failed ("Restart now").
    pub(crate) apply_error: Option<String>,
}

impl PrintCraftApp {
    /// Help ▸ Check for updates: ask for the latest release and show the outcome.
    pub fn check_for_updates(&mut self) {
        if self.update_source.is_none() || cfg!(target_arch = "wasm32") {
            self.open_url(RELEASES_PAGE);
            return;
        }
        self.updates.open = true;
        self.updates.quiet = false;
        self.start_update_check();
    }

    /// Ask the source on its own thread, unless a check is already running.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_update_check(&mut self) {
        let Some(source) = self.update_source.clone() else { return };
        if matches!(self.updates.check, Check::Running(_)) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            // The receiver may be gone (the app quit): nothing to report to then.
            let _ = tx.send(source());
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.updates.check = Check::Running(rx);
    }

    #[cfg(target_arch = "wasm32")]
    fn start_update_check(&mut self) {}

    /// Start the check at launch once, and pick up a finished check (each frame).
    pub(crate) fn poll_updates(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.update_check_on_launch && !self.updates.launched {
                self.updates.launched = true;
                self.updates.quiet = !self.updates.open;
                self.start_update_check();
            }
            if let Check::Running(rx) = &self.updates.check {
                let result = match rx.try_recv() {
                    Ok(r) => r,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the update check stopped unexpectedly".into()),
                };
                if std::mem::take(&mut self.updates.quiet) {
                    // At launch, speak up only when there is something to install.
                    self.updates.open = matches!(&result, Ok(r) if is_newer(&r.version, current_version()));
                }
                self.updates.check = Check::Done(result);
            }
        }
    }
}

/// The Updates dialog.
pub(crate) fn dialog(app: &mut PrintCraftApp, ctx: &egui::Context) {
    if !app.updates.open {
        return;
    }
    let t = theme::Tokens::get(ctx);
    let current = current_version();
    // CivilOps PDF installs downloaded updates itself; restarting needs everything saved.
    let installs = app.update_apply.is_some();
    let unsaved = app.first_dirty().is_some();
    let mut close = false;
    let mut download: Option<String> = None;
    let mut ready = false;
    let mut restart = false;
    let modal = egui::Modal::new(egui::Id::new("updates")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.horizontal(|ui| {
            ui.add(crate::icons::image("cloud", 22.0, t.accent));
            ui.label(egui::RichText::new(tl!("Check for updates")).font(theme::semibold(16.0)));
        });
        ui.add_space(8.0);
        match &app.updates.check {
            Check::Idle => {
                ui.label(tl!("No check has run yet."));
            }
            #[cfg(not(target_arch = "wasm32"))]
            Check::Running(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tl!("Checking for a newer version…"));
                });
            }
            Check::Done(Ok(r)) if is_newer(&r.version, current) && installs => {
                let version = r.version.trim_start_matches(['v', 'V']);
                ui.label(egui::RichText::new(format!("CivilOps PDF {version} has been downloaded.")).strong());
                ui.label(
                    egui::RichText::new(format!(
                        "You have version {current}. The new version is installed when you restart CivilOps PDF, now or next time."
                    ))
                    .color(t.text_muted),
                );
                let notes = r.notes.trim();
                if !notes.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("What's new").font(theme::semibold(13.0)));
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        ui.label(notes);
                    });
                }
                if unsaved {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Save your changes first to restart now.").color(t.text_muted));
                }
                if let Some(e) = &app.updates.apply_error {
                    ui.add_space(6.0);
                    ui.label(format!("Couldn't install the update: {e}"));
                }
                ready = true;
            }
            Check::Done(Ok(r)) if is_newer(&r.version, current) => {
                let version = r.version.trim_start_matches(['v', 'V']);
                ui.label(egui::RichText::new(format!("CivilOps PDF {version} is available.")).strong());
                ui.label(
                    egui::RichText::new(crate::i18n::fmt(
                        tl!("You have version {c}. Download the new version from its release page."),
                        &[("c", current)],
                    ))
                    .color(t.text_muted),
                );
                download = Some(r.url.clone());
            }
            Check::Done(Ok(_)) => {
                ui.label(format!("CivilOps PDF {current} is up to date."));
            }
            Check::Done(Err(e)) => {
                ui.label(crate::i18n::fmt(tl!("Couldn't check for updates: {e}"), &[("e", &e.to_string())]));
                ui.label(
                    egui::RichText::new(crate::i18n::fmt(
                        tl!("You have version {c}. All releases are listed at {page}."),
                        &[("c", current), ("page", RELEASES_PAGE)],
                    ))
                    .color(t.text_muted),
                );
            }
        }
        ui.add_space(10.0);
        let footer = if installs {
            "Updates come from download.civilops.de. Your documents and settings stay as they are."
        } else {
            "Asks GitHub for the latest release. Nothing is downloaded or installed automatically."
        };
        ui.label(egui::RichText::new(footer).color(t.text_muted).small());
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ready {
                restart = ui.add_enabled_ui(!unsaved, |ui| widgets::pill_button(ui, "Restart now", true)).inner.clicked();
                close = widgets::pill_button(ui, "Later", false).clicked();
            } else if let Some(url) = download.take() {
                let get = widgets::pill_button(ui, "Download", true).clicked();
                let later = widgets::pill_button(ui, "Later", false).clicked();
                close = get || later;
                download = get.then_some(url);
            } else if widgets::pill_button(ui, tl!("Close"), true).clicked() {
                close = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
        download = None;
        restart = false;
    }
    if restart && let Some(apply) = app.update_apply.clone() {
        match apply() {
            // The updater waits for this app to close, installs, and starts the new version.
            Ok(()) => {
                app.updates.open = false;
                app.quit(ctx);
            }
            Err(e) => app.updates.apply_error = Some(e),
        }
        return;
    }
    if close {
        app.updates.open = false;
        if let Some(url) = download {
            app.open_url(&url);
        }
    }
}
