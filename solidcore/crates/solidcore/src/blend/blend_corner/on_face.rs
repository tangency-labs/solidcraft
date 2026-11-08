//! Whether a Setback's spring curve stays inside its trimmed face: it comes within no gap of
//! any of the face's other edges (solidcore#2343 (d), the owner's ruling B there).
//!
//! Upstream: none. The fork's ruling on smlib#85 part C asked that the spring curve's
//! parameter-plane control triangle `(P^, X^, Q^)` lie inside the trimmed face; that is the
//! convex-hull bound for the curve itself, sound but loose (it refuses setbacks whose spring
//! curve stays on the face). Ruling B on solidcore#2343 checks the curve.
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`Clearance`]     | none     |
//! | [`bezier_pieces`] | none     |
//! | [`clear_of`]      | none     |
//!
//! Decided on control points, never at sample points (`docs/definitions-and-certificates.md`
//! §1): a rational Bézier piece with positive weights lies in the convex hull of its
//! control points (Farin, *Curves and Surfaces for CAGD*). Two pieces whose hulls are
//! proved farther apart than the gap and both pieces' rounding (their boxes', or their gap
//! along the line between their centroids: `apart`) come no nearer than the gap. Otherwise the wider of the two is halved (de Casteljau on its
//! homogeneous points) and the halves are tried in turn. A piece halved [`DEPTH`] times is
//! halved no further, and a pair still undecided then, or once [`PAIRS`] pairs have been
//! tried, is [`Clearance::Near`]: an undecided spring curve is not taken as clear. The
//! rounding is Higham's `gamma` of the operations each point has been through, as
//! `blend_setback::containment` bounds its patches.
//!
//! Deviations: no recursion: the pairs are kept on an explicit stack, each piece halved at
//! most [`DEPTH`] times, at most [`PAIRS`] pairs per curve and piece list.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::math::{gamma, NOW, NOZ};

const OP: &str = "blend::blend_corner::on_face::clear_of";

/// A homogeneous point `(x w, y w, z w, w)`.
pub type Hom = [f64; 4];

/// How many times a piece is halved at most: a piece of a part's size, tens of millimetres,
/// is then under 1e-10 long, the rounding of the part's coordinates (§4.4 of the
/// definitions doc).
pub const DEPTH: usize = 40;

/// How many pairs of pieces [`clear_of`] tries at most for one curve against one list of
/// pieces. Near a closest approach a few pairs stay at each halving, so a decided case needs
/// a few hundred.
pub const PAIRS: usize = 4096;

/// Whether one curve comes within a gap of another ([`clear_of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clearance {
    /// Proved to stay farther than the gap from the other everywhere.
    Clear,
    /// Within the gap somewhere, or not shown not to be within [`DEPTH`] and [`PAIRS`].
    Near,
}

/// The rational Bézier pieces of `curve` as homogeneous points, one piece per knot span: of
/// the B-spline a line, a conic or a B-spline curve is built on (the whole of it, which holds
/// an edge it is trimmed to). `None` for a curve of another kind, a planar (2D) one, or one
/// whose weights are not all positive and finite: such a curve has no hull to bound it by.
///
/// # Errors
///
/// Those of the B-spline's decomposition into its Bézier pieces.
pub fn bezier_pieces(curve: &Curve) -> Result<Option<Vec<Vec<Hom>>>> {
    let base: &BSplineCurve = match curve.kind() {
        CurveKind::BSpline(c) => c,
        CurveKind::Line(c) => c.base(),
        CurveKind::Ellipse(c) => c.base(),
        CurveKind::Hyperbola(c) => &c.base,
        CurveKind::Parabola(c) => c.base(),
        _ => return Ok(None),
    };
    let Some(nurb) = base.nurb.as_ref() else {
        return Ok(None);
    };
    let mut pieces = Vec::new();
    for bez in nurb.decompose_bez()? {
        let mut piece = Vec::new();
        for cp in bez.cpts() {
            if cp.z == NOZ {
                return Ok(None);
            }
            let h = if cp.w == NOW {
                [cp.x, cp.y, cp.z, 1.0]
            } else {
                [cp.x, cp.y, cp.z, cp.w]
            };
            if !h.iter().all(|c| c.is_finite()) || h[3] <= 0.0 {
                return Ok(None);
            }
            piece.push(h);
        }
        pieces.push(piece);
    }
    Ok(Some(pieces))
}

/// A piece on the stack: its homogeneous points, how many more times it may be halved, and
/// the rounding bound of its Euclidean points (that of the piece it was halved from).
#[derive(Clone)]
struct Piece {
    points: Vec<Hom>,
    depth: usize,
    rounding: f64,
}

