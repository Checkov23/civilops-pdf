//! CivilOps PDF: the tool palette across the top replaces the mode bar and the floating quick bar.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use printcraft_ui_egui::comments::CommentTool;
use printcraft_ui_egui::{LeftPanel, PrintCraftApp, QuickTool};

/// A one-page document with a proper xref table.
fn one_page() -> Vec<u8> {
    let objs =
        ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 595 842] >>", "<< /Type /Page /Parent 2 0 R >>"];
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

fn harness(civilops: bool) -> Harness<'static, PrintCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        // The labels below are English, whatever language the system has.
        app.set_option("language", "en").unwrap();
        app.civilops_layout = civilops;
        if civilops {
            app.left_open = false;
        }
        app.open_bytes("plan.pdf", None, one_page()).expect("fixture opens");
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn the_palette_replaces_mode_bar_and_quick_bar() {
    let h = harness(true);
    for label in ["Select", "Hand", "Text", "Objects", "Length", "Area", "Scale", "Note", "Highlight", "Cloud", "More", "Print", "Save", "All tools"]
    {
        h.get_by_label(label);
    }
    // The menus sit in the tab strip; PrintCraft's Menu button and mode tabs are gone.
    h.get_by_label("File");
    h.get_by_label("Help");
    assert!(h.query_by_label("Menu").is_none());
    assert!(h.query_by_label("E-Sign").is_none());
    assert!(h.query_by_label("Read").is_none());
    // The floating quick bar's buttons are gone too (its tools are on the palette).
    assert!(h.query_by_label("Select (V)").is_none());
    assert!(!h.state().left_open, "the plan fills the window");
}

#[test]
fn palette_tools_switch_without_opening_side_panels() {
    let mut h = harness(true);
    h.get_by_label("Objects").click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::Objects);
    h.get_by_label("Text").click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::EditText);
    assert!(!h.state().left_open, "Text keeps the plan's space in the CivilOps layout");
    h.get_by_label("Length").click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::Comment(CommentTool::MeasureLength));
    h.get_by_label("Area").click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::Comment(CommentTool::MeasureArea));
    h.get_by_label("Cloud").click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::Comment(CommentTool::Cloud));
    h.get_by_label("Select").click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::Select);
}

#[test]
fn more_lists_the_other_tools_and_shows_the_one_in_use() {
    let mut h = harness(true);
    h.get_by_label("More").click();
    h.run_steps(3);
    h.get_by_label(CommentTool::Arrow.label()).click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::Comment(CommentTool::Arrow));
    // The button now names the tool in use.
    h.get_by_label(CommentTool::Arrow.label());
    assert!(h.query_by_label("More").is_none());
}

#[test]
fn all_tools_opens_and_closes_the_side_panel() {
    let mut h = harness(true);
    h.get_by_role_and_label(Role::Button, "All tools").click();
    h.run_steps(3);
    assert!(h.state().left_open);
    assert_eq!(h.state().left, LeftPanel::AllTools);
    // The panel's heading says "All tools" too: click the palette's button.
    h.get_by_role_and_label(Role::Button, "All tools").click();
    h.run_steps(3);
    assert!(!h.state().left_open);
}

#[test]
fn scale_opens_the_scale_dialog() {
    let mut h = harness(true);
    h.get_by_label("Scale").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog, Some(printcraft_ui_egui::Dialog::Scale));
}

#[test]
fn printcraft_layout_is_unchanged_without_the_switch() {
    let h = harness(false);
    h.get_by_label("Menu");
    h.get_by_label("E-Sign");
    h.get_by_label("Select (V)");
    assert!(h.query_by_label("Objects").is_none());
}
