//! The exact surface of a constant round whose spine is a plane conic (opt-in, not
//! upstream): the spine as rational quadratic segments lying on both offsets, the contacts
//! and the ball's arc between them as Bernstein polynomials in the spine's parameter, and
//! the round as the rational B-spline surface they make.
//!
//! The nodal quartic of two offsets touching at one point is swept the same way, as
//! rational quartic segments.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmFilletExact.cpp`
//! and `source/SMLib/inc/SmFilletSolver.h` (tangency-labs/smlib@0061c41, with the conic-pair
//! dispatch of tangency-labs/smlib@1b5f1e7 and the nodal quartic and conic span direction
//! of tangency-labs/smlib@dda3b5d), behind
//! [`UpstreamFixes::fillet_exact_round_classes`](crate::kernel::UpstreamFixes::fillet_exact_round_classes)
//! (WR-669).
//!
//! | Rust                                   | Fork                                              |
//! |----------------------------------------|---------------------------------------------------|
//! | [`calc_rational_spine_fillet_geom`]    | `SmFilletSolver::CalcRationalSpineFilletGeom`     |
//! | `Poly`, `VPoly`, `Segment` (private)   | `sm_Poly`, `sm_VPoly`, `sm_Segment`               |
//! | `binom`, `elevate`, `mul`, `add`, `scale`, `eval`, `split` (private) | `sm_Binom`, `sm_Elevate`, `sm_Mul`, `sm_Add`, `sm_Scale`, `sm_Eval`, `sm_Split` |
//! | `v_const`, `v_add`, `v_mul`, `v_scale`, `dot`, `dot_const`, `v_eval` (private) | `sm_VConst`, `sm_VAdd`, `sm_VMul`, `sm_VScale`, `sm_Dot`, `sm_DotConst`, `sm_VEval` |
//! | `Contact`, `contact_of` (private)      | `sm_Contact`, `sm_ContactOf`                      |
//! | `round_rows` (private)                 | `sm_RoundRows`                                    |
//! | `ConicKind`, `Conic`, `plane_section`, `conic_point`, `conic_param`, `conic_segment` (private) | `sm_ConicKind`, `sm_Conic`, `sm_PlaneSection`, `sm_ConicPoint`, `sm_ConicParam`, `sm_ConicSegment` |
//! | `nodal_quartic`, `NodalArc` (private)  | `sm_NodalQuartic` (its out-params)                |
//! | `bil` (private)                       | `sm_NodalQuartic`'s `bil` lambda                  |
//!
//! Failures (#15034): [`BlendFailure::SectionNotConic`],
//! [`BlendFailure::NodalArcUnavailable`], [`BlendFailure::CurveProjectionEmpty`],
//! [`BlendFailure::RationalWeightsUnavailable`] and
//! [`BlendFailure::RationalRoundOutOfTolerance`] carry their measurements;
//! [`BlendFailure::InputMissing`] names caller inputs. Defects explain a zero nodal
//! gradient excluded by classification, the
//! rejection of a pure cross-term tangent cone with two real branches, or a successful
//! order-1 evaluation missing its promised derivative. Impossible checks are deleted:
//! branch choices have two values, builders supply nonempty positive-degree rows,
//! segment spans iterate consecutive knots, and the two rails are built as an array.
//!
//! Deviations:
//! - **A chamfer's two offsets (not the fork, #2950).** `round_rows` takes the spine's offset
//!   from each face: the fork takes one, so an unequal chamfer's rail missed its face.
//! - **Out-params.** `sm_NodalQuartic` returns its construction and branch tangents.
//!   The polynomial helpers return their results; `sm_RoundRows` returns
//!   its rows.
//! - **Storing.** The surface, rails and spine are built first and stored together; the
//!   fork stores the surface and rails before it builds the spine, so a failure of the
//!   spine's `CreateCanonical` leaves them stored before the error propagates (the
//!   fillet operation fails either way).
//! - **Segment count bounded.** A non-finite span, or one needing more than
//!   [`MAX_SPINE_SEGMENTS`] segments, is `SM_ERR` (the fork's `ULONG` conversion of a
//!   non-finite count is undefined and a huge one exhausts memory); geometry never gets
//!   near it (a hyperbola's parameter is an `asinh`).
//! - **Where the edge's middle lies on an ellipse (not the fork's, #18824).** The spine
//!   runs from the trace's first centre to its last the way round an ellipse that passes the
//!   edge's middle. The fork read the middle off the ellipse by its angle about the
//!   ellipse's centre, in the ellipse's plane; here it is read by its angle round the
//!   cylinder's or cone's axis (`ellipse_param_about_axis`), which the edge and the spine
//!   share. A chamfer whose ball nearly fills a 1 mm round has a spine ellipse 0.07 mm
//!   across, its plane 0.97 mm off the edge along a tilted axis: the fork's reading put the
//!   middle outside the short arc, and the round ran the long way round, its rails off
//!   their faces.
//! - No recursion: the weight-positivity halving is the fork's work list (left half
//!   first), at most 6 levels deep, as the fork bounds it.
//! - **Nodal arithmetic.** The nodal quartic is constructed directly in Bernstein form
//!   on each segment, from the cone direction's symmetric bilinear polar form. Its
//!   homogeneous quartic is never expanded in powers and converted back: that lost
//!   precision by cancellation near the last end. This changes its numerics for
//!   correctness; the rational construction and its degree are unchanged.
//! - **Conic arithmetic.** The quadric is restricted in the cutting plane's local
//!   principal frame. Its coefficients and centred constant follow directly from
//!   the rotational quadratic form, avoiding cancellation on shallow cones.

use std::collections::VecDeque;

use super::{BlendQuadric, BlendSpine, ExactBlend, QuadricKind};
use crate::blend::fillet_geom::p1::{FilletBrep, FilletGeomId};
use crate::blend::fillet_solver::p1::{FilletSolverOps, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p2::{eval_offset, store_parts, FilletGeomParts};
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::Curve;
use crate::error::blend::{
    BlendFailure, BlendInput, NodalArc as NodalArcReason, SectionShape, WeightStep,
};
use crate::error::{Result, SolidError};
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::vector2d::Point2;
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{max2, min2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::{edge, Brep};

const OP: &str = "blend::fillet_exact::rational";

/// The most spine segments a conic is cut into before the weights are made positive.
pub const MAX_SPINE_SEGMENTS: f64 = 1.0e5;

/// `sm_Poly`: a polynomial by its Bernstein coefficients on `[0, 1]`.
type Poly = Vec<f64>;

/// `sm_VPoly`: three polynomials, a vector's coordinates.
#[derive(Debug, Clone, Default)]
struct VPoly {
    a: [Poly; 3],
}

/// `sm_Segment`: a rational Bezier segment, homogeneous `(M, w)`.
#[derive(Debug, Clone, Default)]
struct Segment {
    m: VPoly,
    w: Poly,
}

/// Coefficient `i` (the fork's `[i]`, always in range where it reads).
fn at(p: &[f64], i: usize) -> f64 {
    p.get(i).copied().unwrap_or(0.0)
}

/// Sets coefficient `i` (the fork's `[i] =`).
fn put(p: &mut [f64], i: usize, v: f64) {
    if let Some(x) = p.get_mut(i) {
        *x = v;
    }
}

/// `x > 0.0` (`!positive` is the fork's `!( x > 0.0 )`, true for NaN).
fn positive(x: f64) -> bool {
    x.partial_cmp(&0.0) == Some(std::cmp::Ordering::Greater)
}

/// The degree of `p` (`size() - 1`, `0` for none).
fn degree(p: &[f64]) -> usize {
    p.len().saturating_sub(1)
}

/// `sm_Binom`.
fn binom(n: usize, k: usize) -> f64 {
    let mut d = 1.0;
    for i in 1..=k {
        // n - k + i as the fork's int arithmetic (k <= n here)
        let f = (n + i).saturating_sub(k) as f64;
        d = d * f / i as f64;
    }
    d
}

/// `sm_Elevate`: `p` degree-elevated to `to`.
fn elevate(p: &[f64], to: usize) -> Poly {
    let mut sp: Poly = p.to_vec();
    while !sp.is_empty() && degree(&sp) < to {
        let n = degree(&sp);
        let mut q = vec![0.0; n + 2];
        put(&mut q, 0, at(&sp, 0));
        put(&mut q, n + 1, at(&sp, n));
        for i in 1..=n {
            let f = i as f64 / (n + 1) as f64;
            put(&mut q, i, f * at(&sp, i - 1) + (1.0 - f) * at(&sp, i));
        }
        sp = q;
    }
    sp
}

/// `sm_Mul`.
fn mul(a: &[f64], b: &[f64]) -> Poly {
    let (m, n) = (degree(a), degree(b));
    let mut c = vec![0.0; m + n + 1];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            if let Some(cij) = c.get_mut(i + j) {
                *cij += binom(m, i) * binom(n, j) / binom(m + n, i + j) * ai * bj;
            }
        }
    }
    c
}

/// `sm_Add(crA, crB, dB)`: `a + db b`.
fn add(a: &[f64], b: &[f64], db: f64) -> Poly {
    let n = a.len().max(b.len()).saturating_sub(1);
    let mut sa = elevate(a, n);
    let sb = elevate(b, n);
    for (x, &y) in sa.iter_mut().zip(sb.iter()) {
        *x += db * y;
    }
    sa
}

/// `sm_Scale`.
fn scale(a: &[f64], d: f64) -> Poly {
    a.iter().map(|&x| x * d).collect()
}

/// `sm_Eval`: de Casteljau at `t`.
fn eval(p: &[f64], t: f64) -> f64 {
    let mut s: Poly = p.to_vec();
    let len = s.len();
    for k in 1..len {
        for i in 0..len - k {
            let v = (1.0 - t) * at(&s, i) + t * at(&s, i + 1);
            put(&mut s, i, v);
        }
    }
    at(&s, 0)
}

/// `sm_Split`: the halves of a polynomial (de Casteljau at 1/2).
fn split(p: &[f64]) -> (Poly, Poly) {
    let n = p.len();
    let mut s: Poly = p.to_vec();
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    if n == 0 {
        return (l, r);
    }
    put(&mut l, 0, at(&s, 0));
    put(&mut r, n - 1, at(&s, n - 1));
    for k in 1..n {
        for i in 0..n - k {
            let v = 0.5 * (at(&s, i) + at(&s, i + 1));
            put(&mut s, i, v);
        }
        put(&mut l, k, at(&s, 0));
        put(&mut r, n - 1 - k, at(&s, n - 1 - k));
    }
    (l, r)
}

/// `sm_Const`.
fn konst(d: f64) -> Poly {
    vec![d]
}

/// `sm_VConst`.
fn v_const(v: Vec3) -> VPoly {
    VPoly {
        a: [konst(v.x), konst(v.y), konst(v.z)],
    }
}

/// `sm_VAdd(crA, crB, dB)`.
fn v_add(a: &VPoly, b: &VPoly, db: f64) -> VPoly {
    let [a0, a1, a2] = &a.a;
    let [b0, b1, b2] = &b.a;
    VPoly {
        a: [add(a0, b0, db), add(a1, b1, db), add(a2, b2, db)],
    }
}

/// `sm_VMul`.
fn v_mul(a: &VPoly, b: &[f64]) -> VPoly {
    let [a0, a1, a2] = &a.a;
    VPoly {
        a: [mul(a0, b), mul(a1, b), mul(a2, b)],
    }
}

/// `sm_VScale`.
fn v_scale(a: &VPoly, d: f64) -> VPoly {
    let [a0, a1, a2] = &a.a;
    VPoly {
        a: [scale(a0, d), scale(a1, d), scale(a2, d)],
    }
}

/// `sm_Dot`.
fn dot(a: &VPoly, b: &VPoly) -> Poly {
    let [a0, a1, a2] = &a.a;
    let [b0, b1, b2] = &b.a;
    add(&add(&mul(a0, b0), &mul(a1, b1), 1.0), &mul(a2, b2), 1.0)
}

