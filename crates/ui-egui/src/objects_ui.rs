//! CivilOps PDF: the Objects tool, as Foxit's Edit Object: everything a page draws, that is lines,
//! areas and hatches, images, gradients, texts and blocks. Hover outlines an object; click selects
//! it (Shift adds or removes); a double-click selects the whole polyline it belongs to (pieces of
//! the same look joined end to end, up to a branch) or, for a loose piece, the hatch it is part of
//! (the pieces of the same look drawn one after another). Dragging over the plan selects with a
//! box, as in CAD: to the right what lies inside, to the left (green) everything it touches; Ctrl+A
//! selects everything on the page. Dragging the selection moves it; its corner and side handles
//! resize it (corners keep the proportions unless Shift is held); the round handle above turns it
//! (Shift: in 15° steps). Delete removes it; arrow keys nudge it; the toolbar above it changes the
//! line (colour, width, style), the fill and the opacity, turns or flips it, explodes blocks, or
//! selects every object drawn the same way. Each change is one undoable engine edit.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, vec2};
use printcraft_engine::{Edit, ObjectChange, PageObject, PlanObjectKind as ObjectKind};
use printcraft_render::DocInfo;

use crate::canvas::{DocView, PageXform};

const SELECT: Color32 = Color32::from_rgb(0x14, 0x73, 0xE6);
/// A crossing box (dragged to the left), as in CAD.
const CROSSING: Color32 = Color32::from_rgb(0x1E, 0x9E, 0x4A);
/// At most this many objects join one polyline or hatch (double-click).
const MAX_CHAIN: usize = 20_000;
/// Ends closer than this (points) join.
const JOIN: f64 = 0.05;
/// Hit tolerance in screen pixels.
const TOLERANCE: f64 = 5.0;
/// At most this many selected objects are outlined (more are shown by their boxes).
const MAX_OUTLINES: usize = 2000;
/// Half a resize handle's size, and how far the turn handle sits above the box (screen pixels).
const GRIP: f32 = 4.5;
const KNOB: f32 = 24.0;
/// A loose piece (a double-click looks for its hatch) has at most this many points.
const PIECE_POINTS: usize = 4;

/// Line and fill colours offered (plan colours).
const COLOURS: [(&str, [f64; 3]); 12] = [
    ("Black", [0.0, 0.0, 0.0]),
    ("Gray", [0.5, 0.5, 0.5]),
    ("White", [1.0, 1.0, 1.0]),
    ("Red", [0.89, 0.0, 0.0]),
    ("Orange", [1.0, 0.55, 0.0]),
    ("Yellow", [1.0, 0.85, 0.0]),
    ("Green", [0.0, 0.6, 0.2]),
    ("Cyan", [0.0, 0.7, 0.8]),
    ("Blue", [0.0, 0.32, 0.86]),
    ("Magenta", [0.8, 0.0, 0.6]),
    ("Brown", [0.55, 0.35, 0.15]),
    ("Navy", [0.11, 0.23, 0.37]),
];

/// Line widths offered, in millimetres (DIN ISO 128).
const WIDTHS_MM: [f64; 5] = [0.18, 0.25, 0.35, 0.5, 0.7];

/// Line styles offered: dash and gap lengths in millimetres (after DIN ISO 128).
const STYLES: [(&str, &[f64]); 4] = [("Solid", &[]), ("Dashed", &[3.0, 1.5]), ("Dash-dot", &[6.0, 1.5, 0.5, 1.5]), ("Dotted", &[0.5, 1.0])];

/// Opacities offered, in percent.
const OPACITIES: [u32; 4] = [100, 75, 50, 25];

/// Per-document state: the objects per page, the selection and a drag in progress.
#[derive(Default)]
pub struct ObjectView {
    cache: HashMap<usize, (u64, Arc<Vec<PageObject>>)>,
    /// The selected objects: (page, indices into the page's objects).
    pub selected: Option<(usize, Vec<usize>)>,
    drag: Option<Drag>,
    /// The toolbar's own colour (its colour picker).
    custom: Option<Color32>,
}

/// A handle on the selection's box.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grip {
    /// A corner (0 top left, then clockwise): resize, keeping the proportions unless Shift.
    Corner(usize),
    /// The middle of a side (0 top, then clockwise): resize in one direction.
    Side(usize),
    /// The round handle above the box: turn.
    Turn,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Move,
    /// A selection box.
    Marquee,
    Grip(Grip),
}

#[derive(Clone, Copy)]
struct Drag {
    page: usize,
    start: Pos2,
    action: Action,
    /// The selection's box on screen when the drag began.
    bounds: Rect,
}

/// An affine map `[a b c d e f]`: x' = a·x + c·y + e, y' = b·x + d·y + f (as PDF's `cm`).
type Affine = [f64; 6];

impl ObjectView {
    /// The objects of `page`, read once per change of the document.
    pub(crate) fn objects(&mut self, doc: &printcraft_engine::Document, page: usize) -> Arc<Vec<PageObject>> {
        let generation = doc.edit_generation();
        match self.cache.get(&page) {
            Some((g, o)) if *g == generation => o.clone(),
            _ => {
                let o = Arc::new(doc.page_objects(page));
                self.cache.insert(page, (generation, o.clone()));
                o
            }
        }
    }

    /// Select every object of `page` (Ctrl+A).
    pub(crate) fn select_page(&mut self, doc: &printcraft_engine::Document, page: usize) {
        let n = self.objects(doc, page).len();
        self.selected = (n > 0).then(|| (page, (0..n).collect()));
    }

    fn selection(&self, page: usize) -> Vec<usize> {
        self.selected.as_ref().filter(|(p, _)| *p == page).map(|(_, s)| s.clone()).unwrap_or_default()
    }
}

fn to_user(xf: &PageXform, info: &DocInfo, page: usize, p: Pos2) -> [f64; 2] {
    let (vx, vy) = xf.screen_to_view(p);
    let u = info.pages[page].view_to_user(vx, vy);
    [u[0] as f64, u[1] as f64]
}

fn to_screen(xf: &PageXform, info: &DocInfo, page: usize, u: [f64; 2]) -> Pos2 {
    let v = info.pages[page].user_to_view(u[0] as f32, u[1] as f32);
    xf.norm_to_screen(v[0] / xf.pw.max(1e-3), v[1] / xf.ph.max(1e-3))
}

/// Screen pixels per point.
fn zoom(xf: &PageXform) -> f64 {
    let long = xf.rect.width().max(xf.rect.height()) as f64;
    let page = xf.pw.max(xf.ph) as f64;
    (long / page.max(1.0)).max(1e-6)
}

