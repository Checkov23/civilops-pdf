//! CivilOps PDF: the tool palette across the top, as in CivilOps Bauabrechnung's plan mode and
//! CivilOps CAD: each tool with its icon above its name, in groups, the selected one filled with
//! the accent. Shown instead of PrintCraft's mode bar and floating quick bar when
//! [`PrintCraftApp::civilops_layout`] is on; the plan fills the window and every other tool stays
//! one click away under "All tools".

use egui::{Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, Stroke, vec2};

use crate::comments::CommentTool;
use crate::theme::{self, Tokens};
use crate::{LeftPanel, PrintCraftApp, QuickTool, icons};

/// The comment tools behind "More", in their groups.
pub const MORE: [&[CommentTool]; 3] = [
    &[CommentTool::TextBox, CommentTool::Callout, CommentTool::Attach],
    &[CommentTool::Underline, CommentTool::StrikeOut, CommentTool::Squiggly, CommentTool::Caret, CommentTool::ReplaceText],
    &[
        CommentTool::Ink,
        CommentTool::Line,
        CommentTool::Arrow,
        CommentTool::Rectangle,
        CommentTool::Oval,
        CommentTool::PolyLine,
        CommentTool::Polygon,
        CommentTool::Eraser,
    ],
];

/// One tool: its icon above its name; `active` fills it with the accent, `menu` adds a chevron.
fn tool(ui: &mut egui::Ui, icon: &str, label: &str, tip: &str, active: bool, menu: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let font = theme::medium(11.5);
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(crate::i18n::shown(label), font.clone(), t.text).size().x);
    let chevron = if menu { 12.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2((text_w + chevron).max(30.0) + 16.0, 50.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label));
    let enabled = ui.is_enabled();
    let fill = if active {
        t.accent
    } else if enabled && resp.is_pointer_button_down_on() {
        t.pressed
    } else if enabled && resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    let (icon_tint, text_tint) = match (active, enabled) {
        (true, _) => (Color32::WHITE, Color32::WHITE),
        (false, true) => (t.icon, t.text),
        (false, false) => (t.text_faint, t.text_faint),
    };
    icons::paint(ui, Rect::from_center_size(rect.center_top() + vec2(0.0, 17.0), vec2(18.0, 18.0)), icon, 18.0, icon_tint);
    let base = rect.center_bottom() - vec2(chevron / 2.0, 11.0);
    ui.painter().text(base, Align2::CENTER_CENTER, label, font, text_tint);
    if menu {
        let at = Rect::from_center_size(base + vec2(text_w / 2.0 + 8.0, 0.0), vec2(10.0, 10.0));
        icons::paint(ui, at, "chevron-down", 10.0, text_tint);
    }
    if tip.is_empty() || !enabled { resp } else { resp.on_hover_text(tip) }
}

/// A thin vertical line between two groups.
fn divider(ui: &mut egui::Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(11.0, 50.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range().shrink(8.0), Stroke::new(1.0, t.divider));
}

/// The palette (below the tab strip).
pub fn show(app: &mut PrintCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("civilops_tool_bar")
        .exact_size(66.0)
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(10, 8)).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let has_doc = app.active.is_some();
                ui.add_enabled_ui(has_doc, |ui| tools(app, ui, &t));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let all_open = app.left_open && app.left == LeftPanel::AllTools;
                    if tool(ui, "layout-grid", "All tools", "Every tool: pages, forms, protect, export and more", all_open, false).clicked() {
                        if all_open {
                            app.left_open = false;
                        } else {
                            app.left = LeftPanel::AllTools;
                            app.left_open = true;
                        }
                    }
                    if tool(ui, "search", "Find", "Find tools and commands (Ctrl+K)", app.palette_open, false).clicked() {
                        app.palette_open = true;
                    }
                });
            });
        });
}

