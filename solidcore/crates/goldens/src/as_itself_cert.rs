//! Certificate of an offset's base extended as itself (#21288, WR-458; the kernel's
//! `extend_as_itself` in `solidcore::surface::offset_surface`): a surface of revolution
//! whose generator is an arc of one circle (or a torus or sphere) continued as the same
//! surface of revolution, read off the extension's own control net. No sampling.
//!
//! With `B` the base's NURB and `E` the extension's, the rows `r` of a net taken round the
//! axis and its columns `j` along the generator, all in E's indices, and `r0` the row of
//! B's first rotation row:
//!
//! * **(A) E is B on B's domain, bit for bit.** In each direction E's knots in B's
//!   interval are B's, with each end that E grows once less (multiplicity `p` instead of
//!   `p + 1`), and B's net is the block of E's net at the offset of the rows E adds before
//!   it, bit for bit. On a span inside B's domain the basis functions of degree `p` read
//!   only the `p` knots on either side of it (Cox-de Boor), which are the same in both,
//!   so E evaluates to B there exactly: the gap is `0`.
//! * **(T) Every row is a rotation-scaling of row `r0`.** With the axis through `o` along
//!   unit `z`, `e1` the unit radial direction of `r0`'s control point farthest from the
//!   axis (column `j*`) and `e2 = z x e1`, each row `r` defines the complex number
//!   `lambda_r = radial(P_{r,j*}) / |radial(P_{r0,j*})|` in the basis `(e1, e2)` and the
//!   weight ratio `a_r = w_{r,j*} / w_{r0,j*}`; then for every column `j`,
//!   `P_{r,j} = foot(P_{r0,j}) + lambda_r radial(P_{r0,j})` (complex product in the plane
//!   square to the axis) and `w_{r,j} = a_r w_{r0,j}`. So for every `(u, v)`,
//!   `E(u, v) = foot(C(v)) + lambda(u) radial(C(v))`, `C` the rational curve of row `r0`
//!   and `lambda(u) = sum N_r a_r lambda_r / sum N_r a_r` (the `M_j(v)` sums factor out:
//!   the maps are affine and the weights a product).
//! * **(R) `lambda(u)` has modulus one.** Round the axis the knots are those of rational
//!   quadratic spans (interior knots of multiplicity 2, `2 n + 1` rows), and column `j*`'s
//!   rows are, span by span, rational quadratic arcs of one circle about the axis
//!   ([`arc_on_circle`]), all turning one way. Then `foot + lambda(u) radial(P_{r0,j*})`
//!   is on that circle for every `u`, so `|lambda(u)| = 1`: `lambda(u)` is a turn about
//!   the axis, and `E(u, v)` is `C(v)` turned about the axis.
//! * **(G) `C` is on the generator's circle.** Along the generator the knots are those of
//!   rational quadratic spans too, and row `r0`'s spans are arcs of one circle in the
//!   meridian plane through the axis and `e1` (normal `e2`), the circle read off B's first
//!   generator span; row `r0` is at a knot of multiplicity at least `p` of E (B's first
//!   rotation knot), so `C` is E's own curve there, on the surface.
//!
//! So E is the revolution of the generator's circle about the axis: B's own surface (A,
//! with B's net among the rows checked), continued as itself, and G1 across every join
//! (consecutive arcs of one circle turning one way meet tangent).
//!
//! Rounding: each check is an equality of the construction's rounded values, allowed
//! `gamma(ROUNDING_OPS)` of the net's magnitude (its largest `|P|` plus `|o|`), or
//! relatively for weights. The construction forms each control point from B's through
//! at most 3 chained turns (`turn_about`: 17 rounded operations and a sine and a cosine,
//! each within an ulp, counted as 2 operations each), a radial scaling (4), the middle
//! points' `1 / cos` (2) and the homogeneous round trip (2): at most 3 x 21 + 8 = 71
//! operations on values of the net's magnitude; each check's own evaluation adds at most
//! 40 more (a residual of products of differences). `ROUNDING_OPS = 128` bounds the sum.
//! What a pass proves is the statement up to those allowances (as `crate::g1r_cert`'s).

