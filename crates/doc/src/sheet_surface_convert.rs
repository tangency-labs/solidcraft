//! Convert to Sheet Metal (#6748): a body's faces as a sheet of panels, rolls
//! among them, laid out flat by `solidcraft-develop`. No face is fixed: every face of the sheet
//! is a panel like any other, and the flat is drawn in the largest one's plane.
//!
//! The sheet is every face of a solid, or of one side of a shell or a plate (the side its largest
//! face is on, reached across every edge but a rim's), less the faces the user skips. A cylinder
//! or a cone is a roll that keeps its modeled radius, joined by seams where the sheet runs on
//! smoothly. Every sharp edge between two faces of the sheet is cut open, half the rule's gap
//! from the plane halfway between the two faces as `sheet.convert` cuts it, unless the user picks
//! it as a bend. A ring of bends and seams is refused, naming its edges. A solid's material lies
//! inside its faces, or outside them; a shell or a plate keeps its own.
//!
//! No panel comes first, and nothing depends on which way a joint runs. Two bends standing on one
//! panel and meeting at a corner of it end where their tangent lines on it cross, with the
//! rule's corner relief: a notch; a round or a square relief, which belongs to the corner and is
//! cut from every piece of the flat it reaches; or a tear, the bends' ends running tight against
//! the plane of the rip that parts their other panels. A bend that a rip cuts beside ends where
//! its metal reaches the rip's plane, and a panel that runs on past a bend's end gets the rule's
//! bend relief there. A wall standing on part of another panel's edge bends only where it
//! stands.
//!
//! Everything is read from the body's B-rep record, exactly: each face's surface and loops, each
//! edge's curve. A shell's or a plate's other side is, of the faces level with each face and
//! facing back at it, the one it meets first over the most of its area; what a hole of a face
//! opens onto (a hole through, a recess, a boss) is found by following the faces round it. A roll's edges are read into its
//! chart at points, as lines where they run straight there and splines through the points
//! elsewhere. A roll that closes round its axis opens along its own seam.
//!
//! Not converted yet: a free-form face (not developed from its spline yet), a sphere or a torus
//! (neither is developable), a bend on the edge of a roll, and a shell's face that meets the
//! sheet at a sharp edge but is not a plane (it is left out).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use solidcraft_develop::{
    Anchor, Bend, Corner, CornerRelief, Datum, Development, Fold, Joint, JointKind, Lap, Panel, Relief, Rigid2, Rigid3, Rip, Sheet, Surface, Tail,
    ZoneEnds, curve_through, develop, fold,
};
use solidcraft_geom::brep::Loop as Bounds;
use solidcraft_geom::brep::query::Query;
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::{Body, FaceSurface, face_surface};

use crate::sheet::FlatWork;

use crate::sheet::ReliefShape;
use crate::sheet_surface::Piece;
use crate::{DocError, ModelBody, Result};

/// Normals closer than this (radians) meet smoothly: the sheet runs on across their edge.
const SMOOTH: f64 = 1e-6;

/// The rule's values Convert works with.
#[derive(Clone, Copy, Debug)]
pub struct ConvertRule {
    pub thickness: f64,
    pub k_factor: f64,
    pub bend_radius: f64,
    /// What a rip leaves between the panels it cuts apart.
    pub gap: f64,
    /// What is cut where two bends on one base meet at a corner of it, and how big.
    pub corner_relief: ReliefShape,
    pub corner_relief_size: f64,
    /// The bend relief where a bend ends beside more of its base: how wide along the bend, and
    /// how far inside the base's tangent line it is cut from.
    pub relief_width: f64,
    pub relief_depth: f64,
}

/// What a sheet's bends end with (see [`settle`]): the gap a rip leaves between its panels, the
/// relief at a corner where two bends meet and its size, and the bend relief where a bend ends
/// beside more of its panel (how wide along the bend, and how far inside the panel's tangent
/// line it is cut from).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Reliefs {
    pub gap: f64,
    pub corner: CornerRelief,
    pub corner_size: f64,
    pub width: f64,
    pub depth: f64,
}

impl Reliefs {
    pub fn of(rule: &ConvertRule) -> Reliefs {
        Reliefs {
            gap: rule.gap,
            corner: corner_relief(rule.corner_relief),
            corner_size: rule.corner_relief_size,
            width: rule.relief_width,
            depth: rule.relief_depth,
        }
    }
}

/// A rule's corner relief as a sheet's corner has it.
pub fn corner_relief(shape: ReliefShape) -> CornerRelief {
    match shape {
        ReliefShape::Notch => CornerRelief::Notch,
        ReliefShape::Tear => CornerRelief::Tear,
        ReliefShape::Round => CornerRelief::Round,
        ReliefShape::Rectangular => CornerRelief::Square,
    }
}

/// A rip whose edge leaves a bend's end this near the bend's own way (the cosine between them)
/// carries on along the bend: both ways are exact, to a rounding.
const ALONGSIDE: f64 = 1.0 - 1e-9;

/// Relief sizes below this (mm) are not cut, as on a flanged sheet.
const MIN_RELIEF: f64 = 0.01;

/// A body converted to sheet metal: its sheet of panels and joints and their development, the body's face each panel
/// is and where that face puts the panel's frame in space, a point on the edge of each joint,
/// the sheet's thickness (a shell's own), and why any relief the rule asks for was not cut.
#[derive(Clone, Debug)]
pub struct SurfaceConversion {
    pub sheet: Sheet,
    pub development: Development,
    pub faces: Vec<usize>,
    pub places: Vec<Rigid3>,
    pub edges: Vec<Vec3>,
    pub thickness: f64,
    pub notes: Vec<String>,
    pub reliefs: Reliefs,
    /// The sheet keeps the body's own thickness (a shell's or a plate's), so its faces' recesses
    /// are the sheet's.
    pub own_thickness: bool,
    /// The flat's pieces, cut once for both the flat and the folded solid.
    pieces: std::sync::OnceLock<Vec<(Piece, Region2)>>,
}

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

fn at(p: Vec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)
}

/// What a face of the body is as a panel, and how its points map to the surface's parameters,
/// which run with `u x v` toward the material (opposite the face's outward normal).
#[derive(Clone, Copy, Debug)]
enum Shape {
    Plane { plane: Plane, out: Vec3 },
    Cylinder { origin: Vec3, axis: Vec3, e1: Vec3, e2: Vec3, radius: f64, inward: bool },
    Cone { apex: Vec3, axis: Vec3, e1: Vec3, e2: Vec3, half: f64, inward: bool },
}

impl Shape {
    /// Face `f`'s shape. A plane's outward normal, and whether a cylinder's or cone's material
    /// lies toward its axis, are the face's own side as its shell uses it (from the record, at a
    /// point of the face within `accept` mm); `flip` puts the material outside the body instead.
    fn of(b: &Body, q: &Query, f: usize, flip: bool, accept: f64) -> std::result::Result<Shape, &'static str> {
        let frame = |axis: Vec3| {
            let pick = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
            let e1 = (pick - axis * pick.dot(axis)).normalized().unwrap_or(Vec3::X);
            (e1, axis.cross(e1))
        };
        let side = || -> Option<(Vec3, Vec3)> {
            let p = face_point(q, f)?;
            Some((p, q.face_normal(f, q.face_uv(f, p, accept)?)?))
        };
        match face_surface(b, f) {
            Some(FaceSurface::Plane { normal, point }) => {
                let normal: Vec3 = *normal;
                let normal = side().map_or(normal, |(_, n)| if n.dot(normal) < 0.0 { -normal } else { normal });
                let out = if flip { -normal } else { normal };
                Plane::from_normal(point, -out).map(|plane| Shape::Plane { plane, out }).ok_or("a face of the body has no plane")
            }
            Some(FaceSurface::Cylinder { origin, axis, radius }) => {
                let axis = *axis;
                let (e1, e2) = frame(axis);
                let (p, n) = side().ok_or("a face of the body has no side")?;
                let inward = (n.dot((p - origin) - axis * (p - origin).dot(axis)) > 0.0) != flip;
                Ok(Shape::Cylinder { origin, axis, e1, e2, radius, inward })
            }
            Some(FaceSurface::Cone { apex, axis, half_angle }) => {
                let axis = *axis;
                let (e1, e2) = frame(axis);
                let (p, n) = side().ok_or("a face of the body has no side")?;
                let inward = (n.dot((p - apex) - axis * (p - apex).dot(axis)) > 0.0) != flip;
                Ok(Shape::Cone { apex, axis, e1, e2, half: half_angle, inward })
            }
            Some(FaceSurface::Sphere { .. } | FaceSurface::Torus { .. }) => Err("a sphere or a torus is not developable"),
            None => Err("not supported yet: a free-form face"),
        }
    }

    fn surface(&self) -> Surface {
        match *self {
            Shape::Plane { .. } => Surface::Plane,
            // The panel's radius is the face's; with the material outward the face is the inside.
            Shape::Cylinder { radius, inward, .. } => Surface::Cylinder { radius, material_inward: inward },
            Shape::Cone { half, inward, .. } => Surface::Cone { half_angle: half, material_inward: inward },
        }
    }

    /// The parameters of a point of the face, its angle not yet unwrapped.
    fn params(&self, p: Vec3) -> Vec2 {
        let turn = |w: Vec3, e1: Vec3, e2: Vec3, inward: bool| w.dot(e2).atan2(w.dot(e1)) * if inward { 1.0 } else { -1.0 };
        match *self {
            Shape::Plane { plane, .. } => plane.to_local(p),
            Shape::Cylinder { origin, axis, e1, e2, inward, .. } => Vec2::new((p - origin).dot(axis), turn(p - origin, e1, e2, inward)),
            Shape::Cone { apex, e1, e2, inward, .. } => Vec2::new((p - apex).len(), turn(p - apex, e1, e2, inward)),
        }
    }

    fn angular(&self) -> bool {
        !matches!(self, Shape::Plane { .. })
    }

    /// Where the face puts its panel's frame (see `solidcraft_develop::fold`): a plane's
    /// parameters are its frame's x and y, toward the material its z; a cylinder's or a cone's
    /// axis is its frame's z, its angle measured from x.
    fn frame(&self) -> Option<Rigid3> {
        match *self {
            Shape::Plane { plane, .. } => Rigid3::frame(plane.origin, plane.x, plane.y),
            Shape::Cylinder { origin, e1, e2, .. } => Rigid3::frame(origin, e1, e2),
            Shape::Cone { apex, e1, e2, .. } => Rigid3::frame(apex, e1, e2),
        }
    }

    /// The outward unit normal at a point of the face.
    fn out(&self, p: Vec3) -> Vec3 {
        let side = |inward: bool| if inward { 1.0 } else { -1.0 };
        match *self {
            Shape::Plane { out, .. } => out,
            Shape::Cylinder { origin, axis, inward, .. } => {
                ((p - origin) - axis * (p - origin).dot(axis)).normalized().unwrap_or(axis) * side(inward)
            }
            Shape::Cone { apex, axis, half, inward, .. } => {
                let c = ((p - apex) - axis * (p - apex).dot(axis)).normalized().unwrap_or(axis);
                (c * half.cos() - axis * half.sin()) * side(inward)
            }
        }
    }
}

/// One use of an edge by a face's loop: the edge's parameters and points, run as the loop runs,
/// and their places in the face's chart, the angle round a roll unwrapped as the loop runs.
struct UseChart {
    edge: usize,
    forward: bool,
    ts: Vec<f64>,
    points: Vec<Vec3>,
    chart: Vec<Vec2>,
}

impl UseChart {
    /// Points, chart places and parameters in the edge's own order, from its start to its end.
    fn along(&self) -> (Vec<Vec3>, Vec<Vec2>, Vec<f64>) {
        let (mut p, mut c, mut t) = (self.points.clone(), self.chart.clone(), self.ts.clone());
        if !self.forward {
            p.reverse();
            c.reverse();
            t.reverse();
        }
        (p, c, t)
    }
}