fn tools(app: &mut PrintCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let current = app.quick_tool;
    let comment = |c: CommentTool| current == QuickTool::Comment(c);
    if tool(ui, "mouse-pointer-2", "Select", "Select text and comments (V)", current == QuickTool::Select, false).clicked() {
        app.quick_tool = QuickTool::Select;
    }
    if tool(ui, "hand", "Hand", "Move the page by dragging (H)", current == QuickTool::Hand, false).clicked() {
        app.quick_tool = QuickTool::Hand;
    }
    divider(ui, t);
    if tool(ui, "type", "Text", "Click text in the plan and type to change it", current == QuickTool::EditText, false).clicked() {
        app.execute("edit.edit_text");
    }
    if tool(
        ui,
        "square-dashed-mouse-pointer",
        "Objects",
        "Everything on the page: lines, areas, hatches, images, texts, blocks. Click, double-click for a whole line or hatch, or drag a box; then move, resize, turn, recolour or delete",
        current == QuickTool::Objects,
        false,
    )
    .clicked()
    {
        app.execute("edit.objects");
    }
    divider(ui, t);
    if tool(ui, CommentTool::MeasureLength.icon(), "Length", "Measure a length with the page's scale", comment(CommentTool::MeasureLength), false)
        .clicked()
    {
        app.execute(CommentTool::MeasureLength.command());
    }
    if tool(ui, CommentTool::MeasureArea.icon(), "Area", "Measure an area with the page's scale", comment(CommentTool::MeasureArea), false).clicked()
    {
        app.execute(CommentTool::MeasureArea.command());
    }
    // The page's scale on the button itself (1:250), so it shows at a glance.
    let scale = app
        .active
        .and_then(|i| app.views.get(i))
        .and_then(|v| app.session.get(v.id).and_then(|d| d.page_scale(v.current)))
        .map(|s| s.ratio_text())
        .filter(|t| t.starts_with("1:"));
    let scale_label = scale.as_deref().unwrap_or("Scale");
    if tool(ui, "sliders-horizontal", scale_label, "Set the scale: 1:n or calibrate on the plan", comment(CommentTool::Calibrate), false).clicked() {
        app.execute("plan.scale");
    }
    divider(ui, t);
    for (c, label, tip) in [
        (CommentTool::Note, "Note", "Place a note on the page"),
        (CommentTool::Highlight, "Highlight", "Highlight text or an area"),
        (CommentTool::Cloud, "Cloud", "Draw a revision cloud"),
    ] {
        if tool(ui, c.icon(), label, tip, comment(c), false).clicked() {
            app.execute(c.command());
        }
    }
    more(app, ui, t);
    divider(ui, t);
    if tool(ui, "printer", "Print", "Print (Ctrl+P)", false, false).clicked() {
        app.execute("print.dialog");
    }
    if tool(ui, "save", "Save", "Save (Ctrl+S)", false, false).clicked() {
        app.execute("file.save");
    }
}

/// "More ▾": the other comment and drawing tools; shows the one in use.
fn more(app: &mut PrintCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    let in_use = match app.quick_tool {
        QuickTool::Comment(c) if MORE.iter().any(|g| g.contains(&c)) => Some(c),
        _ => None,
    };
    let (icon, label) = in_use.map_or(("pen-line", "More"), |c| (c.icon(), c.label()));
    let resp = tool(ui, icon, label, "More comment and drawing tools", in_use.is_some(), true);
    egui::Popup::menu(&resp).align(egui::RectAlign::BOTTOM_START).gap(4.0).show(|ui| {
        ui.set_min_width(200.0);
        for (k, group) in MORE.iter().enumerate() {
            if k > 0 {
                ui.separator();
            }
            for c in group.iter() {
                let on = in_use == Some(*c);
                let (row, click) = ui.allocate_exact_size(vec2(200.0, 28.0), Sense::click());
                if click.hovered() {
                    ui.painter().rect_filled(row, CornerRadius::same(4), t.hover);
                }
                icons::paint(ui, Rect::from_min_size(row.min + vec2(8.0, 6.0), vec2(16.0, 16.0)), c.icon(), 16.0, t.text);
                ui.painter().text(row.left_center() + vec2(34.0, 0.0), Align2::LEFT_CENTER, c.label(), theme::regular(13.0), t.text);
                if on {
                    icons::paint(ui, Rect::from_min_size(row.right_top() + vec2(-24.0, 7.0), vec2(14.0, 14.0)), "check", 14.0, t.accent);
                }
                let click = click.on_hover_cursor(egui::CursorIcon::PointingHand);
                click.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, c.label()));
                if click.clicked() {
                    app.execute(c.command());
                    ui.close();
                }
            }
        }
    });
}