fn apply(m: &Affine, p: Pos2) -> Pos2 {
    let (x, y) = (p.x as f64, p.y as f64);
    Pos2::new((m[0] * x + m[2] * y + m[4]) as f32, (m[1] * x + m[3] * y + m[5]) as f32)
}

/// The linear map `m` applied about the point `c`.
fn about(m: Affine, c: Pos2) -> Affine {
    let (cx, cy) = (c.x as f64, c.y as f64);
    [m[0], m[1], m[2], m[3], cx - m[0] * cx - m[2] * cy, cy - m[1] * cx - m[3] * cy]
}

fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// The selected objects' box in user space.
fn user_bounds(objects: &[PageObject], selected: &[usize]) -> Option<[f64; 4]> {
    selected.iter().filter_map(|i| objects.get(*i)).map(|o| o.rect).reduce(union)
}

/// The selected objects' box on screen.
fn screen_bounds(xf: &PageXform, info: &DocInfo, page: usize, objects: &[PageObject], selected: &[usize]) -> Option<Rect> {
    user_bounds(objects, selected).map(|r| xf.user_rect(info, page, r.map(|v| v as f32)))
}

fn box_corners(r: Rect) -> [Pos2; 4] {
    [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
}

fn box_sides(r: Rect) -> [Pos2; 4] {
    [r.center_top(), r.right_center(), r.center_bottom(), r.left_center()]
}

/// The handles of a box. The middle of a side gets one only where the box is deep enough that it
/// doesn't cover the object (a thin strip keeps its corners).
fn grips(r: Rect) -> Vec<(Grip, Pos2)> {
    let room = (GRIP + 3.0) * 4.0;
    let mut v: Vec<(Grip, Pos2)> = box_corners(r).iter().enumerate().map(|(k, p)| (Grip::Corner(k), *p)).collect();
    for (k, p) in box_sides(r).iter().enumerate() {
        let depth = if k % 2 == 0 { r.height() } else { r.width() };
        if depth >= room {
            v.push((Grip::Side(k), *p));
        }
    }
    v.push((Grip::Turn, r.center_top() - vec2(0.0, KNOB)));
    v
}

/// The handle at `p`, if any (the nearest, where they crowd a small box).
fn grip_at(r: Rect, p: Pos2) -> Option<Grip> {
    grips(r).into_iter().map(|(g, q)| (g, (p - q).length())).filter(|(_, d)| *d <= GRIP + 3.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(g, _)| g)
}

fn grip_cursor(g: Grip) -> egui::CursorIcon {
    match g {
        Grip::Corner(0 | 2) => egui::CursorIcon::ResizeNwSe,
        Grip::Corner(_) => egui::CursorIcon::ResizeNeSw,
        Grip::Side(0 | 2) => egui::CursorIcon::ResizeVertical,
        Grip::Side(_) => egui::CursorIcon::ResizeHorizontal,
        Grip::Turn => egui::CursorIcon::Grab,
    }
}

/// What a drag to `p` does on screen.
fn drag_transform(d: &Drag, p: Pos2, shift: bool) -> Affine {
    let delta = p - d.start;
    let r = d.bounds;
    match d.action {
        Action::Move | Action::Marquee => [1.0, 0.0, 0.0, 1.0, delta.x as f64, delta.y as f64],
        Action::Grip(Grip::Turn) => {
            let c = r.center();
            let (a0, a1) = ((d.start.y - c.y).atan2(d.start.x - c.x), (p.y - c.y).atan2(p.x - c.x));
            let mut t = (a1 - a0) as f64;
            if shift {
                let step = 15f64.to_radians();
                t = (t / step).round() * step;
            }
            about([t.cos(), t.sin(), -t.sin(), t.cos(), 0.0, 0.0], c)
        }
        Action::Grip(g) => {
            let (k, corner) = match g {
                Grip::Corner(k) => (k, true),
                Grip::Side(k) => (k, false),
                Grip::Turn => (0, false),
            };
            let points = if corner { box_corners(r) } else { box_sides(r) };
            let (from, anchor) = (points[k % 4], points[(k + 2) % 4]);
            let v0 = from - anchor;
            let v1 = v0 + delta;
            // No turning inside out: at least 2 % of the size.
            let ratio = |a: f32, b: f32| if a.abs() > 0.5 { ((b / a) as f64).max(0.02) } else { 1.0 };
            let (sx, sy) = if corner && !shift {
                let s = if v0.length_sq() > 0.25 { (v1.dot(v0) / v0.length_sq()) as f64 } else { 1.0 };
                (s.max(0.02), s.max(0.02))
            } else if corner {
                (ratio(v0.x, v1.x), ratio(v0.y, v1.y))
            } else if k % 2 == 0 {
                (1.0, ratio(v0.y, v1.y))
            } else {
                (ratio(v0.x, v1.x), 1.0)
            };
            about([sx, 0.0, 0.0, sy, 0.0, 0.0], anchor)
        }
    }
}

/// The page transform that does to the selection what `m` does on screen.
fn page_transform(xf: &PageXform, info: &DocInfo, page: usize, bounds: [f64; 4], m: &Affine) -> Option<[f64; 6]> {
    let centre = [(bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0];
    let l = (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]).max(1.0);
    let p = [centre, [centre[0] + l, centre[1]], [centre[0], centre[1] + l]];
    let q = p.map(|u| to_user(xf, info, page, apply(m, to_screen(xf, info, page, u))));
    printcraft_engine::affine_from_points(p, q)
}

/// A quarter turn or a flip about the selection's centre, made exact (the screen round trip
/// leaves crumbs).
fn exact(m: [f64; 6], bounds: [f64; 4]) -> [f64; 6] {
    let c = [(bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0];
    let r = |v: f64| if (v - v.round()).abs() < 1e-3 { v.round() } else { v };
    let (a, b, cc, d) = (r(m[0]), r(m[1]), r(m[2]), r(m[3]));
    [a, b, cc, d, c[0] - a * c[0] - cc * c[1], c[1] - b * c[0] - d * c[1]]
}

/// What of an object shows, as polylines (user space) and whether each is closed: the area of a
/// clipping path drawn just for it, else its lines cut to the box of its clipping paths.
fn visible(o: &PageObject) -> Vec<(Vec<[f64; 2]>, bool)> {
    if !o.clip_outline.is_empty() {
        return o.clip_outline.iter().map(|s| (s.clone(), true)).collect();
    }
    let Some(c) = o.clip else { return o.subpaths.iter().cloned().zip(o.closed.iter().copied()).collect() };
    let mut out: Vec<(Vec<[f64; 2]>, bool)> = Vec::new();
    for (sp, closed) in o.subpaths.iter().zip(o.closed.iter()) {
        if sp.iter().all(|p| p[0] >= c[0] && p[0] <= c[2] && p[1] >= c[1] && p[1] <= c[3]) {
            out.push((sp.clone(), *closed));
            continue;
        }
        let back = if *closed { sp.last().zip(sp.first()) } else { None };
        let mut run: Vec<[f64; 2]> = Vec::new();
        for (a, b) in sp.windows(2).filter_map(|w| w.first().zip(w.get(1))).chain(back) {
            match cut(*a, *b, c) {
                Some((p, q)) => {
                    if run.last().is_none_or(|l| (l[0] - p[0]).abs() > 1e-9 || (l[1] - p[1]).abs() > 1e-9) {
                        if run.len() > 1 {
                            out.push((std::mem::take(&mut run), false));
                        }
                        run = vec![p];
                    }
                    run.push(q);
                }
                None => {
                    if run.len() > 1 {
                        out.push((std::mem::take(&mut run), false));
                    }
                    run.clear();
                }
            }
        }
        if run.len() > 1 {
            out.push((run, false));
        }
    }
    out
}

/// The part of the segment a–b inside the box `r` (Liang–Barsky), if any.
fn cut(a: [f64; 2], b: [f64; 2], r: [f64; 4]) -> Option<([f64; 2], [f64; 2])> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, a[0] - r[0]), (dx, r[2] - a[0]), (-dy, a[1] - r[1]), (dy, r[3] - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then(|| ([a[0] + t0 * dx, a[1] + t0 * dy], [a[0] + t1 * dx, a[1] + t1 * dy]))
}

fn outline(painter: &egui::Painter, xf: &PageXform, info: &DocInfo, page: usize, o: &PageObject, m: Option<&Affine>, stroke: Stroke) {
    for (sp, closed) in visible(o).iter().map(|(s, c)| (s, c)) {
        let mut pts: Vec<Pos2> = sp
            .iter()
            .map(|u| {
                let s = to_screen(xf, info, page, *u);
                m.map_or(s, |m| apply(m, s))
            })
            .collect();
        if *closed && let Some(f) = pts.first().copied() {
            pts.push(f);
        }
        if pts.len() == 1 {
            painter.circle_stroke(pts[0], 2.0, stroke);
        } else {
            painter.add(egui::Shape::line(pts, stroke));
        }
    }
}

/// Outline the selected objects of `page` with their handles (and, while dragging, where they
/// go).
pub(crate) fn paint_selection(
    painter: &egui::Painter,
    xf: &PageXform,
    page: usize,
    info: &DocInfo,
    objects: &[PageObject],
    view: &DocView,
    pointer: Option<Pos2>,
) {
    let ov = &view.plan_objects;
    let selected = ov.selection(page);
    let shift = painter.ctx().input(|i| i.modifiers.shift);
    let moving = ov.drag.filter(|d| d.page == page && d.action != Action::Marquee).and_then(|d| pointer.map(|p| drag_transform(&d, p, shift)));
    if selected.len() > MAX_OUTLINES {
        for &i in &selected {
            if let Some(o) = objects.get(i) {
                let r = xf.user_rect(info, page, o.rect.map(|v| v as f32));
                match &moving {
                    Some(m) => {
                        let pts: Vec<Pos2> = box_corners(r).iter().map(|p| apply(m, *p)).collect();
                        painter.add(egui::Shape::closed_line(pts, Stroke::new(1.0, SELECT)));
                    }
                    None => {
                        painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, SELECT), egui::StrokeKind::Middle);
                    }
                }
            }
        }
    } else {
        for &i in &selected {
            if let Some(o) = objects.get(i) {
                let faint = if moving.is_some() { 0.35 } else { 0.9 };
                outline(painter, xf, info, page, o, None, Stroke::new(2.5, SELECT.gamma_multiply(faint)));
                if let Some(m) = &moving {
                    outline(painter, xf, info, page, o, Some(m), Stroke::new(1.5, SELECT));
                }
            }
        }
    }
    // The box and its handles.
    if let Some(r) = screen_bounds(xf, info, page, objects, &selected) {
        let thin = Stroke::new(1.0, SELECT);
        match (&moving, ov.drag) {
            (Some(m), Some(d)) => {
                let pts: Vec<Pos2> = box_corners(d.bounds).iter().map(|p| apply(m, *p)).collect();
                painter.add(egui::Shape::closed_line(pts, thin));
                if d.action == Action::Grip(Grip::Turn)
                    && let Some(p) = pointer
                {
                    let deg = (m[1].atan2(m[0]).to_degrees()).round();
                    painter.text(p + vec2(14.0, -14.0), egui::Align2::LEFT_BOTTOM, format!("{deg}°"), egui::FontId::proportional(13.0), SELECT);
                }
            }
            _ if ov.drag.is_none() => {
                painter.rect_stroke(r, CornerRadius::ZERO, thin, egui::StrokeKind::Middle);
                let knob = r.center_top() - vec2(0.0, KNOB);
                painter.line_segment([r.center_top(), knob], thin);
                for (g, p) in grips(r) {
                    if g == Grip::Turn {
                        painter.circle(p, GRIP + 0.5, Color32::WHITE, Stroke::new(1.5, SELECT));
                    } else {
                        painter.rect(
                            Rect::from_center_size(p, vec2(GRIP * 2.0, GRIP * 2.0)),
                            CornerRadius::ZERO,
                            Color32::WHITE,
                            Stroke::new(1.5, SELECT),
                            egui::StrokeKind::Middle,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    if let (Some(d), Some(p)) = (ov.drag.filter(|d| d.page == page && d.action == Action::Marquee), pointer) {
        let r = Rect::from_two_pos(d.start, p);
        let c = if p.x < d.start.x { CROSSING } else { SELECT };
        painter.rect(r, CornerRadius::ZERO, c.gamma_multiply(0.08), Stroke::new(1.0, c), egui::StrokeKind::Middle);
    }
}

/// Objects on a page: hover, select, box-select, move, resize, turn, delete, nudge. Returns
/// `true` when the pointer was used.
pub(crate) fn input(
    ui: &egui::Ui,
    resp: &egui::Response,
    xf: &PageXform,
    page: usize,
    info: &DocInfo,
    objects: &[PageObject],
    view: &mut DocView,
) -> bool {
    let tol = TOLERANCE / zoom(xf);
    let shift_held = ui.input(|i| i.modifiers.shift);
    let selected = view.plan_objects.selection(page);
    // As in CAD, a line within reach wins over an area it runs under or over; else the topmost
    // object there.
    let hit = |p: Pos2| -> Option<usize> {
        let u = to_user(xf, info, page, p);
        let near = |o: &&PageObject| u[0] >= o.rect[0] - tol && u[0] <= o.rect[2] + tol && u[1] >= o.rect[1] - tol && u[1] <= o.rect[3] + tol;
        let mut candidates = objects.iter().enumerate().rev().filter(|(_, o)| near(o));
        let line =
            candidates.clone().find(|(_, o)| matches!(o.kind, ObjectKind::Path { stroked: true, .. }) && o.line_distance(u) <= tol).map(|(i, _)| i);
        line.or_else(|| candidates.find(|(_, o)| o.distance(u) <= tol).map(|(i, _)| i))
    };
    let bounds = screen_bounds(xf, info, page, objects, &selected);

    // A drag in progress: finish it on release.
    if let Some(d) = view.plan_objects.drag.filter(|d| d.page == page) {
        let p = ui.input(|i| i.pointer.interact_pos()).unwrap_or(d.start);
        if resp.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
            view.plan_objects.drag = None;
            let moved = (p - d.start).length() > 1.0;
            match d.action {
                Action::Marquee => {
                    let (a, b) = (to_user(xf, info, page, d.start), to_user(xf, info, page, p));
                    let r = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
                    let crossing = p.x < d.start.x;
                    let mut sel = if shift_held { selected } else { Vec::new() };
                    for (i, o) in objects.iter().enumerate() {
                        let inside = o.rect[0] >= r[0] && o.rect[2] <= r[2] && o.rect[1] >= r[1] && o.rect[3] <= r[3];
                        if (inside || (crossing && touches(o, r))) && !sel.contains(&i) {
                            sel.push(i);
                        }
                    }
                    view.plan_objects.selected = (!sel.is_empty()).then_some((page, sel));
                }
                Action::Move if moved && !selected.is_empty() => {
                    let (a, b) = (to_user(xf, info, page, d.start), to_user(xf, info, page, p));
                    view.pending_edit = Some(Edit::EditObjects { page, objects: selected, change: ObjectChange::Move([b[0] - a[0], b[1] - a[1]]) });
                }
                Action::Grip(_) if moved => {
                    let m = drag_transform(&d, p, shift_held);
                    if let Some(t) = user_bounds(objects, &selected).and_then(|b| page_transform(xf, info, page, b, &m)) {
                        view.pending_edit = Some(Edit::EditObjects { page, objects: selected, change: ObjectChange::Transform(t) });
                    }
                }
                _ => {}
            }
        } else {
            ui.ctx().set_cursor_icon(match d.action {
                Action::Marquee => egui::CursorIcon::Crosshair,
                Action::Move => egui::CursorIcon::Grabbing,
                Action::Grip(Grip::Turn) => egui::CursorIcon::Grabbing,
                Action::Grip(g) => grip_cursor(g),
            });
        }
        return true;
    }

    // Keys act on the selection (Ctrl+A, selecting everything, is read by the app).
    if !ui.ctx().egui_wants_keyboard_input() && !selected.is_empty() {
        if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            view.plan_objects.selected = None;
            view.pending_edit = Some(Edit::EditObjects { page, objects: selected, change: ObjectChange::Delete });
            return true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            view.plan_objects.selected = None;
            return true;
        }
        // Arrow keys nudge by one screen pixel (Shift: ten), in the page as shown.
        let step = if shift_held { 10.0 } else { 1.0 };
        let nudge = ui.input(|i| {
            let mut v = egui::Vec2::ZERO;
            if i.key_pressed(egui::Key::ArrowLeft) {
                v.x -= step;
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                v.x += step;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                v.y -= step;
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                v.y += step;
            }
            v
        });
        if nudge != egui::Vec2::ZERO {
            let c = xf.rect.center();
            let (a, b) = (to_user(xf, info, page, c), to_user(xf, info, page, c + nudge));
            view.pending_edit = Some(Edit::EditObjects { page, objects: selected, change: ObjectChange::Move([b[0] - a[0], b[1] - a[1]]) });
            return true;
        }
    }

    // The selection's handles (the turn handle sits above the box, maybe off the page).
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let on_grip = |p: Option<Pos2>| bounds.zip(p).and_then(|(b, p)| grip_at(b, p));
    if resp.drag_started()
        && let Some(o) = ui.input(|i| i.pointer.press_origin())
        && let (Some(g), Some(b)) = (on_grip(Some(o)), bounds)
    {
        view.plan_objects.drag = Some(Drag { page, start: o, action: Action::Grip(g), bounds: b });
        return true;
    }
    if let Some(g) = on_grip(pointer) {
        ui.ctx().set_cursor_icon(grip_cursor(g));
        // A double-click still selects what lies under the handle (a hatch line, a strip).
        if !resp.double_clicked() {
            return true;
        }
    }

    let Some(p) = pointer.filter(|p| xf.rect.contains(*p)) else { return false };
    let hovered = hit(p);
    if let Some(h) = hovered
        && let Some(o) = objects.get(h)
        && !selected.contains(&h)
    {
        outline(ui.painter(), xf, info, page, o, None, Stroke::new(2.0, SELECT.gamma_multiply(0.55)));
    }
    if resp.drag_started()
        && let Some(o) = ui.input(|i| i.pointer.press_origin()).filter(|o| xf.rect.contains(*o))
    {
        // As in CAD: dragging the selection moves it; a drag anywhere else selects with a box (a
        // dense plan has hardly an empty spot, and an object never moves by accident).
        let action = if hit(o).is_some_and(|h| selected.contains(&h)) { Action::Move } else { Action::Marquee };
        view.plan_objects.drag = Some(Drag { page, start: o, action, bounds: bounds.unwrap_or(Rect::NOTHING) });
        return true;
    }
    // Double-click: the whole polyline, or the hatch of a loose piece (Shift adds it).
    if resp.double_clicked()
        && let Some(h) = hovered
    {
        let mut sel = if shift_held { selected } else { Vec::new() };
        let mut found = chain(objects, h);
        if found.len() <= 1 {
            found = hatch(objects, h);
        }
        for i in found {
            if !sel.contains(&i) {
                sel.push(i);
            }
        }
        view.plan_objects.selected = Some((page, sel));
        return true;
    }
    if resp.clicked() || resp.secondary_clicked() {
        match hovered {
            Some(h) if shift_held => {
                let mut sel = selected;
                match sel.iter().position(|x| *x == h) {
                    Some(k) => {
                        sel.remove(k);
                    }
                    None => sel.push(h),
                }
                view.plan_objects.selected = (!sel.is_empty()).then_some((page, sel));
            }
            Some(h) if resp.secondary_clicked() && selected.contains(&h) => {}
            Some(h) => view.plan_objects.selected = Some((page, vec![h])),
            None => {
                if !shift_held {
                    view.plan_objects.selected = None;
                }
                return false;
            }
        }
        return true;
    }
    match hovered {
        Some(h) => {
            ui.ctx().set_cursor_icon(if selected.contains(&h) { egui::CursorIcon::Move } else { egui::CursorIcon::PointingHand });
            true
        }
        None => false,
    }
}

fn color32(c: [f64; 3]) -> Color32 {
    // Float-to-int `as` saturates: no panic on odd values.
    Color32::from_rgb((c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8)
}

fn swatch(ui: &mut egui::Ui, name: &str, c: [f64; 3]) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::click());
    ui.painter().circle(r.center(), 7.5, color32(c), Stroke::new(1.0, Color32::from_gray(140)));
    if resp.hovered() {
        ui.painter().circle_stroke(r.center(), 9.0, Stroke::new(1.5, SELECT));
    }
    resp.on_hover_text(name).clicked()
}

/// The colours to pick from, and an own colour; returns the one picked.
fn palette(ui: &mut egui::Ui, custom: &mut Option<Color32>) -> Option<[f64; 3]> {
    let mut picked = None;
    for row in COLOURS.chunks(6) {
        ui.horizontal(|ui| {
            for (name, c) in row {
                if swatch(ui, name, *c) {
                    picked = Some(*c);
                }
            }
        });
    }
    ui.collapsing("Own colour", |ui| {
        let mut c = custom.unwrap_or(Color32::BLACK);
        egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque);
        *custom = Some(c);
        if ui.button("Use this colour").clicked() {
            picked = Some([c.r() as f64 / 255.0, c.g() as f64 / 255.0, c.b() as f64 / 255.0]);
        }
    });
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// What one selected object is, for the toolbar.
fn kind_name(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Path { filled: false, .. } => "1 line",
        ObjectKind::Path { .. } => "1 area",
        ObjectKind::Form => "1 block",
        ObjectKind::Image => "1 image",
        ObjectKind::Shading => "1 gradient",
        ObjectKind::Text => "1 text",
    }
}

/// A menu that stays open while its colour picker is used (it closes when something is chosen,
/// or on a click outside).
fn sticky_menu(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    let config = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new(label).config(config).ui(ui, content);
}

/// The floating toolbar above the selection: line (colour, width, style), fill, opacity, turn
/// and flip, explode, delete.
pub(crate) fn toolbar(ctx: &egui::Context, view: &mut DocView, info: &DocInfo) -> Option<Edit> {
    let (page, sel) = view.plan_objects.selected.clone()?;
    if view.plan_objects.drag.is_some() {
        return None;
    }
    let xf = view.page_xform(page)?;
    let objects = view.plan_objects.cache.get(&page).map(|(_, o)| o.clone())?;
    let chosen: Vec<&PageObject> = sel.iter().filter_map(|i| objects.get(*i)).collect();
    if chosen.is_empty() {
        view.plan_objects.selected = None;
        return None;
    }
    let ub = user_bounds(&objects, &sel)?;
    let bounds = xf.user_rect(info, page, ub.map(|v| v as f32));
    // A block keeps the colours it sets itself: it is recoloured after exploding it.
    let lines = chosen.iter().any(|o| matches!(o.kind, ObjectKind::Path { stroked: true, .. }));
    let fills = chosen.iter().any(|o| matches!(o.kind, ObjectKind::Path { filled: true, .. } | ObjectKind::Text));
    let blocks = chosen.iter().any(|o| o.kind == ObjectKind::Form);
    let viewport = view.viewport_rect();
    // Above the box and its turn handle; below it when there is no room above.
    let above = bounds.top() - KNOB - 50.0;
    let y = if above >= viewport.top() + 8.0 { above } else { (bounds.bottom() + 14.0).min(viewport.bottom() - 48.0) };
    let pos =
        Pos2::new(bounds.left().clamp(viewport.left() + 8.0, (viewport.right() - 560.0).max(viewport.left() + 8.0)), y.max(viewport.top() + 8.0));
    let mut change = None;
    let mut select_similar = false;
    let mut turn: Option<Affine> = None;
    let mut custom = view.plan_objects.custom;
    egui::Area::new(egui::Id::new(("object-toolbar", view.id.0))).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(8, 5)).corner_radius(CornerRadius::same(8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let what = match chosen.as_slice() {
                    [one] => kind_name(one.kind).to_string(),
                    _ => format!("{} selected", chosen.len()),
                };
                ui.label(egui::RichText::new(what).strong());
                if ui.button("Select similar").on_hover_text("Every object on this page drawn like the selection").clicked() {
                    select_similar = true;
                }
                ui.separator();
                if lines {
                    sticky_menu(ui, "Line", |ui| {
                        ui.label(egui::RichText::new("Colour").small());
                        if let Some(c) = palette(ui, &mut custom) {
                            change = Some(ObjectChange::Stroke(c));
                        }
                        ui.separator();
                        ui.label(egui::RichText::new("Line width").small());
                        for mm in WIDTHS_MM {
                            let label = format!("{} mm", printcraft_engine::plan_measure::format_number(mm, 2));
                            if ui.button(label).clicked() {
                                change = Some(ObjectChange::Width(mm * 72.0 / 25.4));
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.label(egui::RichText::new("Line style").small());
                        for (name, dashes) in STYLES {
                            if ui.button(name).clicked() {
                                change = Some(ObjectChange::Dash(dashes.iter().map(|mm| mm * 72.0 / 25.4).collect()));
                                ui.close();
                            }
                        }
                    });
                }
                if fills {
                    sticky_menu(ui, "Fill", |ui| {
                        if let Some(c) = palette(ui, &mut custom) {
                            change = Some(ObjectChange::Fill(c));
                        }
                    });
                }
                ui.menu_button("Opacity", |ui| {
                    for pct in OPACITIES {
                        if ui.button(format!("{pct} %")).clicked() {
                            change = Some(ObjectChange::Opacity(pct as f64 / 100.0));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                ui.menu_button("Rotate", |ui| {
                    // On screen (y down): clockwise takes right to down.
                    let options: [(&str, Affine); 5] = [
                        ("Rotate clockwise", [0.0, 1.0, -1.0, 0.0, 0.0, 0.0]),
                        ("Rotate counterclockwise", [0.0, -1.0, 1.0, 0.0, 0.0, 0.0]),
                        ("Rotate 180°", [-1.0, 0.0, 0.0, -1.0, 0.0, 0.0]),
                        ("Flip horizontally", [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
                        ("Flip vertically", [1.0, 0.0, 0.0, -1.0, 0.0, 0.0]),
                    ];
                    for (k, (label, m)) in options.into_iter().enumerate() {
                        if k == 3 {
                            ui.separator();
                        }
                        if ui.button(label).clicked() {
                            turn = Some(about(m, bounds.center()));
                            ui.close();
                        }
                    }
                });
                if blocks && ui.button("Explode").on_hover_text("Split the block into its parts, so each can be changed on its own").clicked() {
                    change = Some(ObjectChange::Explode);
                }
                ui.separator();
                if ui.button("Delete").clicked() {
                    change = Some(ObjectChange::Delete);
                }
            });
        });
    });
    view.plan_objects.custom = custom;
    if select_similar {
        view.plan_objects.selected = Some((page, similar(&objects, &sel)));
        return None;
    }
    if let Some(m) = turn {
        change = page_transform(&xf, info, page, ub, &m).map(|t| ObjectChange::Transform(exact(t, ub)));
    }
    let change = change?;
    if matches!(change, ObjectChange::Delete | ObjectChange::Explode) {
        view.plan_objects.selected = None;
    }
    Some(Edit::EditObjects { page, objects: sel, change })
}

/// How an object is drawn: two objects with the same look belong to the same kind of line (or
/// are the same block or image placed twice).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Look {
    kind: ObjectKind,
    stroke: Option<[i32; 3]>,
    fill: Option<[i32; 3]>,
    /// Line width in hundredths of a point.
    width: i64,
    /// The block or image drawn (a hash of its name).
    resource: Option<u64>,
}

fn look(o: &PageObject) -> Look {
    use std::hash::{Hash, Hasher};
    // Float-to-int `as` saturates (NaN becomes 0): no panic on odd values.
    let q = |c: Option<[f64; 3]>| c.map(|c| c.map(|v| (v * 255.0).round() as i32));
    let resource = o.resource.as_ref().map(|r| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        r.hash(&mut h);
        h.finish()
    });
    Look { kind: o.kind, stroke: q(o.stroke), fill: q(o.fill), width: (o.width * 100.0).round() as i64, resource }
}

/// Every object of the page drawn like one of `selected` ("Select similar").
fn similar(objects: &[PageObject], selected: &[usize]) -> Vec<usize> {
    let looks: HashSet<Look> = selected.iter().filter_map(|i| objects.get(*i)).map(look).collect();
    objects.iter().enumerate().filter(|(_, o)| looks.contains(&look(o))).map(|(i, _)| i).collect()
}

/// The ends of an object's lines (where another piece of the same line can join it).
fn ends(o: &PageObject) -> Vec<[f64; 2]> {
    o.subpaths.iter().flat_map(|sp| [sp.first().copied(), sp.last().copied()]).flatten().collect()
}

/// Whether `next` starts where `prev` ends: one polyline drawn piece by piece.
fn continues(prev: &PageObject, next: &PageObject) -> bool {
    let end = prev.subpaths.last().and_then(|sp| sp.last());
    let start = next.subpaths.first().and_then(|sp| sp.first());
    matches!((end, start), (Some(p), Some(q)) if (p[0] - q[0]).abs() <= JOIN && (p[1] - q[1]).abs() <= JOIN)
}

/// The polyline `start` belongs to (double-click). Plans exported piece by piece draw each edge
/// on its own, but one after another: first the run of pieces drawn in a row with the same look,
/// each starting where the one before ends (a parcel's outline, even where neighbours share its
/// corners).
/// When there is none, the pieces of that look joined end to end, up to a branch (three or more
/// ends at one point).
fn chain(objects: &[PageObject], start: usize) -> Vec<usize> {
    let Some(first) = objects.get(start) else { return Vec::new() };
    let want = look(first);
    let (mut lo, mut hi) = (start, start);
    while hi - lo + 1 < MAX_CHAIN
        && let (Some(prev), Some(cur)) = (lo.checked_sub(1).and_then(|i| objects.get(i)), objects.get(lo))
        && look(prev) == want
        && continues(prev, cur)
    {
        lo -= 1;
    }
    while hi - lo + 1 < MAX_CHAIN
        && let (Some(cur), Some(next)) = (objects.get(hi), objects.get(hi + 1))
        && look(next) == want
        && continues(cur, next)
    {
        hi += 1;
    }
    if hi > lo {
        return (lo..=hi).collect();
    }
    let key = |p: [f64; 2]| ((p[0] / JOIN).round() as i64, (p[1] / JOIN).round() as i64);
    // The objects of that look ending at each point.
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, o) in objects.iter().enumerate().filter(|(_, o)| look(o) == want) {
        for p in ends(o) {
            let list = at.entry(key(p)).or_default();
            if !list.contains(&i) {
                list.push(i);
            }
        }
    }
    let joined = |p: [f64; 2]| -> Vec<usize> {
        let (kx, ky) = key(p);
        let mut v: Vec<usize> = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &i in at.get(&(kx.saturating_add(dx), ky.saturating_add(dy))).into_iter().flatten() {
                    if !v.contains(&i) {
                        v.push(i);
                    }
                }
            }
        }
        v
    };
    let mut seen: HashSet<usize> = HashSet::from([start]);
    let mut order = vec![start];
    let mut todo = vec![start];
    while let Some(i) = todo.pop() {
        let Some(o) = objects.get(i) else { continue };
        for p in ends(o) {
            let here = joined(p);
            // A branch: the polyline stops at it.
            if here.len() > 2 {
                continue;
            }
            for j in here {
                if order.len() < MAX_CHAIN && seen.insert(j) {
                    order.push(j);
                    todo.push(j);
                }
            }
        }
    }
    order
}

