//! Not upstream: whether two bodies meet (share material or touch within a tolerance),
//! decided only where it is proved: [`meets`] and its distance, [`boundary_distance`].
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`meets`], [`boundary_distance`], [`Prepared`] | none (`SmApiBrepRelationship`, ported as [`crate::query::api_queries::brep_relationship`], is a Newton solve: its gap is a point pair, never a lower bound, and its containment counts samples) |
//!
//! **The bracket.** The distance between the two bodies' boundaries is enclosed in `[lower,
//! upper]`:
//!
//! - `lower`: the least, over pairs of pieces of the two bodies' faces, of the distance
//!   between their hulls' boxes. The pieces are those of
//!   [`crate::query::distance_bound`] (each face's NURBS form over its UV box, its Bézier
//!   patches about one common point, every coefficient's error carried and each box widened
//!   by it), so no point of either boundary is nearer the other than `lower`. Pairs wait on a
//!   heap, nearest first; the nearest is split (its larger piece, in four), so `lower`
//!   rises. A piece whose UV rectangle is proved off its face (no trim curve's box meets it,
//!   and its centre classifies outside the face) is dropped: a face's hole is no material.
//! - `upper` and its witness: a pair of points, one on each body, each on its face's surface
//!   at parameters the face classifies decisively inside (`face_classify_uv`, the face
//!   itself: neither on its boundary nor undecided), their distance rounded up. The
//!   candidates are the centres of the pairs taken from the heap, each moved to the other
//!   surface's closest point and back ([`crate::query::api_queries::surface_closest_point`],
//!   a few times), so two surfaces that cross reach a distance of their rounding.
//!
//! **Meets.** At a tolerance `tol`: `Yes` when `upper <= tol` (with the witness); when
//! `lower > tol`, the boundaries do not touch, and the bodies meet only if one encloses the
//! other: a point of each boundary is classified in the other body, `Yes` when one is
//! inside its material, `No` when both are outside. A classification on the other's boundary
//! or not decided is no answer, and neither is a bracket that still straddles `tol` when
//! the pairs reach [`MAX_PAIRS`]: [`Verdict::Undecided`], with the bracket, which names
//! the size that would decide.
//!
//! Deviations: none (no upstream counterpart). No recursion: the pairs wait on a heap.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::geom::certified::{gamma, Verdict};
use crate::geom::extent1d::Extent1d;
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::geom::vector2d::Point2;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::mass::integrator::properties::region_material;
use crate::math::Point3;
use crate::query::api_queries::{
    edge_closest_point, face_bounding_box, face_classify_uv, surface_closest_point,
};
use crate::query::brep::BrepClassifier;
use crate::query::distance_bound::{pieces_about, Piece};
use crate::surface::surface::Surface;
use crate::topology::{edgeuse::p1 as eu1, loopuse, Brep, FaceId};
use crate::{EntityRef, Result, SolidError};

const OP: &str = "query::contact::meets";

/// The most piece pairs made for one answer.
pub const MAX_PAIRS: usize = 1 << 16;

/// The times a witness is moved to the other surface's closest point and back.
const PROJECTIONS: usize = 6;

/// How two bodies stand: the verdict at the tolerance asked, and the boundary distance's
/// bracket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// `Yes` (they share material or their boundaries come within the tolerance), `No`, or
    /// `Undecided` (the bracket straddles the tolerance at the work's limit, or a
    /// classification was not decided).
    pub verdict: Verdict,
    /// A certified lower bound of the distance between the boundaries.
    pub lower: f64,
    /// An upper bound (the witness's distance, rounded up); infinite without a witness.
    pub upper: f64,
    /// Two points, one on each boundary, `upper` apart.
    pub witness: Option<[Point3; 2]>,
}

/// A face's pieces and what decides whether a piece is on it.
struct FacePieces {
    face: FaceId,
    surface: Surface,
    /// Its form's certified distance from its definition, lost from every bound.
    slack: f64,
    pieces: Vec<Piece>,
    /// The boxes of its own UV trim curves `[u0, u1, v0, v1]`; `None` when a trim curve or
    /// its box is missing (then no piece is dropped).
    trims: Option<Vec<[f64; 4]>>,
}

