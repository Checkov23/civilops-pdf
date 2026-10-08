//! Measuring (ISO 32000-2 §12.9), a CivilOps PDF addition: drawing scales, measurement comments
//! (lengths along one or more segments, areas) and their values.
//!
//! - A **scale** maps PDF points (default user space) to real units. A page's scale is stored as
//!   a viewport (`/VP`, §12.9.2) over the whole page with a rectilinear measure dictionary
//!   (`/Measure /Subtype /RL`, §12.9.3), so other viewers measure with it too. Viewports a CAD
//!   program wrote are read as well, but only when they name their unit: AutoCAD leaves the unit
//!   blank, and its factor is often paper millimetres, so a blank unit is never taken for metres.
//! - A **measurement** is a Line (`/IT /LineDimension`, two points), PolyLine
//!   (`/IT /PolyLineDimension`) or Polygon (`/IT /PolygonDimension`) annotation with its own
//!   `/Measure`, the value as `/Contents`, and an appearance that shows the value
//!   ([`appearance`], called by [`crate::appearance::build`]).
//!
//! Numbers are written the German way (`1.234,56 m`), as the measure dictionaries say (`/RD`,
//! `/RT`).

use printcraft_cos::{Dict, Document, ObjRef, Object, PdfString, Stream};
use printcraft_fonts::{helvetica_width, literal, win_ansi};

use crate::{AnnotError, Meta, NewAnnotation, Rgb, Shape, Style, add_annotation, annots, n, num_array, page_ref, set_appearance};

/// Metres per PDF point on paper at 1:1 (1 pt = 1/72 in = 0.352 7̅ mm).
pub const METRES_PER_POINT: f64 = 0.0254 / 72.0;

/// The name of the viewport CivilOps PDF writes for a page's scale.
const VIEWPORT_NAME: &str = "CivilOps PDF Massstab";
/// Marks that viewport, and the measurements CivilOps PDF made (private keys other readers ignore).
const PC_SCALE: &[u8] = b"PCScale";
const PC_MEASURE: &[u8] = b"PCMeasure";
/// The page's `/Rotate` when the measurement was made, so its value stays upright on screen.
const PC_ROTATE: &[u8] = b"PCRotate";

/// Caption size in points.
pub const CAPTION_SIZE: f64 = 10.0;

/// A drawing scale: how many real units one PDF point stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    /// Real units per default user space unit (1/72 in on the paper).
    pub per_point: f64,
    /// The unit, e.g. "m".
    pub unit: String,
}

impl Scale {
    /// A plan scale 1:`n` in metres, for a page at its true paper size.
    pub fn ratio(n: f64) -> Option<Scale> {
        (n.is_finite() && n > 0.0 && n <= 10_000_000.0).then(|| Scale { per_point: n * METRES_PER_POINT, unit: "m".into() })
    }

    /// Calibration: two points `pdf_len` points apart are `real` units apart.
    pub fn calibrated(pdf_len: f64, real: f64, unit: &str) -> Option<Scale> {
        let s = Scale { per_point: real / pdf_len, unit: unit.trim().to_string() };
        (pdf_len.is_finite() && pdf_len >= 1.0 && real.is_finite() && real > 0.0 && s.valid()).then_some(s)
    }

    pub fn valid(&self) -> bool {
        self.per_point.is_finite() && self.per_point > 0.0 && self.per_point < 1e7 && !self.unit.trim().is_empty() && self.unit.chars().count() <= 8
    }

    /// The plan scale this is, 1:n, when the unit is metres.
    pub fn denominator(&self) -> Option<f64> {
        (self.unit == "m").then(|| self.per_point / METRES_PER_POINT)
    }

    /// "1:250" (or "1:333,3"), or for other units "1 pt = 0,0882 cm".
    pub fn ratio_text(&self) -> String {
        match self.denominator() {
            Some(d) if (d - d.round()).abs() < 0.05 => format!("1:{}", format_number(d.round(), 0)),
            Some(d) => format!("1:{}", format_number(d, 1)),
            None => format!("1 pt = {} {}", format_number(self.per_point, 4), self.unit),
        }
    }
}

/// `v` with `decimals` places, German style: 1234.5 → "1.234,50".
pub fn format_number(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return "?".into();
    }
    let s = format!("{:.*}", decimals.min(10), v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((s.as_str(), ""));
    let digits = int.len();
    let mut grouped = String::with_capacity(digits + digits / 3);
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (digits - i).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    let negative = v < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0');
    let sign = if negative { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped},{frac}") }
}

/// What a measurement measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasureKind {
    /// The length of a line through the points (two points: a distance).
    Length,
    /// The area of the polygon through the points.
    Area,
}

