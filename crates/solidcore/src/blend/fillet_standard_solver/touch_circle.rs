//! A variable round as the union of its balls (opt-in, not upstream): the knots where a
//! round's curves turn, and whether its balls fold over one another.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmFilletSection.cpp`
//! (`sm_FilletCurveCorners`, from tangency-labs/smlib@6eecf04; `sm_BallsFold`, from
//! tangency-labs/smlib@d622107 = d01d7b0's 2714d37), called by `SmGnSectionFSG::CircleSurface`,
//! the fork's circular section, under `fixes.fillet_gn_section_generator` (WR-685, #2250).
//!
//! | Rust                      | Fork (`SmFilletSection.cpp`) |
//! |---------------------------|------------------------------|
//! | [`fillet_curve_corners`]  | `sm_FilletCurveCorners`      |
//! | [`balls_fold`]            | `sm_BallsFold`               |
//!
//! Deviations: out-params (`rCorners`, `rbFolds`, `rdRadius`) are return values; the
//! fold's radius is `None` where the balls do not fold. Every loop is bounded by the rails'
//! knots (three stations a span, 17 points an arc).

use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;

const OP_CORNERS: &str = "blend::fillet_standard_solver::touch_circle::fillet_curve_corners";
const OP_FOLD: &str = "blend::fillet_standard_solver::touch_circle::balls_fold";

/// Points along each section's arc in [`balls_fold`] (`lArc`).
const ARC_POINTS: usize = 17;

/// Position and first derivative of `c` at `t`.
fn pv(c: &BSplineCurve, t: f64, from_left: bool, op: &'static str) -> Result<(Vec3, Vec3)> {
    let v = c.evaluate(t, 1, from_left, false)?;
    match *v.as_slice() {
        [p, d, ..] => Ok((p, d)),
        _ => Err(SolidError::sm_err(op)),
    }
}

/// `sm_FilletCurveCorners`: the interior knots of the first rail, ascending, where the
/// centre line or a rail turns, its tangents before and after the knot differing by more
/// than `ang_tol_rad`. The three curves share their knots; a turn comes from a corner in
/// the radius.
///
/// # Errors
///
/// Those of `GetKnots`, `Evaluate` and `AngleBetween`.
pub fn fillet_curve_corners(
    center_line: &BSplineCurve,
    rail1: &BSplineCurve,
    rail2: &BSplineCurve,
    ang_tol_rad: f64,
) -> Result<Vec<f64>> {
    let mut corners = Vec::new();
    let (knots, _) = rail1.knots(None)?;
    let ivl = rail1.natural_interval();
    let tol = EFF_ZERO_SQRT * (1.0 + ivl.min().abs() + ivl.max().abs());
    let curves = [center_line, rail1, rail2];
    let mut previous: Option<f64> = None;
    for &knot in &knots {
        let repeated = previous == Some(knot);
        previous = Some(knot);
        if repeated || knot < ivl.min() + tol || knot > ivl.max() - tol {
            continue;
        }
        let mut corner = false;
        for c in curves {
            if corner {
                break;
            }
            if !c.natural_interval().contains_value(knot, 0.0) {
                continue;
            }
            let (_, before) = pv(c, knot, false, OP_CORNERS)?;
            let (_, after) = pv(c, knot, true, OP_CORNERS)?;
            if before.length_squared() < EFF_ZERO_SQ || after.length_squared() < EFF_ZERO_SQ {
                continue;
            }
            if before.angle_between(after)? > ang_tol_rad {
                corner = true;
            }
        }
        if corner {
            corners.push(knot);
        }
    }
    Ok(corners)
}

