//! CivilOps PDF: Edit a PDF ▸ edit existing objects, as Foxit's Edit Object does: everything a
//! page draws in its own content (vector paths: lines, curves, areas and hatches; images; shadings,
//! that is gradients; text objects; form XObjects: blocks and symbols), and changing it: move,
//! resize, rotate or flip (a transform), delete, recolour, line width and line style, opacity, and
//! exploding a block into its parts.
//!
//! Like [`crate::images`]: the operators that draw an object are wrapped in `q … Q` with the
//! change (`cm` for a transform; colour, `w`, `d` or `gs` operators), or dropped; only the streams
//! that draw the chosen objects are rewritten, as new objects. A page's content streams are read
//! as one stream (§7.8.2), so a `cm` or an open `q` in one stream still applies in the next.
//!
//! What an object sets for the content after it (a text object's font and colour) is set again
//! after its `Q`, and kept when it is deleted. A clipping path drawn just for one object
//! (`q … W n <object> Q`, as clipped hatches and gradients are drawn) belongs to it and moves with
//! it; other clipping paths aren't objects: changing one would change everything after it.

use std::collections::HashMap;

use printcraft_content::{Matrix, Op, parse, serialize_ops};
use printcraft_cos::{Dict, Document, Name, Object, Stream};

use crate::EditError;

/// At most this many objects are listed per page (pathological files stay responsive).
const MAX_OBJECTS: usize = 500_000;
/// At most this many points per object are kept for hit testing.
const MAX_POINTS: usize = 20_000;
/// Segments a curve is flattened into for hit testing.
const CURVE_STEPS: usize = 8;
/// At most this many dash lengths (`d`).
const MAX_DASHES: usize = 16;

/// What kind of object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    /// A path, stroked and/or filled.
    Path { stroked: bool, filled: bool },
    /// A form XObject drawn with `Do` (a block or symbol).
    Form,
    /// An image: an image XObject drawn with `Do`, or an inline image.
    Image,
    /// A shading drawn with `sh` (a gradient), as far as its clipping path reaches.
    Shading,
    /// A text object (`BT … ET`).
    Text,
}

impl ObjectKind {
    /// Whether it covers its area (a click inside hits it), rather than being only lines.
    pub fn covers(self) -> bool {
        !matches!(self, ObjectKind::Path { filled: false, .. })
    }
}

/// An object drawn on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct PageObject {
    pub kind: ObjectKind,
    /// Its box in user space.
    pub rect: [f64; 4],
    /// Its outline in user space, as polylines (curves flattened), for hit testing and
    /// highlighting. A form's, image's, shading's or text's is its box.
    pub subpaths: Vec<Vec<[f64; 2]>>,
    /// Which subpaths are closed.
    pub closed: Vec<bool>,
    /// The stroke and fill colours when they are device colours (gray, RGB, CMYK as RGB).
    pub stroke: Option<[f64; 3]>,
    pub fill: Option<[f64; 3]>,
    /// The line width in user space.
    pub width: f64,
    /// The box of the clipping paths it is drawn in, in user space: what lies outside is hidden
    /// (a viewport's edge, a hatch's area).
    pub clip: Option<[f64; 4]>,
    /// The outline of a clipping path drawn just for it (a clipped hatch's area), else empty.
    pub clip_outline: Vec<Vec<[f64; 2]>>,
    /// The XObject a block or an image draws (its resource name): the same block placed twice
    /// draws the same one.
    pub resource: Option<String>,
    stream: usize,
    first: usize,
    last: usize,
    /// The CTM where it is painted, and where its operators start (they differ when a clipping
    /// path drawn for it comes first, with a `cm` in between).
    ctm: [f64; 6],
    start_ctm: [f64; 6],
}

impl PageObject {
    /// An object that is on no page (tests, previews): its geometry and look only. Its box is
    /// the extent of its points.
    pub fn detached(
        kind: ObjectKind,
        subpaths: Vec<Vec<[f64; 2]>>,
        closed: Vec<bool>,
        stroke: Option<[f64; 3]>,
        fill: Option<[f64; 3]>,
        width: f64,
    ) -> Self {
        let rect = bbox(&subpaths).filter(|r| r.iter().all(|v| v.is_finite())).unwrap_or([0.0; 4]);
        let id = Matrix::IDENTITY.0;
        PageObject {
            kind,
            rect,
            subpaths,
            closed,
            stroke,
            fill,
            width,
            clip: None,
            clip_outline: Vec::new(),
            resource: None,
            stream: 0,
            first: 0,
            last: 0,
            ctm: id,
            start_ctm: id,
        }
    }

    /// Whether `p` lies where the object can show (inside its clipping paths).
    pub fn shows_at(&self, p: [f64; 2]) -> bool {
        let in_box = self.clip.is_none_or(|c| p[0] >= c[0] && p[0] <= c[2] && p[1] >= c[1] && p[1] <= c[3]);
        in_box && (self.clip_outline.is_empty() || self.clip_outline.iter().any(|s| contains(s, p)))
    }