/// The length of the open line through `points`, in PDF units.
pub fn length(points: &[[f64; 2]]) -> f64 {
    points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum()
}

/// The area enclosed by the polygon through `points` (shoelace formula), in square PDF units.
pub fn area(points: &[[f64; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let twice: f64 = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum();
    twice.abs() / 2.0
}

/// The perimeter of the closed polygon through `points`, in PDF units.
pub fn perimeter(points: &[[f64; 2]]) -> f64 {
    match (points.first(), points.last()) {
        (Some(a), Some(b)) if points.len() >= 3 => length(points) + (a[0] - b[0]).hypot(a[1] - b[1]),
        _ => length(points),
    }
}

/// The measured value in real units (metres, or square metres for areas).
pub fn value(kind: MeasureKind, points: &[[f64; 2]], scale: &Scale) -> f64 {
    match kind {
        MeasureKind::Length => length(points) * scale.per_point,
        MeasureKind::Area => area(points) * scale.per_point * scale.per_point,
    }
}

/// The value as shown: "12,34 m", "1.234,56 m²".
pub fn value_text(kind: MeasureKind, points: &[[f64; 2]], scale: &Scale) -> String {
    let v = value(kind, points, scale);
    match kind {
        MeasureKind::Length => format!("{} {}", format_number(v, 2), scale.unit),
        MeasureKind::Area => format!("{} {}²", format_number(v, 2), scale.unit),
    }
}

/// The rectilinear measure dictionary (§12.9.3, Table 266) for `scale`.
pub fn measure_dict(scale: &Scale) -> Dict {
    let format = |unit: &str, c: f64| -> Object {
        let mut d = Dict::new();
        d.set(b"Type".to_vec(), Object::name("NumberFormat"));
        d.set(b"U".to_vec(), PdfString::text(unit));
        d.set(b"C".to_vec(), Object::Real(c));
        d.set(b"D".to_vec(), Object::Int(100));
        d.set(b"RD".to_vec(), PdfString::text(","));
        d.set(b"RT".to_vec(), PdfString::text("."));
        Object::Array(vec![Object::Dict(d)])
    };
    let mut m = Dict::new();
    m.set(b"Type".to_vec(), Object::name("Measure"));
    m.set(b"Subtype".to_vec(), Object::name("RL"));
    m.set(b"R".to_vec(), PdfString::text(&scale.ratio_text()));
    m.set(b"X".to_vec(), format(&scale.unit, scale.per_point));
    m.set(b"D".to_vec(), format(&scale.unit, 1.0));
    m.set(b"A".to_vec(), format(&format!("{}²", scale.unit), 1.0));
    m
}

/// The scale a measure dictionary describes, when it is rectilinear and names its unit.
pub fn scale_of(doc: &Document, measure: &Object) -> Option<Scale> {
    let m = doc.resolve(measure);
    let m = m.as_dict()?;
    if m.name(b"Subtype").is_some_and(|s| s != b"RL") {
        return None;
    }
    let first = |key: &[u8]| -> Option<Dict> {
        let arr = doc.resolve(m.get(key)?);
        let first = arr.as_array()?.first()?.clone();
        doc.resolve(&first).as_dict().cloned()
    };
    let x = first(b"X")?;
    let cx = x.get(b"C").and_then(Object::as_f64)?;
    // The distance format converts X units to distance units; its label is what we show.
    let (cd, unit) = match first(b"D") {
        Some(d) => (d.get(b"C").and_then(Object::as_f64).unwrap_or(1.0), d.get(b"U").and_then(Object::as_string).map(PdfString::to_text)),
        None => (1.0, None),
    };
    let unit = unit.filter(|u| !u.trim().is_empty()).or_else(|| x.get(b"U").and_then(Object::as_string).map(PdfString::to_text))?;
    let s = Scale { per_point: cx * cd, unit: unit.trim().to_string() };
    s.valid().then_some(s)
}

/// A viewport of a page (§12.9.2): an area with its own scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    pub bbox: [f64; 4],
    pub name: Option<String>,
    /// Its scale, when it names a unit.
    pub scale: Option<Scale>,
    /// Written by CivilOps PDF (the page's scale).
    pub ours: bool,
}

fn rect_of(o: &Object) -> Option<[f64; 4]> {
    let a = o.as_array()?;
    let v: Vec<f64> = a.iter().map(Object::as_f64).collect::<Option<_>>()?;
    match v.as_slice() {
        [a, b, c, d] if v.iter().all(|x| x.is_finite()) => Some([a.min(*c), b.min(*d), a.max(*c), b.max(*d)]),
        _ => None,
    }
}

/// The viewports of page `page` (0-based), in the order the page lists them.
pub fn viewports(doc: &Document, page: usize) -> Result<Vec<Viewport>, AnnotError> {
    let p = page_ref(doc, page)?;
    let obj = doc.get(p);
    let Some(vp) = obj.as_dict().and_then(|d| d.get(b"VP")) else { return Ok(Vec::new()) };
    let list = doc.resolve(vp);
    let Some(list) = list.as_array() else { return Ok(Vec::new()) };
    Ok(list
        .iter()
        .take(10_000)
        .filter_map(|e| {
            let d = doc.resolve(e);
            let d = d.as_dict()?;
            let bbox = d.get(b"BBox").map(|b| doc.resolve(b)).and_then(|b| rect_of(&b))?;
            Some(Viewport {
                bbox,
                name: d.get(b"Name").and_then(Object::as_string).map(PdfString::to_text),
                scale: d.get(b"Measure").and_then(|m| scale_of(doc, m)),
                ours: d.get(PC_SCALE).is_some_and(|v| matches!(v, Object::Bool(true))),
            })
        })
        .collect())
}

/// The scale for measuring at `at` on page `page`: the last viewport listed whose box holds the
/// point and whose measure names its unit (§12.9.2: later viewports are on top).
pub fn scale_at(doc: &Document, page: usize, at: [f64; 2]) -> Option<Scale> {
    let vps = viewports(doc, page).ok()?;
    vps.into_iter()
        .rev()
        .find(|v| v.scale.is_some() && at[0] >= v.bbox[0] - 0.5 && at[0] <= v.bbox[2] + 0.5 && at[1] >= v.bbox[1] - 0.5 && at[1] <= v.bbox[3] + 0.5)
        .and_then(|v| v.scale)
}

/// The scale set for the whole page with CivilOps PDF, if any.
pub fn page_scale(doc: &Document, page: usize) -> Option<Scale> {
    viewports(doc, page).ok()?.into_iter().rev().find(|v| v.ours).and_then(|v| v.scale)
}

/// A page's media box and rotation, inherited from the page tree when the page has none.
fn page_geometry(doc: &Document, page: ObjRef) -> ([f64; 4], i64) {
    let mut media = None;
    let mut rotate = None;
    let mut node = Some(page);
    let mut depth = 0;
    while let Some(r) = node {
        depth += 1;
        if depth > 64 {
            break;
        }
        let obj = doc.get(r);
        let Some(d) = obj.as_dict() else { break };
        if media.is_none() {
            media = d.get(b"MediaBox").map(|b| doc.resolve(b)).and_then(|b| rect_of(&b));
        }
        if rotate.is_none() {
            rotate = d.get(b"Rotate").map(|o| doc.resolve(o)).and_then(|o| o.as_int());
        }
        if media.is_some() && rotate.is_some() {
            break;
        }
        node = d.reference(b"Parent");
    }
    (media.unwrap_or([0.0, 0.0, 612.0, 792.0]), rotate.unwrap_or(0).rem_euclid(360))
}

/// Set (or with `None`, remove) the scale of page `page`: CivilOps PDF's viewport over the whole
/// page, listed last so it applies everywhere on the page. With `update`, the measurements made
/// with CivilOps PDF on the page are recomputed with the new scale. Returns how many were.
pub fn set_page_scale(doc: &mut Document, page: usize, scale: Option<&Scale>, update: bool) -> Result<usize, AnnotError> {
    if let Some(s) = scale
        && !s.valid()
    {
        return Err(AnnotError::Invalid("the scale must be a positive number with a unit".into()));
    }
    let p = page_ref(doc, page)?;
    let (media, _) = page_geometry(doc, p);
    let mut list: Vec<Object> = {
        let obj = doc.get(p);
        let vp = obj.as_dict().and_then(|d| d.get(b"VP")).map(|v| doc.resolve(v));
        vp.and_then(|v| v.as_array().cloned()).unwrap_or_default()
    };
    list.retain(|e| !doc.resolve(e).as_dict().is_some_and(|d| d.get(PC_SCALE).is_some()));
    if let Some(s) = scale {
        let mut v = Dict::new();
        v.set(b"Type".to_vec(), Object::name("Viewport"));
        v.set(b"BBox".to_vec(), num_array(&media));
        v.set(b"Name".to_vec(), PdfString::text(VIEWPORT_NAME));
        v.set(b"Measure".to_vec(), Object::Dict(measure_dict(s)));
        v.set(PC_SCALE.to_vec(), Object::Bool(true));
        list.push(Object::Dict(v));
    }
    doc.update_dict(p, |d| {
        if list.is_empty() {
            d.remove(b"VP");
        } else {
            d.set(b"VP".to_vec(), Object::Array(list));
        }
    })?;
    let mut updated = 0;
    if update && let Some(s) = scale {
        for (r, _) in measurements_on(doc, p).into_iter().filter(|(_, ours)| *ours) {
            if refresh(doc, r, s).is_ok() {
                updated += 1;
            }
        }
    }
    Ok(updated)
}

/// The dimension intents (§12.5.6.7, §12.5.6.9).
fn kind_of(d: &Dict) -> Option<MeasureKind> {
    match (d.name(b"Subtype")?, d.name(b"IT")?) {
        (b"Line", b"LineDimension") | (b"PolyLine", b"PolyLineDimension") => Some(MeasureKind::Length),
        (b"Polygon", b"PolygonDimension") => Some(MeasureKind::Area),
        _ => None,
    }
}

/// Whether an annotation dictionary is a measurement.
pub fn is_measurement(d: &Dict) -> bool {
    kind_of(d).is_some()
}

/// The measurement annotations of a page: (reference, made with CivilOps PDF).
fn measurements_on(doc: &Document, page: ObjRef) -> Vec<(ObjRef, bool)> {
    annots(doc, page)
        .iter()
        .filter_map(|e| {
            let r = e.as_ref()?;
            let obj = doc.get(r);
            let d = obj.as_dict()?;
            is_measurement(d).then(|| (r, d.get(PC_MEASURE).is_some()))
        })
        .collect()
}

/// The points of a measurement annotation, in user space.
fn points_of(d: &Dict) -> Option<Vec<[f64; 2]>> {
    let key: &[u8] = if d.name(b"Subtype") == Some(b"Line") { b"L" } else { b"Vertices" };
    let v: Vec<f64> = d.get(key)?.as_array()?.iter().map(Object::as_f64).collect::<Option<_>>()?;
    let pts: Vec<[f64; 2]> = v.as_chunks::<2>().0.iter().map(|p| [p[0], p[1]]).collect();
    (pts.len() >= 2 && pts.iter().all(|p| p[0].is_finite() && p[1].is_finite())).then_some(pts)
}

/// A measurement on a page: what it measures, its points, value and scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    /// Its index in the page's `/Annots`.
    pub index: usize,
    pub kind: MeasureKind,
    pub points: Vec<[f64; 2]>,
    /// The value in real units (`None` when its scale names no unit).
    pub value: Option<f64>,
    pub text: String,
    pub scale: Option<Scale>,
}