/// A body made ready for contact tests: its faces' pieces about a common point.
pub struct Prepared {
    origin: Point3,
    faces: Vec<FacePieces>,
}

/// A pair of pieces, one of each body, with its lower bound.
struct Pair {
    lower: f64,
    a: (usize, Piece),
    b: (usize, Piece),
}

impl PartialEq for Pair {
    fn eq(&self, o: &Self) -> bool {
        self.lower.total_cmp(&o.lower) == Ordering::Equal
    }
}
impl Eq for Pair {}
impl PartialOrd for Pair {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Pair {
    // the heap pops the smallest bound first
    fn cmp(&self, o: &Self) -> Ordering {
        o.lower.total_cmp(&self.lower)
    }
}

/// The box of body `b`'s faces, `(lo, hi)`.
fn body_box(k: &Kernel, b: &Brep) -> Result<(Point3, Point3)> {
    let mut lo = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &f in b.face_list() {
        let e = face_bounding_box(k, b, f, false)?;
        let (l, h) = (e.min(), e.max());
        lo = Point3::new(lo.x.min(l.x), lo.y.min(l.y), lo.z.min(l.z));
        hi = Point3::new(hi.x.max(h.x), hi.y.max(h.y), hi.z.max(h.z));
    }
    if !(lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z) {
        return Err(SolidError::invalid_input(OP, "a body without faces"));
    }
    Ok((lo, hi))
}

/// The common point two bodies' pieces are made about: the centre of their boxes.
fn common_origin(k: &Kernel, a: &Brep, b: &Brep) -> Result<Point3> {
    let ((al, ah), (bl, bh)) = (body_box(k, a)?, body_box(k, b)?);
    let mid = |x: f64, y: f64, z: f64, w: f64| 0.5 * (x.min(z) + y.max(w));
    Ok(Point3::new(
        mid(al.x, ah.x, bl.x, bh.x),
        mid(al.y, ah.y, bl.y, bh.y),
        mid(al.z, ah.z, bl.z, bh.z),
    ))
}

/// The parts each Bézier segment of a trim curve is cut into for its boxes: a rectangle
/// that no part's box meets is proved clear of the curve.
const TRIM_PARTS: usize = 16;

/// The boxes, `[u0, u1, v0, v1]`, of a UV trim curve's NURBS form: each Bézier segment cut in
/// [`TRIM_PARTS`] by de Casteljau on its homogeneous points, each part in the box of its
/// Euclidean points (its hull: positive weights), widened by the subdivision's rounding;
/// `None` for a form whose weights are not positive.
fn nurb_boxes(c: &crate::nurbs::crv_basic::Curve) -> Result<Option<Vec<[f64; 4]>>> {
    use crate::nurbs::math::NOW;
    let mut out = Vec::new();
    for seg in c.decompose_bez()? {
        let pts: Vec<[f64; 3]> = seg
            .cpts()
            .iter()
            .map(|q| {
                let w = if q.w == NOW { 1.0 } else { q.w };
                [q.x, q.y, w]
            })
            .collect();
        if pts
            .iter()
            .any(|q| !(q[2] > 0.0 && q.iter().all(|v| v.is_finite())))
        {
            return Ok(None);
        }
        let size = pts.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
        let wmin = pts.iter().fold(f64::INFINITY, |m, q| m.min(q[2]));
        // the right part from t: de Casteljau at t on the homogeneous points
        let cut = |p: &[[f64; 3]], t: f64| -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
            let mut level = p.to_vec();
            let (mut left, mut right) = (Vec::new(), Vec::new());
            while let (Some(&f), Some(&l)) = (level.first(), level.last()) {
                left.push(f);
                right.push(l);
                level = level
                    .windows(2)
                    .filter_map(|w| match w {
                        [a, b] => Some([
                            (1.0 - t) * a[0] + t * b[0],
                            (1.0 - t) * a[1] + t * b[1],
                            (1.0 - t) * a[2] + t * b[2],
                        ]),
                        _ => None,
                    })
                    .collect();
            }
            right.reverse();
            (left, right)
        };
        let degree = pts.len().saturating_sub(1);
        // TRIM_PARTS levels of convex combinations at most: their rounding, of the size
        let err = gamma(3 * degree * TRIM_PARTS + 3) * size;
        let mut rest = pts;
        for i in 0..TRIM_PARTS {
            let t = 1.0 / (TRIM_PARTS - i) as f64;
            let (part, next) = if i + 1 == TRIM_PARTS {
                (rest.clone(), Vec::new())
            } else {
                cut(&rest, t)
            };
            rest = next;
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for q in &part {
                for (k, v) in [q[0] / q[2], q[1] / q[2]].into_iter().enumerate() {
                    if let (Some(l), Some(h)) = (lo.get_mut(k), hi.get_mut(k)) {
                        *l = l.min(v);
                        *h = h.max(v);
                    }
                }
            }
            // a quotient of coefficients within err: within err (1 + |x|) / (w - err)
            let widen = err * (1.0 + size / wmin) / (wmin - err).max(f64::MIN_POSITIVE)
                + gamma(2) * size / wmin;
            out.push([lo[0] - widen, hi[0] + widen, lo[1] - widen, hi[1] + widen]);
        }
    }
    Ok(Some(out))
}

