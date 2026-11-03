//! The G1 fill of a setback corner beside curved faces (cylinders, cones, spheres), as a
//! graph over the corner's axis, certified no more concave than what each patch is G1 to
//! (solidcore#2343, built in solidcore from the SolidCraft fork's work in progress on
//! smlib#85 part C, tangency-labs/smlib 37c9e1d: `sm_SbGraphG1Curved`, `sm_SbGeometricRows`,
//! `sm_SbLineSpine`, `sm_SbRoundRowK`, `sm_SbTDetWith`, `sm_SbNormalField`,
//! `sm_SbNoMoreConcave`; no fork commit).
//!
//! As the planar fill ([`super::graph`]), with three changes where a face is curved (every
//! face of such a corner is taken this way, planes too):
//! - Each face's spring curve is [`super::spring::spring_curve`]'s, exactly on the face; its
//!   half from round m's rail end is setback patch m's side on that face (a fixed column). It
//!   is matched to the round there by a scale c, the Moebius weight ratio rho and the round's
//!   speed lambda off its end (`rho = w0 / (alpha - kappa mu)`, `lambda = d c rho kappa`, from
//!   the half's second control point `alpha (P, 1) + kappa nu'` and the round's
//!   along-derivative `mu (P, 1) + nu'`: both along the rail), then multiplied at the base
//!   point by the cubic `[1, 1, q2, q3]` that makes its last two weights 1 and keeps its first
//!   two control points (so the polynomial interior map meets it there): degree + 3.
//! - The column next to it is G1 to the face by construction (Hahn's connecting functions;
//!   Liu and Hoschek 1989; `DeRose` 1990): `column 1 = column 0 + (alpha T1 + beta T2 +
//!   gamma H0) / q`, T1 and T2 the face's parametric partials along the spring curve, alpha
//!   and beta polynomials, gamma cubic; chosen first (its rows 0, 1 the round's, its
//!   projections and weights at rows `N - 1`, N the interior map's, the rest the least second
//!   differences), then fixed. The interior map's heights there must equal the fixed
//!   columns': equalities, as a plane's were.
//! - The convexity conditions are the owner's ruling (a) on #85: each patch no more concave
//!   than what it is G1 to, `II + |k| I` PSD, k the most negative normal curvature of its faces
//!   and of its round's end row (0 where all are convex); sufficient, and affine in the
//!   heights: `w^3 s (l, m, n) + |k| o J` PSD, J the first fundamental form of the projection
//!   times `w^4` (`I >= I` of the projection; `w^4 |Pu x Pv| >= w |o|`). A setback patch beside
//!   a face where `k < 0` is certified by the faces' normal field ([`no_more_concave`]), exact
//!   along its columns.
//!
//! Each round's end rows come from its geometry ([`geometric_rows`]: the arc about its
//! ball's centre and its derivative across the end along the rails), not from its
//! parametrization; its raw jets give its own curvature ([`round_row_k`]).
//!
//! Not in the fork (the owner's answers on #2243, solidcore-native under the same flag):
//! - The programme also minimises the net's own thin-plate energy (`net_fairness_rows`):
//!   the surface's energy, taken at 8 x 8 Gauss points, does not see a high-degree net that
//!   rings between them, and the certificate's search raised the degree until such a net
//!   certified (net test #5 failing, the net 1.5 mm past a face).
//! - A patch is certified only with its net: never turning back (`check_brep`'s test #5,
//!   `net_monotone`) and contained (`net_contained`).
//! - Its rows are solved continued and violated only past their rounding
//!   (`dual_qp_rows`'s `native`), and its slack for rounding grows with the corner's
//!   distance from the origin (`Strict`).
//! - With `margin` (the growth naming a setback), every test of the certificate on the
//!   programme's solution must hold by more than its own rounding bound (`Strict`), so that
//!   the same corner built afresh (the programme the same, strictly convex, its solution
//!   the same to rounding) is certified too: the setback named builds when asked for.
//!
//! | Rust                          | Fork (37c9e1d)              |
//! |-------------------------------|-----------------------------|
//! | `graph_g1_curved` (crate)     | `sm_SbGraphG1Curved`        |
//! | `geometric_rows` (private)    | `sm_SbGeometricRows`        |
//! | `line_spine` (private)        | `sm_SbLineSpine`            |
//! | `round_row_k` (private)       | `sm_SbRoundRowK`            |
//! | `t_det_with` (private)        | `sm_SbTDetWith`             |
//! | `normal_field` (private)      | `sm_SbNormalField`          |
//! | [`no_more_concave`]           | `SmBlendSetback::NoMoreConcave`, `sm_SbNoMoreConcave` |
//! | [`round_end_curvature`]       | `SmBlendSetback::RoundEndCurvature` |
//!
//! | `net_fairness_rows` (private) | none (#2243)                |
//! | `net_monotone` (private)      | none (#2243)                |
//! | `net_contained` (private)     | none (#2243)                |
//!
//! Deviations: the fork's process-wide work counters (`CurvedCounts`) are the fill's
//! [`FillStats`]; its degree search is least degree first, as the fork's caller asks it
//! (`bLeastFirst`). The certificate's subdivision is bounded by [`GRAPH_DEPTH`]; no other
//! recursion.

use super::certify::{
    affine_fundamentals, affine_net, fundamentals, minor_without, t_axpy, t_det3, t_du, t_dv,
    t_make, t_mul_fast, t_nonneg, t_pieces, t_scale, AffinePatch,
};
use super::convexity::{GRAPH_DIRECTIONS, GRAPH_LEVELS};
use super::graph::{fill_from, GRAPH_DEPTH, P2};
use super::qp::{dual_qp_rows, QpOutcome};
use super::spring::spring_curve;
use super::{
    affine_solutions, along_line, axpy, bern_elevate, bern_mul, binomial, el, el_mut, el_ref,
    least_squares, monomials, over_patch, round_jets, second, sm_err, smax, smax_usize, smin, Bern,
    Face, FaceKind, Fill, FillStats, Hom, Net, Refusal, Round, RoundJets, Tensor, GW, GX, HOM_ZERO,
};
use crate::blend::fillet_exact::{
    classify_blend_spine, BlendQuadric, BlendSpine, ExactBlend, QuadricKind,
};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, PI};
use crate::nurbs::b_spline_surface::BSplineSurface;

/// A homogeneous point's Euclidean point.
fn pt(h: &Hom) -> Vec3 {
    Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])
}

/// Component `c` of homogeneous points, as Bernstein coefficients.
fn comp_of(v: &[Hom], c: usize) -> Result<Bern> {
    v.iter().map(|h| el(h, c)).collect()
}

/// The Bernstein mean of a row: its point at the middle, `sum_j C(q, j) / 2^q H_j`.
fn row_middle(row: &[Hom]) -> Result<Hom> {
    let q = row.len().checked_sub(1).ok_or_else(sm_err)?;
    let mut m = HOM_ZERO;
    for (j, h) in row.iter().enumerate() {
        m = axpy(&m, binomial(q, j)? / 2.0_f64.powf(q as f64), h);
    }
    Ok(m)
}

/// The circumcentre of three points (`B + ((b |a|^2 - a |b|^2) x (a x b)) / 2 |a x b|^2`,
/// `a = A - B'`, `b = M - B'` about the third), `None` where they are collinear.
fn circumcentre(a: Vec3, m: Vec3, b: Vec3) -> Option<Vec3> {
    let a0 = a - b;
    let b0 = m - b;
    let axb = a0.cross(b0);
    let axb2 = axb.dot(axb);
    if !(axb2 > 0.0) {
        return None;
    }
    Some(b + ((b0 * a0.dot(a0) - a0 * b0.dot(b0)).cross(axb)) * (0.5 / axb2))
}

/// A net's four components as polynomials.
fn comps(net: &Net) -> Result<[Tensor; 4]> {
    let mk = |c: usize| -> Result<Tensor> {
        let mut t = t_make(net.p, net.q);
        for (i, ti) in t.c.iter_mut().enumerate() {
            *ti = el(el_ref(&net.pts, i)?, c)?;
        }
        Ok(t)
    };
    Ok([mk(0)?, mk(1)?, mk(2)?, mk(3)?])
}

/// `w^4` times the first fundamental form of the net with the components `h`: `J_k` from
/// `X_u = w H_u - w_u H`, `X_v` likewise (`(Xu . Xu, Xu . Xv, Xv . Xv)`).
fn first_form(h: &[Tensor; 4]) -> Result<[Tensor; 3]> {
    let mut hu: [Tensor; 4] = Default::default();
    let mut hv: [Tensor; 4] = Default::default();
    for c in 0..4 {
        *el_mut(&mut hu, c)? = t_du(el_ref(h, c)?)?;
        *el_mut(&mut hv, c)? = t_dv(el_ref(h, c)?)?;
    }
    let [_, _, _, h3] = h;
    let mut xu: [Tensor; 3] = Default::default();
    let mut xv: [Tensor; 3] = Default::default();
    for c in 0..3 {
        *el_mut(&mut xu, c)? = t_axpy(
            &t_mul_fast(h3, el_ref(&hu, c)?)?,
            -1.0,
            &t_mul_fast(&hu[3], el_ref(h, c)?)?,
        )?;
        *el_mut(&mut xv, c)? = t_axpy(
            &t_mul_fast(h3, el_ref(&hv, c)?)?,
            -1.0,
            &t_mul_fast(&hv[3], el_ref(h, c)?)?,
        )?;
    }
    let mut j: [Tensor; 3] = Default::default();
    for (k, jk) in j.iter_mut().enumerate() {
        let pa = if k == 2 { &xv } else { &xu };
        let pb = if k == 0 { &xu } else { &xv };
        let mut s = t_mul_fast(&pa[0], &pb[0])?;
        for c in 1..3 {
            s = t_axpy(&s, 1.0, &t_mul_fast(el_ref(pa, c)?, el_ref(pb, c)?)?)?;
        }
        *jk = s;
    }
    Ok(j)
}

/// `-t`.
fn negated(t: &Tensor) -> Tensor {
    let mut n = t.clone();
    for x in &mut n.c {
        *x = -*x;
    }
    n
}

/// `s t`.
fn scaled(t: &Tensor, s: f64) -> Tensor {
    let mut n = t.clone();
    for x in &mut n.c {
        *x *= s;
    }
    n
}

/// The thin-plate energy of a net's own control points, as rows whose squares sum to it: the
/// functional of [`second`] with each derivative of the Bezier patch replaced by its net's
/// degree-scaled differences (`X_uu ~ p (p - 1) D2_u P`, `X_uv ~ p q D_u D_v P`,
/// `X_vv ~ q (q - 1) D2_v P`, the Euclidean points P), each difference standing for its
/// share of the unit square. For a net close to its surface (a low-degree surface raised)
/// it is close to the surface's energy; unlike the surface's at its Gauss points, it sees a
/// net that rings between them.
fn net_fairness_rows(net: &Net) -> Result<Vec<f64>> {
    let (p, q) = (net.p, net.q);
    let at = |i: usize, j: usize| -> Result<Vec3> {
        let h = el(&net.pts, i * (q + 1) + j)?;
        Ok(pt(&h))
    };
    let mut rows = Vec::new();
    let push = |rows: &mut Vec<f64>, c: f64, v: Vec3| {
        let w = sqrt(c);
        rows.push(w * v.x);
        rows.push(w * v.y);
        rows.push(w * v.z);
    };
    let (pf, qf) = (p as f64, q as f64);
    if p >= 2 {
        let c = pf * pf * (pf - 1.0) / (qf + 1.0);
        for i in 1..p {
            for j in 0..=q {
                let d = at(i + 1, j)? - at(i, j)? * 2.0 + at(i - 1, j)?;
                push(&mut rows, c, d);
            }
        }
    }
    if p >= 1 && q >= 1 {
        let c = 2.0 * pf * qf;
        for i in 0..p {
            for j in 0..q {
                let d = at(i + 1, j + 1)? - at(i + 1, j)? - at(i, j + 1)? + at(i, j)?;
                push(&mut rows, c, d);
            }
        }
    }
    if q >= 2 {
        let c = qf * qf * (qf - 1.0) / (pf + 1.0);
        for i in 0..=p {
            for j in 1..q {
                let d = at(i, j + 1)? - at(i, j)? * 2.0 + at(i, j - 1)?;
                push(&mut rows, c, d);
            }
        }
    }
    Ok(rows)
}