/// The measurements on page `page` (0-based), with their values.
pub fn measurements(doc: &Document, page: usize) -> Result<Vec<Measurement>, AnnotError> {
    let p = page_ref(doc, page)?;
    Ok(annots(doc, p)
        .iter()
        .enumerate()
        .filter_map(|(index, e)| {
            let d = doc.resolve(e);
            let d = d.as_dict()?;
            let kind = kind_of(d)?;
            let points = points_of(d)?;
            let scale = d.get(b"Measure").and_then(|m| scale_of(doc, m));
            let text = d.get(b"Contents").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
            Some(Measurement { index, kind, value: scale.as_ref().map(|s| value(kind, &points, s)), points, text, scale })
        })
        .collect())
}

/// A measurement to add.
#[derive(Clone, Debug, PartialEq)]
pub struct NewMeasurement {
    /// 0-based page index.
    pub page: usize,
    pub kind: MeasureKind,
    /// In user space: at least two points for a length, three for an area.
    pub points: Vec<[f64; 2]>,
    pub scale: Scale,
    pub style: Style,
    pub author: String,
}

/// The default look of measurements: a dark red line, 1.5 pt.
pub fn default_style() -> Style {
    Style { color: [0.80, 0.07, 0.07], opacity: 1.0, width: 1.5, fill: None }
}