use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::math::{gamma, CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::{Surface, SurfaceKind};

/// The rounded operations a check allows (see the module docs).
const ROUNDING_OPS: usize = 128;

/// What [`certify_as_itself`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AsItselfCert {
    /// Rational quadratic spans E adds round the axis.
    pub(crate) rot_spans_added: usize,
    /// Rational quadratic spans E adds along the generator.
    pub(crate) gen_spans_added: usize,
    /// The largest residual of any check relative to its allowance.
    pub(crate) worst: f64,
}

/// The homogeneous components of `c` (as `crate::g1r_cert`'s `h`).
fn h(c: CPoint) -> [f64; 4] {
    let z = if c.z == NOZ { 0.0 } else { c.z };
    let w = if c.w == NOW { 1.0 } else { c.w };
    [c.x, c.y, z, w]
}

/// `c`'s point and weight.
fn pw(c: CPoint) -> (Vec3, f64) {
    let [x, y, z, w] = h(c);
    (Vec3::new(x / w, y / w, z / w), w)
}

/// The surface of revolution a base that extends as itself is, read through its kind.
fn revolution(k: &Kernel, base: &Surface) -> Result<SurfOfRevolution, String> {
    match base.kind_with_nurb(k).map_err(|e| e.to_string())? {
        SurfaceKind::SurfOfRevolution(r) => Ok((**r).clone()),
        SurfaceKind::Torus(t) => Ok(t.base().clone()),
        SurfaceKind::Sphere(s) => Ok(s.base().clone()),
        SurfaceKind::Cone(c) => Ok(c.rev().clone()),
        _ => Err(format!(
            "a {} is not a surface of revolution with a circle for its generator",
            base.type_code()
        )),
    }
}

/// Knots of rational quadratic spans: degree 2, ends of multiplicity 3, every interior
/// knot of multiplicity 2, `2 n + 1` control points for `n` spans. Returns `n`.
fn quadratic_spans(knots: &[f64], deg: usize, count: usize) -> Result<usize, String> {
    if deg != 2 {
        return Err(format!("degree {deg}, not 2"));
    }
    let mut distinct: Vec<(f64, usize)> = Vec::new();
    for &t in knots {
        match distinct.last_mut() {
            Some((x, m)) if *x == t => *m += 1,
            _ => distinct.push((t, 1)),
        }
    }
    let n = distinct.len().saturating_sub(1);
    let last = distinct.len().saturating_sub(1);
    for (i, &(t, m)) in distinct.iter().enumerate() {
        let want = if i == 0 || i == last { 3 } else { 2 };
        if m != want {
            return Err(format!("knot {t} of multiplicity {m}, not {want}"));
        }
    }
    if n == 0 || count != 2 * n + 1 {
        return Err(format!("{count} control points for {n} spans"));
    }
    Ok(n)
}

/// (A) in one direction: E's knots `ke` on B's interval are B's `kb`, each end E grows
/// once less (multiplicity `p`, not `p + 1`). Returns the control points E adds before
/// B's first (one per knot E adds below B's interval, less the end knot it lowers).
fn sub_knots(kb: &[f64], ke: &[f64]) -> Result<usize, String> {
    let (Some(&lo), Some(&hi)) = (kb.first(), kb.last()) else {
        return Err("no knots".into());
    };
    let below = ke.iter().filter(|&&t| t < lo).count();
    let above = ke.iter().filter(|&&t| t > hi).count();
    let mut want: Vec<f64> = kb.to_vec();
    if below > 0 {
        want.remove(0);
    }
    if above > 0 {
        want.pop();
    }
    let inside: Vec<f64> = ke.iter().copied().filter(|&t| lo <= t && t <= hi).collect();
    if inside
        .iter()
        .map(|t| t.to_bits())
        .ne(want.iter().map(|t| t.to_bits()))
    {
        return Err(format!(
            "E's knots on B's interval {inside:?} are not B's {want:?}"
        ));
    }
    Ok(below.saturating_sub(1))
}

