//! CivilOps PDF: measuring tools (scale, length, area), AGENTS.md §3.
//!
//! Geometry follows the automation convention: points from the top-left of the displayed page,
//! y down; values are in real units (metres for a 1:n scale).

use printcraft_engine::Edit;
use printcraft_engine::measure::{self, MeasureKind, Measurement, NewMeasurement, Scale};
use printcraft_render::PageInfo;
use serde_json::{Value, json};

use crate::comments::{DEFAULT_AUTHOR, parse_color};
use crate::{Args, Automation, Result, ToolError, failed};

fn to_user(p: &PageInfo, x: f64, y: f64) -> [f64; 2] {
    let [ux, uy] = p.view_to_user(x as f32, y as f32);
    [ux as f64, uy as f64]
}

fn to_view(p: &PageInfo, u: [f64; 2]) -> [f64; 2] {
    let [x, y] = p.user_to_view(u[0] as f32, u[1] as f32);
    let r = |v: f32| (v as f64 * 100.0).round() / 100.0;
    [r(x), r(y)]
}

fn scale_json(s: &Scale) -> Value {
    json!({ "ratio": s.ratio_text(), "denominator": s.denominator(), "per_point": s.per_point, "unit": s.unit })
}

/// `[[x, y], …]` → user-space points.
fn points(a: &Args, key: &str, info: &PageInfo) -> Result<Vec<[f64; 2]>> {
    let wrong = || ToolError::InvalidArgs(format!("{key} must be an array of [x, y] points"));
    let list = a.get(key).and_then(Value::as_array).ok_or_else(wrong)?;
    if list.len() > 10_000 {
        return Err(ToolError::InvalidArgs(format!("{key}: at most 10000 points")));
    }
    list.iter()
        .map(|p| match p.as_array().map(|p| p.iter().map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
            Some([Some(x), Some(y)]) if x.is_finite() && y.is_finite() => Ok(to_user(info, *x, *y)),
            _ => Err(wrong()),
        })
        .collect()
}

impl Automation {
    /// `measure_scale`: read the scales of pages, or set one (`ratio` 1:n, or `calibrate`).
    pub(crate) fn measure_scale(&mut self, a: &Args) -> Result<Value> {
        let n = self.doc(a)?.info.pages.len();
        let pages = match a.opt_ints("pages")? {
            Some(_) => self.pages(a, "pages")?,
            None => (0..n).collect(),
        };
        let remove = a.opt_bool("remove")?.unwrap_or(false);
        let ratio = a.opt_num("ratio")?;
        let calibrate = a.get("calibrate");
        let chosen = [remove, ratio.is_some(), calibrate.is_some()].iter().filter(|x| **x).count();
        if chosen > 1 {
            return Err(ToolError::InvalidArgs("pass one of ratio, calibrate or remove".into()));
        }
        if chosen == 1 {
            let scale = if remove {
                None
            } else if let Some(r) = ratio {
                Some(Scale::ratio(r).ok_or_else(|| ToolError::InvalidArgs("ratio must be a positive number (250 for 1:250)".into()))?)
            } else {
                // calibrate: { from: [x, y], to: [x, y], length: real length, unit }
                let c = calibrate.and_then(Value::as_object).ok_or_else(|| ToolError::InvalidArgs("calibrate must be an object".into()))?;
                let first = pages.first().copied().unwrap_or(0);
                let info = self.doc(a)?.info.pages[first].clone();
                let pt = |k: &str| -> Result<[f64; 2]> {
                    match c.get(k).and_then(Value::as_array).map(|p| p.iter().map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
                        Some([Some(x), Some(y)]) if x.is_finite() && y.is_finite() => Ok(to_user(&info, *x, *y)),
                        _ => Err(ToolError::InvalidArgs(format!("calibrate.{k} must be [x, y]"))),
                    }
                };
                let (f, t) = (pt("from")?, pt("to")?);
                let real =
                    c.get("length").and_then(Value::as_f64).ok_or_else(|| ToolError::InvalidArgs("calibrate.length must be a number".into()))?;
                let unit = c.get("unit").and_then(Value::as_str).unwrap_or("m");
                let pdf = (t[0] - f[0]).hypot(t[1] - f[1]);
                Some(Scale::calibrated(pdf, real, unit).ok_or_else(|| {
                    ToolError::InvalidArgs("calibrate needs two points at least 1 pt apart and a positive length with a unit".into())
                })?)
            };
            let update = a.opt_bool("update")?.unwrap_or(true);
            self.apply(a, Edit::SetPageScale { pages: pages.clone(), scale, update })?;
        }
        let doc = self.doc(a)?;
        let list: Vec<Value> = pages
            .iter()
            .map(|&p| {
                let vps: Vec<Value> = doc
                    .viewports(p)
                    .iter()
                    .map(|v| json!({ "bbox": v.bbox, "name": v.name, "scale": v.scale.as_ref().map(scale_json), "ours": v.ours }))
                    .collect();
                json!({ "page": p + 1, "scale": doc.page_scale(p).as_ref().map(scale_json), "viewports": vps })
            })
            .collect();
        Ok(json!({ "pages": list }))
    }

