//! Denser output from marched curves, without more Newton solves.
//!
//! Marching places exact points as far apart as the step and turn limits
//! allow. Between two of them the curve is filled in by cubic Hermite
//! interpolation on its exact end tangents, nA × nB, and on both surfaces'
//! parameter velocities along it: on a segment that turns by θ the inserted
//! points lie within about R θ^4 / 384 of the curve (3e-8 R at 0.06 rad), and
//! the parameters with them. A segment whose surfaces are parallel at an end
//! (a coincident region's boundary, sampled finely where it was made) has no
//! such tangent and is left as it is.

use crate::intersect::{Curve, CurvePoint, Intersection, Param};
use crate::patch::Patch;
use crate::piece::{cross, dot, norm, scale, sub, unit, Piece, V3};
use crate::small::{solve_regularized, System};
use crate::Error;

/// Each patch's pieces, to evaluate derivatives at patch parameters.
struct Pieces(Vec<Vec<Piece>>);

impl Pieces {
    fn new(body: &[Patch]) -> Result<Pieces, Error> {
        body.iter()
            .enumerate()
            .map(|(i, p)| p.bezier_pieces(i))
            .collect::<Result<_, _>>()
            .map(Pieces)
    }

    /// The surface's point, unit normal and derivatives (per unit patch parameter) at a curve point's parameters.
    fn at(&self, at: &Param) -> Option<(V3, V3, V3, V3)> {
        let pieces = self.0.get(at.patch)?;
        let piece = pieces
            .iter()
            .find(|p| p.u0 <= at.u && at.u <= p.u1 && p.v0 <= at.v && at.v <= p.v1)?;
        Self::on(piece, at)
    }

    /// As `at`, but a parameter a hair past the patch's domain is read on its nearest piece.
    fn at_any(&self, at: &Param) -> Option<(V3, V3, V3, V3)> {
        let piece = self.nearest(at)?.0;
        Self::on(piece, at)
    }

    /// The piece nearest the parameters, and how far past it they are, in
    /// units of its own size.
    fn nearest(&self, at: &Param) -> Option<(&Piece, f64)> {
        let pieces = self.0.get(at.patch)?;
        let distance = |p: &Piece| {
            (p.u0 - at.u).max(at.u - p.u1).max(0.0) / (p.u1 - p.u0)
                + (p.v0 - at.v).max(at.v - p.v1).max(0.0) / (p.v1 - p.v0)
        };
        pieces
            .iter()
            .map(|p| (p, distance(p)))
            .min_by(|x, y| x.1.total_cmp(&y.1))
    }

    /// Whether the parameters are in the patch's domain, to a hair: past it a
    /// patch's polynomial runs on as a surface the patch is not (across a
    /// tube's crease, its seam, the other side is another sheet: a point
    /// settled there was 9.4e-4 off it, ssi-corpus `y23_t2_ex5_two_loops`).
    fn holds(&self, at: &Param) -> bool {
        self.nearest(at).is_some_and(|(_, d)| d <= 1e-9)
    }

    fn on(piece: &Piece, at: &Param) -> Option<(V3, V3, V3, V3)> {
        let (du, dv) = (piece.u1 - piece.u0, piece.v1 - piece.v0);
        let e = piece.evaluate((at.u - piece.u0) / du, (at.v - piece.v0) / dv);
        let (su, sv) = (scale(e.ds, 1.0 / du), scale(e.dt, 1.0 / dv));
        let n = cross(su, sv);
        let l = norm(n);
        (l > 0.0).then(|| (e.point, scale(n, 1.0 / l), su, sv))
    }
}

/// The parameter velocity (du, dv) whose image under [Su Sv] is closest to `t`.
fn velocity(su: V3, sv: V3, t: V3) -> [f64; 2] {
    let damping = 0.5e-12 * (dot(su, su) + dot(sv, sv));
    solve_regularized(&System::columns2(su, sv, t), damping).map_or([0.0, 0.0], |x| [x[0], x[1]])
}

/// A curve point's unit tangent along the chord's way, and both surfaces' parameter velocities along it.
fn tangent(a: &Pieces, b: &Pieces, q: &CurvePoint, chord: V3) -> Option<(V3, [f64; 2], [f64; 2])> {
    let (_, na, sua, sva) = a.at(&q.a)?;
    let (_, nb, sub_, svb) = b.at(&q.b)?;
    let t = cross(na, nb);
    if !(norm(t) > 1e-6) {
        return None;
    }
    let t = unit(t);
    let t = if dot(t, chord) < 0.0 {
        scale(t, -1.0)
    } else {
        t
    };
    Some((t, velocity(sua, sva, t), velocity(sub_, svb, t)))
}