    /// The end points of its segments (snapping targets for measuring): the corners of lines and
    /// shapes, and of blocks, where they show; not of images, gradients or text.
    pub fn vertices(&self) -> impl Iterator<Item = [f64; 2]> + '_ {
        let snaps = matches!(self.kind, ObjectKind::Path { .. } | ObjectKind::Form);
        self.subpaths.iter().filter(move |_| snaps).flatten().copied().filter(|p| self.shows_at(*p))
    }

    /// The distance from `p` to the object (0 inside an area, an image, a block or a text), in
    /// user space; where its clipping paths hide it, it is out of reach.
    pub fn distance(&self, p: [f64; 2]) -> f64 {
        if !self.shows_at(p) {
            return f64::MAX;
        }
        if self.kind.covers() && self.subpaths.iter().any(|s| contains(s, p)) {
            return 0.0;
        }
        self.outline_distance(p)
    }

    /// The distance from `p` to the object's lines (its outline, inside or not), where they show.
    pub fn line_distance(&self, p: [f64; 2]) -> f64 {
        if !self.shows_at(p) {
            return f64::MAX;
        }
        self.outline_distance(p)
    }

    fn outline_distance(&self, p: [f64; 2]) -> f64 {
        let mut best = f64::MAX;
        for (sp, closed) in self.subpaths.iter().zip(self.closed.iter().chain(std::iter::repeat(&false))) {
            for w in sp.windows(2) {
                best = best.min(segment_distance(p, w[0], w[1]));
            }
            if *closed && let (Some(a), Some(b)) = (sp.last(), sp.first()) {
                best = best.min(segment_distance(p, *a, *b));
            }
            if sp.len() == 1 {
                best = best.min((p[0] - sp[0][0]).hypot(p[1] - sp[0][1]));
            }
        }
        best
    }
}

fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - (a[0] + t * dx)).hypot(p[1] - (a[1] + t * dy))
}

