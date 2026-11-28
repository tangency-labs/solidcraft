//! Healing tolerance-scale defects of a B-rep by exact topology edits whose every decision is a
//! certified bound (not upstream): edges that lie wholly within their tolerance of one of their
//! vertices are squeezed into it, the edges that met the removed vertex are re-ended exactly at
//! the survivor, and the faces this leaves bounded by two coincident edges are collapsed.
//!
//! Upstream: none. SMLib's healer has the same aim in `SmHealData::Fix_DegenEdges`
//! ([`crate::heal::heal_data`]), but it finds degenerate edges by a sampled length against the
//! context's approximation tolerance (so an edge shorter than its own, larger tolerance is not
//! one), always keeps the start vertex, and leaves the neighbour edges' curves ending at the
//! vertex it deletes. The healer's step now decides with [`short_edge_bound`] and squeezes with
//! [`collapse_short_edge`] (`HealData::fix_degen_edges`), each edge into its own midpoint.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`short_edge_bound`], [`squeeze_targets`], [`collapse_short_edge`], [`collapse_short_edges`] | none (`SmHealData::Fix_DegenEdges` does the squeeze uncertified) |
//! | [`collapse_coincident_two_edge_faces`], [`cut_degenerate_pieces`], [`heal_short_edges_and_slivers`] | none |
//!
//! **Certificates.** Every acceptance is an outward-rounded bound over the whole curve, never a
//! sample (`docs/definitions-and-certificates.md` §7.5, §7.6):
//! - vertices joined by edges shorter than their tolerance form clusters, and each cluster's
//!   point `P` is the centre of the box of its vertices ([`squeeze_targets`]; for one short
//!   edge, the midpoint of its two vertices). An edge is squeezed into `P` only when its whole
//!   curve lies within the edge's tolerance of `P`
//!   ([`crate::heal::stitch::bounds::curve_to_point`]: interval de Boor enclosures of every
//!   knot span). The vertex kept moves to `P`, the other goes, and every edge that met either
//!   vertex has that end moved to `P`;
//! - each such edge gets a new curve whose end is `P`: a line is rebuilt through its other end
//!   and `P`; any other curve with a B-spline form has that form, trimmed to the edge, with its
//!   end control point moved onto `P` (a clamped end is its end control point). The new curve
//!   is accepted only when its whole distance from every face the edge bounds is certified
//!   within half the edge's tolerance ([`crate::query::curve_surface_gap::curve_face_gap_bound`];
//!   half: SMLib holds an edge's tolerance to at least twice its gap to its faces, and raises it
//!   to that when a trim is attached), its end within half the vertex's tolerance of `P`
//!   ([`crate::heal::stitch::bounds::point_at_curve`]), and its other end (which the trimmed
//!   form may have snapped to a knot) within its own vertex's tolerance of that vertex;
//! - a trim on a plane is made exactly: the plane's parameters are an affine map of space, so
//!   the curve's control points mapped into them give the curve's projection onto the plane, on
//!   the curve's own knots and weights. On any other surface SMLib's trim maker makes it
//!   (`CreateUVTrimCurve`), which raises the edge's tolerance when it needs to: the heal's
//!   caller refuses a result where any tolerance grew;
//! - a face bounded by two edges between the same two vertices is collapsed only when the two
//!   curves are within the kept edge's tolerance of each other everywhere
//!   ([`crate::heal::stitch::bounds::curves`], under their endpoint correspondence), the kept
//!   edge's curve is within half its tolerance of the face across the removed edge (the face
//!   that will use it), and the kept edge's tolerance is the larger;
//! - a piece of no length is cut out of a B-spline (a Bézier piece between two C0 joints whose
//!   control points the check calls one point) only when the cut moves the curve no farther
//!   than the edge's tolerance and the new curve is within half of it from its faces. Several
//!   cuts on one edge are bounded by the sum of their bounds, from the curve as read.
//!
//! No tolerance is grown: what cannot be certified is left as it is and reported with why
//! ([`Refusal`]).
//!
//! Deviations: no recursion. One squeeze removes an edge, so the passes are bounded by the
//! edge count, each at most [`MAX_EDGES`] edges; cancellation is polled per edge (in the
//! squeeze's clusters too) and per cut.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::EntityRef;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::ZONE_TOL_3D;
use crate::geom::vector2d::Point2;
use crate::heal::stitch::bounds;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p2::ControlPointForm;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::crv_basic::Curve as Nurbs;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::query::curve_surface_gap::curve_face_gap_bound;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1;
use crate::topology::edge::{self, NotifyEnv};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3 as eu3;
use crate::topology::face::p2 as face2;
use crate::topology::loopuse as lu1;
use crate::topology::{vertex, Brep, EdgeId, FaceId, VertexId, VertexuseId};

const OP: &str = "heal::tolerance_defects";

/// The most edges one pass looks at.
pub const MAX_EDGES: usize = 1 << 20;

/// How far a curve may lie from a face it bounds, as a share of its edge's tolerance: SMLib
/// holds an edge's tolerance to at least twice that gap (`SmEdgeuse::SetUVTrimCurve` raises it
/// to twice the gap it is given).
const GAP_SHARE: f64 = 0.5;

