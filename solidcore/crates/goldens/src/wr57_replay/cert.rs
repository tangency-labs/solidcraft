//! The certificates of the replay's substituted calls ([`classify`]), their counts per
//! unit, and the convex hull certificate of the vertex row.
//!
//! Each substituted call is classed by what is proven about this branch's answer against
//! main's at that call, every evaluation's rounding charged:
//! - `find_degen_param_for_direction`, both answering at different parameters: the
//!   alignment of the surviving tangent with the (oriented) direction, enclosed at both
//!   answers ([`crate::certify::alignment`]), is at this branch's answer not below main's
//!   ("proven more aligned" where the enclosures part, else "aligned equally");
//! - both answering at the same parameters, only `converged` moved (main `false`): the
//!   answer is a local maximum of the alignment, proven with the certified slopes as the
//!   `surface_p7` certificate does (an end where it grows outward; an interior answer
//!   where its curvature is negative);
//! - the inversion, both answering: this branch's answer is not farther from the test
//!   point than main's ([`crate::certify::distance`]; "proven nearer" where the enclosures
//!   part; for a maximize, `farthest`, mirrored);
//! - main refusing, this branch answering: the answer is not farther than the guess, and
//!   proven the closest point of the surface to `1e-6` relative (the surface's certified
//!   lower bound, [`solidcore::query::distance_bound::surface_distance_lower_bound`], over
//!   the whole domain); where that bound is too coarse, the answer proven nearer than the
//!   guess (enclosures apart: an added answer that improves on the caller's start, not a
//!   proof that it is the global closest point);
//! - main answering, this branch refusing: main's answer is proven farther than the guess,
//!   or a point of the domain's boundary is proven nearer the test point than main's
//!   answer by more than the call's tolerance (main's answer, reported converged, is then
//!   not the closest point; `heal_data_p3`'s sliver surfaces, whose `u = 0` edge is a
//!   segment 1e-5 long, not a pole, with the test points on it);
//! - both refusing: no answer on either side.
//!
//! A call none of these proves is classed `ARGUED ...` and listed; a call proven worse is
//! classed `FAULT ...`, which fails the replay.

use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::surface::surface::replay::{Answer, Call, Seam};
use solidcore::surface::surface::Surface;
use solidcore::Kernel;

/// The class of one substituted call: what is proven about this branch's answer (`own`)
/// against main's (`main`).
pub(super) fn classify(c: &Call<'_>, main: &Answer) -> String {
    // (a kernel of its own, with the call's tolerances and fixes: the runner's kernel and
    // its caches are not touched)
    let k = Kernel {
        tolerances: c.kernel.tolerances,
        fixes: c.kernel.fixes,
        ..Kernel::default()
    };
    match c.seam {
        Seam::Degen => degen(&k, c, main),
        Seam::Invert => invert(&k, c, main),
    }
    .to_string()
}

fn f(w: u64) -> f64 {
    f64::from_bits(w)
}