/// The boxes of face `f`'s own UV trim curves (each its NURBS form's parts,
/// [`nurb_boxes`], or the curve's whole box where it has none); `None` where a trim curve or
/// its box is missing.
fn trim_boxes(k: &Kernel, b: &Brep, f: FaceId) -> Result<Option<Vec<[f64; 4]>>> {
    use crate::curve::curve::CurveKind;
    let Some(fu) = b.face(f)?.faceuse else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
        for eu in loopuse::edgeuses(b, lu, None, None)? {
            // its own, or its mate's on the face's other side (the same face's parameters)
            let Some(c) = eu1::uv_trim_curve(b, eu)?.0 else {
                return Ok(None);
            };
            let nurb = match c.kind() {
                CurveKind::BSpline(x) => x.gw_nurb(),
                CurveKind::Ellipse(x) => x.base().gw_nurb(),
                CurveKind::Hyperbola(x) => x.base().gw_nurb(),
                CurveKind::Parabola(x) => x.base().gw_nurb(),
                CurveKind::Line(x) => x.base().gw_nurb(),
                _ => None,
            };
            if let Some(n) = nurb {
                if let Some(boxes) = nurb_boxes(n)? {
                    out.extend(boxes);
                    continue;
                }
            }
            let interval: Extent1d = c.natural_interval();
            let boxes = c.calculate_bounding_box(
                k,
                &interval,
                crate::nurbs::b_spline_curve::BoundingBoxRequest {
                    normal: true,
                    ..Default::default()
                },
                true,
            )?;
            let Some(e) = boxes.normal else {
                return Ok(None);
            };
            let (l, h) = (e.min(), e.max());
            // the box's own rounding, a few ulps of its size
            let r = 16.0 * f64::EPSILON * (l.x.abs() + h.x.abs() + l.y.abs() + h.y.abs());
            out.push([l.x - r, h.x + r, l.y - r, h.y + r]);
        }
    }
    Ok(Some(out))
}

impl Prepared {
    /// Body `b`'s faces' pieces about `origin`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a body without faces or a face without a surface; those of the
    /// pieces ([`crate::query::distance_bound`]) and the trim curves' boxes.
    pub fn new(k: &Kernel, b: &Brep, origin: Point3) -> Result<Self> {
        let mut faces = Vec::new();
        for &f in b.face_list() {
            let face = b.face(f)?;
            let surface = face
                .surface
                .as_ref()
                .ok_or_else(|| SolidError::invalid_input(OP, "a face without a surface"))?
                .clone();
            let (pieces, slack) = pieces_about(k, &surface, Some(&face.uv_domain), origin)?;
            let trims = trim_boxes(k, b, f)?;
            faces.push(FacePieces {
                face: f,
                surface,
                slack,
                pieces,
                trims,
            });
        }
        if faces.is_empty() {
            return Err(SolidError::invalid_input(OP, "a body without faces"));
        }
        Ok(Self { origin, faces })
    }