/// Face `f` as a panel in `shape`'s chart, from the body's record: its region, and each use of
/// an edge by its loops. A plane's edges are its own exactly (lines, arcs, the spans of free-form
/// curves); a roll's are straight in its parameters where the edge runs along a ruling or round
/// the axis, and elsewhere a cubic spline through the edge's points read within `sag` mm.
fn face_chart(b: &Body, q: &Query, shape: &Shape, f: usize, sag: f64, tol: f64) -> std::result::Result<(Region2, Vec<UseChart>), String> {
    let tau = std::f64::consts::TAU;
    let face = q.record().faces.get(f).ok_or("a face the body does not have")?;
    let mut loops: Vec<(Loop2, Vec<UseChart>)> = Vec::new();
    for l in &face.loops {
        let Bounds::Edges { edges } = l else { continue };
        let (mut segs, mut uses): (Vec<Seg2>, Vec<UseChart>) = (Vec::new(), Vec::new());
        for oe in edges {
            let e = oe.edge as usize;
            let mut read = q.edge_points(e, sag).ok_or("an edge of the face can't be read")?;
            if !oe.forward {
                read.reverse();
            }
            let (ts, points): (Vec<f64>, Vec<Vec3>) = read.into_iter().unzip();
            let mut chart: Vec<Vec2> = points.iter().map(|p| shape.params(*p)).collect();
            if shape.angular() {
                // Unwrapped along the edge, then onto the turn the loop is on so far.
                for k in 1..chart.len() {
                    let (Some(prev), Some(cur)) = (chart.get(k - 1).copied(), chart.get_mut(k)) else { continue };
                    cur.y += ((prev.y - cur.y) / tau).round() * tau;
                }
                let so_far = uses.last().and_then(|u: &UseChart| u.chart.last()).map(|p| p.y);
                if let (Some(r), Some(first)) = (so_far, chart.first().map(|p| p.y)) {
                    let shift = ((r - first) / tau).round() * tau;
                    for c in &mut chart {
                        c.y += shift;
                    }
                }
            }
            let exact = exact_edge(b, shape, e, &chart, tol);
            segs.extend(if exact.is_empty() { curve_through(&chart) } else { exact });
            uses.push(UseChart { edge: e, forward: oe.forward, ts, points, chart });
        }
        let ends = uses.first().and_then(|u| u.chart.first()).zip(uses.last().and_then(|u| u.chart.last()));
        let Some((first, last)) = ends else { continue };
        let closes = if shape.angular() {
            (first.x - last.x).abs() <= tol * 10.0 && (first.y - last.y).abs() <= 1e-6
        } else {
            first.dist(*last) <= tol * 10.0
        };
        if !closes {
            return Err("an outline of the face does not close round it".into());
        }
        loops.push((Loop2 { segs }, uses));
    }
    let area = |l: &Loop2| l.signed_area().abs();
    loops.sort_by(|x, y| area(&y.0).total_cmp(&area(&x.0)));
    let mut it = loops.into_iter();
    let Some((outer, mut uses)) = it.next() else { return Err("a face of the body has no edges".into()) };
    let mut holes = Vec::new();
    for (mut hole, mut hole_uses) in it {
        if shape.angular() {
            // A hole is unwrapped on its own: bring it to the turn the outline is on.
            let mean = |l: &Loop2| l.segs.iter().map(|s| s.start().y).sum::<f64>() / l.segs.len().max(1) as f64;
            let shift = ((mean(&outer) - mean(&hole)) / tau).round() * tau;
            if shift != 0.0 {
                hole = solidcraft_develop::Rigid2 { cos: 1.0, sin: 0.0, shift: Vec2::new(0.0, shift) }.loop2(&hole);
                for u in &mut hole_uses {
                    for c in &mut u.chart {
                        c.y += shift;
                    }
                }
            }
        }
        holes.push(hole);
        uses.extend(hole_uses);
    }
    Ok((Region2 { outer: outer.ccw(), holes: holes.iter().map(|h| h.ccw().reversed()).collect() }, uses))
}

/// The normal of the plane halfway between panels `x` and `y` through the edge between them,
/// toward `x`'s side. `e` runs along the edge with `x` on its left seen from outside; normals are
/// outward.
fn rip_plane(nx: Vec3, ny: Vec3, e: Vec3) -> Option<Vec3> {
    let (wx, wy) = (nx.cross(e), e.cross(ny));
    (wx - wy).normalized().filter(|m| wx.dot(*m) > 1e-9)
}

/// A rip as the corners see it: its joint, its edge's two ends, and at each the plane halfway
/// between its faces there (normal toward its `a` face).
/// The edge's way at each end, along the edge from its start to its end, comes last.
type RipEnds = (usize, [Vec3; 2], [Option<Vec3>; 2], [Vec3; 2]);

/// Edge `e` of the body exactly in a planar face's parameters (lines, circular arcs, free-form
/// spans), run the way its points `path` there run; empty on a roll, and where the body's edge
/// doesn't end where the points do.
fn exact_edge(b: &Body, shape: &Shape, e: usize, path: &[Vec2], tol: f64) -> Vec<Seg2> {
    let Shape::Plane { plane, .. } = shape else { return Vec::new() };
    let (Some(first), Some(last)) = (path.first(), path.last()) else { return Vec::new() };
    let Some(Some(curve)) = b.planar_edges(plane, &[e]).into_iter().next() else { return Vec::new() };
    let Some((p, q)) = curve.first().zip(curve.last()).map(|(p, q)| (p.start(), q.end())) else { return Vec::new() };
    // A closed edge (a whole circle) ends where it starts: it runs the path's way where it
    // leaves its start toward the path's second point.
    let closed = first.dist(*last) <= tol;
    let leaves = path.get(1).zip(curve.first()).is_some_and(|(next, s)| s.start_tangent().dot(*next - *first) > 0.0);
    if p.dist(*first) <= tol && q.dist(*last) <= tol && (!closed || leaves) {
        curve
    } else if q.dist(*first) <= tol && p.dist(*last) <= tol {
        curve.iter().rev().map(Seg2::reversed).collect()
    } else {
        Vec::new()
    }
}

/// How far a bend is cut back square along its edge from the corner `v` so that none of its
/// metal comes nearer than half the gap to a rip's plane through `v` (normal `m`, toward the side
/// the bend keeps), as the rip cuts the panels beside it. `d` runs along the edge from `v`; `na`
/// and `nb` are the two panels' normals away from their material. The bend's metal is the ring
/// sector between radii `r` and `r + t` round its axis, which lies `r + t` inside both faces
/// where it turns toward the material (the faces are its outside) and `r` outside them where it
/// turns away (they are its inside). None where running along the edge leads no further from the
/// plane.
fn bend_cut(v: Vec3, d: Vec3, m: Vec3, normals: (Vec3, Vec3), toward: bool, r: f64, t: f64, gap: f64) -> Option<f64> {
    let away = d.dot(m);
    if away <= 1e-9 {
        return None;
    }
    let (axis, ua, across, theta) = bend_section(v, normals, toward, r, t)?;
    let (cu, cv) = (ua.dot(m), across.dot(m));
    let lowest = (-cv).atan2(-cu).rem_euclid(std::f64::consts::TAU);
    let turns = [0.0, theta, if lowest <= theta { lowest } else { 0.0 }];
    let nearest =
        turns.iter().flat_map(|a| [r, r + t].map(|radius| (axis - v).dot(m) + radius * (a.cos() * cu + a.sin() * cv))).fold(f64::INFINITY, f64::min);
    Some(((gap / 2.0 - nearest) / away).max(0.0))
}

/// A bend as its ends are worked out: its joint, its edge's ends (its `a` panel's material on the
/// left running from the first), its panels' normals `(a, b)` away from their material, whether
/// it turns toward the material, its edge's length, its anchors, how far each anchor lies from
/// its tangent line, and its inside radius.
struct BendEdge {
    joint: usize,
    ends: [Vec3; 2],
    normals: (Vec3, Vec3),
    toward: bool,
    len: f64,
    a: Anchor,
    b: Anchor,
    setback: f64,
    radius: f64,
}

impl BendEdge {
    /// The bend's anchor on panel `p`, and the side of it the panel's material lies on (1 on
    /// the left, the `a` anchor's; -1 on the right, the `b` one's).
    fn on(&self, p: usize) -> Option<(Anchor, f64)> {
        if self.a.panel == p {
            Some((self.a, 1.0))
        } else if self.b.panel == p {
            Some((self.b, -1.0))
        } else {
            None
        }
    }

    /// The bend's anchor on its panel other than `p`, and the side of it that panel's material
    /// lies on.
    fn other(&self, p: usize) -> Option<(Anchor, f64)> {
        self.on(if self.a.panel == p { self.b.panel } else { self.a.panel })
    }

    /// The bend's tangent line on its panel at anchor `at` (`side` as from [`BendEdge::on`]).
    fn tangent(&self, (at, side): (Anchor, f64)) -> (Vec2, Vec2) {
        (at.origin + at.dir.perp() * (side * self.setback), at.dir)
    }
}

/// Two bends meeting at a corner of a panel both stand on: each one's end there (the bend, and
/// 0 for its start or 1 for its end), the corner of their edges, the panel, and how far along
/// each bend their tangent lines on the panel cross. Neither panel round the corner comes first:
/// the bends end alike whichever way their joints run.
struct BendCorner {
    ends: [(usize, usize); 2],
    v: Vec3,
    panel: usize,
    at: [f64; 2],
}

/// Where a region's outline crosses a line (a point and a unit direction), as lengths along the
/// line from its point: the least and the greatest. `None` where it doesn't.
fn extent_across(r: &Region2, (p, u): (Vec2, Vec2)) -> Option<(f64, f64)> {
    let n = u.perp();
    let mut out: Option<(f64, f64)> = None;
    for l in std::iter::once(&r.outer).chain(&r.holes) {
        let pts: Vec<Vec2> = (&l.polyline(1e-4)).into_iter().copied().collect();
        for (k, a) in pts.iter().enumerate() {
            let Some(b) = pts.get((k + 1) % pts.len()) else { continue };
            let (sa, sb) = ((*a - p).dot(n), (*b - p).dot(n));
            if (sa > 0.0) != (sb > 0.0) {
                let along = (*a + (*b - *a) * (sa / (sa - sb)) - p).dot(u);
                out = Some(out.map_or((along, along), |(lo, hi)| (lo.min(along), hi.max(along))));
            }
        }
    }
    out
}

/// The point where two lines, each a point and a direction, cross.
fn crossing((p, u): (Vec2, Vec2), (q, w): (Vec2, Vec2)) -> Option<Vec2> {
    let cross = u.cross(w);
    (cross.abs() > 1e-12).then(|| p + u * ((q - p).cross(w) / cross))
}

/// The corners where two bends standing on one panel meet (see [`BendCorner`]).
fn bend_corners(bent: &[BendEdge], near: impl Fn(Vec3, Vec3) -> bool) -> Vec<BendCorner> {
    let ends = || bent.iter().enumerate().flat_map(|(i, b)| b.ends.iter().enumerate().map(move |(e, p)| (i, e, b, *p)));
    let shared = |x: &BendEdge, y: &BendEdge| [x.a.panel, x.b.panel].into_iter().find(|p| y.on(*p).is_some());
    let (mut out, mut used) = (Vec::new(), Vec::new());
    for (i, e, bi, v) in ends() {
        if used.contains(&(i, e)) {
            continue;
        }
        let other = ends().find(|(k, f, bk, p)| *k != i && !used.contains(&(*k, *f)) && near(*p, v) && shared(bi, bk).is_some());
        let Some((k, f, bk, _)) = other else { continue };
        let Some(panel) = shared(bi, bk) else { continue };
        let (Some(ai), Some(ak)) = (bi.on(panel), bk.on(panel)) else { continue };
        let into = |(at, _): (Anchor, f64), end: usize| if end == 0 { at.dir } else { -at.dir };
        let (di, dk) = (into(ai, e), into(ak, f));
        // A convex corner: each bend runs off into the panel's side of the other.
        if !(dk.dot(ai.0.dir.perp() * ai.1) > 1e-9 && di.dot(ak.0.dir.perp() * ak.1) > 1e-9) {
            continue;
        }
        let Some(c) = crossing(bi.tangent(ai), bk.tangent(ak)) else { continue };
        used.extend([(i, e), (k, f)]);
        out.push(BendCorner { ends: [(i, e), (k, f)], v, panel, at: [(c - ai.0.origin).dot(ai.0.dir), (c - ak.0.origin).dot(ak.0.dir)] });
    }
    out
}

