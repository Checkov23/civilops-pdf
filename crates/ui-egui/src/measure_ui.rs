//! CivilOps PDF: Measure ▸ Set scale: a plan scale 1:n, or a calibration on a known distance, for
//! the current page or every page. A measurement made on a page without a scale waits here and
//! is added together with the scale (one undo step).

use printcraft_engine::Edit;
use printcraft_engine::measure::{MeasureKind, NewMeasurement, Scale, format_number};

use crate::PrintCraftApp;
use crate::theme::{self, Tokens};

/// The scales offered as buttons (1:n).
const COMMON: [u32; 6] = [100, 200, 250, 500, 1000, 2000];

/// The dialog's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScaleDraft {
    /// The page the dialog is for (0-based).
    pub page: usize,
    /// The n of 1:n as typed ("250").
    pub ratio: String,
    pub all_pages: bool,
    /// Calibration: two points on `page` (user space), and the real length typed (metres).
    pub calibration: Option<([f64; 2], [f64; 2])>,
    pub length: String,
    /// A measurement waiting for the scale.
    pub pending: Option<NewMeasurement>,
    /// Focus the text field on the next frame.
    pub focus: bool,
}

/// What the dialog asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScaleAction {
    None,
    Apply,
    Cancel,
    /// Close, and click two points of a known distance on the page.
    Calibrate,
    /// Take the page's scale off.
    Remove,
}

/// A number as typed, German or English style ("1.234,5", "1234.5", "12,5").
pub fn parse_number(s: &str) -> Option<f64> {
    let t: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
    let t = if t.contains(',') { t.replace('.', "").replace(',', ".") } else { t };
    t.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

impl ScaleDraft {
    /// A draft for `page`, starting from its current scale (or 1:250 for a plan's first scale).
    pub fn for_page(page: usize, current: Option<&Scale>) -> Self {
        let ratio =
            current.and_then(Scale::denominator).map(|d| format_number(d, if (d - d.round()).abs() < 0.05 { 0 } else { 1 })).unwrap_or_default();
        ScaleDraft { page, ratio: ratio.replace('.', ""), focus: true, ..Default::default() }
    }

    /// The length clicked for a calibration, in PDF points.
    fn calibration_length(&self) -> Option<f64> {
        self.calibration.map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1]))
    }

    /// The scale the dialog describes now.
    pub fn scale(&self) -> Option<Scale> {
        match self.calibration_length() {
            Some(len) => Scale::calibrated(len, parse_number(&self.length)?, "m"),
            None => Scale::ratio(parse_number(self.ratio.trim().trim_start_matches("1:").trim_start_matches("1 :"))?),
        }
    }

    /// The edit Apply makes: the scale (with the waiting measurement, as one undo step).
    pub fn edit(&self, page_count: usize) -> Option<Edit> {
        let scale = self.scale()?;
        let pages = if self.all_pages && page_count > 1 { (0..page_count).collect() } else { vec![self.page] };
        let set = Edit::SetPageScale { pages, scale: Some(scale.clone()), update: true };
        Some(match &self.pending {
            Some(m) => {
                let label = if m.kind == MeasureKind::Area { "Measure area" } else { "Measure length" };
                Edit::Batch { label: label.into(), edits: vec![set, Edit::AddMeasurement(NewMeasurement { scale, ..m.clone() })] }
            }
            None => set,
        })
    }
}