/// Why an edit was not made: the proof it needs was not found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refusal {
    /// The edge is closed (one vertex at both ends).
    Closed,
    /// No enclosure of the edge's curve is known (a curve kind the bounds do not cover).
    NotEnclosed,
    /// The edge reaches farther than its tolerance from the midpoint of its vertices, where it
    /// would be squeezed: the certified bound.
    TooLong(f64),
    /// Another edge also joins the edge's two vertices: squeezing would close it on itself.
    Parallel(EdgeId),
    /// An edge meeting the edge's vertices is closed there.
    NeighbourClosed(EdgeId),
    /// An edge meeting the edge's vertices has no curve that can be re-ended (no line and no
    /// B-spline form).
    NeighbourKind(EdgeId),
    /// An edge meeting the edge's vertices, re-ended, is not certified within half its
    /// tolerance of one of its faces: the bound, or `None` when none could be found.
    NeighbourOffFace(EdgeId, FaceId, Option<f64>),
    /// An edge meeting the edge's vertices, re-ended, does not end within half the vertex's
    /// tolerance of the new point, or its other end is not within its own vertex's tolerance
    /// of that vertex: the bound.
    NeighbourEnd(EdgeId, f64),
    /// The new point is not certified within half the kept vertex's tolerance of a face
    /// around it (bounded by the gap of a re-ended edge's whole curve to that face plus the
    /// new point's distance from that curve's end): the bound.
    VertexOffFace(FaceId, f64),
    /// The two edges of a two-edge face are not certified within the kept edge's tolerance of
    /// each other: the bound, or infinity when none could be found.
    NotCoincident(f64),
    /// The kept edge of a two-edge face is not certified within half its tolerance of the
    /// face across the other edge: the bound, or `None`.
    KeptOffFace(FaceId, Option<f64>),
    /// A two-edge face proved coincident was left by the collapse, whose loop area test did
    /// not find it of zero area.
    NotZeroArea,
    /// The kept edge's tolerance is smaller than the other's: collapsing would grow it.
    WouldGrow,
    /// A piece of no length in the edge's curve could not be cut out within the edge's
    /// tolerance: the certified bound of how far that cut, with the edge's cuts before it,
    /// would move the curve as read.
    PieceTooLong(f64),
    /// The edge's curve, a piece of no length cut out, is not certified within half its
    /// tolerance of one of its faces: the bound, or `None`.
    CutOffFace(FaceId, Option<f64>),
    /// The edge's curve, a piece of no length cut out, does not end within its vertex's
    /// tolerance of the vertex: the bound.
    CutEnd(f64),
}

/// An edge squeezed into a vertex.
#[derive(Clone, Debug, PartialEq)]
pub struct Squeezed {
    /// The edge removed.
    pub edge: EdgeId,
    /// The vertex kept, moved to the midpoint of the two.
    pub survivor: VertexId,
    /// The vertex removed.
    pub removed: VertexId,
    /// The certified bound of the edge's distance from the midpoint (at most its tolerance).
    pub bound: f64,
    /// How far the vertex kept moved (half the distance between the two vertices).
    pub moved: f64,
    /// The edges re-ended at the midpoint, each with the largest certified gap of its new
    /// curve to its faces.
    pub re_ended: Vec<(EdgeId, f64)>,
}

/// What [`collapse_short_edges`] and [`collapse_coincident_two_edge_faces`] did, and what they
/// would have done but could not prove.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Healed {
    /// The edges squeezed, in order.
    pub squeezed: Vec<Squeezed>,
    /// The two-edge faces collapsed, each with the certified bound of its edges' distance.
    pub collapsed: Vec<(FaceId, f64)>,
    /// Pieces of no length cut out of edge curves: the edge and the certified bound of how far
    /// its cuts, together, moved the curve.
    pub cut_pieces: Vec<(EdgeId, f64)>,
    /// Edges shorter than their tolerance by their vertices that were left, with why, and
    /// edges whose pieces of no length were left uncut, with why ([`Refusal::is_cut`]).
    pub refused_edges: Vec<(EdgeId, Refusal)>,
    /// Two-edge faces that were left, with why.
    pub refused_faces: Vec<(FaceId, Refusal)>,
}

impl Refusal {
    /// Whether this is a refusal of [`cut_degenerate_pieces`] (which runs once, before the
    /// squeezes) rather than of a squeeze or collapse.
    pub fn is_cut(&self) -> bool {
        matches!(
            self,
            Refusal::PieceTooLong(_) | Refusal::CutOffFace(..) | Refusal::CutEnd(_)
        )
    }
}

fn invalid(what: &'static str) -> SolidError {
    SolidError::invalid_input(OP, what)
}

fn curve_of(b: &Brep, e: EdgeId) -> Result<&Curve> {
    edge::curve(b, e)?.ok_or_else(|| invalid("an edge without a curve"))
}

/// The vertex an edge's squeeze keeps (the end with the larger tolerance; the start on a tie)
/// and the other, or `None` for a closed edge.
fn ends(b: &Brep, e: EdgeId) -> Result<Option<(VertexId, VertexId)>> {
    let (s, t) = edge::start_end_vertices(b, e)?;
    let (Some(s), Some(t)) = (s, t) else {
        return Err(invalid("an edge without its vertices"));
    };
    if s == t {
        return Ok(None);
    }
    Ok(Some(
        if vertex::tolerance(b, t)? > vertex::tolerance(b, s)? {
            (t, s)
        } else {
            (s, t)
        },
    ))
}

/// `x <= bound`, false for a NaN.
fn within(x: f64, bound: f64) -> bool {
    x <= bound
}

/// `x > 0`, false for a NaN.
fn positive(x: f64) -> bool {
    x > 0.0
}

/// The midpoint of two points.
fn midpoint(a: Point3, c: Point3) -> Point3 {
    Point3::new(0.5 * (a.x + c.x), 0.5 * (a.y + c.y), 0.5 * (a.z + c.z))
}

/// A certified bound of how far edge `e`'s curve, over its whole interval, lies from `p`, the
/// point a squeeze would put its two vertices at (by default the midpoint of the two):
/// `Err(Refusal)` for a closed edge or one whose curve has no enclosure. The edge is short (lies
/// within its tolerance of the point) when the bound is at most [`edge::tolerance`].
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling edge; `InvalidInput` for an edge without a curve
/// or vertices.
pub fn short_edge_bound(
    b: &Brep,
    e: EdgeId,
    p: Option<Point3>,
) -> Result<std::result::Result<f64, Refusal>> {
    let Some((v, w)) = ends(b, e)? else {
        return Ok(Err(Refusal::Closed));
    };
    let p = match p {
        Some(p) => p,
        None => midpoint(vertex::point(b, v)?, vertex::point(b, w)?),
    };
    let c = curve_of(b, e)?;
    Ok(match bounds::curve_to_point(c, &edge::interval(b, e)?, p) {
        Ok(r) if r.is_finite() => Ok(r),
        _ => Err(Refusal::NotEnclosed),
    })
}

/// The root of `i`'s cluster in the disjoint-set forest `parent` ([`squeeze_targets`]): its
/// smallest vertex index, as a parent is always a smaller index (so the walk ends). Every
/// vertex on the walk is pointed at the root afterwards (path compression), so a cluster
/// joined as a chain is walked in full once, not once per vertex; the roots stay the same.
fn root(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while let Some(&q) = parent.get(r) {
        if q == r {
            break;
        }
        r = q;
    }
    let mut x = i;
    while x != r {
        let Some(slot) = parent.get_mut(x) else {
            break;
        };
        let next = *slot;
        *slot = r;
        x = next;
    }
    r
}