fn degen(k: &Kernel, c: &Call<'_>, main: &Answer) -> &'static str {
    let (m, n) = match (main, &c.own) {
        (Err(_), Err(_)) => return "degen: both refuse",
        (Ok(_), Err(_)) => return "ARGUED degen: main answers, this branch refuses",
        (Err(_), Ok(_)) => return "ARGUED degen: main refuses, this branch answers",
        (Ok(m), Ok(n)) => (m, n),
    };
    let w = c.input;
    let uv = Point2::new(f(w[0]), f(w[1]));
    let from_pole = w[5] == 1;
    let singular_u = w[6] == 1;
    let s = c.surface;
    let Ok(domain) = s.natural_uv_domain_with_nurb(k) else {
        return "ARGUED degen: no domain";
    };
    // (the search's orientation of the direction: toward or away from the pole)
    let (t, t_mid) = if singular_u {
        (uv.y, 0.5 * (domain.min().y + domain.max().y))
    } else {
        (uv.x, 0.5 * (domain.min().x + domain.max().x))
    };
    let Ok(mut d) = Vec3::new(f(w[2]), f(w[3]), f(w[4])).unitize() else {
        return "ARGUED degen: no direction";
    };
    if (t > t_mid && from_pole) || (t <= t_mid && !from_pole) {
        d = -d;
    }
    let (mu, nu) = (Point2::new(f(m[0]), f(m[1])), Point2::new(f(n[0]), f(n[1])));
    if m[..2] == n[..2] {
        return if m[2] == 0 && n[2] == 1 {
            local_maximum(k, s, &domain, nu, d, singular_u)
        } else {
            "ARGUED degen: the same parameters, this branch reports not converged"
        };
    }
    let at = |p: Point2| crate::certify::alignment(k, s, p, d, singular_u);
    match (at(mu), at(nu)) {
        (_, None) => "ARGUED degen: this branch's alignment is not enclosed",
        (None, Some(_)) => "ARGUED degen: main's tangent is not enclosed away from zero",
        (Some(m), Some(n)) => {
            if n.0 + n.1 < m.0 - m.1 {
                "FAULT degen: this branch's answer is proven less aligned"
            } else if n.0 - n.1 > m.0 + m.1 {
                "degen: proven more aligned"
            } else {
                "degen: aligned equally to the rounding"
            }
        }
    }
}

/// Whether `answer` (main's parameters, now reported converged) is proven a local maximum
/// of the alignment along the singular direction, as `surface_p7`'s
/// `wr57_moved_degen_answers_are_not_less_aligned` proves it.
fn local_maximum(
    k: &Kernel,
    s: &Surface,
    domain: &solidcore::geom::extent2d::Extent2d,
    answer: Point2,
    d: Vec3,
    singular_u: bool,
) -> &'static str {
    use solidcore::surface::surface::p7::{alignment_slopes, certified_alignment_evaluator};
    let sd = if singular_u {
        SurfParamType::U
    } else {
        SurfParamType::V
    };
    let (lo, hi) = if singular_u {
        (domain.min().x, domain.max().x)
    } else {
        (domain.min().y, domain.max().y)
    };
    let at_s = if singular_u { answer.x } else { answer.y };
    let scale = 1.0 + solidcore::nurbs::math::gamma(16);
    let Ok(Some(eval)) = certified_alignment_evaluator(k, s) else {
        return "ARGUED degen: converged now, no certified evaluator";
    };
    let slopes = |x: f64| alignment_slopes(&eval, answer, d, sd, x);
    let outward = |x: f64| {
        slopes(x).is_some_and(|e| {
            let err = e.f_error * scale;
            (x == lo && e.f < -err) || (x == hi && e.f > err)
        })
    };
    let Ok((closed, _)) = s.is_closed(k, domain, sd, None, true) else {
        return "ARGUED degen: converged now, closure unknown";
    };
    if at_s == lo || at_s == hi {
        let other = if at_s == lo { hi } else { lo };
        if outward(at_s) && (!closed || outward(other)) {
            "degen: converged now, an end proven a local maximum"
        } else {
            "ARGUED degen: converged now, an end not proven a local maximum"
        }
    } else if s
        .knots_with_nurb(k, sd, None)
        .map_or(true, |(kv, _)| kv.contains(&at_s))
    {
        "ARGUED degen: converged now, at a knot"
    } else if slopes(at_s).is_some_and(|e| e.f_prime + e.f_prime_error * scale < 0.0) {
        "degen: converged now, interior, curvature proven negative"
    } else {
        "ARGUED degen: converged now, interior, curvature not proven negative"
    }
}