/// How strictly the fill's certificate is taken. Its tests allow rounding a slack of `1e-12`
/// of each quantity's scale (two orders above the coefficients' own rounding, about `1e-14`
/// of it), times `factor`, `1 + |o| / size`: the fill's inputs were made in world
/// coordinates, `|o|` from the origin (`world`), the corner `size` across, and carry
/// rounding of that size (#2343: a fill made again after its corner was moved far). With
/// `margin` (the growth naming a setback), half the slack: the certificate then holds by
/// half its slack more than it asks, far above the rounding by which the same corner built
/// afresh differs, so that build holds it too. (Not `a >= +eps`: a G1 patch's conditions
/// vanish exactly along its columns on a plane, where no margin is possible.)
#[derive(Debug, Clone, Copy)]
struct Strict {
    factor: f64,
    world: f64,
    margin: bool,
}

impl Strict {
    /// The plain certificate of a corner at the origin ([`no_more_concave`]'s).
    const PLAIN: Self = Self {
        factor: 1.0,
        world: 0.0,
        margin: false,
    };

    /// The slack allowed below 0 for a quantity of `scale`.
    fn eps(self, scale: f64) -> f64 {
        let eps = 1.0e-12 * self.factor * scale;
        if self.margin {
            0.5 * eps
        } else {
            eps
        }
    }
}

