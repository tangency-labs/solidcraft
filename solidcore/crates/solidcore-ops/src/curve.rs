//! Free curves: a B-spline from the control points a designer places, one of its control points
//! moved, the curve under an affine map, and its curvature comb. The curve is the record's
//! ([`Curve`]); the knots, the map's carrying of the control points and the curvature are the
//! kernel's ([`solidcore::nurbs::knots::clamped_uniform`],
//! [`solidcore::nurbs::crv_basic::Curve::transform`],
//! [`solidcore::query::continuity::curve_comb`]). Ops checks the inputs and converts.

use solidcore::curve::from_parameters::from_parameters;
use solidcore::geom::certified::Verdict;
use solidcore::nurbs::knots::{clamped_uniform, MAX_DEGREE};
use solidcore::nurbs::math::{MatrixType, RealMatrix};
use solidcore::query::continuity;
use solidcore::SolidError;
use solidcore_record::brep::Curve;

use crate::body::REGULAR_RATIO;
use crate::continuity::{tooth, CombTooth, MAX_COMB_TEETH, MIN_COMB_TEETH};
use crate::curve_knots::{net_of, record_of};
use crate::session::run;
use crate::sweep::{coord_ok, CURVE_MAX_POINTS};
use crate::{Error, Result};

/// The clamped B-spline with `points` as its control points, of `degree` (at most one less than
/// the number of points: two points make a line, three a quadratic), on `[0, 1]` with the
/// kernel's uniform knots. It passes through its first and last point.
///
/// # Errors
/// [`Error::Argument`] for fewer than two points, more than [`CURVE_MAX_POINTS`], a coordinate
/// that is not finite or past the coordinate bound, or a degree of 0 or past the kernel's most.
pub fn cv_curve(points: &[[f64; 3]], degree: usize) -> Result<Curve> {
    if points.len() < 2 || points.len() > CURVE_MAX_POINTS || !points.iter().all(|p| coord_ok(p)) {
        return Err(Error::Argument);
    }
    if degree == 0 || degree > MAX_DEGREE {
        return Err(Error::Argument);
    }
    let degree = degree.min(points.len() - 1);
    let knots = clamped_uniform(points.len(), degree).map_err(|_| Error::Argument)?;
    Ok(Curve::Spline {
        knots,
        points: points.to_vec(),
    })
}

/// `curve` with its control point `index` at `to`: its knots, degree and every other point as
/// they were. A spline's or a polyline's point. On a closed periodic spline (unclamped knots, its
/// polygon repeating its first `degree` points after its last, as
/// [`crate::curve_fit::closed_cv_curve`] makes it) a repeated point moves with its copy, so the
/// curve stays closed and smooth.
///
/// # Errors
/// [`Error::Argument`] for another kind of curve (a rational spline's point carries its weight),
/// an index past its points, or a point that is not finite or past the coordinate bound.
pub fn move_cv(curve: &Curve, index: usize, to: [f64; 3]) -> Result<Curve> {
    if !coord_ok(&to) {
        return Err(Error::Argument);
    }
    let twin = match curve {
        Curve::Spline { knots, points } => periodic_twin(knots, points, index),
        _ => None,
    };
    let mut moved = curve.clone();
    let (Curve::Spline { points, .. } | Curve::Polyline { points }) = &mut moved else {
        return Err(Error::Argument);
    };
    *points.get_mut(index).ok_or(Error::Argument)? = to;
    if let Some(copy) = twin {
        *points.get_mut(copy).ok_or(Error::Argument)? = to;
    }
    Ok(moved)
}

