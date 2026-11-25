//! Closed forms for pairs of quadrics: where both patches lie on surfaces of a
//! pair solved here, the surfaces' intersection is computed exactly, as a
//! conic, then clipped to the patches' domains. Nothing is traced or
//! subdivided.
//!
//! A plane and a sphere meet in the circle about the foot of the sphere's
//! center, of radius r = √((R − |d|)(R + |d|)) for the center's distance d from
//! the plane. Where R and |d| are close their difference is exact (Sterbenz),
//! so a loop of radius 1e-7 on a unit sphere cut 5e-15 below its pole keeps
//! its radius to rounding, where the field path's g = `f_S`(P), whose minimum is
//! −1e-14 of a field of size 1, reads as rounding there and gives a point
//! (ssi-corpus c7_tiny_loop_sphere_1e-7). d is a compensated dot product, so a
//! tilted plane cuts as exactly.
//!
//! A plane and a B-spline patch whose exact implicit form has degree 2
//! (`algebraic`; fitted here when the patch is of degree 2 at most) meet in the
//! conic of the form restricted to the plane. Where that conic's quadratic part
//! is definite it is an ellipse about its center, and its size comes from the
//! form's value there, one evaluation told from the rounding by the form's own
//! bound: a paraboloid cut 1e-14 above its apex keeps its loop of radius 1e-7
//! (ssi-corpus c7_tiny_loop_parab_1e-7). Other conics are left to the field path.
//!
//! Points of a curve are placed on each patch by inversion (`invert::Side`),
//! and each patch's domain clips it by parameters, never by distance.

use std::f64::consts::TAU;

use crate::algebraic::{Algebraic, Fitter};
use crate::field::Field;
use crate::geom3::{Frame, V3 as Exact3};
use crate::intersect::{Curve, CurvePoint, Param};
use crate::invert::Side;
use crate::patch::Patch;
use crate::piece::{add, cross, dot, norm, scale, sub, Piece, V3};
use crate::surface3::Surf;
use crate::Error;

/// What the closed forms take from the run.
pub(crate) struct Settings {
    /// The gap below which surfaces apart still touch.
    pub contact: f64,
    pub max_step: f64,
    pub max_turn: f64,
}

/// What a pair comes to.
#[derive(Default)]
pub(crate) struct Closed {
    /// Arcs a patch's boundary cut, oriented along nA × nB, to be joined with
    /// the other pairs' arcs.
    pub arcs: Vec<Vec<CurvePoint>>,
    /// Closed curves wholly inside both patches, finished: a loop smaller than
    /// the link would not survive joining (all its points within the link of
    /// its first, it reads as a curve of no length).
    pub loops: Vec<Curve>,
    /// Points where the surfaces touch.
    pub touches: Vec<CurvePoint>,
}

/// Parameter margin still inside a domain, as the field path counts it.
const INSIDE: f64 = -1e-12;