/// A tear at corner `k`, where a rip parts the bends' other panels: each bend's ends there, on
/// its `a` and `b` sides, and the tear. On the panel the bends share, the bend ends where their
/// tangent lines cross; on its other panel, where that panel's material, cut back by the rip,
/// meets its tangent line; and between, it runs tight against the rip's plane. `None` at an open
/// corner, where no rip parts the two panels.
#[allow(clippy::too_many_arguments)]
fn tear(
    bent: &[BendEdge],
    joints: &[Joint],
    rips: &[RipEnds],
    places: &[Rigid3],
    k: &BendCorner,
    t: f64,
    near: impl Fn(Vec3, Vec3) -> bool,
) -> std::result::Result<Option<[(f64, f64, Relief); 2]>, String> {
    let [(i, _), (j, _)] = k.ends;
    let (Some(bi), Some(bj)) = (bent.get(i), bent.get(j)) else { return Ok(None) };
    let (Some(wi), Some(wj)) = (bi.other(k.panel), bj.other(k.panel)) else { return Ok(None) };
    let walls = [wi.0.panel, wj.0.panel];
    let rip = rips.iter().find_map(|(r, ends, planes, _)| {
        let jr = joints.get(*r)?;
        let pair = [jr.a.panel, jr.b.panel];
        let [wa, wb] = walls;
        let end = ends.iter().position(|p| near(*p, k.v))?;
        (pair == [wa, wb] || pair == [wb, wa]).then_some((jr, end, planes.get(end).copied().flatten()?))
    });
    let Some((joint, end, m)) = rip else { return Ok(None) };
    let JointKind::Rip(rip) = &joint.kind else { return Ok(None) };
    let at_end = |v: &[Vec2]| {
        if end == 0 { v.first().zip(v.get(1)).map(|(p, q)| (*p, *q - *p)) } else { v.last().zip(v.iter().rev().nth(1)).map(|(p, q)| (*p, *p - *q)) }
    };
    let turn = if end == 0 { rip.turns.first() } else { rip.turns.last() };
    let cut = rip.lap.cuts(turn.copied().unwrap_or_default(), t);
    let mut out = [(0.0, 0.0, Relief::None); 2];
    for ((slot, (b, wall)), corner_at) in out.iter_mut().zip([(bi, wi), (bj, wj)]).zip(k.at) {
        // The wall's edge at the corner, cut back into its material (on the left of the rip's
        // way on its `a` panel, on the right on its `b` one), and the bend's tangent line on the
        // wall.
        let (edge, back) = if joint.a.panel == wall.0.panel { (&rip.edges.0, cut.0) } else { (&rip.edges.1, -cut.1) };
        let Some((point, way)) = at_end(edge) else { return Err("a rip has no edge".into()) };
        let Some(way) = way.normalized() else { return Err("a rip's edge has no length".into()) };
        let Some(q) = crossing(b.tangent(wall), (point + way.perp() * back, way)) else {
            return Err("a bend runs along the rip".into());
        };
        let wall_at = (q - wall.0.origin).dot(wall.0.dir);
        // The rip's plane through the corner, toward the bend's own wall, in the bend's frame
        // at its `a` anchor.
        let own = if joint.a.panel == wall.0.panel { m } else { -m };
        let Some((normal, offset)) = anchor_plane(places, b.a, own, k.v) else { return Err("a bend's panel has no place".into()) };
        let (near_at, far_at) = if b.a.panel == k.panel { (corner_at, wall_at) } else { (wall_at, corner_at) };
        *slot = (near_at, far_at, Relief::Tear { normal, offset });
    }
    Ok(Some(out))
}

/// The plane through `v` with normal `m`, in a bend's frame at its `a` anchor `a` (`x` along the
/// joint, `y` toward the material, `z` toward the `b` panel): its normal there and its offset.
fn anchor_plane(places: &[Rigid3], a: Anchor, m: Vec3, v: Vec3) -> Option<(Vec3, f64)> {
    let place = places.get(a.panel)?;
    let (o, x, y) = (place.point(a.origin.extend(0.0)), place.vector(a.dir.extend(0.0)), place.z);
    let z = x.cross(y);
    Some((Vec3::new(m.dot(x), m.dot(y), m.dot(z)), (v - o).dot(m)))
}

/// A bend's section at the corner `v` of its edge: its axis there, the unit radius toward its
/// `a` face and the one square to it toward its `b` face, and how far it turns. `na` and `nb`
/// are the faces' normals away from their material. The bend's metal is the ring sector between
/// radii `r` and `r + t` round its axis, which lies `r + t` inside both faces where the bend
/// turns toward the material (the faces are its outside) and `r` outside them where it turns
/// away (they are its inside). None where the faces lie flat or fold onto each other.
fn bend_section(v: Vec3, (na, nb): (Vec3, Vec3), toward: bool, r: f64, t: f64) -> Option<(Vec3, Vec3, Vec3, f64)> {
    let cos = na.dot(nb).clamp(-1.0, 1.0);
    if 1.0 + cos <= 1e-9 {
        return None;
    }
    let depth = if toward { -(r + t) } else { r };
    let axis = v + (na + nb) * (depth / (1.0 + cos));
    let (ua, ub) = if toward { (na, nb) } else { (-na, -nb) };
    let across = (ub - ua * ub.dot(ua)).normalized()?;
    Some((axis, ua, across, cos.acos()))
}

/// Where face shape `g` lies against face shape `f` when it is level with it: a parallel plane,
/// or a roll on the same axis (a cone opening the same way at the same angle). How deep behind
/// `f` it lies, into `f`'s material (negative in front of it), and whether it faces the way `f`
/// does.
fn level_of(f: &Shape, g: &Shape, tol: f64) -> Option<(f64, bool)> {
    let side = |inward: bool| if inward { 1.0 } else { -1.0 };
    match (*f, *g) {
        (Shape::Plane { plane: pf, out: of }, Shape::Plane { plane: pg, out: og }) => {
            (of.cross(og).len() <= 1e-9).then(|| ((pf.origin - pg.origin).dot(of), of.dot(og) > 0.0))
        }
        (Shape::Cylinder { origin: a, axis: u, radius: r, inward: i, .. }, Shape::Cylinder { origin: c, axis: w, radius: k, inward: j, .. }) => {
            let off = (c - a) - u * (c - a).dot(u);
            (u.cross(w).len() <= 1e-9 && off.len() <= tol * 10.0).then(|| ((r - k) * side(i), i == j))
        }
        (Shape::Cone { apex: a, axis: u, half: h, inward: i, .. }, Shape::Cone { apex: c, axis: w, half: k, inward: j, .. }) => {
            let off = (c - a) - u * (c - a).dot(u);
            (u.dot(w) >= 1.0 - 1e-9 && (h - k).abs() <= 1e-9 && off.len() <= tol * 10.0).then(|| ((c - a).dot(u) * h.sin() * side(i), i == j))
        }
        _ => None,
    }
}

/// How to chart a face on shape `g`, level with `f` (see [`level_of`]), in `f`'s parameters, each
/// point where the normal through it meets `f`: the shape to chart it with, and the shift that
/// then puts it in `f`'s chart.
fn charted_on(f: &Shape, g: &Shape) -> Option<(Shape, Vec2)> {
    match (*f, *g) {
        (Shape::Plane { plane: pf, .. }, Shape::Plane { plane: pg, out }) => {
            let n = pf.normal();
            let plane = Plane::new(pf.origin + n * (pg.origin - pf.origin).dot(n), pf.x, pf.y)?;
            Some((Shape::Plane { plane, out }, Vec2::ZERO))
        }
        (Shape::Cylinder { origin, axis, e1, e2, inward, .. }, Shape::Cylinder { radius, .. }) => {
            Some((Shape::Cylinder { origin, axis, e1, e2, radius, inward }, Vec2::ZERO))
        }
        (Shape::Cone { apex: a, axis, e1, e2, half, inward }, Shape::Cone { apex: c, .. }) => {
            Some((Shape::Cone { apex: c, axis, e1, e2, half, inward }, Vec2::new((c - a).dot(axis) * half.cos(), 0.0)))
        }
        _ => None,
    }
}

/// What a loop of a shell's or a plate's face opens onto, followed across the faces round it to
/// the first faces level with the face: material standing on the face (a boss, at that face); a
/// floor facing the way the face does behind it (a recess: its walls and floors, a hole through
/// its floor or not); the face's other side, or a floor facing away (a hole through); or none
/// of these.
enum Opening {
    Raised(usize),
    Recess(BTreeSet<usize>),
    Through,
    Open,
}

/// What an edge between two faces of the sheet is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Seam,
    Bend,
    Rip,
}

