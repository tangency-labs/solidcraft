//! Linked (associative) reference geometry: curves and points projected into a sketch from
//! model edges, faces, bodies, other sketches and the origin. The sketch only stores the link
//! (what it refers to) and the geometry it produced last time; resolving a link against the
//! model happens in the document crate, which hands the new geometry to [`Sketch::update_link`].
//!
//! Linked points are fixed and linked circles keep their radius, so the solver treats them as
//! constants; they still bound profiles and take constraints and dimensions like any curve.

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb2, Vec2, Vec3};

use crate::model::{Curve, CurveKind, Result, Sketch, SketchError, in_range, radius_in_range};

/// Points of one link closer than this are merged into one point.
const MERGE_TOL: f64 = 1e-7;

#[path = "link_loops.rs"]
mod loops;
#[path = "link_rebuild.rs"]
mod rebuild;
/// Most entities a single link may produce.
pub const MAX_LINK_ENTITIES: usize = 20_000;

/// How the geometry was brought in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// Projected onto the sketch plane along its normal.
    #[default]
    Project,
    /// Included as is (curves in the sketch plane; others are flattened onto it).
    Include,
    /// Section curves of a body or face with the sketch plane.
    Intersect,
    /// Text outlines (drawn like normal geometry; edit the text to change them).
    Text,
}

/// What a link refers to. Model geometry is referenced by a point on it plus the body name and
/// re-found on every recompute (like fillet edges), so it follows upstream edits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LinkSource {
    /// A body edge through `at`.
    Edge { body: String, at: Vec3 },
    /// The boundary loops of the body face through `at` (Intersect: the face's section).
    Face { body: String, at: Vec3 },
    /// A whole body: its silhouette (Project) or its section (Intersect).
    Body { body: String },
    /// A body vertex at `at`.
    Vertex { body: String, at: Vec3 },
    /// A curve of another sketch.
    SketchCurve { sketch: u64, curve: String },
    /// A point of another sketch.
    SketchPoint { sketch: u64, point: String },
    /// The model origin.
    Origin,
    /// An origin axis (`X`, `Y`, `Z`).
    Axis { name: String },
    /// One of the sketch's own axes (`x` or `y`): the line through the sketch origin along it,
    /// whatever plane the sketch is on (see [`sketch_axis_geom`]).
    SketchAxis { name: String },
    /// An origin plane (`XY`, `XZ`, `YZ`) or a construction plane (by name): its trace on the
    /// sketch plane.
    Plane { name: String },
    /// A curve of a sketch projected onto a body face (through `at`) along the normal of the
    /// sketch that holds the link: a 3D curve.
    OnSurface { sketch: u64, curve: String, body: String, at: Vec3 },
    /// Where two bodies or faces (`Body` / `Face` sources) meet: 3D curves.
    Intersection { a: Box<LinkSource>, b: Box<LinkSource> },
    /// An isoparametric curve of the face through `at`: `dir` is `u` (around a curved face),
    /// `v` (along it) or an explicit direction `[x, y, z]` (as text).
    Iso { body: String, at: Vec3, dir: String },
    /// The outline of a body spun about an axis (world line), laid into the sketch plane.
    Spun { body: String, origin: Vec3, dir: Vec3 },
    /// Text: baseline start `at` (sketch coordinates), capital height and angle (radians), in
    /// `font` (a font file path or an installed family name; none: the built-in font).
    Text {
        text: String,
        at: Vec2,
        height: f64,
        #[serde(default)]
        angle: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font: Option<String>,
        /// Bold or italic, alignment, spacing and the curve the text follows.
        #[serde(default, skip_serializing_if = "crate::text::TextStyle::is_plain")]
        style: crate::text::TextStyle,
        /// The expressions behind the numbers above that were typed as expressions.
        #[serde(default, skip_serializing_if = "TextExprs::is_empty")]
        exprs: TextExprs,
    },
}

/// The expressions behind a text's numbers that were typed as expressions: its place (`x`, `y`),
/// its height and its angle; and behind its words (`text`), an expression of the design's text
/// parameters. A number or text without an expression is the one typed. The numbers and words
/// of `LinkSource::Text` are what the expressions gave last: the document evaluates them against
/// its parameters before the text's outlines are rebuilt (`Document::apply_text_values`), and
/// the sketch crate, which knows no parameters, only keeps them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextExprs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// The words: text parameters and quoted texts joined by `+` (`Label + '-L'`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl TextExprs {
    pub fn is_empty(&self) -> bool {
        *self == TextExprs::default()
    }

    /// The expressions of numbers set, each with what it sets (the words' expression is of
    /// texts, not numbers: [`TextExprs::text`]).
    pub fn list(&self) -> Vec<(&'static str, &String)> {
        [("Text X", &self.x), ("Text Y", &self.y), ("Text height", &self.height), ("Text angle", &self.angle)]
            .into_iter()
            .filter_map(|(what, e)| e.as_ref().map(|e| (what, e)))
            .collect()
    }

    pub fn list_mut(&mut self) -> Vec<&mut String> {
        [&mut self.x, &mut self.y, &mut self.height, &mut self.angle].into_iter().filter_map(Option::as_mut).collect()
    }
}

impl LinkSource {
    /// Short description for messages and the browser.
    pub fn describe(&self) -> String {
        match self {
            LinkSource::Edge { body, .. } => format!("edge of {body}"),
            LinkSource::Face { body, .. } => format!("face of {body}"),
            LinkSource::Body { body } => body.clone(),
            LinkSource::Vertex { body, .. } => format!("vertex of {body}"),
            LinkSource::SketchCurve { curve, .. } => format!("sketch curve {curve}"),
            LinkSource::SketchPoint { point, .. } => format!("sketch point {point}"),
            LinkSource::Origin => "origin".into(),
            LinkSource::Axis { name } => format!("{name} axis"),
            LinkSource::SketchAxis { name } => format!("sketch {name} axis"),
            LinkSource::Plane { name } => format!("{name} plane"),
            LinkSource::OnSurface { curve, .. } => format!("{curve} on a face"),
            LinkSource::Intersection { a, b } => format!("intersection of {} and {}", a.describe(), b.describe()),
            LinkSource::Spun { body, .. } => format!("spun profile of {body}"),
            LinkSource::Iso { body, .. } => format!("isoparametric curve of {body}"),
            LinkSource::Text { text, .. } => format!("text \"{}\"", text.chars().take(20).collect::<String>()),
        }
    }