    /// `measure_add`: a length or area measurement with the page's scale (or `ratio`).
    pub(crate) fn measure_add(&mut self, a: &Args) -> Result<Value> {
        let page = self.page(a)?;
        let info = self.doc(a)?.info.pages[page].clone();
        let kind = match a.str("kind")? {
            "length" => MeasureKind::Length,
            "area" => MeasureKind::Area,
            other => return Err(ToolError::InvalidArgs(format!("kind must be length or area, not {other:?}"))),
        };
        let pts = points(a, "points", &info)?;
        let first = *pts.first().ok_or_else(|| ToolError::InvalidArgs("points must not be empty".into()))?;
        let scale = match a.opt_num("ratio")? {
            Some(r) => Scale::ratio(r).ok_or_else(|| ToolError::InvalidArgs("ratio must be a positive number".into()))?,
            None => self
                .doc(a)?
                .scale_at(page, first)
                .ok_or_else(|| failed(format!("page {} has no scale: set one with measure_scale (ratio or calibrate), or pass ratio", page + 1)))?,
        };
        let mut style = measure::default_style();
        if let Some(c) = a.opt_str("color")? {
            style.color = parse_color(c)?;
        }
        if let Some(w) = a.opt_num("width")? {
            style.width = w.clamp(0.0, 50.0);
        }
        let author = a.opt_str("author")?.unwrap_or(DEFAULT_AUTHOR).to_string();
        let value = measure::value(kind, &pts, &scale);
        let text = measure::value_text(kind, &pts, &scale);
        let m = NewMeasurement { page, kind, points: pts, scale, style, author };
        let mut out = self.apply(a, Edit::AddMeasurement(m))?;
        out["value"] = json!(value);
        out["text"] = json!(text);
        Ok(out)
    }

    /// `measure_list`: the measurements of a page (or all pages) with their values and totals.
    pub(crate) fn measure_list(&self, a: &Args) -> Result<Value> {
        let doc = self.doc(a)?;
        let n = doc.info.pages.len();
        let pages: Vec<usize> = match a.opt_int("page")? {
            Some(p) if p >= 1 && (p as usize) <= n => vec![p as usize - 1],
            Some(p) => return Err(ToolError::InvalidArgs(format!("page {p} is out of range: the document has {n} pages"))),
            None => (0..n).collect(),
        };
        let (mut total_length, mut total_area) = (0.0, 0.0);
        let mut list = Vec::new();
        for p in pages {
            let info = &doc.info.pages[p];
            for m in doc.measurements(p) {
                let Measurement { index, kind, points, value, text, scale } = m;
                match (kind, value) {
                    (MeasureKind::Length, Some(v)) => total_length += v,
                    (MeasureKind::Area, Some(v)) => total_area += v,
                    _ => {}
                }
                list.push(json!({
                    "page": p + 1,
                    "index": index + 1,
                    "kind": if kind == MeasureKind::Area { "area" } else { "length" },
                    "value": value,
                    "text": text,
                    "scale": scale.as_ref().map(scale_json),
                    "points": points.iter().map(|u| to_view(info, *u)).collect::<Vec<_>>(),
                }));
            }
        }
        Ok(json!({ "count": list.len(), "measurements": list, "total_length": total_length, "total_area": total_area }))
    }
}