/// The hatch a loose piece belongs to (double-click where no other piece joins it): hatches
/// exported line by line, or areas exported strip by strip, are drawn as a run of small pieces of
/// the same look, each close to the ones before. Anything else is just itself.
fn hatch(objects: &[PageObject], start: usize) -> Vec<usize> {
    let Some(first) = objects.get(start) else { return Vec::new() };
    let piece = |o: &PageObject| matches!(o.kind, ObjectKind::Path { .. }) && o.subpaths.iter().map(Vec::len).sum::<usize>() <= PIECE_POINTS;
    if !piece(first) {
        return vec![start];
    }
    let want = look(first);
    let size = |r: [f64; 4]| (r[2] - r[0]).hypot(r[3] - r[1]);
    // Close: within the largest piece's size of what was found so far.
    let near = |r: [f64; 4], area: [f64; 4], reach: f64| {
        r[0] <= area[2] + reach && r[2] >= area[0] - reach && r[1] <= area[3] + reach && r[3] >= area[1] - reach
    };
    let (mut area, mut reach) = (first.rect, size(first.rect).max(1.0));
    let (mut lo, mut hi) = (start, start);
    while hi - lo + 1 < MAX_CHAIN
        && let Some(o) = lo.checked_sub(1).and_then(|i| objects.get(i))
        && look(o) == want
        && piece(o)
        && near(o.rect, area, reach)
    {
        lo -= 1;
        area = union(area, o.rect);
        reach = reach.max(size(o.rect));
    }
    while hi - lo + 1 < MAX_CHAIN
        && let Some(o) = objects.get(hi + 1)
        && look(o) == want
        && piece(o)
        && near(o.rect, area, reach)
    {
        hi += 1;
        area = union(area, o.rect);
        reach = reach.max(size(o.rect));
    }
    (lo..=hi).collect()
}