    /// The bodies this source reads geometry from (an intersection reads two).
    pub fn bodies(&self) -> Vec<&str> {
        match self {
            LinkSource::Edge { body, .. }
            | LinkSource::Face { body, .. }
            | LinkSource::Body { body }
            | LinkSource::Vertex { body, .. }
            | LinkSource::OnSurface { body, .. }
            | LinkSource::Iso { body, .. }
            | LinkSource::Spun { body, .. } => vec![body.as_str()],
            LinkSource::Intersection { a, b } => {
                let mut bodies = a.bodies();
                bodies.extend(b.bodies());
                bodies
            }
            LinkSource::SketchCurve { .. }
            | LinkSource::SketchPoint { .. }
            | LinkSource::Origin
            | LinkSource::Axis { .. }
            | LinkSource::SketchAxis { .. }
            | LinkSource::Plane { .. }
            | LinkSource::Text { .. } => Vec::new(),
        }
    }
}

/// Half length of the line a sketch axis link produces.
pub const SKETCH_AXIS_HALF: f64 = 100.0;

/// The geometry of sketch axis `name` (`x` or `y`, sketch coordinates); `None` for any other name.
pub fn sketch_axis_geom(name: &str) -> Option<LinkGeom> {
    let u = match name {
        "x" => Vec2::X,
        "y" => Vec2::Y,
        _ => return None,
    };
    Some(LinkGeom::Line(Vec2::ZERO - u * SKETCH_AXIS_HALF, u * SKETCH_AXIS_HALF))
}

/// One link and its state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    #[serde(default)]
    pub kind: LinkKind,
    pub source: LinkSource,
    /// Persistent model face identity, resolved by the document before geometry refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_name: Option<String>,
    /// Persistent model edge identity of an `Edge` source, resolved by the document before
    /// geometry refresh. None: the edge is the one nearest the source's point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_name: Option<String>,
    /// Automatically projected borders owned by a durable sketch face attachment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_face: bool,
    /// The reference could not be found on the last recompute; the geometry is the last good one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lost: bool,
}

/// Geometry a link produces, in sketch coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkGeom {
    Point(Vec2),
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
    /// Counter-clockwise arc around `c` from `a` to `b`.
    Arc {
        c: Vec2,
        a: Vec2,
        b: Vec2,
    },
    /// Conic from `a` to `b` with control `apex` (rho 0.5: a quadratic Bézier).
    Conic {
        a: Vec2,
        apex: Vec2,
        b: Vec2,
        rho: f64,
    },
    /// Full ellipse: centre, end of the major axis, minor radius.
    Ellipse {
        c: Vec2,
        major: Vec2,
        minor: f64,
    },
    /// Spline through (`control == false`) or over (`control`) the points.
    Spline {
        pts: Vec<Vec2>,
        control: bool,
        degree: u8,
        periodic: bool,
    },
}

impl LinkGeom {
    fn finite(&self) -> bool {
        let ok = |p: &Vec2| in_range(*p);
        match self {
            LinkGeom::Point(p) => ok(p),
            LinkGeom::Line(a, b) => ok(a) && ok(b) && a.dist(*b) > MERGE_TOL,
            LinkGeom::Circle(c, r) => ok(c) && radius_in_range(*r),
            LinkGeom::Arc { c, a, b } => ok(c) && ok(a) && ok(b) && c.dist(*a) > 1e-9 && a.dist(*b) > MERGE_TOL,
            LinkGeom::Conic { a, apex, b, rho } => ok(a) && ok(apex) && ok(b) && a.dist(*b) > MERGE_TOL && *rho > 1e-6 && *rho < 1.0 - 1e-6,
            LinkGeom::Ellipse { c, major, minor } => ok(c) && ok(major) && c.dist(*major) > 1e-9 && radius_in_range(*minor),
            LinkGeom::Spline { pts, control, degree, periodic } => {
                (!*periodic || (!*control && *degree == 3 && crate::periodic_beziers(pts).is_ok()))
                    && (2..=crate::curves::MAX_SPLINE_POINTS).contains(&pts.len())
                    && pts.iter().all(ok)
                    && (1..=solidcraft_geom::MAX_BEZIER_DEGREE as u8).contains(degree)
            }
        }
    }
}

