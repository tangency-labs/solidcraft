//! Curves fitted to points: through points (edit points), closed, and rebuilt to a degree and span count.
//! The curve is the record's ([`Curve`]); the knots, the fits and the bound are the kernel's
//! ([`interp`]: global interpolation, The NURBS Book's §9.2.1; [`periodic_uniform`];
//! [`approx_knots`]: least squares at given parameters, §9.4.1; [`certified_distance`] and
//! [`interpolation_bound`]). Ops checks the inputs and converts; every fit comes with the
//! kernel's certified bound of how far it is from what it fits.

use solidcore::nurbs::crv_deviation::{certified_distance, interpolation_bound};
use solidcore::nurbs::crv_fit::p1::{approx_knots, param_values, ParamMethod};
use solidcore::nurbs::crv_fit::p2::interp;
use solidcore::nurbs::geom::Interval;
use solidcore::nurbs::knots::{clamped_uniform, periodic_uniform, KnotVector, Side, MAX_DEGREE};
use solidcore::Vec3;
use solidcore_record::brep::{check_curve, Budget, Curve, MAX_SPLINE_DEGREE};

use crate::curve_knots::{nurb_of, record_of};
use crate::session::run;
use crate::sweep::{coord_ok, CURVE_MAX_POINTS};
use crate::{Error, Result};

/// The clamped B-spline of `degree` through `points` in order, on `[0, 1]`, each point at its
/// share of the polygon's length (chord-length parameters) and the knots their averages (The
/// NURBS Book, eq. 9.8), and the kernel's certified bound of how far the curve misses a point
/// at its parameter ([`interpolation_bound`]): interpolation in floating point is not exact.
///
/// # Errors
/// [`Error::Argument`] for fewer than two points, more than [`CURVE_MAX_POINTS`] (or than a
/// record holds), a coordinate that is not finite or past the coordinate bound, a point
/// repeating the one before it (a chord of no length), or a degree of 0, not below the number
/// of points, or past the kernel's most; the kernel's status otherwise (points so close the fit
/// is singular).
pub fn ep_curve(points: &[[f64; 3]], degree: usize) -> Result<(Curve, f64)> {
    if points.len() < 2 || points.len() > CURVE_MAX_POINTS || !points.iter().all(|p| coord_ok(p)) {
        return Err(Error::Argument);
    }
    if points.windows(2).any(|w| w.first() == w.get(1)) {
        return Err(Error::Argument);
    }
    if degree == 0 || degree >= points.len() || degree > MAX_DEGREE {
        return Err(Error::Argument);
    }
    let data: Vec<Vec3> = points.iter().map(|&[x, y, z]| Vec3::new(x, y, z)).collect();
    run("curve_fit::ep_curve", |_| {
        let params = param_values(&data, ParamMethod::ChordLength)?;
        let fit = interp(&data, degree, ParamMethod::ChordLength)?;
        let bound = interpolation_bound(&fit, &params, &data)?;
        Ok(record_of(&fit).map(|c| (c, bound)))
    })?
}

/// The closed B-spline of `degree` (below the number of points) with `points`
/// as its control polygon, closing from the last back to the first, on `[0, 1]`: periodic,
/// so it joins itself smoothly (continuity `degree - 1`) and passes through none of its
/// points. The record holds it as a spline whose polygon repeats its first `degree` points
/// after its last, on the kernel's unclamped uniform knots ([`periodic_uniform`]);
/// [`crate::curve::move_cv`] moves a repeated point with its copy.
///
/// # Errors
/// [`Error::Argument`] for fewer than three points, more than [`CURVE_MAX_POINTS`] (or than a
/// record holds), a coordinate that is not finite or past the coordinate bound, or a degree
/// of 0, not below the number of points, or past the kernel's most.
pub fn closed_cv_curve(points: &[[f64; 3]], degree: usize) -> Result<Curve> {
    if points.len() < 3 || points.len() > CURVE_MAX_POINTS || !points.iter().all(|p| coord_ok(p)) {
        return Err(Error::Argument);
    }
    if degree == 0 || degree >= points.len() || degree > MAX_DEGREE {
        return Err(Error::Argument);
    }
    let knots = periodic_uniform(points.len(), degree).map_err(|_| Error::Argument)?;
    let mut wrapped = points.to_vec();
    wrapped.extend_from_slice(points.get(..degree).ok_or(Error::Argument)?);
    let curve = Curve::Spline {
        knots,
        points: wrapped,
    };
    check_curve(&curve, &mut Budget::input()).map_err(|_| Error::Argument)?;
    Ok(curve)
}