fn invert(k: &Kernel, c: &Call<'_>, main: &Answer) -> &'static str {
    let w = c.input;
    let p = Vec3::new(f(w[0]), f(w[1]), f(w[2]));
    let guess = Point2::new(f(w[7]), f(w[8]));
    let s = c.surface;
    let at = |a: &[u64]| crate::certify::distance(k, s, p, Point2::new(f(a[0]), f(a[1])));
    let far = c.farthest;
    match (main, &c.own) {
        (Err(_), Err(_)) => "invert: both refuse",
        (Ok(m), Ok(n)) => {
            let (Some(m), Some(n)) = (at(m), at(n)) else {
                return "ARGUED invert: a distance is not enclosed";
            };
            match (far, n.0 > m.1, n.1 < m.0) {
                (false, true, _) => "FAULT invert: this branch's answer is proven farther",
                (false, _, true) => "invert: proven nearer",
                (true, _, true) => "FAULT invert (maximize): this branch's answer is proven nearer",
                (true, true, _) => "invert (maximize): proven farther",
                _ => "invert: as near to the rounding",
            }
        }
        (Err(_), Ok(n)) => {
            if far {
                return "ARGUED invert (maximize): main refuses, this branch answers";
            }
            let (Some(nd), Some(g)) = (at(n), crate::certify::distance(k, s, p, guess)) else {
                return "ARGUED invert: main refuses, a distance is not enclosed";
            };
            if nd.0 > g.1 {
                return "FAULT invert: this branch's answer is proven farther than the guess";
            }
            // (the surface's lower bound over its whole domain only: the grids of
            // `split_lower_bound` took 12 s a call on `fillet_executive_p3`'s calls of
            // this class, distances near 2e-6 that no grid closed to 1e-6 relative, and
            // a row there has 1584 distinct ones; a call not closed here stays ARGUED)
            let measured = crate::certify::measured(s);
            let lo = solidcore::query::distance_bound::surface_distance_lower_bound(
                k, measured, None, p, nd.1,
            );
            if lo.is_ok_and(|lo| nd.1 - lo <= 1e-6 * nd.1) {
                "invert: main refuses, this branch answers the closest point (1e-6)"
            } else if nd.1 < g.0 {
                "invert: main refuses, this branch answers, proven nearer than the guess"
            } else {
                "ARGUED invert: main refuses, this branch answers not farther than the guess"
            }
        }
        (Ok(m), Err(_)) => {
            if far {
                return "ARGUED invert (maximize): main answers, this branch refuses";
            }
            match (at(m), crate::certify::distance(k, s, p, guess)) {
                (Some(m), Some(g)) if m.0 > g.1 => {
                    "invert: this branch refuses main's answer, proven farther than the guess"
                }
                (Some(m), _) if nearer_boundary_witness(k, c, p, m.0) => {
                    "invert: this branch refuses main's answer, a boundary point proven nearer \
                     by more than the tolerance"
                }
                _ => {
                    "ARGUED invert: this branch refuses an answer not proven farther than the guess"
                }
            }
        }
    }
}

/// Whether a point of the call's uv domain boundary is proven nearer `p` than `main_lo`
/// (the lower end of main's answer's enclosed distance) by more than the call's
/// tolerance: main's answer, reported converged, is then not the closest point to within
/// that tolerance, so refusing it loses nothing. The witness is located by plain
/// evaluation at 1025 points along each of the domain's four edges (locating only); the
/// proof is the witness's certified distance ([`crate::certify::distance`]), whose upper
/// end plus the tolerance must lie below `main_lo`.
fn nearer_boundary_witness(k: &Kernel, c: &Call<'_>, p: Vec3, main_lo: f64) -> bool {
    const N: u32 = 1024;
    let w = c.input;
    let (u0, v0, u1, v1, tol) = (f(w[3]), f(w[4]), f(w[5]), f(w[6]), f(w[9]));
    let s = c.surface;
    let mut best: Option<(f64, Point2)> = None;
    for i in 0..=N {
        let t = f64::from(i) / f64::from(N);
        let (u, v) = (u0 + t * (u1 - u0), v0 + t * (v1 - v0));
        for uv in [
            Point2::new(u0, v),
            Point2::new(u1, v),
            Point2::new(u, v0),
            Point2::new(u, v1),
        ] {
            let Ok(q) = s.evaluate_point(k, uv) else {
                continue;
            };
            let d = (q - p).length();
            if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, uv));
            }
        }
    }
    best.and_then(|(_, uv)| crate::certify::distance(k, s, p, uv))
        // (the sum's own rounding charged by one relative epsilon)
        .is_some_and(|(_, hi)| (hi + tol) * (1.0 + f64::EPSILON) < main_lo)
}

/// The certificate classes of `blend.fillet_executive_p3`'s substituted calls (its two
/// rows left; see the module doc of the parent).
pub(super) const EXECUTIVE_P3: &[(&str, usize)] = &[
    ("degen: aligned equally to the rounding", 3),
    (
        "invert: main refuses, this branch answers, proven nearer than the guess",
        8,
    ),
    ("invert: proven nearer", 240),
];
/// Of `blend.fillet_standard_solver_p1`'s.
pub(super) const STANDARD_SOLVER_P1: &[(&str, usize)] =
    &[("degen: aligned equally to the rounding", 8)];