/// The faces of a body that become the sheet.
#[derive(Clone, Copy, Debug)]
pub enum SheetFaces<'a> {
    /// Convert: every face of a solid, or of one side of a shell or a plate, less the faces at
    /// these points.
    AllBut(&'a [Vec3]),
    /// Thicken: the faces at these points, and every face a bend joins to them.
    Picked(&'a [Vec3]),
}

/// Converts `mb` to sheet metal from its `faces`, bending at the `bends` and cutting every other
/// sharp edge between two faces of the sheet open. A bend is picked on its edge, or, where it is
/// modelled with its radius, on its roll: the roll and the faces it runs smoothly into join the
/// sheet. A solid's material lies inside its faces, or outside them when `outward`. The rule's
/// values are read at the sheet's thickness: `rule(Some(t))` for a shell's or a plate's own,
/// `rule(None)` for a solid, which takes the rule's. Points are the body's own; what it says
/// gives them where `to_world` puts them.
pub fn convert_surfaces(
    mb: &ModelBody,
    rule: &dyn Fn(Option<f64>) -> Result<ConvertRule>,
    bends: &[Vec3],
    faces: SheetFaces,
    outward: bool,
    to_world: &crate::Mat,
) -> Result<SurfaceConversion> {
    let at = |p: Vec3| at(crate::apply_point(to_world, p));
    let b = &mb.body;
    if b.is_mesh() {
        return err("a mesh body has no faces to convert: make it a solid first");
    }
    let q = Query::new(b.record());
    let size = b.size();
    if !(size.is_finite() && size > 0.0) {
        return err("the body has no size");
    }
    let (tol, near) = ((size * 1e-6).max(1e-6), (size * 2e-3).max(1e-3));
    // How far an edge's curve may stray from the chord between two of the points it is read at.
    let sag = (size * 1e-4).max(1e-4);
    let (count, edge_count) = (q.record().faces.len(), q.record().edges.len());
    let middle_of = |e: usize| q.edge_interval(e).and_then(|[t0, t1]| q.edge_at(e, 0.5 * (t0 + t1))).map(|d| d.0);
    let on_edge = |p: Vec3, e: usize| q.edge_distance(e, p, sag).is_some_and(|d| d <= near);
    let picked = |picks: &[Vec3], e: usize| picks.iter().any(|p| on_edge(*p, e));
    let face_at = |p: Vec3, what: &str| -> Result<usize> {
        match q.faces_at(p, near).first() {
            Some(&(_, g)) => Ok(g),
            None => err(format!("no face of the body at the {what} point {}", at(p))),
        }
    };
    // A bend picked off every edge is one modelled with its radius, picked on its roll.
    let is_roll = |g: usize| matches!(face_surface(b, g), Some(FaceSurface::Cylinder { .. } | FaceSurface::Cone { .. }));
    let mut rolled = BTreeSet::new();
    for p in bends.iter().filter(|p| !(0..edge_count).any(|e| on_edge(**p, e))) {
        match q.faces_at(*p, near).first() {
            Some(&(_, g)) if is_roll(g) => {
                rolled.insert(g);
            }
            Some(_) => return err(format!("the bend at {} is on a face, not an edge: pick the edge it bends at", at(*p))),
            None => return err(format!("no edge of the body at the bend point {}", at(*p))),
        }
    }
    let (skip, chosen) = match faces {
        SheetFaces::AllBut(points) => (points, None),
        SheetFaces::Picked(points) => (&[][..], Some(points.iter().map(|p| face_at(*p, "picked face")).collect::<Result<BTreeSet<usize>>>()?)),
    };
    let mut skipped = BTreeSet::new();
    for p in skip {
        skipped.insert(face_at(*p, "skipped face")?);
    }
    // The two faces each edge is between (one face twice for a roll's own seam), and which faces
    // meet at an edge.
    let pair = |e: usize| match q.edge_faces(e).as_slice() {
        [x, y] => Some((*x, *y)),
        _ => None,
    };
    let meeting: BTreeSet<(usize, usize)> = (0..edge_count).filter_map(pair).flat_map(|(x, y)| [(x, y), (y, x)]).collect();
    let touches = |g: usize, h: usize| meeting.contains(&(g, h));

    // Each face as modelled, in its chart: a point inside it with its outward normal there, and
    // how far it spans.
    let modelled: Vec<Option<Shape>> = (0..count).map(|f| Shape::of(b, &q, f, false, tol).ok()).collect();
    let probe = |f: usize| modelled.get(f).copied().flatten();
    let charts: Vec<Option<(Region2, Vec<UseChart>)>> = (0..count).map(|f| probe(f).and_then(|s| face_chart(b, &q, &s, f, sag, tol).ok())).collect();
    let extent = |f: usize| -> f64 {
        let Some(Some((_, uses))) = charts.get(f) else { return 0.0 };
        let (big, small) =
            (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
        let (lo, hi) = uses.iter().flat_map(|u| &u.points).fold((big, small), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        if lo.x <= hi.x { lo.dist(hi) } else { 0.0 }
    };
    let cut = |a: &[Region2], b: &[Region2], op: solidcraft_kernel::BoolOp| solidcraft_kernel::planar_regions_boolean(a, b, op, tol).ok();
    let area = |r: &[Region2]| r.iter().map(Region2::area).sum::<f64>();
    // Face `g`, level with face `f` (see `level_of`), in `f`'s parameters: each of its points
    // where the normal through it meets `f`. A roll's chart is laid a turn either way too, where
    // that covers more of `f`.
    let projected = |f: usize, g: usize| -> Option<Region2> {
        let (sf, sg) = (probe(f)?, probe(g)?);
        let rf = &charts.get(f)?.as_ref()?.0;
        let (twin, shift) = charted_on(&sf, &sg)?;
        let (rg, _) = face_chart(b, &q, &twin, g, sag, tol).ok()?;
        let turns: &[f64] = if sf.angular() { &[0.0, -std::f64::consts::TAU, std::f64::consts::TAU] } else { &[0.0] };
        turns
            .iter()
            .map(|k| Rigid2 { cos: 1.0, sin: 0.0, shift: shift + Vec2::new(0.0, *k) }.region(&rg))
            .map(|r| {
                let on = cut(std::slice::from_ref(rf), std::slice::from_ref(&r), solidcraft_kernel::BoolOp::Intersect).map_or(0.0, |x| area(&x));
                (on, r)
            })
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, r)| r)
    };
    // Behind face `f`: of the faces level with it that face back at it from inside its material,
    // the one it meets first over the most of its area (a shell's or a plate's other side), and
    // how far behind it lies; with `true`, that it faces back. Found from the faces' surfaces and
    // loops: nearest first, each face takes what of `f` the nearer ones left, so a wall's inside
    // face comes before the wall across the part, and a recess's floor in the other side takes
    // only its own small part.
    let behinds: Vec<Option<(f64, usize)>> = (0..count)
        .map(|f| {
            let sf = probe(f)?;
            let mut found: Vec<(f64, usize)> = (0..count)
                .filter(|g| *g != f)
                .filter_map(|g| {
                    let (depth, same) = level_of(&sf, &probe(g)?, tol)?;
                    (!same && depth > tol).then_some((depth, g))
                })
                .collect();
            found.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            let mut open = vec![charts.get(f)?.as_ref()?.0.clone()];
            let mut best: Option<(f64, f64, usize)> = None;
            for (depth, g) in found {
                let Some(p) = projected(f, g) else { continue };
                let p = std::slice::from_ref(&p);
                let first = cut(&open, p, solidcraft_kernel::BoolOp::Intersect).map_or(0.0, |x| area(&x));
                if first > tol * tol && best.is_none_or(|(a, ..)| first > a) {
                    best = Some((first, depth, g));
                }
                open = cut(&open, p, solidcraft_kernel::BoolOp::Cut).unwrap_or_default();
                if open.is_empty() {
                    break;
                }
            }
            best.map(|(_, depth, g)| (depth, g))
        })
        .collect();
    let behind = |f: usize| behinds.get(f).copied().flatten().map(|(d, g)| (d, g, true));

    // The walk starts on the first face, in the body's own order, that another faces back at a
    // small distance behind, as a shell's or a plate's do: the sheet keeps that thickness and
    // its material. With none, the body is a solid, which takes the rule's thickness, and the
    // walk starts on its first face. Nothing about the sheet depends on where it starts.
    let thin = |f: usize| behind(f).filter(|(d, _, back)| *back && *d > 0.0 && d * 6.0 <= extent(f)).map(|(d, ..)| d);
    // Thickening, the walk starts on the first face picked, and the sheet is a shell's or a
    // plate's where that face has another facing back at it a small distance behind.
    let first_thin = match &chosen {
        Some(set) => set.first().map(|f| (*f, thin(*f))).and_then(|(f, d)| d.map(|d| (f, d))),
        None => (0..count).filter(|f| probe(*f).is_some()).find_map(|f| thin(f).map(|d| (f, d))),
    };
    let (start, sheet) = match (first_thin, &chosen) {
        (Some((f, d)), _) => (f, Some(d)),
        (None, Some(set)) => match set.first() {
            Some(f) => (*f, None),
            None => return err("Thicken starts from a face: pick one"),
        },
        (None, None) => match (0..count).find(|f| probe(*f).is_some()) {
            Some(f) => (f, None),
            None => return err("not supported yet: a body with no face that is a plane, a cylinder or a cone"),
        },
    };
    let t = sheet.map(|d| (d * 1e9).round() / 1e9);
    let rule = &rule(t)?;
    let t = t.unwrap_or(rule.thickness);
    let shapes: Vec<std::result::Result<Shape, &'static str>> = (0..count).map(|f| Shape::of(b, &q, f, outward && sheet.is_none(), tol)).collect();
    let shape = |f: usize| shapes.get(f).copied().unwrap_or(Err("a face the body does not have"));
    let smooth = |e: usize, x: &Shape, y: &Shape| -> bool {
        let m = middle_of(e).unwrap_or_default();
        x.out(m).dot(y.out(m)).clamp(-1.0, 1.0).acos() <= SMOOTH
    };

    // The faces of the sheet: a solid's all, a shell's or a plate's side the start is on. That
    // side runs on smoothly, or across a sharp edge between two planes that is no rim: a face's
    // rims (its thickness faces) touch the face behind it. A wall's outside that runs on in one
    // plane with the base's edge touches it too, and is told from a rim by standing the sheet's
    // thickness in front of the face behind it, unless the face crossed from meets that wall's
    // inside as well: the inside is then the wall's face on its side, and the outside (one face
    // with the base's edge strips beside the wall) a rim from there. A face whose face behind is
    // the sheet's is the sheet's other side.
    let thick = |g: usize| sheet.is_some_and(|d| behind(g).is_some_and(|(e, _, back)| back && (e - d).abs() <= tol * 10.0));
    let rim = |f: usize, g: usize| -> bool {
        let Some((_, h, _)) = behind(f) else { return false };
        touches(g, h) && (!thick(g) || behind(g).is_some_and(|(_, inside, _)| touches(f, inside)))
    };
    // What each loop of a shell's or a plate's face opens onto (see `Opening`), followed across
    // the faces round it to the first faces level with it.
    let mut around: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); count];
    for (x, y) in (0..edge_count).filter_map(pair) {
        if x != y {
            around.get_mut(x).map(|a| a.insert(y));
            around.get_mut(y).map(|a| a.insert(x));
        }
    }
    let opening = |f: usize, edges: &[usize]| -> Opening {
        let Some(sf) = probe(f) else { return Opening::Open };
        let mut seen = BTreeSet::from([f]);
        let mut queue: VecDeque<usize> = edges.iter().filter_map(|e| pair(*e)).flat_map(|(x, y)| [x, y]).filter(|g| *g != f).collect();
        let (mut walls, mut levels) = (BTreeSet::new(), Vec::new());
        while let Some(g) = queue.pop_front() {
            if !seen.insert(g) {
                continue;
            }
            match probe(g).and_then(|sg| level_of(&sf, &sg, tol)) {
                Some((depth, same)) => levels.push((g, depth, same)),
                None => {
                    walls.insert(g);
                    queue.extend(around.get(g).into_iter().flatten().copied());
                }
            }
        }
        let near = tol * 10.0;
        if let Some((g, ..)) = levels.iter().find(|(_, depth, _)| *depth < -near) {
            return Opening::Raised(*g);
        }
        let floors: Vec<usize> = levels.iter().filter(|(_, depth, same)| *same && *depth > near).map(|x| x.0).collect();
        if !floors.is_empty() {
            return Opening::Recess(walls.into_iter().chain(floors).collect());
        }
        if levels.iter().any(|(_, depth, same)| !*same && *depth > near) { Opening::Through } else { Opening::Open }
    };
    // Each face's inner loops (their edges: its holes, and the openings of recesses and bosses)
    // and what they open onto, for a shell's or a plate's faces. Its outer loop is the panel's
    // outline: the faces round it are the sheet's rims or its other panels.
    let loops_of = |f: usize| -> Vec<(Vec<usize>, Opening)> {
        let (Some(face), Some(Some((region, uses)))) = (q.record().faces.get(f), charts.get(f)) else { return Vec::new() };
        let outer = |edges: &[usize]| {
            let first = edges.first().and_then(|e| uses.iter().find(|u| u.edge == *e)).and_then(|u| u.chart.first().copied());
            first.is_some_and(|c| region.outer.segs.iter().any(|s| s.point_at(s.closest_param(c).clamp(0.0, 1.0)).dist(c) <= tol * 100.0))
        };
        face.loops
            .iter()
            .filter_map(|l| match l {
                Bounds::Edges { edges } => Some(edges.iter().map(|oe| oe.edge as usize).collect::<Vec<usize>>()),
                Bounds::Vertex { .. } => None,
            })
            .filter(|edges| !outer(edges))
            .map(|edges| {
                let what = opening(f, &edges);
                (edges, what)
            })
            .collect()
    };
    let openings: Vec<Vec<(Vec<usize>, Opening)>> = if sheet.is_some() { (0..count).map(loops_of).collect() } else { Vec::new() };
    // A face of a recess in face `f` (its wall or its floor): no panel; the recess is read onto
    // `f`'s panel once the sheet is built (see `sheet_surface_cuts`).
    let recessed = |f: usize, g: usize| -> bool {
        openings.get(f).is_some_and(|ls| ls.iter().any(|(_, what)| matches!(what, Opening::Recess(faces) if faces.contains(&g))))
    };
    let mut behind_of: BTreeMap<usize, Option<usize>> = BTreeMap::new();
    let mut side = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(f) = queue.pop_front() {
        for e in 0..edge_count {
            let Some((x, y)) = pair(e) else { continue };
            let g = if x == f {
                y
            } else if y == f {
                x
            } else {
                continue;
            };
            if g == f || side.contains(&g) {
                continue;
            }
            let crosses = sheet.is_none() || {
                let rim = rim(f, g);
                let other_side = behind_of.entry(g).or_insert_with(|| behind(g).map(|h| h.1)).is_some_and(|h| side.contains(&h));
                match (shape(f), shape(g)) {
                    (Ok(sf), Ok(sg)) => {
                        !rim && !other_side
                            && !recessed(f, g)
                            && (smooth(e, &sf, &sg) || matches!((sf, sg), (Shape::Plane { .. }, Shape::Plane { .. })))
                    }
                    _ => false,
                }
            };
            if crosses {
                side.insert(g);
                queue.push_back(g);
            }
        }
    }

    // A pick on the other side of a shell or a plate is the face, or the edge, across the
    // thickness from it on the side the sheet is.
    let across = |g: usize| behind(g).filter(|h| h.2).map(|h| h.1);
    let on_side = |g: usize| if side.contains(&g) { Some(g) } else { across(g).filter(|h| side.contains(h)) };
    let on_side_edge = |e: usize| -> Option<usize> {
        let (x, y) = pair(e)?;
        if side.contains(&x) && side.contains(&y) {
            return Some(e);
        }
        let (hx, hy, m) = (on_side(x)?, on_side(y)?, middle_of(e)?);
        (0..edge_count)
            .filter(|f| pair(*f).is_some_and(|(u, v)| (u, v) == (hx, hy) || (u, v) == (hy, hx)))
            .filter_map(|f| middle_of(f).map(|n| (f, n.dist(m))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(f, _)| f)
    };
    // The edges bent. A bend modelled with its radius may be picked on a tangent edge of its
    // roll: that is a pick of the roll.
    let mut bent: BTreeSet<usize> = BTreeSet::new();
    let mut rolled: BTreeSet<usize> = rolled.iter().filter_map(|g| on_side(*g)).collect();
    for e in (0..edge_count).filter(|e| picked(bends, *e)) {
        let Some((x, y)) = pair(e) else { continue };
        match (shape(x), shape(y)) {
            (Ok(sx), Ok(sy)) if x != y && smooth(e, &sx, &sy) => rolled.extend([x, y].into_iter().filter(|g| is_roll(*g)).filter_map(on_side)),
            _ => bent.extend(on_side_edge(e)),
        }
    }
    let (bent, rolled) = (bent, rolled);

    // Converting, the sheet is that whole side. Thickening, it is the faces picked, the rolls of
    // modelled bends with the faces they run smoothly into, and every face a bend joins to them.
    let mut sheet_faces = match &chosen {
        None => side.clone(),
        Some(set) => {
            let mut faces = BTreeSet::new();
            for &g in set {
                let Some(h) = on_side(g) else {
                    return err(format!("the face at {} is not a face of the sheet's side", at(face_point(&q, g).unwrap_or_default())));
                };
                faces.insert(h);
            }
            for &g in &rolled {
                faces.insert(g);
                for e in 0..edge_count {
                    let Some((x, y)) = pair(e) else { continue };
                    let h = if x == g {
                        y
                    } else if y == g {
                        x
                    } else {
                        continue;
                    };
                    if let (Ok(sg), Ok(sh)) = (shape(g), shape(h))
                        && side.contains(&h)
                        && smooth(e, &sg, &sh)
                    {
                        faces.insert(h);
                    }
                }
            }
            loop {
                let before = faces.len();
                for &e in &bent {
                    let Some((x, y)) = pair(e) else { continue };
                    if faces.contains(&x) || faces.contains(&y) {
                        faces.extend([x, y]);
                    }
                }
                if faces.len() == before {
                    break;
                }
            }
            faces
        }
    };
    sheet_faces.retain(|f| !skipped.contains(f));
    let mut shaped: BTreeMap<usize, Shape> = BTreeMap::new();
    for &f in &sheet_faces {
        match shape(f) {
            Ok(s) => shaped.insert(f, s),
            Err(why) => {
                return err(format!("the face at {} can't be a panel: {why}. Skip it to leave it out", at(face_point(&q, f).unwrap_or_default())));
            }
        };
    }

    // Each edge between two faces of the sheet: a seam where it runs on smoothly, a bend where
    // the user picked it, and cut open everywhere else, as a roll's own seam is.
    let mut roles: Vec<(usize, usize, usize, Role)> = Vec::new();
    for e in 0..edge_count {
        let Some((x, y)) = pair(e) else { continue };
        let (Some(sx), Some(sy)) = (shaped.get(&x), shaped.get(&y)) else { continue };
        let role = if x == y {
            Role::Rip
        } else if smooth(e, sx, sy) {
            Role::Seam
        } else if bent.contains(&e) {
            Role::Bend
        } else {
            Role::Rip
        };
        roles.push((e, x, y, role));
    }
    // A bend is between two faces of the sheet where its edge, or the edge across the thickness
    // from it, is; or on a roll of the sheet.
    let between = |p: &Vec3| {
        let edge = (0..edge_count).filter(|e| on_edge(*p, *e)).filter_map(on_side_edge).any(|e| roles.iter().any(|(f, ..)| *f == e));
        edge || q.faces_at(*p, near).first().and_then(|(_, g)| on_side(*g)).is_some_and(|g| rolled.contains(&g))
    };
    if let Some(p) = bends.iter().find(|p| !between(p)) {
        // A shell's or a plate's bend onto a wall of another thickness: no sheet of one thickness.
        let other = sheet.and_then(|d| {
            (0..edge_count).filter(|e| on_edge(*p, *e)).filter_map(pair).find_map(|(x, y)| {
                let g = if side.contains(&x) { y } else { x };
                behind(g).map(|(dg, ..)| (dg, d)).filter(|(dg, d)| (dg - d).abs() > tol * 10.0)
            })
        });
        if let Some((dg, d)) = other {
            return err(format!("the bend at {} joins a wall {dg:.3} mm thick to a sheet {d:.3} mm thick: not a sheet of even thickness", at(*p)));
        }
        return err(format!("the bend at {} is not between two faces of the sheet", at(*p)));
    }

    // The panels, each piece from its first face in the body's order, across its bends and
    // seams; the first piece's start is the flat's frame.
    let mut left = sheet_faces.clone();
    let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
    // The edges the walk crossed, each to a face it had not reached.
    let mut crossed: BTreeSet<usize> = BTreeSet::new();
    let mut order = Vec::with_capacity(left.len());
    let mut first = chosen.as_ref().map(|_| start).filter(|f| left.contains(f));
    while let Some(root) = first.take().or_else(|| left.first().copied()) {
        left.remove(&root);
        order.push(root);
        let mut queue = VecDeque::from([root]);
        while let Some(f) = queue.pop_front() {
            for &(e, x, y, role) in &roles {
                let g = if x == f {
                    y
                } else if y == f {
                    x
                } else {
                    continue;
                };
                if role != Role::Rip && left.remove(&g) {
                    crossed.insert(e);
                    parent.insert(g, f);
                    order.push(g);
                    queue.push_back(g);
                }
            }
        }
    }
    if order.is_empty() {
        return err("every face of the body is skipped");
    }
    let index: BTreeMap<usize, usize> = order.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let mut panels = Vec::with_capacity(order.len());
    let mut places = Vec::with_capacity(order.len());
    let mut uses_of: BTreeMap<usize, Vec<UseChart>> = BTreeMap::new();
    for &f in &order {
        let Some(shape) = shaped.get(&f) else { continue };
        let point = face_point(&q, f).unwrap_or_default();
        places.push(shape.frame().ok_or_else(|| DocError::Invalid(format!("the face at {} has no frame", at(point))))?);
        let (mut region, uses) =
            face_chart(b, &q, shape, f, sag, tol).map_err(|e| DocError::Invalid(format!("the face at {} can't be a panel: {e}", at(point))))?;
        // A hole of a shell's or a plate's face opening onto a recess is no hole of the sheet:
        // the panel keeps that material. Material standing on the face (a boss) is no sheet of
        // even thickness.
        if let Some(loops) = openings.get(f) {
            // Which of the region's loops a loop of the face is: the one its first edge runs on.
            let on = |l: &Loop2, edges: &[usize]| {
                let first = edges.first().and_then(|e| uses.iter().find(|u| u.edge == *e)).and_then(|u| u.chart.first().copied());
                first.is_some_and(|c| l.segs.iter().any(|s| s.point_at(s.closest_param(c).clamp(0.0, 1.0)).dist(c) <= tol * 100.0))
            };
            for (edges, what) in loops {
                match what {
                    Opening::Raised(g) => {
                        let at_g = face_point(&q, *g).unwrap_or_default();
                        return err(format!("the face at {} is raised at {}: not a plate of even thickness", at(point), at(at_g)));
                    }
                    Opening::Recess(_) => region.holes.retain(|h| !on(h, edges)),
                    Opening::Through | Opening::Open => {}
                }
            }
        }
        uses_of.insert(f, uses);
        panels.push(Panel { name: format!("face {f}"), surface: shape.surface(), region });
    }

    // The joints: every edge between two faces of the sheet, `a` the face the other was reached
    // from where there is one; nothing about the sheet depends on which way a joint runs. Each
    // rip's plane is kept, by its ends and toward its `a` panel, and each bend's edge, for the
    // bends' ends below.
    let (mut joints, mut edges) = (Vec::new(), Vec::new());
    for &(e, x, y, role) in &roles {
        // A ring of smooth seams (a tube) can't lie flat: it opens where the walk closed it, at a
        // seam the walk did not cross. (A ring of bends is the user's to open: where its seam
        // goes is the design's.)
        let role = if role == Role::Seam && !crossed.contains(&e) { Role::Rip } else { role };
        let (Some(&ix), Some(&iy), Some(sx), Some(sy)) = (index.get(&x), index.get(&y), shaped.get(&x), shaped.get(&y)) else { continue };
        let a_first = parent.get(&x) != Some(&y);
        let ((ia, sa, fa), (ib, sb, fb)) = if a_first { ((ix, sx, x), (iy, sy, y)) } else { ((iy, sy, y), (ix, sx, x)) };
        let mid = middle_of(e).unwrap_or_default();
        let (Some(ra), Some(rb)) = (panels.get(ia).map(|p: &Panel| p.region.clone()), panels.get(ib).map(|p: &Panel| p.region.clone())) else {
            continue;
        };
        // The edge on each panel at the same points along it, in its own order: a roll's own
        // seam is two uses of one face, a turn apart.
        let uses = |f: usize| uses_of.get(&f).map(|v| v.iter().filter(|u| u.edge == e).collect::<Vec<&UseChart>>()).unwrap_or_default();
        let (ua, ub) = if fa == fb {
            match uses(fa).as_slice() {
                [p, q] => (*p, *q),
                _ => continue,
            }
        } else {
            match (uses(fa).first(), uses(fb).first()) {
                (Some(p), Some(q)) => (*p, *q),
                _ => continue,
            }
        };
        let (mut points, mut pa, mut ts) = ua.along();
        let (_, mut pb, _) = ub.along();
        let n = points.len();
        if n < 2 || pa.len() != n || pb.len() != n {
            return err(format!("the edge at {} reads differently on its two faces", at(mid)));
        }
        // Which way `a`'s material lies, looked for beside the edge's middle along its way there:
        // beside the line between its ends, a curved edge's panel can lie both ways.
        let left = {
            let (q0, q1, at_k) = if n == 2 {
                (pa[0], pa[1], pa[0].lerp(pa[1], 0.5))
            } else {
                let k = n / 2;
                (pa[k - 1], pa[k + 1], pa[k])
            };
            let extent = ra.outer.polyline(1e-3).bounds().size().len().max(1e-9);
            (q1 - q0).normalized().is_some_and(|d| ra.contains(at_k + d.perp() * (extent * 1e-4)))
        };
        if !left {
            points.reverse();
            pa.reverse();
            pb.reverse();
            ts.reverse();
        }
        let (Some(&from), Some(&a0), Some(&a1), Some(&b0), Some(&b1)) = (points.first(), pa.first(), pa.last(), pb.first(), pb.last()) else {
            continue;
        };
        // An anchor runs along the edge from its start; a closed edge's ends are one point, and
        // it runs the way the edge leaves there.
        let way = |p: &[Vec2], o: Vec2, end: Vec2| (end - o).normalized().or_else(|| p.get(1).and_then(|q| (*q - o).normalized()));
        let (Some(da), Some(db)) = (way(&pa, a0, a1), way(&pb, b0, b1)) else { continue };
        let (mut anchor_a, mut anchor_b) = (Anchor { panel: ia, origin: a0, dir: da }, Anchor { panel: ib, origin: b0, dir: db });
        let kind = if role == Role::Bend {
            let (Shape::Plane { out: na, .. }, Shape::Plane { out: nb, .. }) = (sa, sb) else {
                return err(format!("not supported yet: a bend on the edge of a roll, at {}", at(mid)));
            };
            let theta = na.dot(*nb).clamp(-1.0, 1.0).acos();
            // The bend turns toward the material where `b` rises behind `a`'s face.
            let lift = panels.get(ib).map_or(0.0, |p| {
                let pts = p.region.outer.polyline(1e-3);
                let Shape::Plane { plane, .. } = sb else { return 0.0 };
                (&pts).into_iter().map(|q| (plane.to_world(*q) - from).dot(*na)).fold(0.0, |m: f64, h| if h.abs() > m.abs() { h } else { m })
            });
            let len = a0.dist(a1);
            let mut bend = Bend {
                angle: theta,
                radius: rule.bend_radius,
                toward_material: lift < 0.0,
                datum: Datum::Sharp,
                length: len,
                ends: ZoneEnds { near: (0.0, len), far: (0.0, len) },
                tails: (Tail::Keep, Tail::Keep),
                relief: (Relief::None, Relief::None),
            };
            // A wall standing on part of the other panel's edge, its outside in one plane with
            // that edge where it runs on past the wall, bends only where it stands. Past the
            // wall's ends its face is a strip no taller than the sheet (the other panel's own
            // edge), so its outline crosses its tangent line where it crosses a line just above
            // the strip, and runs further along a line within it. A panel whose sides slant, a
            // triangle's, crosses the two lines in different places and bends its whole edge.
            // Past the wall's ends, what of its face lies in the bend's band is trimmed off it.
            let setback = bend.setback(t);
            let mut len = len;
            let stands = |r: &Region2, o: Vec2, d: Vec2, side: f64| {
                let at = |h: f64| extent_across(r, (o + d.perp() * (side * h), d)).map(|(lo, hi): (f64, f64)| (lo.max(0.0), hi.min(len)));
                let same = |x: (f64, f64), y: (f64, f64)| (x.0 - y.0).abs() <= tol && (x.1 - y.1).abs() <= tol;
                match (setback > t * (1.0 + 1e-9), at(setback), at((t + setback) / 2.0), at(t / 2.0)) {
                    (true, Some(top), Some(above), Some(strip)) if same(top, above) && !same(top, strip) => top,
                    _ => (0.0, len),
                }
            };
            let (stand_a, stand_b) = (stands(&ra, a0, da, 1.0), stands(&rb, b0, db, -1.0));
            let (lo, hi) = (stand_a.0.max(stand_b.0), stand_a.1.min(stand_b.1));
            if hi - lo > tol && (lo > tol || hi < len - tol) {
                anchor_a.origin = a0 + da * lo;
                anchor_b.origin = b0 + db * lo;
                let tail = |s: (f64, f64)| if s.0 > tol || s.1 < len - tol { Tail::Trim } else { Tail::Keep };
                bend.tails = (tail(stand_a), tail(stand_b));
                len = hi - lo;
                bend.length = len;
                bend.ends = ZoneEnds { near: (0.0, len), far: (0.0, len) };
            }
            JointKind::Bend(bend)
        } else if role == Role::Seam {
            JointKind::Seam
        } else {
            // The edge as the body has it, from `from` to `to`, at the points it was read at: on
            // each panel, its exact way at each, and the sheet's turn across it there, from the
            // two faces' own normals.
            let sign = if left { 1.0 } else { -1.0 };
            let ways: Vec<Vec3> = ts.iter().filter_map(|t| q.edge_at(e, *t).and_then(|(_, d)| d.normalized())).map(|w| w * sign).collect();
            if ways.len() != n {
                return err(format!("the edge at {} has no way along it", at(mid)));
            }
            let turns = points.iter().zip(&ways).map(|(p, x)| {
                // In the `a` face's frame: x along the edge, y toward its material, z on toward
                // the `b` face; the `b` face runs on from the edge into its own material.
                let (y, into_b) = (-sa.out(*p), sb.out(*p).cross(*x));
                into_b.dot(y).atan2(into_b.dot(x.cross(y)))
            });
            JointKind::Rip(Rip {
                curves: (exact_edge(b, sa, e, &pa, tol), exact_edge(b, sb, e, &pb, tol)),
                edges: (pa, pb),
                turns: turns.collect(),
                lap: Lap::Miter { gap: rule.gap },
            })
        };
        joints.push(Joint { a: anchor_a, b: anchor_b, kind });
        edges.push(mid);
    }

    let mut model = Sheet { thickness: t, k_factor: rule.k_factor, panels, joints, corners: Vec::new(), cuts: Vec::new() };
    let reliefs = Reliefs::of(rule);
    let notes = settle(&mut model, &places, &reliefs, &|_| None, tol, &at)?;
    // A hole reaching into the band a bend takes from its panel has no one place in the flat.
    for joint in &model.joints {
        let JointKind::Bend(bend) = &joint.kind else { continue };
        let setback = bend.setback(t);
        for (an, side) in [(joint.a, 1.0), (joint.b, -1.0)] {
            let (Some(panel), Some(place)) = (model.panels.get(an.panel), places.get(an.panel)) else { continue };
            if panel.surface != Surface::Plane {
                continue;
            }
            let n = an.dir.perp() * side;
            let band = Region2 {
                outer: Loop2::polygon(&[
                    an.origin,
                    an.origin + an.dir * bend.length,
                    an.origin + an.dir * bend.length + n * setback,
                    an.origin + n * setback,
                ])
                .ccw(),
                holes: Vec::new(),
            };
            for h in &panel.region.holes {
                let hole = Region2 { outer: h.ccw(), holes: Vec::new() };
                let met = solidcraft_kernel::planar_regions_boolean(
                    std::slice::from_ref(&hole),
                    std::slice::from_ref(&band),
                    solidcraft_kernel::BoolOp::Intersect,
                    1e-9,
                )?;
                if met.iter().map(Region2::area).sum::<f64>() > tol * setback.max(1.0) {
                    let c = h.centroid();
                    return err(format!("the hole at {} reaches into a bend: not supported yet", at(place.point(c.extend(0.0)))));
                }
            }
        }
    }
    let development = develop(&model, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
    if !development.loops.is_empty() {
        let names: Vec<String> = development.loops.iter().filter_map(|m| edges.get(m.joint)).map(|p| at(*p)).collect();
        return err(format!(
            "the edge at {} closes a ring of bends and seams: leave one of the ring's edges out of the bends to cut it open",
            names.join(" and the one at ")
        ));
    }
    Ok(SurfaceConversion {
        sheet: model,
        development,
        faces: order,
        places,
        edges,
        thickness: t,
        notes,
        reliefs,
        own_thickness: sheet.is_some(),
        pieces: std::sync::OnceLock::new(),
    })
}

/// Settles where each bend of `sheet` ends and what is cut there, from where `places` put its
/// panels in space. Where two bends standing on one panel meet at a corner of it, both end where
/// their tangent lines on it cross, with the corner's relief (`chosen` for the two bend joints,
/// else the rule's): a notch, a round or a square relief belonging to the corner, or a tear tight
/// against the rip that parts their other panels. Where a rip cuts a panel the bend joins, its
/// metal ends at the rip's plane. Elsewhere the rule's bend relief is cut in whichever panel runs
/// on past the end. A bend's edge is its anchors' first [`Bend::length`], so a sheet settled again
/// ends its bends where it did; the corners are the sheet's again. Why any relief asked for was
/// not cut comes back; `at` names a point in what is said.
pub(crate) fn settle(
    sheet: &mut Sheet,
    places: &[Rigid3],
    reliefs: &Reliefs,
    chosen: &dyn Fn([usize; 2]) -> Option<(CornerRelief, f64)>,
    tol: f64,
    at: &dyn Fn(Vec3) -> String,
) -> Result<Vec<String>> {
    let t = sheet.thickness;
    let (bent, rips) = in_space(sheet, places);
    // Where each bend ends, and what is cut there: (along its `a` anchor, along its `b` anchor,
    // the relief), at its start and at its end.
    let near = |p: Vec3, q: Vec3| p.dist(q) <= tol * 10.0;
    let slot = Relief::Slot { width: reliefs.width, depth: reliefs.depth };
    let mut ends: Vec<[(f64, f64, Relief); 2]> = bent.iter().map(|e| [(0.0, 0.0, slot), (e.len, e.len, slot)]).collect();
    let (mut notes, mut corners) = (Vec::new(), Vec::new());
    let found = bend_corners(&bent, near);
    for k in &found {
        let notched = || k.at.map(|a| (a, a, Relief::Notch));
        let joints = k.ends.map(|(i, _)| bent.get(i).map_or(usize::MAX, |b| b.joint));
        let (relief, size) = chosen(joints).unwrap_or((reliefs.corner, reliefs.corner_size));
        let values = match relief {
            CornerRelief::Tear => match tear(&bent, &sheet.joints, &rips, places, k, t, near) {
                Ok(Some(torn)) => torn,
                Ok(None) => notched(),
                Err(why) => {
                    notes.push(format!("the corner at {} keeps its notch: {why}", at(k.v)));
                    notched()
                }
            },
            CornerRelief::Round | CornerRelief::Square => {
                // How much of each bend runs on from the corner.
                let left = k.ends.iter().zip(k.at).map(|((i, e), a)| bent.get(*i).map_or(0.0, |b| if *e == 0 { b.len - a } else { a }));
                if !(size >= MIN_RELIEF) {
                    notes.push(format!("no corner relief at {}: the relief size {size:.4} mm is below {MIN_RELIEF} mm", at(k.v)));
                } else if left.fold(f64::INFINITY, f64::min) <= size / 2.0 + 1e-6 {
                    notes.push(format!("no corner relief at {}: a relief {size:.3} mm across would take a whole bend away", at(k.v)));
                } else {
                    corners.push(Corner { joints, relief, size });
                }
                notched()
            }
            CornerRelief::Notch => notched(),
        };
        if matches!(relief, CornerRelief::Notch | CornerRelief::Tear) {
            corners.push(Corner { joints, relief, size });
        }
        for ((i, e), value) in k.ends.iter().zip(values) {
            if let Some(slot) = ends.get_mut(*i).and_then(|x| x.get_mut(*e)) {
                *slot = value;
            }
        }
    }
    for (i, b) in bent.iter().enumerate() {
        let Some(d) = (b.ends[1] - b.ends[0]).normalized() else { continue };
        for e in 0..2 {
            if found.iter().any(|k| k.ends.contains(&(i, e))) {
                continue;
            }
            let (v, into) = if e == 0 { (b.ends[0], d) } else { (b.ends[1], -d) };
            // A rip that carries on along the bend's edge, tangent to it, runs alongside the bend
            // rather than across its end, and cuts nothing off it.
            let planes = rips.iter().filter_map(|(r, ends, planes, ways)| {
                let end = ends.iter().position(|p| near(*p, v))?;
                if ways.get(end).is_some_and(|w| w.dot(into).abs() > ALONGSIDE) {
                    return None;
                }
                let m = planes.get(end).copied().flatten()?;
                let rip = sheet.joints.get(*r)?;
                let mine = |p: usize| p == b.a.panel || p == b.b.panel;
                if mine(rip.a.panel) {
                    Some(m)
                } else if mine(rip.b.panel) {
                    Some(-m)
                } else {
                    None
                }
            });
            if let Some(s) = planes.filter_map(|m| bend_cut(v, into, m, b.normals, b.toward, b.radius, t, 0.0)).reduce(f64::max) {
                let a = if e == 0 { s } else { b.len - s };
                if let Some(slot) = ends.get_mut(i).and_then(|x| x.get_mut(e)) {
                    *slot = (a, a, Relief::Notch);
                }
            }
        }
    }
    for (b, [(n0, f0, r0), (n1, f1, r1)]) in bent.iter().zip(ends) {
        if n0 >= n1 && f0 >= f1 {
            return err(format!("the bend at {} is cut away by the rips at its ends", at(b.ends[0].lerp(b.ends[1], 0.5))));
        }
        if let Some(Joint { kind: JointKind::Bend(bend), .. }) = sheet.joints.get_mut(b.joint) {
            bend.ends = ZoneEnds { near: (n0, n1), far: (f0, f1) };
            bend.relief = (r0, r1);
        }
    }
    sheet.corners = corners;
    Ok(notes)
}

/// The bends between two planar panels (anchored at their sharps) and the rips of `sheet`, as
/// [`settle`] works with them: where `places` put their edges in space, and each rip's plane at
/// either end. A rip on a free-form panel is left out.
fn in_space(sheet: &Sheet, places: &[Rigid3]) -> (Vec<BendEdge>, Vec<RipEnds>) {
    let t = sheet.thickness;
    let surface = |p: usize| sheet.panels.get(p).map(|x| &x.surface);
    let mut bent = Vec::new();
    let mut rips: Vec<RipEnds> = Vec::new();
    for (j, joint) in sheet.joints.iter().enumerate() {
        let (Some(sa), Some(sb), Some(pa), Some(pb)) =
            (surface(joint.a.panel), surface(joint.b.panel), places.get(joint.a.panel), places.get(joint.b.panel))
        else {
            continue;
        };
        match &joint.kind {
            JointKind::Bend(bend) if bend.datum == Datum::Sharp && matches!((sa, sb), (Surface::Plane, Surface::Plane)) => {
                let on_a = |s: f64| pa.point((joint.a.origin + joint.a.dir * s).extend(0.0));
                bent.push(BendEdge {
                    joint: j,
                    ends: [on_a(0.0), on_a(bend.length)],
                    normals: (-pa.z, -pb.z),
                    toward: bend.toward_material,
                    len: bend.length,
                    a: joint.a,
                    b: joint.b,
                    setback: bend.setback(t),
                    radius: bend.radius,
                });
            }
            JointKind::Rip(rip) => {
                let (Some(&s0), Some(&s1), Some(&q0), Some(&q1)) = (rip.edges.0.first(), rip.edges.0.last(), rip.edges.1.first(), rip.edges.1.last())
                else {
                    continue;
                };
                let (Some(from), Some(to)) = (chart_point(sa, pa, s0), chart_point(sa, pa, s1)) else { continue };
                // The edge's way at each end: exactly, from either panel's curves where it has
                // them, else from its points' run there.
                let exact = |curves: &[Seg2], surface: &Surface, place: &Rigid3, end: usize| -> Option<Vec3> {
                    let (c, d) = if end == 0 {
                        curves.first().map(|g| (g.start(), g.start_tangent()))?
                    } else {
                        curves.last().map(|g| (g.end(), g.end_tangent()))?
                    };
                    chart_way(surface, place, c, d)?.normalized()
                };
                let run = |end: usize| -> Option<Vec3> {
                    let pts = &rip.edges.0;
                    let (c, d) = if end == 0 {
                        (*pts.first()?, *pts.get(1)? - *pts.first()?)
                    } else {
                        (*pts.last()?, *pts.last()? - *pts.iter().rev().nth(1)?)
                    };
                    chart_way(sa, pa, c, d)?.normalized()
                };
                let way = |end: usize| exact(&rip.curves.0, sa, pa, end).or_else(|| exact(&rip.curves.1, sb, pb, end)).or_else(|| run(end));
                let (Some(first), Some(last)) = (way(0), way(1)) else { continue };
                let plane = |s: Vec2, q: Vec2, x: Vec3| rip_plane(chart_out(sa, pa, s)?, chart_out(sb, pb, q)?, -x);
                rips.push((j, [from, to], [plane(s0, q0, first), plane(s1, q1, last)], [first, last]));
            }
            _ => {}
        }
    }
    (bent, rips)
}

/// The angle round a roll's axis at the angle parameter `v` of its chart (see `Shape::params`).
fn turned(v: f64, inward: bool) -> f64 {
    if inward { v } else { -v }
}

/// The point of a panel on `surface` at parameters `c`, where `place` puts the panel's frame; none
/// on a free-form panel.
fn chart_point(surface: &Surface, place: &Rigid3, c: Vec2) -> Option<Vec3> {
    let local = match *surface {
        Surface::Plane => c.extend(0.0),
        Surface::Cylinder { radius, material_inward } => {
            let a = turned(c.y, material_inward);
            Vec3::new(radius * a.cos(), radius * a.sin(), c.x)
        }
        Surface::Cone { half_angle, material_inward } => {
            let a = turned(c.y, material_inward);
            Vec3::new(half_angle.sin() * a.cos(), half_angle.sin() * a.sin(), half_angle.cos()) * c.x
        }
        Surface::Spline(_) => return None,
    };
    Some(place.point(local))
}

/// The unit normal away from the material at parameters `c` of a panel on `surface`.
fn chart_out(surface: &Surface, place: &Rigid3, c: Vec2) -> Option<Vec3> {
    let side = |inward: bool| if inward { 1.0 } else { -1.0 };
    let local = match *surface {
        Surface::Plane => Vec3::new(0.0, 0.0, -1.0),
        Surface::Cylinder { material_inward, .. } => {
            let a = turned(c.y, material_inward);
            Vec3::new(a.cos(), a.sin(), 0.0) * side(material_inward)
        }
        Surface::Cone { half_angle, material_inward } => {
            let a = turned(c.y, material_inward);
            (Vec3::new(a.cos(), a.sin(), 0.0) * half_angle.cos() - Vec3::Z * half_angle.sin()) * side(material_inward)
        }
        Surface::Spline(_) => return None,
    };
    Some(place.vector(local))
}

/// The way in space a curve through parameters `c` of a panel on `surface` runs, running `d`
/// there in the parameters.
fn chart_way(surface: &Surface, place: &Rigid3, c: Vec2, d: Vec2) -> Option<Vec3> {
    let local = match *surface {
        Surface::Plane => d.extend(0.0),
        Surface::Cylinder { radius, material_inward } => {
            let a = turned(c.y, material_inward);
            Vec3::Z * d.x + Vec3::new(-a.sin(), a.cos(), 0.0) * (radius * turned(d.y, material_inward))
        }
        Surface::Cone { half_angle, material_inward } => {
            let a = turned(c.y, material_inward);
            let (sin, cos) = half_angle.sin_cos();
            Vec3::new(sin * a.cos(), sin * a.sin(), cos) * d.x + Vec3::new(-a.sin(), a.cos(), 0.0) * (c.x * sin * turned(d.y, material_inward))
        }
        Surface::Spline(_) => return None,
    };
    Some(place.vector(local))
}

/// Names of the edges of `body` at the bend points, to find them again after the body changes. A
/// point on no edge gets none, and keeps its place: a bend picked on its roll, or a point the
/// conversion then says is no bend.
pub fn bend_edge_names(st: &crate::ModelState, body: &str, pts: &[Vec3]) -> Vec<String> {
    let Some(b) = st.body(body) else { return Vec::new() };
    if b.body.is_mesh() {
        return vec![String::new(); pts.len()];
    }
    let q = Query::new(b.body.record());
    let size = b.body.size();
    let (near, sag) = ((size * 2e-3).max(1e-3), (size * 1e-4).max(1e-4));
    let edges = q.record().edges.len();
    let on = |p: &Vec3| (0..edges).any(|e| q.edge_distance(e, *p, sag).is_some_and(|d| d <= near));
    pts.iter().zip(crate::naming::edge_names_at(b, pts)).map(|(p, name)| if on(p) { name } else { String::new() }).collect()
}

/// A planar face of `b`: its plane and its outward normal, as the record has its side.
pub(crate) fn planar_face(b: &Body, q: &Query, f: usize, tol: f64) -> Option<(Plane, Vec3)> {
    match Shape::of(b, q, f, false, tol) {
        Ok(Shape::Plane { plane, out }) => Some((plane, out)),
        _ => None,
    }
}

/// Face `f` of `b`, a plane whose outward normal is `out`, exactly in the coordinates of `plane`
/// (any plane parallel to it).
pub(crate) fn face_region(b: &Body, q: &Query, plane: &Plane, out: Vec3, f: usize, tol: f64) -> Option<Region2> {
    let sag = (b.size() * 1e-4).max(1e-4);
    face_chart(b, q, &Shape::Plane { plane: *plane, out }, f, sag, tol).ok().map(|(r, _)| r)
}

/// A point of face `f`: halfway along the first edge of its outer loop.
fn face_point(q: &Query, f: usize) -> Option<Vec3> {
    let face = q.record().faces.get(f)?;
    let first = face.loops.iter().find_map(|l| match l {
        Bounds::Edges { edges } => edges.first(),
        Bounds::Vertex { .. } => None,
    })?;
    let e = first.edge as usize;
    let [t0, t1] = q.edge_interval(e)?;
    q.edge_at(e, 0.5 * (t0 + t1)).map(|d| d.0)
}

impl SurfaceConversion {
    /// The flat pattern: each placed panel less its trims, and the bend zones, joined.
    pub fn flat(&self) -> Result<Vec<Region2>> {
        crate::sheet_surface::join_pieces(self.pieces()?, &mut FlatWork::new())
    }

    /// The flat's pieces: each placed panel less its trims, and the zones, all less the corners'
    /// reliefs. Cut the first time they are asked for.
    fn pieces(&self) -> Result<&[(Piece, Region2)]> {
        if let Some(p) = self.pieces.get() {
            return Ok(p);
        }
        let cut = crate::sheet_surface::pieces(&self.development, &mut FlatWork::new())?;
        Ok(self.pieces.get_or_init(|| cut))
    }

    /// The folded sheet: its solid, sewn from the flat's pieces carried into space.
    pub fn solid(&self) -> Result<Body> {
        let places: Vec<Option<Rigid3>> = self.places.iter().copied().map(Some).collect();
        let folded = fold_onto(&self.sheet, &self.development, &places)?;
        crate::sheet_surface_fold::solid(&self.development, &folded, self.thickness, self.pieces()?, &mut FlatWork::new())
    }
}

/// `sheet` folded, each piece of its flat where its first panel's place puts it. Every other
/// panel with a place of its own (the face it was converted from) must fold onto it through the
/// bends and seams that reach it: anywhere else, a joint was read wrong, and that is an error
/// rather than a sheet in the wrong place.
pub(crate) fn fold_onto(sheet: &Sheet, dev: &Development, places: &[Option<Rigid3>]) -> Result<Fold> {
    let root = |p: &Vec<usize>| p.first().and_then(|i| places.get(*i)).copied().flatten();
    let roots: Vec<Rigid3> =
        dev.pieces.iter().map(root).collect::<Option<_>>().ok_or_else(|| DocError::Invalid("a piece of the sheet has no place".into()))?;
    let folded = fold(sheet, dev, &roots).map_err(|e| DocError::Invalid(e.to_string()))?;
    let reach = dev.panels.iter().flatten().map(|p| p.region.outer.polyline(1e-3).bounds().size().len()).fold(1.0, f64::max);
    for (i, (got, own)) in folded.places.iter().zip(places).enumerate() {
        let (Some(got), Some(own)) = (got, own) else { continue };
        let off = got.shift.dist(own.shift) + reach * (got.x.dist(own.x) + got.y.dist(own.y) + got.z.dist(own.z));
        if off > (reach * 1e-9).max(1e-9) {
            let name = sheet.panels.get(i).map_or_else(|| format!("panel {i}"), |p| p.name.clone());
            return err(format!("the sheet folds {name} {off:.3e} mm from where the body has it"));
        }
    }
    Ok(folded)
}

/// A sheet metal body made from surfaces (#6748), as the model keeps it: its sheet of panels and
/// joints, the master every later feature changes; where each panel lies in space, for the panels
/// that the body's faces placed; and its flat pattern as last laid out, which
/// `sheet.flat_pattern` shows. The body itself is the folded sheet, or its flat while unfolded.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceSheet {
    pub body: String,
    pub rule: String,
    pub sheet: Sheet,
    /// Each panel's frame in space where a face of the body put it; none for a panel a later
    /// feature added, which lies where its joints fold it.
    pub places: Vec<Option<Rigid3>>,
    /// The body is the flat pattern, lying in the plane of the sheet's first panel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unfolded: bool,
    pub thickness: f64,
    pub k_factor: f64,
    /// The flat material, in the plane of the sheet's first panel.
    pub regions: Vec<Region2>,
    pub bends: Vec<SurfaceBendLine>,
    /// The largest strain any panel's flat leaves: none but on a free-form panel.
    pub stretch: f64,
    /// How many of the panels are rolls.
    pub rolls: usize,
    /// What the sheet's bends end with, as its rule had it.
    pub reliefs: Reliefs,
    /// A feature's own corner relief, in place of the rule's, for a bend it made: the bend's
    /// joint, the relief and its size. A corner takes the relief of the later of its two bends
    /// that has one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corner_reliefs: Vec<(usize, CornerRelief, f64)>,
    /// Why a relief asked for was not cut, as the sheet was last built.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The recesses cut into its flat panels (see `sheet_surface_cuts`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recesses: Vec<crate::Recess>,
    /// The recesses in the flat, as pockets measured from its face on the sheet's surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pockets: Vec<crate::sheet::SheetPocket>,
}