/// Add a measurement; returns its index in the page's `/Annots`.
pub fn add_measurement(doc: &mut Document, m: &NewMeasurement, meta: &Meta) -> Result<usize, AnnotError> {
    if !m.scale.valid() {
        return Err(AnnotError::Invalid("the scale must be a positive number with a unit".into()));
    }
    if m.points.len() > 10_000 || !m.points.iter().all(|p| p[0].is_finite() && p[1].is_finite() && p[0].abs() < 1e7 && p[1].abs() < 1e7) {
        return Err(AnnotError::Invalid("invalid points".into()));
    }
    // Repeated clicks on the same spot add nothing.
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(m.points.len());
    for p in &m.points {
        if points.last().is_none_or(|l| (l[0] - p[0]).hypot(l[1] - p[1]) > 1e-6) {
            points.push(*p);
        }
    }
    let shape = match m.kind {
        MeasureKind::Length if points.len() == 2 => Shape::Line { from: points[0], to: points[1], arrow: false },
        MeasureKind::Length if points.len() > 2 => Shape::PolyLine { vertices: points.clone() },
        MeasureKind::Length => return Err(AnnotError::Invalid("a length needs two points".into())),
        MeasureKind::Area if points.len() >= 3 && area(&points) > 0.0 => Shape::Polygon { vertices: points.clone(), cloud: false },
        MeasureKind::Area => return Err(AnnotError::Invalid("an area needs three points that enclose something".into())),
    };
    let text = value_text(m.kind, &points, &m.scale);
    let new = NewAnnotation { page: m.page, shape, style: m.style.clone(), contents: text, author: m.author.clone() };
    let index = add_annotation(doc, &new, meta)?;
    let p = page_ref(doc, m.page)?;
    let r = annots(doc, p).get(index).and_then(Object::as_ref).ok_or(AnnotError::NoSuchAnnotation { page: m.page, index })?;
    let (_, rotate) = page_geometry(doc, p);
    doc.update_dict(r, |d| {
        let (intent, subject) = match m.kind {
            MeasureKind::Length if points.len() == 2 => ("LineDimension", "Length"),
            MeasureKind::Length => ("PolyLineDimension", "Length"),
            MeasureKind::Area => ("PolygonDimension", "Area"),
        };
        d.set(b"IT".to_vec(), Object::name(intent));
        d.set(b"Subj".to_vec(), PdfString::text(subject));
        if points.len() == 2 {
            d.set(b"Cap".to_vec(), Object::Bool(true));
            d.set(b"CP".to_vec(), Object::name("Top"));
            d.set(b"LE".to_vec(), Object::Array(vec![Object::name("Butt"), Object::name("Butt")]));
        }
        d.set(PC_MEASURE.to_vec(), Object::Bool(true));
        if rotate != 0 {
            d.set(PC_ROTATE.to_vec(), Object::Int(rotate));
        }
    })?;
    refresh(doc, r, &m.scale)?;
    Ok(index)
}