/// The point each vertex goes to when the short edges around it are squeezed: vertices joined
/// by edges whose ends lie within the edge's tolerance of each other (the candidates) form
/// clusters, and a cluster goes to the centre of the box of its points, which no point of it is
/// farther from than half the box's diagonal. One point per cluster, fixed before any squeeze,
/// so a cluster of several short edges (a sliver triangle) ends at one place however its
/// squeezes are ordered; vertices of no cluster are not listed.
///
/// Cancellation is polled per edge.
///
/// # Errors
///
/// Those of the topology reads; `Cancelled` when `k`'s token is.
pub fn squeeze_targets(k: &Kernel, b: &Brep) -> Result<Vec<(VertexId, Point3)>> {
    let vertices = p1::vertices(b, None)?;
    let positions: BTreeMap<VertexId, usize> =
        vertices.iter().enumerate().map(|(i, &v)| (v, i)).collect();
    let index = |v: VertexId| positions.get(&v).copied();
    let mut parent: Vec<usize> = (0..vertices.len()).collect();
    let mut joined = vec![false; vertices.len()];
    for e in p1::edges(b, None)? {
        k.check_cancelled(OP)?;
        let Some((v, w)) = ends(b, e)? else {
            continue;
        };
        let d = bounds::points(vertex::point(b, v)?, vertex::point(b, w)?)?;
        if !within(d, edge::tolerance(b, e)?) {
            continue;
        }
        let (Some(i), Some(j)) = (index(v), index(w)) else {
            continue;
        };
        let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
        let (lo, hi) = (ri.min(rj), ri.max(rj));
        if let Some(slot) = parent.get_mut(hi) {
            *slot = lo;
        }
        for x in [i, j] {
            if let Some(slot) = joined.get_mut(x) {
                *slot = true;
            }
        }
    }
    let mut boxes: BTreeMap<usize, (Point3, Point3)> = BTreeMap::new();
    for (i, &v) in vertices.iter().enumerate() {
        if !joined.get(i).copied().unwrap_or(false) {
            continue;
        }
        let r = root(&mut parent, i);
        let p = vertex::point(b, v)?;
        boxes
            .entry(r)
            .and_modify(|(lo, hi)| {
                *lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                *hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            })
            .or_insert((p, p));
    }
    let mut out = Vec::new();
    for (i, &v) in vertices.iter().enumerate() {
        if !joined.get(i).copied().unwrap_or(false) {
            continue;
        }
        let r = root(&mut parent, i);
        if let Some((lo, hi)) = boxes.get(&r) {
            out.push((v, midpoint(*lo, *hi)));
        }
    }
    Ok(out)
}

/// The B-spline that [`curve_face_gap_bound`] encloses for `c`: a line's own B-spline part, a
/// B-spline curve itself.
fn b_spline_of(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(s) => Some(s),
        CurveKind::Line(l) => Some(&l.base),
        _ => None,
    }
}

/// Edge `n`'s curve re-ended at `p` at its start (`at_start`) or its end: a line rebuilt
/// through its other end, any other curve's trimmed B-spline form with that end's control
/// point moved onto `p`. `None` for a curve with neither.
fn re_ended(k: &Kernel, b: &Brep, n: EdgeId, at_start: bool, p: Point3) -> Result<Option<Curve>> {
    let c = curve_of(b, n)?;
    let ivl = edge::interval(b, n)?;
    if let CurveKind::Line(_) = c.kind() {
        let (start, end) = if at_start {
            (p, c.evaluate_point(k, ivl.max())?)
        } else {
            (c.evaluate_point(k, ivl.min())?, p)
        };
        return Ok(Line::create_line_segment(3, start, end, None)
            .ok()
            .map(Curve::from));
    }
    let Some(mut t) = edge::create_trimmed_nurbs_curve(k, b, n)? else {
        return Ok(None);
    };
    let CurveKind::BSpline(s) = t.kind_mut() else {
        return Ok(None);
    };
    let count = s.control_points()?.len();
    let Some(last) = count.checked_sub(1) else {
        return Ok(None);
    };
    let index = if at_start { 0 } else { last };
    let (_, w) = s.control_point(ControlPointForm::EuclideanRational, index)?;
    if !positive(w) {
        return Ok(None);
    }
    s.set_control_point(ControlPointForm::EuclideanRational, index, p, w)?;
    Ok(Some(t))
}

/// The certified gap of curve `c` (over its natural interval) to each face edge `n` bounds,
/// each within [`GAP_SHARE`] of the edge's tolerance: `Err(face, bound)` for the first that is
/// not.
fn face_gaps(
    k: &Kernel,
    b: &Brep,
    n: EdgeId,
    c: &Curve,
) -> Result<std::result::Result<Gaps, (FaceId, Option<f64>)>> {
    let tol = edge::tolerance(b, n)?;
    let range = c.natural_interval();
    let mut gaps = Vec::new();
    for f in edge::faces(b, n, None)? {
        let gap = match b_spline_of(c) {
            Some(s) => curve_face_gap_bound(k, b, f, s, [range.min(), range.max()], tol / 100.0)?,
            None => None,
        };
        match gap {
            Some(g) if g <= GAP_SHARE * tol => gaps.push((f, g)),
            other => return Ok(Err((f, other))),
        }
    }
    Ok(Ok(gaps))
}

/// A curve's certified gap to each face its edge bounds.
type Gaps = Vec<(FaceId, f64)>;

/// An edge re-ended for a squeeze: the edge, its new curve and its certified face gaps.
type ReEnded = (EdgeId, Curve, Vec<(FaceId, f64)>);

/// Whether curve `c` at parameter `t` is certified within vertex `x`'s tolerance of its point:
/// `Err(bound)` when it is not (infinite when no bound was found).
fn end_on_vertex(
    b: &Brep,
    x: Option<VertexId>,
    c: &Curve,
    t: f64,
) -> Result<std::result::Result<(), f64>> {
    let Some(x) = x else {
        return Err(invalid("an edge without its vertices"));
    };
    Ok(match bounds::point_at_curve(vertex::point(b, x)?, c, t) {
        Ok(d) if within(d, vertex::tolerance(b, x)?) => Ok(()),
        Ok(d) => Err(d),
        Err(_) => Err(f64::INFINITY),
    })
}