/// A bend in a sheet's flat: its zone, its center line, how far it turns (radians), its inside
/// radius, whether it turns toward the material, and the zone's width.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceBendLine {
    pub zone: Region2,
    pub start: Vec2,
    pub end: Vec2,
    pub angle: f64,
    pub radius: f64,
    pub up: bool,
    pub allowance: f64,
    /// The tabs the cuts leave of its zone: they ride flat with a panel, and the bend's line
    /// stops at them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<Region2>,
}

impl SurfaceSheet {
    /// A converted body's sheet as the model keeps it, for the body `body` under the rule `rule`,
    /// and its folded solid.
    pub fn converted(body: &str, rule: &str, c: &SurfaceConversion) -> Result<(SurfaceSheet, Body)> {
        let mut sheet = SurfaceSheet {
            body: body.into(),
            rule: rule.into(),
            sheet: c.sheet.clone(),
            places: c.places.iter().copied().map(Some).collect(),
            unfolded: false,
            thickness: c.thickness,
            k_factor: c.sheet.k_factor,
            regions: Vec::new(),
            bends: Vec::new(),
            stretch: 0.0,
            rolls: 0,
            reliefs: c.reliefs,
            corner_reliefs: Vec::new(),
            notes: c.notes.clone(),
            recesses: Vec::new(),
            pockets: Vec::new(),
        };
        sheet.laid_out(&c.development, c.pieces()?, c.flat()?);
        Ok((sheet, c.solid()?))
    }