/// The index of the copy of control point `index` of a closed periodic spline: one whose knots
/// are periodic ([`solidcore::nurbs::knots::is_periodic`]) and whose last `degree` points
/// repeat its first `degree`. `None` for another spline, or a point that is not repeated.
fn periodic_twin(knots: &[f64], points: &[[f64; 3]], index: usize) -> Option<usize> {
    let degree = knots.len().checked_sub(points.len() + 1)?;
    let distinct = points.len().checked_sub(degree)?;
    if !solidcore::nurbs::knots::is_periodic(knots, degree) {
        return None;
    }
    let repeats =
        (0..degree).all(|i| points.get(i).is_some() && points.get(i) == points.get(i + distinct));
    if !repeats {
        return None;
    }
    if index < degree {
        Some(index + distinct)
    } else {
        index.checked_sub(distinct).filter(|i| *i < degree)
    }
}

/// `curve` under the affine `matrix`, as [`Body::transform`](crate::Body::transform) takes it:
/// rows 0-2 the images of x, y and z, row 3 the translation (`p' = p·A + t`), the fourth column
/// `(0, 0, 0, 1)`. Every control point is carried by the map
/// ([`solidcore::nurbs::crv_basic::Curve::transform`], `N_CrvTransform`); the degree, the knots
/// and the weights stay as they were. An affine map
/// commutes with the convex combinations a NURBS curve is made of, so the result is the image of
/// the curve itself, to the rounding of each carried point (none for a map whose entries are 0
/// and ±1 on a spline's points). Reflections, non-uniform scales and shears are maps like any
/// other. A spline or a rational spline, clamped or closed (a closed curve's repeated points are
/// carried alike, so it stays closed).
///
/// # Errors
/// [`Error::Argument`], before the curve is read, for a matrix the kernel does not prove regular
/// ([`solidcore::Transform::is_regular`] at [`REGULAR_RATIO`]: an entry not finite, a fourth
/// column that is not `(0, 0, 0, 1)`, a smallest scale not proved above the ratio of the
/// largest); [`Error::Argument`] for another kind of curve, one the record refuses, or an image
/// the record refuses (a coordinate carried past what a double holds); the kernel's status
/// otherwise.
pub fn transform(curve: &Curve, matrix: &[[f64; 4]; 4]) -> Result<Curve> {
    let map = solidcore::Transform { m: *matrix };
    if map.is_regular(REGULAR_RATIO) != Verdict::Yes {
        return Err(Error::Argument);
    }
    let mut nurb = net_of(curve)?;
    // The kernel's matrix acts on a column (`x' = r00 x + r01 y + r02 z + r03`): the record's
    // row-vector matrix, its entries moved to their transposed places.
    let [[a00, a01, a02, a03], [a10, a11, a12, a13], [a20, a21, a22, a23], [a30, a31, a32, a33]] =
        *matrix;
    let rows = vec![
        vec![a00, a10, a20, a30],
        vec![a01, a11, a21, a31],
        vec![a02, a12, a22, a32],
        vec![a03, a13, a23, a33],
    ];
    run("curve::transform", |_| {
        let columns = RealMatrix::from_rows(3, 3, rows, MatrixType::Full, 3)?;
        nurb.transform(&columns)?;
        Ok(record_of(&nurb))
    })?
}