/// The new curve of each edge meeting `v` or `w` (but `e`), re-ended at `p` and certified
/// (module docs); `Err(Refusal)` for the first that is not.
fn certified_neighbours(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    v: VertexId,
    w: VertexId,
    p: Point3,
) -> Result<std::result::Result<Vec<ReEnded>, Refusal>> {
    let vertex_tol = vertex::tolerance(b, v)?;
    let mut out: Vec<ReEnded> = Vec::new();
    let mut seen: Vec<EdgeId> = Vec::new();
    for n in vertex::edges(b, v, None)?
        .into_iter()
        .chain(vertex::edges(b, w, None)?)
    {
        if n == e || seen.contains(&n) {
            continue;
        }
        seen.push(n);
        let (s, t) = edge::start_end_vertices(b, n)?;
        let at = |x: Option<VertexId>| x == Some(v) || x == Some(w);
        if s == t {
            return Ok(Err(Refusal::NeighbourClosed(n)));
        }
        if at(s) && at(t) {
            return Ok(Err(Refusal::Parallel(n)));
        }
        let at_start = at(s);
        let Some(c) = re_ended(k, b, n, at_start, p)? else {
            return Ok(Err(Refusal::NeighbourKind(n)));
        };
        let range = c.natural_interval();
        let (end, far, far_vertex) = if at_start {
            (range.min(), range.max(), t)
        } else {
            (range.max(), range.min(), s)
        };
        let near = match bounds::point_at_curve(p, &c, end) {
            Ok(d) if d <= GAP_SHARE * vertex_tol => d,
            Ok(d) => return Ok(Err(Refusal::NeighbourEnd(n, d))),
            Err(_) => return Ok(Err(Refusal::NeighbourEnd(n, f64::INFINITY))),
        };
        // the trimmed form may snap its interval to a knot: its far end, unmoved, must still
        // meet its own vertex within that vertex's tolerance
        if let Err(d) = end_on_vertex(b, far_vertex, &c, far)? {
            return Ok(Err(Refusal::NeighbourEnd(n, d)));
        }
        match face_gaps(k, b, n, &c)? {
            Ok(gaps) => {
                // the new point is within `near` of the curve's end, so within `g + near` of
                // each face the curve is within `g` of (the sum rounded up, never below the
                // exact one)
                let off = |g: f64| crate::geom::certified::next_up(g + near);
                if let Some(&(f, g)) = gaps
                    .iter()
                    .find(|(_, g)| !within(off(*g), GAP_SHARE * vertex_tol))
                {
                    return Ok(Err(Refusal::VertexOffFace(f, off(g))));
                }
                out.push((n, c, gaps));
            }
            Err((f, bound)) => return Ok(Err(Refusal::NeighbourOffFace(n, f, bound))),
        }
    }
    Ok(Ok(out))
}

/// The plane of `surface` over `domain` as an affine map: its point at the domain's lower
/// corner and its two parameter derivatives; `None` for a surface that is not a plane, or
/// whose points at the domain's corners are not that map's (to a relative 1e-12).
fn plane_map(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
) -> Result<Option<(Point3, Vec3, Vec3, Point2)>> {
    if !matches!(surface.kind(), SurfaceKind::Plane(_)) {
        return Ok(None);
    }
    let (u0, u1, v0, v1) = (
        domain.u_min(),
        domain.u_max(),
        domain.v_min(),
        domain.v_max(),
    );
    if !(u0 < u1 && v0 < v1) {
        return Ok(None);
    }
    let o = surface.evaluate_point(k, Point2::new(u0, v0))?;
    let a = surface.evaluate_point(k, Point2::new(u1, v0))?;
    let c = surface.evaluate_point(k, Point2::new(u0, v1))?;
    let d = surface.evaluate_point(k, Point2::new(u1, v1))?;
    let su = (a - o) * (1.0 / (u1 - u0));
    let sv = (c - o) * (1.0 / (v1 - v0));
    let fourth = o + su * (u1 - u0) + sv * (v1 - v0);
    let size = (a - o).length() + (c - o).length() + o.length();
    if !within((fourth - d).length(), 1e-12 * size) {
        return Ok(None);
    }
    Ok(Some((o, su, sv, Point2::new(u0, v0))))
}

/// The trim of curve `c` on a plane given as an affine map ([`plane_map`]): its control points
/// mapped into the plane's parameters, on its own knots and weights, so the trim's points are
/// the curve's points projected onto the plane.
fn planar_trim(c: &Curve, map: &(Point3, Vec3, Vec3, Point2)) -> Result<Option<Curve>> {
    let Some(s) = b_spline_of(c) else {
        return Ok(None);
    };
    let (o, su, sv, base) = *map;
    let nurb = s.nurb_ref()?;
    // the normal equations of [su sv] x = q - o
    let (a11, a12, a22) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let det = a11 * a22 - a12 * a12;
    if !positive(det) {
        return Ok(None);
    }
    let mut cpts = Vec::new();
    for q in nurb.cpts() {
        let w = if q.w == NOW { 1.0 } else { q.w };
        if !positive(w) {
            return Ok(None);
        }
        let p = Point3::new(q.x / w, q.y / w, q.z / w);
        let r = p - o;
        let (b1, b2) = (su.dot(r), sv.dot(r));
        let u = base.x + (a22 * b1 - a12 * b2) / det;
        let v = base.y + (a11 * b2 - a12 * b1) / det;
        cpts.push(CPoint {
            x: if q.w == NOW { u } else { u * w },
            y: if q.w == NOW { v } else { v * w },
            z: NOZ,
            w: q.w,
        });
    }
    let trim = Nurbs::new(cpts, nurb.degree(), nurb.knots().to_vec())?;
    Ok(Some(Curve::from(BSplineCurve::from_nurb(trim, 2, false))))
}