/// Recompute a measurement's value with `scale`: its `/Measure`, `/Contents`, `/Rect` (which
/// holds the value's label) and appearance.
fn refresh(doc: &mut Document, r: ObjRef, scale: &Scale) -> Result<(), AnnotError> {
    let d = doc.get(r).as_dict().cloned().ok_or(AnnotError::Invalid("not a measurement".into()))?;
    let kind = kind_of(&d).ok_or(AnnotError::Invalid("not a measurement".into()))?;
    let points = points_of(&d).ok_or(AnnotError::Invalid("the measurement has no points".into()))?;
    let text = value_text(kind, &points, scale);
    let w = d.get(b"BS").and_then(Object::as_dict).and_then(|b| b.get(b"W")).and_then(Object::as_f64).unwrap_or(1.0).max(0.0);
    let rotate = d.int(PC_ROTATE).unwrap_or(0);
    let rect = extent(kind, &points, &text, w, rotate);
    doc.update_dict(r, |d| {
        d.set(b"Measure".to_vec(), Object::Dict(measure_dict(scale)));
        d.set(b"Contents".to_vec(), PdfString::text(&text));
        d.set(b"Rect".to_vec(), num_array(&rect));
    })?;
    set_appearance(doc, r)
}

// ── appearance ──────────────────────────────────────────────────────────────────────────────

/// Where the value label goes: its centre, its direction (unit vector along the text, upright on
/// screen) and its size (width, height).
struct Label {
    centre: [f64; 2],
    dir: [f64; 2],
    width: f64,
    height: f64,
}

impl Label {
    /// The label's corners (for `/Rect`), with a margin for its background.
    fn corners(&self) -> [[f64; 2]; 4] {
        let [ux, uy] = self.dir;
        let (nx, ny) = (-uy, ux);
        let (hw, hh) = (self.width / 2.0 + 2.0, self.height / 2.0 + 1.5);
        let c = self.centre;
        [
            [c[0] - ux * hw - nx * hh, c[1] - uy * hw - ny * hh],
            [c[0] + ux * hw - nx * hh, c[1] + uy * hw - ny * hh],
            [c[0] + ux * hw + nx * hh, c[1] + uy * hw + ny * hh],
            [c[0] - ux * hw + nx * hh, c[1] - uy * hw + ny * hh],
        ]
    }
}

/// The unit vector of the screen's "right" in user space on a page turned by `rotate` degrees.
fn screen_right(rotate: i64) -> [f64; 2] {
    match rotate.rem_euclid(360) {
        90 => [0.0, 1.0],
        180 => [-1.0, 0.0],
        270 => [0.0, -1.0],
        _ => [1.0, 0.0],
    }
}

