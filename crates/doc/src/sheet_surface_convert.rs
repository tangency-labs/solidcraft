//! Convert to Sheet Metal (#6748): a body's faces as a sheet of panels, rolls
//! among them, laid out flat by `solidcraft-develop`. No face is fixed: every face of the sheet
//! is a panel like any other, and the flat is drawn in the largest one's plane.
//!
//! Convert takes every face of the body, a solid's or a surface body's, less the faces the user
//! skips (or the faces picked, and every face a bend joins to them): each is a panel, however thin
//! the body. A thin plate converts to six panels parted by rips, never to one sheet; nothing about
//! the body's thickness is read. Its material is the rule's thickness on the faces' side the
//! conversion's `direction` says, and panels whose material would overlap (a thin block's two
//! sides, each the rule's thickness in) are refused, naming them: their folded pieces are
//! intersected exactly. So is a panel its rips leave no material (the mitres from its edges
//! passing each other), from the outline the development draws for it. A face the user picks to
//! engrave is no panel: its recess is kept on the panel round it, as deep as its floor lies
//! behind that panel's face. Recognize reads a body that is sheet metal already
//! (`SheetFaces::Sheet`): its larger side, every face of which has its other side one thickness
//! behind it, the shell's or the plate's own recesses kept. The sheet
//! is the rule's, as the tooling makes it: the rule's thickness, taken into the material from
//! that side whatever the body's, and its bends at the rule's radius. A cylinder modelled between
//! two planes, its inside radius short of 10^0.7 times the body's thickness (DIN 6935), is a bend
//! at the planes' virtual sharp; any other cylinder or cone is a roll that keeps its modeled
//! radius, joined by seams where the sheet runs on smoothly. Every sharp edge between two faces
//! of the sheet is cut open, half the rule's gap from the plane halfway between the two faces as
//! `sheet.convert` cuts it, unless the user picks it as a bend. A ring of bends and seams is
//! refused, naming its edges. A solid's material lies inside its faces, or outside them.
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
//! edge's curve. Recognizing, a shell's or a plate's other side is, of the faces level with each
//! face and facing back at it, the one it meets first over the most of its area; what a hole of a
//! face opens onto (a hole through, a recess, a boss) is found by following the faces round it.
//! Converting, no face has another side. A roll's edges are read into its
//! chart at points, as lines where they run straight there and splines through the points
//! elsewhere. A roll that closes round its axis opens along its own seam.
//!
//! Not converted yet: a free-form face (not developed from its spline yet), a sphere or a torus
//! (neither is developable), a bend on the edge of a roll, and a shell's face that meets the
//! sheet at a sharp edge but is not a plane (it is left out).

use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use solidcore_record::brep::Loop as Bounds;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::{
    Anchor, Bend, Corner, CornerRelief, Cut, Datum, Development, Fold, Joint, JointKind, Lap, Panel, Relief, Rigid2, Rigid3, Rip, Sheet, Surface,
    Tail, ZoneEnds, curve_through, develop, fold,
};
use solidcraft_geom::brep::query::Query;
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

    /// The relief at a free end of a bend that a panel runs on past: the rule's bend relief, a
    /// slot `width` wide cut from `depth` inside the panel's tangent line. The joint holds it (see
    /// [`settle`]); a feature that ends a bend short of its edge cuts nothing itself.
    pub fn bend_end(&self) -> Relief {
        Relief::Slot { width: self.width, depth: self.depth }
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
    /// The body's own thickness, where Recognize read one (a shell's or a plate's): its faces'
    /// recesses are read there and kept on the sheet, which takes the rule's. Convert reads none.
    pub own_thickness: Option<f64>,
    /// The engravings Convert keeps, of the faces the user picked to engrave: each a recess of its
    /// panel from the sheet's surface, as deep as its floor lies behind the panel's face.
    pub recesses: Vec<crate::Recess>,
    /// Each bend modelled with its radius: its joint and the inside radius the body has there.
    pub modelled_radii: Vec<(usize, f64)>,
    /// The rule's bend radius, every bend's.
    pub rule_radius: f64,
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

/// An edge read exactly in a plane as segments, one per Bezier span: a free-form curve of several
/// spans split at its knots (exact: knot insertion on its homogeneous points). None for a rational
/// span above degree two, which no segment holds.
fn segments(pieces: &[solidcraft_kernel::PlanarPiece]) -> Option<Vec<Seg2>> {
    use solidcraft_geom::bspline::HPoint;
    use solidcraft_geom::spline_space::{Space, embed};
    let mut out = Vec::new();
    for piece in pieces {
        let s = match piece {
            solidcraft_kernel::PlanarPiece::Seg(seg) => {
                out.push(*seg);
                continue;
            }
            solidcraft_kernel::PlanarPiece::Spline(s) => s,
        };
        let d = s.degree;
        if d == 0 {
            return None;
        }
        let weight = |i: usize| s.weights.get(i).copied().unwrap_or(1.0);
        let ctrl: Vec<HPoint> = s.points.iter().enumerate().map(|(i, p)| HPoint::weighted(Vec3::new(p.x, p.y, 0.0), weight(i))).collect();
        let from = Space::new(d, s.knots.clone()).ok()?;
        let [lo, hi] = from.domain();
        let mut breaks = s.knots.clone();
        breaks.dedup();
        let mut knots = vec![lo; d + 1];
        for &k in breaks.iter().filter(|&&k| k > lo && k < hi) {
            knots.extend(std::iter::repeat_n(k, d));
        }
        knots.extend(std::iter::repeat_n(hi, d + 1));
        let spans = embed(&from, &ctrl, &Space::new(d, knots).ok()?).ok()?;
        let rational = s.weights.windows(2).any(|w| w[0] != w[1]);
        // Clamped, every inner knot of multiplicity d: span i is control points i d ..= i d + d.
        for span in (0..(spans.len() - 1) / d).filter_map(|i| spans.get(i * d..=i * d + d)) {
            let pts: Vec<Vec2> = span.iter().map(|h| h.project().map(|p| Vec2::new(p.x, p.y))).collect::<Option<_>>()?;
            out.push(match (rational, pts.as_slice()) {
                (false, _) => solidcraft_geom::bezier(&pts)?,
                (true, &[a, apex, b]) => Seg2::Conic { a, apex, b, w: span.get(1)?.w / (span.first()?.w * span.get(2)?.w).sqrt() },
                _ => return None,
            });
        }
    }
    Some(out)
}

/// Edge `e` of the body exactly in a planar face's parameters (lines, circular arcs, free-form
/// spans), run the way its points `path` there run; empty on a roll, and where the body's edge
/// doesn't end where the points do.
fn exact_edge(b: &Body, shape: &Shape, e: usize, path: &[Vec2], tol: f64) -> Vec<Seg2> {
    let Shape::Plane { plane, .. } = shape else { return Vec::new() };
    let (Some(first), Some(last)) = (path.first(), path.last()) else { return Vec::new() };
    let Some(Some(pieces)) = b.planar_edges(plane, &[e]).into_iter().next() else { return Vec::new() };
    let Some(curve) = segments(&pieces) else { return Vec::new() };
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
pub(crate) fn bend_cut(v: Vec3, d: Vec3, m: Vec3, normals: (Vec3, Vec3), toward: bool, r: f64, t: f64, gap: f64) -> Option<f64> {
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

/// Region `r` run on past its edge from `a` to `z` (a line of its outline) by `by`, square across
/// it, out to a modelled bend's sharp. At each end, the line before or after the edge is carried
/// on to the sharp where it heads out across the edge; elsewhere (it runs on along the edge's
/// line, or is round) the end steps out square to the edge, a step the bend's setback takes up
/// again.
fn to_sharp(r: &Region2, (a, z): (Vec2, Vec2), by: f64, tol: f64) -> std::result::Result<Region2, String> {
    let segs = &r.outer.segs;
    let n = segs.len();
    let near = tol * 100.0;
    // The line of the outline the edge lies along (the outline may run on along it past the
    // edge's ends), and the edge's ends in the line's own order.
    let along = |s: &Seg2, x: Vec2| -> Option<f64> {
        let Seg2::Line { a: p, b: q } = *s else { return None };
        let d = (q - p).normalized()?;
        let u = (x - p).dot(d);
        ((x - p - d * u).len() <= near && u >= -near && u <= p.dist(q) + near).then_some(u)
    };
    let k = (0..n)
        .find(|k| along(&segs[*k], a).is_some() && along(&segs[*k], z).is_some())
        .ok_or("meets a modelled bend along an edge that is not on a line of its outline")?;
    let (ua, uz) = (along(&segs[k], a).unwrap_or(0.0), along(&segs[k], z).unwrap_or(0.0));
    let (p, q) = if ua <= uz { (a, z) } else { (z, a) };
    let d = (q - p).normalized().ok_or("meets a modelled bend along an edge with no length")?;
    let extent = r.outer.polyline(1e-3).bounds().size().len().max(1e-9);
    let out = if r.contains(p.lerp(q, 0.5) + d.perp() * (extent * 1e-4)) { -d.perp() } else { d.perp() };
    // Where the edge ends at an end of the line, the line before or after it may carry on.
    let (p_end, q_end) = (p.dist(segs[k].start()) <= near, q.dist(segs[k].end()) <= near);
    // The line `s`, from `from` to the end `at`, carried on past `at` to the sharp, where it
    // heads out across the edge.
    let carried = |s: &Seg2, at: Vec2, from: Vec2| -> Option<Vec2> {
        let Seg2::Line { .. } = s else { return None };
        let w = (at - from).normalized()?;
        (w.dot(out) > 1e-6 && w.cross(d).abs() > 1e-6).then(|| at + w * (by / w.dot(out)))
    };
    let (before, after) = (&segs[(k + n - 1) % n], &segs[(k + 1) % n]);
    let p_out = carried(before, p, before.start()).filter(|_| p_end).unwrap_or(p + out * by);
    let q_out = carried(after, q, after.end()).filter(|_| q_end).unwrap_or(q + out * by);
    let band = Region2 { outer: Loop2::polygon(&[p, q, q_out, p_out]).ccw(), holes: Vec::new() };
    let joined =
        solidcraft_kernel::planar_regions_boolean(std::slice::from_ref(r), std::slice::from_ref(&band), solidcraft_kernel::BoolOp::Union, tol)
            .map_err(|e| e.to_string())?;
    match <[Region2; 1]>::try_from(joined) {
        Ok([one]) => Ok(one),
        Err(_) => Err("run on to a modelled bend's sharp is not one piece".into()),
    }
}

/// Each roll's lines in the flat (see [`RollLine`]): its rulings where its angle is least and
/// greatest, and halfway between, each across the roll's region and then where the flat
/// `regions` have material, folded flat by `fold`.
fn roll_lines(sheet: &Sheet, fold: &Fold, regions: &[Region2]) -> Vec<RollLine> {
    let mut out = Vec::new();
    for (i, p) in sheet.panels.iter().enumerate() {
        if matches!(p.surface, Surface::Plane | Surface::Spline(_)) {
            continue;
        }
        let pts = p.region.outer.polyline(1e-3);
        let (lo, hi) = (&pts).into_iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q.y), h.max(q.y)));
        let left = (&pts).into_iter().fold(f64::INFINITY, |l, q| l.min(q.x));
        if !(hi > lo && left.is_finite()) {
            continue;
        }
        // The rulings at its least and greatest angle are read a hair inside it.
        let hair = (hi - lo) * 1e-6;
        for (kind, at, read) in
            [(RollEdge::Start, lo, lo + hair), (RollEdge::End, hi, hi - hair), (RollEdge::Middle, 0.5 * (lo + hi), 0.5 * (lo + hi))]
        {
            let Some((a, b)) = extent_across(&p.region, (Vec2::new(left, read), Vec2::X)) else { continue };
            let (Some(s), Some(e)) = (fold.flat_point(i, Vec2::new(left + a, at)), fold.flat_point(i, Vec2::new(left + b, at))) else { continue };
            out.extend(on_material(s, e, regions).into_iter().map(|(start, end)| RollLine { panel: i, kind, start, end }));
        }
    }
    out
}