/// `sm_DotConst`.
fn dot_const(a: &VPoly, v: Vec3) -> Poly {
    let [a0, a1, a2] = &a.a;
    add(
        &add(&scale(a0, v.x), &scale(a1, v.y), 1.0),
        &scale(a2, v.z),
        1.0,
    )
}

/// `sm_VEval`.
fn v_eval(a: &VPoly, t: f64) -> Point3 {
    let [a0, a1, a2] = &a.a;
    Point3::new(eval(a0, t), eval(a1, t), eval(a2, t))
}

/// `sm_Contact`: the contact of the ball with one face along a spine segment, the unit
/// direction from the centre to it `E / D`. `bw`: `D` is the spine's own weight (or 1).
#[derive(Debug, Clone)]
struct Contact {
    e: VPoly,
    d: Poly,
    bw: bool,
    plane: bool,
}

/// `sm_ContactOf(crO, dContact, crS)`.
fn contact_of(o: &BlendQuadric, contact: f64, s: &Segment) -> Contact {
    let ds = contact * o.side;
    // V = M - p w
    let v = v_add(&s.m, &v_mul(&v_const(o.point), &s.w), -1.0);
    match o.kind {
        QuadricKind::Plane => Contact {
            e: v_const(o.dir * contact),
            d: konst(1.0),
            bw: true,
            plane: true,
        },
        QuadricKind::Sphere => Contact {
            e: v_scale(&v, ds / o.radius),
            d: s.w.clone(),
            bw: true,
            plane: false,
        },
        QuadricKind::Cylinder => {
            let h = dot_const(&v, o.dir);
            let r = v_add(&v, &v_mul(&v_const(o.dir), &h), -1.0);
            Contact {
                e: v_scale(&r, ds / o.radius),
                d: s.w.clone(),
                bw: true,
                plane: false,
            }
        }
        // SM_BQ_CONE (the fork's default): n = ((V - H d) cos^2 a - H d sin^2 a) / (H sin a)
        _ => {
            let (c, sn) = (o.angle.cos(), o.angle.sin());
            let h = dot_const(&v, o.dir);
            let r = v_add(&v, &v_mul(&v_const(o.dir), &h), -1.0);
            let n = v_add(&v_scale(&r, c * c), &v_mul(&v_const(o.dir), &h), -sn * sn);
            Contact {
                e: v_scale(&n, ds),
                d: scale(&h, sn),
                bw: false,
                plane: false,
            }
        }
    }
}

/// `sm_RoundRows(crA, crB, crS, dR, bArc, rX, rW)`: the homogeneous rows `(X, W)` of the
/// round over one spine segment from contact `a` to contact `b`: three for the ball's arc
/// (the rational quadratic through the meeting point of its end tangents, weights
/// `2 G L`, `G L`, `Da Db L` with `G = Da Db (1 + cos)`), two for a chamfer's line. Face
/// `a` is a plane, sphere or cylinder when either is. `radii` are the spine's offsets from
/// `a` and `b`: the ball's radius twice for an arc; a chamfer's two (the fork takes one).
fn round_rows(
    a: &Contact,
    b: &Contact,
    s: &Segment,
    radii: [f64; 2],
    arc: bool,
) -> (Vec<VPoly>, Vec<Poly>) {
    let [ra, rb] = radii;
    let mut rx = Vec::new();
    let mut rw = Vec::new();
    // contact numerators: A over w (bW) or w Da
    let num = |c: &Contact, r: f64| {
        if c.plane {
            v_add(&s.m, &v_mul(&c.e, &s.w), r)
        } else if c.bw {
            v_add(&s.m, &c.e, r)
        } else {
            v_add(&v_mul(&s.m, &c.d), &v_mul(&c.e, &s.w), r)
        }
    };
    let na = num(a, ra);
    let nb = num(b, rb);
    if !arc {
        // a chamfer: the line from contact to contact, each row over its own denominator
        rx.push(na);
        rw.push(if a.bw { s.w.clone() } else { mul(&s.w, &a.d) });
        rx.push(nb);
        rw.push(if b.bw { s.w.clone() } else { mul(&s.w, &b.d) });
        return (rx, rw);
    }
    let dadb = mul(&a.d, &b.d);
    let g = add(&dadb, &dot(&a.e, &b.e), 1.0);
    // r w (Ea Db + Eb Da), the middle row's offset from G M
    let mid = v_mul(
        &v_add(&v_mul(&a.e, &b.d), &v_mul(&b.e, &a.d), 1.0),
        &scale(&s.w, ra),
    );
    if a.bw {
        // L = w
        rx.push(v_scale(&v_mul(&na, &g), 2.0));
        rw.push(scale(&mul(&g, &s.w), 2.0));
        rx.push(v_add(&v_mul(&s.m, &g), &mid, 1.0));
        rw.push(mul(&g, &s.w));
        if b.bw {
            rx.push(v_mul(&nb, &dadb));
        } else {
            rx.push(v_mul(&nb, &a.d));
        }
        rw.push(mul(&dadb, &s.w));
    } else {
        // both cones: L = w Da
        rx.push(v_scale(&v_mul(&na, &g), 2.0));
        rw.push(scale(&mul(&mul(&g, &s.w), &a.d), 2.0));
        rx.push(v_mul(&v_add(&v_mul(&s.m, &g), &mid, 1.0), &a.d));
        rw.push(mul(&mul(&g, &s.w), &a.d));
        let da2 = mul(&a.d, &a.d);
        rx.push(v_mul(&nb, &da2));
        rw.push(mul(&mul(&da2, &b.d), &s.w));
    }
    (rx, rw)
}

/// `sm_ConicKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConicKind {
    Ellipse,
    Hyperbola,
    Parabola,
}

/// `sm_Conic`: ellipse `C + cos t A1 + sin t A2`, hyperbola `C + s (cosh t A1 + sinh t A2)`
/// on the branch `s = +-1`, parabola `C + t A2 + t^2 A1`.
#[derive(Debug, Clone, Copy)]
struct Conic {
    kind: ConicKind,
    c: Point3,
    a1: Vec3,
    a2: Vec3,
    branch: f64,
}

/// `sm_PlaneSection(crPoint, crNormal, crQ, rConic)`: the conic in which the plane through
/// `point` with unit `normal` meets the quadric `q`.
///
/// In the principal frame, with `p = q.point - point`, the restriction is
/// `R (x-pu)^2 + L (y-pv)^2 - 2 B pn (y-pv) + N pn^2 - k0`.
/// Here `L = R t² + H s²`, `B = (H-R) st`, `N = R s² + H t²`,
/// and `s²+t² = 1`. Thus `L N - B² = R H`: completing the square gives
/// the centred constant `R H pn²/L - k0` directly, without a matrix eigensolve.
#[allow(clippy::many_single_char_names)]
fn plane_section(point: Point3, normal: Vec3, q: &BlendQuadric) -> Result<Conic> {
    // A rotational quadric has radial coefficient R and axial coefficient H.
    // Use the cutting plane's principal directions: u perpendicular to the
    // quadric's axis, v in the plane containing that axis and the plane normal.
    // Its restriction is diagonal in this frame, even on a shallow cone.
    let normal = normal.unitize()?;
    let (axis, radial, axial, k0) = match q.kind {
        QuadricKind::Sphere => (normal, 1.0, 1.0, q.radius * q.radius),
        QuadricKind::Cylinder => (q.dir.unitize()?, 1.0, 0.0, q.radius * q.radius),
        QuadricKind::Cone => {
            let (sin, cos) = q.angle.sin_cos();
            (q.dir.unitize()?, cos * cos, -sin * sin, 0.0)
        }
        _ => {
            return Err(SolidError::blend(
                OP,
                BlendFailure::SectionNotConic {
                    reason: SectionShape::NoQuadraticForm,
                    quadratic_scale: 0.0,
                    discriminant: None,
                    centered_value: None,
                },
            ))
        }
    };
    let cross = normal.cross(axis);
    let u = if cross == Vec3::ZERO {
        // A plane square to the axis cuts a circle: either in-plane direction
        // is principal. Choose the coordinate least aligned with its normal.
        let seed = if normal.x.abs() <= normal.y.abs() && normal.x.abs() <= normal.z.abs() {
            Vec3::new(1.0, 0.0, 0.0)
        } else if normal.y.abs() <= normal.z.abs() {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        };
        normal.cross(seed).unitize()?
    } else {
        cross
            .length_and_unit()
            .ok_or_else(|| {
                SolidError::blend(
                    OP,
                    BlendFailure::SectionNotConic {
                        reason: SectionShape::NoQuadraticForm,
                        quadratic_scale: 0.0,
                        discriminant: None,
                        centered_value: None,
                    },
                )
            })?
            .1
    };
    let v = normal.cross(u).unitize()?;
    let (s, t) = (axis.dot(v), axis.dot(normal));
    let (l0, l1) = (radial, radial * t * t + axial * s * s);
    let coupling = (axial - radial) * s * t;
    let p = q.point - point;
    let (pu, pv, pn) = (p.dot(u), p.dot(v), p.dot(normal));
    let dscale = max2(l0.abs(), l1.abs());
    if dscale <= 0.0 || l0 <= EFF_ZERO * dscale {
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionNotConic {
                reason: SectionShape::NoQuadraticForm,
                quadratic_scale: dscale,
                discriminant: None,
                centered_value: None,
            },
        ));
    }
    if l1.abs() <= EFF_ZERO * dscale {
        // F = R (x-pu)^2 - 2 B pn (y-pv) + N pn^2 - k0.
        // A zero linear term gives lines, not a parabola.
        let linear = coupling * pn;
        if linear.abs() <= EFF_ZERO * dscale {
            // R x^2 + (N pn^2 - k0) = 0: no real roots, or one/two lines.
            let constant = (radial * s * s + axial * t * t) * pn * pn - k0;
            let discriminant = -4.0 * radial * constant;
            return Err(SolidError::blend(
                OP,
                BlendFailure::SectionNotConic {
                    reason: if discriminant < 0.0 {
                        SectionShape::Empty
                    } else {
                        SectionShape::Lines
                    },
                    quadratic_scale: dscale,
                    discriminant: Some(discriminant),
                    centered_value: None,
                },
            ));
        }
        let normal_coefficient = radial * s * s + axial * t * t;
        let vertex = pv + (normal_coefficient * pn * pn - k0) / (2.0 * linear);
        return Ok(Conic {
            kind: ConicKind::Parabola,
            c: point + u * pu + v * vertex,
            a1: v * (radial / (2.0 * linear)),
            a2: u,
            branch: 1.0,
        });
    }
    let c = point + u * pu + v * (pv + coupling * pn / l1);
    // The Schur complement of the v coefficient is R H / l1. Read the
    // centred constant from it, without subtracting squared apex coordinates
    // or subtracting the distant centre from the apex in world coordinates.
    let q0 = radial * axial * pn * pn / l1 - k0;
    let (d0, d1) = (-q0 / l0, -q0 / l1);
    let (kind, a1, a2) = if d0 > 0.0 && d1 > 0.0 {
        (ConicKind::Ellipse, u * d0.sqrt(), v * d1.sqrt())
    } else if d0 > 0.0 {
        (ConicKind::Hyperbola, u * d0.sqrt(), v * (-d1).sqrt())
    } else if d1 > 0.0 {
        (ConicKind::Hyperbola, v * d1.sqrt(), u * (-d0).sqrt())
    } else {
        // At q0 = 0 a definite form is a point; an indefinite form is two lines.
        let reason = if q0 == 0.0 {
            if (l0 > 0.0) == (l1 > 0.0) {
                SectionShape::Point
            } else {
                SectionShape::Lines
            }
        } else {
            SectionShape::Empty
        };
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionNotConic {
                reason,
                quadratic_scale: dscale,
                discriminant: None,
                centered_value: Some(q0),
            },
        ));
    };
    Ok(Conic {
        kind,
        c,
        a1,
        a2,
        branch: 1.0,
    })
}

