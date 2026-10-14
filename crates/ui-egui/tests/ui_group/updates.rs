//! Help ▸ Check for updates (issue #28), with stand-in release sources (no network).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use printcraft_ui_egui::PrintCraftApp;
use printcraft_ui_egui::updates::{Release, UpdateSource, current_version, is_newer};

fn source(answer: Result<&str, &str>) -> UpdateSource {
    let answer = answer.map(str::to_string).map_err(str::to_string);
    Arc::new(move || {
        answer.clone().map(|v| Release { url: format!("https://github.com/storytold/printcraft/releases/tag/{v}"), version: v, ..Default::default() })
    })
}

fn harness(answer: Result<&str, &str>) -> Harness<'static, PrintCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.update_source = Some(source(answer));
        app
    })
}

/// Run frames until the background check has reported (or give up).
fn settle(h: &mut Harness<'static, PrintCraftApp>) {
    for _ in 0..200 {
        h.run_steps(2);
        if h.query_by_label_contains("Checking for a newer version").is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
}

#[test]
fn versions_compare_by_number() {
    assert!(is_newer("v0.2.0", "0.1.1"));
    assert!(is_newer("v0.1.10", "0.1.9"));
    assert!(is_newer("1", "0.9.9"));
    assert!(!is_newer("v0.1.1", "0.1.1"));
    assert!(!is_newer("v0.1.0", "0.1.1"));
    assert!(!is_newer("v0.1.1-beta.2", "0.1.1"), "suffixes are ignored");
    assert!(!is_newer("nightly", "0.1.1"), "a tag that isn't a version is never newer");
    assert!(!is_newer("v1.2.3.4", "0.1.1"));
    assert!(!is_newer("v99999999999999999999.0.0", "0.1.1"), "out of range");
}

#[test]
fn a_newer_release_is_offered_for_download() {
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("CivilOps PDF 99.0.0 is available");
    h.get_by_label("Download");
    h.get_by_label("Later").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("is available").is_none(), "Later closes the dialog");
}

#[test]
fn an_up_to_date_or_failed_check_says_so() {
    let mut h = harness(Ok(current_version()));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("is up to date");
    assert!(h.query_by_label("Download").is_none());

    let mut h = harness(Err("couldn't reach GitHub"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("Couldn't check for updates: couldn't reach GitHub");
}

#[test]
fn nothing_is_asked_until_the_user_checks() {
    // No check at start (the owner's decision): the source is only called from the command.
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.set_option("language", "en").unwrap();
        // Settings from a build that had the startup option are ignored.
        app.restore(r#"{"check_updates_at_start": true}"#);
        let counted = counted.clone();
        app.update_source = Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(Release {
                version: "v99.0.0".into(),
                url: "https://github.com/storytold/printcraft/releases/tag/v99.0.0".into(),
                ..Default::default()
            })
        }));
        app
    });
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(h.query_by_label_contains("is available").is_none());
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    h.get_by_label_contains("CivilOps PDF 99.0.0 is available");
}

// CivilOps PDF: the app downloads updates itself and installs them on a restart.

/// A one-page document with a proper xref table.
fn one_page() -> Vec<u8> {
    let objs =
        ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 300] >>", "<< /Type /Page /Parent 2 0 R >>"];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// An app that installs updates itself: `answer` is what the source downloaded, `installed`
/// counts the installs, `unsaved` leaves a document with an unsaved change open.
fn installing(
    answer: Result<&'static str, &'static str>,
    apply: Result<(), &'static str>,
    installed: Arc<AtomicUsize>,
    unsaved: bool,
) -> Harness<'static, PrintCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.set_option("language", "en").unwrap();
        if unsaved {
            app.open_bytes("doc.pdf", None, one_page()).expect("fixture opens");
            app.apply_edit(printcraft_engine::Edit::RotatePages { pages: vec![0], degrees: 90 });
        }
        app.update_source = Some(Arc::new(move || {
            answer
                .map(|v| Release { version: v.into(), notes: "- Messen mit Fang\n- Objekte ändern".into(), ..Default::default() })
                .map_err(str::to_string)
        }));
        app.update_apply = Some(Arc::new(move || {
            installed.fetch_add(1, Ordering::SeqCst);
            apply.map_err(str::to_string)
        }));
        app
    })
}

#[test]
fn a_downloaded_update_installs_on_restart() {
    let installed = Arc::new(AtomicUsize::new(0));
    let mut h = installing(Ok("0.99.0"), Ok(()), installed.clone(), false);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("CivilOps PDF 0.99.0 has been downloaded");
    h.get_by_label_contains("Messen mit Fang");
    assert!(h.query_by_label("Download").is_none(), "nothing to download by hand");
    h.get_by_label("Restart now").click();
    h.run_steps(3);
    assert_eq!(installed.load(Ordering::SeqCst), 1);
    assert!(h.query_by_label_contains("has been downloaded").is_none(), "the dialog closes and the app quits");
}