    /// Lays the sheet out again after a change to it, and builds its body: the folded sheet, or
    /// its flat while it is unfolded.
    pub fn build(&mut self) -> Result<Body> {
        let dev = develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        if let Some(m) = dev.loops.first() {
            let panel = self.sheet.joints.get(m.joint).map_or(0, |j| j.a.panel);
            let name = self.sheet.panels.get(panel).map_or("", |p| p.name.as_str());
            return err(format!("the sheet closes a ring of bends and seams at {name}: cut one of the ring's joints open"));
        }
        self.settle(&dev)?;
        let dev = develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        let mut work = FlatWork::new();
        let pieces = crate::sheet_surface::pieces(&dev, &mut work)?;
        let regions = crate::sheet_surface::join_pieces(&pieces, &mut work)?;
        self.laid_out(&dev, &pieces, regions);
        if self.unfolded {
            let base = self.places.first().copied().flatten().ok_or_else(|| DocError::Invalid("the sheet's first panel has no place".into()))?;
            let frames: Vec<Option<Rigid3>> = dev.placements.iter().map(|p| p.map(|r| crate::sheet_surface_cuts::lift(&base, &r))).collect();
            return self.recessed(self.flat_solid()?, &frames);
        }
        let folded = fold_onto(&self.sheet, &dev, &self.places)?;
        let body = crate::sheet_surface_fold::solid(&dev, &folded, self.thickness, &pieces, &mut work)?;
        self.recessed(body, &folded.places)
    }