/// The curvature comb of `curve` over `interval` of its parameter: the kernel's curvature vector
/// at `teeth` even steps, both ends included. Stations where the curve has no tangent are left
/// out.
///
/// # Errors
/// [`Error::Argument`] for a tooth count outside [`MIN_COMB_TEETH`]..=[`MAX_COMB_TEETH`], or a
/// curve or interval the kernel refuses; the kernel's status otherwise.
pub fn curve_comb(curve: &Curve, interval: [f64; 2], teeth: usize) -> Result<Vec<CombTooth>> {
    if !(MIN_COMB_TEETH..=MAX_COMB_TEETH).contains(&teeth)
        || !interval.iter().all(|t| t.is_finite())
    {
        return Err(Error::Argument);
    }
    let record = solidcore_convert::curve(curve).map_err(|_| Error::Argument)?;
    run("curve_comb", |k| {
        let made = match from_parameters(k, &record, interval) {
            Ok(made) => made,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
        Ok(Ok(continuity::curve_comb(
            k,
            &made.curve,
            made.interval,
            teeth,
            false,
        )?
        .into_iter()
        .map(tooth)
        .collect()))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];

    #[test]
    fn a_cubic_through_four_points_is_clamped_on_its_end_points() {
        let c = cv_curve(&SQUARE, 3).unwrap();
        let Curve::Spline { knots, points } = &c else {
            panic!("a spline")
        };
        assert_eq!(knots, &vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(points, &SQUARE.to_vec());
        // Clamped: the curve starts and ends on its first and last points (the comb's end teeth
        // stand on the curve's ends).
        let ends = curve_comb(&c, [0.0, 1.0], 2).unwrap();
        assert_eq!(ends.len(), 2);
        for (tooth, cv) in ends.iter().zip([SQUARE[0], SQUARE[3]]) {
            assert!(
                tooth.at.iter().zip(cv).all(|(a, b)| (a - b).abs() < 1e-12),
                "{:?}",
                tooth.at
            );
        }
        // Fewer points lower the degree: two points make a line, three a quadratic.
        let Curve::Spline { knots, .. } = cv_curve(&SQUARE[..2], 3).unwrap() else {
            panic!()
        };
        assert_eq!(knots, vec![0.0, 0.0, 1.0, 1.0]);
        let Curve::Spline { knots, .. } = cv_curve(&SQUARE[..3], 5).unwrap() else {
            panic!()
        };
        assert_eq!(knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn bad_control_points_are_refused() {
        assert_eq!(cv_curve(&SQUARE[..1], 3), Err(Error::Argument));
        assert_eq!(
            cv_curve(&[[0.0; 3], [f64::NAN, 0.0, 0.0]], 1),
            Err(Error::Argument)
        );
        assert_eq!(cv_curve(&SQUARE, 0), Err(Error::Argument));
        assert_eq!(cv_curve(&SQUARE, MAX_DEGREE + 1), Err(Error::Argument));
    }

    #[test]
    fn moving_a_cv_changes_only_that_point() {
        let c = cv_curve(&SQUARE, 3).unwrap();
        let moved = move_cv(&c, 2, [12.0, 14.0, 3.0]).unwrap();
        let (
            Curve::Spline {
                knots: k0,
                points: p0,
            },
            Curve::Spline {
                knots: k1,
                points: p1,
            },
        ) = (&c, &moved)
        else {
            panic!()
        };
        assert_eq!(k0, k1);
        assert_eq!(p1[2], [12.0, 14.0, 3.0]);
        for i in [0, 1, 3] {
            assert_eq!(p0[i], p1[i]);
        }
        assert_eq!(move_cv(&c, 4, [0.0; 3]), Err(Error::Argument));
        assert_eq!(
            move_cv(&c, 1, [f64::INFINITY, 0.0, 0.0]),
            Err(Error::Argument)
        );
        assert_eq!(
            move_cv(
                &Curve::Line {
                    start: [0.0; 3],
                    end: [1.0; 3]
                },
                0,
                [0.0; 3]
            ),
            Err(Error::Argument)
        );
    }

    #[test]
    fn moving_a_repeated_cv_of_a_closed_curve_moves_its_copy() {
        let c = crate::curve_fit::closed_cv_curve(&SQUARE, 3).unwrap();
        // points 0, 1 and 2 are repeated as 4, 5 and 6
        for (index, copy) in [(1, 5), (5, 1), (0, 4), (6, 2)] {
            let moved = move_cv(&c, index, [5.0, -3.0, 2.0]).unwrap();
            let Curve::Spline { points, knots } = &moved else {
                panic!()
            };
            assert_eq!(points[index], [5.0, -3.0, 2.0]);
            assert_eq!(points[copy], [5.0, -3.0, 2.0]);
            let Curve::Spline { knots: k0, .. } = &c else {
                panic!()
            };
            assert_eq!(knots, k0);
        }
        // the one point not repeated moves alone
        let moved = move_cv(&c, 3, [5.0, -3.0, 2.0]).unwrap();
        let (Curve::Spline { points: p0, .. }, Curve::Spline { points: p1, .. }) = (&c, &moved)
        else {
            panic!()
        };
        for i in [0, 1, 2, 4, 5, 6] {
            assert_eq!(p0[i], p1[i]);
        }
        // repeated points on unclamped knots that are not periodic move alone
        let (a, b, c) = ([0.0; 3], [4.0, 0.0, 0.0], [4.0, 4.0, 0.0]);
        let open = Curve::Spline {
            knots: vec![-2.0, -1.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0],
            points: vec![a, b, c, a, b],
        };
        let moved = move_cv(&open, 0, [1.0, 1.0, 1.0]).unwrap();
        let Curve::Spline { points, .. } = &moved else {
            panic!()
        };
        assert_eq!((points[0], points[3]), ([1.0, 1.0, 1.0], a));
        // a clamped curve whose last point is its first is not periodic
        let mut ring = SQUARE.to_vec();
        ring.push(SQUARE[0]);
        let open = cv_curve(&ring, 3).unwrap();
        let moved = move_cv(&open, 0, [1.0, 1.0, 1.0]).unwrap();
        let Curve::Spline { points, .. } = &moved else {
            panic!()
        };
        assert_eq!(points[4], SQUARE[0]);
    }

    /// The matrix of the reflection about the plane y = 0.
    const MIRROR_Y: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    #[test]
    fn a_cubic_reflected_about_y_0_has_its_cvs_mirrored_exactly() {
        let wave = [
            [0.0, 1.0, 0.5],
            [2.0, 3.0, -1.0],
            [4.0, -2.0, 0.25],
            [6.0, 0.5, 2.0],
            [8.0, 4.0, 0.0],
        ];
        let c = cv_curve(&wave, 3).unwrap();
        let image = transform(&c, &MIRROR_Y).unwrap();
        let (
            Curve::Spline { knots, points },
            Curve::Spline {
                knots: k1,
                points: p1,
            },
        ) = (&c, &image)
        else {
            panic!("a spline: {image:?}")
        };
        assert_eq!(k1, knots);
        let mirrored: Vec<[f64; 3]> = points.iter().map(|&[x, y, z]| [x, -y, z]).collect();
        assert_eq!(p1, &mirrored);
        // Twice is the curve again.
        assert_eq!(transform(&image, &MIRROR_Y).unwrap(), c);
    }

    #[test]
    fn a_rational_curve_keeps_its_weights_and_knots() {
        // A quarter circle of radius 10 about the origin, as a rational quadratic.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        let image = transform(&arc, &MIRROR_Y).unwrap();
        let (
            Curve::RationalSpline { knots, homogeneous },
            Curve::RationalSpline {
                knots: k1,
                homogeneous: h1,
            },
        ) = (&arc, &image)
        else {
            panic!("a rational spline: {image:?}")
        };
        assert_eq!(k1, knots);
        for (p, q) in homogeneous.iter().zip(h1) {
            assert_eq!(q[3], p[3], "the weight");
            let mirrored = [p[0], -p[1], p[2]];
            assert!(
                q.iter().zip(mirrored).all(|(a, b)| (a - b).abs() < 1e-12),
                "{q:?} against {mirrored:?}"
            );
        }
    }

    #[test]
    fn a_motion_takes_rows_as_the_axes_images_and_row_3_as_the_move() {
        // A quarter turn about z (x to y, y to -x), then a move by (1, 2, 3): the convention of
        // `Body::transform`.
        let turn = [
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 2.0, 3.0, 1.0],
        ];
        let image = transform(&cv_curve(&SQUARE, 3).unwrap(), &turn).unwrap();
        let Curve::Spline { points, .. } = &image else {
            panic!("a spline: {image:?}")
        };
        let moved: Vec<[f64; 3]> = SQUARE
            .iter()
            .map(|&[x, y, z]| [1.0 - y, 2.0 + x, 3.0 + z])
            .collect();
        assert_eq!(points, &moved);
    }

    #[test]
    fn a_closed_curve_stays_closed() {
        let c = crate::curve_fit::closed_cv_curve(&SQUARE, 3).unwrap();
        let image = transform(&c, &MIRROR_Y).unwrap();
        let (Curve::Spline { knots, .. }, Curve::Spline { knots: k1, points }) = (&c, &image)
        else {
            panic!("a spline: {image:?}")
        };
        assert_eq!(k1, knots);
        // points 0, 1 and 2 are repeated as 4, 5 and 6
        for (p, copy) in points.iter().zip(&points[4..]) {
            assert_eq!(p, copy);
        }
        assert_eq!(points[1], [10.0, 0.0, 0.0]);
    }

    #[test]
    fn a_singular_matrix_or_another_curve_is_refused() {
        let c = cv_curve(&SQUARE, 3).unwrap();
        let mut flat = MIRROR_Y;
        flat[1][1] = 0.0;
        assert_eq!(transform(&c, &flat), Err(Error::Argument));
        let mut projective = MIRROR_Y;
        projective[0][3] = 1.0;
        assert_eq!(transform(&c, &projective), Err(Error::Argument));
        let mut nan = MIRROR_Y;
        nan[3][0] = f64::NAN;
        assert_eq!(transform(&c, &nan), Err(Error::Argument));
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(transform(&line, &MIRROR_Y), Err(Error::Argument));
    }

    #[test]
    fn an_arcs_comb_points_at_its_centre_with_the_inverse_radius() {
        // A quarter circle of radius 10 about (1, 2, 3) in its z plane, as a rational quadratic.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let (cx, cy, cz) = (1.0, 2.0, 3.0);
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [cx + 10.0, cy, cz, 1.0],
                [(cx + 10.0) * w, (cy + 10.0) * w, cz * w, w],
                [cx, cy + 10.0, cz, 1.0],
            ],
        };
        let comb = curve_comb(&arc, [0.0, 1.0], 9).unwrap();
        assert_eq!(comb.len(), 9);
        for t in &comb {
            let to_centre = [cx - t.at[0], cy - t.at[1], cz - t.at[2]];
            let r2 = to_centre.iter().map(|x| x * x).sum::<f64>();
            assert!((r2 - 100.0).abs() < 1e-7, "on the arc: r² = {r2}");
            for (k, d) in t.curvature.iter().zip(to_centre) {
                assert!(
                    (k - d / 100.0).abs() < 1e-9,
                    "κ N = (centre - p) / r²: {:?} at {:?}",
                    t.curvature,
                    t.at
                );
            }
        }
    }

    #[test]
    fn a_straight_splines_comb_is_zero_and_bad_counts_are_refused() {
        let line = cv_curve(&[[0.0; 3], [5.0, 5.0, 0.0], [10.0, 10.0, 0.0]], 2).unwrap();
        let comb = curve_comb(&line, [0.0, 1.0], 5).unwrap();
        assert_eq!(comb.len(), 5);
        assert!(
            comb.iter()
                .all(|t| t.curvature.iter().all(|k| k.abs() < 1e-12)),
            "{comb:?}"
        );
        assert_eq!(
            curve_comb(&line, [0.0, 1.0], MIN_COMB_TEETH - 1),
            Err(Error::Argument)
        );
        assert_eq!(
            curve_comb(&line, [0.0, 1.0], MAX_COMB_TEETH + 1),
            Err(Error::Argument)
        );
        assert_eq!(curve_comb(&line, [0.0, f64::NAN], 5), Err(Error::Argument));
    }
}
