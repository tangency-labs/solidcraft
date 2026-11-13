//! The corner's piece of a face at a Setback beside curved faces, and whether it holds
//! another edge or loop of the face (solidcore#2343 (d), condition 2).
//!
//! Upstream: none. The brief's item (d) asks that the piece the corner takes out of each face,
//! between the face's spring curve and the vertex, hold no other edge or loop of the face;
//! condition 1 (the spring curve inside its trimmed face, [`super::on_face`]) does not see a
//! loop wholly inside the piece, and the fill was built over it.
//!
//! | Rust               | Upstream |
//! |--------------------|----------|
//! | [`CornerPiece`]    | none     |
//! | [`Decision`]       | none     |
//! | [`OtherEdge`]      | none     |
//! | [`section_trace`]  | none     |
//! | [`restrict`]       | none     |
//! | `winding` (private) | none    |
//!
//! The piece's boundary is a closed chain of rational Bezier sides with positive weights,
//! each held in 3-D and in the face's parameter plane ([`FacePlane`], where each side is
//! rational too): the face's two blended edges from the vertex to their setback points, each
//! end section's trace on the face from its setback point to its rail end (a plane section of
//! the face, [`section_trace`]), and the spring curve's two halves. The decision, for each
//! other edge of the face:
//! - (a) every side but the spring curve's halves (condition 1 checks those) keeps clear of
//!   the edge by more than the tolerances, proved on control-point hulls
//!   ([`on_face::clear_of`]: explicit-stack halving bounded by [`on_face::DEPTH`] and
//!   [`on_face::PAIRS`], polling the kernel's cancel token);
//! - (b) given (a), the edge (connected, and meeting no side) lies wholly inside the piece or
//!   wholly outside it, so its winding number in the parameter plane about any one of its
//!   points decides which. That one point is exact because (a) is proved, not a sample. The
//!   winding number is a sum of turning angles over pieces of the boundary each proved, on its
//!   control points, to lie in an open half-plane through the point, so that its angle is the
//!   one between its ends; a piece not so placed is halved, at most [`on_face::DEPTH`] times
//!   and over [`on_face::PAIRS`] pieces in all. The sum is an integer multiple of a full turn
//!   to within a quarter turn or it is not taken.
//!
//! [`Decision::Clear`] only where every other edge is proved clear and outside;
//! [`Decision::Blocked`] where one is proved inside; [`Decision::Undecided`] (which refuses
//! as Blocked does) otherwise. Rounding is Higham's `gamma` of the operations each point has
//! been through, at the scale of the corner's coordinates (as [`on_face`] counts it), and
//! every acceptance comparison is written so that NaN refuses.
//!
//! Deviations: no recursion: the halvings are kept on explicit stacks, bounded as above.

use crate::blend::blend_corner::on_face::{self, Clearance, Hom, Span};
use crate::blend::blend_setback::{Face, FaceKind, FacePlane, Hom2};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::math::gamma;

const OP: &str = "blend::blend_corner::piece";

/// Whether a corner's piece of a face holds another edge or loop of the face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Proved to hold none: every other edge of the face keeps clear of its boundary and lies
    /// outside it.
    Clear,
    /// Proved to hold one: an edge keeps clear of its boundary and lies inside it.
    Blocked,
    /// Neither proved within the bounds on the work: refused, as [`Decision::Blocked`] is.
    Undecided,
}

/// One side of a [`CornerPiece`]'s boundary: a rational Bezier piece with positive weights.
#[derive(Debug, Clone)]
pub(crate) struct Side {
    /// Its homogeneous points in 3-D.
    pub(crate) space: Vec<Hom>,
    /// Its homogeneous points in the face's parameter plane ([`FacePlane::net`]).
    pub(crate) plane: Vec<Hom2>,
    /// What its construction may put it off the curve it stands for, past what
    /// [`on_face::clear_of`] counts itself: added to the gap it keeps from other edges.
    pub(crate) off: f64,
    /// A spring curve's half (condition 1 checks it against the face's other edges).
    pub(crate) spring: bool,
    /// The rounding bound of its plane points' Euclidean coordinates, through the plane's
    /// map and the halvings [`winding`] may make ([`CornerPiece::map_rounding`]).
    pub(crate) plane_off: f64,
}