/// The label of a measurement.
fn label(kind: MeasureKind, points: &[[f64; 2]], text: &str, w: f64, rotate: i64) -> Option<Label> {
    let width = helvetica_width(text, CAPTION_SIZE);
    let height = CAPTION_SIZE;
    match kind {
        MeasureKind::Length => {
            // Along the longest segment, above it as seen on screen.
            let (a, b) = points.windows(2).map(|s| (s[0], s[1])).max_by(|x, y| {
                let l = |s: &([f64; 2], [f64; 2])| (s.1[0] - s.0[0]).hypot(s.1[1] - s.0[1]);
                l(x).total_cmp(&l(y))
            })?;
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            if len <= 0.0 {
                return None;
            }
            let mut dir = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
            // Keep the text upright on screen: it reads left to right in the screen's sense.
            let right = screen_right(rotate);
            if dir[0] * right[0] + dir[1] * right[1] < -1e-9
                || (dir[0] * right[0] + dir[1] * right[1]).abs() <= 1e-9 && (dir[0] * right[1] - dir[1] * right[0]) > 0.0
            {
                dir = [-dir[0], -dir[1]];
            }
            let normal = [-dir[1], dir[0]];
            let off = w / 2.0 + 2.0 + height / 2.0;
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            Some(Label { centre: [mid[0] + normal[0] * off, mid[1] + normal[1] * off], dir, width, height })
        }
        MeasureKind::Area => {
            let c = centroid(points)?;
            Some(Label { centre: c, dir: screen_right(rotate), width, height })
        }
    }
}

/// The area centroid of a polygon (the mean of its points when it has no area).
fn centroid(points: &[[f64; 2]]) -> Option<[f64; 2]> {
    if points.is_empty() {
        return None;
    }
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for (p, q) in points.iter().zip(points.iter().cycle().skip(1)) {
        let cross = p[0] * q[1] - q[0] * p[1];
        a += cross;
        cx += (p[0] + q[0]) * cross;
        cy += (p[1] + q[1]) * cross;
    }
    if a.abs() < 1e-9 {
        let k = points.len() as f64;
        return Some([points.iter().map(|p| p[0]).sum::<f64>() / k, points.iter().map(|p| p[1]).sum::<f64>() / k]);
    }
    Some([cx / (3.0 * a), cy / (3.0 * a)])
}

/// `/Rect` of a measurement: its points, end ticks and label.
fn extent(kind: MeasureKind, points: &[[f64; 2]], text: &str, w: f64, rotate: i64) -> [f64; 4] {
    let tick = tick_length(w);
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut add = |p: [f64; 2]| {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    };
    for p in points {
        add(*p);
    }
    if let Some(l) = label(kind, points, text, w, rotate) {
        for c in l.corners() {
            add(c);
        }
    }
    let pad = w / 2.0 + tick + 1.0;
    [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]
}

fn tick_length(w: f64) -> f64 {
    4.0 + 1.5 * w
}

fn nums(d: &Dict, key: &[u8]) -> Option<Vec<f64>> {
    d.get(key)?.as_array()?.iter().map(Object::as_f64).collect()
}

fn colour(d: &Dict) -> Rgb {
    match nums(d, b"C").as_deref() {
        Some([g]) => [*g; 3],
        Some([r, g, b]) => [*r, *g, *b],
        Some([c, m, y, k]) => [(1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)],
        _ => default_style().color,
    }
    .map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 })
}

