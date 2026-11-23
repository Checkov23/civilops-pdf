//! CivilOps PDF: a value right-aligned in a header box (a weighing slip, a delivery note) is typed
//! and applied where it stands (23.11.2026). Before, the editor grew to the right while the value
//! was typed, the applied text wrapped or moved, and the Format text window covered the value.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use printcraft_ui_egui::PrintCraftApp;

/// A header box as exporters write it, in Helvetica 7 pt on a 600 x 800 pt page: an address block
/// on the left, labels at x = 300 and values right-aligned to x = 570 (Helvetica advances:
/// "06.10.2022" 35.028 pt, "10000" 19.46 pt, "+49 2632 3180" 46.9 pt).
fn header_box_pdf() -> Vec<u8> {
    let body = concat!(
        "BT /F1 7 Tf 1 0 0 1 70 760 Tm (Firma) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 70 751 Tm (Strasse 1) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 70 742 Tm (12345 Ort) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 70 733 Tm (Land) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 300 700 Tm (Datum) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 534.972 700 Tm (06.10.2022) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 300 680 Tm (Kunde) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 550.54 680 Tm (10000) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 300 660 Tm (Telefon) Tj ET\n",
        "BT /F1 7 Tf 1 0 0 1 523.1 660 Tm (+49 2632 3180) Tj ET\n",
    );
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
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

/// The CivilOps layout (Format text floats over the plan) with Edit text on.
fn harness() -> Harness<'static, PrintCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PrintCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.civilops_layout = true;
        app.left_open = false;
        app.open_bytes("header.pdf", None, header_box_pdf()).expect("fixture opens");
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("edit.edit_text"));
    h.run_steps(2);
    h
}

/// The paragraph `text` on page 1 and its box on screen.
fn paragraph(h: &Harness<'static, PrintCraftApp>, text: &str) -> (printcraft_engine::TextBlock, egui::Rect) {
    let s = h.state();
    let page = s.views[0].page_screen_rect(0).expect("page 1 on screen");
    let k = page.width() / 600.0;
    let blocks = s.session.get(s.views[0].id).expect("document").text_blocks(0);
    let block = blocks
        .iter()
        .find(|b| b.text == text)
        .cloned()
        .unwrap_or_else(|| panic!("no paragraph {text:?} in {:?}", blocks.iter().map(|b| &b.text).collect::<Vec<_>>()));
    let r = block.rect;
    let at = |x: f64, y: f64| egui::pos2(page.left() + x as f32 * k, page.top() + (800.0 - y) as f32 * k);
    (block, egui::Rect::from_min_max(at(r[0], r[3]), at(r[2], r[1])))
}

fn click_at(h: &mut Harness<'static, PrintCraftApp>, at: egui::Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    h.drag_at(at);
    h.run_steps(1);
    h.drop_at(at);
    h.run_steps(4);
}

#[test]
fn a_right_aligned_value_grows_to_the_left_as_typed_and_as_applied() {
    let mut h = harness();
    let (source, value) = paragraph(&h, "06.10.2022");
    click_at(&mut h, value.center());
    assert_eq!(h.state().views[0].line_editor.as_ref().map(|e| e.text.as_str()), Some("06.10.2022"), "the value opens for typing");
    let draft = "06.10.2022 bis 31.12.2023";
    h.state_mut().views[0].line_editor.as_mut().expect("open").text = draft.into();
    h.run_steps(3);
    let editor = h.get_by_label("Text editing line").rect();
    assert!((editor.right() - value.right()).abs() < 3.0, "the editor keeps the value's right edge: {editor:?} for {value:?}");
    assert!(editor.left() < value.left() - 40.0, "and grows to the left: {editor:?} for {value:?}");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert!(h.state().views[0].line_editor.is_none(), "Enter applies a one-line value");
    let (edited, applied) = paragraph(&h, draft);
    assert_eq!(edited.rows.len(), 1, "the value stays on one row");
    assert!((edited.rect[2] - source.rect[2]).abs() < 0.5, "its right edge stays: {:?} -> {:?}", source.rect, edited.rect);
    assert!((applied.left() - editor.left()).abs() < 6.0, "the text lands where it was typed: {applied:?}, typed in {editor:?}");
}

#[test]
fn the_format_text_window_leaves_a_value_at_the_top_right_uncovered() {
    let mut h = harness();
    let (_, value) = paragraph(&h, "06.10.2022");
    click_at(&mut h, value.center());
    assert!(h.state().views[0].line_editor.is_some(), "the value opens for typing");
    h.run_steps(2);
    let window = h.ctx.memory(|m| m.area_rect(egui::Id::unique("civilops-format-text"))).expect("the Format text window shows");
    assert!(!window.intersects(value.expand(4.0)), "the Format text window {window:?} covers the value {value:?}");
}