/// `sm_ConicPoint`.
fn conic_point(k: &Conic, t: f64) -> Point3 {
    match k.kind {
        ConicKind::Ellipse => k.c + k.a1 * t.cos() + k.a2 * t.sin(),
        ConicKind::Hyperbola => k.c + (k.a1 * t.cosh() + k.a2 * t.sinh()) * k.branch,
        ConicKind::Parabola => k.c + k.a2 * t + k.a1 * (t * t),
    }
}

/// `sm_ConicParam`: the conic's parameter of a point on it (or near it).
fn conic_param(k: &Conic, x: Point3) -> f64 {
    let d = x - k.c;
    let px = d.dot(k.a1) / k.a1.dot(k.a1);
    let py = d.dot(k.a2) / k.a2.dot(k.a2);
    match k.kind {
        ConicKind::Ellipse => py.atan2(px),
        ConicKind::Hyperbola => (py * k.branch).asinh(),
        ConicKind::Parabola => py,
    }
}

/// The parameter of an ellipse `k` on the plane through `at` with unit normal `normal`, a
/// section of `q`, the offset of `face`, that `x` stands at round `q`'s axis: `x` moved
/// along a cylinder's or cone's axis onto the plane, and the conic's point on the half-line
/// to it from where the axis crosses the plane (both keep the angle about the axis, which
/// the conic's points, the faces' and their edges' share). A cylinder offset past its axis
/// (a ball larger than a convex face's radius) lies the other way round it, its `side`
/// turned: there the ball's centre is half a turn from its contact, and `x` is read on the
/// half-line the other way. Not the fork's, which read `x` off the conic about
/// its centre: a point of the edge, far from a small conic against its size, reads there at
/// an angle that has nothing to do with where it lies round the axis. Of another quadric,
/// and of an axis along the plane, `x` is read about the conic's centre
/// ([`conic_param`]).
///
/// # Errors
/// [`BlendFailure::ExactBlendAxisCannotPlaceEdge`], a check of the caller's input with status
/// `SM_ERR`, when the half-line has no direction (`x` on the axis) or does not cross the
/// conic ahead of the axis (the axis crosses the plane outside it, which a plane section of
/// the axis's own cylinder or cone never does): the exact blend given is then not the one of
/// the edge, the way round is unknown, and is not read about the conic's centre instead.
fn ellipse_param_about_axis(
    k: &Conic,
    at: Point3,
    normal: Vec3,
    q: &BlendQuadric,
    face: &BlendQuadric,
    x: Point3,
) -> Result<f64> {
    let along = q.dir.dot(normal);
    if !matches!(q.kind, QuadricKind::Cylinder | QuadricKind::Cone) || along.abs() <= EFF_ZERO {
        return Ok(conic_param(k, x));
    }
    let axis_at = q.point + q.dir * ((at - q.point).dot(normal) / along);
    let moved = x - q.dir * ((x - at).dot(normal) / along);
    // in the ellipse's own coordinates, where it is the unit circle
    let coords = |p: Point3| {
        let d = p - k.c;
        (d.dot(k.a1) / k.a1.dot(k.a1), d.dot(k.a2) / k.a2.dot(k.a2))
    };
    let (a0, a1) = coords(axis_at);
    let (m0, m1) = coords(moved);
    let turned = q.kind == QuadricKind::Cylinder
        && face.kind == QuadricKind::Cylinder
        && q.side != face.side;
    let way = if turned { -1.0 } else { 1.0 };
    let (w0, w1) = (way * (m0 - a0), way * (m1 - a1));
    let ww = w0 * w0 + w1 * w1;
    let aw = a0 * w0 + a1 * w1;
    let disc = aw * aw - ww * (a0 * a0 + a1 * a1 - 1.0);
    // in the conic's unit-circle coordinates, a half-line shorter than the kernel's zero
    // has no direction
    let length = ww.sqrt();
    if length <= EFF_ZERO || disc < 0.0 {
        return Err(SolidError::blend(
            OP,
            BlendFailure::ExactBlendAxisCannotPlaceEdge {
                length,
                discriminant: disc,
                crossing: None,
            },
        ));
    }
    // the half-line's own crossing, ahead of the axis
    let mu = (disc.sqrt() - aw) / ww;
    if !positive(mu) {
        return Err(SolidError::blend(
            OP,
            BlendFailure::ExactBlendAxisCannotPlaceEdge {
                length,
                discriminant: disc,
                crossing: Some(mu),
            },
        ));
    }
    Ok((a1 + mu * w1).atan2(a0 + mu * w0))
}

/// `sm_ConicSegment`: one rational quadratic segment of the conic from `t0` to `t1`.
fn conic_segment(k: &Conic, t0: f64, t1: f64) -> Segment {
    let p0 = conic_point(k, t0);
    let p2 = conic_point(k, t1);
    let tm = 0.5 * (t0 + t1);
    let h = 0.5 * (t1 - t0);
    let mut w = 1.0;
    let p1 = match k.kind {
        ConicKind::Ellipse => {
            w = h.cos();
            k.c + (k.a1 * tm.cos() + k.a2 * tm.sin()) * (1.0 / w)
        }
        ConicKind::Hyperbola => {
            w = h.cosh();
            k.c + (k.a1 * tm.cosh() + k.a2 * tm.sinh()) * (k.branch / w)
        }
        // P(t0) + (t1 - t0)/2 P'(t0)
        ConicKind::Parabola => p0 + (k.a2 + k.a1 * (2.0 * t0)) * h,
    };
    let ap = [p0, p1 * w, p2];
    Segment {
        m: VPoly {
            a: [
                ap.iter().map(|p| p.x).collect(),
                ap.iter().map(|p| p.y).collect(),
                ap.iter().map(|p| p.z).collect(),
            ],
        },
        w: vec![1.0, w, 1.0],
    }
}

/// `x^T M y`, summed row by row as the fork's `bil`.
fn bil(m: &[[f64; 3]; 3], x: &[f64; 3], y: &[f64; 3]) -> f64 {
    let mut s = 0.0;
    for (xi, row) in x.iter().zip(m) {
        for (mij, yj) in row.iter().zip(y) {
            s += xi * mij * yj;
        }
    }
    s
}

/// A nodal quartic's rational cone construction and both branch tangents at its node.
struct NodalArc {
    quadric: [[f64; 3]; 3],
    cone: [[f64; 3]; 3],
    node: Point3,
    gradient: Vec3,
    ray: Vec3,
    from: Vec3,
    to: Vec3,
    branch: [Vec3; 2],
}

impl NodalArc {
    /// The line through the base ray in the cone's projective plane.
    fn direction(&self, t: f64) -> Vec3 {
        self.from * (1.0 - t) + self.to * t
    }

    /// The symmetric bilinear polar form of
    /// `V(w) = (w.C.w) ray - 2 (ray.C.w) w`, so `V(w)` lies on the cone.
    fn polar(&self, u: Vec3, v: Vec3) -> Vec3 {
        let r = [self.ray.x, self.ray.y, self.ray.z];
        let ua = [u.x, u.y, u.z];
        let va = [v.x, v.y, v.z];
        self.ray * bil(&self.cone, &ua, &va)
            - v * bil(&self.cone, &r, &ua)
            - u * bil(&self.cone, &r, &va)
    }

    /// `G = gradient.V` and `W = V.A.V`, evaluated without power coefficients.
    fn gw(&self, t: f64) -> (f64, f64) {
        let w = self.direction(t);
        let v = self.polar(w, w);
        let a = [v.x, v.y, v.z];
        (self.gradient.dot(v), bil(&self.quadric, &a, &a))
    }

    /// The exact rational quartic on `[a, b]`, constructed in its own Bernstein basis.
    /// The quadratic direction has controls `polar(u,u), polar(u,v), polar(v,v)`;
    /// Bernstein products give `W = V.A.V` and `M = node W - 2 (gradient.V) V`.
    fn segment(&self, a: f64, b: f64) -> Segment {
        let (u, v) = (self.direction(a), self.direction(b));
        let controls = [self.polar(u, u), self.polar(u, v), self.polar(v, v)];
        let direction = VPoly {
            a: [
                controls.iter().map(|p| p.x).collect(),
                controls.iter().map(|p| p.y).collect(),
                controls.iter().map(|p| p.z).collect(),
            ],
        };
        let g = dot_const(&direction, self.gradient);
        let mut w = konst(0.0);
        for (vi, row) in direction.a.iter().zip(&self.quadric) {
            for (vj, &aij) in direction.a.iter().zip(row) {
                w = add(&w, &mul(vi, vj), aij);
            }
        }
        Segment {
            m: v_add(
                &v_mul(&v_const(self.node), &w),
                &v_mul(&direction, &g),
                -2.0,
            ),
            w,
        }
    }
    /// The builder's 16 nodal segments, with their homogeneous sign chosen at the midpoint.
    fn segments(&self, s0: f64, s1: f64) -> Vec<Segment> {
        let mut segs = Vec::new();
        let wsign = if self.gw(0.5).1 > 0.0 { 1.0 } else { -1.0 };
        let n = 16;
        for i in 0..n {
            let a = s0 + (s1 - s0) * f64::from(i) / f64::from(n);
            let bb = s0 + (s1 - s0) * f64::from(i + 1) / f64::from(n);
            let mut sg = self.segment(a, bb);
            sg.m = v_scale(&sg.m, wsign);
            sg.w = scale(&sg.w, wsign);
            segs.push(sg);
        }
        segs
    }
}

/// `sm_NodalQuartic(crO1, crO2, crNode, crFrom, crTo, iBase, aBranch, aX, rW, rG)`: the
/// nodal quartic two offsets meet in, through `node` where they touch, from the direction
/// `from` (from the node) to `to`: the second point of the line from the node along each
/// direction of the cone of directions the two quadrics share there. `second_base` picks
/// the second branch as the sweep's base ray instead of the first.
///
/// `Q1(q + s v) = 2 s g1.v + s^2 v.A1.v` and `Q2` likewise with `g2 = k g1` at the node:
/// they meet again where `v.(A2 - k A1).v = 0`, a cone, swept rationally by the second
/// point on it of the lines through one of its rays `v0` (a branch's tangent there) in
/// the directions `w(s) = (1 - s) from + s to`; the point of the quartic along `v` is
/// `q - 2 (g1.v) / (v.A1.v) v`.
fn nodal_quartic(
    o1: &BlendQuadric,
    o2: &BlendQuadric,
    node: Point3,
    from: Vec3,
    to: Vec3,
    second_base: bool,
) -> Result<NodalArc> {
    let (a1, b1, _c1) = o1.implicit();
    let (a2, b2, _c2) = o2.implicit();
    let q = [node.x, node.y, node.z];
    let mut g1 = [0.0; 3];
    let mut g2 = [0.0; 3];
    for (((g1i, g2i), (r1, r2)), (bi1, bi2)) in g1
        .iter_mut()
        .zip(g2.iter_mut())
        .zip(a1.iter().zip(a2.iter()))
        .zip(b1.iter().zip(b2.iter()))
    {
        *g1i = *bi1;
        *g2i = *bi2;
        for ((m1, m2), qj) in r1.iter().zip(r2.iter()).zip(q.iter()) {
            *g1i += m1 * qj;
            *g2i += m2 * qj;
        }
    }
    let [g1x, g1y, g1z] = g1;
    let [g2x, g2y, g2z] = g2;
    let g11 = g1x * g1x + g1y * g1y + g1z * g1z;
    if g11 <= 0.0 {
        return Err(SolidError::blend(
            OP,
            BlendFailure::Defect {
                why: "forming the nodal quartic's tangent cone found a zero gradient on its first \
                      offset, though the nodal classification excludes collapsed sphere and \
                      cylinder offsets and a cone's apex",
            },
        ));
    }
    let kk = (g1x * g2x + g1y * g2y + g1z * g2z) / g11;
    let mut c = [[0.0; 3]; 3];
    for ((ci, r1), r2) in c.iter_mut().zip(a1.iter()).zip(a2.iter()) {
        for ((cij, m1), m2) in ci.iter_mut().zip(r1.iter()).zip(r2.iter()) {
            *cij = m2 - kk * m1;
        }
    }
    // a ray of the cone square to g1: a branch's tangent
    let sn = Vec3::new(g1x, g1y, g1z).unitize()?;
    let mut st1 = if sn.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    st1 = st1 - sn * st1.dot(sn);
    st1 = st1.unitize()?;
    let st2 = sn.cross(st1);
    let t1 = [st1.x, st1.y, st1.z];
    let t2 = [st2.x, st2.y, st2.z];
    let c11 = bil(&c, &t1, &t1);
    let c12 = bil(&c, &t1, &t2);
    let c22 = bil(&c, &t2, &t2);
    // c11 cos^2 + 2 c12 cos sin + c22 sin^2 = 0: the two branches' tangents
    let d = c12 * c12 - c11 * c22;
    if d < 0.0 {
        return Err(SolidError::blend(
            OP,
            BlendFailure::NodalArcUnavailable {
                reason: NodalArcReason::IsolatedNode,
                discriminant: Some(d),
                candidates: 0,
            },
        ));
    }
    if c11 == 0.0 && c22 == 0.0 {
        if c12 == 0.0 {
            return Err(SolidError::blend(
                OP,
                BlendFailure::NodalArcUnavailable {
                    reason: NodalArcReason::HigherOrderContact,
                    discriminant: Some(d),
                    candidates: 0,
                },
            ));
        }
        return Err(SolidError::blend(
            OP,
            BlendFailure::Defect {
                why: "forming the nodal branch tangents rejected a pure cross-term form, whose \
                      positive discriminant gives two real branch tangents",
            },
        ));
    }
    let mut branch = [Vec3::ZERO; 2];
    for (kb, bk) in branch.iter_mut().enumerate() {
        let root = if kb == 0 { 1.0 } else { -1.0 } * d.sqrt();
        // (cos, sin) along (c22, -c12 + root) or (-c12 - root, c11), the better conditioned
        let sb = if c22.abs() >= c11.abs() {
            st1 * c22 + st2 * (-c12 + root)
        } else {
            st1 * (-c12 - root) + st2 * c11
        };
        *bk = sb.unitize()?;
    }
    let [branch0, branch1] = branch;
    Ok(NodalArc {
        quadric: a1,
        cone: c,
        node,
        gradient: Vec3::new(g1x, g1y, g1z),
        ray: if second_base { branch1 } else { branch0 },
        from,
        to,
        branch,
    })
}