/// Whether a net's rows and columns never turn back: `check_brep`'s `SmBSplineSurface` test
/// #5 on its Euclidean points, the patch open both ways (a turn of more than 170 degrees
/// between neighbouring steps). With `margin`, also every turn below 170 degrees by more than
/// its rounding bound, `4 eps (|P| / |a| + |P| / |b|)` radians for steps a and b between
/// points of size up to |P| (in world coordinates).
fn net_monotone(net: &Net, strict: Strict) -> Result<bool> {
    let points: Vec<Point3> = net.pts.iter().map(pt).collect();
    if crate::surface::surface::assert_valid::grid_has_non_monotonic_points(
        &points,
        net.q + 1,
        false,
        false,
    )? {
        return Ok(false);
    }
    if !strict.margin {
        return Ok(true);
    }
    let (p, q) = (net.p, net.q);
    let at = |i: usize, j: usize| el(&points, i * (q + 1) + j);
    let limit = 170.0 * PI / 180.0;
    for (outer, inner, along_rows) in [(p + 1, q + 1, true), (q + 1, p + 1, false)] {
        for o in 0..outer {
            let point = |t: usize| if along_rows { at(o, t) } else { at(t, o) };
            let mut size = strict.world;
            for t in 0..inner {
                size = smax(size, point(t)?.length() + strict.world);
            }
            for t in 2..inner {
                let a = point(t - 1)? - point(t - 2)?;
                let b = point(t)? - point(t - 1)?;
                let (la, lb) = (a.length(), b.length());
                if !(la > 0.0 && lb > 0.0) {
                    continue;
                }
                let turn = b.angle_between(a).unwrap_or(0.0);
                let bound = 4.0 * f64::EPSILON * (size / la + size / lb);
                if turn + bound > limit {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Whether a patch's net is contained (#2343; owner on #2243): every control point on the
/// fill's side of every face (`convex`: in the material; else out of it), past a plane by at
/// most `tol`, past a curved face by at most `tol` plus how far that face's tangent planes
/// leave it within the net, `kappa L^2 / 2` (kappa its largest normal curvature there, L the
/// diagonal of the net's box): where the patch is G1 to a curved face, its next column lies on
/// those tangent planes by construction. With `margin`, by more than the rounding of the
/// distances, `16 eps` times the points' and faces' size in world coordinates.
fn net_contained(
    net: &Net,
    faces: &[Face],
    convex: bool,
    tol: f64,
    strict: Strict,
) -> Result<bool> {
    let side = if convex { 1.0 } else { -1.0 };
    let points: Vec<Point3> = net.pts.iter().map(pt).collect();
    let first = *points.first().ok_or_else(sm_err)?;
    let (mut lo, mut hi) = (first, first);
    for x in &points {
        lo = Vec3::new(smin(lo.x, x.x), smin(lo.y, x.y), smin(lo.z, x.z));
        hi = Vec3::new(smax(hi.x, x.x), smax(hi.y, x.y), smax(hi.z, x.z));
    }
    let diagonal = (hi - lo).length();
    for face in faces {
        let mut allowed = tol;
        if face.kind != FaceKind::Plane {
            let kappa = if face.kind == FaceKind::Cone {
                let mut hmin = BIG_DOUBLE;
                for x in &points {
                    hmin = smin(hmin, (*x - face.point).dot(face.axis));
                }
                let rho = face.radius + face.slope * hmin;
                if !(rho > 0.0) {
                    return Ok(false);
                }
                1.0 / (rho * sqrt(1.0 + face.slope * face.slope))
            } else {
                1.0 / face.radius
            };
            allowed += 0.5 * kappa * diagonal * diagonal;
        }
        for x in &points {
            let past = side * face.distance(*x).0;
            let rounding = if strict.margin {
                16.0 * f64::EPSILON * ((*x - face.point).length() + face.radius + strict.world)
            } else {
                0.0
            };
            if past + rounding > allowed {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The sign `t` is certified to have over the whole square (1 or -1, to `strict`'s slack),
/// 0 where neither.
fn certified_sign(t: &Tensor, strict: Strict) -> Result<f64> {
    let eps = strict.eps(t_scale(t));
    Ok(if t_nonneg(t, eps, GRAPH_DEPTH)? == 1 {
        1.0
    } else if t_nonneg(&negated(t), eps, GRAPH_DEPTH)? == 1 {
        -1.0
    } else {
        0.0
    })
}

/// `sm_SbTDetWith`: `det(H, Hu, Hv, (V, 0))` of a homogeneous net and a polynomial vector
/// field V (3 components).
fn t_det_with(net: &Net, v: &[Tensor; 3]) -> Result<Tensor> {
    let h = comps(net)?;
    let mut hu: [Tensor; 4] = Default::default();
    let mut hv: [Tensor; 4] = Default::default();
    for c in 0..4 {
        *el_mut(&mut hu, c)? = t_du(el_ref(&h, c)?)?;
        *el_mut(&mut hv, c)? = t_dv(el_ref(&h, c)?)?;
    }
    let mut s = t_make(0, 0);
    for (c, vc) in v.iter().enumerate() {
        let pm = minor_without([&h, &hu, &hv], c)?;
        let sign = if (3 + c) % 2 == 0 { 1.0 } else { -1.0 };
        s = t_axpy(&s, sign, &t_mul_fast(vc, &t_det3(pm)?)?)?;
    }
    Ok(s)
}

/// `sm_SbNormalField`: a setback patch's normal field (ruling (a)'s certificate): V (3
/// components) and `g > 0` with `V / g = (1 - v) N_A + v N_B`, `N_F` the unit normal field of
/// face F (`a` on the column `v = 0`, `b` on `v = 1`) from F's implicit form at the patch's
/// own points, turned to e's side: `N_F = G_F / q_F`, `G_F` and `q_F` polynomial in the net,
/// `|G_F| = q_F` wherever the point is on F. Cleared: `V = (1 - v) q_B G_A + v q_A G_B`,
/// `g = q_A q_B`. Along a column where the patch is G1 to its face, `V / g` is the patch's
/// unit normal there.
fn normal_field(net: &Net, a: &Face, b: &Face, e: Vec3) -> Result<([Tensor; 3], Tensor)> {
    let mut gs: [[Tensor; 3]; 2] = Default::default();
    let mut sq: [Tensor; 2] = Default::default();
    for (side, (gse, sqe)) in gs.iter_mut().zip(sq.iter_mut()).enumerate() {
        let face = if side == 0 { a } else { b };
        let j = if side == 0 { 0 } else { net.q };
        if face.kind == FaceKind::Plane {
            for (c, g) in gse.iter_mut().enumerate() {
                *g = t_make(0, 0);
                *el_mut(&mut g.c, 0)? = face.normal.component(c);
            }
            *sqe = t_make(0, 0);
            *el_mut(&mut sqe.c, 0)? = 1.0;
        } else {
            let dp = [face.point.x, face.point.y, face.point.z];
            let da = [face.axis.x, face.axis.y, face.axis.z];
            let mut su: [Tensor; 3] = Default::default();
            let mut sw = t_make(net.p, net.q);
            let mut sh = t_make(net.p, net.q);
            for (i, w) in sw.c.iter_mut().enumerate() {
                *w = el_ref(&net.pts, i)?[3];
            }
            for (c, u) in su.iter_mut().enumerate() {
                *u = t_make(net.p, net.q);
                for (i, ui) in u.c.iter_mut().enumerate() {
                    *ui = el(el_ref(&net.pts, i)?, c)? - el(&sw.c, i)? * el(&dp, c)?;
                }
                for (hi, &ui) in sh.c.iter_mut().zip(&u.c) {
                    *hi += ui * el(&da, c)?;
                }
            }
            if face.kind == FaceKind::Sphere {
                *gse = su;
                *sqe = t_axpy(&t_make(net.p, net.q), face.radius, &sw)?;
            } else {
                // cylinder (slope 0) or cone: rho w = R w + slope h; G = U - h a - slope rho w
                // a, |G| = sqrt(1 + slope^2) rho w on the face: the constant kept in q
                let rho = t_axpy(
                    &t_axpy(&t_make(net.p, net.q), face.radius, &sw)?,
                    face.slope,
                    &sh,
                )?;
                for (c, g) in gse.iter_mut().enumerate() {
                    let dac = el(&da, c)?;
                    *g = t_axpy(
                        &t_axpy(el_ref(&su, c)?, -dac, &sh)?,
                        -face.slope * dac,
                        &rho,
                    )?;
                }
                *sqe = t_axpy(
                    &t_make(net.p, net.q),
                    sqrt(1.0 + face.slope * face.slope),
                    &rho,
                )?;
            }
        }
        // its side: G . e > 0 at the column's middle
        let mut dot = 0.0;
        for (c, g) in gse.iter().enumerate() {
            let jj = if g.q == 0 { 0 } else { j };
            let mut mid = 0.0;
            for i in 0..=g.p {
                mid += el(&g.c, i * (g.q + 1) + jj)? * binomial(g.p, i)?;
            }
            dot += mid * e.component(c);
        }
        if dot < 0.0 {
            for g in gse.iter_mut() {
                for x in &mut g.c {
                    *x = -*x;
                }
            }
        }
    }
    let lo = Tensor {
        p: 0,
        q: 1,
        c: vec![1.0, 0.0],
    };
    let hi = Tensor {
        p: 0,
        q: 1,
        c: vec![0.0, 1.0],
    };
    let [g0, g1] = &gs;
    let [q0, q1] = &sq;
    let mut v: [Tensor; 3] = Default::default();
    for (c, vc) in v.iter_mut().enumerate() {
        *vc = t_axpy(
            &t_mul_fast(&t_mul_fast(&lo, q1)?, el_ref(g0, c)?)?,
            1.0,
            &t_mul_fast(&t_mul_fast(&hi, q0)?, el_ref(g1, c)?)?,
        )?;
    }
    Ok((v, t_mul_fast(q0, q1)?))
}

/// `sm_SbNoMoreConcave`: ruling (a)'s certificate on a patch G1 to its faces `a` (along
/// `v = 0`) and `b` (`v = 1`): `II >= k I` (`k < 0`), II with the unit normal toward e's side.
/// With `Phi = w^3 |Pu x Pv|` and any `Q > 0`, `Phi >= 2 Q Phi^2 / (Q^2 + Phi^2)`
/// (`(Phi - Q)^2 >= 0`), equal where `Q = Phi`. `Q = s ot / g`, `ot = det(H, Hu, Hv, (V, 0))`
/// (V, g: [`normal_field`]), certified one-signed (s): `Q = Phi` along a column where the
/// patch is G1 to its face. Cleared of `(ot^2 + g^2 Phi^2) w^2 > 0`, with
/// `S = w^2 Phi^2 = J0 J2 - J1^2` (J the net's first fundamental form times `w^4`):
/// `s [D w^3 (l, m, n) + 2 |k| ot g S J]` PSD, `D = w^2 ot^2 + g^2 S`, certified as `s C11`,
/// `s C22`, `C11 C22 - C12^2 >= 0`. 1 certified, 0 a negative value found, -1 undecided (ot
/// not certified one-signed counts as undecided).
fn no_more_concave_net(
    net: &Net,
    a: &Face,
    b: &Face,
    e: Vec3,
    k: f64,
    strict: Strict,
) -> Result<i32> {
    let f = fundamentals(net, e)?;
    let (v, g) = normal_field(net, a, b, e)?;
    let ot = t_det_with(net, &v)?;
    let s = certified_sign(&ot, strict)?;
    if s == 0.0 {
        return Ok(-1);
    }
    let h = comps(net)?;
    let j = first_form(&h)?;
    let w2 = t_mul_fast(&h[3], &h[3])?;
    let ss = t_axpy(&t_mul_fast(&j[0], &j[2])?, -1.0, &t_mul_fast(&j[1], &j[1])?)?;
    let d = t_axpy(
        &t_mul_fast(&w2, &t_mul_fast(&ot, &ot)?)?,
        1.0,
        &t_mul_fast(&t_mul_fast(&g, &g)?, &ss)?,
    )?;
    let dw3 = t_mul_fast(&d, &t_mul_fast(&w2, &h[3])?)?;
    let lift = t_mul_fast(&t_mul_fast(&ot, &g)?, &ss)?;
    // each entry's rounding is that of its two terms (where II + |k| I is singular they
    // cancel)
    let mut cm: [Tensor; 3] = Default::default();
    let mut term = [0.0; 3];
    for kk in 0..3 {
        let sa = t_mul_fast(&dw3, el_ref(&f, kk)?)?;
        let sb = t_mul_fast(&lift, el_ref(&j, kk)?)?;
        *el_mut(&mut term, kk)? = t_scale(&sa) + 2.0 * k.abs() * t_scale(&sb);
        *el_mut(&mut cm, kk)? = t_axpy(&sa, -2.0 * k, &sb)?;
    }
    let sl = scaled(&cm[0], s);
    let sn = scaled(&cm[2], s);
    let disc = t_axpy(
        &t_mul_fast(&cm[0], &cm[2])?,
        -1.0,
        &t_mul_fast(&cm[1], &cm[1])?,
    )?;
    let eps = [
        strict.eps(term[0]),
        strict.eps(term[2]),
        strict.eps(term[0] * term[2] + term[1] * term[1]),
    ];
    let mut result = 1;
    for (check, &ep) in [&sl, &sn, &disc].into_iter().zip(&eps) {
        let non = t_nonneg(check, ep, GRAPH_DEPTH)?;
        if non == 0 {
            return Ok(0);
        }
        result = if non < result { non } else { result };
    }
    Ok(result)
}

/// `SmBlendSetback::NoMoreConcave`: the owner's ruling (a) on #85 on one setback patch (a
/// single Bezier span, as [`super::fill_hole`] makes them) G1 to `a` along its `v = 0` and to
/// `b` along `v = 1`: whether `II >= k I` (`k < 0`) over the whole patch, II taken with the
/// unit normal on `into`'s side (into the fill's convex side), certified by the faces' normal
/// field (exact along the columns): `Some(true)` certified, `Some(false)` refused (a value
/// below), `None` undecided.
///
/// # Errors
/// `SM_ERR_INVALID_INPUT` for a patch of more than one span or a k that is not negative.
pub fn no_more_concave(
    patch: &BSplineSurface,
    a: &Face,
    b: &Face,
    into: Vec3,
    k: f64,
) -> Result<Option<bool>> {
    let c = patch.canonical()?;
    let p = patch.degree(crate::nurbs::b_spline_surface::SurfParamType::U);
    let q = patch.degree(crate::nurbs::b_spline_surface::SurfParamType::V);
    if c.u_knots.len() != 2
        || c.v_knots.len() != 2
        || !(k < 0.0)
        || c.control_points.len() != (p + 1) * (q + 1)
    {
        return Err(crate::error::SolidError::invalid_input(
            "blend::blend_setback::no_more_concave",
            "a patch of one span and a negative k",
        ));
    }
    let mut pts = Vec::with_capacity(c.control_points.len());
    for (i, x) in c.control_points.iter().enumerate() {
        let w = if c.weights.len() == c.control_points.len() {
            el(&c.weights, i)?
        } else {
            1.0
        };
        pts.push([x.x * w, x.y * w, x.z * w, w]);
    }
    let net = Net { p, q, pts };
    Ok(
        match no_more_concave_net(&net, a, b, into, k, Strict::PLAIN)? {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        },
    )
}

/// `sm_SbGeometricRows`: a round's end rows from its geometry (owner, section 24): `jets`
/// (its jets at its end, 0 and 1) replaced by its end section as the rational quadratic arc
/// about its ball's centre (raised to the degree of the derivative's) and its derivative
/// across the end `T + beta(t) V(t)` in the round's tangent plane (T the unit normal of the
/// section's plane toward the rails' increasing parameter, V the arc's homogeneous tangent,
/// beta linear, set so that it runs along the rails at the ends): two parametrizations of
/// one round give the same rows. Returns the rows' degree.
fn geometric_rows(jets: &mut Vec<Vec<Hom>>, q: usize) -> core::result::Result<usize, Refusal> {
    let j0 = el_ref(jets, 0)?.clone();
    let j1 = el_ref(jets, 1)?.clone();
    let a = pt(&el(&j0, 0)?);
    let m = pt(&row_middle(&j0)?);
    let b = pt(&el(&j0, q)?);
    let not_arc =
        || Refusal::invalid("SmBlendSetback: a round's end section is not a circular arc");
    let c = circumcentre(a, m, b).ok_or_else(not_arc)?;
    let ua = a - c;
    let rr = ua.length();
    let ua = ua.unitize()?;
    let ub = (b - c).unitize()?;
    let cos = ua.dot(ub);
    if !(cos > -1.0 + 1.0e-9) {
        return Err(Refusal::invalid(
            "SmBlendSetback: a round's end section spans half its ball or more",
        ));
    }
    let mut t = ua.cross(ub).unitize()?;
    // T toward the round's rails' increasing parameter (its raw derivative along, at the
    // middle)
    let dm = row_middle(&j1)?;
    let dx = Vec3::new(
        dm[0] - dm[3] * m.x,
        dm[1] - dm[3] * m.y,
        dm[2] - dm[3] * m.z,
    );
    if dx.dot(t) < 0.0 {
        t *= -1.0;
    }
    let w1 = sqrt(0.5 * (1.0 + cos));
    let p1 = c + (ua + ub) * (rr / (1.0 + cos));
    // the arc, degree 2, and its homogeneous tangent V = w H' - w' H (spatial, degree 2
    // after the cancellation, computed as degree 3)
    let pts = [a, p1, b];
    let dw = [1.0, w1, 1.0];
    let mut hc: [Bern; 4] = Default::default();
    for (cc, hcc) in hc.iter_mut().enumerate() {
        for (x, &wj) in pts.iter().zip(&dw) {
            hcc.push(if cc == 3 { wj } else { x.component(cc) * wj });
        }
    }
    let deriv = |bb: &[f64]| -> Bern {
        let n = bb.len().saturating_sub(1);
        bb.iter()
            .zip(bb.iter().skip(1))
            .map(|(x0, x1)| n as f64 * (x1 - x0))
            .collect()
    };
    let mut sv: [Bern; 3] = Default::default();
    for (cc, svc) in sv.iter_mut().enumerate() {
        let t1 = bern_mul(&hc[3], &deriv(el_ref(&hc, cc)?))?;
        let t2 = bern_mul(&deriv(&hc[3]), el_ref(&hc, cc)?)?;
        *svc = t1.iter().zip(&t2).map(|(x, y)| x - y).collect();
    }
    // the derivative across the end: T + beta(t) V(t), beta linear, its ends set so that it
    // runs along the rails there (the raw derivative's direction at each rail end: the
    // rail's tangent, a direction of the geometry): T + beta V in the round's tangent plane
    // all along (G1)
    let mut beta = [0.0; 2];
    for (e, be) in beta.iter_mut().enumerate() {
        let col = if e == 0 { 0 } else { q };
        let h0 = el(&j0, col)?;
        let h1 = el(&j1, col)?;
        let p0 = pt(&h0);
        let d1 = Vec3::new(
            h1[0] - h1[3] * p0.x,
            h1[1] - h1[3] * p0.y,
            h1[2] - h1[3] * p0.z,
        );
        let i = if e == 0 { 0 } else { sv[0].len() - 1 };
        let v0 = Vec3::new(el(&sv[0], i)?, el(&sv[1], i)?, el(&sv[2], i)?);
        let along = d1.dot(t);
        if !(along > 0.0) {
            return Err(Refusal::invalid(
                "SmBlendSetback: a round's rail turns back across its end section",
            ));
        }
        *be = d1.dot(v0) / (v0.dot(v0) * along);
    }
    let deg = sv[0].len(); // beta V's degree
    let mut row = vec![HOM_ZERO; deg + 1];
    let mut across = vec![HOM_ZERO; deg + 1];
    for (cc, hcc) in hc.iter().enumerate() {
        let up = bern_elevate(hcc, deg)?;
        for (rj, &x) in row.iter_mut().zip(&up) {
            *el_mut(rj, cc)? = x;
        }
    }
    for (cc, svc) in sv.iter().enumerate() {
        let bv = bern_mul(&beta, svc)?;
        let tc = t.component(cc);
        for (aj, &x) in across.iter_mut().zip(&bv) {
            *el_mut(aj, cc)? = tc + x;
        }
    }
    for aj in &mut across {
        aj[3] = 0.0;
    }
    *jets = vec![row, across];
    Ok(deg)
}

/// `sm_SbLineSpine`: whether a constant round between faces `a` (its row's start `ends[0]`
/// on it) and `b` (`ends[1]`) has a line for its spine, by the one classification
/// ([`classify_blend_spine`]): the faces as their natural quadrics, offset through the
/// ball's centre (the row's circumcentre).
fn line_spine(a: &Face, b: &Face, ends: [Point3; 2], row: &[Hom], tol: f64) -> Result<bool> {
    let mid = row_middle(row)?;
    let Some(c) = circumcentre(ends[0], pt(&mid), ends[1]) else {
        return Ok(false);
    };
    let mut blend = ExactBlend {
        radius: (ends[0] - c).length(),
        ..ExactBlend::default()
    };
    for (e, face) in [a, b].into_iter().enumerate() {
        let mut quadric = BlendQuadric::default();
        match face.kind {
            FaceKind::Plane => {
                quadric.kind = QuadricKind::Plane;
                quadric.point = face.point;
                quadric.dir = face.normal;
            }
            FaceKind::Sphere => {
                quadric.kind = QuadricKind::Sphere;
                quadric.point = face.point;
                quadric.radius = face.radius;
                quadric.side = face.side;
            }
            FaceKind::Cylinder => {
                quadric.kind = QuadricKind::Cylinder;
                quadric.point = face.point;
                quadric.dir = face.axis;
                quadric.radius = face.radius;
                quadric.side = face.side;
            }
            FaceKind::Cone => {
                // the apex where R + slope h = 0, the axis into the nappe the face is on
                quadric.kind = QuadricKind::Cone;
                quadric.point = face.point - face.axis * (face.radius / face.slope);
                quadric.dir = face.axis * if face.slope > 0.0 { 1.0 } else { -1.0 };
                quadric.angle = face.slope.abs().atan();
                quadric.side = face.side;
            }
        }
        let end = el(&ends, e)?;
        let n = quadric.normal(end);
        let off = (c - end).dot(n);
        *el_mut(&mut blend.contact, e)? = if off < 0.0 { 1.0 } else { -1.0 };
        *el_mut(&mut blend.faces, e)? = quadric;
        *el_mut(&mut blend.offsets, e)? = quadric.offset(off, tol);
    }
    classify_blend_spine(c, tol, &mut blend);
    Ok(blend.spine == BlendSpine::Line)
}

/// `sm_SbRoundRowK`: a round's least normal curvature along its end row (the owner's ruling
/// (a), the rounds' part of k), II with the unit normal toward e's side, to rounding. `jets`:
/// its jets there (0 ..= 2), the row a circular arc (a constant round's section about its
/// ball's centre C, radius r: its unit normal `(P - C) / r`, exactly, so `|Pu x Pv|` is
/// polynomial there). On the degree 2 net with those jets at `u = 0` (`P0 = H`,
/// `P1 = H + Hs / 2`, `P2 = H + Hs + Hss / 2`) restricted to `u = 0`: k is certified a lower
/// bound where `s w^4 r (l, m, n) - k sigma Dc J` is PSD on the row (`sigma Dc =
/// |det(H, Hs, Ht, (H_xyz - w C, 0))|`, s the side of o); the largest such k (bisection to
/// `1e-13` of it, each step certified) is taken, 0 where the row is certified not concave (an
/// end on a plane face exactly 0 along its rail, `plane_end`; a line spine's round, a
/// cylinder, 0 along its rails everywhere, `line`).
fn round_row_k(
    jets: &[Vec<Hom>],
    e: Vec3,
    plane_end: [bool; 2],
    line: bool,
) -> core::result::Result<f64, Refusal> {
    if jets.len() < 3 {
        return Err(sm_err().into());
    }
    let (j0, j1, j2) = (el_ref(jets, 0)?, el_ref(jets, 1)?, el_ref(jets, 2)?);
    let q = j0.len().checked_sub(1).ok_or_else(sm_err)?;
    let mut pts = vec![HOM_ZERO; 3 * (q + 1)];
    for j in 0..=q {
        let (h, hs, hss) = (el(j0, j)?, el(j1, j)?, el(j2, j)?);
        for c in 0..4 {
            let (x, xs, xss) = (el(&h, c)?, el(&hs, c)?, el(&hss, c)?);
            *el_mut(el_mut(&mut pts, j)?, c)? = x;
            *el_mut(el_mut(&mut pts, (q + 1) + j)?, c)? = x + 0.5 * xs;
            *el_mut(el_mut(&mut pts, 2 * (q + 1) + j)?, c)? = x + xs + 0.5 * xss;
        }
    }
    let net = Net { p: 2, q, pts };
    // the arc's centre: the circumcentre of its ends and its middle
    let at = |t: f64| -> Result<Vec3> {
        let mut h = HOM_ZERO;
        for (j, hj) in j0.iter().enumerate() {
            h = axpy(
                &h,
                binomial(q, j)? * t.powf(j as f64) * (1.0 - t).powf((q - j) as f64),
                hj,
            );
        }
        Ok(pt(&h))
    };
    let not_arc =
        || Refusal::invalid("SmBlendSetback: a round's end section is not a circular arc");
    let c = circumcentre(at(0.0)?, at(0.5)?, at(1.0)?).ok_or_else(not_arc)?;
    let r = (at(0.0)? - c).length();
    let mut sw = t_make(2, q);
    for (i, w) in sw.c.iter_mut().enumerate() {
        *w = el_ref(&net.pts, i)?[3];
    }
    let mut su: [Tensor; 3] = Default::default();
    for (cc, u) in su.iter_mut().enumerate() {
        let dc = c.component(cc);
        *u = t_make(2, q);
        for (i, ui) in u.c.iter_mut().enumerate() {
            let h = el(&net.pts, i)?;
            *ui = el(&h, cc)? - h[3] * dc;
        }
    }
    let row = |t: &Tensor| -> Result<Tensor> {
        let mut r = t_make(0, t.q);
        for (j, x) in r.c.iter_mut().enumerate() {
            *x = el(&t.c, j)?;
        }
        Ok(r)
    };
    {
        // on the sphere about C of radius r along the row (the section a circular arc)
        let mut sq = t_make(0, 0);
        for u in &su {
            let ru = row(u)?;
            sq = t_axpy(&sq, 1.0, &t_mul_fast(&ru, &ru)?)?;
        }
        let rw = row(&sw)?;
        let r2 = t_mul_fast(&rw, &rw)?;
        let scale = t_scale(&sq) + r * r * t_scale(&r2);
        sq = t_axpy(&sq, -r * r, &r2)?;
        if t_scale(&sq) > 1.0e-11 * scale {
            return Err(not_arc());
        }
    }
    let f = fundamentals(&net, e)?;
    let dc = row(&t_det_with(&net, &su)?)?;
    let o = row(&f[3])?;
    let s = certified_sign(&o, Strict::PLAIN)?;
    let sigma = certified_sign(&dc, Strict::PLAIN)?;
    if s == 0.0 || sigma == 0.0 {
        return Err(Refusal::invalid(
            "SmBlendSetback: a round's end section turns across the corner's axis",
        ));
    }
    let h = comps(&net)?;
    let jj = first_form(&h)?;
    let w4 = row(&t_mul_fast(
        &t_mul_fast(&h[3], &h[3])?,
        &t_mul_fast(&h[3], &h[3])?,
    )?)?;
    let mut sa: [Tensor; 3] = Default::default();
    let mut sb: [Tensor; 3] = Default::default();
    for k in 0..3 {
        *el_mut(&mut sa, k)? = scaled(&t_mul_fast(&w4, &row(el_ref(&f, k)?)?)?, s * r);
        *el_mut(&mut sb, k)? = scaled(&t_mul_fast(&dc, &row(el_ref(&jj, k)?)?)?, sigma);
    }
    // an end of the row on a PLANE face: there the round is G1 to the plane along its rail,
    // so its second fundamental form on the rail's direction is the plane's, zero
    // (Meusnier): its (rail, rail) and (rail, across) entries are 0 at that end exactly, set
    // so (the end's Bernstein coefficient is the value there)
    for (e, &on_plane) in plane_end.iter().enumerate() {
        if on_plane {
            for sak in sa.iter_mut().take(2) {
                let i = if e == 0 { 0 } else { sak.c.len() - 1 };
                *el_mut(&mut sak.c, i)? = 0.0;
            }
        }
    }
    // a round whose spine is a line (two planes; a plane or cylinder parallel to a
    // cylinder's axis) is a cylinder, its rails its rulings: those entries are 0 all along
    if line {
        for sak in sa.iter_mut().take(2) {
            sak.c.fill(0.0);
        }
    }
    // whether sA - k sB is certified PSD on the row
    let certified = |k: f64| -> Result<bool> {
        let mut cm: [Tensor; 3] = Default::default();
        let mut dt = [0.0; 3];
        for i in 0..3 {
            *el_mut(&mut cm, i)? = t_axpy(el_ref(&sa, i)?, -k, el_ref(&sb, i)?)?;
            *el_mut(&mut dt, i)? = t_scale(el_ref(&sa, i)?) + k.abs() * t_scale(el_ref(&sb, i)?);
        }
        let disc = t_axpy(
            &t_mul_fast(&cm[0], &cm[2])?,
            -1.0,
            &t_mul_fast(&cm[1], &cm[1])?,
        )?;
        Ok(t_nonneg(&cm[0], 1.0e-12 * dt[0], GRAPH_DEPTH)? == 1
            && t_nonneg(&cm[2], 1.0e-12 * dt[2], GRAPH_DEPTH)? == 1
            && t_nonneg(
                &disc,
                1.0e-12 * (dt[0] * dt[2] + dt[1] * dt[1]),
                GRAPH_DEPTH,
            )? == 1)
    };
    if certified(0.0)? {
        return Ok(0.0);
    }
    let mut lo = -1.0 / r;
    let mut hi = 0.0;
    for _ in 0..64 {
        if certified(lo)? {
            break;
        }
        hi = lo;
        lo *= 2.0;
    }
    if !certified(lo)? {
        return Err(Refusal::sm_err(
            "SmBlendSetback: a round's least normal curvature at its end cannot be bounded",
        ));
    }
    while hi - lo > 1.0e-13 * lo.abs() {
        let mid = 0.5 * (lo + hi);
        if certified(mid)? {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

/// `SmBlendSetback::RoundEndCurvature`: a constant round's least normal curvature along its
/// end section (its `end`), II with the unit normal toward `into`, to rounding (the rounds'
/// part of ruling (a)'s k); 0 where the section is nowhere concave.
///
/// # Errors
/// [`Refusal`] where the section is not a circular arc, or its jets cannot be taken.
pub fn round_end_curvature(
    kernel: &Kernel,
    round: &Round<'_>,
    into: Vec3,
) -> core::result::Result<f64, Refusal> {
    let RoundJets { h, .. } = round_jets(kernel, round, 2)?;
    round_row_k(&h, into, [false, false], false)
}

/// A spring half matched to its round and to the interior map (`Side`).
#[derive(Debug, Clone, Default)]
struct Side {
    /// `m_vCol`: the column, degree `deg`.
    col: Vec<Hom>,
    /// `m_lDeg`.
    deg: usize,
    /// `m_vT`: the face's partials along it (Moebius applied).
    t: [Vec<Hom>; 2],
    /// `m_dLambda`: the round's speed off its end there.
    lambda: f64,
    /// `m_sD`: the interior map's field there (the column's derivative at the base point,
    /// negated, in the plane).
    d: P2,
}

/// What the corner's curved G1 fill is built from (`sm_SbGraphG1Curved`'s arguments).
pub(super) struct CurvedInput<'a, 'r> {
    /// `crWorldFaces`.
    pub faces: &'a [Face],
    /// `crRounds`.
    pub rounds: &'a [Round<'r>],
    /// `crWorldJets` (to order 2).
    pub jets: &'a [Vec<Vec<Hom>>],
    /// `crQIn`.
    pub qs: &'a [usize],
    /// `crRound`.
    pub round: &'a [usize],
    /// `crFace`.
    pub face: &'a [usize],
    /// `crRev`.
    pub rev: &'a [bool],
    /// `dTol`.
    pub tol: f64,
    /// Whether the certificate must hold by more than its own rounding bound (each test's
    /// slack halved: the growth naming a setback, #2343).
    pub margin: bool,
}

/// `sm_SbGraphG1Curved`: the G1 fill beside curved faces (module docs), least degree first.
#[allow(clippy::too_many_lines)] // the fork's one function
pub(super) fn graph_g1_curved(
    input: &CurvedInput<'_, '_>,
    stats: &mut FillStats,
) -> core::result::Result<Fill, Refusal> {
    let CurvedInput {
        faces: world_faces,
        rounds,
        jets: world_jets,
        qs: q_in,
        round: cycle_round,
        face: cycle_face,
        rev: cycle_rev,
        tol,
        margin,
    } = *input;
    let n = cycle_round.len();
    let mut qs: Vec<usize> = q_in.to_vec();
    // the frame about the rails' ends' centre (rounding of the corner's size, not its place)
    let mut o = Vec3::ZERO;
    {
        let mut count = 0usize;
        for (r, jr) in world_jets.iter().enumerate() {
            let j0 = el_ref(jr, 0)?;
            for e in 0..2 {
                let h = el(j0, if e == 0 { 0 } else { el(&qs, r)? })?;
                o += pt(&h);
                count += 1;
            }
        }
        o *= 1.0 / count as f64;
    }
    // the inputs' rounding relative to the corner (Strict)
    let strict = {
        let mut size = 0.0;
        for (r, jr) in world_jets.iter().enumerate() {
            let j0 = el_ref(jr, 0)?;
            for e in 0..2 {
                let h = el(j0, if e == 0 { 0 } else { el(&qs, r)? })?;
                size = smax(size, (pt(&h) - o).length());
            }
        }
        if !(size > 0.0) {
            return Err(sm_err().into());
        }
        Strict {
            factor: 1.0 + o.length() / size,
            world: o.length(),
            margin,
        }
    };
    let mut jets: Vec<Vec<Vec<Hom>>> = world_jets.to_vec();
    for jr in &mut jets {
        for jk in jr.iter_mut() {
            for h in jk.iter_mut() {
                h[0] -= h[3] * o.x;
                h[1] -= h[3] * o.y;
                h[2] -= h[3] * o.z;
            }
        }
    }
    let mut faces: Vec<Face> = world_faces.to_vec();
    for f in &mut faces {
        f.point -= o;
    }
    let face_of = |m: usize| -> Result<&Face> { el_ref(&faces, el(cycle_face, m % n)?) };
    // each round's end section and its derivative across it from the GEOMETRY (owner,
    // section 24); the raw jets stay for the round's curvature
    let raw_jets = jets.clone();
    let raw_q = qs.clone();
    for (jr, q) in jets.iter_mut().zip(qs.iter_mut()) {
        *q = geometric_rows(jr, *q)?;
    }

    // each cycle patch's round rows, its rails' ends and their directions toward the corner
    let mut h0s: Vec<Vec<Hom>> = vec![Vec::new(); n];
    let mut h1s: Vec<Vec<Hom>> = vec![Vec::new(); n];
    let mut end0 = vec![Point3::ZERO; n];
    let mut end1 = vec![Point3::ZERO; n];
    let mut mids = vec![Point3::ZERO; n];
    let mut dir0 = vec![Vec3::ZERO; n];
    let mut dir1 = vec![Vec3::ZERO; n];
    for m in 0..n {
        let r = el(cycle_round, m)?;
        let q = el(&qs, r)?;
        let sign = if el_ref(rounds, r)?.corner_above {
            1.0
        } else {
            -1.0
        };
        let jr = el_ref(&jets, r)?;
        let (j0, j1) = (el_ref(jr, 0)?, el_ref(jr, 1)?);
        let rev = el(cycle_rev, m)?;
        let mut h0 = vec![HOM_ZERO; q + 1];
        let mut h1 = vec![HOM_ZERO; q + 1];
        for c in 0..=q {
            let cc = if rev { q - c } else { c };
            *el_mut(&mut h0, c)? = el(j0, cc)?;
            *el_mut(&mut h1, c)? = axpy(&HOM_ZERO, sign, el_ref(j1, cc)?);
        }
        for e in 0..2 {
            let col = if e == 0 { 0 } else { q };
            let a = el(&h0, col)?;
            let b = el(&h1, col)?;
            let p = pt(&a);
            if face_of(m + e)?.distance(p).0.abs() > tol {
                return Err(Refusal::invalid(
                    "SmBlendSetback: a round does not meet its faces at its end",
                ));
            }
            let t = Vec3::new(
                (b[0] - p.x * b[3]) / a[3],
                (b[1] - p.y * b[3]) / a[3],
                (b[2] - p.z * b[3]) / a[3],
            )
            .unitize()?;
            *el_mut(if e == 0 { &mut end0 } else { &mut end1 }, m)? = p;
            *el_mut(if e == 0 { &mut dir0 } else { &mut dir1 }, m)? = t;
        }
        *el_mut(&mut mids, m)? = pt(&row_middle(&h0)?);
        *el_mut(&mut h0s, m)? = h0;
        *el_mut(&mut h1s, m)? = h1;
    }
    // convex or concave, and the axis e into the convex side
    let mut sum = Vec3::ZERO;
    let mut convex = 0;
    for m in 0..n {
        let face = face_of(m)?;
        sum += face.normal;
        let side = if face.distance(el(&mids, m)?).0 < 0.0 {
            1
        } else {
            -1
        };
        if convex != 0 && side != convex {
            return Err(Refusal::invalid(
                "SmBlendSetback: a convex G1 fill needs rounds all convex or all concave",
            ));
        }
        convex = side;
    }
    let e = (sum * if convex > 0 { -1.0 } else { 1.0 }).unitize()?;
    let mut a1 = Vec3::new(1.0, 0.0, 0.0);
    if e.y.abs() < e.x.abs() && e.y.abs() <= e.z.abs() {
        a1 = Vec3::new(0.0, 1.0, 0.0);
    } else if e.z.abs() < e.x.abs() && e.z.abs() < e.y.abs() {
        a1 = Vec3::new(0.0, 0.0, 1.0);
    }
    a1 = a1 - e * a1.dot(e);
    let a1 = a1.unitize()?;
    let a2 = e.cross(a1);
    let in_plane = |v: Vec3| P2 {
        x: v.dot(a1),
        y: v.dot(a2),
    };

    // the faces' least normal curvature on the fill's convex side (ruling (a)): a cylinder
    // or sphere whose material is on the other side than a convex body's, -1 / R; such a
    // cone, -cos / rho over the spring curves' least radius (bounded below by its control
    // points' axial coordinates)
    let mut face_k = vec![0.0; n];
    // each round's least normal curvature along its end row (the rounds' part of k)
    let mut round_k = vec![0.0; n];
    for (r, rk) in round_k.iter_mut().enumerate() {
        // the faces at the row's ends: which are planes, and the round's spine by the one
        // classification from the faces' kinds and placement
        let mut plane_end = [false, false];
        let mut ends = [Point3::ZERO; 2];
        let mut face_at = [faces.len(); 2];
        let rj0 = el_ref(el_ref(&raw_jets, r)?, 0)?;
        for e in 0..2 {
            let h = el(rj0, if e == 0 { 0 } else { el(&raw_q, r)? })?;
            *el_mut(&mut ends, e)? = pt(&h);
            for (f, face) in faces.iter().enumerate() {
                if face.distance(el(&ends, e)?).0.abs() <= tol {
                    *el_mut(&mut face_at, e)? = f;
                    *el_mut(&mut plane_end, e)? = face.kind == FaceKind::Plane;
                }
            }
        }
        let mut line = false;
        if face_at[0] < faces.len() && face_at[1] < faces.len() {
            line = line_spine(
                el_ref(&faces, face_at[0])?,
                el_ref(&faces, face_at[1])?,
                ends,
                rj0,
                tol,
            )?;
        }
        *rk = round_row_k(el_ref(&raw_jets, r)?, e, plane_end, line)?;
    }

    // the spring curves: per face m, patch m's half (its v = 0 column) and patch m - 1's (its
    // v = 1 column), matched to their rounds and to the interior map; their tangent-plane
    // directions
    let mut side0: Vec<Side> = vec![Side::default(); n];
    let mut side1: Vec<Side> = vec![Side::default(); n];
    let mut xi = vec![P2::default(); n];
    for m in 0..n {
        let mp = (m + n - 1) % n;
        let face = *face_of(m)?;
        let spring = spring_curve(
            &face,
            el(&end0, m)?,
            el(&dir0, m)?,
            el(&end1, mp)?,
            -el(&dir1, mp)?,
        )?;
        // the face's curvature bound
        if face.kind != FaceKind::Plane && ((face.side < 0.0) == (convex > 0)) {
            let fk = el_mut(&mut face_k, el(cycle_face, m)?)?;
            if face.kind == FaceKind::Cone {
                let mut hmin = BIG_DOUBLE;
                for half in [&spring.from_p, &spring.from_q] {
                    for h in &half.curve {
                        hmin = smin(hmin, (pt(h) - face.point).dot(face.axis));
                    }
                }
                let rho = face.radius + face.slope * hmin;
                if !(rho > 0.0) {
                    return Err(Refusal::invalid(
                        "SmBlendSetback: a spring curve reaches a cone's apex",
                    ));
                }
                *fk = -1.0 / (rho * sqrt(1.0 + face.slope * face.slope));
            } else {
                *fk = -1.0 / face.radius;
            }
        }
        for s in 0..2 {
            let half = if s == 0 {
                &spring.from_p
            } else {
                &spring.from_q
            };
            let patch = if s == 0 { m } else { mp };
            let ds = half.curve.len().checked_sub(1).ok_or_else(sm_err)?;
            let mut sc: Vec<Hom> = half.curve.clone();
            // the round there: its rail end H_r0 and its along-derivative H_r1
            let q = el(&qs, el(cycle_round, patch)?)?;
            let r0 = el(el_ref(&h0s, patch)?, if s == 0 { 0 } else { q })?;
            let r1 = el(el_ref(&h1s, patch)?, if s == 0 { 0 } else { q })?;
            let p = pt(&r0);
            let w0 = el(&sc, 0)?[3];
            let sc1 = el(&sc, 1)?;
            let alpha = sc1[3];
            let sb = Vec3::new(
                sc1[0] - alpha * p.x,
                sc1[1] - alpha * p.y,
                sc1[2] - alpha * p.z,
            );
            let mu = r1[3];
            let nu = Vec3::new(r1[0] - mu * p.x, r1[1] - mu * p.y, r1[2] - mu * p.z);
            // the half's tangent part over the round's (both along the rail)
            let kappa = sb.dot(nu) / nu.dot(nu);
            let rho = w0 / (alpha - kappa * mu);
            let dc = r0[3] / w0;
            let lambda = ds as f64 * dc * rho * kappa;
            if !(kappa > 0.0) || !(rho > 0.0) || !(lambda > 0.0) {
                return Err(Refusal::invalid(
                    "SmBlendSetback: a spring curve cannot be matched to its round's end",
                ));
            }
            let mut pow = 1.0;
            for h in &mut sc {
                for x in h.iter_mut() {
                    *x *= dc * pow;
                }
                pow *= rho;
            }
            // the base point's weights 1, 1: times [1, 1, q2, q3]
            let wl = el(&sc, ds)?[3];
            let dwl = ds as f64 * (el(&sc, ds)?[3] - el(&sc, ds - 1)?[3]);
            let q3 = 1.0 / wl;
            let q2 = q3 * (1.0 + dwl / (3.0 * wl));
            if !(q2 > 0.0) {
                return Err(Refusal::invalid(
                    "SmBlendSetback: a spring curve's weight would not stay positive",
                ));
            }
            let factor = [1.0, 1.0, q2, q3];
            let mut col = vec![HOM_ZERO; ds + 4];
            for c in 0..4 {
                let comp: Bern = comp_of(&sc, c)?;
                let prod = bern_mul(&comp, &factor)?;
                for (ci, &x) in col.iter_mut().zip(&prod) {
                    *el_mut(ci, c)? = x;
                }
            }
            el_mut(&mut col, ds + 3)?[3] = 1.0;
            el_mut(&mut col, ds + 2)?[3] = 1.0;
            // the partials, the same reparametrization (rho^i; a direction's scale is free)
            let mut tv: [Vec<Hom>; 2] = [Vec::new(), Vec::new()];
            for (tt, src) in tv.iter_mut().zip(&half.partials) {
                let mut pw = 1.0;
                for h in src {
                    tt.push([h[0] * pw, h[1] * pw, h[2] * pw, h[3] * pw]);
                    pw *= rho;
                }
            }
            // the field: minus the column's derivative at the base point, in the plane
            let cl = el(&col, ds + 3)?;
            let cl1 = el(&col, ds + 2)?;
            let der = Vec3::new(cl[0] - cl1[0], cl[1] - cl1[1], cl[2] - cl1[2]) * (ds + 3) as f64;
            let side = Side {
                col,
                deg: ds + 3,
                t: tv,
                lambda,
                d: in_plane(der * -1.0),
            };
            if s == 0 {
                *el_mut(&mut xi, m)? = in_plane(Vec3::new(cl[0], cl[1], cl[2]));
                *el_mut(&mut side0, m)? = side;
            } else {
                *el_mut(&mut side1, mp)? = side;
            }
        }
    }
    let nf = n as f64;
    let mut centre = P2::default();
    let mut rho = 0.0;
    for p in &xi {
        centre = P2 {
            x: centre.x + p.x / nf,
            y: centre.y + p.y / nf,
        };
    }
    for p in &xi {
        let (dx, dy) = (p.x - centre.x, p.y - centre.y);
        rho = smax(rho, sqrt(dx * dx + dy * dy));
    }
    if !(rho > 0.0) {
        return Err(sm_err().into());
    }
    let bs: Vec<P2> = xi
        .iter()
        .map(|p| P2 {
            x: (p.x - centre.x) / rho,
            y: (p.y - centre.y) / rho,
        })
        .collect();
    let height_of = |h: &Hom| h[0] * e.x + h[1] * e.y + h[2] * e.z;

    // the degrees: up from the least, then 9 (the fork's caller asks least first)
    let mut top: Vec<Net> = Vec::new();
    let mut top_degree = 0;
    let degrees: Vec<usize> = (3..=9).collect();
    'degrees: for &d in &degrees {
        let mono = monomials(d);
        let big_m = mono.len();
        let mut ncs = vec![0usize; n];
        let mut qvs = vec![0usize; n];
        let mut first_free = vec![0usize; n];
        let mut unknowns = big_m;
        for m in 0..n {
            let nc = smax_usize(el_ref(&side0, m)?.deg, el_ref(&side1, m)?.deg) + 3;
            let q1 = el(&qs, el(cycle_round, m)?)? + 1;
            let qv = smax_usize(smax_usize(q1, d), 5);
            *el_mut(&mut ncs, m)? = nc;
            *el_mut(&mut qvs, m)? = qv;
            *el_mut(&mut first_free, m)? = unknowns;
            unknowns += (nc - 3) * (qv - 3); // rows 2 ..= Nc - 2 of columns 2 ..= qv - 2
        }
        let mut patches: Vec<AffinePatch> = vec![AffinePatch::default(); 2 * n];
        let mut aeq: Vec<f64> = Vec::new();
        let mut beq: Vec<f64> = Vec::new();
        let mut failed = false;
        for m in 0..n {
            let nc = el(&ncs, m)?;
            let qv = el(&qvs, m)?;
            let (s0m, s1m) = (el_ref(&side0, m)?, el_ref(&side1, m)?);
            // rows 0, 1: the round's, at its speeds off its end (linear across)
            let mut rows = vec![vec![HOM_ZERO; qv + 1]; nc + 1];
            let lam = [s0m.lambda / nc as f64, s1m.lambda / nc as f64];
            let (h0, h1) = (el_ref(&h0s, m)?, el_ref(&h1s, m)?);
            for c in 0..4 {
                let s0: Bern = comp_of(h0, c)?;
                let s1: Bern = comp_of(h1, c)?;
                let s1 = bern_mul(&lam, &s1)?;
                let s0 = bern_elevate(&s0, qv)?;
                let s1 = bern_elevate(&s1, qv)?;
                for j in 0..=qv {
                    *el_mut(el_mut(el_mut(&mut rows, 0)?, j)?, c)? = el(&s0, j)?;
                    *el_mut(el_mut(el_mut(&mut rows, 1)?, j)?, c)? = el(&s0, j)? + el(&s1, j)?;
                }
            }
            // rows Nc - 1, Nc: the interior map's projection along edge m and its field there
            let x0 = el(&xi, m)?;
            let x1 = el(&xi, (m + 1) % n)?;
            let (d0, d1) = (s0m.d, s1m.d);
            for c in 0..3 {
                let da1 = a1.component(c);
                let da2 = a2.component(c);
                let sp = [x0.x * da1 + x0.y * da2, x1.x * da1 + x1.y * da2];
                let sdp = [d0.x * da1 + d0.y * da2, d1.x * da1 + d1.y * da2];
                let s3 = bern_elevate(&sp, qv)?;
                let sde = bern_elevate(&sdp, qv)?;
                for j in 0..=qv {
                    *el_mut(el_mut(el_mut(&mut rows, nc)?, j)?, c)? = el(&s3, j)?;
                    *el_mut(el_mut(el_mut(&mut rows, nc - 1)?, j)?, c)? =
                        el(&s3, j)? + el(&sde, j)? / nc as f64;
                }
            }
            for j in 0..=qv {
                el_mut(el_mut(&mut rows, nc - 1)?, j)?[3] = 1.0;
                el_mut(el_mut(&mut rows, nc)?, j)?[3] = 1.0;
            }
            // rows 2 ..= Nc - 2: the cubic with the same end rows and derivatives, raised to
            // Nc (the planar fill's own shape across; their heights free in columns
            // 2 ..= qv - 2)
            for j in 0..=qv {
                for c in 0..4 {
                    let dk = nc as f64 / 3.0;
                    let r0 = el(&el(el_ref(&rows, 0)?, j)?, c)?;
                    let r1 = el(&el(el_ref(&rows, 1)?, j)?, c)?;
                    let rn = el(&el(el_ref(&rows, nc)?, j)?, c)?;
                    let rn1 = el(&el(el_ref(&rows, nc - 1)?, j)?, c)?;
                    let cubic = [r0, r0 + dk * (r1 - r0), rn + dk * (rn1 - rn), rn];
                    let up = bern_elevate(&cubic, nc)?;
                    for i in 2..=nc - 2 {
                        *el_mut(el_mut(el_mut(&mut rows, i)?, j)?, c)? = el(&up, i)?;
                    }
                }
            }
            // the fixed columns: the spring halves (0 and qv; rows 0, 1 the round's) and the
            // columns G1 to the faces by construction (1 and qv - 1)
            let mut fixed_height: [[[Hom; 2]; 2]; 2] = [[[HOM_ZERO; 2]; 2]; 2];
            for ee in 0..2 {
                let side = if ee == 0 { s0m } else { s1m };
                let j_edge = if ee == 0 { 0 } else { qv };
                let j_next = if ee == 0 { 1 } else { qv - 1 };
                // the side's column at Nc - 3 (for gamma's product) and at Nc
                let mut col_low = vec![HOM_ZERO; nc - 2];
                let mut col = vec![HOM_ZERO; nc + 1];
                for c in 0..4 {
                    let comp: Bern = comp_of(&side.col, c)?;
                    let low = bern_elevate(&comp, nc - 3)?;
                    let high = bern_elevate(&comp, nc)?;
                    for (ci, &x) in col_low.iter_mut().zip(&low) {
                        *el_mut(ci, c)? = x;
                    }
                    for (ci, &x) in col.iter_mut().zip(&high) {
                        *el_mut(ci, c)? = x;
                    }
                }
                *el_mut(&mut col, 0)? = el(el_ref(&rows, 0)?, j_edge)?;
                *el_mut(&mut col, 1)? = el(el_ref(&rows, 1)?, j_edge)?;
                for i in 0..nc - 1 {
                    *el_mut(el_mut(&mut rows, i)?, j_edge)? = el(&col, i)?;
                }
                *el_mut(el_mut(&mut fixed_height, ee)?, 0)? = [el(&col, nc - 1)?, el(&col, nc)?];
                // the constructive column: unknowns alpha (Nc - k1 + 1), beta (Nc - k2 + 1),
                // gamma (4)
                let k1 = side.t[0].len().checked_sub(1).ok_or_else(sm_err)?;
                let k2 = side.t[1].len().checked_sub(1).ok_or_else(sm_err)?;
                if k1 > nc || k2 > nc {
                    failed = true;
                    break;
                }
                let la = nc - k1 + 1;
                let lbt = nc - k2 + 1;
                let lu = la + lbt + 4;
                // each unknown's contribution to the column (Bernstein of degree Nc,
                // homogeneous)
                let mut basis = vec![vec![HOM_ZERO; nc + 1]; lu];
                for (u, bu) in basis.iter_mut().enumerate() {
                    for c in 0..4 {
                        let (unit, other): (Bern, Bern) = if u < la {
                            let mut unit = vec![0.0; la];
                            *el_mut(&mut unit, u)? = 1.0;
                            (unit, comp_of(&side.t[0], c)?)
                        } else if u < la + lbt {
                            let mut unit = vec![0.0; lbt];
                            *el_mut(&mut unit, u - la)? = 1.0;
                            (unit, comp_of(&side.t[1], c)?)
                        } else {
                            let mut unit = vec![0.0; 4];
                            *el_mut(&mut unit, u - la - lbt)? = 1.0;
                            (unit, comp_of(&col_low, c)?)
                        };
                        let prod = bern_elevate(&bern_mul(&unit, &other)?, nc)?;
                        for (bi, &x) in bu.iter_mut().zip(&prod) {
                            *el_mut(bi, c)? = x / qv as f64;
                        }
                    }
                }
                // the conditions: rows 0, 1 the round's (4 each); rows Nc - 1, Nc the interior
                // map's projection and weight (3 each)
                let mut sa: Vec<f64> = Vec::new();
                let mut sbv: Vec<f64> = Vec::new();
                let add_row = |sa: &mut Vec<f64>,
                               sbv: &mut Vec<f64>,
                               i: usize,
                               w: [f64; 4],
                               target: f64|
                 -> Result<()> {
                    for bu in &basis {
                        let bui = el(bu, i)?;
                        let mut s = 0.0;
                        for (wc, bc) in w.iter().zip(&bui) {
                            s += wc * bc;
                        }
                        sa.push(s);
                    }
                    let ci = el(&col, i)?;
                    let mut base = 0.0;
                    for (wc, cc) in w.iter().zip(&ci) {
                        base += wc * cc;
                    }
                    sbv.push(target - base);
                    Ok(())
                };
                for i in 0..2 {
                    for c in 0..4 {
                        let mut w = [0.0; 4];
                        *el_mut(&mut w, c)? = 1.0;
                        let target = el(&el(el_ref(&rows, i)?, j_next)?, c)?;
                        add_row(&mut sa, &mut sbv, i, w, target)?;
                    }
                }
                for i in nc - 1..=nc {
                    let w1 = [a1.x, a1.y, a1.z, 0.0];
                    let w2 = [a2.x, a2.y, a2.z, 0.0];
                    let ww = [0.0, 0.0, 0.0, 1.0];
                    let rn = el(el_ref(&rows, i)?, j_next)?;
                    add_row(
                        &mut sa,
                        &mut sbv,
                        i,
                        w1,
                        rn[0] * a1.x + rn[1] * a1.y + rn[2] * a1.z,
                    )?;
                    add_row(
                        &mut sa,
                        &mut sbv,
                        i,
                        w2,
                        rn[0] * a2.x + rn[1] * a2.y + rn[2] * a2.z,
                    )?;
                    add_row(&mut sa, &mut sbv, i, ww, 1.0)?;
                }
                let mut sz = 0.0;
                for &x in &sbv {
                    sz = smax(sz, x.abs());
                }
                let nrows = sbv.len();
                let Some(aff) = affine_solutions(sa, sbv, nrows, lu, 1.0e-9 * (1.0 + sz))? else {
                    failed = true;
                    break;
                };
                let mut y0 = aff.x0;
                let z0 = aff.z;
                // the rest: column 1's middle rows as near as they can be to the interpolated
                // rows the free columns start from (their projections and weights)
                if !z0.is_empty() {
                    let lf = z0.len();
                    let mut mrow: Vec<f64> = Vec::new();
                    let mut crow: Vec<f64> = Vec::new();
                    for i in 2..=nc - 2 {
                        let wp1 = [a1.x, a1.y, a1.z, 0.0];
                        let wp2 = [a2.x, a2.y, a2.z, 0.0];
                        let ww = [0.0, 0.0, 0.0, 1.0];
                        for w in [wp1, wp2, ww] {
                            let rn = el(el_ref(&rows, i)?, j_next)?;
                            let ci = el(&col, i)?;
                            let mut target = 0.0;
                            let mut base = 0.0;
                            for ((wc, rc), cc) in w.iter().zip(&rn).zip(&ci) {
                                target += wc * rc;
                                base += wc * cc;
                            }
                            let mut coef = vec![0.0; lu];
                            for (u, cu) in coef.iter_mut().enumerate() {
                                let bui = el(el_ref(&basis, u)?, i)?;
                                for (wc, bc) in w.iter().zip(&bui) {
                                    *cu += wc * bc;
                                }
                            }
                            let mut at_y0 = base;
                            for (cu, yu) in coef.iter().zip(&y0) {
                                at_y0 += cu * yu;
                            }
                            for zf in &z0 {
                                let mut t = 0.0;
                                for (cu, zu) in coef.iter().zip(zf) {
                                    t += cu * zu;
                                }
                                mrow.push(t);
                            }
                            crow.push(target - at_y0);
                        }
                    }
                    // and a least nudge in every free direction (the rows may leave some free)
                    for g in 0..lf {
                        for f in 0..lf {
                            mrow.push(if f == g { 1.0e-9 } else { 0.0 });
                        }
                        crow.push(0.0);
                    }
                    let nr = crow.len();
                    let Some(tt) = least_squares(mrow, crow, nr, lf, 1.0e-14)? else {
                        failed = true;
                        break;
                    };
                    for (zf, &tf) in z0.iter().zip(&tt) {
                        for (yu, &zu) in y0.iter_mut().zip(zf) {
                            *yu += zu * tf;
                        }
                    }
                }
                let mut next = vec![HOM_ZERO; nc + 1];
                for (i, ni) in next.iter_mut().enumerate() {
                    *ni = el(&col, i)?;
                    for (bu, &yu) in basis.iter().zip(&y0) {
                        *ni = axpy(ni, yu, el_ref(bu, i)?);
                    }
                }
                *el_mut(&mut next, 0)? = el(el_ref(&rows, 0)?, j_next)?;
                *el_mut(&mut next, 1)? = el(el_ref(&rows, 1)?, j_next)?;
                for i in 0..nc - 1 {
                    *el_mut(el_mut(&mut rows, i)?, j_next)? = el(&next, i)?;
                }
                *el_mut(el_mut(&mut fixed_height, ee)?, 1)? = [el(&next, nc - 1)?, el(&next, nc)?];
            }
            if failed {
                break;
            }
            // h's monomials along the edge and along the field (rows Nc - 1, Nc)
            let (bm, bn) = (el(&bs, m)?, el(&bs, (m + 1) % n)?);
            let sx = [bm.x, bn.x];
            let sy = [bm.y, bn.y];
            let sdx = [d0.x / rho, d1.x / rho];
            let sdy = [d0.y / rho, d1.y / rho];
            let mut z2: Vec<Vec<f64>> = vec![Vec::new(); big_m];
            let mut z3: Vec<Vec<f64>> = vec![Vec::new(); big_m];
            for (j, &(a, b)) in mono.iter().enumerate() {
                let mut s1 = vec![0.0; d + 1];
                if a >= 1 {
                    let t =
                        bern_elevate(&bern_mul(&along_line(a - 1, b, &sx, &sy, d - 1)?, &sdx)?, d)?;
                    for i in 0..=d {
                        *el_mut(&mut s1, i)? += a as f64 * el(&t, i)?;
                    }
                }
                if b >= 1 {
                    let t =
                        bern_elevate(&bern_mul(&along_line(a, b - 1, &sx, &sy, d - 1)?, &sdy)?, d)?;
                    for i in 0..=d {
                        *el_mut(&mut s1, i)? += b as f64 * el(&t, i)?;
                    }
                }
                let v0 = bern_elevate(&along_line(a, b, &sx, &sy, d)?, qv)?;
                let v1 = bern_elevate(&s1, qv)?;
                let mut z2j = vec![0.0; qv + 1];
                for i in 0..=qv {
                    *el_mut(&mut z2j, i)? = el(&v0, i)? + el(&v1, i)? / nc as f64;
                }
                *el_mut(&mut z3, j)? = v0;
                *el_mut(&mut z2, j)? = z2j;
            }
            let patch = el_mut(&mut patches, m)?;
            patch.base.p = nc;
            patch.base.q = qv;
            patch.base.pts = vec![HOM_ZERO; (nc + 1) * (qv + 1)];
            for i in 0..=nc {
                for j in 0..=qv {
                    *el_mut(&mut patch.base.pts, i * (qv + 1) + j)? = el(el_ref(&rows, i)?, j)?;
                }
            }
            for jm in 0..big_m {
                let mut z = t_make(nc, qv);
                for j in 0..=qv {
                    *el_mut(&mut z.c, (nc - 1) * (qv + 1) + j)? = el(el_ref(&z2, jm)?, j)?;
                    *el_mut(&mut z.c, nc * (qv + 1) + j)? = el(el_ref(&z3, jm)?, j)?;
                }
                patch.index.push(jm);
                patch.z.push(z);
            }
            // the free middle heights
            let mut at = el(&first_free, m)?;
            for i in 2..=nc - 2 {
                for j in 2..=qv - 2 {
                    let mut z = t_make(nc, qv);
                    *el_mut(&mut z.c, i * (qv + 1) + j)? = 1.0;
                    patch.index.push(at);
                    at += 1;
                    patch.z.push(z);
                }
            }
            // the equalities: the interior map's heights at rows Nc - 1, Nc of the fixed
            // columns
            for (ee, fh) in fixed_height.iter().enumerate() {
                for (jj, fhj) in fh.iter().enumerate() {
                    let col = if ee == 0 { jj } else { qv - jj };
                    for (k, target) in fhj.iter().enumerate() {
                        let row = nc - 1 + k;
                        let idx = row * (qv + 1) + col;
                        let mut row_eq = vec![0.0; unknowns];
                        for (t, &ix) in patch.index.iter().enumerate() {
                            *el_mut(&mut row_eq, ix)? += el(&el_ref(&patch.z, t)?.c, idx)?;
                        }
                        aeq.extend_from_slice(&row_eq);
                        beq.push(height_of(target) - height_of(&el(&patch.base.pts, idx)?));
                    }
                }
            }
        }
        if failed {
            continue;
        }
        // the interior parts, as the planar fill's
        for q in 0..n {
            let bq = el(&bs, q)?;
            let bnext = el(&bs, (q + 1) % n)?;
            let bprev = el(&bs, (q + n - 1) % n)?;
            let mq = P2 {
                x: (bq.x + bnext.x) * 0.5,
                y: (bq.y + bnext.y) * 0.5,
            };
            let mp = P2 {
                x: (bq.x + bprev.x) * 0.5,
                y: (bq.y + bprev.y) * 0.5,
            };
            let sx = Tensor {
                p: 1,
                q: 1,
                c: vec![0.0, mp.x, mq.x, bq.x],
            };
            let sy = Tensor {
                p: 1,
                q: 1,
                c: vec![0.0, mp.y, mq.y, bq.y],
            };
            let patch = el_mut(&mut patches, n + q)?;
            patch.base.p = d;
            patch.base.q = d;
            patch.base.pts = vec![HOM_ZERO; (d + 1) * (d + 1)];
            let sxd = super::tensor_elevate(&sx, d, d)?;
            let syd = super::tensor_elevate(&sy, d, d)?;
            for (i, ptq) in patch.base.pts.iter_mut().enumerate() {
                let xi1 = centre.x + rho * el(&sxd.c, i)?;
                let xi2 = centre.y + rho * el(&syd.c, i)?;
                *ptq = [
                    xi1 * a1.x + xi2 * a2.x,
                    xi1 * a1.y + xi2 * a2.y,
                    xi1 * a1.z + xi2 * a2.z,
                    1.0,
                ];
            }
            for (j, &(a, b)) in mono.iter().enumerate() {
                patch.index.push(j);
                patch.z.push(over_patch(a, b, &sx, &sy, d)?);
            }
        }
        let mut size = 0.0;
        for &x in &beq {
            size = smax(size, x.abs());
        }
        let neq = beq.len();
        let Some(affine) = affine_solutions(aeq, beq, neq, unknowns, 1.0e-9 * (1.0 + size))? else {
            continue;
        };
        if affine.z.is_empty() {
            continue;
        }
        let (yp, zn) = (affine.x0, affine.z);
        let free = zn.len();

        // the energy (as the planar fill's)
        let energy_rows = |net: &Net| -> Result<Vec<f64>> {
            let mut rows = Vec::new();
            for (&g1x, &g1w) in GX.iter().zip(&GW) {
                for (&g2x, &g2w) in GX.iter().zip(&GW) {
                    let r = second(net, g1x, g2x)?;
                    let w = sqrt(g1w * g2w);
                    for c in 0..3 {
                        rows.push(w * r[0].component(c));
                        rows.push(w * sqrt(2.0) * r[1].component(c));
                        rows.push(w * r[2].component(c));
                    }
                }
            }
            rows.extend(net_fairness_rows(net)?);
            Ok(rows)
        };
        let mut e0: Vec<f64> = Vec::new();
        let mut e_start: Vec<usize> = Vec::new();
        let mut patch_e0: Vec<Vec<f64>> = Vec::with_capacity(2 * n);
        for patch in &patches {
            let rows = energy_rows(&patch.base)?;
            e_start.push(e0.len());
            e0.extend_from_slice(&rows);
            patch_e0.push(rows);
        }
        let le = e0.len();
        let mut ae = vec![0.0; le * unknowns];
        {
            let mut unit = vec![0.0; unknowns];
            for (p, patch) in patches.iter().enumerate() {
                for &u in &patch.index {
                    *el_mut(&mut unit, u)? = 1.0;
                    let rows = energy_rows(&affine_net(patch, &unit, e)?)?;
                    *el_mut(&mut unit, u)? = 0.0;
                    let start = el(&e_start, p)?;
                    let base = el_ref(&patch_e0, p)?;
                    for (i, &ri) in rows.iter().enumerate() {
                        *el_mut(&mut ae, (start + i) * unknowns + u)? += ri - el(base, i)?;
                    }
                }
            }
        }
        let mut bm = vec![0.0; le * free];
        let mut cv = vec![0.0; le];
        for i in 0..le {
            let mut s = el(&e0, i)?;
            for u in 0..unknowns {
                s += el(&ae, i * unknowns + u)? * el(&yp, u)?;
            }
            *el_mut(&mut cv, i)? = s;
            for f in 0..free {
                let mut t = 0.0;
                let znf = el_ref(&zn, f)?;
                for u in 0..unknowns {
                    t += el(&ae, i * unknowns + u)? * el(znf, u)?;
                }
                *el_mut(&mut bm, i * free + f)? = t;
            }
        }
        let nets_at = |t: &[f64]| -> Result<Vec<Net>> {
            let mut y = yp.clone();
            for f in 0..free {
                let znf = el_ref(&zn, f)?;
                for (u, yu) in y.iter_mut().enumerate() {
                    *yu += el(znf, u)? * el(t, f)?;
                }
            }
            let mut nets = Vec::with_capacity(2 * n);
            for patch in &patches {
                nets.push(affine_net(patch, &y, e)?);
            }
            Ok(nets)
        };

        // each patch's ruling-(a) matrix: w^3 (l, m, n) + |k| o J (J the projection's w^4 I),
        // affine in the heights; o's sign certified
        let mut ks = vec![0.0; 2 * n];
        for m in 0..n {
            let km = smin(
                smin(
                    el(&face_k, el(cycle_face, m)?)?,
                    el(&face_k, el(cycle_face, (m + 1) % n)?)?,
                ),
                el(&round_k, el(cycle_round, m)?)?,
            );
            *el_mut(&mut ks, m)? = km;
        }
        for m in 0..n {
            let kq = smin(
                el(&face_k, el(cycle_face, m)?)?,
                smin(el(&ks, m)?, el(&ks, (m + n - 1) % n)?),
            );
            *el_mut(&mut ks, n + m)? = kq;
        }
        let mut q0s: Vec<[Tensor; 3]> = Vec::with_capacity(2 * n);
        let mut qjs: Vec<[Vec<Tensor>; 3]> = Vec::with_capacity(2 * n);
        let mut signs = vec![0.0; 2 * n];
        for (p, patch) in patches.iter().enumerate() {
            let af = affine_fundamentals(patch, e)?;
            let sp = certified_sign(
                &af.o,
                Strict {
                    margin: false,
                    ..strict
                },
            )?;
            *el_mut(&mut signs, p)? = sp;
            if sp == 0.0 {
                return Err(Refusal::sm_err(
                    "SmBlendSetback: the fill folds (a patch's normal turns across the corner's axis)",
                ));
            }
            let kp = el(&ks, p)?;
            // w^3 and the projection's J
            let base = &patch.base;
            let mut h: [Tensor; 4] = Default::default();
            for (c, hc) in h.iter_mut().enumerate() {
                *hc = t_make(base.p, base.q);
                for (i, x) in hc.c.iter_mut().enumerate() {
                    let ptb = el(&base.pts, i)?;
                    let mut v = el(&ptb, c)?;
                    if c < 3 {
                        v -= height_of(&ptb) * e.component(c);
                    }
                    *x = v;
                }
            }
            let w2 = t_mul_fast(&h[3], &h[3])?;
            let w3 = t_mul_fast(&w2, &h[3])?;
            let jp = first_form(&h)?;
            let mut q0p: [Tensor; 3] = Default::default();
            let mut qjp: [Vec<Tensor>; 3] = Default::default();
            for k in 0..3 {
                // (where k = 0, w^3 > 0 changes nothing but the sufficient conditions: left
                // out)
                let afq = el_ref(&af.q0, k)?;
                let mut q0k = if kp < 0.0 {
                    t_mul_fast(&w3, afq)?
                } else {
                    afq.clone()
                };
                if kp < 0.0 {
                    q0k = t_axpy(&q0k, -kp, &t_mul_fast(&af.o, el_ref(&jp, k)?)?)?;
                }
                *el_mut(&mut q0p, k)? = q0k;
                let out = el_mut(&mut qjp, k)?;
                for qt in el_ref(&af.qj, k)? {
                    out.push(if kp < 0.0 {
                        t_mul_fast(&w3, qt)?
                    } else {
                        qt.clone()
                    });
                }
            }
            q0s.push(q0p);
            qjs.push(qjp);
        }
        // the QP (as the planar fill's)
        let mut gm = vec![0.0; free * free];
        let mut gv = vec![0.0; free];
        let mut trace = 0.0;
        for a in 0..free {
            for b in a..free {
                let mut s = 0.0;
                for i in 0..le {
                    s += el(&bm, i * free + a)? * el(&bm, i * free + b)?;
                }
                *el_mut(&mut gm, b * free + a)? = 2.0 * s;
                *el_mut(&mut gm, a * free + b)? = 2.0 * s;
            }
            let mut s = 0.0;
            for i in 0..le {
                s += el(&bm, i * free + a)? * el(&cv, i)?;
            }
            *el_mut(&mut gv, a)? = 2.0 * s;
            trace += el(&gm, a * free + a)?;
        }
        for a in 0..free {
            *el_mut(&mut gm, a * free + a)? += 1.0e-12 * trace / free as f64;
        }
        // each interior part's extra direction at its base point: the field there
        let mut kernel = vec![-1.0; 2 * n];
        for q in 0..n {
            let bq = el(&bs, q)?;
            let bprev = el(&bs, (q + n - 1) % n)?;
            let bnext = el(&bs, (q + 1) % n)?;
            let ds = P2 {
                x: (bq.x - bprev.x) * 0.5,
                y: (bq.y - bprev.y) * 0.5,
            };
            let dt = P2 {
                x: (bq.x - bnext.x) * 0.5,
                y: (bq.y - bnext.y) * 0.5,
            };
            let det = ds.x * dt.y - ds.y * dt.x;
            if !(det.abs() > 0.0) {
                return Err(sm_err().into());
            }
            let sig = el_ref(&side0, q)?.d;
            let a = (sig.x * dt.y - sig.y * dt.x) / det;
            let b = (ds.x * sig.y - ds.y * sig.x) / det;
            let mut th = b.atan2(a);
            if th < 0.0 {
                th += PI;
            }
            if th >= PI {
                th -= PI;
            }
            *el_mut(&mut kernel, n + q)? = th;
        }
        let mut cm: Vec<f64> = Vec::new();
        let mut bv: Vec<f64> = Vec::new();
        for (p, patch) in patches.iter().enumerate() {
            let mut theta: Vec<f64> = (0..GRAPH_DIRECTIONS)
                .map(|k| PI * k as f64 / GRAPH_DIRECTIONS as f64)
                .collect();
            let kp = el(&kernel, p)?;
            if kp >= 0.0 {
                theta.push(kp);
            }
            theta.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            theta.push(el(&theta, 0)? + PI);
            let q0p = el_ref(&q0s, p)?;
            let qjp = el_ref(&qjs, p)?;
            let (mut big_p, mut big_q) = (0, 0);
            for t in q0p {
                big_p = smax_usize(big_p, t.p);
                big_q = smax_usize(big_q, t.q);
            }
            let mut pc0: Vec<Vec<Tensor>> = vec![Vec::new(); 3];
            let mut pcf: Vec<Vec<Tensor>> = vec![Vec::new(); 3 * free];
            let mut scale = 0.0;
            for k in 0..3 {
                let q0k = el_ref(q0p, k)?;
                let mut s0 = super::tensor_elevate(q0k, big_p, big_q)?;
                let mut sf = vec![t_make(big_p, big_q); free];
                let qjk = el_ref(qjp, k)?;
                for (t, &u) in patch.index.iter().enumerate() {
                    let sj = super::tensor_elevate(el_ref(qjk, t)?, big_p, big_q)?;
                    scale = smax(scale, t_scale(&sj));
                    let ypu = el(&yp, u)?;
                    for (s0i, &sji) in s0.c.iter_mut().zip(&sj.c) {
                        *s0i += ypu * sji;
                    }
                    for (f, sff) in sf.iter_mut().enumerate() {
                        let z = el(el_ref(&zn, f)?, u)?;
                        if z == 0.0 {
                            continue;
                        }
                        for (sfi, &sji) in sff.c.iter_mut().zip(&sj.c) {
                            *sfi += z * sji;
                        }
                    }
                }
                scale = smax(scale, t_scale(q0k));
                *el_mut(&mut pc0, k)? = t_pieces(&s0, GRAPH_LEVELS)?;
                for (f, sff) in sf.iter().enumerate() {
                    *el_mut(&mut pcf, k * free + f)? = t_pieces(sff, GRAPH_LEVELS)?;
                }
            }
            let sp = el(&signs, p)?;
            let mut row_t = vec![0.0; free];
            let pc00 = el_ref(&pc0, 0)?;
            for pc in 0..pc00.len() {
                for i in 0..el_ref(pc00, pc)?.c.len() {
                    for a in 0..theta.len() - 1 {
                        for pair in 0..2 {
                            let ta = el(&theta, a)?;
                            let (c0, s0) = (ta.cos(), ta.sin());
                            let (c1, s1) = if pair == 0 {
                                (c0, s0)
                            } else {
                                let tb = el(&theta, a + 1)?;
                                (tb.cos(), tb.sin())
                            };
                            let w = [c0 * c1, c0 * s1 + s0 * c1, s0 * s1];
                            let mut cst = 0.0;
                            let mut dmax = 0.0;
                            for (k, &wk) in w.iter().enumerate() {
                                cst += sp * wk * el(&el_ref(el_ref(&pc0, k)?, pc)?.c, i)?;
                            }
                            for (f, rf) in row_t.iter_mut().enumerate() {
                                let mut s = 0.0;
                                for (k, &wk) in w.iter().enumerate() {
                                    s += sp
                                        * wk
                                        * el(&el_ref(el_ref(&pcf, k * free + f)?, pc)?.c, i)?;
                                }
                                *rf = s;
                                dmax = smax(dmax, s.abs());
                            }
                            if !(dmax > 1.0e-11 * scale) {
                                continue;
                            }
                            for &rf in &row_t {
                                cm.push(rf / dmax);
                            }
                            bv.push((-cst + 1.0e-12 * strict.factor * scale) / dmax);
                        }
                    }
                }
            }
        }
        stats.solves += 1;
        stats.curved_degrees += 1;
        stats.curved_rows += bv.len();
        let outcome = dual_qp_rows(&gm, &gv, &cm, &bv, free, bv.len(), 1.0e-13, true)?;
        let t = match outcome {
            QpOutcome::Solved(t) => t,
            QpOutcome::Infeasible { proven: true } if d == 9 => break 'degrees,
            _ => continue,
        };
        let mut nets = nets_at(&t)?;
        // the certificate, on the nets themselves: o one-signed, s C11, s C22,
        // C11 C22 - C12^2 >= 0
        let mut certified = true;
        for (p, net) in nets.iter().enumerate() {
            if !certified {
                break;
            }
            let f = fundamentals(net, e)?;
            let s = certified_sign(&f[3], strict)?;
            if s == 0.0 {
                certified = false;
                break;
            }
            let kp = el(&ks, p)?;
            // beside a face where k < 0: exact along the faces' columns (no_more_concave)
            if p < n && kp < 0.0 {
                let fa = face_of(p)?;
                let fb = face_of(p + 1)?;
                if no_more_concave_net(net, fa, fb, e, kp, strict)? != 1 {
                    certified = false;
                }
                continue;
            }
            // the shift's J from the net's projection (as above) and w^3
            let mut h: [Tensor; 4] = Default::default();
            for (c, hc) in h.iter_mut().enumerate() {
                *hc = t_make(net.p, net.q);
                for (i, x) in hc.c.iter_mut().enumerate() {
                    let ptn = el(&net.pts, i)?;
                    let mut v = el(&ptn, c)?;
                    if c < 3 {
                        v -= height_of(&ptn) * e.component(c);
                    }
                    *x = v;
                }
            }
            let w3 = t_mul_fast(&t_mul_fast(&h[3], &h[3])?, &h[3])?;
            let jn = if kp < 0.0 {
                Some(first_form(&h)?)
            } else {
                None
            };
            let mut cmk: [Tensor; 3] = Default::default();
            for k in 0..3 {
                let fk = el_ref(&f, k)?;
                let mut c = if kp < 0.0 {
                    t_mul_fast(&w3, fk)?
                } else {
                    fk.clone()
                };
                if let Some(jn) = &jn {
                    c = t_axpy(&c, -kp, &t_mul_fast(&f[3], el_ref(jn, k)?)?)?;
                }
                *el_mut(&mut cmk, k)? = c;
            }
            let sl = scaled(&cmk[0], s);
            let sn = scaled(&cmk[2], s);
            let disc = t_axpy(
                &t_mul_fast(&cmk[0], &cmk[2])?,
                -1.0,
                &t_mul_fast(&cmk[1], &cmk[1])?,
            )?;
            for check in [&sl, &sn, &disc] {
                if t_nonneg(check, strict.eps(t_scale(check)), GRAPH_DEPTH)? != 1 {
                    certified = false;
                    break;
                }
            }
        }
        // the net: never turning back (check_brep's test #5), and contained (net_contained)
        if certified {
            for net in &nets {
                if !net_monotone(net, strict)?
                    || !net_contained(net, &faces, convex > 0, tol, strict)?
                {
                    certified = false;
                    break;
                }
            }
        }
        if !certified {
            continue;
        }
        // back to the world
        for net in &mut nets {
            for h in &mut net.pts {
                h[0] += h[3] * o.x;
                h[1] += h[3] * o.y;
                h[2] += h[3] * o.z;
            }
        }
        if d == 9 {
            top = nets;
            top_degree = d;
            continue;
        }
        return Ok(fill_from(&nets, cycle_round, cycle_face, cycle_rev, d)?);
    }
    if !top.is_empty() {
        return Ok(fill_from(
            &top,
            cycle_round,
            cycle_face,
            cycle_rev,
            top_degree,
        )?);
    }
    stats.uncertified = true;
    Err(Refusal::sm_err(
        "SmBlendSetback: no G1 fill beside a curved face up to degree 9 is certified (ruling (a)) at these setbacks",
    ))
}

/// [`super::containment`] beside a curved face (the fork's `Containment`, 37c9e1d): 0 where
/// every patch of `fill` is certified on the material's side of every face, out of the
/// material at a convex corner (`convex`), into it at a concave one; `f64::INFINITY` (the
/// fork's `HUGE_VAL`) where that is not certified. Each face's implicit form at a patch, of
/// the sign of its distance times its side: a plane's `w (X - X0) . N`; a cylinder's or cone's
/// `|U - h a|^2 - rho_w^2`, `U = H_xyz - w X0`, `h = U . a`, `rho_w = R w + slope h`; a
/// sphere's `|U|^2 - R^2 w^2`. It is certified one-signed over the patch in Bernstein form
/// (subdivided up to [`GRAPH_DEPTH`] times), the sign the material's. Along a setback
/// patch's column on the face (`v = 0` or `1`) the form vanishes to second order (the column
/// on the face, the patch G1 to it): there it is `v^2 R` (or `(1 - v)^2 R`), the division
/// exact in Bernstein form, and `R` is certified instead, its dropped coefficients checked to
/// be rounding (within `1e-11` of the form's scale; the sign test's slack `1e-12` of it).
///
/// # Errors
/// `SM_ERR` for a patch with a weight not positive, of more than one span, or of degree
/// below 2 across a face it lies on.
pub(super) fn containment_curved(fill: &Fill, faces: &[Face], convex: bool) -> Result<f64> {
    let d_side = if convex { 1.0 } else { -1.0 };
    let n = fill.face.len();
    for (i, patch) in fill.patches.iter().enumerate() {
        let canonical = patch.canonical()?;
        if canonical.weights.iter().any(|&w| !(w > 0.0)) {
            return Err(sm_err()); // a patch with a weight not positive
        }
        if canonical.u_knots.len() != 2 || canonical.v_knots.len() != 2 {
            return Err(sm_err()); // Bernstein coefficients only for one span
        }
        let (p, q) = (canonical.u_degree, canonical.v_degree);
        let pts = &canonical.control_points;
        let rational = canonical.weights.len() == pts.len();
        for (f, face) in faces.iter().enumerate() {
            let mut w = t_make(p, q);
            for (k, x) in w.c.iter_mut().enumerate() {
                *x = if rational {
                    el(&canonical.weights, k)?
                } else {
                    1.0
                };
            }
            let origin = [face.point.x, face.point.y, face.point.z];
            let axis = [face.axis.x, face.axis.y, face.axis.z];
            let mut h = t_make(p, q);
            let mut u: [Tensor; 3] = Default::default();
            for (c, uc) in u.iter_mut().enumerate() {
                *uc = t_make(p, q);
                for (k, x) in uc.c.iter_mut().enumerate() {
                    let pt = *pts.get(k).ok_or_else(sm_err)?;
                    *x = el(&w.c, k)? * (pt.component(c) - el(&origin, c)?);
                }
                if face.kind != FaceKind::Sphere {
                    for (hk, &uk) in h.c.iter_mut().zip(&uc.c) {
                        *hk += uk * el(&axis, c)?;
                    }
                }
            }
            let mut form;
            let scale;
            if face.kind == FaceKind::Plane {
                // a plane beside a curved face: w (X - X0) . N, the distance's sign already
                form = t_make(p, q);
                let mut s = 0.0;
                for (c, uc) in u.iter().enumerate() {
                    let nc = face.normal.component(c);
                    form = t_axpy(&form, nc, uc)?;
                    s += nc.abs() * t_scale(uc);
                }
                scale = s;
                for x in &mut form.c {
                    *x *= -d_side;
                }
            } else {
                let mut a = t_make(0, 0);
                for (c, uc) in u.iter().enumerate() {
                    let r = if face.kind == FaceKind::Sphere {
                        uc.clone()
                    } else {
                        t_axpy(uc, -el(&axis, c)?, &h)?
                    };
                    a = t_axpy(&a, 1.0, &t_mul_fast(&r, &r)?)?;
                }
                let rho = if face.kind == FaceKind::Sphere {
                    t_axpy(&t_make(p, q), face.radius, &w)?
                } else {
                    t_axpy(&t_axpy(&t_make(p, q), face.radius, &w)?, face.slope, &h)?
                };
                let b = t_mul_fast(&rho, &rho)?;
                form = t_axpy(&a, -1.0, &b)?;
                scale = t_scale(&a) + t_scale(&b);
                // >= 0 where contained
                for x in &mut form.c {
                    *x *= -d_side * face.side;
                }
            }
            // a column of this patch on this face
            let col = if i < n {
                if el(&fill.face, i)? == f {
                    Some(0)
                } else if el(&fill.face, (i + 1) % n)? == f {
                    Some(1)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(col) = col {
                let qq = form.q;
                if qq < 2 {
                    return Err(sm_err());
                }
                let mut r = t_make(form.p, qq - 2);
                let mut dropped: f64 = 0.0;
                for a in 0..=form.p {
                    for j in 0..=qq {
                        let c = el(&form.c, a * (qq + 1) + j)?;
                        let jj = if col == 0 { j } else { qq - j }; // the power of v (or 1 - v)
                        if jj < 2 {
                            dropped = smax(dropped, c.abs());
                            continue;
                        }
                        let f2 =
                            (qq as f64) * ((qq - 1) as f64) / ((jj as f64) * ((jj - 1) as f64));
                        let at = a * (qq - 1) + if col == 0 { j - 2 } else { j };
                        *el_mut(&mut r.c, at)? = c * f2;
                    }
                }
                if dropped > 1.0e-11 * scale {
                    return Ok(f64::INFINITY);
                }
                form = r;
            }
            if t_nonneg(&form, 1.0e-12 * scale, GRAPH_DEPTH)? != 1 {
                return Ok(f64::INFINITY);
            }
        }
    }
    Ok(0.0)
}