/// Makes the trims of edge `e`'s uses again from its curve, after its old ones are deleted:
/// on a plane exactly ([`planar_trim`]), attached with its certified gap to the plane; on any
/// other surface by SMLib's trim maker. A use on no face (a wire's) has no trim.
fn remake_trims(k: &Kernel, b: &mut Brep, e: EdgeId, gaps: &[(FaceId, f64)]) -> Result<()> {
    for eu in edge::edgeuses(b, e, None, None)? {
        eu1::delete_uv_trim_curve(b, eu)?;
    }
    for eu in edge::edgeuses(b, e, None, None)? {
        if eu1::uv_trim_curve(b, eu)?.0.is_some() {
            continue;
        }
        let Some(f) = eu1::face(b, eu)? else {
            continue;
        };
        let rec = b.face(f)?;
        let exact = match rec.surface.as_ref() {
            Some(surface) => match plane_map(k, surface, &rec.uv_domain)? {
                Some(map) => planar_trim(curve_of(b, e)?, &map)?.map(|t| (f, t)),
                None => None,
            },
            None => None,
        };
        match exact {
            Some((f, trim)) => {
                let gap = gaps
                    .iter()
                    .find(|(g, _)| *g == f)
                    .map(|(_, d)| *d)
                    .ok_or_else(|| invalid("a trim on a face whose gap was not certified"))?;
                eu1::set_uv_trim_curve(
                    b,
                    eu,
                    eu1::UvTrimCurveArg::New(Box::new(trim)),
                    gap,
                    true,
                    None,
                )?;
            }
            None => {
                eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
            }
        }
    }
    Ok(())
}

/// Squeezes edge `e` into `target` (by default the midpoint of its two vertices) when every
/// point of it is certified within the edge's tolerance of that point ([`short_edge_bound`]):
/// the vertex with the larger tolerance stays and moves there, the other goes, and every edge
/// that met either is re-ended exactly at the point and certified on its faces (module docs).
/// `Ok(Err(Refusal))`, with nothing changed, when a proof is missing. Run inside an edit
/// ([`crate::topology::brep::p3::edit_brep`]).
///
/// # Errors
///
/// Those of the bounds' reads, of [`crate::topology::brep::p3::squeeze_edge`], of the curve
/// replacement and of the trims' remaking.
pub fn collapse_short_edge(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    e: EdgeId,
    target: Option<Point3>,
) -> Result<std::result::Result<Squeezed, Refusal>> {
    let bound = match short_edge_bound(b, e, target)? {
        Ok(found) => found,
        Err(why) => return Ok(Err(why)),
    };
    if !within(bound, edge::tolerance(b, e)?) {
        return Ok(Err(Refusal::TooLong(bound)));
    }
    let Some((v, w)) = ends(b, e)? else {
        return Ok(Err(Refusal::Closed));
    };
    let (pv, pw) = (vertex::point(b, v)?, vertex::point(b, w)?);
    let p = target.unwrap_or_else(|| midpoint(pv, pw));
    let neighbours = match certified_neighbours(k, b, e, v, w, p)? {
        Ok(n) => n,
        Err(why) => return Ok(Err(why)),
    };
    crate::topology::brep::p3::squeeze_edge(k, b, env, e, v)?;
    let vref = (env.key_of)(EntityRef::Vertex(v));
    let uref = |vu: VertexuseId| (env.key_of)(EntityRef::Vertexuse(vu));
    vertex::set_point(
        b,
        v,
        p,
        &mut *env.records,
        &mut *env.attributes,
        env.context.as_deref_mut(),
        vref,
        &uref,
    )?;
    let mut re_ended = Vec::new();
    for (n, c, gaps) in neighbours {
        edge::replace_3d_curve(b, n, Some(c), env)?;
        remake_trims(k, b, n, &gaps)?;
        re_ended.push((n, gaps.iter().map(|(_, g)| *g).fold(0.0, f64::max)));
    }
    Ok(Ok(Squeezed {
        edge: e,
        survivor: v,
        removed: w,
        bound,
        moved: bounds::points(pv, p)?,
        re_ended,
    }))
}

/// [`collapse_short_edge`] on every edge of `b` whose curve is shorter than its tolerance by
/// its vertices' distance or less, pass after pass (a squeeze changes its neighbours) until a
/// pass squeezes nothing: what was squeezed and what was refused (each edge once, the last
/// refusal). Run inside an edit.
///
/// # Errors
///
/// As [`collapse_short_edge`]; `InvalidInput` past [`MAX_EDGES`] edges.
pub fn collapse_short_edges(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    targets: &[(VertexId, Point3)],
    healed: &mut Healed,
) -> Result<bool> {
    let edges = p1::edges(b, None)?;
    if edges.len() > MAX_EDGES {
        return Err(invalid("more edges than a heal pass takes"));
    }
    let mut any = false;
    // After the first pass, only edges at a vertex the previous pass moved or re-ended an edge
    // at are tried again: a refusal reads only the edges at its own two vertices, so any other
    // would be refused the same way.
    let mut dirty: Option<BTreeSet<VertexId>> = None;
    // each pass that continues removes an edge
    for _ in 0..=edges.len() {
        let mut squeezed = false;
        let mut touched: BTreeSet<VertexId> = BTreeSet::new();
        for e in p1::edges(b, None)? {
            k.check_cancelled(OP)?;
            if b.edge(e).is_err() {
                continue;
            }
            // only edges whose vertices are within its tolerance: the candidates
            let Some((v, w)) = ends(b, e)? else {
                continue;
            };
            if dirty
                .as_ref()
                .is_some_and(|d| !d.contains(&v) && !d.contains(&w))
            {
                continue;
            }
            let d = bounds::points(vertex::point(b, v)?, vertex::point(b, w)?)?;
            if !within(d, edge::tolerance(b, e)?) {
                continue;
            }
            let target = targets
                .iter()
                .find(|(x, _)| *x == v || *x == w)
                .map(|(_, p)| *p);
            match collapse_short_edge(k, b, env, e, target)? {
                Ok(s) => {
                    // the edge is gone: none of its refusals stands
                    healed.refused_edges.retain(|(x, _)| *x != e);
                    touched.insert(s.survivor);
                    for (n, _) in &s.re_ended {
                        if let (Some(a), Some(c)) = edge::start_end_vertices(b, *n)? {
                            touched.insert(a);
                            touched.insert(c);
                        }
                    }
                    healed.squeezed.push(s);
                    squeezed = true;
                }
                Err(why) => {
                    // a refusal of this squeeze replaces the edge's last; one of a cut stays
                    healed
                        .refused_edges
                        .retain(|(x, why)| *x != e || why.is_cut());
                    healed.refused_edges.push((e, why));
                }
            }
        }
        if !squeezed {
            break;
        }
        any = true;
        dirty = Some(touched);
    }
    // edges since squeezed are not refused
    let live = p1::edges(b, None)?;
    healed.refused_edges.retain(|(e, _)| live.contains(e));
    Ok(any)
}