/// The piece a Setback corner takes out of one of its faces (module docs): its boundary from
/// the vertex, as a chain of sides that closes.
#[derive(Debug, Clone)]
pub(crate) struct CornerPiece {
    /// The face's parameter plane, in the frame its spring curve is built in.
    pub(crate) plane: FacePlane,
    /// The boundary's sides in order round it.
    pub(crate) sides: Vec<Side>,
}

/// Another edge of a piece's face, as [`CornerPiece::decide`] takes it.
#[derive(Debug, Clone)]
pub(crate) struct OtherEdge {
    /// Its curve's rational Bezier pieces in 3-D over the edge (or the whole curve).
    pub(crate) pieces: Vec<Vec<Hom>>,
    /// The gap it must keep from the piece's boundary: the corner's tolerance and its own.
    pub(crate) gap: f64,
    /// Points on it, any of which (the first the plane maps) its winding number is taken at.
    pub(crate) points: Vec<Point3>,
}

impl CornerPiece {
    /// A piece in `plane` with no sides yet.
    pub(crate) fn new(plane: FacePlane) -> Self {
        Self {
            plane,
            sides: Vec::new(),
        }
    }

    /// Adds the side with homogeneous 3-D points `space`, off the curve it stands for by at
    /// most `off`; `false` (nothing added) where its points or weights are not finite and
    /// positive, or the plane does not map it ([`FacePlane::net`]).
    pub(crate) fn push(&mut self, space: Vec<Hom>, off: f64, spring: bool) -> bool {
        if space.len() < 2
            || !space
                .iter()
                .all(|h| h.iter().all(|c| c.is_finite()) && h[3] > 0.0)
        {
            return false;
        }
        let Some(plane) = self.plane.net(&space) else {
            return false;
        };
        let plane_off = self.map_rounding(&space, &plane);
        self.sides.push(Side {
            space,
            plane,
            off,
            spring,
            plane_off,
        });
        true
    }

    /// The rounding bound of the Euclidean points of `plane`, the plane's net of `space`
    /// ([`FacePlane::net`]), after the halvings [`winding`] may make: `3 gamma(ops + 2)` of the
    /// map's terms' size (the 3-D points' coordinates and the frame's origin, the radius, and
    /// the slope's share) times one more than the plane points' size, over the least plane
    /// weight per 3-D weight (a quadric's `rho + u1` or `R + u3`), `ops` the map's (at most
    /// 32 and four per 3-D point) and the halvings' (`DEPTH` times the plane degree).
    /// Infinite where a weight is not positive.
    fn map_rounding(&self, space: &[Hom], plane: &[Hom2]) -> f64 {
        let o = self
            .plane
            .o
            .x
            .abs()
            .max(self.plane.o.y.abs())
            .max(self.plane.o.z.abs());
        let (mut size, mut w_max) = (0.0f64, 0.0f64);
        for h in space {
            let most = h[0].abs().max(h[1].abs()).max(h[2].abs());
            size = size.max(most / h[3]);
            w_max = w_max.max(h[3]);
        }
        let size = (size + o + self.plane.radius.abs()) * (1.0 + self.plane.slope.abs());
        let (mut reach, mut w_min) = (0.0f64, f64::INFINITY);
        for h in plane {
            reach = reach.max((h[0] / h[2]).abs()).max((h[1] / h[2]).abs());
            w_min = w_min.min(h[2]);
        }
        let per = match self.plane.kind {
            FaceKind::Cylinder | FaceKind::Cone => w_max * w_max,
            FaceKind::Plane | FaceKind::Sphere => w_max,
        };
        let ops = 32 + 4 * space.len() + on_face::DEPTH * plane.len().saturating_sub(1);
        let shrink = 1.0 - gamma(ops);
        let least = w_min / per;
        if !(least > 0.0 && shrink > 0.0 && reach.is_finite()) {
            return f64::INFINITY;
        }
        3.0 * gamma(ops + 2) * size * (1.0 + reach) / (least * shrink)
    }