/// The pieces of the line from `s` to `e` where `regions` have material: cut where it crosses
/// their outlines, each piece kept where the material lies beside its middle.
fn on_material(s: Vec2, e: Vec2, regions: &[Region2]) -> Vec<(Vec2, Vec2)> {
    let line = [Seg2::Line { a: s, b: e }];
    let mut cuts: Vec<f64> = vec![0.0, 1.0];
    for r in regions {
        for l in std::iter::once(&r.outer).chain(&r.holes) {
            cuts.extend(solidcraft_develop::crossings(&line, &l.segs).iter().map(|c| c.1));
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|x, y| (*x - *y).abs() <= 1e-12);
    let Some(d) = (e - s).normalized() else { return Vec::new() };
    let hair = s.dist(e) * 1e-6;
    cuts.windows(2)
        .map(|w| (s.lerp(e, w[0]), s.lerp(e, w[1])))
        .filter(|(a, b)| a.dist(*b) > 1e-9)
        .filter(|(a, b)| {
            let mid = a.lerp(*b, 0.5);
            [mid, mid + d.perp() * hair, mid - d.perp() * hair].iter().any(|m| regions.iter().any(|r| r.contains(*m)))
        })
        .collect()
}

/// Where a region's outline crosses a line (a point and a unit direction), as lengths along the
/// line from its point: the least and the greatest. `None` where it doesn't.
pub(crate) fn extent_across(r: &Region2, (p, u): (Vec2, Vec2)) -> Option<(f64, f64)> {
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

/// The corners where two bends standing on one panel meet (see [`BendCorner`]): every such pair
/// of bend ends, so that a bend's end where three bends meet (a face's two sides at an
/// icosahedron's corner, a flange on a wall's end edge) is in each corner it makes, whatever
/// order the bends were made in.
fn bend_corners(bent: &[BendEdge], near: impl Fn(Vec3, Vec3) -> bool) -> Vec<BendCorner> {
    let ends: Vec<(usize, usize, &BendEdge, Vec3)> =
        bent.iter().enumerate().flat_map(|(i, b)| b.ends.iter().enumerate().map(move |(e, p)| (i, e, b, *p))).collect();
    let shared = |x: &BendEdge, y: &BendEdge| [x.a.panel, x.b.panel].into_iter().find(|p| y.on(*p).is_some());
    let mut out = Vec::new();
    for (n, &(i, e, bi, v)) in ends.iter().enumerate() {
        for &(k, f, bk, p) in ends.iter().skip(n + 1) {
            if k == i || !near(p, v) {
                continue;
            }
            let Some(panel) = shared(bi, bk) else { continue };
            let (Some(ai), Some(ak)) = (bi.on(panel), bk.on(panel)) else { continue };
            let into = |(at, _): (Anchor, f64), end: usize| if end == 0 { at.dir } else { -at.dir };
            let (di, dk) = (into(ai, e), into(ak, f));
            // A convex corner: each bend runs off into the panel's side of the other.
            if !(dk.dot(ai.0.dir.perp() * ai.1) > 1e-9 && di.dot(ak.0.dir.perp() * ak.1) > 1e-9) {
                continue;
            }
            let Some(c) = crossing(bi.tangent(ai), bk.tangent(ak)) else { continue };
            out.push(BendCorner { ends: [(i, e), (k, f)], v, panel, at: [(c - ai.0.origin).dot(ai.0.dir), (c - ak.0.origin).dot(ak.0.dir)] });
        }
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

/// Where a bend's free end at `end` along its anchors meets each panel's tangent line, `(a, b)`,
/// for a panel whose outline ends there: the side beside the end of the panel's edge along its
/// anchor nearest the bend's end, or, for a panel with no edge on its anchor (its sharp drawn
/// inside its edge), the edge whose line, laid out with the bend in the flat, passes through the
/// end on the anchor's line and reaches the band between that and the tangent line. The zone then
/// carries on that side, as the blank does: a flange on a slanted edge continues the plate's
/// edges beside it. `None` for a panel that runs on past the end, or ends square to it otherwise.
fn outline_ends(sheet: &Sheet, b: &BendEdge, bend: &Bend, end: f64, tol: f64) -> (Option<f64>, Option<f64>) {
    let t = sheet.thickness;
    let (width, setback) = (bend.allowance(t, sheet.k_factor), bend.setback(t));
    let reach = (width.abs() + setback + t) * 4.0;
    // Across the joint in the flat, out from the `a` anchor: each panel's points, its anchor's
    // line (`base`) and its tangent line. The `a` panel's material lies left of its anchor, the
    // `b` panel's right of its own, which lies `width - 2·setback` out.
    let side = |an: &Anchor, into: Vec2, base: f64, sign: f64, tangent: f64| -> Option<f64> {
        let region = &sheet.panels.get(an.panel)?.region;
        let flat = |p: Vec2| ((p - an.origin).dot(an.dir), base + sign * (p - an.origin).dot(into));
        // Each loop's segments in order, its lines laid out (a curve, `None`, has no line to
        // carry on, and keeps its place: the side beside an edge is its neighbour in the loop).
        let lines: Vec<Vec<Option<((f64, f64), (f64, f64))>>> = std::iter::once(&region.outer)
            .chain(&region.holes)
            .map(|l| l.segs.iter().map(|g| if let Seg2::Line { a: p, b: q } = *g { Some((flat(p), flat(q))) } else { None }).collect())
            .collect();
        // Where a line meets the tangent line, near the end.
        let meets = |(p, q): ((f64, f64), (f64, f64))| -> Option<f64> {
            let (ds, dy) = (q.0 - p.0, q.1 - p.1);
            if dy.abs() <= 1e-9 * (ds.abs() + dy.abs()) {
                return None;
            }
            let s = p.0 + ds * (tangent - p.1) / dy;
            ((s - end).abs() <= reach).then_some(s)
        };
        let on_anchor = |p: (f64, f64)| (p.1 - base).abs() <= tol;
        let mut beside: Option<(f64, f64)> = None;
        for segs in &lines {
            let n = segs.len();
            for (k, &(p, q)) in segs.iter().enumerate().filter_map(|(k, s)| s.as_ref().map(|s| (k, s))) {
                if !(on_anchor(p) && on_anchor(q)) {
                    continue;
                }
                for (corner, next) in [(p, segs.get((k + n - 1) % n)), (q, segs.get((k + 1) % n))] {
                    // At the end: on it, or as far off it as the side's slant carries it from
                    // the `a` anchor's line to this one (a side laid on in the flat).
                    let Some(&Some((g0, g1))) = next else { continue };
                    let (ds, dy) = (g1.0 - g0.0, g1.1 - g0.1);
                    let slant = if dy.abs() > 1e-9 * (ds.abs() + dy.abs()) { (ds / dy * base).abs() } else { 0.0 };
                    let off = (corner.0 - end).abs();
                    if off <= slant + tol
                        && beside.is_none_or(|(o, _)| off < o)
                        && let Some(s) = meets((g0, g1))
                    {
                        beside = Some((off, s));
                    }
                }
            }
        }
        if let Some((_, s)) = beside {
            return Some(s);
        }
        let mut found = None;
        for &(p, q) in lines.iter().flatten().flatten() {
            let (ds, dy) = (q.0 - p.0, q.1 - p.1);
            if dy.abs() <= 1e-9 * (ds.abs() + dy.abs()) {
                continue;
            }
            let through = p.0 + ds * (base - p.1) / dy;
            let (lo, hi) = (p.1.min(q.1), p.1.max(q.1));
            if (through - end).abs() > tol || hi < tangent.min(base) - tol || lo > tangent.max(base) + tol {
                continue;
            }
            found = meets((p, q)).or(found);
        }
        found
    };
    (side(&b.a, b.a.dir.perp(), 0.0, -1.0, -setback), side(&b.b, -b.b.dir.perp(), width - 2.0 * setback, 1.0, width - setback))
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
/// the first faces level with the face: material standing on the face (a boss, at that face, and
/// the boss's faces: its walls and the faces level with the face in front of it); a floor facing
/// the way the face does behind it (a recess: its walls and floors, a hole through its floor or
/// not); the face's other side, or a floor facing away (a hole through); or none of these.
enum Opening {
    Raised(usize, BTreeSet<usize>),
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

/// The inside radius, in thicknesses, from which a cylinder modelled between two planes is a
/// roll, not a bend: 10^0.7, where DIN 6935's k(R/T) = 0.65 + 0.5 lg(R/T) reaches 1 and the
/// neutral fiber lies halfway through the sheet. Below it the fiber moves toward the inside, as a
/// bend's K-factor takes it.
const ROLL_FROM: f64 = 5.011_872_336_272_722;

/// A bend modelled with its radius: a cylinder of the sheet between two of its planes, each
/// tangent to it along a straight edge parallel to its axis. The cylinder is no panel; the two
/// planes run on to where they cross, the bend's virtual sharp, and bend there with the rule's
/// radius, the tooling's, whatever the model's.
struct Modelled {
    /// The cylinder's faces: one, or the pieces cut-outs across the bend parted it into (a bend
    /// split by cut-outs across it is one bend).
    rolls: Vec<usize>,
    /// Each plane, the edges it meets the cylinder along (one for each piece, in the order of
    /// `rolls`), and how far it runs on past them, square across, to the sharp.
    sides: [(usize, Vec<usize>, f64); 2],
    /// The sharp's ends, square across from the cylinder's ends.
    sharp: (Vec3, Vec3),
    /// The cylinder's inside radius, as modelled.
    inside: f64,
}

impl Modelled {
    /// Takes in `piece`, another face of the same cylinder between the same two planes: its
    /// faces and edges, and the sharp run on as far as either reaches.
    fn join(&mut self, piece: Modelled) {
        let Some(d) = (self.sharp.1 - self.sharp.0).normalized() else { return };
        let o = self.sharp.0;
        let ends = [self.sharp.0, self.sharp.1, piece.sharp.0, piece.sharp.1];
        let along = |p: &&Vec3| (**p - o).dot(d);
        let lo = ends.iter().min_by(|x, y| along(x).total_cmp(&along(y)));
        let hi = ends.iter().max_by(|x, y| along(x).total_cmp(&along(y)));
        if let (Some(lo), Some(hi)) = (lo, hi) {
            self.sharp = (*lo, *hi);
        }
        self.rolls.extend(piece.rolls);
        for (mine, theirs) in self.sides.iter_mut().zip(piece.sides) {
            mine.1.extend(theirs.1);
        }
    }
}

/// The faces of a body that become the sheet.
#[derive(Clone, Copy, Debug)]
pub enum SheetFaces<'a> {
    /// Convert: every face of the body, however thin, less the faces at these points.
    AllBut(&'a [Vec3]),
    /// Convert: the faces at these points, and every face a bend joins to them.
    Picked(&'a [Vec3]),
    /// Recognize: every face of one side of a body that is sheet metal already (a shell, a plate,
    /// an imported part), the side of the face at `face` or else of the body's largest face, less
    /// the faces at `skip`. Every face of that side has its other side one thickness behind it.
    Sheet { face: Option<Vec3>, skip: &'a [Vec3] },
}

/// What Recognize reads of a body that is sheet metal already (#6748), before any rule applies:
/// its own thickness, the inside radius of each bend modelled with its radius, and a point on a
/// face of the side that is the sheet's surface (where `to_world` puts it).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetMeasure {
    pub thickness: f64,
    pub radii: Vec<f64>,
    pub surface: Vec3,
}

/// What [`read_surfaces`] gives: the conversion, or with no rule, the body measured.
enum Reading {
    Converted(Box<SurfaceConversion>),
    Measured(SheetMeasure),
}

/// Measures `mb` for Recognize (#6748), as [`convert_surfaces`] reads it with
/// [`SheetFaces::Sheet`]: its own thickness, its modelled bends' inside radii and its surface
/// side, the side of the face at `face` or else of its largest face. A body that is no sheet of
/// one thickness is refused, naming the thicknesses found.
pub fn measure_sheet_body(mb: &ModelBody, face: Option<Vec3>, skip: &[Vec3], to_world: &crate::Mat) -> Result<SheetMeasure> {
    match read_surfaces(mb, None, &[], SheetFaces::Sheet { face, skip }, &[], false, to_world)? {
        Reading::Measured(m) => Ok(m),
        Reading::Converted(_) => err("the body was converted, not measured"),
    }
}

/// Converts `mb` to sheet metal from its `faces`, bending at the `bends` and cutting every other
/// sharp edge between two faces of the sheet open. A bend is picked on its edge, or, where it is
/// modelled with its radius, on its roll: the roll and the faces it runs smoothly into join the
/// sheet. A solid's material lies inside its faces, or outside them when `outward`. The sheet is
/// the rule's, as the tooling makes it: its thickness and its bends' radius are the rule's
/// (`rule(None)`), whatever the body's. Converting, every face is a panel and the faces at
/// `engrave` are engravings of the panels round them; recognizing, a shell's or a plate's own
/// thickness only tells its sides apart and how a modelled corner is rounded. Points are the
/// body's own; what it says gives them where `to_world` puts them.
pub fn convert_surfaces(
    mb: &ModelBody,
    rule: &dyn Fn(Option<f64>) -> Result<ConvertRule>,
    bends: &[Vec3],
    faces: SheetFaces,
    engrave: &[Vec3],
    outward: bool,
    to_world: &crate::Mat,
) -> Result<SurfaceConversion> {
    match read_surfaces(mb, Some(rule), bends, faces, engrave, outward, to_world)? {
        Reading::Converted(c) => Ok(*c),
        Reading::Measured(_) => err("the body was measured, not converted"),
    }
}

/// [`convert_surfaces`]; with no rule, the body measured (see [`measure_sheet_body`]) once its
/// sheet's faces and modelled bends are read.
fn read_surfaces(
    mb: &ModelBody,
    rule: Option<&dyn Fn(Option<f64>) -> Result<ConvertRule>>,
    bends: &[Vec3],
    faces: SheetFaces,
    engrave: &[Vec3],
    outward: bool,
    to_world: &crate::Mat,
) -> Result<Reading> {
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
        SheetFaces::Sheet { skip, .. } => (skip, None),
    };
    // The face picked first (`face`), as the user gave it.
    let first_pick = match faces {
        SheetFaces::Picked(points) => points.first().map(|p| face_at(*p, "picked face")).transpose()?,
        _ => None,
    };
    let mut skipped = BTreeSet::new();
    for p in skip {
        skipped.insert(face_at(*p, "skipped face")?);
    }
    // Recognize alone reads a body as sheet metal (#6748): a shell's or a plate's other side, its
    // rims, and its recesses, which it keeps. Convert reads none of these: every face it takes is a
    // panel, and a recess is an engraving only where the user picks its faces to engrave.
    let recognizing = matches!(faces, SheetFaces::Sheet { .. });
    if recognizing && !engrave.is_empty() {
        return err("Recognize keeps a body's recesses as they are: faces to engrave are Convert's");
    }
    let mut engraved = BTreeSet::new();
    for p in engrave {
        let g = face_at(*p, "engraved face")?;
        if skipped.contains(&g) {
            return err(format!("the face at {} is picked both to skip and to engrave", at(*p)));
        }
        if chosen.as_ref().is_some_and(|c| c.contains(&g)) {
            return err(format!("the face at {} is picked both for the sheet and to engrave", at(*p)));
        }
        engraved.insert(g);
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
    // Recognizing, behind face `f`: of the faces level with it that face back at it from inside
    // its material, the one it meets first over the most of its area (a shell's or a plate's other
    // side), and how far behind it lies; with `true`, that it faces back. Found from the faces'
    // surfaces and loops: nearest first, each face takes what of `f` the nearer ones left, so a
    // wall's inside face comes before the wall across the part, and a recess's floor in the other
    // side takes only its own small part.
    // Converting, no face has one: every face is a panel of the sheet, however near another faces
    // back at it, as a surface body's faces always are (it has no thickness of its own).
    let surface = mb.is_surface();
    let behinds: Vec<Option<(f64, usize)>> = (0..count)
        .map(|f| {
            let sf = probe(f).filter(|_| recognizing && !surface)?;
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

    // Recognizing, a face that another faces back at a small distance behind, as a shell's or a
    // plate's do: the sheet keeps that thickness and its material. (Converting, none: see
    // `behinds`.)
    let thin = |f: usize| behind(f).filter(|(d, _, back)| *back && *d > 0.0 && d * 6.0 <= extent(f)).map(|(d, ..)| d);
    // A face's area, exactly from its chart: a plane's chart is the plane, a cylinder's runs along
    // its axis and round it in radians. A cone's is no constant times its chart's: none.
    let face_area = |f: usize| -> f64 {
        let chart = charts.get(f).and_then(Option::as_ref).map_or(0.0, |(r, _)| r.area().abs());
        match probe(f) {
            Some(Shape::Plane { .. }) => chart,
            Some(Shape::Cylinder { radius, .. }) => chart * radius,
            _ => 0.0,
        }
    };
    // Recognizing, where the walk starts, and so which side of a shell or a plate is the sheet's
    // surface: the face given, else the largest face with another behind it; of two as large (a
    // plate's sides), the one facing most up, then most along y and x, then the one higher up;
    // and from there the larger of the body's two sides (below). Chosen from the picks and the
    // geometry, never from the order of the body's faces. Converting, the body is surfaces, which
    // take the rule's thickness: every face is the sheet's (the faces picked, with every face a
    // bend joins to them), and nothing depends on where the walk starts.
    let first_thin = match faces {
        SheetFaces::Sheet { face: Some(p), .. } => {
            let f = face_at(p, "surface face")?;
            match thin(f) {
                Some(d) => Some((f, d)),
                None => return err(format!("the face at {} has no face behind it a sheet's thickness away: Convert takes its surfaces", at(p))),
            }
        }
        SheetFaces::AllBut(_) | SheetFaces::Picked(_) => None,
        SheetFaces::Sheet { face: None, .. } => {
            let facing = |f: usize| {
                let p = face_point(&q, f).unwrap_or_default();
                let n = probe(f).map_or(Vec3::default(), |s| s.out(p));
                [n.z, n.y, n.x, p.z, p.y, p.x]
            };
            let larger = |x: usize, y: usize| {
                let (ax, ay) = (face_area(x), face_area(y));
                if (ax - ay).abs() > 1e-9 * ax.max(ay) {
                    ax.total_cmp(&ay)
                } else {
                    facing(x).partial_cmp(&facing(y)).unwrap_or(std::cmp::Ordering::Equal)
                }
            };
            (0..count).filter_map(|f| thin(f).map(|d| (f, d))).max_by(|x, y| larger(x.0, y.0))
        }
    };
    if recognizing && first_thin.is_none() {
        return err("not sheet metal: no face of the body has another facing back at it a small, even distance behind; Convert takes its surfaces");
    }
    let (start, sheet) = match (first_thin, &chosen) {
        (Some((f, d)), _) => (f, Some(d)),
        (None, Some(_)) => match first_pick {
            Some(f) => (f, None),
            None => return err("pick a face of the sheet"),
        },
        (None, None) => match (0..count).find(|f| probe(*f).is_some()) {
            Some(f) => (f, None),
            None => return err("not supported yet: a body with no face that is a plane, a cylinder or a cone"),
        },
    };
    let own = sheet.map(|d| (d * 1e9).round() / 1e9);
    // The rule; measuring, there is none, and the body's own thickness stands in for the rule's.
    let rule = match rule {
        Some(rule) => Some(rule(None)?),
        None => None,
    };
    let t = match (&rule, own) {
        (Some(rule), _) => rule.thickness,
        (None, Some(d)) => d,
        (None, None) => return err("not sheet metal: the body has no even thickness of its own"),
    };
    // The body as modelled: its own thickness where Recognize read one, else the rule's.
    let modelled_t = own.unwrap_or(t);
    let shapes: Vec<std::result::Result<Shape, &'static str>> = (0..count).map(|f| Shape::of(b, &q, f, outward && sheet.is_none(), tol)).collect();
    let shape = |f: usize| shapes.get(f).copied().unwrap_or(Err("a face the body does not have"));
    let smooth = |e: usize, x: &Shape, y: &Shape| -> bool {
        let m = middle_of(e).unwrap_or_default();
        x.out(m).dot(y.out(m)).clamp(-1.0, 1.0).acos() <= SMOOTH
    };

    // The faces of the sheet: converting, every face; recognizing, the side of the shell or the
    // plate the start is on. That side runs on smoothly, or across a sharp edge between two planes
    // that is no rim: a face's rims (its thickness faces) touch the face behind it. A wall's
    // outside that runs on in one plane with the base's edge touches it too, and is told from a
    // rim by standing the sheet's thickness in front of the face behind it, unless the face
    // crossed from meets that wall's inside as well: the inside is then the wall's face on its
    // side, and the outside (one face with the base's edge strips beside the wall) a rim from
    // there. A face whose face behind is the sheet's is the sheet's other side.
    let thick = |g: usize| sheet.is_some_and(|d| behind(g).is_some_and(|(e, _, back)| back && (e - d).abs() <= tol * 10.0));
    let rim = |f: usize, g: usize| -> bool {
        let Some((_, h, _)) = behind(f) else { return false };
        touches(g, h) && (!thick(g) || behind(g).is_some_and(|(_, inside, _)| touches(f, inside)))
    };
    // Recognizing, what each loop of a shell's or a plate's face opens onto (see `Opening`),
    // followed across the faces round it to the first faces level with it.
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
            let standing = levels.iter().filter(|(_, depth, _)| *depth < -near).map(|x| x.0);
            return Opening::Raised(*g, walls.into_iter().chain(standing).collect());
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
    // `f`'s panel once the sheet is built (see `sheet_surface_cuts`). A face of material standing
    // on `f` (a boss) is none either: the walk does not cross onto it, and `f`'s panel refuses it
    // as no sheet of even thickness.
    let recessed = |f: usize, g: usize| -> bool {
        openings.get(f).is_some_and(|ls| {
            ls.iter().any(|(_, what)| matches!(what, Opening::Recess(faces) | Opening::Raised(_, faces) if faces.contains(&g)))
        })
    };
    // The side a walk from face `from` reaches, crossing onto none of the faces in `rims`.
    let reach = |from: usize, rims: &BTreeSet<usize>| -> BTreeSet<usize> {
        let mut side = BTreeSet::from([from]);
        let mut queue = VecDeque::from([from]);
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
                    let other_side = behind(g).is_some_and(|h| side.contains(&h.1));
                    match (shape(f), shape(g)) {
                        (Ok(sf), Ok(sg)) => {
                            !rim && !other_side
                                && !rims.contains(&g)
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
        side
    };
    // The side a walk from face `from` reaches. A face that a face of the side, one with the
    // sheet's thickness behind it, has for a rim is no face of the side, whichever face the walk
    // meets it from first: a plate's end, a rim of its face, is met as well from a wall's outside
    // that runs on in one plane with the plate's edge (as a bracket's does), which takes it for
    // none, its own face behind lying elsewhere. So the walk is taken again with such faces left
    // out, until the side holds none.
    let walk = |from: usize| -> BTreeSet<usize> {
        let mut rims: BTreeSet<usize> = BTreeSet::new();
        loop {
            let side = reach(from, &rims);
            if sheet.is_none() {
                return side;
            }
            let more: Vec<usize> = side
                .iter()
                .copied()
                .filter(|g| *g != from && side.iter().any(|k| k != g && thick(*k) && touches(*k, *g) && rim(*k, *g)))
                .collect();
            if more.is_empty() {
                return side;
            }
            rims.extend(more);
        }
    };
    let mut side = walk(start);
    // Recognizing with no face given, a shell's or a plate's surface is the larger of its two
    // sides, the outside of its bends (by `face_area`, a cone's left out); of two as large (a
    // plate's), the side of its largest face, where the walk started.
    let mut start = start;
    let given = matches!(faces, SheetFaces::Sheet { face: Some(_), .. });
    if let (false, Some(_), Some((_, h, true))) = (given, sheet, behind(start)) {
        let size = |s: &BTreeSet<usize>| -> f64 { s.iter().map(|f| face_area(*f)).sum() };
        let other = walk(h);
        if size(&other) > size(&side) * (1.0 + 1e-9) {
            (start, side) = (h, other);
        }
    }
    // Recognizing, every face of the side has its other side the sheet's thickness behind it, or
    // none: a body of walls of two thicknesses is no one sheet.
    if let (true, Some(d)) = (recognizing, sheet) {
        let mut found: Vec<f64> =
            side.iter().filter_map(|g| behind(*g)).filter(|(e, _, back)| *back && (e - d).abs() > tol * 10.0).map(|(e, ..)| e).collect();
        if !found.is_empty() {
            found.push(d);
            found.sort_by(f64::total_cmp);
            found.dedup_by(|x, y| (*x - *y).abs() <= tol * 10.0);
            let list: Vec<String> = found.iter().map(|e| format!("{e:.3}")).collect();
            return err(format!("walls {} mm thick: not one sheet; Convert takes its surfaces", list.join(" and ")));
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
    // The planes a cylinder meets smoothly along its edges.
    let planes_beside = |g: usize| -> BTreeSet<usize> {
        let Ok(sg) = shape(g) else { return BTreeSet::new() };
        (0..edge_count)
            .filter_map(|e| {
                let (x, y) = pair(e)?;
                let h = if x == g && y != g {
                    y
                } else if y == g && x != g {
                    x
                } else {
                    return None;
                };
                let sh = shape(h).ok()?;
                (matches!(sh, Shape::Plane { .. }) && smooth(e, &sg, &sh)).then_some(h)
            })
            .collect()
    };
    // Faces `g` and `h` of one cylinder between the same two planes: pieces of one bend that
    // cut-outs across it parted.
    let one_bend = |g: usize, h: usize| -> bool {
        let Ok(Shape::Cylinder { origin: o1, axis: a1, radius: r1, inward: i1, .. }) = shape(g) else { return false };
        let Ok(Shape::Cylinder { origin: o2, axis: a2, radius: r2, inward: i2, .. }) = shape(h) else { return false };
        let off = (o2 - o1) - a1 * (o2 - o1).dot(a1);
        let planes = planes_beside(g);
        let coaxial = a1.cross(a2).len() <= 1e-9 && off.len() <= tol * 10.0;
        coaxial && (r1 - r2).abs() <= tol * 10.0 && i1 == i2 && planes.len() == 2 && planes == planes_beside(h)
    };
    // A pick on one piece of a bend that cut-outs across it split bends every piece.
    let more: Vec<usize> = side.iter().copied().filter(|h| is_roll(*h) && !rolled.contains(h) && rolled.iter().any(|g| one_bend(*g, *h))).collect();
    rolled.extend(more);
    let (bent, rolled) = (bent, rolled);

    // With no faces picked, the sheet is that whole side (converting, every face). With faces
    // picked, it is those, the rolls of modelled bends with the faces they run smoothly into, and
    // every face a bend joins to them.
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
    sheet_faces.retain(|f| !skipped.contains(f) && !engraved.contains(f));
    let mut shaped: BTreeMap<usize, Shape> = BTreeMap::new();
    for &f in &sheet_faces {
        match shape(f) {
            Ok(s) => shaped.insert(f, s),
            Err(why) => {
                return err(format!("the face at {} can't be a panel: {why}. Skip it to leave it out", at(face_point(&q, f).unwrap_or_default())));
            }
        };
    }

    // Converting, the engravings the user picked (#6748), by the panel each is cut into: the edges
    // of the hole in the panel's face it opens from, and its floors, each as deep as it lies
    // behind the face and drawn in the panel's chart. The faces picked to engrave are no panels,
    // and the hole they open from is no hole of the sheet: the panel keeps that material, less the
    // recess. Read exactly from the faces: each floor is level with the panel's face, facing its
    // way, behind it and short of the rule's thickness; each wall is square to the face; a face
    // level with the panel's face (an island, as in an O) is the panel's surface, and nothing is
    // cut there. A cut whose faces are not picked stays panels: it may be a pocket the design means.
    let mut engravings: BTreeMap<usize, Vec<Engraving>> = BTreeMap::new();
    let mut reached: BTreeSet<usize> = BTreeSet::new();
    for (&f, sf) in shaped.iter().filter(|_| !engraved.is_empty()) {
        let (Some(face), Some(Some((region, uses)))) = (q.record().faces.get(f), charts.get(f)) else { continue };
        let point = at(face_point(&q, f).unwrap_or_default());
        for l in &face.loops {
            let Bounds::Edges { edges } = l else { continue };
            let edges: Vec<usize> = edges.iter().map(|oe| oe.edge as usize).collect();
            let beside: Vec<usize> = edges.iter().filter_map(|e| pair(*e)).flat_map(|(x, y)| [x, y]).filter(|g| *g != f).collect();
            if !beside.iter().any(|g| engraved.contains(g)) {
                continue;
            }
            // Its outer loop is the panel's outline: an engraving there runs off the face.
            let first = edges.first().and_then(|e| uses.iter().find(|u| u.edge == *e)).and_then(|u| u.chart.first().copied());
            if first.is_some_and(|c| region.outer.segs.iter().any(|s| s.point_at(s.closest_param(c).clamp(0.0, 1.0)).dist(c) <= tol * 100.0)) {
                return err(format!("the engraving in the face at {point} reaches the face's edge: not supported yet"));
            }
            if beside.iter().any(|g| !engraved.contains(g)) {
                return err(format!("the engraving in the face at {point} has walls not picked to engrave: pick its walls and its floor"));
            }
            let Shape::Plane { out: n, .. } = *sf else {
                return err(format!("not supported yet: an engraving in a roll, at {point}"));
            };
            let mut group = BTreeSet::new();
            let mut queue: VecDeque<usize> = beside.into_iter().collect();
            while let Some(g) = queue.pop_front() {
                if group.insert(g) {
                    queue.extend(around.get(g).into_iter().flatten().copied().filter(|h| engraved.contains(h)));
                }
            }
            let near = tol * 10.0;
            let mut floors = Vec::new();
            for &g in &group {
                let at_g = at(face_point(&q, g).unwrap_or_default());
                let unread = |why: String| DocError::Invalid(format!("the engraved face at {at_g} can't be read: {why}"));
                let sg = shape(g).map_err(|why| unread(why.to_string()))?;
                match level_of(sf, &sg, tol) {
                    // An island: the panel's own surface.
                    Some((depth, true)) if depth.abs() <= near => {}
                    Some((depth, true)) if depth > near && depth < t => {
                        let (twin, shift) = charted_on(sf, &sg).ok_or_else(|| unread("no chart on the face's plane".into()))?;
                        let (floor, _) = face_chart(b, &q, &twin, g, sag, tol).map_err(unread)?;
                        floors.push((depth, Rigid2 { cos: 1.0, sin: 0.0, shift }.region(&floor)));
                    }
                    Some((depth, true)) if depth > near => {
                        return err(format!(
                            "the engraving at {at_g} is {depth:.3} mm deep, the sheet {t:.3} mm thick: an engraving stops short of the sheet's thickness"
                        ));
                    }
                    Some((_, true)) => {
                        return err(format!("the engraved floor at {at_g} lies in front of the face at {point}, away from its material"));
                    }
                    Some((_, false)) => {
                        return err(format!("the engraved face at {at_g} faces back at the face at {point}: no floor of an engraving"));
                    }
                    None => {
                        let square = match sg {
                            Shape::Plane { out, .. } => out.dot(n).abs() <= 1e-9,
                            Shape::Cylinder { axis, .. } => axis.cross(n).len() <= 1e-9,
                            Shape::Cone { .. } => false,
                        };
                        if !square {
                            return err(format!(
                                "the engraved face at {at_g} is neither a floor level with the face at {point} nor a wall square to it: not supported yet"
                            ));
                        }
                    }
                }
            }
            if floors.is_empty() {
                return err(format!("the engraving in the face at {point} has no floor: a hole through is no engraving"));
            }
            reached.extend(group);
            engravings.entry(f).or_default().push((edges, floors));
        }
    }
    if let Some(g) = engraved.iter().find(|g| !reached.contains(g)) {
        let at_g = at(face_point(&q, *g).unwrap_or_default());
        return err(format!("the face at {at_g}, picked to engrave, is no face of a recess in a face of the sheet"));
    }

    // The bends modelled with their radius (see `Modelled`): a cylinder whose inside radius is
    // short of ROLL_FROM thicknesses, between two planes of the sheet that cross. Their cylinders
    // leave the sheet's faces.
    let mut modelled: Vec<Modelled> = Vec::new();
    for (&f, sf) in &shaped {
        let Shape::Cylinder { origin, axis, radius, inward, .. } = *sf else { continue };
        let inside = if inward { radius - modelled_t } else { radius };
        if !(inside >= 0.0 && inside < ROLL_FROM * modelled_t) {
            continue;
        }
        let mut sides: Vec<(usize, usize)> = Vec::new();
        for e in 0..edge_count {
            let Some((x, y)) = pair(e) else { continue };
            let g = if x == f && y != f {
                y
            } else if y == f && x != f {
                x
            } else {
                continue;
            };
            let Some(sg @ Shape::Plane { .. }) = shaped.get(&g) else { continue };
            let Some([t0, t1]) = q.edge_interval(e) else { continue };
            let (Some((p0, _)), Some((p1, _))) = (q.edge_at(e, t0), q.edge_at(e, t1)) else { continue };
            let along = (p1 - p0).normalized().is_some_and(|d| d.cross(axis).len() <= 1e-6);
            if along && smooth(e, sf, sg) && !sides.iter().any(|(h, _)| *h == g) {
                sides.push((g, e));
            }
        }
        let &[(a, ea), (bb, eb)] = sides.as_slice() else { continue };
        let (Some(Shape::Plane { plane: pa, out: na }), Some(Shape::Plane { plane: pb, out: nb })) = (shaped.get(&a), shaped.get(&bb)) else {
            continue;
        };
        if na.cross(*nb).len() <= 1e-9 {
            continue;
        }
        // Each plane's tangent line is the axis's foot on it; it runs on square across that line
        // to where the other plane crosses it, the sharp, as far as the cylinder runs along its
        // axis.
        let run_on = |plane: &Plane, n: Vec3, m: Vec3, o: Vec3| -> Option<(f64, Vec3)> {
            let foot = origin - n * (origin - plane.origin).dot(n);
            let w = n.cross(axis).normalized()?;
            let by = (o - foot).dot(m) / w.dot(m);
            by.is_finite().then(|| (by.abs(), foot + w * by))
        };
        let (Some((sa, on)), Some((sb, _))) = (run_on(pa, *na, *nb, pb.origin), run_on(pb, *nb, *na, pa.origin)) else { continue };
        let along: Vec<f64> = [ea, eb].iter().filter_map(|e| q.edge_points(*e, sag)).flatten().map(|(_, p)| (p - origin).dot(axis)).collect();
        let (lo, hi) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| (l.min(*x), h.max(*x)));
        if !(hi > lo) {
            continue;
        }
        let at = |x: f64| on + axis * (x - (on - origin).dot(axis));
        let (s0, s1) = (at(lo), at(hi));
        // The planes in order, so that the pieces of one bend line up.
        let sides = if a <= bb { [(a, vec![ea], sa), (bb, vec![eb], sb)] } else { [(bb, vec![eb], sb), (a, vec![ea], sa)] };
        let piece = Modelled { rolls: vec![f], sides, sharp: (s0, s1), inside };
        // Another piece of a bend already found (cut-outs across it parted them) joins it.
        let same = |m: &&mut Modelled| {
            m.rolls.first().is_some_and(|g| one_bend(*g, f)) && m.sides[0].0 == piece.sides[0].0 && m.sides[1].0 == piece.sides[1].0
        };
        match modelled.iter_mut().find(same) {
            Some(m) => m.join(piece),
            None => modelled.push(piece),
        }
    }
    for m in &modelled {
        for g in &m.rolls {
            shaped.remove(g);
            sheet_faces.remove(g);
        }
    }
    // Measuring: the body's own thickness, each modelled bend's inside radius, and the face the
    // walk started on, whose side is the sheet's surface.
    let Some(rule) = rule.as_ref() else {
        let radii = modelled.iter().map(|m| m.inside).collect();
        let surface = crate::apply_point(to_world, face_point(&q, start).unwrap_or_default());
        return Ok(Reading::Measured(SheetMeasure { thickness: t, radii, surface }));
    };

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
        // On a modelled bend: its cylinder, or an edge of it.
        let modelled_at = (0..edge_count)
            .filter(|e| on_edge(*p, *e))
            .filter_map(on_side_edge)
            .filter_map(pair)
            .any(|(x, y)| modelled.iter().any(|m| m.rolls.contains(&x) || m.rolls.contains(&y)));
        edge || modelled_at || q.faces_at(*p, near).first().and_then(|(_, g)| on_side(*g)).is_some_and(|g| rolled.contains(&g))
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
            for m in &modelled {
                let (x, y) = (m.sides[0].0, m.sides[1].0);
                let g = if x == f {
                    y
                } else if y == f {
                    x
                } else {
                    continue;
                };
                if left.remove(&g) {
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
    let mut recesses: Vec<crate::Recess> = Vec::new();
    for &f in &order {
        let Some(shape) = shaped.get(&f) else { continue };
        let point = face_point(&q, f).unwrap_or_default();
        places.push(shape.frame().ok_or_else(|| DocError::Invalid(format!("the face at {} has no frame", at(point))))?);
        let (mut region, uses) =
            face_chart(b, &q, shape, f, sag, tol).map_err(|e| DocError::Invalid(format!("the face at {} can't be a panel: {e}", at(point))))?;
        // Which of the region's loops a loop of the face is: the one its first edge runs on.
        let on = |l: &Loop2, edges: &[usize]| {
            let first = edges.first().and_then(|e| uses.iter().find(|u| u.edge == *e)).and_then(|u| u.chart.first().copied());
            first.is_some_and(|c| l.segs.iter().any(|s| s.point_at(s.closest_param(c).clamp(0.0, 1.0)).dist(c) <= tol * 100.0))
        };
        // An engraving picked in the face: the hole it opens from is no hole of the sheet, and
        // each of its floors is a recess of the panel from the sheet's surface, as deep as it lies.
        for (edges, floors) in engravings.get(&f).into_iter().flatten() {
            region.holes.retain(|h| !on(h, edges));
            let panel = panels.len();
            recesses.extend(floors.iter().map(|(depth, r)| crate::Recess { panel, region: r.clone(), far: false, depth: *depth }));
        }
        // A hole of a shell's or a plate's face opening onto a recess is no hole of the sheet:
        // the panel keeps that material. Material standing on the face (a boss) is no sheet of
        // even thickness.
        if let Some(loops) = openings.get(f) {
            for (edges, what) in loops {
                match what {
                    Opening::Raised(g, _) => {
                        let at_g = face_point(&q, *g).unwrap_or_default();
                        return err(format!("the face at {} is raised at {}: not a plate of even thickness", at(point), at(at_g)));
                    }
                    Opening::Recess(_) => region.holes.retain(|h| !on(h, edges)),
                    Opening::Through | Opening::Open => {}
                }
            }
        }
        // Along a modelled bend, the panel runs on to the bend's sharp.
        for (g, e, by) in modelled.iter().flat_map(|m| m.sides.iter().flat_map(|(g, es, by)| es.iter().map(move |e| (*g, *e, *by)))) {
            let Some(u) = uses.iter().find(|u| u.edge == e).filter(|_| g == f) else { continue };
            let (Some(&a), Some(&z)) = (u.chart.first(), u.chart.last()) else { continue };
            region = to_sharp(&region, (a, z), by, tol).map_err(|e| DocError::Invalid(format!("the face at {} {e}", at(point))))?;
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

    // Each modelled bend, at its sharp, with the rule's radius; the joint and the radius it was
    // modelled with are kept, to read the body as it is (see `SurfaceConversion::as_modelled`).
    let mut modelled_radii: Vec<(usize, f64)> = Vec::new();
    // Each modelled bend's joint.
    let mut bend_of: Vec<(usize, usize)> = Vec::new();
    for (mi, m) in modelled.iter().enumerate() {
        let (x, y) = (m.sides[0].0, m.sides[1].0);
        let (a, b) = if parent.get(&x) != Some(&y) { (x, y) } else { (y, x) };
        let (Some(&ia), Some(&ib), Some(Shape::Plane { plane: pa, out: na }), Some(Shape::Plane { plane: pb, out: nb })) =
            (index.get(&a), index.get(&b), shaped.get(&a), shaped.get(&b))
        else {
            continue;
        };
        let Some(ra) = panels.get(ia).map(|p| p.region.clone()) else { continue };
        // Run so that `a`'s material lies to the left: looked for beside the sharp where a piece
        // of the cylinder runs along it (a cut-out across the bend may leave none at its middle).
        let (mut s0, mut s1) = m.sharp;
        let (c0, c1) = (pa.to_local(s0), pa.to_local(s1));
        let extent = ra.outer.polyline(1e-3).bounds().size().len().max(1e-9);
        let on_sharp = |p: Vec3| {
            let d = s1 - s0;
            s0 + d * ((p - s0).dot(d) / d.dot(d).max(1e-300))
        };
        let first = m.sides.iter().find(|(g, ..)| *g == a).and_then(|(_, es, _)| es.first());
        let beside = first.and_then(|e| middle_of(*e)).map_or(s0.lerp(s1, 0.5), on_sharp);
        let mid = pa.to_local(beside);
        if !(c1 - c0).normalized().is_some_and(|d| ra.contains(mid + d.perp() * (extent * 1e-4))) {
            (s0, s1) = (s1, s0);
        }
        let (a0, a1, b0, b1) = (pa.to_local(s0), pa.to_local(s1), pb.to_local(s0), pb.to_local(s1));
        let (Some(da), Some(db)) = ((a1 - a0).normalized(), (b1 - b0).normalized()) else { continue };
        // The bend turns toward the material where `b` rises behind `a`'s face.
        let lift = panels.get(ib).map_or(0.0, |p| {
            let pts = p.region.outer.polyline(1e-3);
            (&pts).into_iter().map(|q| (pb.to_world(*q) - s0).dot(*na)).fold(0.0, |m: f64, h| if h.abs() > m.abs() { h } else { m })
        });
        let len = s0.dist(s1);
        let bend = Bend {
            angle: na.dot(*nb).clamp(-1.0, 1.0).acos(),
            radius: rule.bend_radius,
            toward_material: lift < 0.0,
            datum: Datum::Sharp,
            length: len,
            ends: ZoneEnds { near: (0.0, len), far: (0.0, len) },
            tails: (Tail::Keep, Tail::Keep),
            relief: (Relief::None, Relief::None),
        };
        modelled_radii.push((joints.len(), m.inside));
        bend_of.push((mi, joints.len()));
        joints.push(Joint {
            a: Anchor { panel: ia, origin: a0, dir: da },
            b: Anchor { panel: ib, origin: b0, dir: db },
            kind: JointKind::Bend(bend),
        });
        edges.push(s0.lerp(s1, 0.5));
    }

    // What of each modelled bend's zone no face of its cylinder covers: the cut-outs across it,
    // and at its ends whatever the body's corner left (a relief's bite, a torn end). Of the strip
    // the zone takes along its sharp, each face of the cylinder is read in the cylinder's own
    // parameters (along its axis, the angle round it) and laid into the flat chart of the bend's
    // `a` panel exactly as the development lays the zone beside it: along the sharp as the bend
    // runs, and across from that panel's tangent line by the angle turned at the rule's neutral
    // radius. Each piece left is kept with the ends of the strip it reaches.
    let mut uncovered: Vec<(usize, Region2, bool, bool)> = Vec::new();
    for &(mi, j) in &bend_of {
        let (Some(m), Some(Joint { a: an, kind: JointKind::Bend(bend), .. })) = (modelled.get(mi), joints.get(j)) else { continue };
        let (Some(place), Some(&face_a)) = (places.get(an.panel), order.get(an.panel)) else { continue };
        let (Some((_, on_a, _)), Some((_, on_b, _))) = (m.sides.iter().find(|(g, ..)| *g == face_a), m.sides.iter().find(|(g, ..)| *g != face_a))
        else {
            continue;
        };
        let (o3, d3) = (place.point(an.origin.extend(0.0)), place.vector(an.dir.extend(0.0)));
        let toward_b = Vec2::new(an.dir.y, -an.dir.x);
        let (setback, width, neutral) = (bend.setback(t), bend.allowance(t, rule.k_factor), bend.radius + rule.k_factor * t);
        let (origin2, dir2, len) = (an.origin, an.dir, bend.length);
        let lay = |along: f64, across: f64| origin2 + dir2 * along + toward_b * across;
        let strip = Region2 {
            outer: Loop2::polygon(&[lay(0.0, -setback), lay(len, -setback), lay(len, width - setback), lay(0.0, width - setback)]).ccw(),
            holes: Vec::new(),
        };
        let mut left = vec![strip];
        for (k, &f) in m.rolls.iter().enumerate() {
            let Ok(sf @ Shape::Cylinder { origin, axis, .. }) = shape(f) else { continue };
            let (Some(&ea), Some(&eb)) = (on_a.get(k), on_b.get(k)) else { continue };
            let (region, uses) =
                face_chart(b, &q, &sf, f, sag, tol).map_err(|e| DocError::Invalid(format!("a modelled bend's face can't be read: {e}")))?;
            // The angle of the face's edge along each plane.
            let angle = |e: usize| {
                let pts = &uses.iter().find(|u| u.edge == e)?.chart;
                (!pts.is_empty()).then(|| pts.iter().map(|c| c.y).sum::<f64>() / pts.len() as f64)
            };
            let (Some(va), Some(vb)) = (angle(ea), angle(eb)) else { continue };
            let turn = if vb >= va { 1.0 } else { -1.0 };
            let (base, rate) = ((origin - o3).dot(d3), axis.dot(d3));
            let map = |c: Vec2| lay(base + rate * c.x, -setback + turn * (c.y - va) * neutral);
            let laid = |l: &Loop2| -> Option<Loop2> {
                let segs = l.segs.iter().map(|s| (!matches!(s, Seg2::Arc { .. })).then(|| s.map_controls(map))).collect::<Option<Vec<_>>>()?;
                Some(Loop2 { segs })
            };
            let (Some(outer), Some(holes)) = (laid(&region.outer), region.holes.iter().map(laid).collect::<Option<Vec<_>>>()) else {
                return err(format!("not supported yet: a round edge in the face of the bend at {}", at(o3)));
            };
            let piece = Region2 { outer: outer.ccw(), holes: holes.into_iter().map(|h| h.ccw().reversed()).collect() };
            left = solidcraft_kernel::planar_regions_boolean(&left, std::slice::from_ref(&piece), solidcraft_kernel::BoolOp::Cut, tol)?;
        }
        for r in left.into_iter().filter(|r| r.area() > tol * tol) {
            let x = |p: Vec2| (p - origin2).dot(dir2);
            let reaches = |end: f64| r.outer.segs.iter().any(|s| (x(s.start()) - end).abs() <= tol * 10.0);
            let (at_start, at_end) = (reaches(0.0), reaches(len));
            uncovered.push((j, r, at_start, at_end));
        }
    }

    let mut model = Sheet { thickness: t, k_factor: rule.k_factor, panels, joints, corners: Vec::new(), cuts: Vec::new() };
    // Each corner where two bends standing on one panel meet, closed as a flanged sheet's is (see
    // `close_corners_on`): both bends run to the corner, the panels beyond them meet on the line
    // their surfaces share there, and a rip of the rule's gap parts them; settled, the corner
    // then takes the rule's relief. Only a corner the body left open closes: panels that meet
    // already (a shell's walls), or are joined to others there (five faces round a corner), stay
    // as they are, and so does a corner whose panels' sides at it are no straight edges from
    // their bends.
    let framed: Vec<Option<Rigid3>> = places.iter().copied().map(Some).collect();
    let made = model.joints.len();
    let ends_of = |joint: &Joint| match &joint.kind {
        JointKind::Bend(bend) => Some((joint.a.origin, joint.a.origin + joint.a.dir * bend.length)),
        _ => None,
    };
    let before: Vec<Option<(Vec2, Vec2)>> = model.joints.iter().map(ends_of).collect();
    let body_joints = model.joints.clone();
    for j in 0..made {
        if before.get(j).copied().flatten().is_some() {
            crate::sheet_surface_features::close_corners_on(&mut model, &framed, rule.gap, j, Some(&body_joints), &|_, _| false)?;
        }
    }
    // A point on each new rip's edge, as each joint has.
    for joint in model.joints.iter().skip(made) {
        let mid = match &joint.kind {
            JointKind::Rip(rip) => rip.edges.0.first().zip(rip.edges.0.last()).map(|(p, q)| p.lerp(*q, 0.5)),
            _ => None,
        };
        let place = places.get(joint.a.panel);
        edges.push(mid.zip(place).map_or(Vec3::default(), |(p, place)| place.point(p.extend(0.0))));
    }
    // What the body's corner left at a bend's end is the sheet's where the corner stays as
    // modelled, and the rule's relief's where it was closed.
    for (j, region, at_start, at_end) in uncovered {
        let (Some(Some((s0, s1))), Some(now)) = (before.get(j), model.joints.get(j).and_then(ends_of)) else { continue };
        let closed = |was: Vec2, is: Vec2| was.dist(is) > tol;
        if (at_start && closed(*s0, now.0)) || (at_end && closed(*s1, now.1)) {
            continue;
        }
        let Some(panel) = model.joints.get(j).map(|x| x.a.panel) else { continue };
        model.cuts.push(Cut { panel, region });
    }
    let reliefs = Reliefs::of(rule);
    let mut notes = settle(&mut model, &places, &reliefs, &|_| None, &|_, _| false, tol, &at)?;
    // Where the rule's tooling changes the body, say so.
    if let Some(own) = own.filter(|own| (own - t).abs() > tol) {
        notes.insert(0, format!("the body is {own:.3} mm thick: the sheet is the rule's {t:.3} mm"));
    }
    for (j, inside) in &modelled_radii {
        if (inside - rule.bend_radius).abs() > tol {
            let p = edges.get(*j).copied().unwrap_or_default();
            notes.push(format!("the corner at {} modelled R {inside:.3} bends at the rule's R {:.3}", at(p), rule.bend_radius));
        }
    }
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
    let development = develop(&model, &made_anchors(&model, &places)).map_err(|e| DocError::Invalid(e.to_string()))?;
    if !development.loops.is_empty() {
        let names: Vec<String> = development.loops.iter().filter_map(|m| edges.get(m.joint)).map(|p| at(*p)).collect();
        return err(format!(
            "the edge at {} closes a ring of bends and seams: leave one of the ring's edges out of the bends to cut it open",
            names.join(" and the one at ")
        ));
    }
    // A panel its rips leave no material (#6748): each rip cuts it back from its edge by the
    // rule's mitre, and where those cuts pass each other (a face narrower than the rule's
    // thickness taken in from both its edges, as a thin block's sides under a thick rule) the
    // outline the development draws for it turns inside out. Read from that outline exactly, its
    // signed area from its own lines and arcs: none, or less, is no material. Refused by name
    // before any solid is sewn from it. A planar panel's outline runs counter-clockwise in the
    // flat as in its chart; a roll's is not read here.
    let planar = |i: usize| model.panels.get(i).is_some_and(|p| p.surface == Surface::Plane);
    let gone: Vec<String> = development
        .panels
        .iter()
        .enumerate()
        .filter(|(i, flat)| planar(*i) && flat.as_ref().is_some_and(|f| f.region.outer.signed_area() <= 0.0))
        .map(|(i, _)| format!("the panel from the face at {}", at(order.get(i).and_then(|f| face_point(&q, *f)).unwrap_or_default())))
        .collect();
    if let [rest @ .., last] = gone.as_slice() {
        let list = if rest.is_empty() { last.clone() } else { format!("{} and {last}", rest.join(", ")) };
        return err(format!(
            "{list}: the rips round each leave it no material, the rule's {t:.3} mm mitred in from its edges past each other. Skip those faces, or use a thinner rule"
        ));
    }
    let mut conversion = SurfaceConversion {
        sheet: model,
        development,
        faces: order,
        places,
        edges,
        thickness: t,
        notes,
        reliefs,
        own_thickness: own,
        recesses,
        modelled_radii,
        rule_radius: rule.bend_radius,
        pieces: std::sync::OnceLock::new(),
    };
    // Converting, each face takes the rule's thickness on its material's side, whatever lies
    // behind it, so two panels' material may meet (a thin block's two sides, each the rule's
    // thickness in): refused, naming them, from the folded parts themselves (see
    // `SurfaceConversion::overlap`). Recognize reads one side of a sheet of one thickness.
    if !recognizing {
        let (shared, unchecked) = conversion.overlap()?;
        let name = |p: Part| match p {
            Part::Panel(i) => {
                let face = conversion.faces.get(i).and_then(|f| face_point(&q, *f));
                format!("the panel from the face at {}", at(face.unwrap_or_default()))
            }
            Part::Bend(j) => format!("the bend at {}", at(conversion.edges.get(j).copied().unwrap_or_default())),
        };
        if let Some((x, y, m)) = shared {
            return err(format!(
                "{} and {} overlap at {}: each is the rule's {t:.3} mm behind its face, and their material meets there. Skip one of the faces, or convert under a thinner rule",
                name(x),
                name(y),
                at(m)
            ));
        }
        let said: Vec<String> =
            unchecked.into_iter().map(|(x, y, why)| format!("whether {} and {} overlap could not be worked out: {why}", name(x), name(y))).collect();
        conversion.notes.extend(said);
    }
    Ok(Reading::Converted(Box::new(conversion)))
}

/// An engraving Convert keeps (see [`read_surfaces`]): the edges of the hole in its panel's face
/// it opens from, and its floors, each as deep as it lies behind the face, in the panel's chart.
type Engraving = (Vec<usize>, Vec<(f64, Region2)>);

/// A part of a folded sheet, as [`SurfaceConversion::overlap`] weighs it: a panel with the tabs
/// that ride with it, or the zone of a bend (by its joint).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Panel(usize),
    Bend(usize),
}

/// What [`SurfaceConversion::overlap`] finds: the first two parts that share material, with the
/// middle of what they share; and each pair whose share could not be worked out, with why.
type Overlaps = (Option<(Part, Part, Vec3)>, Vec<(Part, Part, String)>);

/// Settles where each bend of `sheet` ends and what is cut there, from where `places` put its
/// panels in space. Where two bends standing on one panel meet at a corner of it, both end where
/// their tangent lines on it cross, with the corner's relief (`chosen` for the two bend joints,
/// else the rule's): a notch, a round or a square relief belonging to the corner, or a tear tight
/// against the rip that parts their other panels. Where a rip cuts a panel the bend joins, its
/// metal ends at the rip's plane, kept off it as a mitre keeps its panels; so does a notched end at
/// a corner a third bend meets, where a rip parts its flange from the third's (side flanges across
/// a bend). Where the stretches of a split edge meet, each bend's end there butts the other's
/// (`butt` says which ends do, by the bend's joint and whether the end is at its anchors' length or
/// their origin, see [`SurfaceSheet::junctions`]): it is no corner, ends square across its joint
/// and is cut nothing. Elsewhere the rule's bend relief ([`Reliefs::bend_end`]) is cut in
/// whichever panel runs on past the end. A bend's edge is its anchors' first [`Bend::length`], so
/// a sheet settled again ends its bends where it did; the corners are the sheet's again. Why any
/// relief asked for was not cut comes back; `at` names a point in what is said.
pub(crate) fn settle(
    sheet: &mut Sheet,
    places: &[Rigid3],
    reliefs: &Reliefs,
    chosen: &dyn Fn([usize; 2]) -> Option<(CornerRelief, f64)>,
    butt: &dyn Fn(usize, bool) -> bool,
    tol: f64,
    at: &dyn Fn(Vec3) -> String,
) -> Result<Vec<String>> {
    let t = sheet.thickness;
    let (bent, rips) = in_space(sheet, places);
    // Where each bend ends, and what is cut there: (along its `a` anchor, along its `b` anchor,
    // the relief), at its start and at its end.
    let near = |p: Vec3, q: Vec3| p.dist(q) <= tol * 10.0;
    let slot = reliefs.bend_end();
    let mut ends: Vec<[(f64, f64, Relief); 2]> = bent.iter().map(|e| [(0.0, 0.0, slot), (e.len, e.len, slot)]).collect();
    let (mut notes, mut corners) = (Vec::new(), Vec::new());
    // Whether bend `i`'s end `e` (0 at its anchors' origin, 1 at their length) butts the next
    // bend of a split edge.
    let butts = |i: usize, e: usize| bent.get(i).is_some_and(|b| butt(b.joint, e == 1));
    // No corner is made where two bends of a split edge butt.
    let found: Vec<BendCorner> = bend_corners(&bent, near).into_iter().filter(|k| !k.ends.iter().any(|&(i, e)| butts(i, e))).collect();
    // Which bend ends a corner has settled already.
    let mut settled: Vec<[bool; 2]> = vec![[false; 2]; bent.len()];
    // How far bend `b`'s end `e` is cut back from its corner to keep its metal off the rips that
    // end there (see `bend_cut`), of those `side` takes (1 where the bend keeps the rip's `a` side,
    // -1 its `b` side, from the rip's two panels). A rip that carries on along the bend's edge,
    // tangent to it, runs alongside the bend rather than across its end, and cuts nothing off it.
    // A mitre keeps the bend's metal as far off its plane as the panels it parts (two returns
    // mitred at a corner, their bends meeting there, stay its gap apart).
    let rip_cut = |b: &BendEdge, e: usize, side: &dyn Fn(usize, usize) -> Option<f64>| -> Option<f64> {
        let d = (b.ends[1] - b.ends[0]).normalized()?;
        let (v, into) = if e == 0 { (b.ends[0], d) } else { (b.ends[1], -d) };
        rips.iter()
            .filter_map(|(r, ends, planes, ways)| {
                let end = ends.iter().position(|p| near(*p, v))?;
                if ways.get(end).is_some_and(|w| w.dot(into).abs() > ALONGSIDE) {
                    return None;
                }
                let m = planes.get(end).copied().flatten()?;
                let rip = sheet.joints.get(*r)?;
                let gap = match &rip.kind {
                    JointKind::Rip(Rip { lap: Lap::Miter { gap }, .. }) => *gap,
                    _ => 0.0,
                };
                Some((m * side(rip.a.panel, rip.b.panel)?, gap))
            })
            .filter_map(|(m, gap)| bend_cut(v, into, m, b.normals, b.toward, b.radius, t, gap))
            .reduce(f64::max)
    };
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
        // Each bend's far panel round the corner (its flange). Where three bends meet (a flange on
        // a wall's end edge, the wall's bend, and a flange beside the wall), a notched end also
        // keeps its metal off a rip there that parts its own flange from a panel other than its
        // partner's, as a free end does. At a tray's corner the rip parts the two bends' own
        // flanges, and the ends stay as the corner leaves them.
        let walls = k.ends.map(|(i, _)| bent.get(i).and_then(|b| b.other(k.panel)).map(|(an, _)| an.panel));
        for (n, ((i, e), mut value)) in k.ends.iter().zip(values).enumerate() {
            if let (Relief::Notch, Some(b), Some(Some(wall))) = (value.2, bent.get(*i), walls.get(n).copied()) {
                let partner = walls.get(1 - n).copied().flatten();
                let side = |ra: usize, rb: usize| {
                    if ra == wall && Some(rb) != partner {
                        Some(1.0)
                    } else if rb == wall && Some(ra) != partner {
                        Some(-1.0)
                    } else {
                        None
                    }
                };
                if let Some(s) = rip_cut(b, *e, &side) {
                    let a = if *e == 0 { value.0.max(s) } else { value.0.min(b.len - s) };
                    value = (a, a, Relief::Notch);
                }
            }
            // A bend's end in two corners (where three bends meet) takes the most cut back of
            // the two, notched: the same whichever corner is met first.
            if let (Some(slot), Some(seen)) = (ends.get_mut(*i).and_then(|x| x.get_mut(*e)), settled.get_mut(*i).and_then(|x| x.get_mut(*e))) {
                *slot = if *seen && *slot != value {
                    let back = |x: f64, y: f64| if *e == 0 { x.max(y) } else { x.min(y) };
                    let a = back(back(slot.0, slot.1), back(value.0, value.1));
                    (a, a, Relief::Notch)
                } else {
                    value
                };
                *seen = true;
            }
        }
    }
    for (i, b) in bent.iter().enumerate() {
        if (b.ends[1] - b.ends[0]).normalized().is_none() {
            continue;
        }
        for e in 0..2 {
            if found.iter().any(|k| k.ends.contains(&(i, e))) {
                continue;
            }
            // Where the stretches of a split edge meet, the bend butts the next one: its end runs
            // square across the joint, where the edge's stretches meet, with no relief (each panel
            // keeps or trims what lies past it as its tail says).
            if butts(i, e) {
                let end = if e == 0 { 0.0 } else { b.len };
                if let Some(slot) = ends.get_mut(i).and_then(|x| x.get_mut(e)) {
                    *slot = (end, end, Relief::None);
                }
                continue;
            }
            // The rips that cut either of the bend's panels.
            let mine = |p: usize| p == b.a.panel || p == b.b.panel;
            let side = |ra: usize, rb: usize| {
                if mine(ra) {
                    Some(1.0)
                } else if mine(rb) {
                    Some(-1.0)
                } else {
                    None
                }
            };
            if let Some(s) = rip_cut(b, e, &side) {
                let a = if e == 0 { s } else { b.len - s };
                if let Some(slot) = ends.get_mut(i).and_then(|x| x.get_mut(e)) {
                    *slot = (a, a, Relief::Notch);
                }
            } else if let Some(Joint { kind: JointKind::Bend(bend), .. }) = sheet.joints.get(b.joint) {
                // A free end: the zone carries on each panel's edge that runs through it.
                let end = if e == 0 { 0.0 } else { b.len };
                let (n, f) = outline_ends(sheet, b, bend, end, tol);
                if let (Some(slot), true) = (ends.get_mut(i).and_then(|x| x.get_mut(e)), n.is_some() || f.is_some()) {
                    slot.0 = n.unwrap_or(end);
                    slot.1 = f.unwrap_or(end);
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

/// Where the flat `dev` of `sheet` is cut along the `junctions` of split edges (see
/// [`SurfaceSheet::slits`]): at each junction whose two bends turn to opposite sides, along the
/// line the two zones' ends share there, from the deeper of the bends' tangent lines on the panel
/// they stand on out to the nearer of their flanges' far edges (each flange's farthest corner on
/// that line). Past that only the longer flange's material lies on the line: nothing of the other
/// is left to part.
fn junction_slits(sheet: &Sheet, dev: &Development, junctions: &[[usize; 2]]) -> Vec<[Vec2; 2]> {
    let mut out = Vec::new();
    for &[ends, starts] in junctions {
        let bend = |j: usize| match sheet.joints.get(j) {
            Some(Joint { b, kind: JointKind::Bend(bend), .. }) => Some((*b, bend.toward_material, bend.length)),
            _ => None,
        };
        let zone = |j: usize| dev.zones.iter().find(|z| z.joint == j);
        let (Some((bx, toward_x, length)), Some((by, toward_y, _)), Some(zx), Some(zy)) = (bend(ends), bend(starts), zone(ends), zone(starts))
        else {
            continue;
        };
        if toward_x == toward_y {
            continue;
        }
        // The one bend ends at the junction and the other starts there: the line across from the
        // panel they stand on toward their flanges.
        let (near_x, near_y) = (zx.near[1], zy.near[0]);
        let Some(n) = (zx.far[1] - near_x).normalized() else { continue };
        // How far out along that line a flange reaches: its panel's farthest corner on it.
        let reach = |an: &Anchor, at: f64| -> Option<f64> {
            let place = dev.placements.get(an.panel).copied().flatten()?;
            let corner = an.origin + an.dir * at;
            let region = &sheet.panels.get(an.panel)?.region;
            region.outer.segs.iter().map(Seg2::start).filter(|v| (*v - corner).dot(an.dir).abs() <= 1e-9).map(|v| place.point(v).dot(n)).reduce(f64::max)
        };
        let (Some(far_x), Some(far_y)) = (reach(&bx, length), reach(&by, 0.0)) else { continue };
        let (start, end) = (near_x.dot(n).min(near_y.dot(n)), far_x.min(far_y));
        if end > start + 1e-9 {
            let along = near_x.dot(n);
            out.push([near_x + n * (start - along), near_x + n * (end - along)]);
        }
    }
    out
}

/// The stretches of the cut `[a, b]` that lie in the material of the flat `regions`: the line split
/// where it crosses their loops, each piece kept where its middle lies inside an outline and in
/// none of its holes. A notch through a junction's far edge leaves the cut from the bend line out
/// to the notch's edge; where it takes the whole line, no cut is left.
fn in_material(regions: &[Region2], [a, b]: [Vec2; 2]) -> Vec<[Vec2; 2]> {
    let line = [Seg2::Line { a, b }];
    let mut at = vec![0.0, 1.0];
    for l in regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)) {
        at.extend(solidcraft_develop::crossings(&line, &l.segs).iter().map(|c| c.1));
    }
    at.sort_by(f64::total_cmp);
    at.dedup();
    let holds = crate::sheet_surface_cuts::holds;
    let inside = |p: Vec2| regions.iter().any(|r| holds(&r.outer, p) && !r.holes.iter().any(|h| holds(h, p)));
    let mut out: Vec<[Vec2; 2]> = Vec::new();
    for w in at.windows(2) {
        let &[t0, t1] = w else { continue };
        if t1 <= t0 || !inside(a.lerp(b, (t0 + t1) / 2.0)) {
            continue;
        }
        let (p, q) = (a.lerp(b, t0), a.lerp(b, t1));
        match out.last_mut() {
            Some(last) if last[1] == p => last[1] = q,
            _ => out.push([p, q]),
        }
    }
    out
}

/// Whether the end of the bend of joint `joint` at its anchors' length (`at_end`), or at their
/// origin, butts the next bend of a split edge, of the pairs `junctions` (see
/// [`SurfaceSheet::junctions`]).
pub(crate) fn butts(junctions: &[[usize; 2]], joint: usize, at_end: bool) -> bool {
    junctions.iter().any(|&[ends, starts]| if at_end { ends == joint } else { starts == joint })
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

    /// Where two parts of the folded sheet share material (see [`Part`]): the first two found, in
    /// the parts' order, and the middle of what they share; and each pair whose share the kernel
    /// could not work out, with why. Each part is its pieces of the flat carried into space through
    /// the sheet's thickness, as the folded solid has them (`sheet_surface_fold::solid`); every two
    /// are intersected exactly and what they share weighed, as `inspect.interference` weighs it. No
    /// sampling and no rule of thumb: parts that only touch (a rip's two panels, kept its gap apart
    /// or none) share no volume. A panel and the zone of its own bend lie either side of the bend's
    /// tangent line on it (a bend turns at most half a turn), so that pair is not weighed.
    fn overlap(&self) -> Result<Overlaps> {
        let mut parts: BTreeMap<Part, Vec<(Piece, Region2)>> = BTreeMap::new();
        for (piece, region) in self.pieces()? {
            let part = match *piece {
                Piece::Panel(i) | Piece::Tab(_, i) => Part::Panel(i),
                Piece::Zone(z) => match self.development.zones.get(z) {
                    Some(zone) => Part::Bend(zone.joint),
                    None => continue,
                },
            };
            parts.entry(part).or_default().push((*piece, region.clone()));
        }
        if parts.len() < 2 {
            return Ok((None, Vec::new()));
        }
        let placed: Vec<Option<Rigid3>> = self.places.iter().copied().map(Some).collect();
        let folded = fold_onto(&self.sheet, &self.development, &placed)?;
        let mut solids = Vec::with_capacity(parts.len());
        for (part, pieces) in parts {
            let made = crate::sheet_surface_fold::solid(&self.development, &folded, self.thickness, &pieces, &mut FlatWork::new(), &|_, _| None)?;
            solids.push((part, made.0));
        }
        let own_bend = |a: Part, b: Part| match (a, b) {
            (Part::Panel(i), Part::Bend(j)) | (Part::Bend(j), Part::Panel(i)) => {
                self.sheet.joints.get(j).is_some_and(|x| x.a.panel == i || x.b.panel == i)
            }
            _ => false,
        };
        let mut unchecked = Vec::new();
        for (k, (a, sa)) in solids.iter().enumerate() {
            for (b, sb) in solids.iter().skip(k + 1) {
                if own_bend(*a, *b) {
                    continue;
                }
                let shared = match solidcraft_kernel::boolean(sa, sb, solidcraft_kernel::BoolOp::Intersect) {
                    Ok(Some(o)) => o,
                    Ok(None) => continue,
                    Err(e) => {
                        unchecked.push((*a, *b, e.to_string()));
                        continue;
                    }
                };
                let m = match solidcraft_kernel::measure_mass(&shared) {
                    Ok(m) => m,
                    Err(e) => {
                        unchecked.push((*a, *b, e.to_string()));
                        continue;
                    }
                };
                // A rounding of the measure (the one `inspect.interference` allows), so that parts
                // touching face to face share none: not a tolerance on where the parts lie.
                if m.volume > 1e-9 * sa.size().max(sb.size()).max(1.0).powi(3) {
                    return Ok((Some((*a, *b, m.centroid)), unchecked));
                }
            }
        }
        Ok((None, unchecked))
    }

    /// The folded sheet: its solid, sewn from the flat's pieces carried into space.
    pub fn solid(&self) -> Result<Body> {
        let places: Vec<Option<Rigid3>> = self.places.iter().copied().map(Some).collect();
        let folded = fold_onto(&self.sheet, &self.development, &places)?;
        Ok(crate::sheet_surface_fold::solid(&self.development, &folded, self.thickness, self.pieces()?, &mut FlatWork::new(), &|_, _| None)?.0)
    }

    /// [`Self::solid`], with what each face was made for (see [`crate::sheet::FaceNamer`]) and
    /// its sketch frame (see [`cap_frame`]), the panels keyed as `sk`'s sheet keys them (the
    /// conversion's own, in the same order) and made as its recipes say, less `sk`'s recesses (the
    /// engravings Convert keeps), each from its panel where the fold puts it.
    pub(crate) fn parted_solid(&self, sk: &SurfaceSheet) -> Result<(Body, Vec<Option<SheetFace>>, Vec<Option<Plane>>)> {
        let places: Vec<Option<Rigid3>> = self.places.iter().copied().map(Some).collect();
        let folded = fold_onto(&self.sheet, &self.development, &places)?;
        let pieces = self.pieces()?;
        let namer = crate::sheet::FaceNamer::new(&sk.sheet, &self.development, pieces, &[], &sk.recipes);
        let made = std::cell::RefCell::new(Vec::new());
        let keep = |p: crate::sheet_surface::Piece, r: crate::sheet::FaceRole| {
            let mut made = made.borrow_mut();
            made.push((p, r));
            Some((made.len() - 1).to_string())
        };
        let (body, keys) = crate::sheet_surface_fold::solid(&self.development, &folded, self.thickness, pieces, &mut FlatWork::new(), &keep)?;
        let made = made.into_inner();
        let at = |k: &Option<String>| k.as_deref().and_then(|k| k.parse::<usize>().ok()).and_then(|k| made.get(k).copied());
        // What each face was made for, and a panel's cap's sketch frame, carried through the cuts.
        type Made = (Option<(SheetPart, Option<f64>)>, Option<Plane>);
        let named: Vec<Option<Made>> = keys
            .iter()
            .map(|k| {
                at(k).map(|(p, r)| {
                    let frame = match (p, r) {
                        (crate::sheet_surface::Piece::Panel(i), crate::sheet::FaceRole::Cap { surface, .. }) => {
                            folded.places.get(i).copied().flatten().and_then(|f| cap_frame(&self.development, f, i, surface, self.thickness))
                        }
                        _ => None,
                    };
                    (namer.part(p, r, &[]), frame)
                })
            })
            .collect();
        let (body, named) = sk.recessed_named(body, named, &folded.places)?;
        let parts = named.iter().map(|n| n.as_ref().and_then(|n| n.0.clone())).collect();
        let frames = named.into_iter().map(|n| n.and_then(|n| n.1)).collect();
        Ok((body, crate::sheet::ranked(parts), frames))
    }
}

/// The flat a sheet is made with (see [`SurfaceSheet::flat`]): where the maker placed its planar
/// panel with the most material, then the one lying lowest in x, y and z. A sheet with no planar
/// panel (a roll) has its flat in the chart of the placed panel with the most material as the
/// development lays it out, then the lowest: the chart the development lays a roll out in. Taken
/// from the panels' geometry, never their order. None where the maker placed no panel.
pub(crate) fn made_flat(sheet: &Sheet, places: &[Option<Rigid3>]) -> Option<Rigid3> {
    let flat = most_material(placed_panels(sheet, places, true), |i| sheet.panels.get(i).map_or(0.0, |p| p.region.area()));
    if flat.is_some() {
        return flat;
    }
    let mut placed = placed_panels(sheet, places, false);
    let (one, more) = (placed.next(), placed.next());
    if more.is_none() {
        return one.map(|(_, at)| at);
    }
    // A roll's chart is not its material's shape: its developed panel is.
    let dev = develop(sheet, &[]).ok()?;
    most_material(placed_panels(sheet, places, false), |i| dev.panels.get(i).and_then(Option::as_ref).map_or(0.0, |p| p.region.area()))
}

/// The panels of `sheet` the maker placed (`places`), only the planar ones where `planar`, each
/// with its place.
fn placed_panels<'a>(sheet: &'a Sheet, places: &'a [Option<Rigid3>], planar: bool) -> impl Iterator<Item = (usize, Rigid3)> + 'a {
    sheet
        .panels
        .iter()
        .enumerate()
        .filter(move |(_, p)| !planar || matches!(p.surface, Surface::Plane))
        .filter_map(move |(i, _)| Some((i, places.get(i).copied().flatten()?)))
}

/// Of `among`, the panel with the most material by `area`, then the one whose place lies lowest
/// in x, y and z: its place.
fn most_material(among: impl Iterator<Item = (usize, Rigid3)>, area: impl Fn(usize) -> f64) -> Option<Rigid3> {
    among
        .min_by(|a, b| {
            let (sa, sb) = (a.1.shift, b.1.shift);
            area(b.0).total_cmp(&area(a.0)).then(sa.x.total_cmp(&sb.x)).then(sa.y.total_cmp(&sb.y)).then(sa.z.total_cmp(&sb.z))
        })
        .map(|(_, at)| at)
}

/// How far (as a share of the sheet's reach) a panel's place may lie off the flat's plane and
/// still lie in it: a rounding of the places, which the maker computed alike.
const IN_FLAT: f64 = 1e-9;

/// What the development of `sheet` lays its flat out from (see `solidcraft_develop::develop`):
/// the panel the flat was taken from (see [`made_flat`]), a roll's too, at the flat's origin; and
/// each planar panel placed (`places`) in the plane of the flat `flat`, facing its way, at the
/// chart position its place puts it in. A panel whose material a cut took keeps its place, and
/// still anchors the flat. Nothing depends on the panels' order.
pub(crate) fn flat_anchors(sheet: &Sheet, places: &[Option<Rigid3>], flat: &Rigid3) -> Vec<(usize, Rigid2)> {
    let back = flat.inverse();
    // How far the sheet reaches, read only for a panel square to the flat.
    let reach = || sheet.panels.iter().map(|p| p.region.outer.polyline(1e-3).bounds().size().len()).fold(1.0, f64::max);
    sheet
        .panels
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let at = places.get(i).copied().flatten()?;
            // The panel the flat was taken from lies at it exactly.
            if at == *flat {
                return Some((i, Rigid2::IDENTITY));
            }
            if !matches!(p.surface, Surface::Plane) {
                return None;
            }
            let m = at.then(&back);
            let level = m.z.dist(Vec3::Z) <= IN_FLAT && m.shift.z.abs() <= IN_FLAT * reach();
            let turn = Vec2::new(m.x.x, m.x.y).normalized()?;
            level.then_some((i, Rigid2 { cos: turn.x, sin: turn.y, shift: Vec2::new(m.shift.x, m.shift.y) }))
        })
        .collect()
}

/// [`flat_anchors`] of a sheet as its maker makes it (see [`made_flat`]).
fn made_anchors(sheet: &Sheet, places: &[Rigid3]) -> Vec<(usize, Rigid2)> {
    let places: Vec<Option<Rigid3>> = places.iter().copied().map(Some).collect();
    made_flat(sheet, &places).map(|flat| flat_anchors(sheet, &places, &flat)).unwrap_or_default()
}

/// `sheet` folded, each piece of its flat where the panels with a place of their own (the faces
/// they were converted from) put it. Every placed panel must fold onto its place through the
/// bends and seams that reach it: anywhere else, a joint was read wrong, and that is an error
/// rather than a sheet in the wrong place. Onto it means within the millionth of its size Convert
/// reads the body's faces to: a body is no more exact than that (a section drawn through points
/// rounded to six places is a sheet to a few millionths of a millimetre), and a joint read wrong
/// misses by far more.
pub(crate) fn fold_onto(sheet: &Sheet, dev: &Development, places: &[Option<Rigid3>]) -> Result<Fold> {
    // The panel a piece is folded from is picked by geometry, never by the panels' order: the one
    // with the most material in its chart, then the one whose place lies lowest in x, y and z.
    // Every other placed panel then folds to its place within the body's tolerance (checked
    // below), so the pick moves nothing. `fold` starts a piece from its first panel: where the
    // pick is another, the piece is folded from the identity first and then moved so that the
    // pick lies at its place.
    let area = |i: usize| sheet.panels.get(i).map_or(0.0, |p| p.region.area());
    let first = |a: &(usize, Rigid3), b: &(usize, Rigid3)| {
        let (sa, sb) = (a.1.shift, b.1.shift);
        area(b.0).total_cmp(&area(a.0)).then(sa.x.total_cmp(&sb.x)).then(sa.y.total_cmp(&sb.y)).then(sa.z.total_cmp(&sb.z))
    };
    let mut roots = Vec::with_capacity(dev.pieces.len());
    let mut moved = Vec::new();
    for (k, piece) in dev.pieces.iter().enumerate() {
        let (i, at) = piece
            .iter()
            .filter_map(|i| places.get(*i).copied().flatten().map(|at| (*i, at)))
            .min_by(first)
            .ok_or_else(|| DocError::Invalid("a piece of the sheet has no place".into()))?;
        if piece.first() == Some(&i) {
            roots.push(at);
        } else {
            roots.push(Rigid3::IDENTITY);
            moved.push((k, i, at));
        }
    }
    let mut folded = fold(sheet, dev, &roots).map_err(|e| DocError::Invalid(e.to_string()))?;
    if !moved.is_empty() {
        for (k, i, at) in moved {
            let from = folded.places.get(i).copied().flatten().ok_or_else(|| DocError::Invalid("a piece of the sheet has no place".into()))?;
            if let Some(root) = roots.get_mut(k) {
                *root = from.inverse().then(&at);
            }
        }
        folded = fold(sheet, dev, &roots).map_err(|e| DocError::Invalid(e.to_string()))?;
    }
    let reach = dev.panels.iter().flatten().map(|p| p.region.outer.polyline(1e-3).bounds().size().len()).fold(1.0, f64::max);
    for (i, (got, own)) in folded.places.iter().zip(places).enumerate() {
        let (Some(got), Some(own)) = (got, own) else { continue };
        let off = got.shift.dist(own.shift) + reach * (got.x.dist(own.x) + got.y.dist(own.y) + got.z.dist(own.z));
        if off > (reach * 1e-6).max(1e-6) {
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
    /// Where the sheet's flat lies in space while it is unfolded: the frame of the chart its
    /// development lays the flat out in, as the feature that made the sheet put it (its sketch's
    /// plane, the converted face's chart). It is the sheet's, no panel's: the sheet keeps it
    /// through every later feature, whatever they add, cut away or reorder, and a copy or a
    /// transfer of the body moves it with the sheet.
    pub flat: Rigid3,
    /// The body is the flat pattern, lying in the sheet's flat.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unfolded: bool,
    pub thickness: f64,
    pub k_factor: f64,
    /// The flat material, in the sheet's flat.
    pub regions: Vec<Region2>,
    pub bends: Vec<SurfaceBendLine>,
    /// The largest strain any panel's flat leaves: none but on a free-form panel.
    pub stretch: f64,
    /// How many of the panels are rolls.
    pub rolls: usize,
    /// The rolls' lines in the flat (see [`RollLine`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roll_lines: Vec<RollLine>,
    /// What the sheet's bends end with, as its rule had it.
    pub reliefs: Reliefs,
    /// A feature's own corner relief, in place of the rule's, for a bend it made: the bend's
    /// joint, the relief, its size, and the feature's place in the timeline. A corner takes the
    /// relief of whichever of its two bends' features comes later in the timeline (the one
    /// evaluated later overrides); two bends of one feature ask for one relief.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corner_reliefs: Vec<(usize, CornerRelief, f64, usize)>,
    /// Where the stretches of a split edge meet (`sheet.flange`'s `segments`, see
    /// [`SurfaceSheet::flanges_on`]), the two bends there: the joint of the bend that ends at the
    /// junction (at its anchors' length) and the joint of the bend that starts there (at their
    /// origin). The two butt: no corner is closed between them and no bend relief is cut at either
    /// end, each ending square across its joint (see [`settle`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub junctions: Vec<[usize; 2]>,
    /// Where the flat is cut along a junction of a split edge whose two stretches bend to opposite
    /// sides (see [`SurfaceSheet::junctions`]), as last laid out: each `[start, end]` in the flat,
    /// from the deeper of the two bends' lines on the panel they stand on out to the nearer of
    /// their flanges' far edges. The cut has no width: the flat's material (`regions`) is the same
    /// either side of it, and the two stretches part along it, as they do folded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slits: Vec<[Vec2; 2]>,
    /// Why a relief asked for was not cut, as the sheet was last built.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The recesses cut into its flat panels (see `sheet_surface_cuts`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recesses: Vec<crate::Recess>,
    /// The recesses in the flat, as pockets measured from its face on the sheet's surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pockets: Vec<crate::sheet::SheetPocket>,
    /// The pieces cuts parted from its panels that hold none of their joints: they stay where
    /// they were (see `sheet_surface_cuts`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loose: Vec<crate::Loose>,
    /// The feature that changed the body in a way the sheet can't hold (a fillet, a join into a
    /// bend), while the body keeps that change: its flat exports are refused until a sheet metal
    /// feature builds the body from the sheet again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altered_by: Option<String>,
    /// Each face of its body's sketch frame, where the sheet gives one: a panel's cap's is the flat
    /// pattern's, as it is read (see [`cap_frame`]). What a sketch on the face is drawn in.
    #[serde(skip)]
    pub frames: Vec<Option<Plane>>,
    /// Holes drilled along its flat panels into their rims (#560), each in its panel's frame
    /// (`base` and `axis` in the chart, `base.z` its depth into the material): cut where the
    /// panel lies, folded or flat.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bores: Vec<crate::sheet::SheetBore>,
    /// Where text cut through it went, in the flat: each letter with its counters, so its DXF
    /// can put them on their own layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<Region2>,
    /// Perforation fields (#732): holes on a grid over the flat, not cut into the body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perforations: Vec<crate::sheet::SheetPerforation>,
    /// For each perforation field, by its feature, how many joints the sheet had when the field
    /// was made: the field keeps clear of those bends, and the bends made after it run through
    /// it, as a press brake bends perforated stock.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perforation_joints: Vec<(u64, usize)>,
    /// The sketch lines that made panels' edges: the panel (by its key, `Panel::name`), each line's
    /// id and its ends in that panel's chart. Any panel a sketch made has its own (a base flange's);
    /// a rim on one is made by that line (see [`RimEdge::Sketch`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch_curves: Vec<(String, String, Vec2, Vec2)>,
    /// Each straight rim of its flat panels as last laid out: the panel, its edge in the panel's
    /// chart and its name there (see [`crate::sheet::FaceNamer`]). A pick is named by them and
    /// found again by its name, so a feature reads them here rather than laying the sheet out
    /// again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rims: Vec<(usize, Vec2, Vec2, String)>,
    /// What each face of its body was made for, as it was last built (see [`SheetFace`]): what it
    /// reports to the model, which names its faces from it (#7990). Never a name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Option<SheetFace>>,
    /// What made each panel, by the panel's key (see [`PanelMade`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipes: Vec<(String, PanelMade)>,
    /// The holes cuts made in its panels (see [`HoleRecord`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<HoleRecord>,
}

/// What a face of a sheet's body was made for, by its builder: the sheet's own structure, never a
/// name (#6748, #7990). The model names the face from it, by what made it (see
/// `lineage::sheet_lineage`). A panel is its key (`Panel::name`), fixed when the panel is made,
/// never its place among the sheet's panels.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SheetPart {
    /// A panel's face on the sheet's surface (`surface`), or the one a thickness into the material.
    Cap { panel: String, surface: bool },
    /// A panel's wall along an edge of its outline, by what made the edge.
    Rim { panel: String, edge: RimEdge },
    /// The curved face of the bend that makes `panel`: its outside (`outer`) or its inside.
    Bend { panel: String, outer: bool },
    /// The end of the bend that makes `panel` at its anchors' origin (`at_origin`), or the other.
    BendEnd { panel: String, at_origin: bool },
    /// A cap of a piece of the bend that makes `panel` riding flat with a panel (a tab).
    Tab { panel: String, surface: bool },
    /// A wall of a hole the cut `feature` made, along its sketch curve `curve`.
    Hole { feature: u64, curve: String },
    /// A wall of the relief cut where the bends that make `panels` (sorted) meet at a corner.
    Relief { panels: [String; 2] },
}

/// What made an edge of a panel's outline.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RimEdge {
    /// A line of the sketch that made the panel (its id).
    Sketch { curve: String },
    /// On a panel a bend makes, the edge along the bend farthest from it.
    Far,
    /// On a panel a bend makes, an edge across the bend's ends: at the anchors' origin
    /// (`at_origin`), or at the other end.
    End { at_origin: bool },
    /// On a panel a bend makes, the stretch of an edge along the bend past one of its ends.
    Past { at_origin: bool },
}

impl RimEdge {
    /// The role of a rim on an edge so made, as the model names it (`side:<curve>`, `far`,
    /// `end0` at the bend's anchors' origin and `end1`, `past0` and `past1` likewise): what made
    /// the edge, never its place in the outline.
    pub(crate) fn role(&self) -> String {
        match self {
            RimEdge::Sketch { curve } => format!("side:{curve}"),
            RimEdge::Far => "far".into(),
            RimEdge::End { at_origin } => if *at_origin { "end0" } else { "end1" }.into(),
            RimEdge::Past { at_origin } => if *at_origin { "past0" } else { "past1" }.into(),
        }
    }
}

/// A face of a sheet's body as its builder made it: what for, and, where several faces are made
/// for one part (a hole's wall's pieces, a bend's tabs), its place among them, from 1, in the order
/// they start along the part's definition (a hole's sketch curve, a bend's anchor, a relief's
/// outline). The place is a rank, never a measure: it holds while sizes change.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SheetFace {
    pub part: SheetPart,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<usize>,
}

/// What made a panel of a sheet made from surfaces: the feature, and what it made the panel from
/// or on. Its faces are named by it (see `lineage::sheet_lineage`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PanelMade {
    /// A base flange's panel, from a profile of its sketch.
    Sketch { feature: u64 },
    /// A contour flange's panel along its sketch line `curve`.
    Contour { feature: u64, curve: String },
    /// A panel a bend makes (a flange, a hem, a path flange's first) on the rim `on` of another
    /// panel; none where what made that rim is not known.
    Bend { feature: u64, on: Option<SheetPart> },
    /// A path flange's panel along its path's sketch line `curve`, past its first.
    Chain { feature: u64, curve: String },
    /// A piece of panel `from` a fold parted off, the `place`th (from 1) the fold parted off it in
    /// the order the pieces start along its line: its faces are pieces of `from`'s, whose chart it
    /// keeps.
    Fold { feature: u64, from: String, place: usize },
    /// A panel Convert made from face `face` of the body it converted.
    Converted { feature: u64, face: usize },
}

impl PanelMade {
    /// The feature that made the panel.
    pub fn feature(&self) -> u64 {
        match self {
            PanelMade::Sketch { feature }
            | PanelMade::Contour { feature, .. }
            | PanelMade::Bend { feature, .. }
            | PanelMade::Chain { feature, .. }
            | PanelMade::Fold { feature, .. }
            | PanelMade::Converted { feature, .. } => *feature,
        }
    }
}

/// A hole a cut made in a panel: the panel (by key) whose chart the hole's loop is drawn in, the
/// cut, and each segment of its loop with the id of the sketch curve under it (empty where none
/// is known) and that curve's segment as drawn, in the chart: what the wall's pieces are ordered
/// along. A fold's pieces keep their panel's chart, so the hole is read there wherever it is.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HoleRecord {
    pub panel: String,
    pub feature: u64,
    pub segs: Vec<(Seg2, String, Option<Seg2>)>,
}

/// The sketch frame of a cap of panel `i` of a sheet developed as `dev`, whose chart `frame` puts
/// in space: on the sheet's surface (`surface`), or a thickness `t` into the material. Its
/// coordinates are the flat pattern's as it is read (see [`SurfaceSheet::shown`]), the other
/// face's mirrored, so a sketch on it measures as the flat pattern does; it faces out of the sheet.
pub(crate) fn cap_frame(dev: &Development, frame: Rigid3, i: usize, surface: bool, t: f64) -> Option<Plane> {
    let back = dev.placements.get(i).copied().flatten()?.inverse();
    let o = back.point(Vec2::new(0.0, 0.0));
    let (x, y) = (back.vector(Vec2::new(1.0, 0.0)), back.vector(Vec2::new(0.0, if surface { -1.0 } else { 1.0 })));
    let z = if surface { 0.0 } else { t };
    Plane::new(frame.point(Vec3::new(o.x, o.y, z)), frame.vector(Vec3::new(x.x, x.y, 0.0)), frame.vector(Vec3::new(y.x, y.y, 0.0)))
}

/// `r` through `map`, a rigid map of the plane (mirrored or not): lines and arcs stay lines and
/// arcs, free-form curves map exactly through their control points.
pub(crate) fn rigid_region(r: &Region2, map: impl Fn(Vec2) -> Vec2) -> Region2 {
    let seg = rigid_seg(&map);
    let map_loop = |lp: &Loop2| Loop2 { segs: lp.segs.iter().map(&seg).collect() };
    Region2 { outer: map_loop(&r.outer).ccw(), holes: r.holes.iter().map(|h| map_loop(h).ccw().reversed()).collect() }
}

/// Each segment through `map`, a rigid map of the plane (mirrored or not), still running the way
/// it ran: where a mirror turns a counterclockwise arc clockwise, it stays so (`rigid_region`
/// turns whole loops back counterclockwise; a segment ordered along keeps its own direction).
pub(crate) fn rigid_seg(map: impl Fn(Vec2) -> Vec2) -> impl Fn(&Seg2) -> Seg2 {
    let (o, ex, ey) = (map(Vec2::new(0.0, 0.0)), map(Vec2::new(1.0, 0.0)), map(Vec2::new(0.0, 1.0)));
    let (ax, ay) = (ex - o, ey - o);
    let mirrored = ax.cross(ay) < 0.0;
    let rot = ax.y.atan2(ax.x);
    move |sg: &Seg2| match *sg {
        Seg2::Line { a, b } => Seg2::Line { a: map(a), b: map(b) },
        Seg2::Arc { center, radius, start, sweep } if mirrored => Seg2::Arc { center: map(center), radius, start: rot - start, sweep: -sweep },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: map(center), radius, start: rot + start, sweep },
        other => other.map_controls(&map),
    }
}

/// A line across a roll in a sheet's flat, along its rulings, as a bend has its tangent and
/// center lines: where the roll starts, where it ends, or its middle; `panel` is the roll's.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RollLine {
    pub panel: usize,
    pub kind: RollEdge,
    pub start: Vec2,
    pub end: Vec2,
}

/// A roll of a sheet's flat, as a bend has its row: its number among the rolls, its panel, its
/// inside radius (none for a cone, whose radius runs along it), how far it turns (degrees), its
/// length along its rulings and how wide it lies in the flat.
#[derive(Clone, Debug, PartialEq)]
pub struct RollInfo {
    pub index: usize,
    pub panel: usize,
    pub radius: Option<f64>,
    pub angle_deg: f64,
    pub length: f64,
    pub width: f64,
}

/// Which of a roll's lines a [`RollLine`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollEdge {
    Start,
    End,
    Middle,
}

impl RollEdge {
    /// Its name, as `sheet.flat_pattern` gives it.
    pub fn name(self) -> &'static str {
        match self {
            RollEdge::Start => "start",
            RollEdge::End => "end",
            RollEdge::Middle => "middle",
        }
    }
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
    /// The bend's joint in the sheet.
    #[serde(default)]
    pub joint: usize,
}

impl SurfaceSheet {
    /// A converted body's sheet as the model keeps it, for the body `body` under the rule `rule`,
    /// and its folded solid. `feature` is the Convert: each panel's key is its and the face of the
    /// body the panel was (see [`PanelMade::Converted`]).
    pub fn converted(body: &str, rule: &str, c: &SurfaceConversion, feature: u64) -> Result<(SurfaceSheet, Body)> {
        let mut recipes = Vec::with_capacity(c.sheet.panels.len());
        let mut keyed = c.sheet.clone();
        for (panel, face) in keyed.panels.iter_mut().zip(&c.faces) {
            panel.name = format!("F{feature}/face{face}");
            recipes.push((panel.name.clone(), PanelMade::Converted { feature, face: *face }));
        }
        // The flat lies where the conversion's development laid it out (see `made_flat`).
        let placed: Vec<Option<Rigid3>> = c.places.iter().copied().map(Some).collect();
        let flat = made_flat(&c.sheet, &placed).ok_or_else(|| DocError::Invalid("the converted body gives the sheet no flat face".into()))?;
        let mut sheet = SurfaceSheet {
            body: body.into(),
            rule: rule.into(),
            sheet: keyed,
            places: placed,
            flat,
            unfolded: false,
            thickness: c.thickness,
            k_factor: c.sheet.k_factor,
            regions: Vec::new(),
            bends: Vec::new(),
            stretch: 0.0,
            rolls: 0,
            roll_lines: Vec::new(),
            reliefs: c.reliefs,
            corner_reliefs: Vec::new(),
            junctions: Vec::new(),
            slits: Vec::new(),
            notes: c.notes.clone(),
            // The engravings Convert keeps; Recognize reads a body's own once it is built.
            recesses: c.recesses.clone(),
            pockets: Vec::new(),
            loose: Vec::new(),
            labels: Vec::new(),
            bores: Vec::new(),
            altered_by: None,
            frames: Vec::new(),
            perforations: Vec::new(),
            perforation_joints: Vec::new(),
            sketch_curves: Vec::new(),
            rims: Vec::new(),
            parts: Vec::new(),
            recipes,
            holes: Vec::new(),
        };
        sheet.laid_out(&c.development, c.pieces()?, c.flat()?)?;
        let (solid, parts, frames) = c.parted_solid(&sheet)?;
        sheet.parts = parts;
        sheet.frames = frames;
        Ok((sheet, solid))
    }

    /// A sheet of one flat panel, `region` in `plane`, its material the rule's thickness along the
    /// plane's normal (a base flange), and its folded body. The sheet's surface is its top, the
    /// face a thickness along the normal, its material below it, as a plate thickened from its top
    /// has it: flanges rise from it.
    /// `feature` is the base flange, whose sketch's lines `curves` are: the panel's key is its own
    /// (see [`PanelMade::Sketch`]).
    #[allow(clippy::too_many_arguments)]
    pub fn base(
        body: &str,
        rule_name: &str,
        rule: &ConvertRule,
        plane: Plane,
        region: Region2,
        curves: &[(String, Vec2, Vec2)],
        feature: u64,
    ) -> Result<(SurfaceSheet, Body)> {
        let top = plane.origin + plane.normal() * rule.thickness;
        let place = Rigid3::frame(top, plane.x, plane.y * -1.0).ok_or_else(|| DocError::Invalid("the sketch's plane has no frame".into()))?;
        // The top's chart runs y the other way: the same outline, mirrored in it (exactly; free-form
        // curves through their control points).
        let f = |p: Vec2| Vec2::new(p.x, -p.y);
        let mirror = |l: &Loop2| {
            let segs = l
                .segs
                .iter()
                .map(|s| match *s {
                    Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: f(center), radius, start: -start, sweep: -sweep },
                    other => other.map_controls(f),
                })
                .collect();
            Loop2 { segs }
        };
        let region = Region2 { outer: mirror(&region.outer).ccw(), holes: region.holes.iter().map(|h| mirror(h).ccw().reversed()).collect() };
        let made = PanelMade::Sketch { feature };
        let panel = Panel { name: format!("F{feature}"), surface: Surface::Plane, region };
        // The panel's own sketch lines.
        let curves = curves.iter().map(|(id, a, b)| (panel.name.clone(), id.clone(), f(*a), f(*b))).collect();
        let recipes = vec![(panel.name.clone(), made)];
        Self::made(body, rule_name, rule, vec![panel], vec![Some(place)], Vec::new(), curves, recipes)
    }

    /// A sheet from an open chain of lines (a contour flange): `pts` in `plane`, the face of the
    /// sheet, its material on the right of the travel (or the left when `flip`) and `width` along
    /// the plane's normal; one panel per line, bent at the rule's radius at each corner, the
    /// corner its virtual sharp. And its folded body. `feature` is the contour flange, `lines` the
    /// sketch line each line of the chain was drawn on, with the line's place on it where it was
    /// drawn in several (`l3.2`): each panel's key is the feature's and its line's (see
    /// [`PanelMade::Contour`]).
    #[allow(clippy::too_many_arguments)]
    pub fn contour(
        body: &str,
        rule_name: &str,
        rule: &ConvertRule,
        plane: &Plane,
        pts: &[Vec2],
        width: f64,
        flip: bool,
        feature: u64,
        lines: &[(String, String)],
    ) -> Result<(SurfaceSheet, Body)> {
        if pts.len() < 2 {
            return err("a contour flange needs an open chain of lines");
        }
        if lines.len() + 1 != pts.len() {
            return err("a contour flange's lines and its chain differ");
        }
        let mut recipes = Vec::with_capacity(lines.len());
        let normal = plane.normal();
        let side = if flip { -1.0 } else { 1.0 };
        let mut panels = Vec::with_capacity(pts.len() - 1);
        let mut places = Vec::with_capacity(pts.len() - 1);
        let mut ways = Vec::with_capacity(pts.len() - 1);
        for (i, w) in pts.windows(2).enumerate() {
            let (a, b) = (plane.to_world(w[0]), plane.to_world(w[1]));
            let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("a zero-length line in the contour".into()))?;
            // The frame's z toward the material: right of the travel (d x n), or left when flipped.
            let (origin, y) = if flip { (a + normal * width, normal * -1.0) } else { (a, normal) };
            places.push(Some(Rigid3::frame(origin, d, y).ok_or_else(|| DocError::Invalid("a contour line has no frame".into()))?));
            let rect = [Vec2::new(0.0, 0.0), Vec2::new(a.dist(b), 0.0), Vec2::new(a.dist(b), width), Vec2::new(0.0, width)];
            let region = Region2 { outer: Loop2::polygon(&rect).ccw(), holes: Vec::new() };
            let (curve, key) = lines.get(i).ok_or_else(|| DocError::Invalid("a contour flange's lines and its chain differ".into()))?;
            let name = format!("F{feature}/{key}");
            recipes.push((name.clone(), PanelMade::Contour { feature, curve: curve.clone() }));
            panels.push(Panel { name, surface: Surface::Plane, region });
            ways.push((b, d, d.cross(normal) * side));
        }
        let chart = |place: &Rigid3, p: Vec3| {
            let q = place.inverse().point(p);
            Vec2::new(q.x, q.y)
        };
        let mut joints = Vec::with_capacity(ways.len().saturating_sub(1));
        for (i, pair) in ways.windows(2).enumerate() {
            let [(corner, da, toward), (_, db, _)] = pair else { continue };
            let angle = da.dot(*db).clamp(-1.0, 1.0).acos();
            if angle < 1e-9 {
                return err("straight corners in a contour flange");
            }
            let (Some(pa), Some(pb)) = (places[i], places[i + 1]) else { continue };
            // Along the corner, so that `a`'s material lies to its left: `a` lies back along its
            // line from the corner.
            let (mut s0, mut s1) = (*corner, *corner + normal * width);
            let (c0, c1) = (chart(&pa, s0), chart(&pa, s1));
            if (c1 - c0).cross(Vec2::new(-1.0, 0.0)) < 0.0 {
                (s0, s1) = (s1, s0);
            }
            let (a0, a1, b0, b1) = (chart(&pa, s0), chart(&pa, s1), chart(&pb, s0), chart(&pb, s1));
            let (Some(dir_a), Some(dir_b)) = ((a1 - a0).normalized(), (b1 - b0).normalized()) else { continue };
            let bend = Bend {
                angle,
                radius: rule.bend_radius,
                toward_material: db.dot(*toward) > 0.0,
                datum: Datum::Sharp,
                length: width,
                ends: ZoneEnds { near: (0.0, width), far: (0.0, width) },
                tails: (Tail::Keep, Tail::Keep),
                relief: (Relief::None, Relief::None),
            };
            let (a, b) = (Anchor { panel: i, origin: a0, dir: dir_a }, Anchor { panel: i + 1, origin: b0, dir: dir_b });
            joints.push(Joint { a, b, kind: JointKind::Bend(bend) });
        }
        Self::made(body, rule_name, rule, panels, places, joints, Vec::new(), recipes)
    }

    /// A sheet of `panels`, each where `places` puts it, joined by `joints`, under the rule, and
    /// its folded body; `sketch_curves` and `recipes` as [`SurfaceSheet`] keeps them.
    #[allow(clippy::too_many_arguments)]
    fn made(
        body: &str,
        rule_name: &str,
        rule: &ConvertRule,
        panels: Vec<Panel>,
        places: Vec<Option<Rigid3>>,
        joints: Vec<Joint>,
        sketch_curves: Vec<(String, String, Vec2, Vec2)>,
        recipes: Vec<(String, PanelMade)>,
    ) -> Result<(SurfaceSheet, Body)> {
        let model = Sheet { thickness: rule.thickness, k_factor: rule.k_factor, panels, joints, corners: Vec::new(), cuts: Vec::new() };
        // The flat lies where the maker placed its largest flat panel (see `made_flat`).
        let flat = made_flat(&model, &places).ok_or_else(|| DocError::Invalid("the sheet's maker gives it no flat panel".into()))?;
        let mut sheet = SurfaceSheet {
            body: body.into(),
            rule: rule_name.into(),
            sheet: model,
            places,
            flat,
            unfolded: false,
            thickness: rule.thickness,
            k_factor: rule.k_factor,
            regions: Vec::new(),
            bends: Vec::new(),
            stretch: 0.0,
            rolls: 0,
            roll_lines: Vec::new(),
            reliefs: Reliefs::of(rule),
            corner_reliefs: Vec::new(),
            junctions: Vec::new(),
            slits: Vec::new(),
            notes: Vec::new(),
            recesses: Vec::new(),
            pockets: Vec::new(),
            loose: Vec::new(),
            labels: Vec::new(),
            bores: Vec::new(),
            altered_by: None,
            frames: Vec::new(),
            perforations: Vec::new(),
            perforation_joints: Vec::new(),
            sketch_curves,
            rims: Vec::new(),
            parts: Vec::new(),
            recipes,
            holes: Vec::new(),
        };
        let solid = sheet.build()?;
        Ok((sheet, solid))
    }

    /// Hole centres of a perforation field in the flat: a grid continuous over the developed flat,
    /// margin clear of its edges, its cut-outs and the zones of the bends the sheet had when the
    /// field was made (see [`crate::sheet::perforation_grid`]). A bend made after the field runs
    /// through it, as a press brake bends perforated stock.
    pub fn perforation_holes(&self, p: &crate::sheet::SheetPerforation) -> Vec<Vec2> {
        let before = self.perforation_joints.iter().find(|(f, _)| *f == p.feature).map_or(0, |(_, n)| *n);
        let zones: Vec<Region2> = self.bends.iter().filter(|b| b.joint < before).map(|b| b.zone.clone()).collect();
        crate::sheet::perforation_grid(&self.regions, &zones, p)
    }

    /// Whether the flat describes the body: refused while a feature the sheet can't hold changed
    /// it (see `altered_by`).
    pub fn flat_matches_body(&self) -> Result<()> {
        match &self.altered_by {
            None => Ok(()),
            Some(feature) => Err(DocError::Invalid(format!(
                "not supported yet: the flat pattern of `{}` can't include {feature}, which changed the body after its sheet metal features, so the flat pattern, DXF and bend table would not match the part; remove or suppress {feature}, or make that shape with sheet metal features",
                self.body
            ))),
        }
    }

    /// `measured`, the B-rep's measure of this sheet's body, with the holes its perforation fields
    /// leave out of the body taken off where the sheet lies now (folded, or laid out flat): a
    /// cylinder through a flat panel, or bent with a bend made after its field (see
    /// [`crate::sheet_surface_perforation`]). `placement` maps the sheet's geometry into the
    /// measurement's frame. The bounds and the topology stay as measured.
    pub fn correct_perforation_properties(&self, measured: &mut solidcraft_kernel::BodyMeasure, placement: &crate::Mat) -> Result<()> {
        let (mut removed, mut area_delta, mut moment_delta) = (0.0, 0.0, Vec3::ZERO);
        self.visit_perforation_holes(placement, |center, volume, area, _| {
            removed += volume;
            area_delta += area;
            // From the measured centre, so that a far placement keeps its digits.
            moment_delta += (measured.centroid - center) * volume;
        })?;
        if removed == 0.0 {
            return Ok(());
        }
        let volume = measured.volume - removed;
        let area = measured.area + area_delta;
        let centroid = measured.centroid + moment_delta / volume;
        if !(volume.is_finite() && volume > 0.0 && area.is_finite() && area >= 0.0 && centroid.is_finite()) {
            return Err(DocError::Invalid(format!("invalid perforation physical properties for `{}`", self.body)));
        }
        measured.volume = volume;
        measured.area = area;
        measured.centroid = centroid;
        Ok(())
    }

    /// `measured`, the mass properties of this sheet's body at unit density (its inertia about
    /// its centroid), with the same holes taken off as [`Self::correct_perforation_properties`]
    /// takes.
    pub fn correct_perforation_mass_properties(&self, measured: &mut solidcraft_geom::MassProperties, placement: &crate::Mat) -> Result<()> {
        use solidcraft_geom::inertia;
        let (mut removed, mut moment_delta, mut removed_inertia) = (0.0, Vec3::ZERO, [[0.0; 3]; 3]);
        self.visit_perforation_holes(placement, |center, volume, _, own| {
            let delta = measured.centroid - center;
            removed += volume;
            moment_delta += delta * volume;
            let about_center = inertia::shifted(&own, volume, delta);
            for (row, hole) in removed_inertia.iter_mut().zip(about_center) {
                for (value, h) in row.iter_mut().zip(hole) {
                    *value += h;
                }
            }
        })?;
        if removed == 0.0 {
            return Ok(());
        }
        let volume = measured.volume - removed;
        let delta = moment_delta / volume;
        let centroid = measured.centroid + delta;
        let mut tensor = measured.inertia;
        for (row, hole) in tensor.iter_mut().zip(removed_inertia) {
            for (value, h) in row.iter_mut().zip(hole) {
                *value -= h;
            }
        }
        // From the old centre to the new one: the parallel-axis term taken back.
        let tensor = inertia::shifted(&tensor, -volume, delta);
        if !(volume.is_finite() && volume > 0.0 && centroid.is_finite() && tensor.iter().flatten().all(|v| v.is_finite())) {
            return Err(DocError::Invalid(format!("invalid perforation inertia for `{}`", self.body)));
        }
        measured.volume = volume;
        measured.centroid = centroid;
        measured.inertia = tensor;
        Ok(())
    }

    /// Each hole of the perforation fields as what it takes out of the body (see
    /// [`crate::sheet_surface_perforation`]), in the frame `placement` maps the sheet into: its
    /// centroid, volume, the area its walls add to the body's faces less the area it opens in
    /// them, and its inertia about its centroid at unit density. Refused: a placement that scales
    /// or shears, and the holes `hole_masses` refuses.
    fn visit_perforation_holes(
        &self,
        placement: &crate::Mat,
        mut visit: impl FnMut(Vec3, f64, f64, solidcraft_geom::inertia::Tensor3),
    ) -> Result<()> {
        if self.perforations.is_empty() {
            return Ok(());
        }
        if placement.iter().flatten().any(|v| !v.is_finite()) {
            return Err(DocError::Invalid(format!("invalid perforation physical properties for `{}`", self.body)));
        }
        let [x, y, z] = [0, 1, 2].map(|i| Vec3::new(placement[i][0], placement[i][1], placement[i][2]));
        let rigid = placement[0][3] == 0.0
            && placement[1][3] == 0.0
            && placement[2][3] == 0.0
            && placement[3][3] == 1.0
            && [x, y, z].iter().all(|a| (a.dot(*a) - 1.0).abs() < 1e-9)
            && x.dot(y).abs() < 1e-9
            && x.dot(z).abs() < 1e-9
            && y.dot(z).abs() < 1e-9;
        if !rigid {
            return Err(DocError::Invalid("not supported yet: perforation physical properties under scale or shear".into()));
        }
        let rotation = [0, 1, 2].map(|r| [0, 1, 2].map(|c| placement[c][r]));
        for hole in crate::sheet_surface_perforation::hole_masses(self)? {
            visit(
                crate::apply_point(placement, hole.centroid),
                hole.volume,
                hole.area,
                solidcraft_geom::inertia::transformed(&hole.inertia, &rotation),
            );
        }
        Ok(())
    }

    /// The sheet with its flat as a flat pattern is read: seen from the sheet's surface, the side
    /// its up bends rise to. The panels' charts face the material, so the flat they lay out is
    /// that view mirrored; here its regions, bends, roll lines, pockets and perforation outlines
    /// are mirrored back (a base flange's flat is its sketch's outline as drawn). A bend row runs
    /// along the bend with the material of the panel the bend was made on (the joint's `a` panel)
    /// on the left of its direction, as the flat is read: the anchors keep that material on their
    /// left in the chart, and the mirror flips handedness, so each bend's centre line is turned
    /// round here.
    pub fn shown(&self) -> SurfaceSheet {
        let f = |p: Vec2| Vec2::new(p.x, -p.y);
        let r = |g: &Region2| rigid_region(g, f);
        let mut out = self.clone();
        out.regions = self.regions.iter().map(r).collect();
        out.slits = self.slits.iter().map(|[a, b]| [f(*a), f(*b)]).collect();
        for b in &mut out.bends {
            b.zone = r(&b.zone);
            // Turned round with the mirror: the material of the panel the bend was made on (the
            // joint's `a` panel) stays on its left.
            (b.start, b.end) = (f(b.end), f(b.start));
            b.tabs = b.tabs.iter().map(r).collect();
        }
        for l in &mut out.roll_lines {
            (l.start, l.end) = (f(l.start), f(l.end));
        }
        for p in &mut out.pockets {
            p.region = r(&p.region);
        }
        for p in &mut out.perforations {
            if let Some(o) = &mut p.outline {
                *o = o.iter().map(r).collect();
            }
        }
        out.labels = self.labels.iter().map(r).collect();
        out
    }

    /// The sheet laid out flat, from its flat (see [`flat_anchors`]): what every feature reads its
    /// flat pattern and its pieces from.
    pub fn develop(&self) -> std::result::Result<Development, solidcraft_develop::DevelopError> {
        develop(&self.sheet, &flat_anchors(&self.sheet, &self.places, &self.flat))
    }

    /// `regions` of a plane parallel to a flat panel, seen through it along the plane's normal,
    /// in the flat: through the panel nearest the plane of those that hold the first region's
    /// inner point (of two as near, the one with more material, then the first by name: never by
    /// the panels' order). None where no panel does.
    pub fn flat_of(&self, plane: &Plane, regions: &[Region2]) -> Option<Vec<Region2>> {
        let dev = self.develop().ok()?;
        let frames = crate::sheet_surface_cuts::frames(self, &dev).ok()?;
        let first = regions.first()?.interior_point();
        let normal = plane.normal();
        let chart = |frame: &Rigid3, p: Vec2| {
            let q = frame.inverse().point(plane.to_world(p));
            Vec2::new(q.x, q.y)
        };
        // Each panel that holds the point: how far its face lies from the plane, and where.
        let mut best: Option<(f64, &Panel, Rigid2, Rigid3)> = None;
        for (i, (panel, frame)) in self.sheet.panels.iter().zip(&frames).enumerate() {
            let (Surface::Plane, Some(frame), Some(Some(at))) = (&panel.surface, frame, dev.placements.get(i)) else { continue };
            if frame.z.cross(normal).len() > 1e-6 || !panel.region.outer.ccw().contains(chart(frame, first)) {
                continue;
            }
            let d = (frame.shift - plane.origin).dot(normal).abs();
            let nearer = best.as_ref().is_none_or(|(e, other, ..)| {
                d.total_cmp(e).then_with(|| other.region.area().total_cmp(&panel.region.area())).then_with(|| panel.name.cmp(&other.name)).is_lt()
            });
            if nearer {
                best = Some((d, panel, *at, *frame));
            }
        }
        let (_, _, at, frame) = best?;
        Some(regions.iter().map(|r| rigid_region(r, |p| at.point(chart(&frame, p)))).collect())
    }

    /// A perforation field's outline in the flat: closed `regions` of a sketch in `plane`, read
    /// through each flat panel whose face or other face the plane lies in, clipped to that panel
    /// (an outline on one face never reaches the next panel in the flat). Refused when the plane
    /// lies on no panel's face.
    pub fn perforation_outline(&self, plane: &Plane, regions: &[Region2]) -> Result<Vec<Region2>> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let frames = crate::sheet_surface_cuts::frames(self, &dev)?;
        let pieces = crate::sheet_surface::pieces(&dev, &mut crate::sheet::FlatWork::new())?;
        let t = self.thickness;
        let mut out = Vec::new();
        let mut found = false;
        for (i, (panel, frame)) in self.sheet.panels.iter().zip(&frames).enumerate() {
            let (Surface::Plane, Some(frame), Some(Some(at))) = (&panel.surface, frame, dev.placements.get(i)) else { continue };
            let z = (plane.origin - frame.shift).dot(frame.z);
            if frame.z.cross(plane.normal()).len() > 1e-6 || !(-1e-6..=t + 1e-6).contains(&z) {
                continue;
            }
            found = true;
            let inv = frame.inverse();
            let map = |p: Vec2| {
                let q = inv.point(plane.to_world(p));
                at.point(Vec2::new(q.x, q.y))
            };
            let mapped: Vec<Region2> = regions.iter().map(|r| rigid_region(r, map)).collect();
            // The panel's material in the flat, short of its bends.
            let own: Vec<Region2> =
                pieces.iter().filter(|(p, _)| matches!(p, crate::sheet_surface::Piece::Panel(k) if *k == i)).map(|(_, r)| r.clone()).collect();
            out.extend(solidcraft_kernel::planar_regions_boolean(&own, &mapped, solidcraft_kernel::BoolOp::Intersect, 1e-9)?);
        }
        if !found {
            return err("the perforation's outline sketch must lie on a face of the sheet");
        }
        Ok(out)
    }

    /// Each roll of the flat (see [`RollInfo`]), from its lines: its length is its middle line's
    /// reach, its width how far its start and end lines lie apart.
    pub fn roll_infos(&self) -> Vec<RollInfo> {
        let t = self.sheet.thickness;
        let mut out = Vec::new();
        for (i, p) in self.sheet.panels.iter().enumerate() {
            let radius = match p.surface {
                Surface::Cylinder { radius, material_inward } => Some(if material_inward { radius - t } else { radius }),
                Surface::Cone { .. } => None,
                Surface::Plane | Surface::Spline(_) => continue,
            };
            let lines = |kind: RollEdge| self.roll_lines.iter().filter(move |l| l.panel == i && l.kind == kind);
            let Some(d) = lines(RollEdge::Middle).next().and_then(|l| (l.end - l.start).normalized()) else { continue };
            let along: Vec<f64> = lines(RollEdge::Middle).flat_map(|l| [l.start.dot(d), l.end.dot(d)]).collect();
            let length = along.iter().copied().fold(f64::NEG_INFINITY, f64::max) - along.iter().copied().fold(f64::INFINITY, f64::min);
            let across = |kind: RollEdge| lines(kind).next().map(|l| l.start.lerp(l.end, 0.5).dot(d.perp()));
            let (Some(a), Some(b)) = (across(RollEdge::Start), across(RollEdge::End)) else { continue };
            let pts = p.region.outer.polyline(1e-3);
            let (lo, hi) = (&pts).into_iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q.y), h.max(q.y)));
            out.push(RollInfo { index: out.len() + 1, panel: i, radius, angle_deg: (hi - lo).to_degrees(), length, width: (a - b).abs() });
        }
        out
    }

    /// The sheet as the body it was converted from has it, `own` thick and its modelled bends
    /// at the radii `modelled` gives (joint, inside radius), not the rule's: what to read the
    /// body's own engravings against.
    pub fn as_modelled(&self, own: f64, modelled: &[(usize, f64)]) -> SurfaceSheet {
        let mut out = self.clone();
        out.sheet.thickness = own;
        for (j, r) in modelled {
            if let Some(Joint { kind: JointKind::Bend(b), .. }) = out.sheet.joints.get_mut(*j) {
                b.radius = *r;
            }
        }
        out
    }

    /// Lays the sheet out again after a change to it, and builds its body: the folded sheet, or
    /// its flat while it is unfolded.
    pub fn build(&mut self) -> Result<Body> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        if let Some(m) = dev.loops.first() {
            let panel = self.sheet.joints.get(m.joint).map_or(0, |j| j.a.panel);
            let name = self.sheet.panels.get(panel).map_or("", |p| p.name.as_str());
            return err(format!("the sheet closes a ring of bends and seams at {name}: cut one of the ring's joints open"));
        }
        self.settle(&dev)?;
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let unrelieved = self.unrelieved(&dev)?;
        self.notes.extend(unrelieved);
        let mut work = FlatWork::new();
        let pieces = crate::sheet_surface::pieces(&dev, &mut work)?;
        let regions = crate::sheet_surface::join_pieces(&pieces, &mut work)?;
        self.laid_out(&dev, &pieces, regions)?;
        // The loose pieces lie in the flat where their panels do.
        let loose: Vec<Region2> =
            self.loose.iter().filter_map(|l| dev.placements.get(l.panel).copied().flatten().map(|at| at.region(&l.region))).collect();
        self.regions.extend(loose);
        // Each panel's frame: folded, or laid flat in the sheet's flat (see `SurfaceSheet::flat`).
        let (folded, frames) = if self.unfolded {
            let base = self.flat;
            (None, dev.placements.iter().map(|p| p.map(|r| crate::sheet_surface_cuts::lift(&base, &r))).collect::<Vec<_>>())
        } else {
            let folded = fold_onto(&self.sheet, &dev, &self.places)?;
            let frames = folded.places.clone();
            (Some(folded), frames)
        };
        let namer = crate::sheet::FaceNamer::new(&self.sheet, &dev, &pieces, &self.sketch_curves, &self.recipes);
        // What the builder made each face for, in the order it makes them: each face's name from
        // the builder is its place in that order, read back below.
        let made = std::cell::RefCell::new(Vec::new());
        let keep = |p: crate::sheet_surface::Piece, r: crate::sheet::FaceRole| {
            let mut made = made.borrow_mut();
            made.push((p, r));
            Some((made.len() - 1).to_string())
        };
        let (mut body, keys) = match &folded {
            Some(folded) => crate::sheet_surface_fold::solid(&dev, folded, self.thickness, &pieces, &mut work, &keep)?,
            // Laid flat in the sheet's flat: the same faces, so the flat's are the folded
            // sheet's (#7990).
            None => crate::sheet_surface_fold::solid_flat(&dev, self.flat, self.thickness, &pieces, &mut work, &keep)?,
        };
        let made = made.into_inner();
        let at = |k: &Option<String>| k.as_deref().and_then(|k| k.parse::<usize>().ok()).and_then(|k| made.get(k).copied());
        // What each face was made for, and a panel's cap's sketch frame (the flat pattern's).
        type Made = (Option<(SheetPart, Option<f64>)>, Option<Plane>);
        let mut names: Vec<Option<Made>> = keys
            .iter()
            .map(|k| {
                at(k).map(|(p, r)| {
                    let frame = match (p, r) {
                        (crate::sheet_surface::Piece::Panel(i), crate::sheet::FaceRole::Cap { surface, .. }) => {
                            frames.get(i).copied().flatten().and_then(|f| cap_frame(&dev, f, i, surface, self.thickness))
                        }
                        _ => None,
                    };
                    (namer.part(p, r, &self.holes), frame)
                })
            })
            .collect();
        // Each loose piece where its panel is.
        for l in &self.loose {
            let frame = frames.get(l.panel).copied().flatten().ok_or_else(|| DocError::Invalid("a loose piece's panel has no place".into()))?;
            let plane = Plane::new(frame.shift, frame.x, frame.y).ok_or_else(|| DocError::Invalid("a loose piece's panel has no plane".into()))?;
            let piece = solidcraft_kernel::extrude_planar_regions(&plane, std::slice::from_ref(&l.region), 0.0, self.thickness)?;
            body = solidcraft_kernel::boolean(&body, &piece, solidcraft_kernel::BoolOp::Union)?
                .ok_or_else(|| DocError::Invalid("a loose piece".into()))?;
            names = crate::sheet::carried(&names, &body);
        }
        let (body, names) = self.recessed_named(body, names, &frames)?;
        let (body, names) = self.bored_named(body, names, &frames)?;
        self.parts = crate::sheet::ranked(names.iter().map(|n| n.as_ref().and_then(|n| n.0.clone())).collect());
        self.frames = names.into_iter().map(|n| n.and_then(|n| n.1)).collect();
        Ok(body)
    }

    /// [`Self::recessed`], carrying `names`, its faces' names, through each cut.
    fn recessed_named<T: Clone>(&self, body: Body, names: Vec<Option<T>>, frames: &[Option<Rigid3>]) -> Result<(Body, Vec<Option<T>>)> {
        let mut out = (body, names);
        for r in &self.recesses {
            let one = SurfaceSheet { recesses: vec![r.clone()], ..self.clone() };
            let body = one.recessed(out.0, frames)?;
            out.1 = crate::sheet::carried(&out.1, &body);
            out.0 = body;
        }
        Ok(out)
    }

    /// [`Self::bored`], carrying `names`, its faces' names, through each bore.
    fn bored_named<T: Clone>(&self, body: Body, names: Vec<Option<T>>, frames: &[Option<Rigid3>]) -> Result<(Body, Vec<Option<T>>)> {
        let mut out = (body, names);
        for b in &self.bores {
            let one = SurfaceSheet { bores: vec![*b], ..self.clone() };
            let body = one.bored(out.0, frames)?;
            out.1 = crate::sheet::carried(&out.1, &body);
            out.0 = body;
        }
        Ok(out)
    }

    /// `body` with the sheet's bores cut from it, each from its panel where `frames` put it.
    fn bored(&self, mut body: Body, frames: &[Option<Rigid3>]) -> Result<Body> {
        for b in &self.bores {
            let frame =
                b.panel.and_then(|p| frames.get(p).copied().flatten()).ok_or_else(|| DocError::Invalid("a bored panel has no place".into()))?;
            let axis = frame.vector(b.axis);
            let base = frame.point(b.base);
            let Some(plane) = axis.normalized().and_then(|a| Plane::new(base, a.any_perp(), a.cross(a.any_perp()))) else { continue };
            let circle = Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), b.radius), holes: Vec::new() };
            let (from, to) = (b.height.min(0.0), b.height.max(0.0));
            let tool = solidcraft_kernel::extrude_planar_regions(&plane, std::slice::from_ref(&circle), from, to)?;
            body = solidcraft_kernel::boolean(&body, &tool, solidcraft_kernel::BoolOp::Cut)?
                .ok_or_else(|| DocError::Invalid("a bore takes the whole sheet".into()))?;
        }
        Ok(body)
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

    /// Where `dev` cut no bend relief past a bend's end that a panel runs on past, the relief
    /// having no width: a note for each, at the end on that panel's tangent line.
    fn unrelieved(&self, dev: &Development) -> Result<Vec<String>> {
        if dev.unrelieved.is_empty() {
            return Ok(Vec::new());
        }
        let places = fold_onto(&self.sheet, dev, &self.places)?.places;
        let t = self.sheet.thickness;
        let mut notes = Vec::new();
        for &(j, panel, u) in &dev.unrelieved {
            let Some(Joint { a, b, kind: JointKind::Bend(bend) }) = self.sheet.joints.get(j) else { continue };
            // The `a` panel's material lies left of its anchor, the `b` panel's right.
            let (an, side) = if a.panel == panel { (a, 1.0) } else { (b, -1.0) };
            let c = an.origin + an.dir * u + an.dir.perp() * (side * bend.setback(t));
            let Some(place) = places.get(panel).copied().flatten() else { continue };
            let note =
                format!("no bend relief at {}: the relief width {:.4} mm leaves nothing to cut", at(place.point(c.extend(0.0))), self.reliefs.width);
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
        Ok(notes)
    }

    /// Settles where the sheet's bends end (see [`settle`]), from where its development `dev`
    /// folds its panels.
    fn settle(&mut self, dev: &Development) -> Result<()> {
        let folded = fold_onto(&self.sheet, dev, &self.places)?;
        let places: Vec<Rigid3> =
            folded.places.iter().copied().collect::<Option<_>>().ok_or_else(|| DocError::Invalid("a panel of the sheet has no place".into()))?;
        let reach = dev.panels.iter().flatten().map(|p| p.region.outer.polyline(1e-3).bounds().size().len()).fold(1.0, f64::max);
        let own = self.corner_reliefs.clone();
        // The bend whose feature comes later in the timeline overrides (never the joints' order,
        // which is when they were made); two bends of one feature ask for the same relief.
        let chosen = move |[x, y]: [usize; 2]| {
            let of = |j: usize| own.iter().find(|(k, ..)| *k == j).copied();
            let later = match (of(x), of(y)) {
                (Some(a), Some(b)) if b.3 > a.3 => Some(b),
                (a, b) => a.or(b),
            };
            later.map(|(_, r, s, _)| (r, s))
        };
        let junctions = self.junctions.clone();
        let butt = move |joint: usize, at_end: bool| butts(&junctions, joint, at_end);
        self.notes = settle(&mut self.sheet, &places, &self.reliefs, &chosen, &butt, (reach * 1e-6).max(1e-6), &at)?;
        Ok(())
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
    /// bends and their tabs, stretch, rolls and their lines, and its panels' rims.
    fn laid_out(&mut self, dev: &Development, pieces: &[(Piece, Region2)], regions: Vec<Region2>) -> Result<()> {
        self.rims = crate::sheet::rims(&self.sheet, dev, pieces, &self.sketch_curves, &self.recipes);
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
                    // Up toward the reader of the flat, who sees it from the surface (see
                    // `shown`): away from the material.
                    up: !b.toward_material,
                    allowance: b.allowance(self.sheet.thickness, self.sheet.k_factor),
                    tabs: pieces.iter().filter(|(p, _)| matches!(p, Piece::Tab(t, _) if *t == k)).map(|(_, r)| r.clone()).collect(),
                    joint: z.joint,
                })
            })
            .collect();
        self.regions = regions;
        self.slits = junction_slits(&self.sheet, dev, &self.junctions).into_iter().flat_map(|s| in_material(&self.regions, s)).collect();
        self.stretch = dev.panels.iter().flatten().map(|p| p.stretch).fold(0.0, f64::max);
        self.rolls = self.sheet.panels.iter().filter(|p| p.surface != Surface::Plane).count();
        self.roll_lines =
            if self.rolls > 0 { roll_lines(&self.sheet, &fold_onto(&self.sheet, dev, &self.places)?, &self.regions) } else { Vec::new() };
        self.thickness = self.sheet.thickness;
        self.k_factor = self.sheet.k_factor;
        let t = self.thickness;
        self.pockets = self
            .recesses
            .iter()
            .filter_map(|r| {
                let place = dev.placements.get(r.panel).copied().flatten()?;
                // The sheet's surface is its top (z t), the face one thickness into the material
                // its bottom (z 0), folded or flat: a pocket's side is the face it is on.
                let (z0, z1) = if r.far { (0.0, r.depth) } else { (t - r.depth, t) };
                Some(crate::sheet::SheetPocket { region: place.region(&r.region), z0, z1 })
            })
            .collect();
        Ok(())
    }
}