/// Of `heal.heal_data_p3`'s.
pub(super) const HEAL_DATA_P3: &[(&str, usize)] = &[
    ("invert: proven nearer", 21),
    (
        "invert: this branch refuses main's answer, a boundary point proven nearer by more \
         than the tolerance",
        58,
    ),
    (
        "invert: this branch refuses main's answer, proven farther than the guess",
        59,
    ),
];
/// Of `intersect.surface_drop_lib_p3`'s.
pub(super) const DROP_LIB_P3: &[(&str, usize)] =
    &[("degen: converged now, an end proven a local maximum", 3)];
/// Of `query.api_queries`'.
pub(super) const API_QUERIES: &[(&str, usize)] = &[("invert: as near to the rounding", 1)];
/// Of `topology.edgeuse_p1`'s.
pub(super) const EDGEUSE_P1: &[(&str, usize)] = &[("degen: aligned equally to the rounding", 8)];
/// Of `topology.edgeuse_p2`'s.
pub(super) const EDGEUSE_P2: &[(&str, usize)] = &[("degen: aligned equally to the rounding", 57)];
/// Of `topology.edgeuse_p3`'s.
pub(super) const EDGEUSE_P3: &[(&str, usize)] = &[("degen: aligned equally to the rounding", 110)];
/// Of `topology.vertex`'s.
pub(super) const VERTEX: &[(&str, usize)] = &[("degen: aligned equally to the rounding", 8)];
/// Of `topology.vertexuse`'s.
pub(super) const VERTEXUSE: &[(&str, usize)] = &[("degen: aligned equally to the rounding", 4)];

#[cfg(test)]
mod vertex {
    use super::super::{cases, default_dir, hash, read, substituted, unit, with_hook, Ans, DATA};
    use solidcore::curve::curve::p2::CurveKind;
    use solidcore::heal::gap::Gap;
    use solidcore::math::Point3;
    use solidcore::nurbs::math::{gamma, CPoint, NOW, NOZ};
    use solidcore::surface::surface::replay::Call;
    use solidcore::topology::vertex::{self as vx, HeldGap, VertexGap};
    use solidcore::topology::Brep;
    use solidcore::Kernel;
    use std::sync::{Arc, Mutex};

    /// The vertex row's case.
    const CASE: &str = "surf_sphere150_rebuild_v3_gaps";
    /// The call its row moves through.
    const CALL: u32 = 4;

