//! CivilOps PDF: object tools (list; move, turn, resize, flip, delete, recolour, line width and
//! style, opacity, explode blocks), AGENTS.md §3.
//!
//! Geometry follows the automation convention: points from the top-left of the displayed page.

use printcraft_engine::{Edit, ObjectChange, ObjectKind};
use serde_json::{Value, json};

use crate::comments::parse_color;
use crate::{Args, Automation, Result, ToolError};

fn hex(c: [f64; 3]) -> String {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

fn kind_name(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Form => "form",
        ObjectKind::Image => "image",
        ObjectKind::Shading => "shading",
        ObjectKind::Text => "text",
        ObjectKind::Path { stroked: true, filled: true } => "shape",
        ObjectKind::Path { filled: true, .. } => "fill",
        ObjectKind::Path { .. } => "line",
    }
}

/// The user-space transform that does, about the centre of `bounds` (user space), what the
/// linear map `lin` (`[a b c d]` on the displayed page, y down) does. Quarter turns and flips
/// (`exact`) come out exact.
fn displayed(info: &printcraft_render::PageInfo, bounds: [f64; 4], lin: [f64; 4], exact: bool) -> Option<[f64; 6]> {
    let c = [(bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0];
    let l = (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]).max(1.0);
    let cv = info.user_to_view(c[0] as f32, c[1] as f32);
    let carry = |u: [f64; 2]| {
        let v = info.user_to_view(u[0] as f32, u[1] as f32);
        let (dx, dy) = ((v[0] - cv[0]) as f64, (v[1] - cv[1]) as f64);
        let w = [cv[0] as f64 + lin[0] * dx + lin[2] * dy, cv[1] as f64 + lin[1] * dx + lin[3] * dy];
        let u = info.view_to_user(w[0] as f32, w[1] as f32);
        [u[0] as f64, u[1] as f64]
    };
    let p = [c, [c[0] + l, c[1]], [c[0], c[1] + l]];
    let m = printcraft_engine::affine_from_points(p, p.map(carry))?;
    if !exact {
        return Some(m);
    }
    let r = |v: f64| v.round();
    let (a, b, cc, d) = (r(m[0]), r(m[1]), r(m[2]), r(m[3]));
    Some([a, b, cc, d, c[0] - a * c[0] - cc * c[1], c[1] - b * c[0] - d * c[1]])
}

impl Automation {
    /// `objects_list`: the objects of a page, optionally only those inside `rect`.
    pub(crate) fn objects_list(&self, a: &Args) -> Result<Value> {
        let page = self.page(a)?;
        let doc = self.doc(a)?;
        let info = &doc.info.pages[page];
        let within = a.nums::<4>("rect")?;
        let limit = a.opt_int("limit")?.unwrap_or(200).clamp(1, 100_000) as usize;
        let r = |x: f32| (x as f64 * 100.0).round() / 100.0;
        let mut count = 0usize;
        let mut list = Vec::new();
        for (i, o) in doc.page_objects(page).iter().enumerate() {
            let (u, v) = (info.user_to_view(o.rect[0] as f32, o.rect[1] as f32), info.user_to_view(o.rect[2] as f32, o.rect[3] as f32));
            let rect = [r(u[0].min(v[0])), r(u[1].min(v[1])), r(u[0].max(v[0])), r(u[1].max(v[1]))];
            if let Some(w) = within
                && !(rect[0] >= w[0].min(w[2]) && rect[2] <= w[0].max(w[2]) && rect[1] >= w[1].min(w[3]) && rect[3] <= w[1].max(w[3]))
            {
                continue;
            }
            count += 1;
            if list.len() < limit {
                list.push(json!({
                    "object": i + 1,
                    "kind": kind_name(o.kind),
                    "rect": rect,
                    "stroke": o.stroke.map(hex),
                    "fill": o.fill.map(hex),
                    "width": (o.width * 100.0).round() / 100.0,
                }));
            }
        }
        Ok(json!({ "page": page + 1, "count": count, "objects": list }))
    }