/// `sm_BallsFold`: whether the balls of a round whose radius varies fold over one another
/// between `a` and `b` (no corner between), and the ball's radius where they first do.
///
/// The round is the envelope of its balls; each touches it along the circle square to the
/// centre line through its two contacts (the rails), the arc between them the round's
/// section. Where the balls' touch circles move back along the centre line the envelope
/// folds over itself. Between consecutive stations (the rails' knots and two points in
/// each span) each section's arc must lie ahead of the one before's plane and behind the
/// next one's, within `tol`. A degenerate centre line is no case for this check.
///
/// # Errors
///
/// Those of `GetKnots`, `Evaluate`, `EvaluatePoint` and `Unitize`.
pub fn balls_fold(
    center_line: &BSplineCurve,
    rail1: &BSplineCurve,
    rail2: &BSplineCurve,
    a: f64,
    b: f64,
    tol: f64,
) -> Result<Option<f64>> {
    let (knots, _) = rail1.knots(None)?;
    let mut at = vec![a];
    at.extend(knots.iter().copied().filter(|&kn| kn > a && kn < b));
    at.push(b);
    // each span's thirds too
    let mut stations = Vec::new();
    for w in at.windows(2) {
        if let [lo, hi] = *w {
            for k in 0..3 {
                stations.push(lo + (hi - lo) * f64::from(k) / 3.0);
            }
        }
    }
    stations.push(b);

    // each station's section: the plane through its touch circle (point, unit normal along
    // the centre line) and points along the arc between the rails
    let mut origins: Vec<Point3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut arcs: Vec<Point3> = Vec::new();
    let mut radii: Vec<f64> = Vec::new();
    for (i, &t) in stations.iter().enumerate() {
        let (c, along) = pv(center_line, t, i == 0, OP_FOLD)?;
        let p1 = rail1.evaluate_point(t)?;
        let p2 = rail2.evaluate_point(t)?;
        if along.length() <= EFF_ZERO {
            return Ok(None); // a degenerate centre line is no case for this check
        }
        let along = along.unitize()?;
        let m = c + along * (0.5 * ((p1 - c).dot(along) + (p2 - c).dot(along)));
        let u = p1 - m;
        let v = p2 - m;
        let rho = u.length();
        radii.push(c.distance(p1));
        origins.push(m);
        normals.push(along);
        if rho <= EFF_ZERO || v.length() <= EFF_ZERO {
            arcs.extend(core::iter::repeat_n(p1, ARC_POINTS));
            continue;
        }
        let u = u.unitize()?;
        // the arc from the first rail to the second, the short way (the section's own)
        let mut w = v - u * v.dot(u);
        let wl = w.length();
        let mut phi = 0.0;
        if wl > EFF_ZERO {
            w /= wl;
            phi = v.dot(w).atan2(v.dot(u));
        }
        for k in 0..ARC_POINTS {
            let ang = phi * k as f64 / (ARC_POINTS as f64 - 1.0);
            arcs.push(m + u * (rho * ang.cos()) + w * (rho * ang.sin()));
        }
    }
    let arc_at = |i: usize, k: usize| -> Result<Point3> {
        arcs.get(i * ARC_POINTS + k)
            .copied()
            .ok_or_else(|| SolidError::sm_err(OP_FOLD))
    };
    for i in 0..origins.len().saturating_sub(1) {
        let (Some(&o0), Some(&n0), Some(&o1), Some(&n1)) = (
            origins.get(i),
            normals.get(i),
            origins.get(i + 1),
            normals.get(i + 1),
        ) else {
            return Err(SolidError::sm_err(OP_FOLD));
        };
        let mut folds = false;
        let mut k = 0;
        while k < ARC_POINTS && !folds {
            let next = arc_at(i + 1, k)?;
            let this = arc_at(i, k)?;
            if (next - o0).dot(n0) < -tol || (this - o1).dot(n1) > tol {
                folds = true;
            }
            k += 1;
        }
        if folds {
            return Ok(Some(radii.get(i).copied().unwrap_or(0.0)));
        }
    }
    Ok(None)
}

/// The cuts `sm_BallsFold` is run between in `SmGnSectionFSG::CircleSurface`: the first
/// rail's natural interval split at `corners`.
#[must_use]
pub fn fold_cuts(rail1: &BSplineCurve, corners: &[f64]) -> Vec<f64> {
    let whole: Extent1d = rail1.natural_interval();
    let mut cuts = vec![whole.min()];
    cuts.extend_from_slice(corners);
    cuts.push(whole.max());
    cuts
}

/// The angle above which a knot is a corner in `SmGnSectionFSG::CircleSurface`:
/// `smos_Min(GetThisAngTolRad(), SM_DEG2RAD(SM_CONTINUITY_ANGLE))`.
#[must_use]
pub fn corner_angle(ang_tol_rad: f64) -> f64 {
    let continuity = crate::geom::tol::ANG_TOL_DEG * (crate::math::PI / 180.0);
    if continuity < ang_tol_rad {
        continuity
    } else {
        ang_tol_rad
    }
}

/// The comment `SmGnSectionFSG::CircleSurface` notes with a fold, the radius as C's `%g`
/// (six significant digits, trailing zeros dropped).
#[must_use]
pub fn fold_comment(radius: f64) -> String {
    format!(
        "Fillet Error: the round's balls fold over one another where its radius is {}: the radius changes faster there than a rolling ball can follow",
        format_g(radius)
    )
}

/// C's `%g`: six significant digits, fixed for exponents -4 to 5, else `d.ddddde+XX`,
/// trailing zeros (and a trailing point) dropped.
fn format_g(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".into()
        } else if v > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    if v == 0.0 {
        return "0".into();
    }
    // the exponent after rounding to six significant digits
    let e = format!("{v:.5e}");
    let exp: i32 = e
        .rsplit_once('e')
        .and_then(|(_, x)| x.parse().ok())
        .unwrap_or(0);
    let trim = |s: String| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    if (-4..6).contains(&exp) {
        let decimals = (5 - exp).max(0) as usize;
        trim(format!("{v:.decimals$}"))
    } else {
        let (mantissa, _) = e.rsplit_once('e').unwrap_or((e.as_str(), "0"));
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mantissa.to_string()), exp.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_g_is_cs() {
        assert_eq!(format_g(19.628_412), "19.6284");
        assert_eq!(format_g(2.0), "2");
        assert_eq!(format_g(0.000_123_456_7), "0.000123457");
        assert_eq!(format_g(1_234_567.0), "1.23457e+06");
        assert_eq!(format_g(0.000_012_5), "1.25e-05");
        assert_eq!(format_g(-3.5), "-3.5");
    }

    #[test]
    fn corner_angle_is_the_smaller() {
        assert_eq!(corner_angle(1.0), 0.5 * (crate::math::PI / 180.0));
        assert_eq!(corner_angle(1e-3), 1e-3);
    }
}