/// An interpolated point, checked on both surfaces at its own parameters: kept
/// when the two surface points agree within `gap`, else moved onto the curve by
/// Newton on (uA, vA, uB, vB), held to the plane through it across `along`
/// (the parameter curve can bend more than the space curve: a torus against an
/// ellipsoid, ssi-corpus `y23_t3_ex2`, left points 1.01e-5 off at 0.06 rad).
/// None when that does not converge, the point then left out.
fn settle(a: &Pieces, b: &Pieces, mut q: CurvePoint, along: V3, gap: f64) -> Option<CurvePoint> {
    let target = q.p;
    for _ in 0..8 {
        let (pa, _, sua, sva) = a.at_any(&q.a)?;
        let (pb, _, sub_, svb) = b.at_any(&q.b)?;
        let residual = sub(pa, pb);
        q.p = scale([pa[0] + pb[0], pa[1] + pb[1], pa[2] + pb[2]], 0.5);
        let plane = dot(sub(pa, target), along);
        if norm(residual) <= gap && plane.abs() <= gap.max(1e-300) * 1e3 {
            return Some(q);
        }
        let mut system = System::new(4, 4);
        let rows = system.j.iter_mut().zip(&mut system.r);
        let terms = residual
            .iter()
            .zip(sua.iter().zip(&sva))
            .zip(sub_.iter().zip(&svb));
        for ((row, r), ((&res, (&a_u, &a_v)), (&b_u, &b_v))) in rows.zip(terms) {
            *row = [a_u, a_v, -b_u, -b_v];
            *r = -res;
        }
        system.j[3] = [dot(sua, along), dot(sva, along), 0.0, 0.0];
        system.r[3] = -plane;
        let step = solve_regularized(&system, 0.0)?;
        q.a.u += step[0];
        q.a.v += step[1];
        q.b.u += step[2];
        q.b.v += step[3];
    }
    None
}

/// Every curve with points inserted so that no segment turns more than `max_turn`.
pub fn densify(
    result: &mut Intersection,
    body_a: &[Patch],
    body_b: &[Patch],
    max_turn: f64,
) -> Result<(), Error> {
    let (a, b) = (Pieces::new(body_a)?, Pieces::new(body_b)?);
    for curve in &mut result.curves {
        *curve = densify_curve(curve, &a, &b, max_turn);
    }
    Ok(())
}

fn densify_curve(curve: &Curve, a: &Pieces, b: &Pieces, max_turn: f64) -> Curve {
    let n = curve.points.len();
    let segments = if curve.closed { n } else { n.saturating_sub(1) };
    let mut out = Vec::with_capacity(n);
    for i in 0..segments {
        let (Some(&q0), Some(&q1)) = (curve.points.get(i), curve.points.get((i + 1) % n)) else {
            continue;
        };
        out.push(q0);
        let chord = sub(q1.p, q0.p);
        let length = norm(chord);
        let (Some(t0), Some(t1)) = (tangent(a, b, &q0, chord), tangent(a, b, &q1, chord)) else {
            continue;
        };
        let turn = dot(t0.0, t1.0).clamp(-1.0, 1.0).acos();
        let pieces = (turn / max_turn).ceil() as usize;
        if !(length > 0.0) || !(2..=1000).contains(&pieces) {
            continue;
        }
        // The parameters must run between the ends as their velocities say: not
        // across a seam (u from 360 back to 0) or out of a pole (u undefined
        // there), where interpolating them leaves the surface.
        let follows = |x0: f64, x1: f64, d0: f64, d1: f64| {
            let (actual, predicted) = (x1 - x0, 0.5 * length * (d0 + d1));
            (actual - predicted).abs() <= 0.25 * actual.abs().max(predicted.abs()) + 1e-12
        };
        let consistent = q0.a.patch == q1.a.patch
            && q0.b.patch == q1.b.patch
            && follows(q0.a.u, q1.a.u, t0.1[0], t1.1[0])
            && follows(q0.a.v, q1.a.v, t0.1[1], t1.1[1])
            && follows(q0.b.u, q1.b.u, t0.2[0], t1.2[0])
            && follows(q0.b.v, q1.b.v, t0.2[1], t1.2[1]);
        for k in 1..pieces {
            let s = k as f64 / pieces as f64;
            let (h00, h10, h01, h11) = (
                2.0 * s * s * s - 3.0 * s * s + 1.0,
                s * s * s - 2.0 * s * s + s,
                -2.0 * s * s * s + 3.0 * s * s,
                s * s * s - s * s,
            );
            let mix = |x0: f64, d0: f64, x1: f64, d1: f64| {
                h00 * x0 + h10 * length * d0 + h01 * x1 + h11 * length * d1
            };
            let p = [
                mix(q0.p[0], t0.0[0], q1.p[0], t1.0[0]),
                mix(q0.p[1], t0.0[1], q1.p[1], t1.0[1]),
                mix(q0.p[2], t0.0[2], q1.p[2], t1.0[2]),
            ];
            // Across a seam the parameters cannot be interpolated, but each side
            // of it is a chart: an inserted point starts from the nearer end's
            // parameters, and the polish below places it.
            let starts = if consistent {
                vec![(
                    Param {
                        patch: q0.a.patch,
                        u: mix(q0.a.u, t0.1[0], q1.a.u, t1.1[0]),
                        v: mix(q0.a.v, t0.1[1], q1.a.v, t1.1[1]),
                    },
                    Param {
                        patch: q0.b.patch,
                        u: mix(q0.b.u, t0.2[0], q1.b.u, t1.2[0]),
                        v: mix(q0.b.v, t0.2[1], q1.b.v, t1.2[1]),
                    },
                )]
            } else if s < 0.5 {
                vec![(q0.a, q0.b), (q1.a, q1.b)]
            } else {
                vec![(q1.a, q1.b), (q0.a, q0.b)]
            };
            let direction = unit(sub(q1.p, q0.p));
            let settled = starts.into_iter().find_map(|(pa, pb)| {
                settle(
                    a,
                    b,
                    CurvePoint { p, a: pa, b: pb },
                    direction,
                    1e-9 * length,
                )
                .filter(|q| a.holds(&q.a) && b.holds(&q.b))
            });
            if let Some(point) = settled {
                out.push(point);
            }
        }
    }
    if !curve.closed {
        if let Some(last) = curve.points.last() {
            out.push(*last);
        }
    }
    Curve {
        closed: curve.closed,
        points: out,
    }
}