/// `SmFilletSolver::CalcRationalSpineFilletGeom(crBlend, rTsectPnts, pFilletGeom)`: the
/// round of a conic, conic-pair or nodal-quartic class as the exact rational surface of the ball's arcs
/// (a chamfer's lines), from the ball's centre at the trace's first point to its last
/// (round the whole loop on a closed edge), extended at each end as the solver asks;
/// stored in `fillet_geom` (`None`: the solver's first) with its rails (the v = min and
/// v = max isoparameter curves) and the spine as its centre line. The round is checked
/// against the ball (its rails on their faces, every point at the radius from the spine)
/// before it is kept.
///
/// # Errors
///
/// Named [`BlendFailure`] causes with `SM_ERR` for unusable sections or nodal arcs,
/// an empty closest-point solve, unavailable rational weights or a round outside tolerance;
/// [`BlendFailure::Defect`] for violated nodal or evaluator invariants;
/// [`BlendFailure::InputMissing`] with `SM_ERR_NULL_POINTER` for absent caller inputs;
/// [`SolidError::InvalidInput`] for a class other than conic, conic pair or nodal quartic; those of `SetupOffsetValues`, the solves, evaluations and constructors.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)] // one fork routine
pub fn calc_rational_spine_fillet_geom(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    b: &Brep,
    pseudo: &mut FilletBrep,
    blend: &ExactBlend,
    tsects: &[TsectPnt],
    fillet_geom: Option<FilletGeomId>,
) -> Result<()> {
    let (Some(first), Some(last)) = (tsects.first(), tsects.last()) else {
        return Err(SolidError::invalid_input(OP, "no points"));
    };
    let eu0 = ops.solver().edgeuse(0)?.ok_or_else(|| {
        SolidError::blend(
            OP,
            BlendFailure::InputMissing(BlendInput::SolverEdgeuse { rail: 0 }),
        )
    })?;
    let e = eu::edge(b, eu0)?.ok_or_else(|| {
        SolidError::blend(
            OP,
            BlendFailure::InputMissing(BlendInput::EdgeuseEdge { edgeuse: eu0 }),
        )
    })?;
    let curve: Curve = edge::curve(b, e)?.cloned().ok_or_else(|| {
        SolidError::blend(
            OP,
            BlendFailure::InputMissing(BlendInput::EdgeCurve { edge: e }),
        )
    })?;
    let ivl = edge::interval(b, e)?;
    let tol = b.tolerance();
    let arc = ops
        .solver()
        .fsg
        .as_ref()
        .ok_or_else(|| {
            SolidError::blend(OP, BlendFailure::InputMissing(BlendInput::SurfaceGenerator))
        })?
        .fillet_surface_generator_type()
        == FilletSurfaceGeneratorType::Circular;

    // the ball's centre at the trace's ends, and the edge's direction there
    ops.setup_offset_values(first, None)?;
    let start = eval_offset(k, ops.solver_mut(), 0, first.uv_pos(SrfIndex::First))?;
    ops.setup_offset_values(last, None)?;
    let end = eval_offset(k, ops.solver_mut(), 0, last.uv_pos(SrfIndex::First))?;
    let closed = curve.is_closed(k, &ivl, 0.0)?;
    // the edge's middle, and its direction at the trace's first point (for a closed edge,
    // the way round the trace runs)
    let edge_mid = curve.evaluate_point(k, ivl.evaluate(0.5))?;
    let edge_dir = {
        let sols = curve.global_point_solve(
            k,
            &ivl,
            SolverOperation::Minimize,
            start,
            EFF_ZERO_SQRT,
            SolutionRequested::Single,
            &GlobalPointSolveOptions::<crate::topology::ObjectHandle>::default(),
        )?;
        let t = sols
            .get(0)
            .ok_or_else(|| {
                SolidError::blend(
                    OP,
                    BlendFailure::CurveProjectionEmpty {
                        x: start.x,
                        y: start.y,
                        z: start.z,
                    },
                )
            })?
            .start
            .param(0)?;
        let pv = curve.evaluate(k, t, 1, true, false)?;
        pv.get(1).copied().ok_or_else(|| {
            SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "the edge curve's evaluation succeeded but returned no first derivative, \
                          which the evaluator promises for the orders asked",
                },
            )
        })?
    };

    let s = ops.solver();
    let [o0, o1] = blend.offsets;
    let mut segs: Vec<Segment> = Vec::new();
    if matches!(blend.spine, BlendSpine::Conic | BlendSpine::ConicPair) {
        // the conic: the plane's section of the other quadric (either, for a pair)
        let (q, q_face) = if o0.kind == QuadricKind::Plane {
            (o1, blend.faces[1])
        } else {
            (o0, blend.faces[0])
        };
        let mut kc = plane_section(blend.point, blend.dir, &q)?;
        if kc.kind == ConicKind::Hyperbola && (start - kc.c).dot(kc.a1) < 0.0 {
            kc.branch = -1.0;
        }
        let mut t0 = conic_param(&kc, start);
        let t1 = conic_param(&kc, end);
        // from the first point to the last: round the loop the way the edge runs on a
        // closed edge, else the way along an ellipse that passes the edge's middle
        // (tangency-labs/smlib@dda3b5d), read round the quadric's axis (not the fork's)
        let mut span = t1 - t0;
        let mut dir = if span >= 0.0 { 1.0 } else { -1.0 };
        if kc.kind == ConicKind::Ellipse {
            if closed {
                let h = 1e-6;
                let tan = conic_point(&kc, t0 + h) - conic_point(&kc, t0 - h);
                dir = if tan.dot(edge_dir) >= 0.0 { 1.0 } else { -1.0 };
                span = dir * 2.0 * PI;
            } else {
                // the arc t0 -> t1 one way or the other, by where the edge's middle lies
                // round the quadric's axis
                let mid =
                    ellipse_param_about_axis(&kc, blend.point, blend.dir, &q, &q_face, edge_mid)?;
                let within = |a: f64, span: f64, t: f64| {
                    let mut d = (t - a) * if span >= 0.0 { 1.0 } else { -1.0 };
                    d -= 2.0 * PI * (d / (2.0 * PI)).floor();
                    d <= span.abs()
                };
                while span <= 0.0 {
                    span += 2.0 * PI;
                }
                while span > 2.0 * PI {
                    span -= 2.0 * PI;
                }
                if !within(t0, span, mid) {
                    span -= 2.0 * PI;
                }
                dir = if span >= 0.0 { 1.0 } else { -1.0 };
            }
        }
        // extensions
        if !closed {
            let mut ext = s.surface_extension_factor * span.abs();
            if kc.kind == ConicKind::Ellipse {
                ext = min2(ext, 0.25 * (2.0 * PI - span.abs()));
            }
            if s.extend_before {
                t0 -= dir * ext;
                span += dir * ext;
            }
            if s.extend_after {
                span += dir * ext;
            }
        }
        let dmax = if kc.kind == ConicKind::Ellipse {
            0.5 * PI
        } else {
            0.5
        };
        let nf = (span.abs() / dmax).ceil();
        if !nf.is_finite() || nf > MAX_SPINE_SEGMENTS {
            return Err(SolidError::blend(
                OP,
                BlendFailure::RationalWeightsUnavailable {
                    step: WeightStep::SegmentPartition,
                    observed: nf,
                    limit: MAX_SPINE_SEGMENTS,
                },
            ));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let mut n = nf as usize;
        if kc.kind == ConicKind::Parabola {
            n = 4;
        }
        if n < 2 {
            n = 2;
        }
        for i in 0..n {
            segs.push(conic_segment(
                &kc,
                t0 + span * i as f64 / n as f64,
                t0 + span * (i + 1) as f64 / n as f64,
            ));
        }
    } else if blend.spine == BlendSpine::NodalQuartic {
        // tangency-labs/smlib@dda3b5d: from the node toward the trace's ends; an end at the
        // node (the round fading out there) leaves it along a branch's tangent
        let from_pt = start - blend.point;
        let to_pt = end - blend.point;
        let from_node = from_pt.length() <= tol;
        let to_node = to_pt.length() <= tol;
        if from_node && to_node {
            return Err(SolidError::blend(
                OP,
                BlendFailure::NodalArcUnavailable {
                    reason: NodalArcReason::BothEndsAtNode,
                    discriminant: None,
                    candidates: 0,
                },
            ));
        }
        let mut arc_q = nodal_quartic(&o0, &o1, blend.point, from_pt, to_pt, false)?;
        // the arc from start to end that keeps off the node: G (g1.v) is not zero inside
        // it, nor W
        let mut found = false;
        let mut cand = 0;
        let mut tried = 0;
        while cand < 4 && !found {
            let end_branch = cand / 2;
            let sign = if cand % 2 == 0 { 1.0 } else { -1.0 };
            cand += 1;
            let [branch0, branch1] = arc_q.branch;
            // cand < 4 makes end_branch 0 or 1.
            let eb = if end_branch == 0 { branch0 } else { branch1 };
            let from = if from_node { eb } else { from_pt } * sign;
            let to = if to_node { eb } else { to_pt };
            if !from_node && !to_node && end_branch == 1 {
                continue;
            }
            // the base ray: the branch no end leaves along
            let second_base = (from_node || to_node) && end_branch == 0;
            arc_q = nodal_quartic(&o0, &o1, blend.point, from, to, second_base)?;
            tried += 1;
            let mut ok = true;
            let (g, w) = arc_q.gw(0.5);
            let mut kk = 1;
            while kk < 64 && ok {
                let t = f64::from(kk) / 64.0;
                let (gt, wt) = arc_q.gw(t);
                if !positive(gt * g) || !positive(wt * w) {
                    ok = false;
                }
                kk += 1;
            }
            if !positive(arc_q.gw(0.0).1 * w) || !positive(arc_q.gw(1.0).1 * w) {
                ok = false;
            }
            found = ok;
        }
        if !found {
            // The candidates run through the node or infinity, or fail the endpoint checks.
            return Err(SolidError::blend(
                OP,
                BlendFailure::NodalArcUnavailable {
                    reason: NodalArcReason::NoFiniteRegularArc,
                    discriminant: None,
                    candidates: tried,
                },
            ));
        }
        // s runs from the trace's first point to its last
        let mut s0 = 0.0;
        let mut s1 = 1.0;
        {
            // extend, as far as the node and the point at infinity allow
            let (g0, w0) = arc_q.gw(0.5);
            let clear = |sv: f64| {
                let (g, w) = arc_q.gw(sv);
                g * g0 > 0.0 && w * w0 > 0.0
            };
            let ext = s.surface_extension_factor;
            for kk in 1..=32 {
                let sv = -ext * f64::from(kk) / 32.0;
                if !s.extend_before || !clear(sv - ext / 32.0) {
                    break;
                }
                s0 = sv;
            }
            for kk in 1..=32 {
                let sv = 1.0 + ext * f64::from(kk) / 32.0;
                if !s.extend_after || !clear(sv + ext / 32.0) {
                    break;
                }
                s1 = sv;
            }
        }
        segs = arc_q.segments(s0, s1);
    } else {
        return Err(SolidError::invalid_input(OP, "not a rational spine"));
    }

    let parts = rational_spine_parts(k, blend, segs, arc, tol)?;
    store_parts(ops.solver(), pseudo, fillet_geom, parts)
}