#[test]
fn later_leaves_the_update_for_the_next_start() {
    let installed = Arc::new(AtomicUsize::new(0));
    let mut h = installing(Ok("0.99.0"), Ok(()), installed.clone(), false);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label("Later").click();
    h.run_steps(3);
    assert_eq!(installed.load(Ordering::SeqCst), 0);
    assert!(h.query_by_label_contains("has been downloaded").is_none());
}

#[test]
fn restarting_waits_until_everything_is_saved() {
    let installed = Arc::new(AtomicUsize::new(0));
    let mut h = installing(Ok("0.99.0"), Ok(()), installed.clone(), true);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("Save your changes first to restart now");
    h.get_by_label("Restart now").click();
    h.run_steps(3);
    assert_eq!(installed.load(Ordering::SeqCst), 0, "unsaved changes: nothing is installed");
    h.get_by_label_contains("has been downloaded");
}

#[test]
fn a_failed_install_says_why() {
    let installed = Arc::new(AtomicUsize::new(0));
    let mut h = installing(Ok("0.99.0"), Err("Update.exe is missing"), installed.clone(), false);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label("Restart now").click();
    h.run_steps(3);
    assert_eq!(installed.load(Ordering::SeqCst), 1);
    h.get_by_label_contains("Couldn't install the update: Update.exe is missing");
}

/// Run frames until `done` holds (or give up).
fn until(h: &mut Harness<'static, PrintCraftApp>, done: impl Fn(&Harness<'static, PrintCraftApp>) -> bool) {
    for _ in 0..200 {
        h.run_steps(2);
        if done(h) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
}

#[test]
fn the_check_at_launch_speaks_up_only_when_an_update_is_ready() {
    for (answer, speaks) in [(Ok(current_version()), false), (Err("offline"), false), (Ok("0.99.0"), true)] {
        let asked = Arc::new(AtomicUsize::new(0));
        let counted = asked.clone();
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
            let mut app = PrintCraftApp::new();
            app.update_check_on_launch = true;
            // Allowed at an earlier start (the first one asks, see below).
            app.update_auto = Some(true);
            app.update_source = Some(Arc::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
                answer.map(|v| Release { version: v.into(), ..Default::default() }).map_err(str::to_string)
            }));
            app.update_apply = Some(Arc::new(|| Ok(())));
            app
        });
        let seen = asked.clone();
        until(&mut h, move |h| seen.load(Ordering::SeqCst) > 0 && h.query_by_label_contains("Checking for a newer version").is_none());
        // The thread counts the call before it hands over the answer: give it a moment.
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.run_steps(4);
        assert_eq!(asked.load(Ordering::SeqCst), 1, "asked once at launch: {answer:?}");
        assert_eq!(h.query_by_label_contains("has been downloaded").is_some(), speaks, "{answer:?}");
        assert!(h.query_by_label_contains("is up to date").is_none(), "quiet when there is nothing to install: {answer:?}");
        assert!(h.query_by_label_contains("Couldn't check").is_none(), "quiet when offline: {answer:?}");
    }
}

/// CivilOps PDF: an app that can update itself and has not asked yet; `asked` counts the checks.
fn first_start(asked: Arc<AtomicUsize>) -> Harness<'static, PrintCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.update_check_on_launch = true;
        app.update_source = Some(Arc::new(move || {
            asked.fetch_add(1, Ordering::SeqCst);
            Ok(Release { version: current_version().into(), ..Default::default() })
        }));
        app.update_apply = Some(Arc::new(|| Ok(())));
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn the_first_start_asks_before_looking_for_updates() {
    let asked = Arc::new(AtomicUsize::new(0));
    let mut h = first_start(asked.clone());
    h.get_by_label_contains("which sees your IP address and the program version");
    h.get_by_label("Look for updates automatically at start");
    h.run_steps(4);
    assert_eq!(asked.load(Ordering::SeqCst), 0, "nothing is asked before the answer");
    h.get_by_label("OK").click();
    let seen = asked.clone();
    until(&mut h, move |_| seen.load(Ordering::SeqCst) > 0);
    assert_eq!(h.state().update_auto, Some(true), "on unless switched off");
    assert_eq!(asked.load(Ordering::SeqCst), 1, "then the check at launch runs");
    assert!(h.query_by_label("Look for updates automatically at start").is_none(), "asked once");
}

#[test]
fn switched_off_the_start_stays_quiet_and_help_still_checks() {
    let asked = Arc::new(AtomicUsize::new(0));
    let mut h = first_start(asked.clone());
    h.get_by_label("Look for updates automatically at start").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(10);
    assert_eq!(h.state().update_auto, Some(false));
    assert_eq!(asked.load(Ordering::SeqCst), 0, "no check at launch");
    // Kept with the settings, so the next start asks neither the user nor the server.
    let mut again = PrintCraftApp::new();
    again.restore(&h.state().persist());
    assert_eq!(again.update_auto, Some(false));
    // Asking by hand still works.
    h.state_mut().execute("help.check_updates");
    let seen = asked.clone();
    until(&mut h, move |_| seen.load(Ordering::SeqCst) > 0);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}
