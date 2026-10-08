//! PrintCraft desktop app.
//!
//! Usage: `printcraft [options] [files…]`
//!
//! View options (applied after the files open; also the seed of the UI control channel):
//! `--page N  --zoom 150  --layout continuous|two-up|single  --panel comments|bookmarks|pages|fields|layers|attachments|none
//!  --theme light|dark  --mode all|read|edit|convert|sign  --tool <catalogue id>  --left open|closed
//!  --organize on  --fields on  --dialog properties|shortcuts|about  --palette <query>  --home on`
//!
//! `--control <file>` enables the UI control channel (off by default): the app listens on a random
//! loopback port and writes `{"port", "token", "pid"}` to `<file>` (owner-only permissions).
//! Agents then drive it with `printcraft-cli ui --control <file> <method> …`.

// Release builds on Windows are GUI-subsystem programs, so launching the app doesn't open a console
// window next to it (#57). `--version` and diagnostics then go nowhere when started from a terminal
// (std ignores the missing console handles, so nothing fails); debug builds keep the console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use printcraft_ui_egui::PrintCraftApp;

#[cfg(target_os = "macos")]
mod apple_events;

/// Freedesktop app id: the `.desktop` file name and the hicolor icon name.
const APP_ID: &str = "de.civilops.pdf";

/// The app icon (assets/app-icon/README.md). macOS gets the version on Apple's icon grid, with a
/// transparent margin; Windows and Linux get the full-bleed tile.
#[cfg(target_os = "macos")]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/civilops/civilops-pdf-1024.png");
#[cfg(not(target_os = "macos"))]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/civilops/civilops-pdf-256.png");

fn main() -> eframe::Result {
    // CivilOps PDF installs and updates itself like CivilOps CAD (Velopack). This comes first: it
    // serves the installer's hooks (and exits for them) and installs an update that was downloaded
    // last time, unless another CivilOps PDF window is open.
    civilops_aktualisierung::beim_start();
    // Last-resort guard (AGENTS.md §4): commands, edits, opens and saves catch panics and report
    // them; this hook logs every panic, caught or not, with a backtrace when RUST_BACKTRACE is set.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("printcraft: internal error: {info}");
        let trace = std::backtrace::Backtrace::capture();
        if trace.status() == std::backtrace::BacktraceStatus::Captured {
            eprintln!("{trace}");
        }
    }));
    let mut files = Vec::new();
    let mut options: Vec<(String, String)> = Vec::new();
    let mut control_file: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" => {
                println!("civilops-pdf {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--control" => control_file = args.next(),
            flag if flag.starts_with("--") => {
                let value = args.next().unwrap_or_default();
                options.push((flag.trim_start_matches("--").to_string(), value));
            }
            _ => files.push(a),
        }
    }
    let integrated = cfg!(target_os = "macos");
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("CivilOps PDF")
        .with_inner_size([1440.0, 920.0])
        .with_min_inner_size([820.0, 520.0])
        .with_drag_and_drop(true)
        // Wayland app id.
        .with_app_id(APP_ID);
    // Dock, taskbar, Alt-Tab and launcher icon when running unbundled.
    match eframe::icon_data::from_png_bytes(APP_ICON_PNG) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => eprintln!("printcraft: app icon: {e}"),
    }
    if integrated {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    // eframe would otherwise derive the settings folder from the app id: keep it under "CivilOps PDF".
    let persistence_path = eframe::storage_dir("CivilOps PDF").map(|d| d.join("app.ron"));
    let mut native = eframe::NativeOptions { viewport, persistence_path, ..Default::default() };
    configure_gpu(&mut native);
    // Finder, Open With and the Dock deliver files as Apple events, not arguments; catch the one
    // that launched us as well as later ones. Lives until the event loop returns.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let apple_events = &apple_events;
    eframe::run_native(
        "CivilOps PDF",
        native,
        Box::new(move |cc| {
            let mut app = PrintCraftApp::new();
            // CivilOps PDF: German unless another language was chosen; every label is translated.
            app.language = printcraft_ui_egui::i18n::Language::De;
            egui::set_translator(printcraft_ui_egui::i18n::translate);
            if let Some(json) = cc.storage.and_then(|s| s.get_string("printcraft")) {
                app.restore(&json);
            }
            app.integrated_titlebar = integrated;
            // CivilOps PDF: tool palette on top, menus in the tab strip, the plan fills the window
            // (every other tool stays under "All tools").
            app.civilops_layout = true;
            app.left_open = false;
            // CivilOps PDF updates itself: a quiet check at launch downloads a newer version, which
            // installs at the next start or with "Restart now".
            app.update_source = Some(std::sync::Arc::new(check_for_update));
            if civilops_aktualisierung::installierte_version().is_some() {
                app.update_apply = Some(std::sync::Arc::new(civilops_aktualisierung::nach_dem_beenden_einspielen));
                app.update_check_on_launch = true;
            }
            app.keychain_ids = cfg!(target_os = "macos");
            #[cfg(target_os = "macos")]
            {
                app.os_events = Some(apple_events.connect(&cc.egui_ctx));
            }
            if let Some(file) = &control_file {
                let client = app.attach_control(&cc.egui_ctx);
                match printcraft_ui_egui::control::serve(client).and_then(|ep| write_control_file(file, ep.port, &ep.token).map(|()| ep.port)) {
                    Ok(port) => eprintln!("printcraft: UI control channel on 127.0.0.1:{port} (connection details in {file})"),
                    Err(e) => eprintln!("printcraft: --control {file}: {e}"),
                }
            }
            // Autosave unsaved changes; offer to recover documents a crashed session left behind.
            if let Some(dir) = printcraft_ui_egui::RecoveryStore::default_dir() {
                app.enable_recovery(printcraft_ui_egui::RecoveryStore::new(dir));
            }
            for f in files {
                app.open_path(&f);
            }
            for (k, v) in options {
                if let Err(e) = app.set_option(&k, &v) {
                    eprintln!("printcraft: --{k} {v}: {e}");
                }
            }
            Ok(Box::new(app))
        }),
    )
}

