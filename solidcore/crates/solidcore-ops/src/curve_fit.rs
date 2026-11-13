//! Curves fitted to points: through points (edit points), closed, rebuilt to a degree and span
//! count, and a body's edge as a free curve. The curve is the record's ([`Curve`]); the knots,
//! the fits, the edge's trimmed curve and the bound are the kernel's ([`interp`]: global
//! interpolation, The NURBS Book's §9.2.1; [`periodic_uniform`]; [`approx_knots`]: least
//! squares at given parameters, §9.4.1; [`create_trimmed_nurbs_curve`]; [`certified_distance`]
//! and [`interpolation_bound`]). Ops checks the inputs and converts; every fit comes with the
//! kernel's certified bound of how far it is from what it fits.

use solidcore::curve::from_parameters::{from_parameters, record_of as kernel_record, EdgeCurve};
use solidcore::kernel::Kernel;
use solidcore::nurbs::crv_deviation::{certified_distance, interpolation_bound};
use solidcore::nurbs::crv_fit::end_directions::interp_end_directions;
use solidcore::nurbs::crv_fit::p1::{approx_knots, param_values, ParamMethod};
use solidcore::nurbs::crv_fit::p2::interp;
use solidcore::nurbs::geom::Interval;
use solidcore::nurbs::knots::{clamped_uniform, periodic_uniform, KnotVector, Side, MAX_DEGREE};
use solidcore::topology::edge::create_trimmed_nurbs_curve;
use solidcore::{SolidError, Vec3};
use solidcore_record::brep::{check_curve, Budget, Curve, Invalid, Natural, MAX_SPLINE_DEGREE};

use crate::body::Body;
use crate::curve_knots::{nurb_of, record_of};
use crate::order::{self, Order};
use crate::session::{run, with_body};
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