/// Link geometry as merged points plus curves over them (indices into the point list).
struct Layout {
    pts: Vec<Vec2>,
    /// Free-standing points (not owned by a curve).
    lone: Vec<usize>,
    curves: Vec<LCurve>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LCurve {
    Line(usize, usize),
    Circle(usize, f64),
    Arc(usize, usize, usize),
    Conic(usize, usize, usize, f64),
    Ellipse(usize, usize, f64),
    /// Start, end, first interior point and interior count (interior points are consecutive
    /// and never shared), control flag, degree.
    Spline(usize, usize, usize, usize, bool, u8, bool),
}

fn layout(geom: &[LinkGeom]) -> Result<Layout> {
    if geom.len() > MAX_LINK_ENTITIES {
        return Err(SketchError::TooLarge);
    }
    let mut l = Layout { pts: Vec::new(), lone: Vec::new(), curves: Vec::new() };
    fn pt(pts: &mut Vec<Vec2>, p: Vec2) -> usize {
        if let Some(i) = pts.iter().position(|q| q.dist(p) <= MERGE_TOL) {
            return i;
        }
        pts.push(p);
        pts.len() - 1
    }
    // A face seen edge-on projects its edges onto one another; one line stands for them.
    let same_line = |cs: &[LCurve], a: usize, b: usize| cs.iter().any(|k| matches!(*k, LCurve::Line(x, y) if (x, y) == (a, b) || (x, y) == (b, a)));
    for g in geom.iter().filter(|g| g.finite()) {
        match g.clone() {
            LinkGeom::Point(p) => {
                let i = pt(&mut l.pts, p);
                if !l.lone.contains(&i) {
                    l.lone.push(i);
                }
            }
            LinkGeom::Line(a, b) => {
                let (a, b) = (pt(&mut l.pts, a), pt(&mut l.pts, b));
                if a != b && !same_line(&l.curves, a, b) {
                    l.curves.push(LCurve::Line(a, b));
                }
            }
            LinkGeom::Circle(c, r) => {
                let c = pt(&mut l.pts, c);
                l.curves.push(LCurve::Circle(c, r));
            }
            LinkGeom::Arc { c, a, b } => {
                let r = c.dist(a);
                let b = c + (b - c).normalized().unwrap_or(Vec2::X) * r;
                let (c, a, b) = (pt(&mut l.pts, c), pt(&mut l.pts, a), pt(&mut l.pts, b));
                if a != b && c != a {
                    l.curves.push(LCurve::Arc(c, a, b));
                }
            }
            LinkGeom::Conic { a, apex, b, rho } => {
                let (a, b, x) = (pt(&mut l.pts, a), pt(&mut l.pts, b), pt(&mut l.pts, apex));
                if a != b && x != a && x != b {
                    l.curves.push(LCurve::Conic(a, b, x, rho));
                }
            }
            LinkGeom::Ellipse { c, major, minor } => {
                let (c, m) = (pt(&mut l.pts, c), pt(&mut l.pts, major));
                if c != m {
                    l.curves.push(LCurve::Ellipse(c, m, minor));
                }
            }
            LinkGeom::Spline { pts, control, degree, periodic } => {
                let (Some(first), Some(last)) = (pts.first(), pts.last()) else { continue };
                let s0 = pt(&mut l.pts, *first);
                let first_in = l.pts.len();
                let inner = pts.get(1..pts.len() - 1).unwrap_or_default();
                l.pts.extend(inner.iter().copied());
                let s1 = pt(&mut l.pts, *last);
                l.curves.push(LCurve::Spline(s0, s1, first_in, inner.len(), control, degree, periodic));
            }
        }
    }
    // Lone points that a curve also uses are not lone.
    let used = |i: usize, cs: &[LCurve]| {
        cs.iter().any(|c| match *c {
            LCurve::Line(a, b) => a == i || b == i,
            LCurve::Circle(c, _) => c == i,
            LCurve::Arc(c, a, b) => c == i || a == i || b == i,
            LCurve::Conic(a, b, x, _) => a == i || b == i || x == i,
            LCurve::Ellipse(c, m, _) => c == i || m == i,
            LCurve::Spline(a, b, f, n, ..) => a == i || b == i || (i >= f && i < f + n),
        })
    };
    let curves = l.curves.clone();
    l.lone.retain(|i| !used(*i, &curves));
    // A face seen edge-on: every edge falls on one line, and one line end to end stands for
    // them all (its vertices can be projected one by one).
    if l.lone.is_empty() && l.curves.len() > 1 && flat(&l.curves, &l.pts) {
        let (a, b) = extent(&l.pts);
        l = Layout { pts: vec![a, b], lone: Vec::new(), curves: vec![LCurve::Line(0, 1)] };
    }
    Ok(l)
}

/// The two points farthest apart.
fn extent(pts: &[Vec2]) -> (Vec2, Vec2) {
    let farthest = |from: Vec2| pts.iter().copied().max_by(|p, q| p.dist(from).total_cmp(&q.dist(from))).unwrap_or(from);
    let a = farthest(pts.first().copied().unwrap_or(Vec2::ZERO));
    (a, farthest(a))
}

/// Straight pieces along one line: a face seen edge-on, whose silhouette encloses nothing.
fn flat(curves: &[LCurve], pts: &[Vec2]) -> bool {
    if !curves.iter().all(|c| matches!(c, LCurve::Line(..))) {
        return false;
    }
    let (a, b) = extent(pts);
    let Some(dir) = (b - a).normalized() else { return true };
    let tol = MERGE_TOL.max(1e-9 * a.dist(b));
    pts.iter().all(|p| (*p - a).cross(dir).abs() <= tol)
}

fn same_shape(a: &LCurve, b: &LCurve) -> bool {
    matches!(
        (a, b),
        (LCurve::Line(..), LCurve::Line(..))
            | (LCurve::Circle(..), LCurve::Circle(..))
            | (LCurve::Arc(..), LCurve::Arc(..))
            | (LCurve::Conic(..), LCurve::Conic(..))
            | (LCurve::Ellipse(..), LCurve::Ellipse(..))
            | (LCurve::Spline(..), LCurve::Spline(..))
    )
}

/// Do two curve sets describe the same geometry, regardless of the order the curves come in or
/// the direction a loop was walked? Lines match with either end first; circles by centre and
/// radius; arcs by centre and their (start, end) pair; conics by all their points and rho.
fn same_geometry(a: &[(LCurve, Vec<Vec2>)], b: &[(LCurve, Vec<Vec2>)]) -> bool {
    const TOL: f64 = 1e-6;
    let near = |p: Vec2, q: Vec2| p.dist(q) <= TOL * (1.0 + p.len().max(q.len()));
    let close = |x: f64, y: f64| (x - y).abs() <= TOL * (1.0 + x.abs().max(y.abs()));
    let matches = |(ca, pa): &(LCurve, Vec<Vec2>), (cb, pb): &(LCurve, Vec<Vec2>)| -> bool {
        if pa.len() != pb.len() {
            return false;
        }
        let all = || pa.iter().zip(pb).all(|(p, q)| near(*p, *q));
        match (ca, cb) {
            (LCurve::Line(..), LCurve::Line(..)) => {
                all() || matches!((pa.as_slice(), pb.as_slice()), ([a0, a1], [b0, b1]) if near(*a0, *b1) && near(*a1, *b0))
            }
            (LCurve::Circle(_, ra), LCurve::Circle(_, rb)) => all() && close(*ra, *rb),
            (LCurve::Arc(..), LCurve::Arc(..)) => all(),
            (LCurve::Conic(.., ra), LCurve::Conic(.., rb)) => all() && close(*ra, *rb),
            (LCurve::Ellipse(.., ra), LCurve::Ellipse(.., rb)) => all() && close(*ra, *rb),
            (LCurve::Spline(.., ca, da, pa), LCurve::Spline(.., cb, db, pb)) => ca == cb && da == db && pa == pb && all(),
            _ => false,
        }
    };
    if a.len() != b.len() {
        return false;
    }
    // Unchanged ordered links are common; avoid a quadratic permutation scan.
    if a.iter().zip(b).all(|(x, y)| matches(x, y)) {
        return true;
    }
    if a.len() > 512 {
        return false;
    }
    let mut used = vec![false; b.len()];
    a.iter().all(|x| {
        let hit = b.iter().enumerate().find(|(j, y)| !used.get(*j).copied().unwrap_or(true) && matches(x, y)).map(|(j, _)| j);
        match hit.and_then(|j| used.get_mut(j)) {
            Some(u) => {
                *u = true;
                true
            }
            None => false,
        }
    })
}

/// The point indices a layout curve uses, in order.
/// Is `fresh` the boundary `old` was, with only the stations along its straight lines gone
/// (#1049: straight edges that booleans left split at vertices nothing else used are now one
/// edge)? Every old line lies along exactly one fresh line and the old pieces cover it end to
/// end; every other curve is unchanged. A link saved with the stations keeps its pieces then:
/// the region they bound is the same, and the features that name them by id still find them.
fn only_stations_gone(old: &[(LCurve, Vec<Vec2>)], fresh: &[(LCurve, Vec<Vec2>)]) -> bool {
    const TOL: f64 = 1e-6;
    let is_line = |c: &&(LCurve, Vec<Vec2>)| matches!(c.0, LCurve::Line(..));
    let ends = |c: &(LCurve, Vec<Vec2>)| match c.1.as_slice() {
        [a, b] => Some((*a, *b)),
        _ => None,
    };
    let (old_lines, old_rest): (Vec<_>, Vec<_>) = old.iter().partition(is_line);
    let (fresh_lines, fresh_rest): (Vec<_>, Vec<_>) = fresh.iter().partition(is_line);
    if fresh_lines.len() >= old_lines.len() || old_lines.len() > 512 {
        return false;
    }
    let rest = |xs: Vec<&(LCurve, Vec<Vec2>)>| xs.into_iter().cloned().collect::<Vec<_>>();
    if !same_geometry(&rest(old_rest), &rest(fresh_rest)) {
        return false;
    }
    let Some(fresh_segs) = fresh_lines.iter().map(|c| ends(c)).collect::<Option<Vec<_>>>() else { return false };
    let mut covered = vec![0.0; fresh_segs.len()];
    for c in &old_lines {
        let Some((p, q)) = ends(c) else { return false };
        let on = |x: Vec2, (a, b): (Vec2, Vec2)| {
            let ab = b - a;
            let t = ((x - a).dot(ab) / ab.dot(ab).max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
            x.dist(a + ab * t) <= TOL * (1.0 + a.len().max(b.len()))
        };
        let Some(k) = fresh_segs.iter().position(|seg| on(p, *seg) && on(q, *seg)) else { return false };
        if let Some(slot) = covered.get_mut(k) {
            *slot += p.dist(q);
        }
    }
    fresh_segs.iter().zip(&covered).all(|((a, b), len)| (a.dist(*b) - len).abs() <= TOL * (1.0 + a.len().max(b.len())))
}

fn curve_points(c: &LCurve) -> Vec<usize> {
    match *c {
        LCurve::Line(a, b) => vec![a, b],
        LCurve::Circle(c, _) => vec![c],
        LCurve::Arc(c, a, b) => vec![c, a, b],
        LCurve::Conic(a, b, x, _) => vec![a, b, x],
        LCurve::Ellipse(c, m, _) => vec![c, m],
        LCurve::Spline(a, b, f, n, ..) => {
            let mut v = vec![a];
            v.extend(f..f + n);
            v.push(b);
            v
        }
    }
}

/// Incidence roles distinguish nearby arc centres, endpoints and hole centres during a
/// resize. Spatial proximity alone can select a different kind of point in a dense rim.
fn point_roles(curves: &[LCurve], point_count: usize) -> Vec<[usize; 10]> {
    let mut roles = vec![[0; 10]; point_count];
    let mut add = |point: usize, role: usize| {
        if let Some(count) = roles.get_mut(point).and_then(|r| r.get_mut(role)) {
            *count += 1;
        }
    };
    for curve in curves {
        match *curve {
            LCurve::Line(a, b) => {
                add(a, 0);
                add(b, 0);
            }
            LCurve::Circle(c, _) => add(c, 1),
            LCurve::Arc(c, a, b) => {
                add(c, 2);
                add(a, 3);
                add(b, 3);
            }
            LCurve::Conic(a, b, x, _) => {
                add(a, 4);
                add(b, 4);
                add(x, 5);
            }
            LCurve::Ellipse(c, m, _) => {
                add(c, 6);
                add(m, 7);
            }
            LCurve::Spline(a, b, first, count, ..) => {
                add(a, 8);
                add(b, 8);
                for i in first..first + count {
                    add(i, 9);
                }
            }
        }
    }
    roles
}

impl Sketch {
    pub fn link(&self, id: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.id == id)
    }

    /// The expressions of the sketch's texts and of its points placed by parameters, each with
    /// what it sets (`Text height`, `Point X`…).
    pub fn text_exprs(&self) -> Vec<(&'static str, &String)> {
        self.links
            .iter()
            .filter_map(|l| match &l.source {
                LinkSource::Text { exprs, .. } => Some(exprs.list()),
                _ => None,
            })
            .flatten()
            .chain(self.points.iter().filter_map(|p| p.exprs.as_deref()).flat_map(crate::PointExprs::list))
            .collect()
    }

    /// The expressions of the words of the sketch's texts (of text parameters).
    pub fn text_word_exprs(&self) -> Vec<&String> {
        self.links
            .iter()
            .filter_map(|l| match &l.source {
                LinkSource::Text { exprs, .. } => exprs.text.as_ref(),
                _ => None,
            })
            .collect()
    }

    /// Mutable view of [`Sketch::text_word_exprs`], for renaming the parameters they refer to.
    pub fn text_word_exprs_mut(&mut self) -> Vec<&mut String> {
        self.links
            .iter_mut()
            .filter_map(|l| match &mut l.source {
                LinkSource::Text { exprs, .. } => exprs.text.as_mut(),
                _ => None,
            })
            .collect()
    }

    /// Mutable view of [`Sketch::text_exprs`], for renaming the parameters they refer to.
    pub fn text_exprs_mut(&mut self) -> Vec<&mut String> {
        self.links
            .iter_mut()
            .filter_map(|l| match &mut l.source {
                LinkSource::Text { exprs, .. } => Some(exprs.list_mut()),
                _ => None,
            })
            .flatten()
            .chain(self.points.iter_mut().filter_map(|p| p.exprs.as_deref_mut()).flat_map(crate::PointExprs::list_mut))
            .collect()
    }

    /// Curves owned by a link, in creation order.
    pub fn link_curves(&self, id: &str) -> Vec<usize> {
        self.curves.iter().enumerate().filter(|(_, c)| c.link.as_deref() == Some(id)).map(|(i, _)| i).collect()
    }

    /// Points owned by a link, in creation order.
    pub fn link_points(&self, id: &str) -> Vec<usize> {
        self.points.iter().enumerate().filter(|(_, p)| p.link.as_deref() == Some(id)).map(|(i, _)| i).collect()
    }

    /// Is this curve linked reference geometry?
    pub fn is_linked_curve(&self, ci: usize) -> bool {
        self.curves.get(ci).is_some_and(|c| c.link.is_some())
    }

    /// Is this curve text outline (linked, but drawn like normal geometry)?
    pub fn is_text_curve(&self, ci: usize) -> bool {
        self.curves.get(ci).and_then(|c| c.link.as_deref()).and_then(|l| self.link(l)).is_some_and(|l| l.kind == LinkKind::Text)
    }

    /// Is the curve's link lost (its reference no longer found)?
    pub fn is_lost_curve(&self, ci: usize) -> bool {
        self.curves.get(ci).and_then(|c| c.link.as_deref()).and_then(|l| self.link(l)).is_some_and(|l| l.lost)
    }

    /// The line of sketch axis `name` (`x` or `y`) when the sketch already has one (see
    /// [`Sketch::sketch_axis_line`]).
    pub fn sketch_axis_curve(&self, name: &str) -> Option<usize> {
        self.links
            .iter()
            .filter(|l| matches!(&l.source, LinkSource::SketchAxis { name: n } if n == name))
            .find_map(|l| self.link_curves(&l.id).into_iter().find(|c| matches!(self.curves.get(*c).map(|c| &c.kind), Some(CurveKind::Line { .. }))))
    }

    /// The line of sketch axis `name` (`x` or `y`) that constraints can refer to: a linked
    /// construction line on the axis (it stays there and is left out of profiles), shared by
    /// every reference to that axis and added on first use. `None` when `name` is not a sketch
    /// axis.
    pub fn sketch_axis_line(&mut self, name: &str) -> Result<Option<usize>> {
        let Some(geom) = sketch_axis_geom(name) else { return Ok(None) };
        if let Some(c) = self.sketch_axis_curve(name) {
            return Ok(Some(c));
        }
        let id = self.add_link(LinkKind::Project, LinkSource::SketchAxis { name: name.into() }, &[geom])?;
        let curves = self.link_curves(&id);
        for c in &curves {
            if let Some(cu) = self.curves.get_mut(*c) {
                cu.construction = true;
            }
        }
        curves.first().copied().map(Some).ok_or_else(|| SketchError::Invalid(format!("sketch {name} axis")))
    }

    /// Is all of `geom` already linked into this sketch: the border of the face the sketch was
    /// put on, or an edge projected before? Linking it again would lay a second copy over the
    /// first. Lines match with either end first, circles by centre and radius, arcs by centre
    /// and ends; other curves, and points on their own (the origin, a vertex), never count as
    /// present.
    pub fn link_geometry_present(&self, geom: &[LinkGeom]) -> bool {
        let Ok(lay) = layout(geom) else { return false };
        if lay.curves.is_empty() || !lay.lone.is_empty() {
            return false;
        }
        let at = |i: usize| self.points.get(i).map(|p| p.pos);
        let near = |i: usize, q: Option<Vec2>| lay.pts.get(i).zip(q).is_some_and(|(p, q)| p.dist(q) <= MERGE_TOL);
        let curve_present = |c: &LCurve| {
            self.curves.iter().filter(|k| k.link.is_some()).any(|k| match (*c, &k.kind) {
                (LCurve::Line(a, b), CurveKind::Line { a: p, b: q }) => (near(a, at(*p)) && near(b, at(*q))) || (near(a, at(*q)) && near(b, at(*p))),
                (LCurve::Circle(c, r), CurveKind::Circle { c: p, r: s }) => near(c, at(*p)) && (r - s).abs() <= MERGE_TOL,
                (LCurve::Arc(c, a, b), CurveKind::Arc { c: p, a: q, b: e }) => near(c, at(*p)) && near(a, at(*q)) && near(b, at(*e)),
                _ => false,
            })
        };
        lay.curves.iter().all(curve_present)
    }

    /// Add linked geometry. Returns the link id; the new curve and point ids are
    /// [`Sketch::link_curves`] / [`Sketch::link_points`] of it.
    pub fn add_link(&mut self, kind: LinkKind, source: LinkSource, geom: &[LinkGeom]) -> Result<String> {
        let lay = layout(geom)?;
        if lay.curves.is_empty() && lay.lone.is_empty() {
            return Err(SketchError::Invalid("the reference projects to nothing on this sketch plane".into()));
        }
        if self.points.len() + lay.pts.len() > crate::model::MAX_POINTS {
            return Err(SketchError::TooLarge);
        }
        let id = self.fresh("j")?;
        self.links.push(Link { id: id.clone(), kind, source, face_name: None, edge_name: None, auto_face: false, lost: false });
        self.build_link(&id, &lay)?;
        Ok(id)
    }

    /// Make the entities of link `id` for `lay`. Returns the point each layout point became.
    fn build_link(&mut self, id: &str, lay: &Layout) -> Result<Vec<Option<usize>>> {
        let mut idx: Vec<Option<usize>> = vec![None; lay.pts.len()];
        let mut point = |sk: &mut Sketch, li: usize| -> Result<usize> {
            if let Some(Some(i)) = idx.get(li) {
                return Ok(*i);
            }
            let p = lay.pts.get(li).copied().ok_or_else(|| SketchError::Invalid("link point".into()))?;
            let i = sk.add_point(p, None)?;
            if let Some(sp) = sk.points.get_mut(i) {
                sp.fixed = true;
                sp.link = Some(id.to_string());
            }
            if let Some(slot) = idx.get_mut(li) {
                *slot = Some(i);
            }
            Ok(i)
        };
        for c in &lay.curves {
            let (kind, prefix) = match *c {
                LCurve::Line(a, b) => (CurveKind::Line { a: point(self, a)?, b: point(self, b)? }, "l"),
                LCurve::Circle(c, r) => (CurveKind::Circle { c: point(self, c)?, r }, "c"),
                LCurve::Arc(c, a, b) => (CurveKind::Arc { c: point(self, c)?, a: point(self, a)?, b: point(self, b)? }, "a"),
                LCurve::Conic(a, b, x, rho) => (CurveKind::Conic { a: point(self, a)?, b: point(self, b)?, apex: point(self, x)?, rho }, "n"),
                LCurve::Ellipse(c, m, r) => (CurveKind::Ellipse { c: point(self, c)?, m: point(self, m)?, r }, "e"),
                LCurve::Spline(a, b, f, n, control, degree, periodic) => {
                    let mut pts = vec![point(self, a)?];
                    for i in f..f + n {
                        pts.push(point(self, i)?);
                    }
                    pts.push(point(self, b)?);
                    (CurveKind::Spline { pts, control, degree, periodic }, "s")
                }
            };
            let cid = self.fresh(prefix)?;
            self.curves.push(Curve {
                id: cid,
                kind,
                construction: false,
                reversed: false,
                link: Some(id.to_string()),
                centerline: false,
                fixed: false,
                infinite: false,
            });
        }
        for li in &lay.lone {
            point(self, *li)?;
        }
        Ok(idx)
    }

    /// Replace a link's geometry with a newly resolved one. When the shape is the same (same
    /// curve kinds sharing the same points) the entities keep their ids and only move, so
    /// constraints and dimensions on them survive; otherwise the link is rebuilt, and the
    /// sketch's own curves and constraints on it move to the new entities that match the old
    /// ones (`rebuild_link`). Returns `true` when it was rebuilt.
    pub fn update_link(&mut self, id: &str, geom: &[LinkGeom]) -> Result<bool> {
        if self.link(id).is_none() {
            return Err(SketchError::Unknown(id.into()));
        }
        let lay = layout(geom)?;
        if self.link(id).is_some_and(|l| matches!(l.source, LinkSource::Face { .. }))
            && lay.lone.is_empty()
            && lay.curves.len() <= 512
            && lay.pts.len() <= 512
            && lay.curves.iter().all(|c| matches!(c, LCurve::Line(..) | LCurve::Arc(..) | LCurve::Circle(..)))
            && !flat(&lay.curves, &lay.pts)
            && !loops::valid(&lay.curves, &lay.pts)
        {
            self.set_link_lost(id, true);
            return Err(SketchError::Invalid("projected face boundaries cross, touch or have ambiguous topology; last geometry retained".into()));
        }
        let curves = self.link_curves(id);
        let points = self.link_points(id);
        // Current layout of the link, in the same terms.
        let local = |p: usize| points.iter().position(|q| *q == p);
        let cur: Option<Vec<LCurve>> = curves
            .iter()
            .map(|ci| match self.curves.get(*ci).map(|c| &c.kind) {
                Some(CurveKind::Line { a, b }) => Some(LCurve::Line(local(*a)?, local(*b)?)),
                Some(CurveKind::Circle { c, r }) => Some(LCurve::Circle(local(*c)?, *r)),
                Some(CurveKind::Arc { c, a, b }) => Some(LCurve::Arc(local(*c)?, local(*a)?, local(*b)?)),
                Some(CurveKind::Conic { a, b, apex, rho }) => Some(LCurve::Conic(local(*a)?, local(*b)?, local(*apex)?, *rho)),
                Some(CurveKind::Ellipse { c, m, r }) => Some(LCurve::Ellipse(local(*c)?, local(*m)?, *r)),
                Some(CurveKind::Spline { pts, control, degree, periodic }) => {
                    let (a, b) = (local(*pts.first()?)?, local(*pts.last()?)?);
                    let inner: Vec<usize> =
                        pts.get(1..pts.len().saturating_sub(1)).unwrap_or_default().iter().map(|q| local(*q)).collect::<Option<_>>()?;
                    let f = inner.first().copied().unwrap_or(0);
                    // Interior points were made one after another.
                    if inner.iter().enumerate().any(|(k, x)| *x != f + k) {
                        return None;
                    }
                    Some(LCurve::Spline(a, b, f, inner.len(), *control, *degree, *periodic))
                }
                _ => None,
            })
            .collect();
        // The new layout numbers points in first-use order: curves first, then lone points,
        // exactly as `build_link` creates them.
        let mut order: Vec<usize> = Vec::new();
        let mut visit = |i: usize| {
            if !order.contains(&i) {
                order.push(i);
            }
        };
        for c in &lay.curves {
            match *c {
                LCurve::Line(a, b) => {
                    visit(a);
                    visit(b);
                }
                LCurve::Circle(c, _) => visit(c),
                LCurve::Arc(c, a, b) => {
                    visit(c);
                    visit(a);
                    visit(b);
                }
                LCurve::Conic(a, b, x, _) => {
                    visit(a);
                    visit(b);
                    visit(x);
                }
                LCurve::Ellipse(c, m, _) => {
                    visit(c);
                    visit(m);
                }
                LCurve::Spline(a, b, f, n, ..) => {
                    visit(a);
                    for i in f..f + n {
                        visit(i);
                    }
                    visit(b);
                }
            }
        }
        for i in &lay.lone {
            visit(*i);
        }
        let renum = |i: usize| order.iter().position(|q| *q == i).unwrap_or(usize::MAX);
        let new: Vec<LCurve> = lay
            .curves
            .iter()
            .map(|c| match *c {
                LCurve::Line(a, b) => LCurve::Line(renum(a), renum(b)),
                LCurve::Circle(c, r) => LCurve::Circle(renum(c), r),
                LCurve::Arc(c, a, b) => LCurve::Arc(renum(c), renum(a), renum(b)),
                LCurve::Conic(a, b, x, r) => LCurve::Conic(renum(a), renum(b), renum(x), r),
                LCurve::Ellipse(c, m, r) => LCurve::Ellipse(renum(c), renum(m), r),
                LCurve::Spline(a, b, f, n, ct, d, periodic) => LCurve::Spline(renum(a), renum(b), renum(f), n, ct, d, periodic),
            })
            .collect();
        let same = match &cur {
            Some(cur) => {
                cur.len() == new.len()
                    && order.len() == points.len()
                    && cur.iter().zip(&new).all(|(a, b)| {
                        same_shape(a, b)
                            && match (a, b) {
                                (LCurve::Line(a0, a1), LCurve::Line(b0, b1)) => a0 == b0 && a1 == b1,
                                (LCurve::Circle(a0, _), LCurve::Circle(b0, _)) => a0 == b0,
                                (LCurve::Arc(a0, a1, a2), LCurve::Arc(b0, b1, b2)) => a0 == b0 && a1 == b1 && a2 == b2,
                                (LCurve::Conic(a0, a1, a2, _), LCurve::Conic(b0, b1, b2, _)) => a0 == b0 && a1 == b1 && a2 == b2,
                                (LCurve::Ellipse(a0, a1, _), LCurve::Ellipse(b0, b1, _)) => a0 == b0 && a1 == b1,
                                (LCurve::Spline(a0, a1, a2, a3, a4, a5, a6), LCurve::Spline(b0, b1, b2, b3, b4, b5, b6)) => {
                                    a0 == b0 && a1 == b1 && (a2 == b2 || *a3 == 0) && a3 == b3 && a4 == b4 && a5 == b5 && a6 == b6
                                }
                                _ => false,
                            }
                    })
            }
            None => false,
        };
        if let Some(l) = self.links.iter_mut().find(|l| l.id == id) {
            l.lost = false;
        }
        // The same geometry walked in another order or direction (e.g. a face loop re-read after
        // a rollback) is not a change: keep the entities, and the constraints on them.
        if let Some(cur) = &cur {
            let pos = |pi: usize| self.points.get(pi).map(|p| p.pos);
            let old: Option<Vec<(LCurve, Vec<Vec2>)>> = cur
                .iter()
                .map(|c| curve_points(c).into_iter().map(|li| points.get(li).and_then(|pi| pos(*pi))).collect::<Option<Vec<_>>>().map(|ps| (*c, ps)))
                .collect();
            let fresh: Option<Vec<(LCurve, Vec<Vec2>)>> = lay
                .curves
                .iter()
                .map(|c| curve_points(c).into_iter().map(|li| lay.pts.get(li).copied()).collect::<Option<Vec<_>>>().map(|ps| (*c, ps)))
                .collect();
            let lone_old: Vec<Vec2> = points.iter().filter_map(|pi| pos(*pi)).collect();
            if let (Some(old), Some(fresh)) = (old, fresh)
                && lay.lone.is_empty()
                && lone_old.len() == points.len()
                && (points.len() == lay.pts.len() && same_geometry(&old, &fresh) || only_stations_gone(&old, &fresh))
            {
                return Ok(false);
            }
        }
        // A resized face can enumerate the same boundary in another order. Match the
        // shared-point graph in normalized sketch space before discarding its identity.
        if let Some(cur) = &cur
            && points.len() == lay.pts.len()
            && cur.len() == lay.curves.len()
            && points.len() <= 512
            && cur.len() <= 512
        {
            let old: Vec<Vec2> = points.iter().filter_map(|i| self.point(*i)).collect();
            let normalized = |ps: &[Vec2]| -> Vec<Vec2> {
                let b: Aabb2 = ps.iter().copied().collect();
                ps.iter().map(|p| b.relative(*p)).collect()
            };
            let loop_roles_required = self.link(id).is_some_and(|l| matches!(l.source, LinkSource::Face { .. }))
                && cur.iter().chain(&lay.curves).all(|c| matches!(c, LCurve::Line(..) | LCurve::Arc(..) | LCurve::Circle(..)))
                && lay.lone.is_empty()
                && !flat(cur, &old)
                && !flat(&lay.curves, &lay.pts);
            let loop_roles = loop_roles_required.then(|| loops::roles(cur, &old, &lay.curves, &lay.pts)).flatten();
            if loop_roles_required && loop_roles.is_none() {
                self.set_link_lost(id, true);
                return Err(SketchError::Invalid("projected face boundary identity is ambiguous; last geometry retained".into()));
            }
            let (a, b) = match &loop_roles {
                Some(roles) => (roles.old_normal.clone(), roles.fresh_normal.clone()),
                None => (normalized(&old), normalized(&lay.pts)),
            };
            let (old_roles, fresh_roles) = (point_roles(cur, old.len()), point_roles(&lay.curves, lay.pts.len()));
            let mut mapping = Vec::new();
            for (old_i, p) in a.iter().enumerate() {
                let mut candidates: Vec<(usize, f64)> = b
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        old_roles.get(old_i) == fresh_roles.get(*i)
                            && loop_roles.as_ref().is_none_or(|roles| {
                                roles.old.get(old_i) == roles.fresh.get(*i)
                                    && roles.old_lines.get(old_i).zip(roles.fresh_lines.get(*i)).is_some_and(|(a, b)| loops::same_lines(a, b))
                            })
                    })
                    .map(|(i, q)| (i, p.dist(*q)))
                    .collect();
                candidates.sort_by(|x, y| x.1.total_cmp(&y.1));
                if let Some((i, d)) = candidates.first()
                    && *d < 0.1
                    && candidates.get(1).is_none_or(|(_, second)| *d < *second * 0.5)
                    && !mapping.contains(i)
                {
                    mapping.push(*i);
                } else {
                    break;
                }
            }
            let mut used = Vec::new();
            let mut curve_map = Vec::new();
            if mapping.len() == points.len() {
                for (curve_i, c) in cur.iter().enumerate() {
                    let mapped: Option<Vec<usize>> = curve_points(c).iter().map(|i| mapping.get(*i).copied()).collect();
                    let candidates: Vec<usize> = lay
                        .curves
                        .iter()
                        .enumerate()
                        .filter_map(|(i, fresh)| {
                            if !same_shape(c, fresh) {
                                return None;
                            }
                            let fresh_pts = curve_points(fresh);
                            mapped
                                .as_ref()
                                .is_some_and(|p| {
                                    *p == fresh_pts || matches!(c, LCurve::Line(..)) && p.iter().rev().copied().collect::<Vec<_>>() == fresh_pts
                                })
                                .then_some(i)
                        })
                        .collect();
                    let found = if candidates.len() == 1 {
                        candidates.first().copied()
                    } else if let LCurve::Circle(center, _) = c {
                        // Concentric circular boundaries share a point graph. Their radius
                        // order identifies the inner/outer rings through an upstream resize.
                        let mut previous: Vec<(usize, f64)> = cur
                            .iter()
                            .enumerate()
                            .filter_map(|(i, x)| match x {
                                LCurve::Circle(p, r) if p == center => Some((i, *r)),
                                _ => None,
                            })
                            .collect();
                        let mut incoming: Vec<(usize, f64)> = candidates
                            .iter()
                            .filter_map(|i| match lay.curves.get(*i) {
                                Some(LCurve::Circle(_, r)) => Some((*i, *r)),
                                _ => None,
                            })
                            .collect();
                        previous.sort_by(|a, b| a.1.total_cmp(&b.1));
                        incoming.sort_by(|a, b| a.1.total_cmp(&b.1));
                        let distinct = |xs: &[(usize, f64)]| {
                            xs.windows(2).all(|w| matches!(w, [a, b] if b.1 - a.1 > MERGE_TOL * (1.0 + a.1.abs().max(b.1.abs()))))
                        };
                        if previous.len() == incoming.len() && distinct(&previous) && distinct(&incoming) {
                            previous.iter().position(|(i, _)| *i == curve_i).and_then(|rank| incoming.get(rank).map(|(i, _)| *i))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let found = found.filter(|i| !used.contains(i));
                    if let Some(i) = found {
                        used.push(i);
                        curve_map.push(i);
                    } else {
                        break;
                    }
                }
            }
            if curve_map.len() == cur.len() && mapping.len() == points.len() {
                for (pi, li) in points.iter().zip(mapping) {
                    if let (Some(p), Some(q)) = (self.points.get_mut(*pi), lay.pts.get(li)) {
                        p.pos = *q;
                    }
                }
                for (ci, li) in curves.iter().zip(curve_map) {
                    if let (Some(Curve { kind: CurveKind::Circle { r, .. }, .. }), Some(LCurve::Circle(_, value))) =
                        (self.curves.get_mut(*ci), lay.curves.get(li))
                    {
                        *r = *value;
                    }
                    if let (Some(Curve { kind: CurveKind::Conic { rho, .. }, .. }), Some(LCurve::Conic(.., value))) =
                        (self.curves.get_mut(*ci), lay.curves.get(li))
                    {
                        *rho = *value;
                    }
                    if let (Some(Curve { kind: CurveKind::Ellipse { r, .. }, .. }), Some(LCurve::Ellipse(.., value))) =
                        (self.curves.get_mut(*ci), lay.curves.get(li))
                    {
                        *r = *value;
                    }
                }
                return Ok(false);
            }
        }
        // Equal topology alone does not identify a face boundary: an outer rim and a
        // hole can have identical point graphs. Never move saved curve ids between them
        // when the geometric match is ambiguous; the document reports a lost link.
        if same && self.link(id).is_some_and(|l| matches!(l.source, LinkSource::Face { .. })) {
            self.set_link_lost(id, true);
            return Err(SketchError::Invalid("projected face boundary identity is ambiguous; last geometry retained".into()));
        }
        if same {
            for (k, pi) in points.iter().enumerate() {
                let p = order.get(k).and_then(|li| lay.pts.get(*li)).copied();
                if let (Some(p), Some(sp)) = (p, self.points.get_mut(*pi)) {
                    sp.pos = p;
                }
            }
            for (ci, c) in curves.iter().zip(&new) {
                if let (LCurve::Circle(_, r), Some(Curve { kind: CurveKind::Circle { r: slot, .. }, .. })) = (c, self.curves.get_mut(*ci)) {
                    *slot = *r;
                }
                if let (LCurve::Conic(.., r), Some(Curve { kind: CurveKind::Conic { rho, .. }, .. })) = (c, self.curves.get_mut(*ci)) {
                    *rho = *r;
                }
                if let (LCurve::Ellipse(.., r), Some(Curve { kind: CurveKind::Ellipse { r: slot, .. }, .. })) = (c, self.curves.get_mut(*ci)) {
                    *slot = *r;
                }
            }
            return Ok(false);
        }
        // Rebuild: new entities, and what the sketch held on the old ones moves to them.
        self.rebuild_link(id, &lay, &points, &curves, cur.as_deref())?;
        Ok(true)
    }

    /// Where this sketch's points were in `stored`, the same sketch before its links were
    /// moved to where their sources are now ([`Sketch::update_link`]): point by point, as
    /// [`crate::solve_after_edit`] takes them. Points are matched by id (a rebuilt link has
    /// new ones); a point `stored` does not have is where it is now. `None` when none moved.
    pub fn positions_in(&self, stored: &Sketch) -> Option<Vec<Vec2>> {
        let aligned = self.points.len() == stored.points.len() && self.points.iter().zip(&stored.points).all(|(p, q)| p.id == q.id);
        let was: Vec<Vec2> = if aligned {
            stored.points.iter().map(|p| p.pos).collect()
        } else {
            let by_id: std::collections::BTreeMap<&str, Vec2> = stored.points.iter().map(|p| (p.id.as_str(), p.pos)).collect();
            self.points.iter().map(|p| by_id.get(p.id.as_str()).copied().unwrap_or(p.pos)).collect()
        };
        self.points.iter().zip(&was).any(|(p, w)| p.pos != *w).then_some(was)
    }

    /// Mark a link as lost (its reference was not found); its geometry stays as it was.
    pub fn set_link_lost(&mut self, id: &str, lost: bool) {
        if let Some(l) = self.links.iter_mut().find(|l| l.id == id) {
            l.lost = lost;
        }
    }

    /// Store an updated reference (e.g. the point on an edge after an upstream edit).
    pub fn set_link_source(&mut self, id: &str, source: LinkSource) {
        if let Some(l) = self.links.iter_mut().find(|l| l.id == id) {
            l.source = source;
        }
    }

    /// Turn linked geometry into normal sketch geometry: the link goes, the curves and points
    /// stay where they are and become free.
    pub fn break_link(&mut self, id: &str) -> Result<()> {
        if self.link(id).is_none() {
            return Err(SketchError::Unknown(id.into()));
        }
        for c in &mut self.curves {
            if c.link.as_deref() == Some(id) {
                c.link = None;
            }
        }
        for p in &mut self.points {
            if p.link.as_deref() == Some(id) {
                p.link = None;
                p.fixed = false;
            }
        }
        for w in &mut self.wires {
            if w.link.as_deref() == Some(id) {
                w.link = None;
            }
        }
        self.links.retain(|l| l.id != id);
        Ok(())
    }

    /// Remove links that no longer own any curve or point.
    pub(crate) fn prune_links(&mut self) {
        if self.links.is_empty() {
            return;
        }
        let used = |id: &str, sk: &Sketch| {
            sk.curves.iter().any(|c| c.link.as_deref() == Some(id))
                || sk.points.iter().any(|p| p.link.as_deref() == Some(id))
                || sk.wires.iter().any(|w| w.link.as_deref() == Some(id))
        };
        let keep: Vec<bool> = self.links.iter().map(|l| used(&l.id, self)).collect();
        let mut k = keep.iter();
        self.links.retain(|_| k.next().copied().unwrap_or(true));
    }
}

#[cfg(test)]
#[path = "link_identity_tests.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "link_side_tests.rs"]
mod side_tests;