/// The two edges of a face bounded by one loop of two uses of two different edges between the
/// same two vertices: the edge [`crate::topology::brep::p3::collapse_zero_area_two_edge_faces`]
/// keeps (the loop's first use's), the other, and the other's other face.
fn two_edge_face(b: &Brep, f: FaceId) -> Result<Option<TwoEdgeFace>> {
    let loops = face2::loops(b, f, false, None)?;
    let [l] = loops[..] else {
        return Ok(None);
    };
    let Some(lu) = b.loop_(l)?.loopuse else {
        return Ok(None);
    };
    let uses = lu1::edgeuses(b, lu, None, None)?;
    let [use1, use2] = uses[..] else {
        return Ok(None);
    };
    let (Some(keep), Some(gone)) = (eu1::edge(b, use1)?, eu1::edge(b, use2)?) else {
        return Ok(None);
    };
    if keep == gone {
        return Ok(None);
    }
    let ends_keep = edge::start_end_vertices(b, keep)?;
    let ends_gone = edge::start_end_vertices(b, gone)?;
    if ends_keep.0 == ends_keep.1 {
        return Ok(None);
    }
    let reversed = if ends_keep == ends_gone {
        false
    } else if ends_keep == (ends_gone.1, ends_gone.0) {
        true
    } else {
        return Ok(None);
    };
    let other = edge::faces(b, gone, None)?.into_iter().find(|&g| g != f);
    Ok(Some(TwoEdgeFace {
        keep,
        gone,
        other,
        reversed,
    }))
}

/// A face bounded by two edges between the same two vertices ([`two_edge_face`]).
struct TwoEdgeFace {
    /// The edge the collapse keeps: the loop's first use's.
    keep: EdgeId,
    /// The edge it removes.
    gone: EdgeId,
    /// The other face `gone` bounds.
    other: Option<FaceId>,
    /// `gone` runs the other way from `keep`: it is reversed before the collapse, which takes
    /// two edges that run the same way.
    reversed: bool,
}

/// Collapses each face of `b` bounded by two coincident edges between the same two vertices
/// (as [`crate::topology::brep::p3::collapse_zero_area_two_edge_faces_among`] does), once the
/// coincidence is certified (module docs); faces it cannot prove are left and reported. Run
/// inside an edit.
///
/// # Errors
///
/// Those of the bounds' reads and of the collapse.
pub fn collapse_coincident_two_edge_faces(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    healed: &mut Healed,
) -> Result<bool> {
    let mut proved = Vec::new();
    // Each proof reads the topology as it is now, so one round collapses only faces whose
    // edges and faces no earlier proved face touches (two slivers sharing an edge would
    // otherwise pair edges no proof covered); the rest wait for the next round, proved again.
    let mut used_edges: BTreeSet<EdgeId> = BTreeSet::new();
    let mut used_faces: BTreeSet<FaceId> = BTreeSet::new();
    for f in p1::faces(b, None)? {
        k.check_cancelled(OP)?;
        let Some(TwoEdgeFace {
            keep,
            gone,
            other,
            reversed,
        }) = two_edge_face(b, f)?
        else {
            continue;
        };
        if used_faces.contains(&f)
            || other.is_some_and(|g| used_faces.contains(&g))
            || used_edges.contains(&keep)
            || used_edges.contains(&gone)
        {
            continue;
        }
        let tol = edge::tolerance(b, keep)?;
        // a NaN tolerance on either edge refuses too
        if !within(edge::tolerance(b, gone)?, tol) {
            healed.refused_faces.push((f, Refusal::WouldGrow));
            continue;
        }
        let (ik, ig) = (edge::interval(b, keep)?, edge::interval(b, gone)?);
        let d = bounds::curves(
            curve_of(b, keep)?,
            [ik.min(), ik.max()],
            curve_of(b, gone)?,
            if reversed {
                [ig.max(), ig.min()]
            } else {
                [ig.min(), ig.max()]
            },
        );
        let d = match d {
            Ok(d) if d <= tol => d,
            Ok(d) => {
                healed.refused_faces.push((f, Refusal::NotCoincident(d)));
                continue;
            }
            Err(_) => {
                healed
                    .refused_faces
                    .push((f, Refusal::NotCoincident(f64::INFINITY)));
                continue;
            }
        };
        // the kept edge's curve on its own faces and on the face across the other edge
        let c = curve_of(b, keep)?;
        let mut gaps = Vec::new();
        let mut off = None;
        for g in edge::faces(b, keep, None)?.into_iter().chain(other) {
            if g == f {
                continue;
            }
            let gap = match b_spline_of(c) {
                Some(s) => curve_face_gap_bound(k, b, g, s, [ik.min(), ik.max()], tol / 100.0)?,
                None => None,
            };
            match gap {
                Some(x) if x <= GAP_SHARE * tol => gaps.push((g, x)),
                other => {
                    off = Some((g, other));
                    break;
                }
            }
        }
        if let Some((g, gap)) = off {
            healed.refused_faces.push((f, Refusal::KeptOffFace(g, gap)));
            continue;
        }
        // a reversed other edge gets exact trims again before the collapse: its gaps
        let gone_gaps = if reversed {
            match face_gaps(k, b, gone, curve_of(b, gone)?)? {
                Ok(g) => Some((gone, g)),
                Err((g, gap)) => {
                    healed.refused_faces.push((f, Refusal::KeptOffFace(g, gap)));
                    continue;
                }
            }
        } else {
            None
        };
        used_edges.insert(keep);
        used_edges.insert(gone);
        used_faces.insert(f);
        used_faces.extend(other);
        proved.push((f, d, keep, gone_gaps, gaps));
    }
    if proved.is_empty() {
        return Ok(false);
    }
    // a reversed edge runs the same way as the one kept, exactly (its curve's parameter
    // reversed), its trims made again (the collapse's zero-area test reads them)
    for (_, _, _, gone, _) in &proved {
        if let Some((gone, gone_gaps)) = gone {
            edge::reverse_orientation(k, b, *gone)?;
            remake_trims(k, b, *gone, gone_gaps)?;
        }
    }
    let faces: Vec<FaceId> = proved.iter().map(|(f, ..)| *f).collect();
    crate::topology::brep::p3::collapse_zero_area_two_edge_faces_among(k, b, env, &faces)?;
    let live = p1::faces(b, None)?;
    let mut any = false;
    for (f, d, keep, gone, gaps) in proved {
        if live.contains(&f) {
            // the face stays: its edge reversed above runs its own way again, exactly (its
            // trims made again from its curve)
            if let Some((gone, gone_gaps)) = gone {
                if b.edge(gone).is_ok() {
                    edge::reverse_orientation(k, b, gone)?;
                    remake_trims(k, b, gone, &gone_gaps)?;
                }
            }
            healed.refused_faces.push((f, Refusal::NotZeroArea));
        } else {
            // the uses the kept edge took over carried the other edge's trims: all made
            // again from the kept edge's curve
            remake_trims(k, b, keep, &gaps)?;
            healed.collapsed.push((f, d));
            any = true;
        }
    }
    Ok(any)
}