/// Knots of a degree-1 direction: ends of multiplicity 2, one control point per knot
/// past the first. Returns the spans.
fn linear_spans(knots: &[f64], deg: usize, count: usize) -> Result<usize, String> {
    if deg != 1 {
        return Err(format!("degree {deg}, not 1"));
    }
    if count < 2 || knots.len() != count + 2 {
        return Err(format!("{count} control points for {} knots", knots.len()));
    }
    Ok(count - 1)
}

/// The residual of a check relative to its allowance; an error past one.
fn within(what: &str, resid: f64, allow: f64, worst: &mut f64) -> Result<(), String> {
    let q = resid / allow;
    // (an infinite or undefined residual or allowance proves nothing)
    if !(resid.is_finite() && allow.is_finite() && q <= 1.0) {
        return Err(format!("{what}: residual {resid:e} > {allow:e}"));
    }
    *worst = worst.max(q);
    Ok(())
}

/// One span `(p0, w0), (p1, w1), (p2, w2)` is a rational quadratic arc of the circle
/// about `c` of radius `rho` in the plane through `c` square to unit `n`: the three
/// points in that plane, the ends on the circle, `p1 - p0` and `p1 - p2` square to the
/// radii at the ends (`p1` where the end tangents meet) and
/// `w1 / sqrt(w0 w2) = rho / |p1 - c|`, the cosine of half the arc (an arc of less than a
/// half turn, `0 < cos < 1`). Those are the rational quadratic arc's conditions (the
/// conic through `p0` and `p2` tangent there to `p0 p1` and `p2 p1`, its shape fixed by
/// the weight). Returns the turn's sense, `((p0 - c) x (p2 - c)) . n`'s sign.
#[allow(clippy::too_many_arguments)]
fn arc_on_circle(
    what: &str,
    [(p0, w0), (p1, w1), (p2, w2)]: [(Vec3, f64); 3],
    c: Vec3,
    n: Vec3,
    rho: f64,
    tau: f64,
    worst: &mut f64,
) -> Result<f64, String> {
    for (i, p) in [p0, p1, p2].iter().enumerate() {
        within(
            &format!("{what}: point {i} off the circle's plane"),
            (*p - c).dot(n).abs(),
            tau,
            worst,
        )?;
    }
    within(
        &format!("{what}: start off the circle"),
        ((p0 - c).length() - rho).abs(),
        tau,
        worst,
    )?;
    within(
        &format!("{what}: end off the circle"),
        ((p2 - c).length() - rho).abs(),
        tau,
        worst,
    )?;
    let l0 = (p1 - p0).length();
    let l2 = (p1 - p2).length();
    within(
        &format!("{what}: no tangent at the start"),
        (p1 - p0).dot(p0 - c).abs(),
        tau * (l0 + rho),
        worst,
    )?;
    within(
        &format!("{what}: no tangent at the end"),
        (p1 - p2).dot(p2 - c).abs(),
        tau * (l2 + rho),
        worst,
    )?;
    if !(w0 > 0.0 && w1 > 0.0 && w2 > 0.0) {
        return Err(format!("{what}: a non-positive weight"));
    }
    let d1 = (p1 - c).length();
    let cos = rho / d1;
    if !(cos > 0.0 && cos < 1.0) {
        return Err(format!("{what}: no arc (cos {cos})"));
    }
    within(
        &format!("{what}: middle weight not the half arc's cosine"),
        (w1 / (w0 * w2).sqrt() - cos).abs(),
        tau / rho * (1.0 + cos) + gamma(ROUNDING_OPS),
        worst,
    )?;
    let sense = (p0 - c).cross(p2 - c).dot(n);
    if sense == 0.0 {
        return Err(format!("{what}: no sense of turn"));
    }
    Ok(sense.signum())
}