/// Whether the object's lines touch the box `r` (crossing selection), or a filled shape covers it.
fn touches(o: &PageObject, r: [f64; 4]) -> bool {
    if o.rect[2] < r[0] || o.rect[0] > r[2] || o.rect[3] < r[1] || o.rect[1] > r[3] {
        return false;
    }
    let inside = |p: &[f64; 2]| p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3];
    for (sp, closed) in o.subpaths.iter().zip(o.closed.iter()) {
        if sp.iter().any(inside) {
            return true;
        }
        let back = if *closed { sp.last().zip(sp.first()) } else { None };
        if sp.windows(2).filter_map(|w| w.first().zip(w.get(1))).chain(back).any(|(a, b)| segment_hits(*a, *b, r)) {
            return true;
        }
    }
    o.distance([(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0]) == 0.0
}

/// Whether the segment a–b crosses the box `r` (Liang–Barsky).
fn segment_hits(a: [f64; 2], b: [f64; 2], r: [f64; 4]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, a[0] - r[0]), (dx, r[2] - a[0]), (-dy, a[1] - r[1]), (dy, r[3] - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return false;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return false;
                }
                t1 = t1.min(t);
            }
        }
    }
    t0 <= t1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: [f64; 2], b: [f64; 2], stroke: [f64; 3], width: f64) -> PageObject {
        PageObject::detached(ObjectKind::Path { stroked: true, filled: false }, vec![vec![a, b]], vec![false], Some(stroke), None, width)
    }

    const NAVY: [f64; 3] = [0.11, 0.23, 0.37];
    const RED: [f64; 3] = [0.9, 0.0, 0.0];

    #[test]
    fn a_double_click_follows_the_pieces_of_one_line() {
        // A boundary exported edge by edge: 0–1–2–3, then a branch at 3 (two more edges).
        let objs = vec![
            line([0.0, 0.0], [10.0, 0.0], NAVY, 1.0),
            line([10.0, 0.0], [20.0, 5.0], NAVY, 1.0),
            line([20.0, 5.0], [30.0, 5.0], NAVY, 1.0),
            line([30.0, 5.0], [40.0, 0.0], NAVY, 1.0),
            line([30.0, 5.0], [30.0, 20.0], NAVY, 1.0),
            line([10.0, 0.0], [10.0, -10.0], RED, 1.0),
            line([20.0, 5.0], [25.0, 9.0], NAVY, 2.0),
        ];
        assert_eq!(chain(&objs, 1), [0, 1, 2, 3], "the line as drawn; the branch at (30, 5) is drawn apart");
        assert_eq!(chain(&objs, 4), [4], "the branch alone: (30, 5) is a junction");
        assert!(chain(&objs, 99).is_empty());
    }

    #[test]
    fn a_double_click_takes_a_parcel_as_it_was_drawn() {
        // Two parcels drawn edge by edge, one after the other, sharing the edge (10,0)-(10,10):
        // every shared corner has four pieces, yet each outline comes out whole.
        let p = |a: [f64; 2], b: [f64; 2]| line(a, b, NAVY, 1.0);
        let objs = vec![
            p([0.0, 0.0], [10.0, 0.0]),
            p([10.0, 0.0], [10.0, 10.0]),
            p([10.0, 10.0], [0.0, 10.0]),
            p([0.0, 10.0], [0.0, 0.0]),
            p([10.0, 0.0], [20.0, 0.0]),
            p([20.0, 0.0], [20.0, 10.0]),
            p([20.0, 10.0], [10.0, 10.0]),
            p([10.0, 10.0], [10.0, 0.0]),
        ];
        assert_eq!(chain(&objs, 2), [0, 1, 2, 3]);
        assert_eq!(chain(&objs, 5), [4, 5, 6, 7]);
    }

    #[test]
    fn a_double_click_on_a_hatch_line_takes_the_hatch() {
        // A hatch exported line by line (parallel diagonals 5 pt apart), then a far line of the
        // same look and a polyline.
        let mut objs: Vec<PageObject> = (0..10).map(|k| line([k as f64 * 5.0, 0.0], [k as f64 * 5.0 + 20.0, 20.0], NAVY, 0.5)).collect();
        objs.push(line([500.0, 500.0], [520.0, 520.0], NAVY, 0.5));
        objs.push(PageObject::detached(
            ObjectKind::Path { stroked: true, filled: false },
            vec![vec![[0.0, 30.0], [10.0, 35.0], [20.0, 30.0], [30.0, 35.0], [40.0, 30.0]]],
            vec![false],
            Some(NAVY),
            None,
            0.5,
        ));
        let found = {
            let c = chain(&objs, 4);
            if c.len() <= 1 { hatch(&objs, 4) } else { c }
        };
        assert_eq!(found, (0..10).collect::<Vec<_>>(), "the hatch, not the far line");
        assert_eq!(hatch(&objs, 11), [11], "a polyline is whole by itself");
    }

    #[test]
    fn only_what_shows_is_outlined() {
        // A line from (0, 0) to (100, 0) in a viewport that ends at x = 60.
        let mut o = line([0.0, 0.0], [100.0, 0.0], NAVY, 1.0);
        o.clip = Some([-10.0, -10.0, 60.0, 10.0]);
        assert_eq!(visible(&o), [(vec![[0.0, 0.0], [60.0, 0.0]], false)]);
        // A polyline leaving and entering the box comes out in two pieces.
        let mut z = PageObject::detached(
            ObjectKind::Path { stroked: true, filled: false },
            vec![vec![[0.0, 0.0], [100.0, 0.0], [100.0, 5.0], [0.0, 5.0]]],
            vec![false],
            Some(NAVY),
            None,
            1.0,
        );
        z.clip = Some([-10.0, -10.0, 60.0, 10.0]);
        assert_eq!(visible(&z), [(vec![[0.0, 0.0], [60.0, 0.0]], false), (vec![[60.0, 5.0], [0.0, 5.0]], false)]);
        // A clipping path of its own: its area is the outline.
        o.clip_outline = vec![vec![[0.0, -5.0], [50.0, -5.0], [25.0, 5.0]]];
        assert_eq!(visible(&o), [(vec![[0.0, -5.0], [50.0, -5.0], [25.0, 5.0]], true)]);
    }

    #[test]
    fn a_box_to_the_left_takes_what_it_touches() {
        let diagonal = line([0.0, 0.0], [100.0, 100.0], NAVY, 1.0);
        assert!(touches(&diagonal, [40.0, 40.0, 60.0, 60.0]), "crosses the box");
        assert!(!touches(&diagonal, [60.0, 10.0, 90.0, 30.0]), "only the bounding boxes overlap");
        assert!(touches(&line([0.0, 50.0], [100.0, 50.0], NAVY, 1.0), [40.0, 40.0, 60.0, 60.0]), "passes through");
        assert!(!touches(&line([0.0, 70.0], [100.0, 70.0], NAVY, 1.0), [40.0, 40.0, 60.0, 60.0]));
        let square = PageObject::detached(
            ObjectKind::Path { stroked: false, filled: true },
            vec![vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]]],
            vec![true],
            None,
            Some(NAVY),
            0.0,
        );
        assert!(touches(&square, [40.0, 40.0, 60.0, 60.0]), "a filled shape around the box");
    }

    #[test]
    fn select_similar_takes_every_object_drawn_alike() {
        let objs = vec![
            line([0.0, 0.0], [1.0, 0.0], NAVY, 1.0),
            line([5.0, 5.0], [6.0, 5.0], RED, 1.0),
            line([9.0, 9.0], [9.0, 8.0], NAVY, 1.0),
            line([9.0, 9.0], [9.0, 8.0], NAVY, 1.5),
        ];
        assert_eq!(similar(&objs, &[0]), [0, 2]);
        assert_eq!(similar(&objs, &[0, 1]), [0, 1, 2]);
        // Blocks: the same one placed again, not every block.
        let block = |name: &str, x: f64| {
            let mut b =
                PageObject::detached(ObjectKind::Form, vec![vec![[x, 0.0], [x + 5.0, 0.0], [x + 5.0, 5.0], [x, 5.0]]], vec![true], None, None, 0.0);
            b.resource = Some(name.into());
            b
        };
        let blocks = vec![block("Schacht", 0.0), block("Baum", 10.0), block("Schacht", 20.0)];
        assert_eq!(similar(&blocks, &[0]), [0, 2]);
    }

    #[test]
    fn handles_resize_and_turn() {
        let bounds = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(200.0, 150.0));
        let drag = |action, to: Pos2| Drag { page: 0, start: Pos2::ZERO, action, bounds }.with_start(to);
        // The bottom-right corner dragged 50 px right: the box keeps its proportions about the
        // top-left corner, growing by the drag along its diagonal.
        let d = drag(Action::Grip(Grip::Corner(2)), Pos2::new(200.0, 150.0));
        let m = drag_transform(&d, Pos2::new(250.0, 150.0), false);
        let p = apply(&m, Pos2::new(200.0, 150.0));
        assert!((p.x - 240.0).abs() < 0.01 && (p.y - 170.0).abs() < 0.01, "along the diagonal, 1.4 times: {p:?}");
        assert_eq!(apply(&m, Pos2::new(100.0, 100.0)), Pos2::new(100.0, 100.0), "the opposite corner stays");
        // With Shift, only the way it was dragged.
        let m = drag_transform(&d, Pos2::new(250.0, 150.0), true);
        assert_eq!(apply(&m, Pos2::new(200.0, 150.0)), Pos2::new(250.0, 150.0));
        // The right side: only wider.
        let d = drag(Action::Grip(Grip::Side(1)), Pos2::new(200.0, 125.0));
        let m = drag_transform(&d, Pos2::new(300.0, 140.0), false);
        assert_eq!(apply(&m, Pos2::new(200.0, 150.0)), Pos2::new(300.0, 150.0));
        // Never inside out.
        let m = drag_transform(&d, Pos2::new(-500.0, 140.0), false);
        assert!(m[0] > 0.0);
        // The turn handle, a quarter turn around the centre (150, 125).
        let knob = bounds.center_top() - vec2(0.0, KNOB);
        let d = drag(Action::Grip(Grip::Turn), knob);
        let m = drag_transform(&d, Pos2::new(150.0 + 49.0, 125.0), false);
        let p = apply(&m, Pos2::new(150.0, 100.0));
        assert!((p.x - 175.0).abs() < 0.01 && (p.y - 125.0).abs() < 0.01, "{p:?}");
        // Shift turns in 15° steps.
        let m = drag_transform(&d, Pos2::new(150.0 + 49.0, 125.0 - 10.0), true);
        assert!((m[1].atan2(m[0]).to_degrees() - 75.0).abs() < 1e-6, "{m:?}");
        assert_eq!(grip_at(bounds, Pos2::new(201.0, 151.0)), Some(Grip::Corner(2)));
        assert_eq!(grip_at(bounds, knob + vec2(1.0, 1.0)), Some(Grip::Turn));
        assert_eq!(grip_at(bounds, Pos2::new(150.0, 125.0)), None);
        // A thin strip has no handles in the middle of its long sides: they would cover it.
        let strip = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(300.0, 115.0));
        assert_eq!(grip_at(strip, Pos2::new(200.0, 107.0)), None);
        assert_eq!(grip_at(strip, Pos2::new(300.0, 107.5)), Some(Grip::Side(1)), "its short sides keep theirs");
    }

    #[test]
    fn quarter_turns_come_out_exact() {
        let t = exact([1e-7, 0.9999999, -1.0000001, 2e-8, 3.0, 4.0], [0.0, 0.0, 100.0, 50.0]);
        assert_eq!(t, [0.0, 1.0, -1.0, 0.0, 75.0, -25.0], "turned about the centre (50, 25)");
    }

    impl Drag {
        fn with_start(mut self, start: Pos2) -> Self {
            self.start = start;
            self
        }
    }
}