    /// Whether the piece holds another edge of its face, of `others` (module docs).
    ///
    /// # Errors
    ///
    /// The kernel's cancellation.
    pub(crate) fn decide(&self, k: &Kernel, others: &[OtherEdge]) -> Result<Decision> {
        if self.sides.is_empty() {
            return Ok(Decision::Undecided);
        }
        for other in others {
            // (a): clear of every side but the spring curve's
            for side in self.sides.iter().filter(|s| !s.spring) {
                let gap = other.gap + side.off;
                let gap_valid = gap >= 0.0;
                if !gap_valid
                    || on_face::clear_of(k, &side.space, &other.pieces, gap)? != Clearance::Clear
                {
                    return Ok(Decision::Undecided);
                }
            }
            // (b): inside or outside, at one point
            let Some((x, z)) = other
                .points
                .iter()
                .find_map(|&x| self.plane.point(x).map(|z| (x, z)))
            else {
                return Ok(Decision::Undecided);
            };
            let z_off = self.map_rounding(&[[x.x, x.y, x.z, 1.0]], &[[z[0], z[1], 1.0]]);
            match winding(k, &self.sides, z, z_off)? {
                Some(false) => {}
                Some(true) => return Ok(Decision::Blocked),
                None => return Ok(Decision::Undecided),
            }
        }
        Ok(Decision::Clear)
    }
}

/// The rounding bound of a 3-D homogeneous point set's Euclidean points after `ops` more
/// operations: `3 gamma(ops + 2)` of its largest homogeneous coordinate over its least weight
/// (as [`on_face`]'s pieces count it). Infinite where a weight is not positive.
fn rounding(points: &[Hom], ops: usize) -> f64 {
    let (mut most, mut w_min) = (0.0f64, f64::INFINITY);
    for h in points {
        most = most.max(h[0].abs()).max(h[1].abs()).max(h[2].abs());
        w_min = w_min.min(h[3]);
    }
    let shrink = 1.0 - gamma(ops);
    if !(w_min > 0.0 && shrink > 0.0) {
        return f64::INFINITY;
    }
    3.0 * gamma(ops + 2) * most / (w_min * shrink)
}

/// The homogeneous halves of a rational Bezier at `s` (de Casteljau).
fn split<const N: usize>(p: &[[f64; N]], s: f64) -> (Vec<[f64; N]>, Vec<[f64; N]>) {
    let d = p.len().saturating_sub(1);
    let mut work = p.to_vec();
    let mut left = Vec::new();
    let mut right = Vec::new();
    for k in 0..=d {
        if let Some(&h) = work.first() {
            left.push(h);
        }
        if let Some(&h) = work.get(d - k) {
            right.push(h);
        }
        for i in 0..d - k {
            if let (Some(&a), Some(&b)) = (work.get(i), work.get(i + 1)) {
                if let Some(w) = work.get_mut(i) {
                    for (c, (&ac, &bc)) in w.iter_mut().zip(a.iter().zip(b.iter())) {
                        *c = (1.0 - s) * ac + s * bc;
                    }
                }
            }
        }
    }
    right.reverse();
    (left, right)
}