/// The dialog body.
pub(crate) fn body(ui: &mut egui::Ui, app: &mut PrintCraftApp, t: &Tokens) -> ScaleAction {
    let Some((_, id)) = app.active_ids() else { return ScaleAction::Cancel };
    let Some(doc) = app.session.get(id) else { return ScaleAction::Cancel };
    let pages = doc.info.pages.len();
    let current = doc.page_scale(app.scale_draft.page);
    let d = &mut app.scale_draft;
    let mut action = ScaleAction::None;
    ui.label(egui::RichText::new("Set scale").font(theme::semibold(18.0)));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(match &current {
            Some(s) => format!("This page: {}", s.ratio_text()),
            None => "This page has no scale yet.".to_string(),
        })
        .color(t.text_muted),
    );
    ui.add_space(10.0);
    let field = |ui: &mut egui::Ui, text: &mut String, id: &str, focus: &mut bool| {
        let r = egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, t.border))
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| ui.add(egui::TextEdit::singleline(text).desired_width(110.0).frame(egui::Frame::NONE).id_salt(id)))
            .inner;
        if *focus {
            r.request_focus();
            *focus = false;
        }
        r
    };
    match d.calibration_length() {
        Some(len) => {
            // Paper length of the clicked distance (true size), to check the clicks.
            let mm = len * 25.4 / 72.0;
            ui.label(format!("Distance clicked on the plan: {} mm on paper", format_number(mm, 1)));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Real length");
                field(ui, &mut d.length, "scale-length", &mut d.focus);
                ui.label("m");
            });
            if ui.add(egui::Button::new("Click the distance again…").frame(false)).clicked() {
                action = ScaleAction::Calibrate;
            }
        }
        None => {
            ui.horizontal(|ui| {
                ui.label("Scale  1 :");
                field(ui, &mut d.ratio, "scale-ratio", &mut d.focus);
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for n in COMMON {
                    let on = parse_number(&d.ratio) == Some(f64::from(n));
                    if ui.selectable_label(on, format!("1:{n}")).clicked() {
                        d.ratio = n.to_string();
                    }
                }
            });
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Or calibrate: click two points whose distance you know, then type the length.").color(t.text_muted));
            if ui.button("Calibrate on the plan…").clicked() {
                action = ScaleAction::Calibrate;
            }
        }
    }
    ui.add_space(8.0);
    let scale = d.scale();
    if let Some(s) = &scale {
        let per_mm = s.per_point * 72.0 / 25.4;
        ui.label(egui::RichText::new(format!("{}: 1 mm on paper = {} m", s.ratio_text(), format_number(per_mm, 3))).color(t.text_muted));
    }
    if pages > 1 {
        ui.checkbox(&mut d.all_pages, "Apply to all pages");
    }
    if d.pending.is_some() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Your measurement is added as soon as the scale is set.").color(t.text_muted));
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if current.is_some() && d.pending.is_none() && ui.button("Remove scale").clicked() {
            action = ScaleAction::Remove;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add_enabled(scale.is_some(), egui::Button::new(egui::RichText::new("Apply").color(egui::Color32::WHITE)).fill(t.accent)).clicked()
                || (enter && scale.is_some())
            {
                action = ScaleAction::Apply;
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = ScaleAction::Cancel;
            }
        });
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_scales_from_the_dialog() {
        assert_eq!(parse_number("250"), Some(250.0));
        assert_eq!(parse_number("1.250,5"), Some(1250.5));
        assert_eq!(parse_number("12,5"), Some(12.5));
        assert_eq!(parse_number(" 10.25 "), Some(10.25));
        assert_eq!(parse_number("0"), None);
        assert_eq!(parse_number("abc"), None);
        let d = ScaleDraft { ratio: "1:500".into(), ..Default::default() };
        assert_eq!(d.scale().unwrap().ratio_text(), "1:500");
        // 72 pt clicked for 6,35 m: 1:250.
        let d = ScaleDraft { calibration: Some(([0.0, 0.0], [72.0, 0.0])), length: "6,35".into(), ..Default::default() };
        assert_eq!(d.scale().unwrap().ratio_text(), "1:250");
        assert_eq!(ScaleDraft::for_page(0, Scale::ratio(1000.0).as_ref()).ratio, "1000");
        // A waiting measurement comes along in one step.
        let m = NewMeasurement {
            page: 2,
            kind: MeasureKind::Area,
            points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
            scale: Scale::ratio(1.0).unwrap(),
            style: printcraft_engine::measure::default_style(),
            author: String::new(),
        };
        let d = ScaleDraft { page: 2, ratio: "250".into(), pending: Some(m), ..Default::default() };
        match d.edit(3) {
            Some(Edit::Batch { edits, .. }) => {
                assert!(matches!(&edits[0], Edit::SetPageScale { pages, .. } if pages == &vec![2]));
                assert!(matches!(&edits[1], Edit::AddMeasurement(m) if m.scale.ratio_text() == "1:250"));
            }
            other => panic!("{other:?}"),
        }
    }
}