/// How close the control points of a piece are for the check to call them one point
/// (`SmBSplineCurve` test 15's pole tolerance).
const PIECE_POLE_TOL: f64 = ZONE_TOL_3D / 10.0;

/// The multiplicity of the knot at `index` in `u`, and the index of its last copy: the
/// number of knots equal to it, and the index of the first of them plus that number less one.
#[cfg(test)]
fn multiplicity(u: &[f64], index: usize) -> (usize, usize) {
    let Some(&v) = u.get(index) else {
        return (0, index);
    };
    let first = u.iter().position(|&x| x == v).unwrap_or(index);
    let count = u.iter().filter(|&&x| x == v).count();
    (count, first + count.saturating_sub(1))
}

/// [`multiplicity`] of every knot of `u`, in one sort rather than a scan of `u` per knot: the
/// knots grouped by value (`==`: `-0.0` and `0.0` one value, a NaN equal to none, so its count
/// is 0 and its last copy its own index), each one's count and the first index of its group
/// plus that count less one. Read with [`multiplicity_in`].
fn multiplicities(u: &[f64]) -> Vec<(usize, usize)> {
    let mut sorted: Vec<(f64, usize)> = u.iter().copied().zip(0..).collect();
    // `total_cmp` puts the values `==` takes as one next to each other (-0.0 just before 0.0)
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<(usize, usize)> = (0..u.len()).map(|i| (0, i)).collect();
    let mut start = 0;
    while let Some(&(v, _)) = sorted.get(start) {
        let len = sorted
            .get(start..)
            .map_or(0, |rest| rest.iter().take_while(|(x, _)| *x == v).count());
        let group = sorted.get(start..start + len).unwrap_or(&[]);
        let first = group.iter().map(|&(_, i)| i).min().unwrap_or(0);
        for &(_, i) in group {
            if let Some(slot) = out.get_mut(i) {
                *slot = (len, first + len.saturating_sub(1));
            }
        }
        start += len.max(1);
    }
    out
}

/// [`multiplicity`] of the knot at `index`, read from `table` ([`multiplicities`]).
fn multiplicity_in(table: &[(usize, usize)], index: usize) -> (usize, usize) {
    table.get(index).copied().unwrap_or((0, index))
}

/// The curve `nurb` (a clamped B-spline of degree `p`, one positive weight for every control
/// point) with one interior Bézier piece of no length cut out, and the certified bound of how
/// far the cut moves it: the piece lies in the convex hull of its control points, and the next
/// piece's first control point moves from the piece's last to its first, so no point moves
/// farther than the largest distance of the piece's control points from its first. It is the
/// first such piece whose control points are within [`PIECE_POLE_TOL`] of its first, both of its knots interior and of multiplicity `p` (a C0 joint at each end). The
/// piece's first control point stays, so the next piece starts there; the knots after it move
/// down by its length, so the parameter stays continuous. `None` when there is no such piece.
fn cut_one_piece(nurb: &Nurbs, rational: bool) -> Result<Option<(Curve, f64)>> {
    let (p, u, pw) = (nurb.degree(), nurb.knots(), nurb.cpts());
    // one weight for every control point (a polynomial curve stored rational): the curve is
    // the Euclidean points' polynomial combination, which the bound below reads
    let w0 = pw
        .first()
        .map_or(1.0, |q| if q.w == NOW { 1.0 } else { q.w });
    if p == 0 || !positive(w0) || (rational && pw.iter().any(|q| q.w != w0)) {
        return Ok(None);
    }
    let point = |q: &CPoint| {
        if q.w == NOW {
            Point3::new(q.x, q.y, q.z)
        } else {
            Point3::new(q.x / q.w, q.y / q.w, q.z / q.w)
        }
    };
    let end = u.len().saturating_sub(p + 1);
    let table = multiplicities(u);
    // knot runs: a piece runs from the last copy of one knot value to the next value
    let mut i = p;
    while i < end {
        let (_, last_a) = multiplicity_in(&table, i);
        let Some(next) = last_a.checked_add(1) else {
            break;
        };
        if next > end {
            break;
        }
        let (mult_b, last_b) = multiplicity_in(&table, next);
        let (mult_a, _) = multiplicity_in(&table, i);
        let interior = i > p && last_b < end;
        // a Bézier piece between two C0 joints: CPs last_a - p ..= last_a
        if interior && mult_a == p && mult_b == p {
            let first = last_a - p;
            let piece = pw.get(first..=last_a).unwrap_or(&[]);
            let q0 = piece.first().map(point);
            let mut tiny = q0.is_some();
            if let Some(q0) = q0 {
                for q in piece {
                    tiny &= bounds::points(q0, point(q))? <= PIECE_POLE_TOL;
                }
            }
            if let (true, Some(q0)) = (tiny, q0) {
                let (Some(&a), Some(&b_)) = (u.get(last_a), u.get(next)) else {
                    break;
                };
                // with one positive weight the piece lies in the convex hull of its control
                // points, all within the largest of their distances from the first
                let mut bound = 0.0_f64;
                let mut size = 0.0_f64;
                for q in piece {
                    bound = bound.max(bounds::points(q0, point(q))?);
                    let e = point(q);
                    size = size.max(e.x.abs() + e.y.abs() + e.z.abs());
                }
                // a rational point is its homogeneous one divided by its weight, each
                // coordinate rounded once (at most half an ulp of it): two such points are
                // farther apart than their rounded copies by at most 2 eps of their sizes,
                // which outward-rounding 4 eps covers
                if rational {
                    bound += 4.0 * f64::EPSILON * size;
                }
                let len = b_ - a;
                let mut cpts: Vec<CPoint> = Vec::new();
                for (j, q) in pw.iter().enumerate() {
                    if j <= first || j > last_a {
                        cpts.push(*q);
                    }
                }
                let mut knots: Vec<f64> = Vec::new();
                for (j, &t) in u.iter().enumerate() {
                    if j >= next && j <= last_b {
                        continue;
                    }
                    knots.push(if j > last_b { t - len } else { t });
                }
                let cut = Nurbs::new(cpts, p, knots)?;
                let curve = Curve::from(BSplineCurve::from_nurb(cut, 3, false));
                return Ok(Some((curve, bound)));
            }
        }
        i = next;
    }
    Ok(None)
}