    /// Body `b`'s faces' pieces about the centre of its own box: the preparation of one
    /// body for tests against several ([`meets_prepared`]).
    ///
    /// # Errors
    ///
    /// Those of [`Prepared::new`] and of the faces' boxes.
    pub fn about(k: &Kernel, b: &Brep) -> Result<Self> {
        Self::new(k, b, common_origin(k, b, b)?)
    }
}

/// The certified lower bound of the distance between two pieces about the same point: the
/// larger of the distance of their widened boxes and, along a unit direction `axis` (the
/// witness's, when there is one), the gap between their hulls' extents
/// ([`Piece::extent_along`]), each rounded down. The axis steers which bound is tight, never
/// whether either holds: any direction separates the hulls by at most their distance.
fn pair_lower(a: &Piece, b: &Piece, axis: Option<[f64; 3]>) -> Result<f64> {
    let by_box = box_lower(a, b)?;
    let Some(u) = axis else {
        return Ok(by_box);
    };
    let len = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt() * (1.0 + gamma(4));
    if !(len > 0.0 && len.is_finite()) {
        return Ok(by_box);
    }
    let ((a0, a1), (b0, b1)) = (a.extent_along(u)?, b.extent_along(u)?);
    let gap = (b0 - a1).max(a0 - b1).max(0.0);
    // (the difference rounded down by gamma_1 of its terms, then over the length)
    let gap = (gap - gamma(1) * (a0.abs() + a1.abs() + b0.abs() + b1.abs())).max(0.0);
    let by_axis = gap / len * (1.0 - gamma(2));
    Ok(by_box.max(by_axis))
}

/// The distance of two pieces' widened boxes, rounded down.
fn box_lower(a: &Piece, b: &Piece) -> Result<f64> {
    let ((al, ah), (bl, bh)) = (a.widened_box()?, b.widened_box()?);
    let mut sum = 0.0_f64;
    for i in 0..3 {
        let (Some(&x0), Some(&x1), Some(&y0), Some(&y1)) =
            (al.get(i), ah.get(i), bl.get(i), bh.get(i))
        else {
            return Err(SolidError::sm_err(OP));
        };
        let gap = (y0 - x1).max(x0 - y1).max(0.0);
        // (the difference rounded up by at most gamma_1 of its terms)
        let gap = (gap - gamma(1) * (x0.abs() + x1.abs() + y0.abs() + y1.abs())).max(0.0);
        sum += gap * gap;
    }
    Ok(sum.sqrt() * (1.0 - gamma(5)))
}

/// Where a piece stands against its face: `Some(true)` proved on it and decisively inside
/// at its centre, `Some(false)` proved off it (no trim curve's box meets its rectangle and
/// its centre is outside), `None` not decided.
fn piece_on_face(k: &Kernel, b: &mut Brep, fp: &FacePieces, p: &Piece) -> Result<Option<bool>> {
    let [u0, u1, v0, v1] = p.uv;
    let crosses = match &fp.trims {
        None => true,
        Some(boxes) => boxes
            .iter()
            .any(|t| !(t[1] < u0 || t[0] > u1 || t[3] < v0 || t[2] > v1)),
    };
    let centre = Point2::new(0.5 * (u0 + u1), 0.5 * (v0 + v1));
    let class = match face_classify_uv(k, b, fp.face, centre) {
        Ok(c) => c,
        // a classification that fails decides nothing
        Err(e) if e.is_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    Ok(match class {
        PointClassType::Unknown if !crosses => Some(false),
        PointClassType::Face => Some(true),
        _ => None,
    })
}

/// A point of body `b` for face `fp` at `uv`: the surface's point when the face classifies
/// `uv` decisively inside it; when it classifies it on the face's boundary (within its
/// tolerance, either side), the nearest point of the face's edges to the surface's point,
/// which is on the body whichever side `uv` was; `None` outside or not decided.
fn on_face(k: &Kernel, b: &mut Brep, fp: &FacePieces, uv: Point2) -> Result<Option<Point3>> {
    match face_classify_uv(k, b, fp.face, uv) {
        Ok(PointClassType::Face) => Ok(Some(fp.surface.evaluate_point(k, uv)?)),
        Ok(PointClassType::Edge | PointClassType::Vertex) => {
            let x = fp.surface.evaluate_point(k, uv)?;
            nearest_edge_point(k, b, fp.face, x)
        }
        Err(e) if e.is_cancelled() => Err(e),
        _ => Ok(None),
    }
}

/// The nearest point to `x` of face `f`'s edges (each solved on its edge, so on the body);
/// `None` when no edge's solve succeeds.
fn nearest_edge_point(k: &Kernel, b: &Brep, f: FaceId, x: Point3) -> Result<Option<Point3>> {
    let Some(fu) = b.face(f)?.faceuse else {
        return Ok(None);
    };
    let mut best: Option<(f64, Point3)> = None;
    for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
        for eu in loopuse::edgeuses(b, lu, None, None)? {
            let Some(e) = b.edgeuse(eu)?.edge else {
                continue;
            };
            let Ok(c) = edge_closest_point(k, b, e, x) else {
                continue;
            };
            let d = c.closest.point.distance(x);
            if best.is_none_or(|(m, _)| d < m) {
                best = Some((d, c.closest.point));
            }
        }
    }
    Ok(best.map(|(_, p)| p))
}