    /// `objects_edit`: change objects (numbers from `objects_list`).
    pub(crate) fn objects_edit(&mut self, a: &Args) -> Result<Value> {
        let page = self.page(a)?;
        let all = self.doc(a)?.page_objects(page);
        let n = all.len();
        let wanted = a.ints("objects")?;
        if wanted.is_empty() {
            return Err(ToolError::InvalidArgs("objects must list at least one object".into()));
        }
        let mut objects = Vec::with_capacity(wanted.len());
        for k in wanted {
            if k < 1 || k as usize > n {
                return Err(ToolError::InvalidArgs(format!("object {k} is out of range: page {} has {n} objects", page + 1)));
            }
            objects.push(k as usize - 1);
        }
        let bounds = objects
            .iter()
            .filter_map(|i| all.get(*i))
            .map(|o| o.rect)
            .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
            .unwrap_or_default();
        let info = self.doc(a)?.info.pages[page].clone();
        let need = |key: &str, why: &str| -> Result<f64> { a.opt_num(key)?.ok_or_else(|| ToolError::InvalidArgs(format!("{why} needs `{key}`"))) };
        let transform = |lin: [f64; 4], exact: bool| -> Result<ObjectChange> {
            displayed(&info, bounds, lin, exact)
                .map(ObjectChange::Transform)
                .ok_or_else(|| ToolError::InvalidArgs("the objects have no extent to transform".into()))
        };
        let change = match a.str("action")? {
            "move" => {
                // Points on the page; up is +dy (as text_edit).
                let dx = a.opt_num("dx")?.unwrap_or(0.0);
                let dy = a.opt_num("dy")?.unwrap_or(0.0);
                let (o, p) = (info.view_to_user(0.0, 0.0), info.view_to_user(dx as f32, -dy as f32));
                ObjectChange::Move([(p[0] - o[0]) as f64, (p[1] - o[1]) as f64])
            }
            "rotate" => {
                // Counter-clockwise as displayed (y down on screen).
                let deg = need("angle", "rotate")?;
                let t = deg.to_radians();
                transform([t.cos(), -t.sin(), t.sin(), t.cos()], deg.rem_euclid(90.0) == 0.0)?
            }
            "scale" => {
                let f = need("factor", "scale")?;
                let fy = a.opt_num("factor_y")?.unwrap_or(f);
                if !(f > 0.0 && fy > 0.0 && f < 1000.0 && fy < 1000.0) {
                    return Err(ToolError::InvalidArgs("factor must be above 0 and below 1000".into()));
                }
                transform([f, 0.0, 0.0, fy], false)?
            }
            "flip" => match a.opt_str("axis")?.unwrap_or("horizontal") {
                "horizontal" => transform([-1.0, 0.0, 0.0, 1.0], true)?,
                "vertical" => transform([1.0, 0.0, 0.0, -1.0], true)?,
                other => return Err(ToolError::InvalidArgs(format!("axis must be horizontal or vertical, not {other:?}"))),
            },
            "delete" => ObjectChange::Delete,
            "stroke" => ObjectChange::Stroke(parse_color(a.str("color")?)?),
            "fill" => ObjectChange::Fill(parse_color(a.str("color")?)?),
            "width" => ObjectChange::Width(need("width", "width")?),
            "dash" => {
                let v = a.get("dashes").ok_or_else(|| ToolError::InvalidArgs("dash needs `dashes` (points; [] for solid lines)".into()))?;
                let list = v.as_array().ok_or_else(|| ToolError::InvalidArgs("dashes must be an array of numbers".into()))?;
                let d: Option<Vec<f64>> = list.iter().map(|x| x.as_f64().filter(|f| f.is_finite())).collect();
                ObjectChange::Dash(d.ok_or_else(|| ToolError::InvalidArgs("dashes must be an array of numbers".into()))?)
            }
            "opacity" => ObjectChange::Opacity(need("opacity", "opacity")?),
            "explode" => ObjectChange::Explode,
            other => return Err(ToolError::InvalidArgs(format!("unknown action {other:?}"))),
        };
        self.apply(a, Edit::EditObjects { page, objects, change })
    }
}