    /// `body` with the sheet's recesses cut from it, each from its panel where `frames` put it.
    fn recessed(&self, mut body: Body, frames: &[Option<Rigid3>]) -> Result<Body> {
        let t = self.sheet.thickness;
        // Past the face it opens from, a hair, so that no face of the cut lies on the sheet's.
        let margin = t * 1e-3;
        for r in &self.recesses {
            let frame = frames.get(r.panel).copied().flatten().ok_or_else(|| DocError::Invalid("a recessed panel has no place".into()))?;
            let plane = Plane::new(frame.shift, frame.x, frame.y).ok_or_else(|| DocError::Invalid("a recessed panel has no plane".into()))?;
            let (from, to) = if r.far { (t - r.depth, t + margin) } else { (-margin, r.depth) };
            let tool = solidcraft_kernel::extrude_planar_regions(&plane, std::slice::from_ref(&r.region), from, to)?;
            body = solidcraft_kernel::boolean(&body, &tool, solidcraft_kernel::BoolOp::Cut)?
                .ok_or_else(|| DocError::Invalid("a recess takes the whole sheet".into()))?;
        }
        Ok(body)
    }

    /// Settles where the sheet's bends end (see [`settle`]), from where its development `dev`
    /// folds its panels.
    fn settle(&mut self, dev: &Development) -> Result<()> {
        let folded = fold_onto(&self.sheet, dev, &self.places)?;
        let places: Vec<Rigid3> =
            folded.places.iter().copied().collect::<Option<_>>().ok_or_else(|| DocError::Invalid("a panel of the sheet has no place".into()))?;
        let reach = dev.panels.iter().flatten().map(|p| p.region.outer.polyline(1e-3).bounds().size().len()).fold(1.0, f64::max);
        let own = self.corner_reliefs.clone();
        let chosen = move |[x, y]: [usize; 2]| {
            let of = |j: usize| own.iter().find(|(k, ..)| *k == j).map(|(_, r, s)| (*r, *s));
            of(x.max(y)).or_else(|| of(x.min(y)))
        };
        self.notes = settle(&mut self.sheet, &places, &self.reliefs, &chosen, (reach * 1e-6).max(1e-6), &at)?;
        Ok(())
    }

