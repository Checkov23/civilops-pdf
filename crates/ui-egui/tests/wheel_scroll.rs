//! CivilOps PDF: the mouse wheel scrolls a plan at once, also while the window doesn't have the
//! focus (16.11.2026: in 0.4.0 a plan dropped onto the window from Explorer only scrolled after a
//! click, because PDFCraft #6031 tied the wheel to the window focus).

use egui_kittest::Harness;
use printcraft_ui_egui::PrintCraftApp;

/// `n` A4 pages with a proper xref table.
fn pages(n: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 595 842] >>", kids.join(" ")),
    ];
    objs.extend((0..n).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_string()));
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

/// The CivilOps layout with a plan opened from a file, as dropping it on the window does.
fn harness(name: &str) -> Harness<'static, PrintCraftApp> {
    let dir = std::env::temp_dir().join(format!("civilops-wheel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, pages(12)).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.civilops_layout = true;
        app.left_open = false;
        app.open_path(&path.to_string_lossy());
        app
    });
    // Let the background open finish and the first frames lay the pages out.
    for _ in 0..200 {
        h.run_steps(1);
        if h.state().views.first().and_then(|v| v.page_screen_rect(0)).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.run_steps(6);
    h
}

/// Page 1's top edge before and after one notch of a Windows mouse wheel (one line down) over
/// the page.
fn one_notch(h: &mut Harness<'static, PrintCraftApp>) -> (f32, f32) {
    let view = &h.state().views[0];
    let page = view.page_screen_rect(0).expect("page 1 on screen");
    let over = page.intersect(view.viewport_rect()).center();
    h.input_mut().events.extend([
        egui::Event::PointerMoved(over),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
        },
    ]);
    h.step();
    h.run_steps(20);
    (page.top(), h.state().views[0].page_screen_rect(0).map_or(f32::NAN, |r| r.top()))
}

#[test]
fn a_wheel_notch_scrolls_the_plan() {
    let mut h = harness("focused.pdf");
    let (before, after) = one_notch(&mut h);
    assert!(after < before - 20.0, "before {before}, after {after}");
}

/// Windows doesn't activate the window a file is dropped on: the focus stays in Explorer while
/// the wheel already reaches the window under the pointer.
#[test]
fn a_wheel_notch_scrolls_the_plan_while_another_window_has_the_focus() {
    let mut h = harness("unfocused.pdf");
    h.input_mut().focused = false;
    h.run_steps(2);
    let (before, after) = one_notch(&mut h);
    assert!(after < before - 20.0, "before {before}, after {after}");
}