/// Cuts the pieces of no length out of each edge's B-spline curve ([`cut_one_piece`]) when
/// the certified bound of what the cut moves is within the edge's tolerance, then makes the
/// edge's trims again from the new curve. Run inside an edit.
///
/// # Errors
///
/// Those of the bounds, the curve replacement and the trims' remaking.
pub fn cut_degenerate_pieces(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    healed: &mut Healed,
) -> Result<bool> {
    let mut any = false;
    for e in p1::edges(b, None)? {
        k.check_cancelled(OP)?;
        let tol = edge::tolerance(b, e)?;
        // each cut removes a piece: at most as many cuts as the curve has knots
        let knots = match curve_of(b, e)?.kind() {
            CurveKind::BSpline(s) => s.nurb_ref()?.knots().len(),
            _ => continue,
        };
        // how far the cuts so far moved the curve, from the curve as it was: each cut's bound is
        // from the curve the cut before left, so the bounds add up (the triangle inequality),
        // rounded up so the sum is never below the exact one
        let mut moved = 0.0_f64;
        let mut cut_any = false;
        let mut gaps = Vec::new();
        for _ in 0..knots {
            k.check_cancelled(OP)?;
            let Some(t) = edge::create_trimmed_nurbs_curve(k, b, e)? else {
                break;
            };
            let CurveKind::BSpline(s) = t.kind() else {
                break;
            };
            let Some((c, bound)) = cut_one_piece(s.nurb_ref()?, s.is_rational()?)? else {
                break;
            };
            let total = crate::geom::certified::next_up(moved + bound);
            if !within(total, tol) {
                healed.refused_edges.push((e, Refusal::PieceTooLong(total)));
                break;
            }
            let cut_gaps = match face_gaps(k, b, e, &c)? {
                Ok(g) => g,
                Err((f, gap)) => {
                    healed.refused_edges.push((e, Refusal::CutOffFace(f, gap)));
                    break;
                }
            };
            // the trimmed form may snap its interval to a knot: both ends must still meet
            // their vertices within the vertices' tolerances
            let (s, t) = edge::start_end_vertices(b, e)?;
            let range = c.natural_interval();
            let mut off = None;
            for (x, at) in [(s, range.min()), (t, range.max())] {
                if let Err(d) = end_on_vertex(b, x, &c, at)? {
                    off = Some(d);
                    break;
                }
            }
            if let Some(d) = off {
                healed.refused_edges.push((e, Refusal::CutEnd(d)));
                break;
            }
            edge::replace_3d_curve(b, e, Some(c), env)?;
            // the trims are made from the curve installed, at its gaps
            gaps = cut_gaps;
            moved = total;
            cut_any = true;
        }
        if cut_any {
            remake_trims(k, b, e, &gaps)?;
            healed.cut_pieces.push((e, moved));
            any = true;
        }
    }
    Ok(any)
}

/// Both heals, in turn, until neither changes anything: short edges squeezed, then the
/// two-edge faces that leaves collapsed (each collapse can leave another edge short). Run
/// inside an edit.
///
/// # Errors
///
/// As [`collapse_short_edges`] and [`collapse_coincident_two_edge_faces`].
pub fn heal_short_edges_and_slivers(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
) -> Result<Healed> {
    let mut healed = Healed::default();
    cut_degenerate_pieces(k, b, env, &mut healed)?;
    let targets = squeeze_targets(k, b)?;
    let rounds = p1::faces(b, None)?.len().saturating_add(1);
    for _ in 0..rounds {
        let squeezed = collapse_short_edges(k, b, env, &targets, &mut healed)?;
        healed.refused_faces.clear();
        let collapsed = collapse_coincident_two_edge_faces(k, b, env, &mut healed)?;
        if !squeezed && !collapsed {
            break;
        }
    }
    Ok(healed)
}

#[cfg(test)]
mod tests {
    use super::{multiplicities, multiplicity, multiplicity_in, root};

    /// The table gives every knot's multiplicity and last copy as the scan of the whole vector
    /// does: clamped and repeated knots, signed zeros, a NaN, and an unsorted vector.
    #[test]
    fn the_multiplicity_table_reads_as_the_scan() {
        let vectors: [&[f64]; 6] = [
            &[],
            &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 3.0, 3.0, 3.0, 3.0],
            &[-0.0, 0.0, 0.0, 0.5, 1.0, 1.0, -0.0],
            &[0.0, f64::NAN, 1.0, f64::NAN, 1.0],
            &[2.0, 1.0, 2.0, 0.0, 1.0, 2.0],
            &[5.0],
        ];
        for u in vectors {
            let table = multiplicities(u);
            for i in 0..u.len() + 2 {
                assert_eq!(
                    multiplicity_in(&table, i),
                    multiplicity(u, i),
                    "{u:?} at {i}"
                );
            }
        }
    }

    /// A cluster joined as a chain (each vertex's parent the one before) is walked once: after
    /// one root lookup every vertex on the walk points at the root, which is unchanged.
    #[test]
    fn a_root_lookup_compresses_the_chain_it_walks() {
        let n: usize = 1000;
        let mut parent: Vec<usize> = (0..n).map(|i| i.saturating_sub(1)).collect();
        assert_eq!(root(&mut parent, n - 1), 0);
        assert!(
            parent.iter().all(|&q| q == 0),
            "every vertex walked points at the root"
        );
        // a lookup off the forest is its own root, and changes nothing
        assert_eq!(root(&mut parent, n + 5), n + 5);
        let mut forest = vec![0, 0, 2, 2, 3];
        assert_eq!(root(&mut forest, 4), 2);
        assert_eq!(forest, vec![0, 0, 2, 2, 2]);
    }
}
