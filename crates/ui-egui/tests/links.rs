//! Links (CivilOps PDF): the Help menu, About dialog and home screen open civilops.de. Neither
//! ArtCraft marks nor a "based on PrintCraft" link are shown (the licences ship in the program folder).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use printcraft_engine::links;
use printcraft_ui_egui::{Dialog, PrintCraftApp};

fn harness(setup: impl FnOnce(&mut PrintCraftApp) + 'static) -> Harness<'static, PrintCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        setup(&mut app);
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn home_screen_links() {
    let mut h = harness(|_| {});
    h.get_by_label("Plans and documents: measure, change, comment. Part of CivilOps.");
    assert!(h.query_by_label("Based on PrintCraft (open source)").is_none());
    h.get_by_label("CivilOps website").click();
    h.run_steps(2);
    assert_eq!(h.state().last_opened_url.as_deref(), Some("https://civilops.de"));
}

#[test]
fn about_dialog_shows_the_brand_and_links() {
    let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";
    // With a document open, so the home screen's own links are not on screen.
    let mut h = harness(move |app| {
        app.open_bytes("one.pdf", None, pdf.to_vec()).unwrap();
        app.dialog = Some(Dialog::About);
    });
    assert!(h.query_all_by_label("CivilOps PDF").count() >= 1, "the mark (alt text) and the name");
    assert_eq!(h.query_all_by_label("ArtCraft").count(), 0, "no ArtCraft marks in a fork");
    assert!(h.query_all_by_label_contains("PrintCraft").next().is_none(), "no 'based on PrintCraft' line");
    h.get_by_label("CivilOps website").click();
    h.run_steps(2);
    assert_eq!(h.state().last_opened_url.as_deref(), Some(links::WEBSITE));
}

#[test]
fn help_commands_open_each_link() {
    for l in links::LINKS {
        let mut h = harness(|_| {});
        assert!(h.state_mut().execute(l.command), "{}", l.command);
        assert_eq!(h.state().last_opened_url.as_deref(), Some(l.url));
        assert_eq!(printcraft_engine::commands::command(l.command).unwrap().menu, Some("Help"));
    }
}