/// The pieces of a curve given by its knot spans ([`on_face::bezier_spans`]) over the
/// parameters from `from` to `to` (either way round), in that direction, each with its
/// rounding past [`on_face::clear_of`]'s own count (the two cuts of de Casteljau at most:
/// `2 degree` operations). Empty where the interval holds none of the spans.
pub(crate) fn restrict(spans: &[Span], from: f64, to: f64) -> Vec<(Vec<Hom>, f64)> {
    let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
    let mut out = Vec::new();
    for ([a, b], piece) in spans {
        let (a, b) = (*a, *b);
        let overlaps = b > a && hi > a && lo < b;
        if !overlaps {
            continue;
        }
        let s0 = (lo.max(a) - a) / (b - a);
        let s1 = (hi.min(b) - a) / (b - a);
        let ordered = s1 > s0;
        if !ordered {
            continue;
        }
        let left = if s1 < 1.0 {
            split(piece, s1).0
        } else {
            piece.clone()
        };
        let part = if s0 > 0.0 {
            split(&left, s0 / s1).1
        } else {
            left
        };
        let degree = part.len().saturating_sub(1);
        let off = rounding(&part, 2 * degree);
        out.push((part, off));
    }
    if from > to {
        out.reverse();
        for (part, _) in &mut out {
            part.reverse();
        }
    }
    out
}