/// The distance of two points rounded up: `|p - q|` within `gamma_5` of the terms' size.
fn distance_up(p: Point3, q: Point3) -> f64 {
    let size = p.x.abs() + p.y.abs() + p.z.abs() + q.x.abs() + q.y.abs() + q.z.abs();
    p.distance(q) * (1.0 + gamma(5)) + gamma(5) * size
}

/// The closest point of `fp`'s surface to `x`, kept when its face classifies it decisively
/// inside; `None` for a solve that fails or a point off the face.
fn closest_on(
    k: &Kernel,
    b: &mut Brep,
    fp: &FacePieces,
    x: Point3,
) -> Result<Option<(Point3, Point2)>> {
    let Ok(c) = surface_closest_point(k, &fp.surface, x) else {
        return Ok(None);
    };
    let Some(&uv) = c.uvs.first() else {
        return Ok(None);
    };
    Ok(on_face(k, b, fp, uv)?.map(|p| (p, uv)))
}

/// The bracket between `a` and `b` (as [`Prepared`]), refined until `stop` says done or the
/// pairs reach [`MAX_PAIRS`]: `stop(lower, upper)`.
fn bracket(
    k: &Kernel,
    a: &mut Brep,
    pa: &Prepared,
    b: &mut Brep,
    pb: &Prepared,
    seed: f64,
    stop: &dyn Fn(f64, f64) -> bool,
) -> Result<(f64, f64, Option<[Point3; 2]>, bool)> {
    let mut heap = BinaryHeap::new();
    let mut made = 0_usize;
    for (ia, fa) in pa.faces.iter().enumerate() {
        for (ib, fb) in pb.faces.iter().enumerate() {
            for x in &fa.pieces {
                for y in &fb.pieces {
                    let lower = (pair_lower(x, y, None)? - fa.slack - fb.slack).max(0.0);
                    heap.push(Pair {
                        lower,
                        a: (ia, x.clone()),
                        b: (ib, y.clone()),
                    });
                    made += 1;
                }
            }
        }
    }
    let (mut upper, mut witness) = (f64::INFINITY, None);
    // witnesses first from every face pair whose bound is within `seed`, nearest first (the
    // pairs tie closely where two bodies lie along each other; each is visited once here)
    let mut first: Vec<&Pair> = heap.iter().filter(|p| p.lower <= seed).collect();
    first.sort_by(|x, y| x.lower.total_cmp(&y.lower));
    for pair in first {
        let (Some(fa), Some(fb)) = (pa.faces.get(pair.a.0), pb.faces.get(pair.b.0)) else {
            return Err(SolidError::sm_err(OP));
        };
        if pair.lower < upper {
            witness_from(
                k,
                (&mut *a, fa, &pair.a.1),
                (&mut *b, fb, &pair.b.1),
                &mut upper,
                &mut witness,
            )?;
        }
        if stop(0.0, upper) {
            break;
        }
    }
    loop {
        k.check_cancelled(OP)?;
        let Some(pair) = heap.pop() else {
            // every piece proved off its face: no boundary is there to bound
            return Ok((f64::INFINITY, upper, witness, true));
        };
        let lower = pair.lower.min(upper);
        if stop(lower, upper) {
            return Ok((lower, upper, witness, true));
        }
        if made >= MAX_PAIRS {
            return Ok((lower, upper, witness, false));
        }
        let (ia, x) = &pair.a;
        let (ib, y) = &pair.b;
        let (Some(fa), Some(fb)) = (pa.faces.get(*ia), pb.faces.get(*ib)) else {
            return Err(SolidError::sm_err(OP));
        };
        // a witness from the pair's centres (none can be nearer than the pair's bound, so a
        // pair past the witness so far is skipped)
        if pair.lower < upper {
            witness_from(
                k,
                (&mut *a, fa, x),
                (&mut *b, fb, y),
                &mut upper,
                &mut witness,
            )?;
        }
        // split the larger piece
        let size = |p: &Piece| -> Result<f64> {
            let (l, h) = p.widened_box()?;
            Ok((0..3)
                .map(|i| h.get(i).copied().unwrap_or(0.0) - l.get(i).copied().unwrap_or(0.0))
                .fold(0.0_f64, f64::max))
        };
        let split_a = size(x)? >= size(y)?;
        let (from, keep) = if split_a { (x, y) } else { (y, x) };
        // the witness's direction, for the hulls' extents along it
        let axis = witness.map(|[p, q]| [q.x - p.x, q.y - p.y, q.z - p.z]);
        for q in from.quarters()? {
            let (fp, brep) = if split_a {
                (fa, &mut *a)
            } else {
                (fb, &mut *b)
            };
            if piece_on_face(k, brep, fp, &q)? == Some(false) {
                continue;
            }
            let (qa, qb) = if split_a { (&q, keep) } else { (keep, &q) };
            let lower = (pair_lower(qa, qb, axis)? - fa.slack - fb.slack).max(0.0);
            heap.push(Pair {
                lower,
                a: (*ia, qa.clone()),
                b: (*ib, qb.clone()),
            });
            made += 1;
        }
    }
}