/// The normal appearance of a measurement annotation, or `None` if `d` is not one.
pub fn appearance(d: &Dict) -> Option<Stream> {
    let kind = kind_of(d)?;
    let points = points_of(d)?;
    let rect = nums(d, b"Rect").filter(|r| r.len() == 4 && r.iter().all(|v| v.is_finite()))?;
    let rect = [rect[0].min(rect[2]), rect[1].min(rect[3]), rect[0].max(rect[2]), rect[1].max(rect[3])];
    let col = colour(d);
    let w = d.get(b"BS").and_then(Object::as_dict).and_then(|b| b.get(b"W")).and_then(Object::as_f64).unwrap_or(1.0);
    let w = if w.is_finite() { w.clamp(0.0, 100.0) } else { 1.0 };
    let opacity = d.get(b"CA").and_then(Object::as_f64).filter(|o| o.is_finite()).unwrap_or(1.0).clamp(0.0, 1.0);
    let text = d.get(b"Contents").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default();
    let rotate = d.int(PC_ROTATE).unwrap_or(0);
    let rgb = |c: Rgb, op: &str| format!("{} {} {} {op}\n", n(c[0]), n(c[1]), n(c[2]));

    let mut gs = Dict::new();
    let state = |ca: f64, fill: f64| {
        let mut s = Dict::new();
        s.set(b"Type".to_vec(), Object::name("ExtGState"));
        s.set(b"CA".to_vec(), Object::Real(ca));
        s.set(b"ca".to_vec(), Object::Real(fill));
        Object::Dict(s)
    };
    // GS0: the measurement's own opacity; GS1: the area's light fill; GS2: the label background.
    gs.set(b"GS0".to_vec(), state(opacity, opacity));
    gs.set(b"GS1".to_vec(), state(opacity, 0.15 * opacity));
    gs.set(b"GS2".to_vec(), state(opacity, 0.8 * opacity));
    let mut c = String::from("q\n");
    match kind {
        MeasureKind::Area => {
            c.push_str("/GS1 gs\n");
            c.push_str(&rgb(col, "rg"));
            for (i, p) in points.iter().enumerate() {
                c.push_str(&format!("{} {} {}\n", n(p[0]), n(p[1]), if i == 0 { "m" } else { "l" }));
            }
            c.push_str("h f\n/GS0 gs\n");
            c.push_str(&format!("{}{} w 1 J 1 j\n", rgb(col, "RG"), n(w)));
            for (i, p) in points.iter().enumerate() {
                c.push_str(&format!("{} {} {}\n", n(p[0]), n(p[1]), if i == 0 { "m" } else { "l" }));
            }
            c.push_str("h S\n");
        }
        MeasureKind::Length => {
            c.push_str("/GS0 gs\n");
            c.push_str(&format!("{}{} w 0 J 1 j\n", rgb(col, "RG"), n(w)));
            for (i, p) in points.iter().enumerate() {
                c.push_str(&format!("{} {} {}\n", n(p[0]), n(p[1]), if i == 0 { "m" } else { "l" }));
            }
            c.push_str("S\n");
            // Ticks across both ends (`/LE /Butt`).
            let tick = tick_length(w);
            let ends = [(points.first(), points.get(1)), (points.last(), points.len().checked_sub(2).and_then(|i| points.get(i)))];
            for (end, next) in ends {
                let (Some(e), Some(nx)) = (end, next) else { continue };
                let len = (nx[0] - e[0]).hypot(nx[1] - e[1]);
                if len <= 0.0 {
                    continue;
                }
                let (px, py) = (-(nx[1] - e[1]) / len * tick, (nx[0] - e[0]) / len * tick);
                c.push_str(&format!("{} {} m {} {} l S\n", n(e[0] - px), n(e[1] - py), n(e[0] + px), n(e[1] + py)));
            }
        }
    }
    c.push_str("Q\n");
    let mut out = c.into_bytes();
    let mut res = Dict::new();
    if !text.is_empty()
        && let Some(l) = label(kind, &points, &text, w, rotate)
    {
        // A light background, then the value in the measurement's colour.
        let k = l.corners();
        let mut bg = String::from("q\n/GS2 gs\n1 1 1 rg\n");
        for (i, p) in k.iter().enumerate() {
            bg.push_str(&format!("{} {} {}\n", n(p[0]), n(p[1]), if i == 0 { "m" } else { "l" }));
        }
        bg.push_str("h f\nQ\n");
        out.extend(bg.bytes());
        let [ux, uy] = l.dir;
        // The text's start: half its width back along the label, a third of its size down.
        let x = l.centre[0] - ux * l.width / 2.0 + uy * l.height * 0.35;
        let y = l.centre[1] - uy * l.width / 2.0 - ux * l.height * 0.35;
        out.extend(
            format!("q\n/GS0 gs\nBT\n{}/Helv {} Tf\n{} {} {} {} {} {} Tm ", rgb(col, "rg"), n(CAPTION_SIZE), n(ux), n(uy), n(-uy), n(ux), n(x), n(y))
                .bytes(),
        );
        out.extend(literal(&win_ansi(&text)));
        out.extend_from_slice(b" Tj\nET\nQ\n");
        let mut font = Dict::new();
        font.set(b"Type".to_vec(), Object::name("Font"));
        font.set(b"Subtype".to_vec(), Object::name("Type1"));
        font.set(b"BaseFont".to_vec(), Object::name("Helvetica"));
        font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
        let mut fonts = Dict::new();
        fonts.set(b"Helv".to_vec(), Object::Dict(font));
        res.set(b"Font".to_vec(), Object::Dict(fonts));
    }
    res.set(b"ExtGState".to_vec(), Object::Dict(gs));
    let mut fd = Dict::new();
    fd.set(b"Type".to_vec(), Object::name("XObject"));
    fd.set(b"Subtype".to_vec(), Object::name("Form"));
    fd.set(b"FormType".to_vec(), Object::Int(1));
    fd.set(b"BBox".to_vec(), num_array(&rect));
    fd.set(b"Resources".to_vec(), Object::Dict(res));
    Some(Stream::flate(fd, &out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_the_german_way() {
        assert_eq!(format_number(1234.5, 2), "1.234,50");
        assert_eq!(format_number(0.0, 2), "0,00");
        assert_eq!(format_number(-12.345, 2), "-12,35");
        assert_eq!(format_number(-0.001, 2), "0,00");
        assert_eq!(format_number(1_234_567.0, 0), "1.234.567");
        assert_eq!(format_number(999.999, 2), "1.000,00");
        assert_eq!(format_number(f64::NAN, 2), "?");
    }

    #[test]
    fn scales_lengths_and_areas() {
        let s = Scale::ratio(250.0).unwrap();
        assert_eq!(s.ratio_text(), "1:250");
        // 72 pt = 1 in = 25,4 mm on paper = 6,35 m at 1:250.
        assert_eq!(value_text(MeasureKind::Length, &[[0.0, 0.0], [72.0, 0.0]], &s), "6,35 m");
        // A 72 pt square: 6,35² m².
        let sq = [[0.0, 0.0], [72.0, 0.0], [72.0, 72.0], [0.0, 72.0]];
        assert_eq!(value_text(MeasureKind::Area, &sq, &s), "40,32 m²");
        assert!((perimeter(&sq) - 288.0).abs() < 1e-9);
        // Calibrated on a known 10 m: the same scale.
        let c = Scale::calibrated(72.0 / 6.35 * 10.0, 10.0, "m").unwrap();
        assert!((c.per_point - s.per_point).abs() < 1e-12);
        assert_eq!(c.ratio_text(), "1:250");
        assert!(Scale::ratio(0.0).is_none() && Scale::ratio(f64::INFINITY).is_none() && Scale::ratio(-5.0).is_none());
        assert!(
            Scale::calibrated(0.5, 10.0, "m").is_none()
                && Scale::calibrated(100.0, 0.0, "m").is_none()
                && Scale::calibrated(100.0, 1.0, " ").is_none()
        );
        assert_eq!(Scale::calibrated(100.0, 1.0, "cm").unwrap().ratio_text(), "1 pt = 0,0100 cm");
    }

    #[test]
    fn measure_dictionaries_round_trip_and_blank_units_are_not_metres() {
        let doc = Document::new_empty();
        let s = Scale::ratio(500.0).unwrap();
        let back = scale_of(&doc, &Object::Dict(measure_dict(&s))).unwrap();
        assert!((back.per_point - s.per_point).abs() < 1e-15 && back.unit == "m");
        // AutoCAD: X C 0.08819, U ( ): no unit, so no scale.
        let mut nf = Dict::new();
        nf.set(b"C".to_vec(), Object::Real(0.08819));
        nf.set(b"U".to_vec(), PdfString::text(" "));
        let mut m = Dict::new();
        m.set(b"Subtype".to_vec(), Object::name("RL"));
        m.set(b"X".to_vec(), Object::Array(vec![Object::Dict(nf.clone())]));
        m.set(b"D".to_vec(), Object::Array(vec![Object::Dict(nf)]));
        assert_eq!(scale_of(&doc, &Object::Dict(m)), None);
        // Geospatial measures aren't rectilinear scales.
        let mut g = measure_dict(&s);
        g.set(b"Subtype".to_vec(), Object::name("GEO"));
        assert_eq!(scale_of(&doc, &Object::Dict(g)), None);
    }

    #[test]
    fn labels_stay_upright() {
        // A line drawn right to left reads left to right.
        let l = label(MeasureKind::Length, &[[100.0, 0.0], [0.0, 0.0]], "1,00 m", 1.0, 0).unwrap();
        assert!(l.dir[0] > 0.99 && l.centre[1] > 0.0, "above the line");
        // On a page turned 90°, a horizontal line reads along the screen: downwards in user space.
        let l = label(MeasureKind::Length, &[[0.0, 0.0], [0.0, 100.0]], "1,00 m", 1.0, 90).unwrap();
        assert!(l.dir[1] > 0.99);
        let a = label(MeasureKind::Area, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], "x", 1.0, 0).unwrap();
        assert!((a.centre[0] - 5.0).abs() < 1e-9 && (a.centre[1] - 5.0).abs() < 1e-9);
    }
}
