//! The certificates of the replay's substituted calls ([`classify`]) and their counts per
//! unit.
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
        // (the WR-450 seams' calls never reach the WR-57 hooks)
        Seam::AnalyticCopy | Seam::SplitTrims => "FAULT: not a WR-57 call",
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

/// The certificate classes of `heal.heal_data_p3`'s substituted calls.
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
