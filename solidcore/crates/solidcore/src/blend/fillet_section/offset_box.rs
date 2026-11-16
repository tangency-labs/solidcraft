//! Enclosures of an offset surface `Q(u, v) = S(u, v) + d (S_u x S_v) / |S_u x S_v|` and
//! its partial derivatives up to second order over a UV box of its base NURBS net, in
//! outward-rounded [`Ivl`] arithmetic.
//!
//! Not upstream. Built on [`nurbs_partials_over_piece`] and [`partials_hull`] so UV-lift
//! certification ([`crate::surface::surface::exact_pieces::certified_lift`]) shares the G^n
//! certificate's interval and rational B-spline machinery with zero duplicate interval type
//! or heuristic rounding slack.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::certificate::{nurbs_partials_over_piece, partials_hull};
use super::interval::Ivl;
use super::series::{v_add, v_hull, v_scale, v_sub, V3, VZ};
use super::station::{Nurbs, Partials};
use crate::error::{Result, SolidError};

const OP: &str = "blend::fillet_section::offset_box";

#[inline]
fn v_dot(a: V3<Ivl>, b: V3<Ivl>) -> Ivl {
    (a.x * b.x + a.y * b.y) + a.z * b.z
}

#[inline]
fn v_cross(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// The partials `Q[a][b]`, `a + b <= lk` (`lk <= 2`), of the offset surface
/// `Q(u, v) = S(u, v) + d (S_u x S_v) / |S_u x S_v|` on one polynomial piece `(ku, kv)` of
/// `net` over `[ua, ub] x [va, vb]`, enclosed in [`Ivl`].
///
/// The differential formulas match [`crate::surface::offset_surface::OffsetSurface`]
/// (`EvaluatePlainOrSimple`), evaluated in rounded intervals from `net`'s partials up to
/// order `lk + 1`. Where `|S_u x S_v|` is not proved strictly positive over the box (for
/// example across a wide circular quadrant before halving, or at a pole), `1 / |S_u x S_v|`
/// evaluates to [`Ivl::whole`], which no finite tolerance passes and therefore forces
/// subdivision on smooth spans.
///
/// # Errors
///
/// [`SolidError::PortInvariant`] when `lk > 2`; those of [`nurbs_partials_over_piece`].
#[allow(clippy::too_many_arguments)]
pub fn offset_partials_over_piece(
    net: &Nurbs,
    d: Ivl,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    if lk > 2 {
        return Err(SolidError::port_invariant(
            OP,
            "offset partials requested past second order",
        ));
    }
    // Exact-zero offset identity (Q = S), not a tolerance.
    if d.lo == 0.0 && d.hi == 0.0 {
        return nurbs_partials_over_piece(net, ku, kv, ua, ub, va, vb, lk);
    }
    let b = nurbs_partials_over_piece(net, ku, kv, ua, ub, va, vb, lk + 1)?;
    let s = b.at(0, 0);
    let bu = b.at(1, 0);
    let bv = b.at(0, 1);
    let c = v_cross(bu, bv);
    let m_len = v_dot(c, c).sqrt();
    let inv_len = Ivl::from(1.0) / m_len;
    let n = v_scale(c, inv_len);

    let mut out = Partials::default();
    out.set(0, 0, v_add(s, v_scale(n, d)));
    if lk == 0 {
        return Ok(out);
    }

    let b20 = b.at(2, 0);
    let b11 = b.at(1, 1);
    let b02 = b.at(0, 2);

    let cu = v_add(v_cross(b20, bv), v_cross(bu, b11));
    let mu = v_dot(n, cu);
    let nu = v_scale(v_sub(cu, v_scale(n, mu)), inv_len);
    out.set(1, 0, v_add(bu, v_scale(nu, d)));

    let cv = v_add(v_cross(b11, bv), v_cross(bu, b02));
    let mv = v_dot(n, cv);
    let nv = v_scale(v_sub(cv, v_scale(n, mv)), inv_len);
    out.set(0, 1, v_add(bv, v_scale(nv, d)));
    if lk == 1 {
        return Ok(out);
    }

    let two = Ivl::from(2.0);
    let b30 = b.at(3, 0);
    let b21 = b.at(2, 1);
    let b12 = b.at(1, 2);
    let b03 = b.at(0, 3);

    let cuu = v_add(
        v_add(v_cross(b30, bv), v_cross(v_scale(b20, two), b11)),
        v_cross(bu, b21),
    );
    let muu = (v_dot(cu, cu) + v_dot(c, cuu) - mu * mu) * inv_len;
    let nuu = v_scale(
        v_sub(v_sub(cuu, v_scale(nu, two * mu)), v_scale(n, muu)),
        inv_len,
    );
    out.set(2, 0, v_add(b20, v_scale(nuu, d)));

    let cuv = v_add(v_add(v_cross(b21, bv), v_cross(b20, b02)), v_cross(bu, b12));
    let muv = (v_dot(cu, cv) + v_dot(c, cuv) - mu * mv) * inv_len;
    let nuv = v_scale(
        v_sub(
            v_sub(v_sub(cuv, v_scale(nv, mu)), v_scale(nu, mv)),
            v_scale(n, muv),
        ),
        inv_len,
    );
    out.set(1, 1, v_add(b11, v_scale(nuv, d)));

    let cvv = v_add(
        v_add(v_cross(b12, bv), v_cross(v_scale(b11, two), b02)),
        v_cross(bu, b03),
    );
    let mvv = (v_dot(cv, cv) + v_dot(c, cvv) - mv * mv) * inv_len;
    let nvv = v_scale(
        v_sub(v_sub(cvv, v_scale(nv, two * mv)), v_scale(n, mvv)),
        inv_len,
    );
    out.set(0, 2, v_add(b02, v_scale(nvv, d)));

    Ok(out)
}

/// The offset surface's partials to total order `lk` (`lk <= 2`) over every part of the box
/// `pu` x `pv`, each on its own polynomial piece ([`offset_partials_over_piece`]), hulled
/// ([`partials_hull`]).
///
/// # Errors
///
/// Those of [`offset_partials_over_piece`]; [`SolidError::PortInvariant`] for an empty box.
pub fn offset_partials_over_parts(
    net: &Nurbs,
    d: Ivl,
    pu: &[(f64, f64, usize)],
    pv: &[(f64, f64, usize)],
    lk: usize,
) -> Result<Partials<Ivl>> {
    let mut out: Option<Partials<Ivl>> = None;
    for &(ua, ub, ku) in pu {
        for &(va, vb, kv) in pv {
            let p = offset_partials_over_piece(net, d, ku, kv, ua, ub, va, vb, lk)?;
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out.ok_or_else(|| SolidError::port_invariant(OP, "an offset UV box without parts"))
}

/// Symmetric interval `[-x.mag(), +x.mag()]` componentwise, holding `v` and `-v`.
fn v_sym(v: V3<Ivl>) -> V3<Ivl> {
    let sym = |x: Ivl| {
        let m = x.mag();
        if m.is_nan() {
            Ivl::whole()
        } else {
            Ivl::new(-m, m)
        }
    };
    V3 {
        x: sym(v.x),
        y: sym(v.y),
        z: sym(v.z),
    }
}

fn unit_normal_over_parts(
    net: &Nurbs,
    pu: &[(f64, f64, usize)],
    pv: &[(f64, f64, usize)],
) -> Result<V3<Ivl>> {
    let mut out: Option<V3<Ivl>> = None;
    for &(ua, ub, ku) in pu {
        for &(va, vb, kv) in pv {
            let b = nurbs_partials_over_piece(net, ku, kv, ua, ub, va, vb, 1)?;
            let m = v_cross(b.at(1, 0), b.at(0, 1));
            let inv_l = Ivl::from(1.0) / v_dot(m, m).sqrt();
            let n = v_scale(m, inv_l);
            out = Some(match out {
                None => n,
                Some(h) => v_hull(h, n),
            });
        }
    }
    out.ok_or_else(|| SolidError::port_invariant(OP, "an offset surface UV box without parts"))
}

/// Encloses the total jump of `Q(u, v)` across any multiplicity-`p` inner `u`-knot strictly
/// crossed by `pu` and any multiplicity-`q` inner `v`-knot strictly crossed by `pv`.
///
/// Across a knot of multiplicity strictly below the degree, `net` is at least `C^1`, so
/// `S_u x S_v` and `Q` are continuous (`|S_u x S_v| = 0` already forces [`Ivl::whole`] in
/// [`offset_partials_over_piece`]) and contribute zero jump. Across a knot of multiplicity
/// equal to the degree (`mult == p` or `mult == q`, where `S` itself is identically `C^0`
/// continuous across the knot line), `Q_after - Q_before = d (n_after - n_before)` is
/// enclosed directly in [`Ivl`] with no heuristic slack: when `mono_u` (resp. `mono_v`) holds
/// on the parameter span, the curve crosses each such knot at most once, contributing
/// `v_sym(d (n_after - n_before))`; when the coordinate is not proved monotonic (or
/// `mult > degree`), returns `Ok(None)` so the caller uses the unconditional direct range
/// enclosure on that span.
///
/// # Errors
///
/// Those of [`nurbs_partials_over_piece`].
pub fn offset_crossed_knot_jumps(
    net: &Nurbs,
    d: Ivl,
    pu: &[(f64, f64, usize)],
    pv: &[(f64, f64, usize)],
    mono_u: bool,
    mono_v: bool,
) -> Result<Option<V3<Ivl>>> {
    // Exact-zero offset identity (Q = S), not a tolerance.
    if d.lo == 0.0 && d.hi == 0.0 {
        return Ok(Some(VZ));
    }
    let mut jump = VZ;
    for (&(ua, ub, ku_l), &(uc, ud, ku_r)) in pu.iter().zip(pu.iter().skip(1)) {
        if !(ua < ub && ub == uc && uc < ud) {
            continue;
        }
        let mult = net.u.iter().filter(|&&k| k == ub).count();
        if mult < net.p {
            continue;
        }
        if mult > net.p || !mono_u {
            return Ok(None);
        }
        let n_l = unit_normal_over_parts(net, &[(ub, ub, ku_l)], pv)?;
        let n_r = unit_normal_over_parts(net, &[(ub, ub, ku_r)], pv)?;
        jump = v_add(jump, v_sym(v_scale(v_sub(n_r, n_l), d)));
    }
    for (&(va, vb, kv_l), &(vc, vd, kv_r)) in pv.iter().zip(pv.iter().skip(1)) {
        if !(va < vb && vb == vc && vc < vd) {
            continue;
        }
        let mult = net.v.iter().filter(|&&k| k == vb).count();
        if mult < net.q {
            continue;
        }
        if mult > net.q || !mono_v {
            return Ok(None);
        }
        let n_l = unit_normal_over_parts(net, pu, &[(vb, vb, kv_l)])?;
        let n_r = unit_normal_over_parts(net, pu, &[(vb, vb, kv_r)])?;
        jump = v_add(jump, v_sym(v_scale(v_sub(n_r, n_l), d)));
    }
    Ok(Some(jump))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::nurbs::srf_basic::Surface as NlSurface;

    #[test]
    fn offset_partials_enclose_exact_curved_offset_up_to_second_order() {
        // S(u, v) = (u, v, 0.5 u^2) on [0, 1]^2, offset by d = 0.4.
        let srf_cpts = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 1.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(0.5, 0.0, 0.0, NOW),
                CPoint::new(0.5, 1.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(1.0, 0.0, 0.5, NOW),
                CPoint::new(1.0, 1.0, 0.5, NOW),
            ],
        ];
        let nl = NlSurface::new(
            srf_cpts,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let bs = BSplineSurface::from_nurb(nl, false);
        let net = Nurbs::of(&bs).unwrap();
        let d = 0.4_f64;
        let d_ivl = Ivl::from(d);

        // Point-interval enclosure at (0.5, 0.5) must hold the analytic value tightly.
        let pt_enc = offset_partials_over_piece(&net, d_ivl, 2, 1, 0.5, 0.5, 0.5, 0.5, 2).unwrap();
        let u = 0.5_f64;
        let v = 0.5_f64;
        let s2 = 1.0 + u * u;
        let inv1 = 1.0 / s2.sqrt();
        let inv3 = inv1 / s2;
        let inv5 = inv3 / s2;
        let q_exact = [u - d * u * inv1, v, 0.5 * u * u + d * inv1];
        let qu_exact = [1.0 - d * inv3, 0.0, u - d * u * inv3];
        let qv_exact = [0.0, 1.0, 0.0];
        let quu_exact = [
            3.0 * d * u * inv5,
            0.0,
            1.0 - d * (1.0 - 2.0 * u * u) * inv5,
        ];

        let holds = |ivl: V3<Ivl>, ex: [f64; 3]| {
            ivl.x.lo <= ex[0]
                && ex[0] <= ivl.x.hi
                && ivl.y.lo <= ex[1]
                && ex[1] <= ivl.y.hi
                && ivl.z.lo <= ex[2]
                && ex[2] <= ivl.z.hi
        };
        assert!(holds(pt_enc.at(0, 0), q_exact));
        assert!(holds(pt_enc.at(1, 0), qu_exact));
        assert!(holds(pt_enc.at(0, 1), qv_exact));
        assert!(holds(pt_enc.at(2, 0), quu_exact));
        assert!(holds(pt_enc.at(1, 1), [0.0, 0.0, 0.0]));
        assert!(holds(pt_enc.at(0, 2), [0.0, 0.0, 0.0]));
        assert!(pt_enc.at(0, 0).x.hi - pt_enc.at(0, 0).x.lo < 1e-13);
        assert!(pt_enc.at(2, 0).z.hi - pt_enc.at(2, 0).z.lo < 1e-12);

        // Box enclosure over [0.2, 0.6] x [0.1, 0.9] must enclose every interior sample.
        let box_enc = offset_partials_over_piece(&net, d_ivl, 2, 1, 0.2, 0.6, 0.1, 0.9, 2).unwrap();
        for iu in 0..=4 {
            let us = 0.2 + 0.1 * f64::from(iu);
            let s2_s = 1.0 + us * us;
            let i1 = 1.0 / s2_s.sqrt();
            let i3 = i1 / s2_s;
            let i5 = i3 / s2_s;
            for iv in 0..=4 {
                let vs = 0.1 + 0.2 * f64::from(iv);
                assert!(holds(
                    box_enc.at(0, 0),
                    [us - d * us * i1, vs, 0.5 * us * us + d * i1],
                ));
                assert!(holds(
                    box_enc.at(1, 0),
                    [1.0 - d * i3, 0.0, us - d * us * i3]
                ));
                assert!(holds(
                    box_enc.at(2, 0),
                    [3.0 * d * us * i5, 0.0, 1.0 - d * (1.0 - 2.0 * us * us) * i5],
                ));
            }
        }
    }

    #[test]
    fn offset_crossed_knot_jumps_distinguish_g1_arc_knot_from_c0_crease() {
        // 1. Two-quadrant rational circular cylinder of radius 1 along z (double knot at u = 0.5):
        //    G^1 smooth across u = 0.5, so Q_after - Q_before is pure rounding (~1e-15).
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let cyl_row = |x: f64, y: f64, wt: f64| {
            vec![
                CPoint::new(x * wt, y * wt, 0.0, wt),
                CPoint::new(x * wt, y * wt, 1.0 * wt, wt),
            ]
        };
        let cyl_nl = NlSurface::new(
            vec![
                cyl_row(1.0, 0.0, 1.0),
                cyl_row(1.0, 1.0, w),
                cyl_row(0.0, 1.0, 1.0),
                cyl_row(-1.0, 1.0, w),
                cyl_row(-1.0, 0.0, 1.0),
            ],
            2,
            1,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let cyl_net = Nurbs::of(&BSplineSurface::from_nurb(cyl_nl, false)).unwrap();
        let pu_cyl = [(0.45, 0.5, 2), (0.5, 0.55, 4)];
        let pv_pt = [(0.25, 0.25, 1)];
        let g1_jump =
            offset_crossed_knot_jumps(&cyl_net, Ivl::from(0.5), &pu_cyl, &pv_pt, true, true)
                .unwrap()
                .unwrap();
        assert!(g1_jump.x.mag() < 1e-13, "{g1_jump:?}");
        assert!(g1_jump.y.mag() < 1e-13, "{g1_jump:?}");
        assert!(g1_jump.z.mag() < 1e-13, "{g1_jump:?}");
        let pv_narrow = [(0.25, 0.25 + 1e-8, 1)];
        let g1_narrow =
            offset_crossed_knot_jumps(&cyl_net, Ivl::from(0.5), &pu_cyl, &pv_narrow, true, true)
                .unwrap()
                .unwrap();
        assert!(g1_narrow.x.mag() < 1e-6 && g1_narrow.y.mag() < 1e-6 && g1_narrow.z.mag() < 1e-6);

        // 2. Sharp 90-degree C^0 tent roof z = 0.5 - |u - 0.5| offset by d = 1.0:
        //    unit normal jumps from (-1/sqrt(2), 0, 1/sqrt(2)) to (+1/sqrt(2), 0, 1/sqrt(2)),
        //    so Q_x jumps by sqrt(2) even at a single transverse v point.
        let tent_nl = NlSurface::new(
            vec![
                vec![
                    CPoint::new(0.0, 0.0, 0.0, NOW),
                    CPoint::new(0.0, 1.0, 0.0, NOW),
                ],
                vec![
                    CPoint::new(0.5, 0.0, 0.5, NOW),
                    CPoint::new(0.5, 1.0, 0.5, NOW),
                ],
                vec![
                    CPoint::new(1.0, 0.0, 0.0, NOW),
                    CPoint::new(1.0, 1.0, 0.0, NOW),
                ],
            ],
            1,
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let tent_net = Nurbs::of(&BSplineSurface::from_nurb(tent_nl, false)).unwrap();
        let pu_tent = [(0.4, 0.5, 1), (0.5, 0.6, 2)];
        let c0_jump =
            offset_crossed_knot_jumps(&tent_net, Ivl::from(1.0), &pu_tent, &pv_pt, true, true)
                .unwrap()
                .unwrap();
        assert!(c0_jump.x.hi >= std::f64::consts::SQRT_2);
        assert!(offset_crossed_knot_jumps(
            &tent_net,
            Ivl::from(1.0),
            &pu_tent,
            &pv_pt,
            false,
            true
        )
        .unwrap()
        .is_none());
    }
}