    /// The flat as one extrusion, `thickness` deep into the material from the plane of the
    /// sheet's first panel, where that panel's face put it.
    fn flat_solid(&self) -> Result<Body> {
        let place = self.places.first().copied().flatten().ok_or_else(|| DocError::Invalid("the sheet's first panel has no place".into()))?;
        let plane = Plane::new(place.shift, place.x, place.y).ok_or_else(|| DocError::Invalid("the sheet's first panel has no plane".into()))?;
        Ok(solidcraft_kernel::extrude_planar_regions(&plane, &self.regions, 0.0, self.thickness)?)
    }

    /// The bends of the flat as the sheet tools list them, numbered from 1. A bend is listed
    /// where its center line has material that bends: one entry for each stretch the reliefs,
    /// cut-outs and tabs leave of it, all with the bend's number, and none for a bend they take
    /// whole.
    pub fn bend_infos(&self) -> Vec<crate::sheet::BendInfo> {
        let mut out = Vec::new();
        for (i, b) in self.bends.iter().enumerate() {
            let line = [Seg2::Line { a: b.start, b: b.end }];
            let mut cuts: Vec<f64> = vec![0.0, 1.0];
            for r in self.regions.iter().chain(&b.tabs) {
                for l in std::iter::once(&r.outer).chain(&r.holes) {
                    cuts.extend(solidcraft_develop::crossings(&line, &l.segs).iter().map(|c| c.1));
                }
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|x, y| (*x - *y).abs() <= 1e-12);
            for w in cuts.windows(2) {
                let (u0, u1) = (w[0], w[1]);
                let (start, end) = (b.start.lerp(b.end, u0), b.start.lerp(b.end, u1));
                let mid = start.lerp(end, 0.5);
                if start.dist(end) <= 1e-9 || !self.regions.iter().any(|r| r.contains(mid)) || b.tabs.iter().any(|t| t.contains(mid)) {
                    continue;
                }
                out.push(crate::sheet::BendInfo {
                    index: i + 1,
                    angle_deg: b.angle.to_degrees(),
                    radius: b.radius,
                    up: b.up,
                    allowance: b.allowance,
                    length: start.dist(end),
                    start,
                    end,
                });
            }
        }
        out
    }

    /// Keeps the flat `regions` of the development `dev` (joined from its `pieces`), with its
    /// bends and their tabs, stretch and rolls.
    fn laid_out(&mut self, dev: &Development, pieces: &[(Piece, Region2)], regions: Vec<Region2>) {
        self.bends = dev
            .zones
            .iter()
            .enumerate()
            .filter_map(|(k, z)| {
                let JointKind::Bend(b) = &self.sheet.joints.get(z.joint)?.kind else { return None };
                Some(SurfaceBendLine {
                    zone: z.region.clone(),
                    start: (z.near[0] + z.far[0]) * 0.5,
                    end: (z.near[1] + z.far[1]) * 0.5,
                    angle: b.angle,
                    radius: b.radius,
                    up: b.toward_material,
                    allowance: b.allowance(self.sheet.thickness, self.sheet.k_factor),
                    tabs: pieces.iter().filter(|(p, _)| matches!(p, Piece::Tab(t, _) if *t == k)).map(|(_, r)| r.clone()).collect(),
                })
            })
            .collect();
        self.regions = regions;
        self.stretch = dev.panels.iter().flatten().map(|p| p.stretch).fold(0.0, f64::max);
        self.rolls = self.sheet.panels.iter().filter(|p| p.surface != Surface::Plane).count();
        self.thickness = self.sheet.thickness;
        self.k_factor = self.sheet.k_factor;
        let t = self.thickness;
        self.pockets = self
            .recesses
            .iter()
            .filter_map(|r| {
                let place = dev.placements.get(r.panel).copied().flatten()?;
                let (z0, z1) = if r.far { (t - r.depth, t) } else { (0.0, r.depth) };
                Some(crate::sheet::SheetPocket { region: place.region(&r.region), z0, z1 })
            })
            .collect();
    }
}