impl Piece {
    /// A piece as given: its Euclidean points are within `rounding` of the exact ones after
    /// [`DEPTH`] halvings of `degree` averaging steps each and the division by the weight:
    /// `3 gamma(ops + 2)` of its largest homogeneous coordinate over its least weight (shrunk
    /// by the weights' own rounding), `ops = 8 + DEPTH degree`. `None` where a weight is not
    /// positive or a coordinate not finite.
    fn new(points: &[Hom]) -> Option<Self> {
        let degree = points.len().checked_sub(1)?;
        let (mut most, mut w_min) = (0.0f64, f64::INFINITY);
        for h in points {
            if !h.iter().all(|c| c.is_finite()) {
                return None;
            }
            most = most.max(h[0].abs()).max(h[1].abs()).max(h[2].abs());
            w_min = w_min.min(h[3]);
        }
        let ops = 8 + DEPTH * degree;
        let shrink = 1.0 - gamma(ops);
        if !(w_min > 0.0 && shrink > 0.0) {
            return None;
        }
        Some(Self {
            points: points.to_vec(),
            depth: DEPTH,
            rounding: 3.0 * gamma(ops + 2) * most / (w_min * shrink),
        })
    }

    /// Its Euclidean points; `None` where a weight is not positive.
    fn euclidean(&self) -> Option<Vec<Vec3>> {
        let mut out = Vec::new();
        for h in &self.points {
            if h[3].is_nan() || h[3] <= 0.0 {
                return None;
            }
            out.push(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]));
        }
        Some(out)
    }

    /// Its two halves at the middle of its parameter (de Casteljau), each halvable once less.
    fn halves(&self) -> (Self, Self) {
        let d = self.points.len().saturating_sub(1);
        let mut work = self.points.clone();
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
                        *w = [
                            0.5 * (a[0] + b[0]),
                            0.5 * (a[1] + b[1]),
                            0.5 * (a[2] + b[2]),
                            0.5 * (a[3] + b[3]),
                        ];
                    }
                }
            }
        }
        right.reverse();
        let depth = self.depth.saturating_sub(1);
        (
            Self {
                points: left,
                depth,
                rounding: self.rounding,
            },
            Self {
                points: right,
                depth,
                rounding: self.rounding,
            },
        )
    }
}

/// The widest extent of `points` along an axis.
fn width(points: &[Vec3]) -> f64 {
    let (lo, hi) = extent(points);
    (hi.x - lo.x).max(hi.y - lo.y).max(hi.z - lo.z)
}

/// The box of `points`, its least and greatest corners.
fn extent(points: &[Vec3]) -> (Vec3, Vec3) {
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    (lo, hi)
}

/// A lower bound on the distance between the convex hulls of `a` and `b`, less its own
/// rounding: the larger of the distance between their boxes and their gap along the line
/// between their centroids (any unit direction `u` separates them by `min u . b - max u . a`;
/// that one closes on the hulls' distance as near-parallel pieces shrink, where the boxes'
/// does not). The boxes' gaps are rounded by `gamma(1)` of the largest coordinate `s`, their
/// sum of squares by `gamma(6)`; each projection by `gamma(3) sqrt 3 s |u|`, and the quotient
/// by `|u|` by `gamma(4)`.
fn apart(a: &[Vec3], b: &[Vec3]) -> f64 {
    let ((alo, ahi), (blo, bhi)) = (extent(a), extent(b));
    let size = [alo, ahi, blo, bhi].iter().fold(0.0f64, |m, c| {
        m.max(c.x.abs()).max(c.y.abs()).max(c.z.abs())
    });
    let gap = |l: f64, h: f64| l - h;
    let mut squares = 0.0;
    for d in [
        gap(blo.x, ahi.x).max(gap(alo.x, bhi.x)),
        gap(blo.y, ahi.y).max(gap(alo.y, bhi.y)),
        gap(blo.z, ahi.z).max(gap(alo.z, bhi.z)),
    ] {
        let d = (d - gamma(1) * size).max(0.0);
        squares += d * d;
    }
    let boxes = sqrt(squares * (1.0 - gamma(6))) * (1.0 - gamma(2));
    let centroid = |p: &[Vec3]| {
        let (sum, count) = p
            .iter()
            .fold((Vec3::ZERO, 0.0), |(s, c), &q| (s + q, c + 1.0));
        sum * (1.0 / count)
    };
    let w = centroid(b) - centroid(a);
    // the line between the centroids, and its parts square to each piece's chord (for
    // near-parallel pieces side by side, whatever their shift along each other)
    let square_to = |p: &[Vec3]| match (p.first(), p.last()) {
        (Some(&first), Some(&last)) => {
            let t = last - first;
            let tt = t.dot(t);
            if tt > 0.0 {
                w - t * (w.dot(t) / tt)
            } else {
                w
            }
        }
        _ => w,
    };
    let mut best = boxes;
    for u in [w, square_to(a), square_to(b)] {
        let length = u.length();
        if length.is_nan() || length <= 0.0 {
            continue;
        }
        let reach_a = a.iter().fold(f64::NEG_INFINITY, |m, p| m.max(u.dot(*p)));
        let reach_b = b.iter().fold(f64::INFINITY, |m, p| m.min(u.dot(*p)));
        let along = (reach_b - reach_a) / length * (1.0 - gamma(4))
            - 2.0 * gamma(3) * sqrt(3.0) * size * (1.0 + gamma(4));
        best = best.max(along);
    }
    best
}