/// Even-odd point in polygon.
fn contains(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// The graphics state objects care about.
#[derive(Clone)]
struct Gs {
    ctm: Matrix,
    width: f64,
    stroke: Option<[f64; 3]>,
    fill: Option<[f64; 3]>,
    /// Colour spaces set with `CS`/`cs`: device spaces give a colour, others don't.
    stroke_space: Space,
    fill_space: Space,
    /// The box of the clipping paths in effect, in user space.
    clip: Option<[f64; 4]>,
}

#[derive(Clone, Copy, PartialEq)]
enum Space {
    Gray,
    Rgb,
    Cmyk,
    Other,
}

fn space_of(name: &[u8]) -> Space {
    match name {
        b"DeviceGray" | b"G" => Space::Gray,
        b"DeviceRGB" | b"RGB" => Space::Rgb,
        b"DeviceCMYK" | b"CMYK" => Space::Cmyk,
        _ => Space::Other,
    }
}

fn colour(space: Space, v: &[f64]) -> Option<[f64; 3]> {
    let c = |x: f64| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
    match (space, v) {
        (Space::Gray, [g]) => Some([c(*g); 3]),
        (Space::Rgb, [r, g, b]) => Some([c(*r), c(*g), c(*b)]),
        (Space::Cmyk, [cy, m, y, k]) => Some([(1.0 - c(*cy)) * (1.0 - c(*k)), (1.0 - c(*m)) * (1.0 - c(*k)), (1.0 - c(*y)) * (1.0 - c(*k))]),
        _ => None,
    }
}

fn nums(op: &Op) -> Vec<f64> {
    op.operands.iter().filter_map(Object::as_f64).collect()
}

/// A path being built: its subpaths (user space), where it started and whether it clips.
struct Building {
    first: usize,
    subpaths: Vec<Vec<[f64; 2]>>,
    closed: Vec<bool>,
    current: Option<[f64; 2]>,
    clips: bool,
    points: usize,
}

impl Building {
    fn new(first: usize) -> Self {
        Building { first, subpaths: Vec::new(), closed: Vec::new(), current: None, clips: false, points: 0 }
    }

    fn push(&mut self, p: [f64; 2]) {
        if self.points >= MAX_POINTS {
            return;
        }
        self.points += 1;
        match self.subpaths.last_mut() {
            Some(s) => s.push(p),
            None => {
                self.subpaths.push(vec![p]);
                self.closed.push(false);
            }
        }
    }

    fn start(&mut self, p: [f64; 2]) {
        self.subpaths.push(Vec::new());
        self.closed.push(false);
        self.push(p);
    }
}

fn user(ctm: &Matrix, x: f64, y: f64) -> [f64; 2] {
    let (u, v) = ctm.apply(x, y);
    [u, v]
}

fn bbox(subpaths: &[Vec<[f64; 2]>]) -> Option<[f64; 4]> {
    let mut it = subpaths.iter().flatten();
    let first = it.next()?;
    let mut b = [first[0], first[1], first[0], first[1]];
    for p in it {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    Some(b)
}

fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// The overlap of two boxes (empty boxes have no area but stay where they are).
fn intersect(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    [r[0], r[1], r[2].max(r[0]), r[3].max(r[1])]
}

/// A box's corners, as a closed outline.
fn corners(r: [f64; 4]) -> Vec<Vec<[f64; 2]>> {
    vec![vec![[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]]
}

fn streams(doc: &Document, page: &Dict) -> Vec<(Object, Vec<u8>)> {
    let list: Vec<Object> = match page.get(b"Contents") {
        None => Vec::new(),
        Some(c) => match &*doc.resolve(c) {
            Object::Array(a) => a.clone(),
            _ => vec![c.clone()],
        },
    };
    list.into_iter()
        .map(|o| {
            let data = match &*doc.resolve(&o) {
                Object::Stream(s) => s.decoded().unwrap_or_default(),
                _ => Vec::new(),
            };
            (o, data)
        })
        .collect()
}

fn page_of(doc: &Document, page: usize) -> Result<printcraft_model::Page, EditError> {
    printcraft_model::pages(doc).into_iter().nth(page).ok_or(EditError::NoSuchPage(page))
}

/// A copy of a dictionary entry of `dict` that is (or refers to) a dictionary.
fn sub_dict(doc: &Document, dict: &Dict, key: &[u8]) -> Dict {
    dict.get(key).map(|x| doc.resolve(x)).and_then(|x| x.as_dict().cloned()).unwrap_or_default()
}

fn xobjects(doc: &Document, page: &Dict) -> Dict {
    sub_dict(doc, &sub_dict(doc, page, b"Resources"), b"XObject")
}

/// A clipping path just drawn: its first operator, the depth, the CTM there and its outline.
type LastClip = (usize, usize, Matrix, Vec<Vec<[f64; 2]>>);

/// Where an object's operators start: at a clipping path drawn just for it (right before it, at
/// the same depth, with the depth closed right after the object), else at its own first operator.
/// With the outline of that clipping path, if it is the object's own.
fn own_clip(clip: Option<LastClip>, depth: usize, ops: &[Op], first: usize, last: usize, ctm: Matrix) -> (usize, Matrix, Vec<Vec<[f64; 2]>>) {
    match clip {
        Some((f, d, m, outline)) if d == depth && f < first && ops.get(last + 1).is_some_and(|o| o.op == b"Q") => (f, m, outline),
        _ => (first, ctm, Vec::new()),
    }
}

/// What of `o` shows: its box cut to its clipping paths.
fn clipped(mut o: PageObject, clip: Option<[f64; 4]>, outline: Vec<Vec<[f64; 2]>>) -> PageObject {
    if let Some(c) = clip {
        o.rect = intersect(o.rect, c);
    }
    if let Some(b) = bbox(&outline) {
        o.rect = intersect(o.rect, b);
    }
    o.clip = clip;
    o.clip_outline = outline;
    o
}

/// The objects page `page` (0-based) draws in its own content, in drawing order.
pub fn page_objects(doc: &Document, page: usize) -> Result<Vec<PageObject>, EditError> {
    let p = page_of(doc, page)?;
    let xo = xobjects(doc, &p.dict);
    let crop = p.crop(doc);
    // Text objects: the boxes of their lines, by content stream and `BT` position. Text lines
    // count the operators of the page's streams joined (`Pieces`); the walk below counts each
    // stream's own, so each joined operator is mapped to its stream and its index there.
    let pieces = printcraft_content::Pieces::join(&streams(doc, &p.dict).into_iter().map(|(_, d)| d).collect::<Vec<_>>());
    let mut counts = vec![0usize; pieces.len()];
    let mut local = Vec::new();
    for op in &pieces.parse().ops {
        let piece = pieces.pieces_of(op).1;
        let n = counts.get_mut(piece).map_or(0, |n| {
            *n += 1;
            *n - 1
        });
        local.push((piece, n));
    }
    let mut texts: HashMap<(usize, usize), [f64; 4]> = HashMap::new();
    for l in crate::text::text_lines(doc, page).unwrap_or_default() {
        let r = l.rect;
        if let Some(&key) = local.get(l.text_object().1)
            && r.iter().all(|v| v.is_finite())
        {
            texts.entry(key).and_modify(|b| *b = union(*b, r)).or_insert(r);
        }
    }
    let mut out = Vec::new();
    let mut gs = Gs {
        ctm: Matrix::IDENTITY,
        width: 1.0,
        stroke: Some([0.0; 3]),
        fill: Some([0.0; 3]),
        stroke_space: Space::Gray,
        fill_space: Space::Gray,
        clip: None,
    };
    let mut stack: Vec<Gs> = Vec::new();
    for (si, (_, data)) in streams(doc, &p.dict).into_iter().enumerate() {
        let ops = parse(&data).ops;
        let mut path: Option<Building> = None;
        // The open text object: its `BT` and the CTM there.
        let mut text: Option<(usize, Matrix)> = None;
        // The clipping path drawn last at this depth.
        let mut clip: Option<LastClip> = None;
        // An object without a path of its own (image, shading, block, text): its box as outline.
        let boxed =
            |kind: ObjectKind, outline: Vec<Vec<[f64; 2]>>, first: usize, last: usize, gs: &Gs, start: (usize, Matrix, Vec<Vec<[f64; 2]>>)| {
                let rect = bbox(&outline).unwrap_or([0.0; 4]);
                let closed = vec![true; outline.len()];
                let (first, start_ctm) = if start.0 < first { (start.0, start.1) } else { (first, gs.ctm) };
                let o = PageObject {
                    kind,
                    rect,
                    subpaths: outline,
                    closed,
                    stroke: None,
                    fill: None,
                    width: 0.0,
                    clip: None,
                    clip_outline: Vec::new(),
                    resource: None,
                    stream: si,
                    first,
                    last,
                    ctm: gs.ctm.0,
                    start_ctm: start_ctm.0,
                };
                clipped(o, gs.clip, start.2)
            };
        for (i, op) in ops.iter().enumerate() {
            if out.len() >= MAX_OBJECTS {
                return Ok(out);
            }
            let depth = stack.len();
            match op.op.as_slice() {
                b"BT" => text = Some((i, gs.ctm)),
                b"ET" => {
                    let Some((bt, ctm)) = text.take() else { continue };
                    let Some(r) = texts.get(&(si, bt)) else { continue };
                    let start = own_clip(clip.take(), depth, &ops, bt, i, ctm);
                    let starts_at_bt = start.0 == bt;
                    let mut o = boxed(ObjectKind::Text, corners(*r), bt, i, &gs, start);
                    o.ctm = ctm.0;
                    if starts_at_bt {
                        o.start_ctm = ctm.0;
                    }
                    o.fill = gs.fill;
                    out.push(o);
                }
                b"q" => {
                    if stack.len() < 10_000 {
                        stack.push(gs.clone());
                    }
                    clip = None;
                }
                b"Q" => {
                    if let Some(s) = stack.pop() {
                        gs = s;
                    }
                    clip = None;
                }
                b"cm" => {
                    if let Some(m) = op.nums::<6>() {
                        gs.ctm = Matrix(m).then(&gs.ctm);
                    }
                }
                // Colours and the line width count inside text objects too: they last after `ET`.
                b"w" => gs.width = op.num(0).filter(|w| w.is_finite()).unwrap_or(gs.width).max(0.0),
                b"G" => (gs.stroke_space, gs.stroke) = (Space::Gray, colour(Space::Gray, &nums(op))),
                b"g" => (gs.fill_space, gs.fill) = (Space::Gray, colour(Space::Gray, &nums(op))),
                b"RG" => (gs.stroke_space, gs.stroke) = (Space::Rgb, colour(Space::Rgb, &nums(op))),
                b"rg" => (gs.fill_space, gs.fill) = (Space::Rgb, colour(Space::Rgb, &nums(op))),
                b"K" => (gs.stroke_space, gs.stroke) = (Space::Cmyk, colour(Space::Cmyk, &nums(op))),
                b"k" => (gs.fill_space, gs.fill) = (Space::Cmyk, colour(Space::Cmyk, &nums(op))),
                b"CS" => (gs.stroke_space, gs.stroke) = (op.name(0).map_or(Space::Other, space_of), None),
                b"cs" => (gs.fill_space, gs.fill) = (op.name(0).map_or(Space::Other, space_of), None),
                b"SC" | b"SCN" => gs.stroke = colour(gs.stroke_space, &nums(op)),
                b"sc" | b"scn" => gs.fill = colour(gs.fill_space, &nums(op)),
                _ if text.is_some() => {}
                b"m" => {
                    let Some([x, y]) = op.nums::<2>() else { continue };
                    let b = path.get_or_insert_with(|| Building::new(i));
                    let u = user(&gs.ctm, x, y);
                    b.start(u);
                    b.current = Some(u);
                }
                b"l" => {
                    let Some([x, y]) = op.nums::<2>() else { continue };
                    let b = path.get_or_insert_with(|| Building::new(i));
                    let u = user(&gs.ctm, x, y);
                    b.push(u);
                    b.current = Some(u);
                }
                b"c" | b"v" | b"y" => {
                    let v = nums(op);
                    let b = path.get_or_insert_with(|| Building::new(i));
                    let start = b.current.unwrap_or([0.0, 0.0]);
                    // The control points and end, in user space.
                    let (c1, c2, end) = match (op.op.as_slice(), v.as_slice()) {
                        (b"c", [x1, y1, x2, y2, x3, y3]) => (user(&gs.ctm, *x1, *y1), user(&gs.ctm, *x2, *y2), user(&gs.ctm, *x3, *y3)),
                        (b"v", [x2, y2, x3, y3]) => (start, user(&gs.ctm, *x2, *y2), user(&gs.ctm, *x3, *y3)),
                        (b"y", [x1, y1, x3, y3]) => {
                            let e = user(&gs.ctm, *x3, *y3);
                            (user(&gs.ctm, *x1, *y1), e, e)
                        }
                        _ => continue,
                    };
                    for k in 1..=CURVE_STEPS {
                        let t = k as f64 / CURVE_STEPS as f64;
                        let mt = 1.0 - t;
                        let a = mt * mt * mt;
                        let bb = 3.0 * mt * mt * t;
                        let cc = 3.0 * mt * t * t;
                        let d = t * t * t;
                        b.push([a * start[0] + bb * c1[0] + cc * c2[0] + d * end[0], a * start[1] + bb * c1[1] + cc * c2[1] + d * end[1]]);
                    }
                    b.current = Some(end);
                }
                b"h" => {
                    if let Some(b) = path.as_mut() {
                        if let Some(c) = b.closed.last_mut() {
                            *c = true;
                        }
                        b.current = b.subpaths.last().and_then(|s| s.first()).copied();
                    }
                }
                b"re" => {
                    let Some([x, y, w, h]) = op.nums::<4>() else { continue };
                    let b = path.get_or_insert_with(|| Building::new(i));
                    b.start(user(&gs.ctm, x, y));
                    b.push(user(&gs.ctm, x + w, y));
                    b.push(user(&gs.ctm, x + w, y + h));
                    b.push(user(&gs.ctm, x, y + h));
                    if let Some(c) = b.closed.last_mut() {
                        *c = true;
                    }
                    b.current = Some(user(&gs.ctm, x, y));
                }
                b"W" | b"W*" => {
                    if let Some(b) = path.as_mut() {
                        b.clips = true;
                    }
                }
                paint @ (b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n") => {
                    let Some(mut b) = path.take() else { continue };
                    if b.clips {
                        if let Some(r) = bbox(&b.subpaths) {
                            gs.clip = Some(gs.clip.map_or(r, |c| intersect(c, r)));
                        }
                        clip = (paint == b"n").then_some((b.first, depth, gs.ctm, b.subpaths));
                        continue;
                    }
                    if paint == b"n" || b.subpaths.is_empty() {
                        continue;
                    }
                    let stroked = matches!(paint, b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*");
                    let filled = !matches!(paint, b"S" | b"s");
                    if matches!(paint, b"s" | b"b" | b"b*")
                        && let Some(c) = b.closed.last_mut()
                    {
                        *c = true;
                    }
                    // Filled subpaths are closed implicitly.
                    if filled {
                        b.closed.iter_mut().for_each(|c| *c = true);
                    }
                    let Some(rect) = bbox(&b.subpaths) else { continue };
                    let scale = (gs.ctm.0[0] * gs.ctm.0[3] - gs.ctm.0[1] * gs.ctm.0[2]).abs().sqrt();
                    let (first, start_ctm, outline) = own_clip(clip.take(), depth, &ops, b.first, i, gs.ctm);
                    let o = PageObject {
                        kind: ObjectKind::Path { stroked, filled },
                        rect,
                        subpaths: b.subpaths,
                        closed: b.closed,
                        stroke: if stroked { gs.stroke } else { None },
                        fill: if filled { gs.fill } else { None },
                        width: if stroked { gs.width * scale } else { 0.0 },
                        clip: None,
                        clip_outline: Vec::new(),
                        resource: None,
                        stream: si,
                        first,
                        last: i,
                        ctm: gs.ctm.0,
                        start_ctm: start_ctm.0,
                    };
                    out.push(clipped(o, gs.clip, outline));
                }
                b"sh" => {
                    // A shading fills the clipping path (the page, without one).
                    let area = intersect(gs.clip.unwrap_or(crop), crop);
                    let start = own_clip(clip.take(), depth, &ops, i, i, gs.ctm);
                    out.push(boxed(ObjectKind::Shading, corners(area), i, i, &gs, start));
                }
                b"BI" => {
                    let unit = [user(&gs.ctm, 0.0, 0.0), user(&gs.ctm, 1.0, 0.0), user(&gs.ctm, 1.0, 1.0), user(&gs.ctm, 0.0, 1.0)];
                    let start = own_clip(clip.take(), depth, &ops, i, i, gs.ctm);
                    out.push(boxed(ObjectKind::Image, vec![unit.to_vec()], i, i, &gs, start));
                }
                b"Do" => {
                    let Some(name) = op.name(0) else { continue };
                    let Some(r) = xo.get(name).and_then(Object::as_ref) else { continue };
                    let obj = doc.get(r);
                    let Object::Stream(s) = &*obj else { continue };
                    let outline = match s.dict.name(b"Subtype") {
                        Some(b"Image") => {
                            let unit = [user(&gs.ctm, 0.0, 0.0), user(&gs.ctm, 1.0, 0.0), user(&gs.ctm, 1.0, 1.0), user(&gs.ctm, 0.0, 1.0)];
                            (ObjectKind::Image, unit)
                        }
                        Some(b"Form") => {
                            let fb = s.dict.get(b"BBox").map(|b| doc.resolve(b)).and_then(|b| {
                                let v: Vec<f64> = b.as_array()?.iter().filter_map(Object::as_f64).collect();
                                <[f64; 4]>::try_from(v).ok()
                            });
                            let Some(fb) = fb else { continue };
                            let m = s.dict.get(b"Matrix").and_then(|m| Matrix::from_operands(m.as_array()?)).unwrap_or(Matrix::IDENTITY);
                            let place = m.then(&gs.ctm);
                            (
                                ObjectKind::Form,
                                [user(&place, fb[0], fb[1]), user(&place, fb[2], fb[1]), user(&place, fb[2], fb[3]), user(&place, fb[0], fb[3])],
                            )
                        }
                        _ => continue,
                    };
                    let start = own_clip(clip.take(), depth, &ops, i, i, gs.ctm);
                    let mut o = boxed(outline.0, vec![outline.1.to_vec()], i, i, &gs, start);
                    o.resource = Some(String::from_utf8_lossy(name).into_owned());
                    out.push(o);
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

/// What to do with objects.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectChange {
    /// Move by `[dx, dy]` in user space.
    Move([f64; 2]),
    /// Apply a user-space transform after their own (resize, rotate, flip), as `[a b c d e f]` of
    /// `cm`. Lines keep their width.
    Transform([f64; 6]),
    Delete,
    /// Draw lines in this colour (RGB 0–1).
    Stroke([f64; 3]),
    /// Fill (areas, text) in this colour.
    Fill([f64; 3]),
    /// Line width in user space (points); 0 is the thinnest line the device draws.
    Width(f64),
    /// Line style: dash and gap lengths in user space (points), in turn; empty for solid lines.
    Dash(Vec<f64>),
    /// Opacity of lines and fills, 0 (invisible) to 1.
    Opacity(f64),
    /// Replace blocks (form XObjects) by what they draw, so each part can be changed on its own.
    Explode,
}

impl ObjectChange {
    fn check(&self) -> Result<(), EditError> {
        let bad = |why: &str| Err(EditError::Invalid(why.into()));
        match self {
            ObjectChange::Move(d) if !d.iter().all(|v| v.is_finite() && v.abs() < 1e6) => bad("the objects can't be moved that far"),
            ObjectChange::Transform(m) if !m.iter().all(|v| v.is_finite() && v.abs() < 1e6) || (m[0] * m[3] - m[1] * m[2]).abs() < 1e-9 => {
                bad("the objects can't be transformed like that")
            }
            ObjectChange::Stroke(c) | ObjectChange::Fill(c) if !c.iter().all(|v| v.is_finite()) => bad("invalid colour"),
            ObjectChange::Width(w) if !(w.is_finite() && (0.0..=1000.0).contains(w)) => bad("the line width must be between 0 and 1000 points"),
            ObjectChange::Dash(d)
                if d.len() > MAX_DASHES
                    || !d.iter().all(|v| v.is_finite() && (0.0..=1000.0).contains(v))
                    || (!d.is_empty() && d.iter().all(|v| *v == 0.0)) =>
            {
                bad("invalid line style")
            }
            ObjectChange::Opacity(a) if !(a.is_finite() && (0.0..=1.0).contains(a)) => bad("the opacity must be between 0 and 1"),
            _ => Ok(()),
        }
    }
}

/// The affine map (as `[a b c d e f]` of `cm`) that takes the points `from` to `to`, if `from`
/// spans an area. Tools build a transform on screen and carry three points through it.
pub fn affine_from_points(from: [[f64; 2]; 3], to: [[f64; 2]; 3]) -> Option<[f64; 6]> {
    let (u1, u2) = ([from[1][0] - from[0][0], from[1][1] - from[0][1]], [from[2][0] - from[0][0], from[2][1] - from[0][1]]);
    let (v1, v2) = ([to[1][0] - to[0][0], to[1][1] - to[0][1]], [to[2][0] - to[0][0], to[2][1] - to[0][1]]);
    let det = u1[0] * u2[1] - u2[0] * u1[1];
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let a = (v1[0] * u2[1] - v2[0] * u1[1]) / det;
    let c = (v2[0] * u1[0] - v1[0] * u2[0]) / det;
    let b = (v1[1] * u2[1] - v2[1] * u1[1]) / det;
    let d = (v2[1] * u1[0] - v1[1] * u2[0]) / det;
    let m = [a, b, c, d, to[0][0] - a * from[0][0] - c * from[0][1], to[0][1] - b * from[0][0] - d * from[0][1]];
    m.iter().all(|v| v.is_finite()).then_some(m)
}

/// The scale of a matrix (how much it enlarges lengths, on average).
fn scale_of(m: &[f64; 6]) -> f64 {
    (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
}

/// Operators that set state lasting after an object (`Q` doesn't undo what a text object sets
/// before its `ET`).
fn lasting(op: &Op) -> bool {
    matches!(
        op.op.as_slice(),
        b"Tf"
            | b"Tc"
            | b"Tw"
            | b"Tz"
            | b"TL"
            | b"Ts"
            | b"Tr"
            | b"g"
            | b"G"
            | b"rg"
            | b"RG"
            | b"k"
            | b"K"
            | b"cs"
            | b"CS"
            | b"sc"
            | b"SC"
            | b"scn"
            | b"SCN"
            | b"w"
            | b"J"
            | b"j"
            | b"M"
            | b"d"
            | b"ri"
            | b"i"
            | b"gs"
    )
}

/// Whether the change replaces what `op` sets inside the object (its own colour, width or style).
fn overridden(change: &ObjectChange, op: &Op) -> bool {
    match change {
        ObjectChange::Stroke(_) => matches!(op.op.as_slice(), b"G" | b"RG" | b"K" | b"CS" | b"SC" | b"SCN"),
        ObjectChange::Fill(_) => matches!(op.op.as_slice(), b"g" | b"rg" | b"k" | b"cs" | b"sc" | b"scn"),
        ObjectChange::Width(_) => op.op == b"w",
        ObjectChange::Dash(_) => op.op == b"d",
        _ => false,
    }
}

/// Change objects `indices` (from [`page_objects`]) on `page`; returns how many were changed.
pub fn change_objects(doc: &mut Document, page: usize, indices: &[usize], change: &ObjectChange) -> Result<usize, EditError> {
    change.check()?;
    let n = |v: f64| printcraft_content::num(v);
    let objects = page_objects(doc, page)?;
    let mut chosen: Vec<&PageObject> = Vec::new();
    for &i in indices {
        let o = objects.get(i).ok_or_else(|| EditError::Invalid(format!("page {} has no object {}", page + 1, i + 1)))?;
        if !chosen.iter().any(|c| std::ptr::eq(*c, o)) {
            chosen.push(o);
        }
    }
    if *change == ObjectChange::Explode {
        chosen.retain(|o| o.kind == ObjectKind::Form);
        if chosen.is_empty() && !indices.is_empty() {
            return Err(EditError::Invalid("only blocks (form XObjects) can be exploded".into()));
        }
    }
    if chosen.is_empty() {
        return Ok(0);
    }
    let p = page_of(doc, page)?;
    // The page's own copy of its resources, when the change adds to them.
    let mut resources: Option<Dict> = None;
    let opacity = match change {
        ObjectChange::Opacity(a) => {
            let res = resources.get_or_insert_with(|| sub_dict(doc, &p.dict, b"Resources"));
            Some(opacity_state(doc, res, *a))
        }
        _ => None,
    };
    let all = streams(doc, &p.dict);
    let mut new_list: Vec<Object> = all.iter().map(|(o, _)| o.clone()).collect();
    let mut stream_ids: Vec<usize> = chosen.iter().map(|o| o.stream).collect();
    stream_ids.sort_unstable();
    stream_ids.dedup();
    let mut changed = 0;
    for si in stream_ids {
        let Some((obj, data)) = all.get(si) else { continue };
        let mut ops = parse(data).ops;
        // Later objects first, so earlier operator positions stay valid.
        let mut here: Vec<&PageObject> = chosen.iter().copied().filter(|o| o.stream == si).collect();
        here.sort_by_key(|o| std::cmp::Reverse(o.first));
        for o in here {
            let Some(body) = ops.get(o.first..=o.last).map(<[Op]>::to_vec) else { continue };
            let carried: Vec<Op> = body.iter().filter(|op| lasting(op)).cloned().collect();
            let replacement = match change {
                ObjectChange::Delete => carried,
                ObjectChange::Explode => {
                    let res = resources.get_or_insert_with(|| sub_dict(doc, &p.dict, b"Resources"));
                    let Some((last, before)) = body.split_last() else { continue };
                    let Some(parts) = explode(doc, res, last)? else { continue };
                    let mut v = before.to_vec();
                    v.extend(parts);
                    v
                }
                _ => {
                    let mut before = vec![Op::new("q", vec![])];
                    let stroked = matches!(o.kind, ObjectKind::Path { stroked: true, .. });
                    match change {
                        ObjectChange::Move([dx, dy]) => {
                            // New CTM = CTM × T: as a `cm` before the object, M = C · T · C⁻¹.
                            let c = Matrix(o.start_ctm);
                            let Some(inv) = c.invert() else { continue };
                            let m = c.then(&Matrix([1.0, 0.0, 0.0, 1.0, *dx, *dy])).then(&inv);
                            before.push(Op::new("cm", m.0.iter().map(|v| n(*v)).collect()));
                        }
                        ObjectChange::Transform(t) => {
                            let c = Matrix(o.start_ctm);
                            let Some(inv) = c.invert() else { continue };
                            let m = c.then(&Matrix(*t)).then(&inv);
                            before.push(Op::new("cm", m.0.iter().map(|v| n(*v)).collect()));
                            // Lines keep their width: in the new space it is the old one rescaled.
                            let now = scale_of(&Matrix(o.ctm).then(&Matrix(*t)).0);
                            if stroked && now > 1e-12 {
                                before.push(Op::new("w", vec![n(o.width / now)]));
                            }
                        }
                        ObjectChange::Stroke([r, g, b]) => before.push(Op::new("RG", vec![n(*r), n(*g), n(*b)])),
                        ObjectChange::Fill([r, g, b]) => before.push(Op::new("rg", vec![n(*r), n(*g), n(*b)])),
                        ObjectChange::Width(w) => {
                            // The width operator works in the object's space.
                            let scale = scale_of(&o.ctm);
                            before.push(Op::new("w", vec![n(if scale > 1e-12 { w / scale } else { *w })]));
                        }
                        ObjectChange::Dash(d) => {
                            let scale = scale_of(&o.ctm);
                            let local: Vec<Object> = d.iter().map(|v| n(if scale > 1e-12 { v / scale } else { *v })).collect();
                            before.push(Op::new("d", vec![Object::Array(local), Object::Int(0)]));
                        }
                        ObjectChange::Opacity(_) => {
                            if let Some(name) = &opacity {
                                before.push(Op::new("gs", vec![Object::Name(name.clone())]));
                            }
                        }
                        ObjectChange::Delete | ObjectChange::Explode => {}
                    }
                    let mut v = before;
                    for op in body {
                        if overridden(change, &op) {
                            continue;
                        }
                        // An opacity the object sets itself gives way to the new one.
                        let again = op.op == b"gs" && opacity.is_some();
                        v.push(op);
                        if again && let Some(name) = &opacity {
                            v.push(Op::new("gs", vec![Object::Name(name.clone())]));
                        }
                    }
                    v.push(Op::new("Q", vec![]));
                    v.extend(carried);
                    v
                }
            };
            ops.splice(o.first..=o.last, replacement);
            changed += 1;
        }
        let mut dict = match &*doc.resolve(obj) {
            Object::Stream(s) => s.dict.clone(),
            _ => Dict::new(),
        };
        dict.remove(b"Length");
        let new = doc.add(Object::Stream(Stream::flate(dict, &serialize_ops(&ops))));
        if let Some(slot) = new_list.get_mut(si) {
            *slot = Object::Ref(new);
        }
    }
    doc.update_dict(p.obj, |d| {
        d.set(b"Contents".to_vec(), if new_list.len() == 1 { new_list[0].clone() } else { Object::Array(new_list) });
        // The page gets its own copy of its resources; shared dictionaries stay untouched.
        if let Some(res) = resources {
            d.set(b"Resources".to_vec(), Object::Dict(res));
        }
    })?;
    Ok(changed)
}

/// A name not yet in `dict`: `base`, else `base_2`, `base_3`, …
fn unique(dict: &Dict, base: &[u8]) -> Name {
    if !dict.contains(base) {
        return base.to_vec();
    }
    (2..10_000).map(|k| [base, format!("_{k}").as_bytes()].concat()).find(|n| !dict.contains(n)).unwrap_or_else(|| [base, b"_x"].concat())
}

/// The graphics state with opacity `a` (lines and fills) in `res`, returning its name.
fn opacity_state(doc: &Document, res: &mut Dict, a: f64) -> Name {
    let mut states = sub_dict(doc, res, b"ExtGState");
    let mut s = Dict::new();
    s.set(b"Type".to_vec(), Object::name("ExtGState"));
    s.set(b"CA".to_vec(), Object::Real(a));
    s.set(b"ca".to_vec(), Object::Real(a));
    let base = format!("CivOpacity{}", (a * 100.0).round() as i64).into_bytes();
    let name = match states.get(&base) {
        Some(Object::Dict(d)) if *d == s => base,
        _ => unique(&states, &base),
    };
    states.set(name.clone(), Object::Dict(s));
    res.set(b"ExtGState".to_vec(), Object::Dict(states));
    name
}

/// The resource categories a form's content names (§7.8.3).
const CATEGORIES: [&[u8]; 7] = [b"Font", b"XObject", b"ExtGState", b"ColorSpace", b"Pattern", b"Shading", b"Properties"];

/// Colour spaces named in content without a resource.
fn device_space(name: &[u8]) -> bool {
    matches!(name, b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK" | b"Pattern" | b"G" | b"RGB" | b"CMYK")
}

/// The operators that draw the form drawn by `Do` in place of it: `q`, its matrix, its box as a
/// clipping path, its content (in its optional content group, if it has one) and `Q`. Its
/// resources join `res`; names already used there for something else are renamed. `None` when
/// `Do` doesn't draw a form.
fn explode(doc: &mut Document, res: &mut Dict, do_op: &Op) -> Result<Option<Vec<Op>>, EditError> {
    let n = |v: f64| printcraft_content::num(v);
    let Some(name) = do_op.name(0) else { return Ok(None) };
    let Some(r) = sub_dict(doc, res, b"XObject").get(name).and_then(Object::as_ref) else { return Ok(None) };
    let obj = doc.get(r);
    let Object::Stream(s) = &*obj else { return Ok(None) };
    if s.dict.name(b"Subtype") != Some(b"Form") {
        return Ok(None);
    }
    let data = s.decoded().map_err(|e| EditError::Invalid(format!("the block can't be read: {e}")))?;
    let mut ops = parse(&data).ops;
    // Its resources join the page's; a name taken by something else gets a new one.
    let mut renames: HashMap<(usize, Name), Name> = HashMap::new();
    if let Some(own) = s.dict.get(b"Resources").map(|x| doc.resolve(x)).and_then(|x| x.as_dict().cloned()) {
        for (ci, cat) in CATEGORIES.iter().enumerate() {
            let theirs = sub_dict(doc, &own, cat);
            if theirs.is_empty() {
                continue;
            }
            let mut ours = sub_dict(doc, res, cat);
            for (k, v) in theirs.iter() {
                match ours.get(k) {
                    Some(existing) if existing == v => {}
                    None => ours.set(k.clone(), v.clone()),
                    Some(_) => {
                        let new = unique(&ours, k);
                        ours.set(new.clone(), v.clone());
                        renames.insert((ci, k.clone()), new);
                    }
                }
            }
            res.set(cat.to_vec(), Object::Dict(ours));
        }
    }
    if !renames.is_empty() {
        let renamed = |ci: usize, o: &mut Object| {
            if let Object::Name(nm) = o
                && let Some(new) = renames.get(&(ci, nm.clone()))
            {
                *nm = new.clone();
            }
        };
        for op in &mut ops {
            match op.op.as_slice() {
                b"Tf" => op.operands.first_mut().into_iter().for_each(|o| renamed(0, o)),
                b"Do" => op.operands.first_mut().into_iter().for_each(|o| renamed(1, o)),
                b"gs" => op.operands.first_mut().into_iter().for_each(|o| renamed(2, o)),
                b"CS" | b"cs" => op.operands.first_mut().filter(|o| !o.as_name().is_some_and(device_space)).into_iter().for_each(|o| renamed(3, o)),
                b"SCN" | b"scn" => op.operands.last_mut().into_iter().for_each(|o| renamed(4, o)),
                b"sh" => op.operands.first_mut().into_iter().for_each(|o| renamed(5, o)),
                b"BDC" | b"DP" => op.operands.get_mut(1).into_iter().for_each(|o| renamed(6, o)),
                b"BI" => {
                    if let Some((d, _)) = op.inline.as_mut() {
                        for key in [&b"CS"[..], b"ColorSpace"] {
                            if let Some(o) = d.get_mut(key).filter(|o| !o.as_name().is_some_and(device_space)) {
                                renamed(3, o);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // Its own content stays balanced, so it can't undo the page's state around it.
    let (mut depth, mut marked) = (0usize, 0usize);
    ops.retain(|op| match op.op.as_slice() {
        b"q" => {
            depth += 1;
            true
        }
        b"Q" if depth == 0 => false,
        b"Q" => {
            depth -= 1;
            true
        }
        b"BMC" | b"BDC" => {
            marked += 1;
            true
        }
        b"EMC" if marked == 0 => false,
        b"EMC" => {
            marked -= 1;
            true
        }
        _ => true,
    });
    ops.extend((0..marked).map(|_| Op::new("EMC", vec![])));
    ops.extend((0..depth).map(|_| Op::new("Q", vec![])));
    let mut out = vec![Op::new("q", vec![])];
    if let Some(m) = s.dict.get(b"Matrix").and_then(|m| Matrix::from_operands(m.as_array()?))
        && m != Matrix::IDENTITY
    {
        out.push(Op::new("cm", m.0.iter().map(|v| n(*v)).collect()));
    }
    let fb = s.dict.get(b"BBox").map(|b| doc.resolve(b)).and_then(|b| {
        let v: Vec<f64> = b.as_array()?.iter().filter_map(Object::as_f64).collect();
        <[f64; 4]>::try_from(v).ok()
    });
    if let Some(b) = fb.filter(|b| b.iter().all(|v| v.is_finite())) {
        out.push(Op::new("re", vec![n(b[0].min(b[2])), n(b[1].min(b[3])), n((b[2] - b[0]).abs()), n((b[3] - b[1]).abs())]));
        out.push(Op::new("W", vec![]));
        out.push(Op::new("n", vec![]));
    }
    // Hidden with its layer, as before.
    let layer = s.dict.get(b"OC").cloned().map(|oc| {
        let mut props = sub_dict(doc, res, b"Properties");
        let name = unique(&props, b"CivLayer");
        props.set(name.clone(), oc);
        res.set(b"Properties".to_vec(), Object::Dict(props));
        name
    });
    if let Some(l) = &layer {
        out.push(Op::new("BDC", vec![Object::name("OC"), Object::Name(l.clone())]));
    }
    out.extend(ops);
    if layer.is_some() {
        out.push(Op::new("EMC", vec![]));
    }
    out.push(Op::new("Q", vec![]));
    Ok(Some(out))
}