/// A witness from a pair's centres, each moved to the other's surface's closest point and
/// back ([`PROJECTIONS`] times), kept where both points are decisively on their faces and
/// nearer than `upper`.
fn witness_from(
    k: &Kernel,
    (a, fa, x): (&mut Brep, &FacePieces, &Piece),
    (b, fb, y): (&mut Brep, &FacePieces, &Piece),
    upper: &mut f64,
    witness: &mut Option<[Point3; 2]>,
) -> Result<()> {
    let (_, ua) = x.centre();
    let (_, ub) = y.centre();
    let mut pa_pt = on_face(k, a, fa, Point2::new(ua[0], ua[1]))?;
    let mut pb_pt = on_face(k, b, fb, Point2::new(ub[0], ub[1]))?;
    for _ in 0..PROJECTIONS {
        if let Some(p) = pa_pt {
            if let Some((q, _)) = closest_on(k, b, fb, p)? {
                pb_pt = Some(q);
            }
        }
        if let Some(q) = pb_pt {
            if let Some((p, _)) = closest_on(k, a, fa, q)? {
                pa_pt = Some(p);
            }
        }
        if let (Some(p), Some(q)) = (pa_pt, pb_pt) {
            let d = distance_up(p, q) + fa.slack + fb.slack;
            if d < *upper {
                *upper = d;
                *witness = Some([p, q]);
            }
        }
    }
    Ok(())
}