/// The closed form for patches a and b, with their implicit forms `fa` and
/// `fb`, when their surfaces are a pair solved here; None otherwise. `pa` and
/// `pb` are their Bézier pieces.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pair(
    a: &Patch,
    ia: usize,
    fa: Option<Field>,
    pa: &[Piece],
    b: &Patch,
    ib: usize,
    fb: Option<Field>,
    pb: &[Piece],
    settings: &Settings,
) -> Result<Option<Closed>, Error> {
    let meet = match (&a.analytic, &b.analytic) {
        (Some(Surf::Plane { f }), Some(Surf::Sphere { f: s, r }))
        | (Some(Surf::Sphere { f: s, r }), Some(Surf::Plane { f })) => {
            Some(plane_sphere(f, s.o, *r, settings.contact))
        }
        (Some(Surf::Plane { f }), None) => match quadric_form(b, ib, fb) {
            Some(form) => plane_quadric(f, &form, settings.contact),
            None => return Ok(None),
        },
        (None, Some(Surf::Plane { f })) => match quadric_form(a, ia, fa) {
            Some(form) => plane_quadric(f, &form, settings.contact),
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    let Some(meet) = meet else {
        return Ok(None);
    };
    let sides = [
        Side::with_pieces(a, ia, pa.to_vec())?,
        Side::with_pieces(b, ib, pb.to_vec())?,
    ];
    let mut closed = Closed::default();
    match meet {
        Meet::Apart => {}
        Meet::Touch(p) => closed.touches.extend(touch(&sides, p)),
        Meet::Ellipse(c) => clip(&c, &sides, settings, &mut closed),
    }
    Ok(Some(closed))
}

/// The patch's implicit form of degree 2: the one it has, or fitted when the
/// patch is of degree 2 at most and curved (a plane's form is of degree 1).
fn quadric_form(patch: &Patch, index: usize, field: Option<Field>) -> Option<Algebraic> {
    match field {
        Some(Field::Algebraic(form)) => (form.degree == 2).then(|| form.clone()),
        Some(Field::Analytic(_) | Field::Local(..)) => None,
        None => {
            if patch.u_degree.max(patch.v_degree) != 2 {
                return None;
            }
            let mut fitter = Fitter::new(patch, index)?;
            (0..2)
                .find_map(|_| if fitter.open() { fitter.next() } else { None })
                .filter(|form| form.degree == 2)
        }
    }
}

/// How two surfaces meet.
enum Meet {
    Apart,
    Touch(V3),
    Ellipse(Ellipse),
}

/// center + r1 cos t e1 + r2 sin t e2, for unit, orthogonal e1 and e2.
struct Ellipse {
    center: V3,
    e1: V3,
    e2: V3,
    r1: f64,
    r2: f64,
}

impl Ellipse {
    fn at(&self, t: f64) -> V3 {
        let (s, c) = t.sin_cos();
        add(
            self.center,
            add(scale(self.e1, self.r1 * c), scale(self.e2, self.r2 * s)),
        )
    }
}

/// A plane and the sphere of radius r about `center`. Apart by less than
/// `contact`, or crossing by less than the rounding of d, they touch.
fn plane_sphere(plane: &Frame, center: Exact3, r: f64, contact: f64) -> Meet {
    let z = plane.z;
    // c − o split exactly into a difference and its error, both dotted with
    // the normal: d to the rounding of its last operation.
    let parts = [
        two_sum(center.x, -plane.o.x),
        two_sum(center.y, -plane.o.y),
        two_sum(center.z, -plane.o.z),
    ];
    let d = dot2(
        &[
            parts[0].0, parts[1].0, parts[2].0, parts[0].1, parts[1].1, parts[2].1,
        ],
        &[z.x, z.y, z.z, z.x, z.y, z.z],
    );
    let gap = r - d.abs();
    let band = 4.0 * f64::EPSILON * r.max(d.abs());
    if gap < -contact.max(band) {
        return Meet::Apart;
    }
    let foot = center - z * d;
    if gap <= band {
        // Halfway between the plane's point and the sphere's.
        let sphere = center - z * (r * d.signum());
        return Meet::Touch(arr((foot + sphere) * 0.5));
    }
    let radius = (gap * (r + d.abs())).sqrt();
    Meet::Ellipse(Ellipse {
        center: arr(foot),
        e1: arr(plane.x),
        e2: arr(plane.y()),
        r1: radius,
        r2: radius,
    })
}

/// A plane and a form of degree 2, when the form restricted to the plane is
/// definite (an ellipse, a point, or nothing); None for other conics.
///
/// On the plane o + s e1 + t e2 the form is [s t] K [s t]^T + 2 k · (s, t) +
/// q0. Its center (s, t) = −K⁻¹ k is a critical point, so an error there moves
/// the value only to second order, and the value is the form's own at that
/// point: its rounding (16 ulps of its terms' size, for ten terms of degree 2
/// at most) and its error on the patch (`Algebraic::residual`) bound it.
fn plane_quadric(plane: &Frame, form: &Algebraic, contact: f64) -> Option<Meet> {
    let q = form.quadric()?;
    let o = arr(plane.o);
    let (e1, e2) = (arr(plane.x), arr(plane.y()));
    let local = |v: V3| scale(v, 1.0 / q.length);
    let (p0, a, b) = (local(sub(o, q.center)), local(e1), local(e2));
    let mv = |v: V3| q.m.map(|row| dot(row, v));
    let (ma, mb) = (mv(a), mv(b));
    let lin = add(mv(p0), q.w);
    let mut k = [[dot(a, ma), dot(a, mb)], [dot(b, ma), dot(b, mb)]];
    let mut l = [dot(a, lin), dot(b, lin)];
    let det = k[0][0] * k[1][1] - k[0][1] * k[1][0];
    let size = k[0][0].abs().max(k[1][1].abs()).max(k[0][1].abs());
    if !(det > 1e-12 * size * size) {
        return None;
    }
    // K positive definite from here on, the form's sign with it.
    let sign = k[0][0].signum();
    for row in &mut k {
        row[0] *= sign;
        row[1] *= sign;
    }
    l = [l[0] * sign, l[1] * sign];
    let s = -(k[1][1] * l[0] - k[0][1] * l[1]) / det;
    let t = -(k[0][0] * l[1] - k[1][0] * l[0]) / det;
    let center = add(o, add(scale(e1, s), scale(e2, t)));
    let (f, grad, terms) = form.value(center);
    let value = sign * f;
    let band = (16.0 * f64::EPSILON + form.residual) * terms;
    if value > band {
        // Apart, unless by less than `contact` (to first order, the gap is
        // the value over the gradient, normal to the plane at the center).
        let g = norm(grad);
        return Some(if g > 0.0 && value / g <= contact {
            Meet::Touch(center)
        } else {
            Meet::Apart
        });
    }
    if value >= -band {
        let foot = form.foot(center);
        return Some(Meet::Touch(scale(add(center, foot), 0.5)));
    }
    // The axes: K's eigenvectors, at √(−value / λ) along each.
    let half = 0.5 * (k[0][0] - k[1][1]);
    let root = (half * half + k[0][1] * k[1][0]).sqrt();
    let mean = 0.5 * (k[0][0] + k[1][1]);
    // The smaller as det / λ1, which mean − root would cancel.
    let l1 = mean + root;
    let l2 = det / l1;
    let angle = 0.5 * (2.0 * k[0][1]).atan2(k[0][0] - k[1][1]);
    let (sn, cs) = angle.sin_cos();
    Some(Meet::Ellipse(Ellipse {
        center,
        e1: add(scale(e1, cs), scale(e2, sn)),
        e2: add(scale(e1, -sn), scale(e2, cs)),
        r1: (-value / l1).sqrt(),
        r2: (-value / l2).sqrt(),
    }))
}

fn arr(v: Exact3) -> V3 {
    [v.x, v.y, v.z]
}

/// a + b and its rounding error (Knuth's two-sum).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let z = s - a;
    (s, (a - (s - z)) + (b - z))
}

/// Σ `x_i` `y_i` as if in twice the precision, rounded once (Ogita, Rump and
/// Oishi's Dot2, with fused products).
fn dot2(x: &[f64], y: &[f64]) -> f64 {
    let (mut p, mut s) = (0.0_f64, 0.0_f64);
    for (&a, &b) in x.iter().zip(y) {
        let h = a * b;
        let (q, e) = two_sum(p, h);
        p = q;
        s += e + a.mul_add(b, -h);
    }
    p + s
}

/// p's parameters on both patches, and the smaller of their margins inside the
/// domains. Parameters past a collapsed edge that name the same point are
/// clamped onto it.
fn place(
    sides: &[Side; 2],
    p: V3,
    hints: [Option<(f64, f64)>; 2],
) -> Option<([(f64, f64); 2], f64)> {
    let mut uv = [(0.0, 0.0); 2];
    let mut margin = f64::INFINITY;
    for ((side, hint), slot) in sides.iter().zip(hints).zip(&mut uv) {
        let mut q = side.place(p, hint)?;
        let mut m = side.margin(q, 0.0);
        if m < INSIDE {
            if let Some(c) = side.clamped(p, q) {
                (q, m) = (c, 0.0);
            }
        }
        *slot = q;
        margin = margin.min(m);
    }
    Some((uv, margin))
}

fn point(sides: &[Side; 2], p: V3, uv: [(f64, f64); 2]) -> CurvePoint {
    let param = |side: &Side, (u, v): (f64, f64)| Param {
        patch: side.index,
        u,
        v,
    };
    let [side_a, side_b] = sides;
    let [uv_a, uv_b] = uv;
    CurvePoint {
        p,
        a: param(side_a, uv_a),
        b: param(side_b, uv_b),
    }
}

fn hints(q: &CurvePoint) -> [Option<(f64, f64)>; 2] {
    [Some((q.a.u, q.a.v)), Some((q.b.u, q.b.v))]
}

/// The touch point, when it is on both patches.
fn touch(sides: &[Side; 2], p: V3) -> Option<CurvePoint> {
    let (uv, margin) = place(sides, p, [None, None])?;
    (margin >= INSIDE).then(|| point(sides, p, uv))
}

/// The ellipse's arcs inside both patches. Sampled by parameter, at most the
/// run's turn (its sharpest, at the ends of the major axis, is r1 / r2 times
/// the parameter's step) and step apart, and cut where it leaves a domain by
/// bisection between a sample inside and one outside.
fn clip(c: &Ellipse, sides: &[Side; 2], settings: &Settings, out: &mut Closed) {
    let (long, short) = (c.r1.max(c.r2), c.r1.min(c.r2));
    let n = (TAU * (long / short) / settings.max_turn)
        .max(TAU * long / settings.max_step)
        .clamp(16.0, 4096.0)
        .ceil() as usize;
    let t_of = |k: usize| TAU * k as f64 / n as f64;
    let mut hint = [None, None];
    let samples: Vec<Option<CurvePoint>> = (0..n)
        .map(|k| {
            let p = c.at(t_of(k));
            let (uv, margin) = place(sides, p, hint)?;
            hint = [Some(uv[0]), Some(uv[1])];
            (margin >= INSIDE).then(|| point(sides, p, uv))
        })
        .collect();
    let Some(start) = samples.iter().position(Option::is_none) else {
        let mut points: Vec<CurvePoint> = samples.into_iter().flatten().collect();
        orient(sides, &mut points);
        out.loops.push(Curve {
            closed: true,
            points,
        });
        return;
    };
    // Once around from a sample outside: each run of samples inside is an arc.
    let mut arc: Vec<CurvePoint> = Vec::new();
    for k in start + 1..=start + n {
        let (t0, t1) = (t_of(k - 1), t_of(k));
        let (Some(&before), Some(&here)) = (samples.get((k - 1) % n), samples.get(k % n)) else {
            continue;
        };
        match (before, here) {
            (None, Some(q)) => {
                arc.extend(boundary(c, sides, t1, t0, hints(&q)));
                arc.push(q);
            }
            (Some(_), Some(q)) => arc.push(q),
            (Some(q), None) => {
                arc.extend(boundary(c, sides, t0, t1, hints(&q)));
                let mut done = std::mem::take(&mut arc);
                if done.len() > 1 {
                    orient(sides, &mut done);
                    out.arcs.push(done);
                }
            }
            (None, None) => {}
        }
    }
}

/// The point where the ellipse leaves the domains between parameter `inside`
/// and parameter `outside`, on the inside.
fn boundary(
    c: &Ellipse,
    sides: &[Side; 2],
    mut inside: f64,
    mut outside: f64,
    hint: [Option<(f64, f64)>; 2],
) -> Option<CurvePoint> {
    for _ in 0..60 {
        let mid = 0.5 * (inside + outside);
        if place(sides, c.at(mid), hint).is_some_and(|(_, m)| m >= INSIDE) {
            inside = mid;
        } else {
            outside = mid;
        }
    }
    let p = c.at(inside);
    let (uv, _) = place(sides, p, hint)?;
    Some(point(sides, p, uv))
}

/// Points reversed where they run against nA × nB, summed along them.
fn orient(sides: &[Side; 2], points: &mut [CurvePoint]) {
    let [side_a, side_b] = sides;
    let along: f64 = points
        .windows(2)
        .map(|w| match w {
            [x, y] => {
                let (na, nb) = (side_a.at(x.a.u, x.a.v).1, side_b.at(x.b.u, x.b.v).1);
                dot(cross(na, nb), sub(y.p, x.p))
            }
            _ => 0.0,
        })
        .sum();
    if along < 0.0 {
        points.reverse();
    }
}