/// The certificate of `e`, an offset's base `base` extended as itself (module docs).
pub(crate) fn certify_as_itself(
    k: &Kernel,
    base: &Surface,
    e: &NlSurface,
) -> Result<AsItselfCert, String> {
    let base = match base.kind() {
        SurfaceKind::OffsetSurface(o) => o.base_surface(),
        _ => base,
    };
    let rev = revolution(k, base)?;
    let bn = rev
        .base()
        .gw_nurb()
        .cloned()
        .ok_or("the base has no NURB")?;
    if bn.degrees() != e.degrees() {
        return Err(format!(
            "degrees {:?} against B's {:?}",
            e.degrees(),
            bn.degrees()
        ));
    }
    let (pu, pv) = bn.degrees();
    // (A)
    let off_u = sub_knots(bn.u_knots(), e.u_knots())?;
    let off_v = sub_knots(bn.v_knots(), e.v_knots())?;
    let (bc, ec) = (bn.cpts(), e.cpts());
    for (i, row) in bc.iter().enumerate() {
        for (j, b) in row.iter().enumerate() {
            let x = ec
                .get(i + off_u)
                .and_then(|r| r.get(j + off_v))
                .ok_or("E lost B's rows")?;
            if h(*b).map(f64::to_bits) != h(*x).map(f64::to_bits) {
                return Err(format!("B's control point {i},{j} moved in E"));
            }
        }
    }
    // E's net as g[r][j]: r round the axis, j along the generator
    let rot_u = !rev.swap_uv();
    let (nu, nv) = (ec.len(), ec.first().map_or(0, Vec::len));
    if ec.iter().any(|r| r.len() != nv) {
        return Err("a ragged net".into());
    }
    let at = |r: usize, j: usize| -> Result<(Vec3, f64), String> {
        let c = if rot_u {
            ec.get(r).and_then(|x| x.get(j))
        } else {
            ec.get(j).and_then(|x| x.get(r))
        };
        c.copied()
            .map(pw)
            .ok_or_else(|| format!("no control point {r},{j}"))
    };
    let (n_rot, n_gen, r0, k_rot, k_gen, kb_rot, kb_gen) = if rot_u {
        (
            nu,
            nv,
            off_u,
            e.u_knots(),
            e.v_knots(),
            bn.u_knots(),
            bn.v_knots(),
        )
    } else {
        (
            nv,
            nu,
            off_v,
            e.v_knots(),
            e.u_knots(),
            bn.v_knots(),
            bn.u_knots(),
        )
    };
    let rot_spans = quadratic_spans(k_rot, if rot_u { pu } else { pv }, n_rot)?;
    let p_gen = if rot_u { pv } else { pu };
    let (nb_rot, nb_gen) = if rot_u {
        (bc.len(), bc.first().map_or(0, Vec::len))
    } else {
        (bc.first().map_or(0, Vec::len), bc.len())
    };
    let spans = |k: &[f64], n| {
        if p_gen == 1 {
            linear_spans(k, 1, n)
        } else {
            quadratic_spans(k, p_gen, n)
        }
    };
    let gen_spans = spans(k_gen, n_gen)?;
    let b_rot = quadratic_spans(kb_rot, 2, nb_rot)?;
    let b_gen = spans(kb_gen, nb_gen)?;

    let o = rev.position().origin();
    let z = rev
        .position()
        .z_axis()
        .unitize()
        .map_err(|x| x.to_string())?;
    let radial = |x: Vec3| {
        let v = x - o;
        v - z * z.dot(v)
    };
    let mut big = 0.0_f64;
    for r in 0..n_rot {
        for j in 0..n_gen {
            big = big.max(at(r, j)?.0.length());
        }
    }
    let tau = gamma(ROUNDING_OPS) * (big + o.length());
    let tw = gamma(ROUNDING_OPS);
    let mut worst = 0.0_f64;

    // (T) the reference column: row r0's point farthest from the axis
    let (mut js, mut dmax) = (0, -1.0_f64);
    for j in 0..n_gen {
        let d = radial(at(r0, j)?.0).length();
        if d > dmax {
            dmax = d;
            js = j;
        }
    }
    if dmax.partial_cmp(&tau) != Some(std::cmp::Ordering::Greater) {
        return Err("row r0 lies on the axis".into());
    }
    let (ps, ws) = at(r0, js)?;
    let e1 = radial(ps) * (1.0 / dmax);
    let e2 = z.cross(e1);
    for r in 0..n_rot {
        let (pr, wr) = at(r, js)?;
        let rr = radial(pr);
        let (lx, ly) = (rr.dot(e1) / dmax, rr.dot(e2) / dmax);
        let a = wr / ws;
        for j in 0..n_gen {
            let (q, wq) = at(r0, j)?;
            let rq = radial(q);
            let (qx, qy) = (rq.dot(e1), rq.dot(e2));
            let want = (q - rq) + e1 * (lx * qx - ly * qy) + e2 * (lx * qy + ly * qx);
            let (p, w) = at(r, j)?;
            within(
                &format!("row {r} column {j}: not row r0's turned"),
                (p - want).length(),
                tau,
                &mut worst,
            )?;
            within(
                &format!("row {r} column {j}: weight not row r0's scaled"),
                (w - a * wq).abs(),
                tw * w.abs(),
                &mut worst,
            )?;
        }
    }
    // (R) column j*'s rotation spans: arcs of one circle about the axis, one sense
    let foot = ps - radial(ps);
    let mut sense = 0.0;
    for s in 0..rot_spans {
        let tri = [at(2 * s, js)?, at(2 * s + 1, js)?, at(2 * s + 2, js)?];
        let sg = arc_on_circle(
            &format!("rotation span {s}"),
            tri,
            foot,
            z,
            dmax,
            tau,
            &mut worst,
        )?;
        if sense != 0.0 && sg != sense {
            return Err(format!("rotation span {s} turns back"));
        }
        sense = sg;
    }
    // (G) row r0 along the generator: a line's points on B's line, in order; a circle's
    // spans arcs of the circle of B's first generator span
    let g0 = if rot_u { off_v } else { off_u };
    if p_gen == 1 {
        let (q0, q1) = (at(r0, g0)?.0, at(r0, g0 + 1)?.0);
        let d = q1 - q0;
        let dl = d.length();
        if dl.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Err("B's generator span is a point".into());
        }
        let mut last = f64::NEG_INFINITY;
        for j in 0..n_gen {
            let (q, w) = at(r0, j)?;
            if w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return Err(format!("generator point {j}: a non-positive weight"));
            }
            let t = (q - q0).dot(d) / dl;
            within(
                &format!("generator point {j} off B's line"),
                ((q - q0) - d * (t / dl)).length(),
                tau,
                &mut worst,
            )?;
            if t.partial_cmp(&last) != Some(std::cmp::Ordering::Greater) {
                return Err(format!("generator point {j} turns back along the line"));
            }
            last = t;
        }
    } else {
        let (q0, q1, q2) = (at(r0, g0)?.0, at(r0, g0 + 1)?.0, at(r0, g0 + 2)?.0);
        let m = (q0 + q2) * 0.5;
        let dm = (m - q1).length();
        let l = (q1 - q0).length();
        if !(dm > 0.0 && l > 0.0) {
            return Err("B's first generator span is no arc".into());
        }
        let c = q1 + (m - q1) * (l * l / (dm * dm));
        let rho = (q0 - c).length();
        let n = (q1 - q0)
            .cross(q2 - q0)
            .unitize()
            .map_err(|x| format!("B's first generator span spans no plane: {x}"))?;
        let mut sense = 0.0;
        for s in 0..gen_spans {
            let tri = [at(r0, 2 * s)?, at(r0, 2 * s + 1)?, at(r0, 2 * s + 2)?];
            let sg = arc_on_circle(
                &format!("generator span {s}"),
                tri,
                c,
                n,
                rho,
                tau,
                &mut worst,
            )?;
            if sense != 0.0 && sg != sense {
                return Err(format!("generator span {s} turns back"));
            }
            sense = sg;
        }
    }
    Ok(AsItselfCert {
        rot_spans_added: rot_spans - b_rot,
        gen_spans_added: gen_spans - b_gen,
        worst,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::geom::vector2d::Point2;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
    use solidcore::surface::torus::{Torus, TorusOptions};

    /// An offset of a quarter torus extended past both ends of each direction (and the
    /// extension it holds).
    fn torus_extension(swap: bool) -> (Kernel, Surface, NlSurface) {
        let k = Kernel::default();
        let t = Torus::new(
            &k,
            Vec3::new(1.0, -2.0, 0.5),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Extent2d::from_bounds(10.0, 20.0, 100.0, 140.0),
            5.0,
            1.5,
            TorusOptions {
                swap_uv: swap,
                ..TorusOptions::default()
            },
        )
        .unwrap();
        let base = Surface::from(t);
        let mut o = OffsetSurface::new(&k, 0.25, base.clone(), true, SS_UNKNOWN).unwrap();
        o.set_allow_extension(true);
        let d = o.natural_uv_domain();
        let (s, lo, hi) = (d.size(), d.min(), d.max());
        for uv in [
            Point2::new(hi.x + 0.4 * s.x, hi.y + 0.3 * s.y),
            Point2::new(lo.x - 0.3 * s.x, lo.y - 0.2 * s.y),
        ] {
            o.evaluate_extending(&k, uv, 0, 0, EvaluateFlags::default())
                .unwrap();
        }
        let e = match o.extended_base_surface().map(Surface::kind) {
            Some(SurfaceKind::BSpline(b)) => b.gw_nurb().cloned().unwrap(),
            _ => panic!(
                "no B-spline extension: {:?}",
                o.extended_base_surface().map(Surface::type_code)
            ),
        };
        (k, base, e)
    }

    /// `e` with one of its control points moved by `by` (homogeneous x).
    fn moved(e: &NlSurface, i: usize, j: usize, by: f64) -> NlSurface {
        let mut c = e.cpts();
        c[i][j].x += by;
        let (p, q) = e.degrees();
        NlSurface::new(c, p, q, e.u_knots().to_vec(), e.v_knots().to_vec()).unwrap()
    }

    #[test]
    fn a_torus_extended_as_itself_is_certified() {
        for swap in [false, true] {
            let (k, base, e) = torus_extension(swap);
            let c = certify_as_itself(&k, &base, &e).unwrap();
            assert!(c.rot_spans_added > 0 && c.gen_spans_added > 0, "{c:?}");
            assert!(c.worst <= 1.0);
        }
    }

    #[test]
    fn a_moved_control_point_is_refused() {
        let (k, base, e) = torus_extension(false);
        // an added corner (no base row), an added middle point, and one of B's own
        for (i, j) in [
            (0, 0),
            (1, e.cpts()[0].len() / 2),
            (e.cpts().len() / 2, e.cpts()[0].len() / 2),
        ] {
            let bad = moved(&e, i, j, 1e-9);
            assert!(certify_as_itself(&k, &base, &bad).is_err(), "{i},{j}");
        }
    }

    #[test]
    fn a_moved_weight_is_refused() {
        let (k, base, e) = torus_extension(false);
        let mut c = e.cpts();
        c[1][1].w *= 1.0 + 1e-9;
        let (p, q) = e.degrees();
        let bad = NlSurface::new(c, p, q, e.u_knots().to_vec(), e.v_knots().to_vec()).unwrap();
        assert!(certify_as_itself(&k, &base, &bad).is_err());
    }
}