/// As [`ep_curve`], with the tangent at the start along `start` and at the end along `end`
/// where they are given (the kernel's [`interp_end_directions`]: each direction taken as the end
/// derivative, scaled to the points' polygon length): a half section whose end at a mirror
/// plane runs square to it joins its mirror smoothly. With neither given it is [`ep_curve`].
///
/// # Errors
/// As [`ep_curve`]; [`Error::Argument`] also for a direction with a coordinate that is not
/// finite or of no length, or one given on a curve of degree below 2.
pub fn ep_curve_with_ends(
    points: &[[f64; 3]],
    degree: usize,
    start: Option<[f64; 3]>,
    end: Option<[f64; 3]>,
) -> Result<(Curve, f64)> {
    if start.is_none() && end.is_none() {
        return ep_curve(points, degree);
    }
    if points.len() < 2 || points.len() > CURVE_MAX_POINTS || !points.iter().all(|p| coord_ok(p)) {
        return Err(Error::Argument);
    }
    if points.windows(2).any(|w| w.first() == w.get(1)) {
        return Err(Error::Argument);
    }
    if !(2..=MAX_DEGREE).contains(&degree) {
        return Err(Error::Argument);
    }
    let direction = |d: Option<[f64; 3]>| -> Result<Option<Vec3>> {
        match d {
            Some([x, y, z]) if [x, y, z].iter().all(|c| c.is_finite()) => {
                Ok(Some(Vec3::new(x, y, z)))
            }
            Some(_) => Err(Error::Argument),
            None => Ok(None),
        }
    };
    let (start, end) = (direction(start)?, direction(end)?);
    let data: Vec<Vec3> = points.iter().map(|&[x, y, z]| Vec3::new(x, y, z)).collect();
    run("curve_fit::ep_curve_with_ends", |_| {
        let params = param_values(&data, ParamMethod::ChordLength)?;
        let fit = match interp_end_directions(&data, degree, start, end, ParamMethod::ChordLength) {
            Ok(fit) => fit,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
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

/// Record edge `edge` of `body` (through `order`, its record's [`Order`]) as a free curve: the
/// edge's curve trimmed to the edge ([`create_trimmed_nurbs_curve`]: its B-spline over the
/// edge's interval, a trimmed or intersection edge's included) refitted as a clamped spline of
/// `degree` (2 or 3) with `spans` uniform spans on `[0, 1]` ([`rebuild`]), running the way the
/// edge's curve runs, and the largest distance in mm between the refit and the edge, kept
/// when within `tol`. The distance is between the shapes, whatever their parameters: the
/// kernel's sampled closest-point maximum taken both ways
/// ([`solidcore::curve::curve::Curve::curve_max_distance_between`], its samples by the knot
/// counts and the largest gap refined by a local maximisation). It is measured, not certified:
/// a gap narrower than the kernel's sampling can be missed. The span count is used as given:
/// a refit past `tol` is refused, and more spans fit closer.
///
/// # Errors
/// [`Error::Argument`] for an index past the record's edges, a degree outside `2..=3`, no
/// spans, or a `tol` not finite or negative; [`Error::Unsupported`] for an edge whose curve has
/// no B-spline form, or a rational one (a circle's, an ellipse's: the refit takes polynomial
/// splines); [`Error::PastTolerance`] with the distance when it is past `tol`;
/// [`Error::Record`] for an edge curve the record cannot hold; the kernel's status otherwise.
pub fn edge_as_curve(
    body: &Body,
    order: &Order,
    edge: usize,
    degree: usize,
    spans: usize,
    tol: f64,
) -> Result<(Curve, f64)> {
    if !(2..=3).contains(&degree) || spans == 0 || !tol.is_finite() || tol < 0.0 {
        return Err(Error::Argument);
    }
    let e = order::edge(order, edge)?;
    let trimmed = with_body("curve_fit::edge_as_curve", body, |k, b| {
        let Some(curve) = create_trimmed_nurbs_curve(k, b, e)? else {
            return Ok(Err(Error::Unsupported(
                "an edge whose curve has no B-spline form",
            )));
        };
        let made = kernel_record(k, &curve, &curve.natural_interval())?;
        Ok(match solidcore_convert::record_curve(made.curve) {
            Ok(curve) => Ok(curve),
            Err(Invalid(why)) => Err(Error::Record(why.to_owned())),
        })
    })??;
    if !matches!(trimmed, Curve::Spline { .. }) {
        return Err(Error::Unsupported(
            "a rational edge: the refit takes polynomial splines (its fit's certified bound)",
        ));
    }
    // rebuild's own bound compares the two at one parameter; the edge's parameter is not the
    // refit's spacing, so that bound can stand far above the gap between the shapes
    let (fit, _) = rebuild(&trimmed, degree, spans)?;
    let bound = run("curve_fit::edge_as_curve", |k| {
        let (Some(edge), Some(refit)) = (kernel_curve(k, &trimmed)?, kernel_curve(k, &fit)?) else {
            return Ok(Err(Error::Argument));
        };
        // each curve's samples dropped onto the other, the largest gap refined (0 samples:
        // the kernel's count from the knots, then its local maximisation)
        let there = refit.curve.curve_max_distance_between(
            k,
            &refit.interval,
            &edge.curve,
            edge.interval.min(),
            edge.interval.max(),
            0,
            None,
        )?;
        let back = edge.curve.curve_max_distance_between(
            k,
            &edge.interval,
            &refit.curve,
            refit.interval.min(),
            refit.interval.max(),
            0,
            None,
        )?;
        Ok(Ok(there.distance.max(back.distance)))
    })??;
    if bound <= tol {
        Ok((fit, bound))
    } else {
        Err(Error::PastTolerance { bound })
    }
}

/// The kernel's curve of the record spline `curve` over its whole domain; `None` for a curve
/// the conversion refuses.
fn kernel_curve(k: &Kernel, curve: &Curve) -> solidcore::Result<Option<EdgeCurve>> {
    let (Ok(record), Natural::Range(lo, hi)) = (solidcore_convert::curve(curve), curve.natural())
    else {
        return Ok(None);
    };
    from_parameters(k, &record, [lo, hi]).map(Some)
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
        assert_eq!(r.natural(), Natural::Range(0.0, 1.0));
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

    /// Whether `a` and `b` are within `tol` of each other in every coordinate.
    fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    #[test]
    fn a_box_edge_is_its_line() {
        let b = crate::make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
        let (rec, order) = b.to_record().unwrap();
        assert_eq!(rec.edges.len(), 12);
        for (i, e) in rec.edges.iter().enumerate() {
            assert!(matches!(e.curve, Curve::Line { .. }), "{:?}", e.curve);
            // a straight edge, refitted at one span: no distance but the rounding
            let refit = edge_as_curve(&b, &order, i, 3, 1, 1e-9);
            let Ok((fit, bound)) = &refit else {
                panic!("edge {i} refused: {refit:?}")
            };
            assert!(*bound < 1e-10, "{bound}");
            let Curve::Spline { knots, points } = fit else {
                panic!("a spline: {fit:?}")
            };
            assert_eq!(knots, &vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
            let [v0, v1] = e.vertices.map(|v| rec.vertices[v as usize].point);
            let (first, last) = (points[0], points[3]);
            assert!(
                (near(first, v0, 1e-10) && near(last, v1, 1e-10))
                    || (near(first, v1, 1e-10) && near(last, v0, 1e-10)),
                "edge {i}: {points:?} between {v0:?} and {v1:?}"
            );
            // every control point on the edge's line: (p - v0) x (v1 - v0) = 0
            let d = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            for p in points {
                let q = [p[0] - v0[0], p[1] - v0[1], p[2] - v0[2]];
                let cross = [
                    q[1] * d[2] - q[2] * d[1],
                    q[2] * d[0] - q[0] * d[2],
                    q[0] * d[1] - q[1] * d[0],
                ];
                assert!(near(cross, [0.0; 3], 1e-9), "edge {i}: {p:?} off the line");
            }
        }
    }

    /// The cubic Bézier wave drawn at z = 5 across the sheet [0, 20]^2: control points evenly
    /// spaced in x from -5 to 25 (x = -5 + 30 s), y from the Bernstein polynomial of
    /// 10, 14, 6, 10.
    const CUTTER: [[f64; 3]; 4] = [
        [-5.0, 10.0, 5.0],
        [5.0, 14.0, 5.0],
        [15.0, 6.0, 5.0],
        [25.0, 10.0, 5.0],
    ];

    /// The wave's y at x.
    fn wave_y(x: f64) -> f64 {
        let s = (x + 5.0) / 30.0;
        let r = 1.0 - s;
        10.0 * r * r * r + 3.0 * 14.0 * s * r * r + 3.0 * 6.0 * s * s * r + 10.0 * s * s * s
    }

    /// The sheet [0, 20]^2 at z = 0 cut in two along the wave projected onto it, and the record
    /// index of the cut's edge (the one spline edge; the rest are the square's lines).
    fn cut_sheet() -> (Body, Order, usize) {
        use crate::sweep::{planar_faces, project_curves, Curve as Path};
        let corners = [
            [0.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 20.0, 0.0],
            [0.0, 20.0, 0.0],
        ];
        let square: Vec<Path> = (0..4)
            .map(|i| Path::Line {
                start: corners[i],
                end: corners[(i + 1) % 4],
            })
            .collect();
        let sheet = planar_faces(&square).unwrap();
        let (_, o) = sheet.to_record().unwrap();
        let wave = Path::Nurbs {
            degree: 3,
            points: CUTTER.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        };
        let projected: Vec<Path> = project_curves(&sheet, &o, 0, &[wave], [0.0, 0.0, -1.0], 1e-6)
            .unwrap()
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        let (cut, _) = crate::trim::split_face_by_curves(&sheet, &o, 0, &projected, 1e-6).unwrap();
        let (rec, order) = cut.to_record().unwrap();
        let splines: Vec<usize> = (0..rec.edges.len())
            .filter(|&i| matches!(rec.edges[i].curve, Curve::Spline { .. }))
            .collect();
        assert_eq!(splines.len(), 1, "{:?}", rec.edges);
        (cut, order, splines[0])
    }

    #[test]
    fn a_trimmed_edge_is_refitted_on_it_within_its_bound() {
        // A cubic of four spans: CI measured the certified bound of the refit against the edge
        // at one parameter (rebuild's) at 4.998e-4, an upper bound of the distance between the
        // shapes, so the measured distance is within the pinned 5e-4.
        const REACHED: f64 = 5e-4;
        let (cut, order, edge) = cut_sheet();
        let refit = edge_as_curve(&cut, &order, edge, 3, 4, REACHED);
        let Ok((fit, bound)) = &refit else {
            panic!("refused: {refit:?}")
        };
        assert!((0.0..=REACHED).contains(bound), "{bound}");
        let Curve::Spline { knots, points } = fit else {
            panic!("a spline: {fit:?}")
        };
        assert_eq!((points.len(), knots.len()), (7, 11));
        // within REACHED of the edge, the edge within the projection's 1e-6 of the wave: a gap
        // along y is at most the distance times 1 + |dy/dx| (at most 0.4 on the wave)
        let allowed = (REACHED + 1e-6) * 1.4;
        for i in 0..=200 {
            let p = at(fit, f64::from(i) / 200.0);
            assert!(p[2].abs() <= allowed, "{p:?} off the sheet");
            assert!(
                (-allowed..=20.0 + allowed).contains(&p[0]),
                "{p:?} past the sheet"
            );
            assert!((p[1] - wave_y(p[0])).abs() <= allowed, "{p:?} off the wave");
        }
        // from one side of the sheet to the other
        let (a, b) = (at(fit, 0.0)[0], at(fit, 1.0)[0]);
        let (lo, hi) = (a.min(b), a.max(b));
        assert!(
            lo.abs() <= allowed && (hi - 20.0).abs() <= allowed,
            "{a} {b}"
        );
        // too few spans: a quadratic of one span has no inflection, the wave has one at x = 10
        assert!(matches!(
            edge_as_curve(&cut, &order, edge, 2, 1, 1e-6),
            Err(Error::PastTolerance { bound }) if bound > 1e-6
        ));
    }

    #[test]
    fn a_circular_edge_waits_and_bad_asks_are_refused() {
        let cylinder = crate::make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let (rec, order) = cylinder.to_record().unwrap();
        let circle = rec
            .edges
            .iter()
            .position(|e| matches!(e.curve, Curve::Circle { .. }))
            .unwrap();
        assert!(matches!(
            edge_as_curve(&cylinder, &order, circle, 3, 4, 1e-3),
            Err(Error::Unsupported(_))
        ));

        let b = crate::make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
        let (rec, order) = b.to_record().unwrap();
        let past = rec.edges.len();
        for (edge, degree, spans, tol) in [
            (0, 1, 1, 1e-6),
            (0, 4, 1, 1e-6),
            (0, 3, 0, 1e-6),
            (0, 3, 1, f64::NAN),
            (0, 3, 1, -1.0),
            (past, 3, 1, 1e-6),
        ] {
            assert_eq!(
                edge_as_curve(&b, &order, edge, degree, spans, tol),
                Err(Error::Argument),
                "{edge} {degree} {spans} {tol}"
            );
        }
    }
}