    /// A bound on `|X_j - V|` over the Cartesian points `X_j = x_j / w_j` of `points`
    /// (homogeneous `x_j`, weight `w_j`): per coordinate the FMA `x_jc - V_c w_j` is the
    /// exact difference rounded once, so the exact one is at most `|fma| / (1 - u) <=
    /// |fma| (1 + 2u)`; their sum (the 1-norm, at least the Euclidean one) over `w_j`,
    /// its two sums, the factor and the division charged `gamma(6)`.
    ///
    /// # Panics
    ///
    /// For a weight that is not positive.
    fn hull_bound<'a>(points: impl IntoIterator<Item = &'a CPoint>, v: Point3) -> f64 {
        let u = f64::EPSILON / 2.0;
        let mut d = 0.0_f64;
        for c in points {
            let w = if c.w == NOW { 1.0 } else { c.w };
            assert!(w > 0.0, "a weight that is not positive: {c:?}");
            let z = if c.z == NOZ { 0.0 } else { c.z };
            let s = (-v.x).mul_add(w, c.x).abs()
                + (-v.y).mul_add(w, c.y).abs()
                + (-v.z).mul_add(w, z).abs();
            d = d.max(s * (1.0 + 2.0 * u) / w * (1.0 + gamma(6)));
        }
        d
    }

    /// Whether `knots` are clamped at `t` (`t` the first or last of `p + 1` equal knots).
    fn clamped_end(knots: &[f64], p: usize, t: f64) -> Option<bool> {
        let n = knots.len();
        if knots.iter().take(p + 1).all(|&x| x == t) {
            Some(true)
        } else if n > p && knots[n - p - 1..].iter().all(|&x| x == t) {
            Some(false)
        } else {
            None
        }
    }

    /// The bound on the true gap of `held` (a gap at the vertex `v`): for a face gap, the
    /// face's surface at `face_uv` lies on a clamped boundary of its net, so it is a convex
    /// combination of that boundary's Cartesian control points (positive weights) and its
    /// distance from `v` is at most their [`hull_bound`]; for an edge gap, the edge's curve
    /// at `edge_t` is its clamped end, the end control point exactly.
    ///
    /// # Panics
    ///
    /// Where the gap is not at such a boundary or end.
    fn true_gap_bound(k: &Kernel, b: &Brep, v: Point3, held: &HeldGap) -> f64 {
        match held.gap {
            VertexGap::Face(g) => {
                // (the cached copy's use: the holder where the copy has none)
                let g = solidcore::heal::gap::VertexFaceGap {
                    vertexuse: g.vertexuse.or(Some(held.holder)),
                    ..g
                };
                let face = Gap::from(g).face(b).expect("the face").expect("a face");
                let s = solidcore::topology::face::p2::surface(b, face)
                    .expect("the surface")
                    .expect("a surface");
                let form = solidcore::blend::fillet_section::station::as_b_spline(k, s)
                    .expect("the NURBS form")
                    .expect("a NURBS form");
                let nurb = form.gw_nurb().expect("a NURB");
                let net = nurb.cpts();
                let uv = g.face_uv;
                let (uk, vk) = (nurb.u_knots(), nurb.v_knots());
                let mut best = f64::INFINITY;
                if let Some(start) = clamped_end(vk, nurb.q(), uv.y) {
                    let j = if start { 0 } else { net[0].len() - 1 };
                    best = best.min(hull_bound(net.iter().map(|row| &row[j]), v));
                }
                if let Some(start) = clamped_end(uk, nurb.p(), uv.x) {
                    let i = if start { 0 } else { net.len() - 1 };
                    best = best.min(hull_bound(&net[i], v));
                }
                assert!(
                    best.is_finite(),
                    "face gap {held:?}: face_uv {uv:?} is on no clamped boundary"
                );
                best
            }
            VertexGap::Edge(g) => {
                let g = solidcore::heal::gap::VertexEdgeGap {
                    vertexuse: g.vertexuse.or(Some(held.holder)),
                    ..g
                };
                let edge = Gap::from(g).edge(b, 0).expect("the edge").expect("an edge");
                let curve = solidcore::topology::edge::curve(b, edge)
                    .expect("the curve")
                    .expect("a curve");
                let CurveKind::BSpline(bc) = curve.kind() else {
                    panic!("edge gap {held:?}: not a B-spline curve: {curve:?}");
                };
                let nurb = bc.gw_nurb().expect("a NURB");
                let start = clamped_end(nurb.knots(), nurb.degree(), g.edge_t)
                    .unwrap_or_else(|| panic!("edge gap {held:?}: edge_t not a clamped end"));
                let cp = nurb.cpts();
                hull_bound([if start { &cp[0] } else { &cp[cp.len() - 1] }], v)
            }
        }
    }

    /// This branch's own answer at call [`CALL`] of the case.
    fn own_at_call(u: &dyn crate::UnitDiff, case: &crate::Case) -> Vec<u64> {
        let got = Arc::new(Mutex::new(None));
        let g = Arc::clone(&got);
        let mut n = 0u32;
        with_hook(
            move |c: Call<'_>| {
                if n == CALL {
                    *g.lock().unwrap() = Some(c.own.clone().expect("a value"));
                }
                n += 1;
                c.own
            },
            || u.run_rust(case),
        )
        .expect("the run");
        let got = got.lock().unwrap().take();
        got.expect("the call")
    }

    /// `topology.vertex` `surf_sphere150_rebuild_v3_gaps` (its weighted sum 243 -> 242):
    /// the row moves through call [`CALL`] alone, a `find_degen_param_for_direction` at the
    /// sphere's pole whose `u` this branch answers 3 ulps below main's (15.346539759579205
    /// against 15.34653975957921, the end knot). With main's answer at that call only, the
    /// row is main's; with main's answer at every other call, it is this branch's line.
    ///
    /// Both rows report the true largest gap to the rounding, PROVEN by the convex hull
    /// of the net. The pole is not one point to the bit: rows 1 and 3 of the pole column
    /// are `-5 + 2.8e-16` and `-5 - 1.4e-16` in `x`. Every face gap at the vertex has its
    /// `face_uv` on a clamped boundary of the net (`v = 0`), where the surface is a convex
    /// combination of the boundary's Cartesian control points (positive weights), so its
    /// true distance from the vertex is at most their bound `delta` ([`hull_bound`], the
    /// rounding of computing it charged); every edge gap is at its curve's clamped end, its
    /// end control point. On both runs `delta <= 4e-16`, and every listed gap and the
    /// largest lie in `[0, delta]`. (The true face gap is 0 at main's `u`, the end knot,
    /// and 1.54e-31 at this branch's, by exact rational evaluation; which use holds the
    /// largest is a tie within `delta`.)
    #[test]
    fn the_vertex_row_moves_through_its_pole_call_alone() {
        let row = DATA.iter().find(|r| r.1 == CASE).unwrap();
        let u = unit(row.0);
        let all = cases(u.as_ref());
        let case = &all[row.1];
        let (only, others): (Vec<_>, Vec<_>) = row.6.iter().partition(|e| e.0 == CALL);
        assert_eq!(only.len(), 1);
        let (line, _) = substituted(u.as_ref(), case, &only, false);
        assert_eq!(hash(&line), row.2, "call {CALL} alone gives main's row");
        let (line, _) = substituted(u.as_ref(), case, &others, false);
        let golden = read(&default_dir(), row.0).unwrap();
        assert_eq!(
            golden.get(row.1),
            Some(&line),
            "the others leave this branch's row"
        );
        let now = own_at_call(u.as_ref(), case);
        let Ans::Ok(main_u, ..) = only[0].1 else {
            panic!("main's call {CALL} is a value");
        };
        assert_eq!(f64::from_bits(now[0]), 15.346539759579205);
        assert_eq!(f64::from_bits(main_u), 15.34653975957921);
        assert_eq!(main_u - now[0], 3);

        // the convex hull certificate, on this branch's run and with main's answer
        for (run, tape) in [("this branch", &[][..]), ("main's call", &only[..])] {
            let tape: Vec<(u32, Ans)> = tape.to_vec();
            let (b, v, max, list) = with_hook(
                {
                    let mut n = 0u32;
                    move |c: Call<'_>| {
                        let i = n;
                        n += 1;
                        match tape.iter().find(|e| e.0 == i) {
                            Some(&(_, Ans::Ok(x, y, z))) => Ok(vec![x, y, z]),
                            Some(&(_, Ans::Err(e))) => Err(e()),
                            None => c.own,
                        }
                    }
                },
                || crate::units::topology_vertex::wr57_gaps(case),
            );
            let k = Kernel::default();
            let at = vx::point(&b, v).expect("the vertex");
            let max = max.expect("a largest gap");
            assert!(!list.is_empty(), "{run}: no gaps listed");
            // (a gap the cache holds unmeasured, `-SM_UNDEF_DOUBLE`, has no point to bound)
            let unmeasured = |g: &HeldGap| g.gap.length() == -solidcore::math::UNDEF_DOUBLE;
            for g in list.iter().filter(|g| unmeasured(g)) {
                eprintln!("{run}: {g:?}: not measured");
            }
            let list: Vec<HeldGap> = list.into_iter().filter(|g| !unmeasured(g)).collect();
            assert!(!list.is_empty(), "{run}: no measured gap");
            assert!(
                !unmeasured(&max),
                "{run}: the largest gap {max:?} is not measured"
            );
            let mut delta = 0.0_f64;
            for g in &list {
                let bound = true_gap_bound(&k, &b, at, g);
                eprintln!(
                    "{run}: {g:?}: reported {:e}, true gap in [0, {bound:e}]",
                    g.gap.length()
                );
                delta = delta.max(bound);
            }
            eprintln!("{run}: vertex {at:?}, largest {max:?}, delta {delta:e}");
            assert!(delta <= 4e-16, "{run}: delta {delta:e}");
            for g in list.iter().chain([&max]) {
                let r = g.gap.length();
                assert!(
                    (0.0..=delta).contains(&r),
                    "{run}: the reported gap {r:e} of {g:?} is not in [0, {delta:e}]"
                );
            }
        }
    }
}