/// The root in `(0, 1)` of `a l^2 + b l + c`, where there is exactly one.
fn root_in_unit(a: f64, b: f64, c: f64) -> Option<f64> {
    let inside = |l: f64| l > 0.0 && l < 1.0;
    let roots: Vec<f64> = if a.abs() <= EFF_ZERO * (b.abs() + c.abs()) {
        if b == 0.0 {
            Vec::new()
        } else {
            vec![-c / b]
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc.is_nan() || disc < 0.0 {
            return None;
        }
        let q = -0.5 * (b + b.signum() * sqrt(disc));
        if q == 0.0 {
            vec![0.0]
        } else {
            vec![q / a, c / q]
        }
    };
    let mut found = roots.into_iter().filter(|&l| inside(l));
    let first = found.next()?;
    if found.next().is_some() {
        return None;
    }
    Some(first)
}

/// Where the line `x + l d` crosses the quadric `face`, for `l` in `(0, 1)`: a root of its
/// implicit equation (the sphere's `|u|^2 - R^2`, the cylinder's or cone's `rho^2 - (R +
/// slope h)^2` on the nappe where `R + slope h > 0`). `None` on a plane, or where there is not
/// exactly one such root.
fn shoulder(face: &Face, x: Point3, d: Vec3) -> Option<f64> {
    let a = x - face.point;
    let r = face.radius;
    match face.kind {
        FaceKind::Plane => None,
        FaceKind::Sphere => root_in_unit(d.dot(d), 2.0 * a.dot(d), a.dot(a) - r * r),
        FaceKind::Cylinder | FaceKind::Cone => {
            let ax = face.axis;
            let (ha, hd) = (a.dot(ax), d.dot(ax));
            let (ap, dp) = (a - ax * ha, d - ax * hd);
            let k = face.slope;
            let rho = r + k * ha;
            let l = root_in_unit(
                dp.dot(dp) - k * k * hd * hd,
                2.0 * (ap.dot(dp) - k * rho * hd),
                ap.dot(ap) - rho * rho,
            )?;
            (r + k * (ha + l * hd) > 0.0).then_some(l)
        }
    }
}

/// The trace on `face` of the plane through `e` square to `normal` (an end section's plane,
/// square to its edge at its setback point `e`) from `e` to `s` (the round's rail end on the
/// face, in that plane): a plane section of the face, as a rational Bezier with positive
/// weights, and what it may be off the trace past rounding. The conic through `e` and `s`
/// tangent there to the section (the plane's normal crossed with the face's) meeting at `m`,
/// its weight from the point where the line from the chord's middle to `m` meets the face (the
/// conic's shoulder, `(mid + w m) / (1 + w)`): exact on any quadric. A straight segment where
/// each end lies within a thousandth of `tol` of the other's tangent line (a plane, or a
/// quadric's ruling: the conic then strays from its chord by no more, the second value).
/// `None` where `e` and `s` coincide, a tangent is not defined, the tangents run parallel
/// without the chord along them (half the conic or more) or the shoulder is not found.
pub(crate) fn section_trace(
    face: &Face,
    normal: Vec3,
    e: Point3,
    s: Point3,
    tol: f64,
) -> Option<(Vec<Hom>, f64)> {
    let n = normal.unitize().ok()?;
    let chord = s - e;
    let long = chord.length() > 0.0;
    if !long {
        return None;
    }
    let te = n.cross(face.distance(e).1).unitize().ok()?;
    let ts = n.cross(face.distance(s).1).unitize().ok()?;
    let (off_e, off_s) = (chord.cross(te).length(), chord.cross(ts).length());
    let delta = 1.0e-3 * tol;
    let hom = |p: Point3, w: f64| [p.x * w, p.y * w, p.z * w, w];
    if off_e <= delta && off_s <= delta {
        let net = vec![hom(e, 1.0), hom(s, 1.0)];
        let off = rounding(&net, 8) + off_e.max(off_s);
        return Some((net, off));
    }
    let c = te.cross(ts);
    let cc = c.dot(c);
    let crossing = cc > EFF_ZERO;
    if !crossing {
        return None;
    }
    let alpha = chord.cross(ts).dot(c) / cc;
    let m = e + te * alpha;
    let mid = (e + s) * 0.5;
    let l = shoulder(face, mid, m - mid)?;
    let w = l / (1.0 - l);
    let positive = w > 0.0 && w.is_finite();
    if !positive {
        return None;
    }
    let net = vec![hom(e, 1.0), hom(m, w), hom(s, 1.0)];
    // the tangents' crossing and the shoulder's weight, a few dozen operations from e, s and
    // the face (the conic moves with them by their rounding)
    let off = rounding(&net, 64);
    Some((net, off))
}

/// A boundary piece on [`winding`]'s stack: its homogeneous plane points, how many more
/// times it may be halved, and the rounding bound of its Euclidean points.
struct Arc {
    points: Vec<Hom2>,
    depth: usize,
    rounding: f64,
}

/// Whether the winding number about the plane point `z` (its rounding `z_off`) of the closed
/// chain of `sides` is not zero (module docs, (b)), the gaps between consecutive sides' ends
/// (rounding) closed by straight pieces. `None` where it is not proved within
/// [`on_face::DEPTH`] halvings of a piece and [`on_face::PAIRS`] pieces, or the sum is not
/// within a quarter turn of a whole number of turns.
fn winding(k: &Kernel, sides: &[Side], z: [f64; 2], z_off: f64) -> Result<Option<bool>> {
    let euclid = |h: &Hom2| [h[0] / h[2], h[1] / h[2]];
    let mut stack: Vec<Arc> = Vec::new();
    let mut ends: Vec<([f64; 2], [f64; 2], f64)> = Vec::new();
    for side in sides {
        let (Some(first), Some(last)) = (side.plane.first(), side.plane.last()) else {
            return Ok(None);
        };
        ends.push((euclid(first), euclid(last), side.plane_off));
        stack.push(Arc {
            points: side.plane.clone(),
            depth: on_face::DEPTH,
            rounding: side.plane_off,
        });
    }
    // the closing pieces between one side's end and the next one's start
    for (i, &(_, end, r)) in ends.iter().enumerate() {
        let Some(&(start, _, rn)) = (i + 1).checked_rem(ends.len()).and_then(|j| ends.get(j))
        else {
            return Ok(None);
        };
        if end != start {
            stack.push(Arc {
                points: vec![[end[0], end[1], 1.0], [start[0], start[1], 1.0]],
                depth: on_face::DEPTH,
                rounding: r.max(rn),
            });
        }
    }
    let mut turn = 0.0f64;
    let mut tried = 0;
    while let Some(arc) = stack.pop() {
        k.check_cancelled(OP)?;
        tried += 1;
        if tried > on_face::PAIRS {
            return Ok(None);
        }
        let mut pts = Vec::new();
        for h in &arc.points {
            let positive = h[2] > 0.0;
            if !positive {
                return Ok(None);
            }
            pts.push(euclid(h));
        }
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else {
            return Ok(None);
        };
        // a direction from z the whole hull lies ahead of, by more than the rounding
        let rel = |p: [f64; 2]| [p[0] - z[0], p[1] - z[1]];
        let (ra, rb) = (rel(a), rel(b));
        let u = [ra[0] + rb[0], ra[1] + rb[1]];
        let u1 = u[0].abs() + u[1].abs();
        let ahead = u1 > 0.0
            && pts.iter().all(|&p| {
                let r = rel(p);
                let size = p[0].abs().max(p[1].abs()) + z[0].abs().max(z[1].abs());
                let margin = (arc.rounding + z_off + gamma(4) * 2.0 * size) * u1;
                u[0] * r[0] + u[1] * r[1] > margin
            });
        if ahead {
            turn += (ra[0] * rb[1] - ra[1] * rb[0]).atan2(ra[0] * rb[0] + ra[1] * rb[1]);
            continue;
        }
        if arc.depth == 0 {
            return Ok(None);
        }
        let (left, right) = split(&arc.points, 0.5);
        for half in [right, left] {
            stack.push(Arc {
                points: half,
                depth: arc.depth - 1,
                rounding: arc.rounding,
            });
        }
    }
    let turns = turn / (2.0 * std::f64::consts::PI);
    let whole = turns.round();
    let near_whole = (turns - whole).abs() < 0.25;
    if !near_whole {
        return Ok(None);
    }
    Ok(Some(whole != 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit square's boundary as four sides in the plane z = 0 (its own parameter plane).
    fn square(plane: FacePlane, from: [f64; 2], size: f64) -> CornerPiece {
        let corners = [
            [from[0], from[1]],
            [from[0] + size, from[1]],
            [from[0] + size, from[1] + size],
            [from[0], from[1] + size],
        ];
        let mut piece = CornerPiece::new(plane);
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            assert!(piece.push(
                vec![[a[0], a[1], 0.0, 1.0], [b[0], b[1], 0.0, 1.0]],
                0.0,
                false
            ));
        }
        piece
    }

    fn plane() -> FacePlane {
        let face = Face::plane(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0));
        FacePlane::new(&face, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)).unwrap()
    }

    /// A small circle's quarter arcs about `(cx, cy)` as another edge.
    fn circle(cx: f64, cy: f64, r: f64) -> OtherEdge {
        let w = 0.5f64.sqrt();
        let mut pieces = Vec::new();
        for q in 0..4 {
            let a = f64::from(q) * 0.5 * std::f64::consts::PI;
            let (c0, s0) = (a.cos(), a.sin());
            let (c1, s1) = (
                (a + 0.5 * std::f64::consts::PI).cos(),
                (a + 0.5 * std::f64::consts::PI).sin(),
            );
            pieces.push(vec![
                [cx + r * c0, cy + r * s0, 0.0, 1.0],
                [(cx + r * (c0 + c1)) * w, (cy + r * (s0 + s1)) * w, 0.0, w],
                [cx + r * c1, cy + r * s1, 0.0, 1.0],
            ]);
        }
        OtherEdge {
            pieces,
            gap: 1e-7,
            points: vec![Vec3::new(cx + r, cy, 0.0)],
        }
    }

    #[test]
    fn a_loop_inside_is_blocked_outside_clear_and_across_undecided() {
        let k = Kernel::default();
        let piece = square(plane(), [0.0, 0.0], 1.0);
        assert_eq!(
            piece.decide(&k, &[circle(0.5, 0.5, 0.1)]).unwrap(),
            Decision::Blocked
        );
        assert_eq!(
            piece.decide(&k, &[circle(1.5, 0.5, 0.1)]).unwrap(),
            Decision::Clear
        );
        // just inside and just outside, by a hundredth of the radius
        assert_eq!(
            piece.decide(&k, &[circle(0.5, 0.899, 0.1)]).unwrap(),
            Decision::Blocked
        );
        assert_eq!(
            piece.decide(&k, &[circle(0.5, 1.101, 0.1)]).unwrap(),
            Decision::Clear
        );
        // across the boundary: not clear of it
        assert_eq!(
            piece.decide(&k, &[circle(0.5, 1.0, 0.1)]).unwrap(),
            Decision::Undecided
        );
        // within the gap of it, outside: not shown clear
        let mut near = circle(0.5, 1.1 + 5e-8, 0.1);
        near.gap = 1e-7;
        assert_eq!(piece.decide(&k, &[near]).unwrap(), Decision::Undecided);
    }

    #[test]
    fn a_reversed_boundary_still_holds_its_loop_and_a_nan_side_is_not_taken() {
        let k = Kernel::default();
        let mut piece = square(plane(), [0.0, 0.0], 1.0);
        piece.sides.reverse();
        for s in &mut piece.sides {
            s.space.reverse();
            s.plane.reverse();
        }
        assert_eq!(
            piece.decide(&k, &[circle(0.5, 0.5, 0.1)]).unwrap(),
            Decision::Blocked
        );
        let mut bad = CornerPiece::new(plane());
        assert!(!bad.push(
            vec![[0.0, 0.0, 0.0, 1.0], [f64::NAN, 0.0, 0.0, 1.0]],
            0.0,
            false
        ));
        assert_eq!(
            bad.decide(&k, &[circle(0.5, 0.5, 0.1)]).unwrap(),
            Decision::Undecided
        );
    }

    #[test]
    fn restricting_a_span_keeps_its_points_between_the_parameters() {
        // a quadratic on [2, 4]: x = t on the knot parameter
        let spans = vec![(
            [2.0, 4.0],
            vec![
                [2.0, 0.0, 0.0, 1.0],
                [3.0, 1.0, 0.0, 1.0],
                [4.0, 0.0, 0.0, 1.0],
            ],
        )];
        let parts = restrict(&spans, 3.5, 2.5);
        assert_eq!(parts.len(), 1);
        let p = &parts[0].0;
        assert!((p[0][0] / p[0][3] - 3.5).abs() < 1e-15);
        assert!((p[2][0] / p[2][3] - 2.5).abs() < 1e-15);
        assert!(restrict(&spans, 4.5, 5.0).is_empty());
    }

    #[test]
    fn a_section_of_a_cylinder_is_on_it() {
        let cyl = Face {
            kind: FaceKind::Cylinder,
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 10.0,
            side: 1.0,
            ..Face::default()
        };
        // a plane tilted to the axis through two points of the cylinder
        let e = Vec3::new(10.0, 0.0, 0.0);
        let t = 0.4f64;
        let n = Vec3::new(0.3, -0.2, 1.0).unitize().unwrap();
        // the point of the section at angle t: solve n . (x - e) = 0 for its height
        let x = Vec3::new(10.0 * t.cos(), 10.0 * t.sin(), 0.0);
        let s = Vec3::new(x.x, x.y, -(n.x * (x.x - e.x) + n.y * (x.y - e.y)) / n.z);
        let (net, _) = section_trace(&cyl, n, e, s, 1e-6).unwrap();
        assert_eq!(net.len(), 3);
        for i in 0..=16 {
            let u = f64::from(i) / 16.0;
            let b = [(1.0 - u) * (1.0 - u), 2.0 * u * (1.0 - u), u * u];
            let mut h = [0.0; 4];
            for (c, &bi) in net.iter().zip(&b) {
                for (hc, cc) in h.iter_mut().zip(c) {
                    *hc += bi * cc;
                }
            }
            let p = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
            assert!((p.x.hypot(p.y) - 10.0).abs() < 1e-12, "{i}");
            assert!((p - e).dot(n).abs() < 1e-12, "{i}");
        }
    }
}