/// `curve` refitted as a clamped spline of `degree` with `spans` uniform spans on `[0, 1]`,
/// and the kernel's certified bound of how far it is from `curve` at every parameter
/// ([`certified_distance`]): `curve` is sampled at even steps of its parameter (mapped onto
/// `[0, 1]`), four per control point, and the samples fitted by least squares at those
/// parameters through its two ends ([`approx_knots`]).
///
/// # Errors
/// [`Error::Argument`] for a curve that is not a valid clamped spline (a rational spline,
/// whose bound the kernel does not certify yet, and a closed periodic one are refused), a
/// degree of 0 or past the record's most ([`MAX_SPLINE_DEGREE`]), no spans, fewer than three
/// control points (`spans + degree`) or more than a record holds; the kernel's status
/// otherwise.
pub fn rebuild(curve: &Curve, degree: usize, spans: usize) -> Result<(Curve, f64)> {
    let nurb = nurb_of(curve)?;
    if !matches!(curve, Curve::Spline { .. })
        || degree == 0
        || degree > MAX_SPLINE_DEGREE
        || spans == 0
    {
        return Err(Error::Argument);
    }
    let count = spans.checked_add(degree).ok_or(Error::Argument)?;
    if !(3..=4096).contains(&count) {
        return Err(Error::Argument);
    }
    let samples = count
        .checked_mul(4)
        .and_then(|s| s.checked_add(1))
        .ok_or(Error::Argument)?;
    // even steps on [0, 1]: the distinct knots of a polyline of that many points
    let steps = clamped_uniform(samples, 1).map_err(|_| Error::Argument)?;
    let params = steps.get(1..=samples).ok_or(Error::Argument)?.to_vec();
    let knots = clamped_uniform(count, degree).map_err(|_| Error::Argument)?;
    run("curve_fit::rebuild", |_| {
        let mut unit = nurb.clone();
        unit.reparam_to_interval(Interval { ul: 0.0, ur: 1.0 })?;
        let data = params
            .iter()
            .map(|&t| unit.eval(t, Side::Left))
            .collect::<solidcore::Result<Vec<Vec3>>>()?;
        let fit = approx_knots(
            &data,
            &params,
            &KnotVector::from_slice(&knots)?,
            count - 1,
            degree,
        )?;
        let bound = certified_distance(&unit, &fit)?;
        Ok(record_of(&fit).map(|c| (c, bound)))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::{curve_comb, cv_curve};
    use solidcore::nurbs::crv_basic::Curve as NurbCurve;
    use solidcore::nurbs::math::{CPoint, NOW};

    /// The point of `c` at `t`, by the kernel's evaluator.
    fn at(c: &Curve, t: f64) -> [f64; 3] {
        let p = nurb_of(c).unwrap().eval(t, Side::Left).unwrap();
        [p.x, p.y, p.z]
    }

    fn passes_through(c: &Curve, points: &[[f64; 3]], params: &[f64]) {
        for (p, t) in points.iter().zip(params) {
            let q = at(c, *t);
            assert!(
                p.iter().zip(q).all(|(a, b)| (a - b).abs() < 1e-12),
                "at {t}: {q:?}, not {p:?}"
            );
        }
    }

    #[test]
    fn a_cubic_passes_through_its_points_at_their_share_of_the_length() {
        // Chords of 5, 12 and 5: the points at 0, 5/22, 17/22 and 1 (uniform parameters would
        // put the second at 1/3).
        let points = [
            [0.0, 0.0, 0.0],
            [3.0, 4.0, 0.0],
            [3.0, 4.0, 12.0],
            [6.0, 8.0, 12.0],
        ];
        let (c, bound) = ep_curve(&points, 3).unwrap();
        assert!(bound < 1e-12, "{bound}");
        let Curve::Spline { knots, points: cvs } = &c else {
            panic!("a spline: {c:?}")
        };
        assert_eq!(knots, &vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(cvs.len(), 4);
        passes_through(&c, &points, &[0.0, 5.0 / 22.0, 17.0 / 22.0, 1.0]);
    }

    #[test]
    fn more_points_than_the_degree_make_interior_knots() {
        // Six points with chords of 5: parameters i / 5, interior knots the averages of three.
        let points = [
            [0.0, 0.0, 0.0],
            [3.0, 4.0, 0.0],
            [6.0, 0.0, 0.0],
            [9.0, 4.0, 0.0],
            [12.0, 0.0, 0.0],
            [15.0, 4.0, 0.0],
        ];
        let params: Vec<f64> = (0..6).map(|i| f64::from(i) / 5.0).collect();
        let (c, bound) = ep_curve(&points, 3).unwrap();
        assert!(bound < 1e-12, "{bound}");
        let Curve::Spline { knots, points: cvs } = &c else {
            panic!("a spline: {c:?}")
        };
        assert_eq!(cvs.len(), 6);
        assert_eq!(knots.len(), 10);
        for (k, want) in knots[4..6].iter().zip([0.4, 0.6]) {
            assert!((k - want).abs() < 1e-12, "{knots:?}");
        }
        passes_through(&c, &points, &params);

        let (quadratic, _) = ep_curve(&points, 2).unwrap();
        passes_through(&quadratic, &points, &params);
    }

    #[test]
    fn two_points_make_a_line() {
        let ends = [[1.0, 2.0, 3.0], [4.0, 6.0, 3.0]];
        let (line, bound) = ep_curve(&ends, 1).unwrap();
        assert_eq!(
            line,
            Curve::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: ends.to_vec(),
            }
        );
        assert!(bound < 1e-12, "{bound}");
    }

    #[test]
    fn bad_points_are_refused() {
        let ends = [[0.0; 3], [1.0, 0.0, 0.0]];
        assert_eq!(ep_curve(&ends[..1], 3), Err(Error::Argument));
        assert_eq!(
            ep_curve(&[[0.0; 3], [f64::NAN, 0.0, 0.0]], 1),
            Err(Error::Argument)
        );
        assert_eq!(
            ep_curve(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]], 2),
            Err(Error::Argument)
        );
        assert_eq!(ep_curve(&ends, 0), Err(Error::Argument));
        // a degree not below the number of points is refused, not lowered
        assert_eq!(ep_curve(&ends, 2), Err(Error::Argument));
        assert_eq!(ep_curve(&ends[..], 3), Err(Error::Argument));
        assert_eq!(ep_curve(&ends, MAX_DEGREE + 1), Err(Error::Argument));
    }

    const SQUARE: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];

    /// The kernel's curve of a spline record as it is, clamped or not.
    fn unclamped(c: &Curve) -> NurbCurve {
        let Curve::Spline { knots, points } = c else {
            panic!("a spline: {c:?}")
        };
        let pw = points
            .iter()
            .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
            .collect();
        NurbCurve::new(pw, knots.len() - points.len() - 1, knots.clone()).unwrap()
    }

    #[test]
    fn a_closed_curve_joins_itself_smoothly() {
        let c = closed_cv_curve(&SQUARE, 3).unwrap();
        let Curve::Spline { knots, points } = &c else {
            panic!("a spline: {c:?}")
        };
        assert_eq!(points.len(), 7);
        assert_eq!(&points[4..], &SQUARE[..3]);
        assert_eq!(knots.len(), 11);
        assert_eq!((knots[3], knots[7]), (0.0, 1.0));
        let k = unclamped(&c);
        // the uniform cubic starts at (P0 + 4 P1 + P2) / 6 (the basis at a knot of a uniform
        // cubic is 1/6, 4/6, 1/6), and ends there: read through `derivs`, the general basis
        // (the NLib `eval` keeps upstream's clamped reading of the domain's ends)
        let start = k.derivs(0.0, Side::Right, 0).unwrap()[0];
        let end = k.derivs(1.0, Side::Left, 0).unwrap()[0];
        assert!((start.x - 50.0 / 6.0).abs() < 1e-12 && (start.y - 10.0 / 6.0).abs() < 1e-12);
        assert!((end.x - 50.0 / 6.0).abs() < 1e-12 && (end.y - 10.0 / 6.0).abs() < 1e-12);
        // position, first and second derivatives meet across the seam
        let (a, b) = (
            k.derivs(0.0, Side::Right, 2).unwrap(),
            k.derivs(1.0, Side::Left, 2).unwrap(),
        );
        for (x, y) in a.iter().zip(&b) {
            assert!(
                (x.x - y.x).abs() < 1e-9 && (x.y - y.y).abs() < 1e-9 && (x.z - y.z).abs() < 1e-9,
                "{a:?} and {b:?}"
            );
        }
        // the kernel takes it as an edge's curve: its comb's end teeth stand on one point
        let comb = curve_comb(&c, [0.0, 1.0], 9).unwrap();
        let (first, last) = (comb.first().unwrap(), comb.last().unwrap());
        assert!(
            first
                .at
                .iter()
                .zip(last.at)
                .all(|(x, y)| (x - y).abs() < 1e-9),
            "{:?} and {:?}",
            first.at,
            last.at
        );
        // a degree not below the number of points is refused, not lowered; two points make no
        // closed curve
        let Curve::Spline { knots, points } = closed_cv_curve(&SQUARE[..3], 2).unwrap() else {
            panic!()
        };
        assert_eq!(knots.len() - points.len() - 1, 2);
        assert_eq!(closed_cv_curve(&SQUARE[..3], 3), Err(Error::Argument));
        assert_eq!(closed_cv_curve(&SQUARE[..2], 1), Err(Error::Argument));
        assert_eq!(closed_cv_curve(&SQUARE, 0), Err(Error::Argument));
    }

    #[test]
    fn a_closed_curve_of_degree_p_is_c_p_minus_1_across_its_seam() {
        let hexagon = [
            [10.0, 0.0, 0.0],
            [5.0, 8.0, 1.0],
            [-5.0, 8.0, 0.0],
            [-10.0, 0.0, -1.0],
            [-5.0, -8.0, 0.0],
            [5.0, -8.0, 2.0],
        ];
        for p in 1..=5 {
            let k = unclamped(&closed_cv_curve(&hexagon, p).unwrap());
            // every derivative of order below p meets across the seam
            let (a, b) = (
                k.derivs(0.0, Side::Right, p - 1).unwrap(),
                k.derivs(1.0, Side::Left, p - 1).unwrap(),
            );
            assert_eq!(a.len(), p);
            for (order, (x, y)) in a.iter().zip(&b).enumerate() {
                for (u, v) in [(x.x, y.x), (x.y, y.y), (x.z, y.z)] {
                    assert!(
                        (u - v).abs() <= 1e-9 * (1.0 + u.abs()),
                        "degree {p}, derivative {order}: {x:?} and {y:?}"
                    );
                }
            }
        }
    }

    const WAVE: [[f64; 3]; 6] = [
        [0.0, 0.0, 0.0],
        [1.0, 2.0, 0.0],
        [2.0, -1.0, 0.0],
        [3.0, 3.0, 0.0],
        [4.0, 0.0, 1.0],
        [5.0, 1.0, 0.0],
    ];

    #[test]
    fn a_curve_rebuilt_in_its_own_space_is_itself() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let (r, bound) = rebuild(&c, 3, 3).unwrap();
        assert!(bound < 1e-9, "{bound}");
        let (Curve::Spline { knots: k0, .. }, Curve::Spline { knots: k1, .. }) = (&c, &r) else {
            panic!("splines")
        };
        assert_eq!(k0, k1);
    }

    #[test]
    fn a_rebuilt_curves_bound_holds_its_gap_everywhere() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let (r, bound) = rebuild(&c, 3, 1).unwrap();
        let Curve::Spline { knots, points } = &r else {
            panic!("a spline: {r:?}")
        };
        assert_eq!(points.len(), 4);
        assert_eq!(knots, &vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert!(bound > 0.0);
        let mut largest = 0.0_f64;
        for i in 0..=400 {
            let t = f64::from(i) / 400.0;
            let (p, q) = (at(&c, t), at(&r, t));
            let gap2: f64 = p.iter().zip(q).map(|(a, b)| (a - b) * (a - b)).sum();
            largest = largest.max(gap2);
        }
        assert!(largest <= bound * bound, "{largest} past {bound}²");
        // the ends stay put
        for (t, end) in [(0.0, WAVE[0]), (1.0, WAVE[5])] {
            let p = at(&r, t);
            assert!(
                p.iter().zip(end).all(|(a, b)| (a - b).abs() < 1e-12),
                "{p:?}"
            );
        }
    }

    #[test]
    fn a_curve_on_another_domain_is_rebuilt_on_zero_to_one() {
        let c = Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0],
            points: WAVE[..4].to_vec(),
        };
        let (r, bound) = rebuild(&c, 3, 1).unwrap();
        assert!(bound < 1e-9, "{bound}");
        assert_eq!(
            r.natural(),
            solidcore_record::brep::Natural::Range(0.0, 1.0)
        );
    }

    #[test]
    fn a_rebuild_with_bad_arguments_is_refused() {
        let c = cv_curve(&WAVE, 3).unwrap();
        assert_eq!(rebuild(&c, 0, 3), Err(Error::Argument));
        assert_eq!(rebuild(&c, 3, 0), Err(Error::Argument));
        assert_eq!(rebuild(&c, 1, 1), Err(Error::Argument));
        assert_eq!(rebuild(&c, MAX_SPLINE_DEGREE + 1, 1), Err(Error::Argument));
        let closed = closed_cv_curve(&SQUARE, 3).unwrap();
        assert_eq!(rebuild(&closed, 3, 3), Err(Error::Argument));
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        assert_eq!(rebuild(&arc, 3, 2), Err(Error::Argument));
    }
}