/// Build the surface, rails and leader from the rational spine's segments.
///
/// Both the solver and the mixed-quadric regressions use this construction, including
/// endpoint-weight normalization, bounded positive-weight subdivision and knot placement.
/// All arithmetic and traversal orders are preserved from the original builder.
///
/// # Errors
///
/// Those of the original construction: nonpositive or excessive weight ranges, invalid
/// canonical data, a surface off its ball or supports, and curve/surface evaluations.
fn rational_spine_parts(
    k: &Kernel,
    blend: &ExactBlend,
    mut segs: Vec<Segment>,
    arc: bool,
    tol: f64,
) -> Result<FilletGeomParts> {
    let r = blend.radius;
    let [o0, o1] = blend.offsets;
    // each segment reparametrised (its weights times c^i) so that its end weights agree,
    // and scaled to meet the one before: the same curve, its weights 1 at every joint
    {
        let mut prev = 1.0;
        for sg in &mut segs {
            let d = degree(&sg.w);
            let (w0, wd) = (at(&sg.w, 0), at(&sg.w, d));
            if !positive(w0) || !positive(wd) {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::RationalWeightsUnavailable {
                        step: WeightStep::EndpointWeights,
                        observed: if w0.is_nan() { w0 } else { min2(w0, wd) },
                        limit: 0.0,
                    },
                ));
            }
            let c = (w0 / wd).powf(1.0 / d as f64);
            let mut ci = prev / w0;
            for i in 0..=d {
                let wi = at(&sg.w, i) * ci;
                put(&mut sg.w, i, wi);
                for row in &mut sg.m.a {
                    let x = at(row, i) * ci;
                    put(row, i, x);
                }
                ci *= c;
            }
            prev = at(&sg.w, d);
        }
    }

    // the rows over each segment, segments halved until every weight is positive
    let ia = usize::from(o0.kind == QuadricKind::Cone && o1.kind != QuadricKind::Cone);
    let (oa, ca, ob, cb) = if ia == 0 {
        (o0, blend.contact[0], o1, blend.contact[1])
    } else {
        (o1, blend.contact[1], o0, blend.contact[0])
    };
    // a chamfer's two offsets; a ball's one radius
    let radii = {
        let [r0, r1] = match blend.inputs {
            Some(inputs) if !arc => inputs.offsets.map(f64::abs),
            _ => [r, r],
        };
        if ia == 0 {
            [r0, r1]
        } else {
            [r1, r0]
        }
    };
    let mut rows_x: Vec<Vec<VPoly>> = Vec::new();
    let mut rows_w: Vec<Vec<Poly>> = Vec::new();
    let mut done: Vec<Segment> = Vec::new();
    for sg in &segs {
        let mut todo: VecDeque<(Segment, u32)> = VecDeque::new();
        todo.push_back((sg.clone(), 0));
        while let Some((ss, depth)) = todo.pop_front() {
            let (mut sx, mut swr) = round_rows(
                &contact_of(&oa, ca, &ss),
                &contact_of(&ob, cb, &ss),
                &ss,
                radii,
                arc,
            );
            let mut all_positive = true;
            for w in &swr {
                for &c in w {
                    if !positive(c) {
                        all_positive = false;
                    }
                }
            }
            for &c in &ss.w {
                if !positive(c) {
                    all_positive = false;
                }
            }
            if !all_positive {
                if depth >= 6 {
                    let least = swr.iter().flatten().chain(ss.w.iter()).fold(
                        f64::INFINITY,
                        |least, &coefficient| {
                            if least.is_nan() {
                                least
                            } else {
                                min2(least, coefficient)
                            }
                        },
                    );
                    return Err(SolidError::blend(
                        OP,
                        BlendFailure::RationalWeightsUnavailable {
                            step: WeightStep::PositiveWeights,
                            observed: least,
                            limit: f64::from(depth),
                        },
                    ));
                }
                let mut l = Segment::default();
                let mut rr = Segment::default();
                for ((src, dl), dr) in ss.m.a.iter().zip(l.m.a.iter_mut()).zip(rr.m.a.iter_mut()) {
                    let (a, bb) = split(src);
                    *dl = a;
                    *dr = bb;
                }
                let (lw, rw) = split(&ss.w);
                l.w = lw;
                rr.w = rw;
                // in order: the left half first
                todo.push_front((rr, depth + 1));
                todo.push_front((l, depth + 1));
                continue;
            }
            if ia == 1 {
                sx.reverse();
                swr.reverse();
            }
            rows_x.push(sx);
            rows_w.push(swr);
            done.push(ss);
        }
    }

    // the surface: u along the spine, its knots at the segments' ends spaced by their
    // chords
    let mut deg = 0usize;
    for (rx, rw) in rows_x.iter().zip(rows_w.iter()) {
        for (x, w) in rx.iter().zip(rw.iter()) {
            deg = deg.max(degree(w));
            for c in &x.a {
                deg = deg.max(degree(c));
            }
        }
    }
    // Conics supply at least two quadratic segments, nodal arcs sixteen quartics;
    // subdivision preserves positive degree and round_rows makes exactly 2 or 3 rows.
    let nv = if arc { 3 } else { 2 };
    let nseg = rows_w.len();
    let nu = nseg * deg + 1;
    let mut cps = vec![Point3::ZERO; nu * nv];
    let mut wts = vec![1.0; nu * nv];
    let mut u_knots: Vec<f64> = Vec::new();
    let mut u_mult: Vec<usize> = Vec::new();
    let mut du = 0.0;
    u_knots.push(du);
    u_mult.push(deg + 1);
    for (kk, ((rx, rw), sd)) in rows_x
        .iter()
        .zip(rows_w.iter())
        .zip(done.iter())
        .enumerate()
    {
        for (j, (x, w)) in rx.iter().zip(rw.iter()).enumerate() {
            let sw = elevate(w, deg);
            let [x0, x1, x2] = &x.a;
            let sx = [elevate(x0, deg), elevate(x1, deg), elevate(x2, deg)];
            for i in 0..=deg {
                let lu = kk * deg + i;
                let wi = at(&sw, i);
                if let (Some(p), Some(wt)) = (cps.get_mut(lu * nv + j), wts.get_mut(lu * nv + j)) {
                    *p = Point3::new(at(&sx[0], i) / wi, at(&sx[1], i) / wi, at(&sx[2], i) / wi);
                    *wt = wi;
                }
            }
        }
        let w0 = eval(&sd.w, 0.0);
        let w1 = eval(&sd.w, 1.0);
        let chord = (v_eval(&sd.m, 1.0) * (1.0 / w1)).distance(v_eval(&sd.m, 0.0) * (1.0 / w0));
        du += max2(chord, tol);
        u_knots.push(du);
        u_mult.push(if kk + 1 < nseg { deg } else { deg + 1 });
    }
    // v: from contact 0 to contact 1, about as long as the section
    let vlen = max2(r, tol);
    let v_knots = [0.0, vlen];
    let v_mult = [nv, nv];
    // the weights scaled about 1 (the surface is the same); they must stay within the
    // range a NURBS takes
    {
        let mut wmin = BIG_DOUBLE;
        let mut wmax = 0.0;
        for &w in &wts {
            wmin = min2(wmin, w);
            wmax = max2(wmax, w);
        }
        if !positive(wmin) || wmax > 1e4 * wmin {
            return Err(SolidError::blend(
                OP,
                BlendFailure::RationalWeightsUnavailable {
                    step: WeightStep::WeightRange,
                    observed: if positive(wmin) { wmax / wmin } else { wmin },
                    limit: 1e4,
                },
            ));
        }
        let sc = 1.0 / (wmin * wmax).sqrt();
        for w in &mut wts {
            *w *= sc;
        }
    }
    let round = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: deg,
        v_degree: nv - 1,
        control_points: &cps,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &u_mult,
        v_multiplicities: &v_mult,
        u_knots: &u_knots,
        v_knots: &v_knots,
        knot_type: KnotType::PiecewiseBezierKnots,
        weights: Some(&wts),
        uv_domain: None,
    })?;
    let round = Surface::from(round);

    // the round is the ball's: its rails on their faces, every point at r from the spine
    // One first knot and one per done segment give each segment exactly one span.
    for (sd, (&ua, &ub)) in done.iter().zip(u_knots.iter().zip(u_knots.iter().skip(1))) {
        for a in 0..=4 {
            let t = f64::from(a) / 4.0;
            let w = eval(&sd.w, t);
            let m = v_eval(&sd.m, t) * (1.0 / w);
            let u = ua + (ub - ua) * t;
            for bb in 0..=4 {
                let x = round.evaluate_point(k, Point2::new(u, vlen * f64::from(bb) / 4.0))?;
                let off = if arc { (x.distance(m) - r).abs() } else { 0.0 };
                let on_face = if bb == 0 {
                    blend.faces[0].distance(x)
                } else if bb == 4 {
                    blend.faces[1].distance(x)
                } else {
                    0.0
                };
                if off > tol || on_face > tol {
                    return Err(SolidError::blend(
                        OP,
                        BlendFailure::RationalRoundOutOfTolerance {
                            radius_error: off,
                            support_error: on_face,
                            tolerance: tol,
                        },
                    ));
                }
            }
        }
    }

    // its rails the v = min and v = max isoparameter curves, and the spine
    let uv = round.natural_uv_domain_with_nurb(k)?;
    let rail = |v| -> Result<Curve> {
        Ok(round
            .create_iso_parametric_curve(k, SurfParamType::V, v, 0.0, None, false)?
            .curve)
    };
    let rails = [rail(uv.min().y)?, rail(uv.max().y)?];
    let rail_ivls = rails.each_ref().map(|c| Some(c.natural_interval()));
    let spine = {
        let sdeg = done.first().map_or(0, |d| degree(&d.w));
        let mut sp: Vec<Point3> = Vec::new();
        let mut sw: Vec<f64> = Vec::new();
        let mut sm: Vec<usize> = Vec::new();
        for (kk, d) in done.iter().enumerate() {
            for i in usize::from(kk != 0)..=sdeg {
                let w = at(&d.w, i);
                let [m0, m1, m2] = &d.m.a;
                sp.push(Point3::new(at(m0, i) / w, at(m1, i) / w, at(m2, i) / w));
                sw.push(w);
            }
            sm.push(if kk == 0 { sdeg + 1 } else { sdeg });
        }
        sm.push(sdeg + 1);
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: sdeg,
            control_points: &sp,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &sm,
            knots: &u_knots,
            knot_type: KnotType::PiecewiseBezierKnots,
            weights: Some(&sw),
            trim: None,
        })?
    };
    Ok(FilletGeomParts {
        surface: round,
        rails,
        rail_intervals: rail_ivls,
        center_line: Some(Curve::from(spine)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_exact::{classify_supports, ClassInputs};
    use crate::curve::curve::CurveKind;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::surface::cone::Cone;
    use crate::surface::procedural_blend::convert::{exact_rebuild, exact_to_record};
    use crate::surface::procedural_blend::exact::{ExactClass, ExactRound};
    use crate::surface::sphere::Sphere;

    /// A homogeneous quartic segment as the canonical curve the identity checker reads.
    fn segment_curve(segment: &Segment) -> BSplineCurve {
        let [mx, my, mz] = &segment.m.a;
        let points: Vec<_> = segment
            .w
            .iter()
            .enumerate()
            .map(|(j, &weight)| Point3::new(mx[j], my[j], mz[j]) / weight)
            .collect();
        let maximum = segment.w.iter().copied().fold(0.0_f64, f64::max);
        let weights: Vec<_> = segment.w.iter().map(|w| w / maximum).collect();
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 4,
            control_points: &points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[5, 5],
            knots: &[0.0, 1.0],
            knot_type: KnotType::PiecewiseBezierKnots,
            weights: Some(&weights),
            trim: None,
        })
        .unwrap()
    }

    #[test]
    fn bernstein_products_and_halves() {
        // (1 - t + 2t) = 1 + t ; squared = 1 + 2t + t^2 -> Bernstein [1, 2, 4]
        let p = vec![1.0, 2.0];
        let q = mul(&p, &p);
        assert_eq!(q, vec![1.0, 2.0, 4.0]);
        assert!((eval(&q, 0.5) - 2.25).abs() < 1e-15);
        let (l, r) = split(&q);
        assert!((eval(&l, 1.0) - 2.25).abs() < 1e-15);
        assert!((eval(&r, 0.0) - 2.25).abs() < 1e-15);
        assert!((eval(&l, 0.5) - eval(&q, 0.25)).abs() < 1e-15);
        let e = elevate(&q, 4);
        assert!((eval(&e, 0.3) - eval(&q, 0.3)).abs() < 1e-15);
        assert_eq!(add(&p, &konst(1.0), 1.0), vec![2.0, 3.0]);
    }

    #[test]
    fn a_tilted_cylinder_cut_by_a_plane_is_an_ellipse_its_segments_on_it() {
        for tau in [0.0_f64, f64::EPSILON, 0.5] {
            let q = BlendQuadric {
                kind: QuadricKind::Cylinder,
                point: Point3::ZERO,
                dir: Vec3::new(0.0, -tau.sin(), tau.cos()),
                radius: 4.0,
                angle: 0.0,
                side: 1.0,
                tube: 0.0,
            };
            let kc =
                plane_section(Point3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0), &q).unwrap();
            assert_eq!(kc.kind, ConicKind::Ellipse);
            for i in 0..8 {
                let t = f64::from(i);
                let s = conic_segment(&kc, t, t + 0.7);
                for j in 0..=10 {
                    let x =
                        v_eval(&s.m, f64::from(j) / 10.0) * (1.0 / eval(&s.w, f64::from(j) / 10.0));
                    assert!((x.z - 1.0).abs() < 1e-13);
                    assert!(q.distance(x).abs() < 1e-13, "{}", q.distance(x));
                }
                let p = conic_point(&kc, t);
                assert!((conic_param(&kc, p) - t.sin().atan2(t.cos())).abs() < 1e-12);
            }
        }
    }

    /// A shallow revolved cone cut by a plane has a hyperbolic round. Its exact
    /// certificate must survive rebuilding the supports from their rational forms.
    #[test]
    fn a_shallow_cone_round_keeps_its_certificate_through_its_record() {
        shallow_cone_round_at(&Axis2Placement::default());
    }

    #[test]
    fn a_rotated_shallow_cone_round_keeps_its_certificate_through_its_record() {
        for placement in [
            Axis2Placement::new(
                Point3::ZERO,
                Vec3::new(3.0 / 5.0, 0.0, -4.0 / 5.0),
                Vec3::new(0.0, 1.0, 0.0),
            ),
            Axis2Placement::new(
                Point3::new(60.0, 0.0, 0.0),
                Vec3::new(-1.0, 1e-9, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ),
        ] {
            shallow_cone_round_at(&placement);
        }
    }

    fn shallow_cone_round_at(placement: &Axis2Placement) {
        use crate::surface::plane::Plane;

        let k = Kernel::default();
        let radius = 0.4;
        let slope = 1.0_f64 / 320.0;
        let cone = Surface::from(
            Cone::create_canonical_with_semi_angle(
                &k,
                placement,
                60.0,
                slope.atan().to_degrees(),
                Some(6.0),
                false,
                false,
            )
            .unwrap(),
        );
        let cone_uv = cone.natural_uv_domain_with_nurb(&k).unwrap().mid();
        let quadric = BlendQuadric::from_surface(&k, &cone, cone_uv, 1e-5).unwrap();
        let generator = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 1,
            control_points: &[
                placement.transform_point(Point3::new(60.0, 0.0, 0.0)),
                placement.transform_point(Point3::new(60.0 + 6.0 * slope, 0.0, 6.0)),
            ],
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[2, 2],
            knots: &[0.0, 1.0],
            knot_type: KnotType::PiecewiseBezierKnots,
            weights: None,
            trim: None,
        })
        .unwrap();
        assert!(
            crate::surface::implicit::curve_on_quadrics(&generator, &[quadric.implicit(); 2],)
                .unwrap(),
            "the exact generator lies on its cone's implicit equation"
        );
        let plane = Surface::from(
            Plane::create_canonical(&Axis2Placement::new(
                placement.transform_point(Point3::ZERO),
                placement.transform_vector(Vec3::new(0.0, 1.0, 0.0)),
                placement.transform_vector(Vec3::new(0.0, 0.0, 1.0)),
            ))
            .unwrap(),
        );
        let inputs = ClassInputs {
            uv: [
                plane.natural_uv_domain_with_nurb(&k).unwrap().mid(),
                cone.natural_uv_domain_with_nurb(&k).unwrap().mid(),
            ],
            offsets: [-radius, -radius],
            centre: placement.transform_point(Point3::new(-0.4, 59.6, 0.0)),
            tol: 1e-5,
            ball_on_collapsed: false,
            radii_can_change: false,
        };
        let blend = classify_supports(&k, [&plane, &cone], &inputs).unwrap();
        assert_eq!(blend.spine, BlendSpine::Conic);
        let mut conic = plane_section(blend.point, blend.dir, &blend.offsets[1]).unwrap();
        assert_eq!(conic.kind, ConicKind::Hyperbola);
        if (inputs.centre - conic.c).dot(conic.a1) < 0.0 {
            conic.branch = -1.0;
        }
        let t = conic_param(&conic, inputs.centre);
        let parts = rational_spine_parts(
            &k,
            &blend,
            vec![
                conic_segment(&conic, t - 0.0001, t),
                conic_segment(&conic, t, t + 0.0001),
            ],
            true,
            inputs.tol,
        )
        .unwrap();
        let CurveKind::BSpline(leader) = parts.center_line.as_ref().unwrap().kind() else {
            panic!("the conic leader is a rational quadratic");
        };
        let round = ExactRound::create(
            &k,
            ExactClass::Conic,
            [&plane, &cone],
            radius,
            Some(leader),
            inputs,
        )
        .unwrap();
        let mut net = parts.surface;
        assert_eq!(round.recertify(&k, &net).unwrap(), 0.0);
        net.set_exact_round(Some(round));
        let record = exact_to_record(&k, &net).unwrap();
        let rebuilt = exact_rebuild(&k, &record).unwrap();
        assert_eq!(
            rebuilt.procedural_blend().unwrap().recertify(&k).unwrap(),
            0.0
        );
        let mut off = record;
        let leader = off.leader.as_mut().unwrap();
        let normal = placement.x_axis();
        for (p, w) in leader
            .points
            .iter_mut()
            .zip(leader.weights.as_ref().unwrap())
        {
            p[0] += 0.001 * normal.x * w;
            p[1] += 0.001 * normal.y * w;
            p[2] += 0.001 * normal.z * w;
        }
        assert!(
            exact_rebuild(&k, &off).is_err(),
            "a spine off its plane is refused"
        );
    }

    /// A point of a tilted convex cylinder, far along the axis from the plane of the spine
    /// ellipse, reads at the spine parameter of the ball's centre for that point: the same
    /// angle round the axis where the ball is smaller than the cylinder's radius, half a turn
    /// on where it is larger and the offset lies past the axis (#18824).
    #[test]
    fn a_point_of_the_face_reads_where_its_balls_centre_lies_on_the_spine_ellipse() {
        let tau = 0.7f64;
        let axis = Vec3::new(0.0, tau.sin(), tau.cos());
        let e1 = Vec3::new(1.0, 0.0, 0.0);
        let e2 = axis.cross(e1);
        let face = BlendQuadric {
            kind: QuadricKind::Cylinder,
            point: Point3::ZERO,
            dir: axis,
            radius: 1.0,
            angle: 0.0,
            side: 1.0,
            tube: 0.0,
        };
        let normal = Vec3::new(0.0, 1.0, 0.0);
        for ball in [0.3, 0.966, 1.4] {
            let q = face.offset(-ball, 1e-12);
            assert_eq!(q.side < 0.0, ball > 1.0);
            let at = Point3::new(0.0, 0.5, 0.0);
            let kc = plane_section(at, normal, &q).unwrap();
            assert_eq!(kc.kind, ConicKind::Ellipse);
            for i in 0..12 {
                let phi = 0.3 + 0.5 * f64::from(i);
                let radial = e1 * phi.cos() + e2 * phi.sin();
                // a point of the face 3 along the axis from the plane, and its ball's centre
                // moved along the axis onto the plane
                let x = axis * 3.0 + radial;
                let centre = x - radial * ball;
                let on_plane = centre - axis * ((centre - at).dot(normal) / axis.dot(normal));
                let truth = conic_param(&kc, on_plane);
                let read = ellipse_param_about_axis(&kc, at, normal, &q, &face, x).unwrap();
                let off = (read - truth).sin().abs() + (1.0 - (read - truth).cos());
                assert!(
                    off < 1e-12,
                    "ball {ball} phi {phi}: read {read} truth {truth}"
                );
            }
        }
    }

    /// A point the axis reading cannot place is refused as the caller's input, with status
    /// `SM_ERR`, not read about the conic's centre: one on the axis (the half-line from the
    /// axis has no direction) and one read against a cylinder whose axis crosses the plane
    /// outside the conic (the half-line misses it, or crosses it behind the axis).
    #[test]
    fn a_point_the_axis_reading_cannot_place_is_refused() {
        let face = BlendQuadric {
            kind: QuadricKind::Cylinder,
            point: Point3::ZERO,
            dir: Vec3::new(0.0, 0.6, 0.8),
            radius: 1.0,
            angle: 0.0,
            side: 1.0,
            tube: 0.0,
        };
        let q = face.offset(-0.3, 1e-12);
        let (at, normal) = (Point3::new(0.0, 0.5, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let kc = plane_section(at, normal, &q).unwrap();
        // the refusal's numbers: (length, discriminant, crossing)
        let refused = |r: Result<f64>| -> (f64, f64, Option<f64>) {
            let e = r.unwrap_err();
            assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput, "{e:?}");
            assert_eq!(e.sm_status(), Some(crate::error::SM_ERR), "{e:?}");
            match e.blend_failure() {
                Some(BlendFailure::ExactBlendAxisCannotPlaceEdge {
                    length,
                    discriminant,
                    crossing,
                }) => (*length, *discriminant, *crossing),
                other => panic!("{other:?}"),
            }
        };
        // on the axis, 3 along it from the origin: no direction, no crossing computed
        let (length, _, crossing) = refused(ellipse_param_about_axis(
            &kc,
            at,
            normal,
            &q,
            &face,
            face.dir * 3.0,
        ));
        assert!(length <= EFF_ZERO, "{length}");
        assert_eq!(crossing, None);
        // a cylinder 10 off to the side: its axis crosses the plane outside the conic; a
        // point further out reads along a half-line pointing away from the conic (its line
        // crosses the conic behind the axis), and one off sideways along a half-line that
        // passes the conic by
        let mut far = q;
        far.point = Point3::new(10.0, 0.0, 0.0);
        let read = |x: Point3| {
            refused(ellipse_param_about_axis(
                &kc,
                at,
                normal,
                &far,
                &face,
                x + face.dir * 3.0,
            ))
        };
        let (_, discriminant, crossing) = read(Point3::new(12.0, 0.0, 0.0));
        assert!(discriminant >= 0.0, "{discriminant}");
        assert!(crossing.is_some_and(|mu| mu <= 0.0), "{crossing:?}");
        let (_, discriminant, crossing) = read(Point3::new(10.0, 0.0, 5.0));
        assert!(discriminant < 0.0, "{discriminant}");
        assert_eq!(crossing, None);
    }

    /// A plane that meets the quadric in no conic says how it fails, with status `SM_ERR`.
    #[test]
    fn planes_missing_a_sphere_or_an_axial_cylinder_report_empty_sections() {
        let quadric = |kind: QuadricKind| BlendQuadric {
            kind,
            point: Point3::ZERO,
            dir: Vec3::new(0.0, 0.0, 1.0),
            radius: 4.0,
            angle: 0.0,
            side: 1.0,
            tube: 0.0,
        };
        let missed = plane_section(
            Point3::new(0.0, 0.0, 10.0),
            Vec3::new(0.0, 0.0, 1.0),
            &quadric(QuadricKind::Sphere),
        )
        .unwrap_err();
        assert_eq!(
            missed.blend_failure(),
            Some(&BlendFailure::SectionNotConic {
                reason: SectionShape::Empty,
                quadratic_scale: 1.0,
                discriminant: None,
                centered_value: Some(84.0),
            })
        );
        assert_eq!(missed.sm_status(), Some(crate::error::SM_ERR));
        // a plane along the cylinder's axis, outside it
        let missed_cylinder = plane_section(
            Point3::new(10.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            &quadric(QuadricKind::Cylinder),
        )
        .unwrap_err();
        // y^2 + 84 = 0 has discriminant -336: this is empty, not lines.
        assert_eq!(
            missed_cylinder.blend_failure(),
            Some(&BlendFailure::SectionNotConic {
                reason: SectionShape::Empty,
                quadratic_scale: 1.0,
                discriminant: Some(-336.0),
                centered_value: None,
            })
        );
        assert_eq!(missed_cylinder.sm_status(), Some(crate::error::SM_ERR));
    }

    /// Closed forms distinguish unsupported sections, real lines, and a tangent point.
    #[test]
    fn unusable_sections_distinguish_lines_a_point_and_no_quadratic_form() {
        let sphere = BlendQuadric {
            kind: QuadricKind::Sphere,
            point: Point3::ZERO,
            dir: Vec3::Z,
            radius: 4.0,
            ..BlendQuadric::default()
        };
        let cylinder = BlendQuadric {
            kind: QuadricKind::Cylinder,
            ..sphere
        };
        let cone = BlendQuadric {
            kind: QuadricKind::Cone,
            angle: PI / 4.0,
            ..sphere
        };
        let plane = BlendQuadric {
            kind: QuadricKind::Plane,
            ..sphere
        };
        for (q, point, normal, reason, discriminant, centered_value) in [
            // y^2 - 16 = 0 gives two axial lines, discriminant 64.
            (
                cylinder,
                Point3::ZERO,
                Vec3::X,
                SectionShape::Lines,
                Some(64.0),
                None,
            ),
            // y^2 = 0 gives the cylinder's one tangent line.
            (
                cylinder,
                Vec3::X * 4.0,
                Vec3::X,
                SectionShape::Lines,
                Some(0.0),
                None,
            ),
            // x^2 + y^2 = 0 at the sphere's north pole is a point.
            (
                sphere,
                Vec3::Z * 4.0,
                Vec3::Z,
                SectionShape::Point,
                None,
                Some(0.0),
            ),
            (
                plane,
                Point3::ZERO,
                Vec3::Z,
                SectionShape::NoQuadraticForm,
                None,
                None,
            ),
            // A cone through its apex cut by an axial plane is two lines, not a point.
            (
                cone,
                Point3::ZERO,
                Vec3::X,
                SectionShape::Lines,
                None,
                Some(0.0),
            ),
        ] {
            let result = plane_section(point, normal, &q);
            assert!(result.is_err());
            if let Err(e) = result {
                assert!(
                    matches!(
                        e.blend_failure(),
                        Some(BlendFailure::SectionNotConic {
                            reason: actual,
                            discriminant: actual_discriminant,
                            centered_value: actual_centered,
                            ..
                        }) if *actual == reason && *actual_discriminant == discriminant
                            && *actual_centered == centered_value
                    ),
                    "{e:?}"
                );
                assert_eq!(e.sm_status(), Some(crate::error::SM_ERR));
            }
        }
    }

    /// The three tangent-cone stops stay failures, but no longer share an isolated-node phrase.
    #[test]
    fn nodal_tangent_cones_distinguish_isolated_higher_order_and_pure_cross_terms() {
        let sphere = BlendQuadric {
            kind: QuadricKind::Sphere,
            point: Point3::ZERO,
            radius: 1.0,
            ..BlendQuadric::default()
        };
        let other_sphere = BlendQuadric {
            point: Vec3::Z * 2.0,
            ..sphere
        };
        let diagonal = std::f64::consts::FRAC_1_SQRT_2;
        let cylinder = BlendQuadric {
            kind: QuadricKind::Cylinder,
            dir: Vec3::new(diagonal, diagonal, 0.0),
            ..sphere
        };
        let other_cylinder = BlendQuadric {
            dir: Vec3::new(diagonal, -diagonal, 0.0),
            ..cylinder
        };
        let cases = [
            // At z=1 the spheres' gradients oppose, C=2I and d=-4.
            (sphere, other_sphere, Vec3::Z, BlendFailure::NodalArcUnavailable {
                reason: NodalArcReason::IsolatedNode,
                discriminant: Some(-4.0),
                candidates: 0,
            }),
            // Identical quadrics have C=0 on the tangent plane.
            (sphere, sphere, Vec3::Z, BlendFailure::NodalArcUnavailable {
                reason: NodalArcReason::HigherOrderContact,
                discriminant: Some(0.0),
                candidates: 0,
            }),
            // Equal diagonal terms cancel exactly; the xy terms have opposite signs.
            (cylinder, other_cylinder, Vec3::Z, BlendFailure::Defect {
                why: "forming the nodal branch tangents rejected a pure cross-term form, whose \
                      positive discriminant gives two real branch tangents",
            }),
            (sphere, sphere, Point3::ZERO, BlendFailure::Defect {
                why: "forming the nodal quartic's tangent cone found a zero gradient on its first \
                      offset, though the nodal classification excludes collapsed sphere and \
                      cylinder offsets and a cone's apex",
            }),
        ];
        for (a, b, node, cause) in cases {
            let result = nodal_quartic(&a, &b, node, Vec3::X, Vec3::Y, false);
            assert!(result.is_err());
            if let Err(e) = result {
                assert_eq!(e.blend_failure(), Some(&cause));
                assert_eq!(e.kind(), cause.class());
                assert_eq!(e.sm_status(), Some(crate::error::SM_ERR));
            }
        }
    }

    /// The original nodal pair satisfies both offset equations, including its last end.
    #[test]
    fn two_touching_cylinders_meet_in_a_nodal_quartic_its_segments_on_both() {
        // r3.5 about z and r1.5 about x through (0, 2, 0): touching at (0, 3.5, 0)
        let cyl = |point: Point3, dir: Vec3, radius: f64| BlendQuadric {
            kind: QuadricKind::Cylinder,
            point,
            dir,
            radius,
            angle: 0.0,
            side: 1.0,
            tube: 0.0,
        };
        let a = cyl(Point3::ZERO, Vec3::new(0.0, 0.0, 1.0), 3.5);
        let b = cyl(Point3::new(0.0, 2.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 1.5);
        let node = Point3::new(0.0, 3.5, 0.0);
        let on = |phi: f64| {
            let y = 2.0 + 1.5 * phi.cos();
            Point3::new((3.5f64 * 3.5 - y * y).sqrt(), y, 1.5 * phi.sin())
        };
        let arc = nodal_quartic(
            &a,
            &b,
            node,
            on(0.3) - node,
            on(2.0 * PI - 0.3) - node,
            false,
        )
        .unwrap();
        // the ends where they were asked for, every point on both
        let end = |s: f64| {
            let point = arc.segment(s, s);
            v_eval(&point.m, 0.0) / eval(&point.w, 0.0)
        };
        let mut worst = 0.0f64;
        for i in 0..=64 {
            let x = end(f64::from(i) / 64.0);
            worst = worst.max(a.distance(x).abs()).max(b.distance(x).abs());
        }
        let seg = arc.segment(0.25, 0.5);
        assert!((v_eval(&seg.m, 0.5) / eval(&seg.w, 0.5)).distance(end(0.375)) < 1e-13);
        // A rational exact-class spine must satisfy the offsets to rounding, including
        // the last end, where the old power-basis construction lost about 1e-10.
        let rounding = crate::nurbs::math::gamma(64) * a.radius;
        assert!(worst < rounding, "{worst:e} exceeds {rounding:e}");
        assert!(end(0.0).distance(on(0.3)) < 1e-14);
        // The supplied end direction is itself rounded and its projection near the
        // cone's base ray is ill-conditioned. Keep the original endpoint-match bar;
        // exactness is proved on both offsets, coefficient by coefficient below.
        assert!(end(1.0).distance(on(2.0 * PI - 0.3)) < 1e-9);
        for i in 0..16 {
            let lo = f64::from(i) / 16.0;
            let hi = f64::from(i + 1) / 16.0;
            let curve = segment_curve(&arc.segment(lo, hi));
            assert!(
                crate::surface::implicit::curve_on_quadrics(&curve, &[a.implicit(), b.implicit()])
                    .unwrap(),
                "segment {i} is not identically on both offsets"
            );
        }
    }

    /// The identities survive changes of scale, frame, ordering and projective chart.
    #[test]
    fn nodal_quartic_identities_hold_at_other_scales_frames_and_directions() {
        for size in [1e-3, 1.0, 1e3] {
            for angle in [0.0_f64, 0.7] {
                let origin = Point3::new(7.0, -4.0, 2.0) * size;
                let rotate = |v: Vec3| {
                    Vec3::new(
                        angle.cos() * v.x - angle.sin() * v.y,
                        angle.sin() * v.x + angle.cos() * v.y,
                        v.z,
                    )
                };
                let cylinder = |point: Point3, dir: Vec3, radius: f64| BlendQuadric {
                    kind: QuadricKind::Cylinder,
                    point,
                    dir,
                    radius,
                    angle: 0.0,
                    side: 1.0,
                    tube: 0.0,
                };
                for (r, rho) in [(3.5, 1.5), (7.0, 1.0)] {
                    let a = cylinder(origin, Vec3::new(0.0, 0.0, 1.0), r * size);
                    let b = cylinder(
                        origin + rotate(Vec3::new(0.0, r - rho, 0.0)) * size,
                        rotate(Vec3::new(1.0, 0.0, 0.0)),
                        rho * size,
                    );
                    let node = origin + rotate(Vec3::new(0.0, r, 0.0)) * size;
                    let from_node = |phi: f64| {
                        let y = r - rho + rho * phi.cos();
                        rotate(Vec3::new((r * r - y * y).sqrt(), y - r, rho * phi.sin())) * size
                    };
                    for (first, second) in [(a, b), (b, a)] {
                        for reversed in [false, true] {
                            let (from, to) = if reversed {
                                (from_node(2.0 * PI - 0.3), from_node(0.3))
                            } else {
                                (from_node(0.3), from_node(2.0 * PI - 0.3))
                            };
                            for base in [false, true] {
                                let arc =
                                    nodal_quartic(&first, &second, node, from, to, base).unwrap();
                                for i in 0..16 {
                                    let lo = f64::from(i) / 16.0;
                                    let hi = f64::from(i + 1) / 16.0;
                                    let curve = segment_curve(&arc.segment(lo, hi));
                                    assert!(
                                        crate::surface::implicit::curve_on_quadrics(&curve, &[first.implicit(), second.implicit()]).unwrap(),
                                        "size {size}, angle {angle}, radii {r}/{rho}, reversed {reversed}, base {base}, segment {i}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Non-cylinder pairs with independent closed-form intersection points.
    ///
    /// The cylinder/sphere has x²+y²=4 and z²=8-4x. For the cone/sphere,
    /// x²+y²=z² and x=-z²/2+3z-2. The cylinder/cone uses the circle
    /// (x-1)²+(z-3)²=2 and y²=z²-x². For the two cones, l=z+x, h=z-x
    /// give lh=y² and l(l-4)²=2√2 y²(y+4); t=(l-4)/y parametrizes them.
    /// Their normals agree at the stated nodes; the two cone axes are skew.
    fn other_nodal_pairs() -> [([BlendQuadric; 2], Point3, [Point3; 2]); 4] {
        let cylinder = |point, dir, radius| BlendQuadric {
            kind: QuadricKind::Cylinder,
            point,
            dir,
            radius,
            ..BlendQuadric::default()
        };
        let sphere = |point, radius| BlendQuadric {
            kind: QuadricKind::Sphere,
            point,
            radius,
            ..BlendQuadric::default()
        };
        let cone = BlendQuadric {
            kind: QuadricKind::Cone,
            dir: Vec3::new(0.0, 0.0, 1.0),
            angle: PI / 4.0,
            ..BlendQuadric::default()
        };
        let cylinder_sphere = |phi: f64| {
            let x = 2.0 * phi.cos();
            Point3::new(x, 2.0 * phi.sin(), (8.0 - 4.0 * x).sqrt())
        };
        let cone_sphere = |z: f64| {
            let x = -0.5 * z * z + 3.0 * z - 2.0;
            Point3::new(x, (z * z - x * x).sqrt(), z)
        };
        let cylinder_cone = |phi: f64| {
            let x = 1.0 + 2.0_f64.sqrt() * phi.cos();
            let z = 3.0 + 2.0_f64.sqrt() * phi.sin();
            Point3::new(x, (z * z - x * x).sqrt(), z)
        };
        let cone_cone = |t: f64| {
            let c = 2.0 * 2.0_f64.sqrt();
            let y = 4.0 * (c - t * t) / (t * t * t - c);
            let l = 4.0 + t * y;
            let h = y * y / l;
            Point3::new(0.5 * (l - h), y, 0.5 * (l + h))
        };
        [
            (
                [
                    sphere(Point3::new(-2.0, 0.0, 0.0), 4.0),
                    cylinder(Point3::ZERO, Vec3::new(0.0, 0.0, 1.0), 2.0),
                ],
                Point3::new(2.0, 0.0, 0.0),
                [cylinder_sphere(0.4), cylinder_sphere(1.2)],
            ),
            (
                [sphere(Point3::new(-2.0, 0.0, 6.0), 32.0_f64.sqrt()), cone],
                Point3::new(2.0, 0.0, 2.0),
                [cone_sphere(2.6), cone_sphere(3.4)],
            ),
            (
                [
                    cylinder(
                        Point3::new(1.0, 0.0, 3.0),
                        Vec3::new(0.0, 1.0, 0.0),
                        2.0_f64.sqrt(),
                    ),
                    cone,
                ],
                Point3::new(2.0, 0.0, 2.0),
                [
                    cylinder_cone(-PI / 4.0 + 0.3),
                    cylinder_cone(-PI / 4.0 + 0.9),
                ],
            ),
            (
                [
                    cone,
                    BlendQuadric {
                        point: Point3::new(2.0, -4.0, 2.0),
                        dir: Vec3::new(-0.5, std::f64::consts::FRAC_1_SQRT_2, 0.5),
                        ..cone
                    },
                ],
                Point3::new(2.0, 0.0, 2.0),
                [cone_cone(1.6), cone_cone(1.64)],
            ),
        ]
    }

    /// Each accepted mixed pair's quartic is identically on both quadrics in either chart.
    #[test]
    fn nodal_sphere_and_cone_pairs_satisfy_both_offset_identities() {
        for (offsets, node, ends) in other_nodal_pairs() {
            for q in offsets {
                assert!(q.distance(node) < 1e-14);
                for end in ends {
                    assert!(q.distance(end) < 1e-14);
                }
            }
            for reversed in [false, true] {
                let [a, b] = if reversed {
                    [offsets[1], offsets[0]]
                } else {
                    offsets
                };
                let mut blend = ExactBlend {
                    offsets: [a, b],
                    radius: 0.125,
                    ..ExactBlend::default()
                };
                super::super::classify_blend_spine(ends[0], 1e-7, &mut blend);
                assert_eq!(blend.spine, BlendSpine::NodalQuartic, "{a:?}, {b:?}");
                assert!(blend.point.distance(node) < 1e-14);
                for base in [false, true] {
                    for [from, to] in [ends, [ends[1], ends[0]]] {
                        let arc =
                            nodal_quartic(&a, &b, node, from - node, to - node, base).unwrap();
                        // The original nodal test's endpoint-correspondence bar: the
                        // supplied ray is rounded. These checks also exclude a constant
                        // node, which would satisfy both quadric equations vacuously.
                        for (t, expected) in [(0.0, from), (1.0, to)] {
                            let end = arc.segment(t, t);
                            let actual = v_eval(&end.m, 0.0) / eval(&end.w, 0.0);
                            assert!(
                                actual.distance(expected) < 1e-9,
                                "{a:?}, {b:?}, base {base}, end {t}"
                            );
                        }
                        for i in 0..16 {
                            let curve = segment_curve(
                                &arc.segment(f64::from(i) / 16.0, f64::from(i + 1) / 16.0),
                            );
                            assert!(
                                crate::surface::implicit::curve_on_quadrics(
                                    &curve,
                                    &[a.implicit(), b.implicit()]
                                )
                                .unwrap(),
                                "{a:?}, {b:?}, base {base}, segment {i}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The analytic support with the quadric's normal, plus a regular UV for classification.
    fn quadric_support(k: &Kernel, q: BlendQuadric) -> (Surface, Point2) {
        let axis = if q.kind == QuadricKind::Sphere {
            Vec3::new(0.0, 0.0, 1.0)
        } else {
            q.dir
        };
        let x = if axis.x.abs() < 0.9 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let x = (x - axis * x.dot(axis)).unitize().unwrap();
        let origin = if q.kind == QuadricKind::Cone {
            q.point + axis
        } else {
            q.point
        };
        let frame = Axis2Placement::new(origin, x, axis.cross(x));
        let mut surface = match q.kind {
            QuadricKind::Sphere => {
                Surface::from(Sphere::create_canonical(k, &frame, q.radius).unwrap())
            }
            QuadricKind::Cylinder => {
                Surface::from(Cone::create_canonical_cylinder(k, &frame, q.radius).unwrap())
            }
            QuadricKind::Cone => Surface::from(
                Cone::create_canonical_with_semi_angle(
                    k,
                    &frame,
                    q.angle.tan(),
                    q.angle * 180.0 / PI,
                    Some(6.0),
                    false,
                    false,
                )
                .unwrap(),
            ),
            _ => panic!("a mixed nodal support is a sphere, cylinder or cone"),
        };
        if q.side < 0.0 {
            surface.reverse(k, SurfParamType::U).unwrap();
        }
        let uv = surface.natural_uv_domain_with_nurb(k).unwrap().mid();
        let read = BlendQuadric::from_surface(k, &surface, uv, 1e-7).unwrap();
        assert_eq!(read.kind, q.kind);
        assert_eq!(read.side, q.side);
        (surface, uv)
    }

    /// Mixed pairs retain certified definitions for both contact directions and all normal signs.
    #[test]
    fn nodal_sphere_and_cone_rounds_keep_zero_bound_definitions() {
        let k = Kernel::default();
        let radius = 0.125;
        for (offsets, node, [from, to]) in other_nodal_pairs() {
            for swapped in [false, true] {
                let offsets = if swapped {
                    [offsets[1], offsets[0]]
                } else {
                    offsets
                };
                for sides in [[1.0, 1.0], [1.0, -1.0], [-1.0, 1.0], [-1.0, -1.0]] {
                    for toward in [1.0, -1.0] {
                        let offsets = [
                            BlendQuadric {
                                side: sides[0],
                                ..offsets[0]
                            },
                            BlendQuadric {
                                side: sides[1],
                                ..offsets[1]
                            },
                        ];
                        let contact = sides.map(|side| toward * side);
                        let signed = contact.map(|sign| -radius * sign);
                        let faces = [
                            offsets[0].offset(-signed[0], 1e-7),
                            offsets[1].offset(-signed[1], 1e-7),
                        ];
                        let [(a, uv_a), (b, uv_b)] = faces.map(|face| quadric_support(&k, face));
                        let inputs = ClassInputs {
                            uv: [uv_a, uv_b],
                            offsets: signed,
                            centre: from,
                            tol: 1e-7,
                            ball_on_collapsed: false,
                            radii_can_change: false,
                        };
                        let blend = classify_supports(&k, [&a, &b], &inputs).unwrap();
                        assert_eq!(
                            blend.spine,
                            BlendSpine::NodalQuartic,
                            "{offsets:?}, {contact:?}"
                        );
                        assert_eq!(blend.contact, contact);
                        assert_eq!(blend.faces.map(|face| face.side), sides);
                        assert!(blend.point.distance(node) < inputs.tol);
                        let arc = nodal_quartic(
                            &blend.offsets[0],
                            &blend.offsets[1],
                            blend.point,
                            from - blend.point,
                            to - blend.point,
                            false,
                        )
                        .unwrap();
                        let parts = rational_spine_parts(
                            &k,
                            &blend,
                            arc.segments(0.0, 1.0),
                            true,
                            inputs.tol,
                        )
                        .unwrap();
                        let mut net = parts.surface;
                        let center_line = parts.center_line.unwrap();
                        let CurveKind::BSpline(leader) = center_line.kind() else {
                            panic!("the nodal leader is a rational quartic");
                        };
                        let domain = net.natural_uv_domain_with_nurb(&k).unwrap();
                        let round = ExactRound::create(
                            &k,
                            ExactClass::NodalQuartic,
                            [&a, &b],
                            radius,
                            Some(leader),
                            inputs,
                        )
                        .unwrap();
                        if let Err(e) = round.recertify(&k, &net) {
                            panic!("{offsets:?}, {contact:?}: {e}");
                        }
                        net.set_exact_round(Some(round));
                        assert_eq!(
                            net.procedural_blend().unwrap().exact().unwrap().bound(),
                            0.0
                        );
                        assert_eq!(net.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
                        for i in 0..=4 {
                            let u = domain.evaluate(f64::from(i) / 4.0, 0.0).x;
                            let centre = leader.evaluate_point(u).unwrap();
                            for j in 0..=4 {
                                let p = net
                                    .evaluate_point(
                                        &k,
                                        Point2::new(u, domain.evaluate(0.0, f64::from(j) / 4.0).y),
                                    )
                                    .unwrap();
                                assert!((p.distance(centre) - radius).abs() < 1e-13);
                                if j == 0 {
                                    assert!(faces[0].distance(p) < 1e-13);
                                }
                                if j == 4 {
                                    assert!(faces[1].distance(p) < 1e-13);
                                }
                            }
                        }
                        let record = exact_to_record(&k, &net).unwrap();
                        let bytes = serde_json::to_vec(&record).unwrap();
                        let record = serde_json::from_slice(&bytes).unwrap();
                        let rebuilt = exact_rebuild(&k, &record).unwrap();
                        assert_eq!(
                            rebuilt.procedural_blend().unwrap().recertify(&k).unwrap(),
                            0.0
                        );
                    }
                }
            }
        }
    }
}