/// Help ▸ Check for updates and the check at launch: download a newer CivilOps PDF if there is one
/// (it installs later). Answers with the version that is ready, or with the current one.
fn check_for_update() -> Result<printcraft_ui_egui::updates::Release, String> {
    use printcraft_ui_egui::updates::{Release, current_version};
    if civilops_aktualisierung::installierte_version().is_none() {
        return Err("this copy wasn't set up with the CivilOps PDF installer, so it can't update itself".into());
    }
    Ok(match civilops_aktualisierung::pruefen_und_laden()? {
        Some(update) => Release { version: update.version, notes: update.notizen, ..Default::default() },
        None => Release { version: current_version().into(), ..Default::default() },
    })
}

/// Write the control endpoint so that only the current user can read the token.
fn write_control_file(path: &str, port: u16, token: &str) -> std::io::Result<()> {
    let json = serde_json::json!({ "port": port, "token": token, "pid": std::process::id() }).to_string();
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(json.as_bytes())
}

/// How wgpu finds a GPU. Each choice yields to its wgpu environment variable.
///
/// - Draw on the integrated GPU unless `WGPU_POWER_PREF` says otherwise. A PDF viewer has no use
///   for a discrete GPU, and on hybrid-graphics laptops (NVIDIA Optimus) the discrete one can lose
///   or corrupt its memory across suspend and screen lock, leaving the window illegible (issue #8).
///   It also saves battery. Machines with one GPU are unaffected.
/// - On Windows, use Direct3D 12, falling back to OpenGL, and never load Vulkan drivers unless
///   `WGPU_BACKEND` asks for them. Creating a Vulkan instance loads every installed Vulkan driver
///   into the process, and a faulty one (an Intel driver in issue #37) crashed PrintCraft before
///   its window appeared. D3D12 is the native, best-supported backend there.
fn configure_gpu(native: &mut eframe::NativeOptions) {
    let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut native.wgpu_options.wgpu_setup else { return };
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        setup.power_preference = eframe::wgpu::PowerPreference::LowPower;
    }
    if cfg!(target_os = "windows") && std::env::var_os("WGPU_BACKEND").is_none() {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::GL;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_version_is_the_same_everywhere() {
        // The installer's package, `--version`, About and Check for updates must agree; the
        // release script checks it too.
        assert_eq!(env!("CARGO_PKG_VERSION"), printcraft_ui_egui::updates::current_version());
    }

    #[test]
    fn a_copy_without_installer_says_it_cannot_update_itself() {
        let e = super::check_for_update().unwrap_err();
        assert!(e.contains("installer"), "{e}");
    }

    #[test]
    fn gpu_backends_avoid_vulkan_on_windows_and_prefer_low_power() {
        let mut native = eframe::NativeOptions::default();
        super::configure_gpu(&mut native);
        let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &native.wgpu_options.wgpu_setup else {
            panic!("default setup creates its own instance")
        };
        if std::env::var_os("WGPU_POWER_PREF").is_none() {
            assert_eq!(setup.power_preference, eframe::wgpu::PowerPreference::LowPower);
        }
        if cfg!(target_os = "windows") && std::env::var_os("WGPU_BACKEND").is_none() {
            let backends = setup.instance_descriptor.backends;
            assert!(backends.contains(eframe::wgpu::Backends::DX12), "{backends:?}");
            assert!(!backends.contains(eframe::wgpu::Backends::VULKAN), "issue #37: {backends:?}");
        }
    }
}