/// Whether the rational Bézier curve `curve` (its homogeneous Bernstein coefficients) comes
/// within `gap` of any of `pieces` (each such a curve too): [`Clearance::Clear`] only where
/// that is proved on the pieces' control points (module docs).
///
/// # Errors
///
/// The kernel's cancellation.
pub fn clear_of(k: &Kernel, curve: &[Hom], pieces: &[Vec<Hom>], gap: f64) -> Result<Clearance> {
    let Some(a) = Piece::new(curve) else {
        return Ok(Clearance::Near);
    };
    let mut stack = Vec::new();
    for p in pieces {
        let Some(b) = Piece::new(p) else {
            return Ok(Clearance::Near);
        };
        stack.push((a.clone(), b));
    }
    let mut tried = 0;
    while let Some((a, b)) = stack.pop() {
        k.check_cancelled(OP)?;
        tried += 1;
        if tried > PAIRS {
            return Ok(Clearance::Near);
        }
        let (Some(pa), Some(pb)) = (a.euclidean(), b.euclidean()) else {
            return Ok(Clearance::Near);
        };
        if apart(&pa, &pb) > gap + a.rounding + b.rounding {
            continue;
        }
        let (wa, wb) = (width(&pa), width(&pb));
        // the wider is halved, where it may be; neither: undecided
        let halve_a = a.depth > 0 && (wa >= wb || b.depth == 0);
        if halve_a {
            let (a0, a1) = a.halves();
            stack.push((a1, b.clone()));
            stack.push((a0, b));
        } else if b.depth > 0 {
            let (b0, b1) = b.halves();
            stack.push((a.clone(), b1));
            stack.push((a, b0));
        } else {
            return Ok(Clearance::Near);
        }
    }
    Ok(Clearance::Clear)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter circle of radius `r` about `(cx, cy)` in z = 0, from angle 0 to 90 degrees:
    /// the rational quadratic with middle weight `cos 45`.
    fn quarter(cx: f64, cy: f64, r: f64) -> Vec<Hom> {
        let w = 0.5f64.sqrt();
        vec![
            [cx + r, cy, 0.0, 1.0],
            [(cx + r) * w, (cy + r) * w, 0.0, w],
            [cx, cy + r, 0.0, 1.0],
        ]
    }

    /// A segment from `a` to `b`, degree 1.
    fn segment(a: [f64; 3], b: [f64; 3]) -> Vec<Hom> {
        vec![[a[0], a[1], a[2], 1.0], [b[0], b[1], b[2], 1.0]]
    }

    #[test]
    fn a_segment_near_a_circle_is_clear_by_more_than_the_gap_and_near_within_it() {
        let k = Kernel::default();
        let arc = vec![quarter(0.0, 0.0, 10.0)];
        // the line x + y = c outside the circle is closest to the arc at 45 degrees, c / sqrt 2
        // - 10 away, and runs along it within twice that for a hundredth of a turn
        let c = 2.0f64.sqrt() * (10.0 + 1e-3);
        let chord = segment([c, 0.0, 0.0], [0.0, c, 0.0]);
        assert_eq!(
            clear_of(&k, &chord, &arc, 0.9e-3).unwrap(),
            Clearance::Clear
        );
        assert_eq!(clear_of(&k, &chord, &arc, 1.1e-3).unwrap(), Clearance::Near);
        // inside it, crossing it twice: near at any gap
        let c = 2.0f64.sqrt() * (10.0 - 1e-3);
        let chord = segment([c, 0.0, 0.0], [0.0, c, 0.0]);
        assert_eq!(clear_of(&k, &chord, &arc, 0.0).unwrap(), Clearance::Near);
    }

    #[test]
    fn pieces_far_apart_are_clear_at_once_and_a_bad_weight_is_near() {
        let k = Kernel::default();
        let far = vec![quarter(100.0, 0.0, 1.0)];
        let chord = segment([0.0, 0.0, 0.0], [1.0, 1.0, 0.0]);
        assert_eq!(clear_of(&k, &chord, &far, 1e-6).unwrap(), Clearance::Clear);
        let mut bad = quarter(100.0, 0.0, 1.0);
        bad[1][3] = 0.0;
        assert_eq!(clear_of(&k, &chord, &[bad], 1e-6).unwrap(), Clearance::Near);
    }
}