/// A point of `inner`'s boundary classified in `outer`: `Some(true)` inside its material,
/// `Some(false)` outside it, `None` when none of `inner`'s points tried (its pieces'
/// centres, where their faces hold them: [`on_face`]) is proved more than twice the
/// classification's tolerance from `outer`'s boundary
/// ([`crate::query::distance_bound::face_distance_lower_bound`] over `outer`'s faces), or
/// its classification is not decided.
fn point_in(
    k: &Kernel,
    inner: &mut Brep,
    p_inner: &Prepared,
    outer: &Brep,
) -> Result<Option<bool>> {
    use crate::query::distance_bound::face_distance_lower_bound;
    let tol = brep_zone_tol_3d(k, outer, SmallTopoType::NormalSize);
    let clear = 2.0 * tol;
    for fp in &p_inner.faces {
        for piece in &fp.pieces {
            let (_, uv) = piece.centre();
            let Some(p) = on_face(k, inner, fp, Point2::new(uv[0], uv[1]))? else {
                continue;
            };
            let mut far = true;
            for &g in outer.face_list() {
                if face_distance_lower_bound(k, outer, g, p, clear)? <= clear {
                    far = false;
                    break;
                }
            }
            if !far {
                continue;
            }
            let mut classifier = BrepClassifier::of_brep(outer);
            let pc = match classifier.point_3d_classify(k, p, tol, true) {
                Ok(pc) => pc,
                Err(e) if e.is_cancelled() => return Err(e),
                Err(_) => return Ok(None),
            };
            return Ok(match (pc.point_class(), pc.object()) {
                (PointClassType::Region, Some(EntityRef::Region(r))) => {
                    Some(region_material(classifier.brep(), r)?)
                }
                _ => None,
            });
        }
    }
    Ok(None)
}

/// Whether `a` and `b` meet at `tol` (module docs), `a` prepared once for several `b`s.
///
/// # Errors
///
/// `InvalidInput` for a tolerance not finite and positive, or a body without faces; those
/// of the pieces, the classifications' cancellation and the closest-point solves' setup.
pub fn meets_prepared(
    k: &Kernel,
    a: &mut Brep,
    pa: &Prepared,
    b: &mut Brep,
    tol: f64,
) -> Result<Contact> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the tolerance is finite and positive",
        ));
    }
    let pb = Prepared::new(k, b, pa.origin)?;
    let stop = |lower: f64, upper: f64| upper <= tol || lower > tol;
    let (lower, upper, witness, done) = bracket(k, a, pa, b, &pb, tol, &stop)?;
    let verdict = if upper <= tol {
        Verdict::Yes
    } else if !done {
        Verdict::Undecided
    } else {
        // the boundaries are more than tol apart: they meet only if one encloses the other
        match (point_in(k, b, &pb, a)?, point_in(k, a, pa, b)?) {
            (Some(true), _) | (_, Some(true)) => Verdict::Yes,
            (Some(false), Some(false)) => Verdict::No,
            _ => Verdict::Undecided,
        }
    };
    Ok(Contact {
        verdict,
        lower,
        upper,
        witness,
    })
}

/// [`meets_prepared`] for one pair: `a` prepared about the centre of both bodies' boxes.
///
/// # Errors
///
/// Those of [`meets_prepared`].
pub fn meets(k: &Kernel, a: &mut Brep, b: &mut Brep, tol: f64) -> Result<Contact> {
    let origin = common_origin(k, a, b)?;
    let pa = Prepared::new(k, a, origin)?;
    meets_prepared(k, a, &pa, b, tol)
}

/// The bracket of the distance between `a`'s and `b`'s boundaries, refined until it is at
/// most `width` wide (relative to the upper end, or absolute below 1) or the pairs reach
/// [`MAX_PAIRS`]; the verdict is `Yes` when it is that narrow, `Undecided` when the work
/// ran out first. The bracket always holds the distance; it can stay wider than `width`
/// where a nearest point lies on a face's trim, since a piece straddling the trim carries
/// surface off the face and is cut finer only along the trim.
///
/// # Errors
///
/// `InvalidInput` for a width not finite and positive; those of [`meets`].
pub fn boundary_distance(k: &Kernel, a: &mut Brep, b: &mut Brep, width: f64) -> Result<Contact> {
    if !(width.is_finite() && width > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the width is finite and positive",
        ));
    }
    let origin = common_origin(k, a, b)?;
    let pa = Prepared::new(k, a, origin)?;
    let pb = Prepared::new(k, b, origin)?;
    let stop = |lower: f64, upper: f64| upper - lower <= width * upper.max(1.0);
    let (lower, upper, witness, done) = bracket(k, a, &pa, b, &pb, f64::INFINITY, &stop)?;
    Ok(Contact {
        verdict: if done {
            Verdict::Yes
        } else {
            Verdict::Undecided
        },
        lower,
        upper,
        witness,
    })
}
