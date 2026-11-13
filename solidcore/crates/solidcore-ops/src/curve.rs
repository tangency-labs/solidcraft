//! Free curves: a B-spline from the control points a designer places, one of its control points
//! moved, the curve under an affine map, the curve seen in two views, its curvature comb, and the
//! blend from one curve's end to another's. The curve is the record's ([`Curve`]); the knots, the
//! map's carrying of the control points, the views' meeting, the curvature and the blend are the
//! kernel's ([`solidcore::nurbs::knots::clamped_uniform`],
//! [`solidcore::nurbs::crv_basic::Curve::transform`],
//! [`solidcore::intersect::views::curves_from_views`],
//! [`solidcore::query::continuity::curve_comb`], [`solidcore::curve::blend::blend_curve`]). Ops
//! checks the inputs and converts.

use solidcore::curve::blend::{self as kblend, BlendEnd};
use solidcore::curve::from_parameters::{from_parameters, EdgeCurve};
use solidcore::geom::certified::Verdict;
use solidcore::intersect::views::{curves_from_views, views_do_not_meet};
use solidcore::kernel::Kernel;
use solidcore::nurbs::knots::{clamped_uniform, MAX_DEGREE};
use solidcore::nurbs::math::{MatrixType, RealMatrix};
use solidcore::query::continuity;
use solidcore::{SolidError, Vec3};
use solidcore_record::brep::{check_curve, Budget, Curve, Invalid, Natural};

use crate::body::REGULAR_RATIO;
use crate::continuity::{tooth, CombTooth, MAX_COMB_TEETH, MIN_COMB_TEETH};
use crate::curve_knots::{net_of, record_of};
use crate::session::run;
use crate::sweep::{coord_ok, CURVE_MAX_POINTS};
use crate::{Error, Result};

/// Which end of a curve a blend meets, and how ([`blend_curve`]).
pub use solidcore::curve::blend::{Contact, End};

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

/// The 3D curves seen as `a` along `a_dir` and as `b` along `b_dir` (a side view and a plan
/// view, say): where `a` extruded both ways along `a_dir` meets `b` extruded along `b_dir`, far
/// enough that none of it is left out ([`curves_from_views`], whose docs give the bound). Each
/// curve is a spline within the kernel's default approximation tolerance of both extrusions.
/// The directions need not be unit vectors; `a` and `b` are splines or rational splines, open or
/// closed.
///
/// # Errors
/// [`Error::Argument`] for a direction not finite or past the coordinate bound, of no length,
/// or (within the kernel's angular zero) parallel to the other, or a curve that is not a spline
/// the record takes; [`Error::Empty`] when the views do not meet; [`Error::Record`] for a
/// meeting curve the record cannot hold; the kernel's status otherwise.
pub fn two_view(a: &Curve, a_dir: [f64; 3], b: &Curve, b_dir: [f64; 3]) -> Result<Vec<Curve>> {
    if !coord_ok(&a_dir) || !coord_ok(&b_dir) {
        return Err(Error::Argument);
    }
    let (a, b) = (net_of(a)?, net_of(b)?);
    let ([ax, ay, az], [bx, by, bz]) = (a_dir, b_dir);
    run("curve::two_view", |k| {
        let curves =
            match curves_from_views(k, &a, Vec3::new(ax, ay, az), &b, Vec3::new(bx, by, bz)) {
                Ok(curves) => curves,
                Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
                Err(e) if views_do_not_meet(&e) => return Ok(Err(Error::Empty)),
                Err(e) => return Err(e),
            };
        let mut records = Vec::new();
        for c in &curves {
            let made = solidcore::curve::from_parameters::record_of(k, c, &c.natural_interval())?;
            match solidcore_convert::record_curve(made.curve) {
                Ok(record) => records.push(record),
                Err(Invalid(why)) => return Ok(Err(Error::Record(why.to_owned()))),
            }
        }
        Ok(Ok(records))
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

/// The kernel's curve of `curve` (a spline, a rational spline or a polyline, over its whole
/// parameter range) for a blend: its natural interval the record's, so that its ends are the
/// record's ends.
fn blend_input(k: &Kernel, curve: &Curve) -> solidcore::Result<Result<EdgeCurve>> {
    let kinds = matches!(
        curve,
        Curve::Spline { .. } | Curve::RationalSpline { .. } | Curve::Polyline { .. }
    );
    if !kinds || check_curve(curve, &mut Budget::input()).is_err() {
        return Ok(Err(Error::Argument));
    }
    let Natural::Range(lo, hi) = curve.natural() else {
        return Ok(Err(Error::Argument));
    };
    let Ok(record) = solidcore_convert::curve(curve) else {
        return Ok(Err(Error::Argument));
    };
    let made = match from_parameters(k, &record, [lo, hi]) {
        Ok(made) => made,
        Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
        Err(e) => return Err(e),
    };
    let natural = made.curve.natural_interval();
    if natural.min() != made.interval.min() || natural.max() != made.interval.max() {
        return Ok(Err(Error::Unsupported(
            "a curve whose kernel form runs past its record's interval",
        )));
    }
    Ok(Ok(made))
}

/// The blend from end `a_end` of `a` to end `b_end` of `b`
/// ([`solidcore::curve::blend::blend_curve`]): one Bézier curve of the least degree that meets
/// each end at its contact (G0, G1 or G2; degree 1 from G0 to G0 up to 5 from G2 to G2), its first
/// inner control point off each end along the curve's tangent by the end's shape factor times the
/// chord between the ends. The kernel measures each end's continuity on the curve it built and
/// refuses one short of its contact. `a` and `b` are splines, rational splines or polylines, each
/// over its whole parameter range; the blend is a spline on `[0, 1]` from `a`'s end to `b`'s.
///
/// # Errors
/// - [`Error::Argument`]: another kind of curve, one the record refuses, a shape factor not
///   finite and positive at G1 or G2, ends that coincide, or a zero-length tangent at an end met
///   at G1 or G2;
/// - [`Error::Unsupported`]: a curve whose kernel form runs past the record's interval;
/// - [`Error::Invariant`]: a blend the kernel measures short of a contact;
/// - [`Error::Record`]: a blend the record cannot hold;
/// - the kernel's status otherwise.
#[allow(clippy::too_many_arguments)]
pub fn blend_curve(
    a: &Curve,
    a_end: End,
    a_contact: Contact,
    a_shape: f64,
    b: &Curve,
    b_end: End,
    b_contact: Contact,
    b_shape: f64,
) -> Result<Curve> {
    run("curve::blend_curve", |k| {
        let ka = match blend_input(k, a)? {
            Ok(made) => made,
            Err(e) => return Ok(Err(e)),
        };
        let kb = match blend_input(k, b)? {
            Ok(made) => made,
            Err(e) => return Ok(Err(e)),
        };
        let from = BlendEnd {
            curve: &ka.curve,
            end: a_end,
            contact: a_contact,
            shape: a_shape,
        };
        let to = BlendEnd {
            curve: &kb.curve,
            end: b_end,
            contact: b_contact,
            shape: b_shape,
        };
        let made = match kblend::blend_curve(k, &from, &to) {
            Ok(made) => made,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
        let interval = made.curve.natural_interval();
        let record = solidcore::curve::from_parameters::record_of(k, &made.curve, &interval)?;
        Ok(match solidcore_convert::record_curve(record.curve) {
            Ok(curve) => Ok(curve),
            Err(Invalid(why)) => Err(Error::Record(why.to_owned())),
        })
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

    /// The kernel's default approximation tolerance of a traced curve: half its zone tolerance
    /// of 1e-5.
    const APPROX_TOL: f64 = 5e-6;

    /// The side view, z = f(x) in the plane y = 0: a cubic Bézier curve whose control points
    /// are evenly spaced in x, so x = 9 t; its slope |dz/dx| at most 1 (its polygon's).
    fn side() -> Curve {
        cv_curve(
            &[
                [0.0, 0.0, 0.0],
                [3.0, 0.0, 2.0],
                [6.0, 0.0, -1.0],
                [9.0, 0.0, 1.0],
            ],
            3,
        )
        .unwrap()
    }

    /// The plan view, y = g(x) in the plane z = 0, x = shift - 3 + 15 t; |dy/dx| at most 0.6.
    fn plan(shift: f64) -> Curve {
        cv_curve(
            &[
                [shift - 3.0, 1.0, 0.0],
                [shift + 2.0, 4.0, 0.0],
                [shift + 7.0, 2.0, 0.0],
                [shift + 12.0, 5.0, 0.0],
            ],
            3,
        )
        .unwrap()
    }

    /// The point of the spline `c` at `t`, by the kernel's evaluator.
    fn at(c: &Curve, t: f64) -> [f64; 3] {
        let p = net_of(c)
            .unwrap()
            .eval(t, solidcore::nurbs::knots::Side::Left)
            .unwrap();
        [p.x, p.y, p.z]
    }

    #[test]
    fn a_side_and_a_plan_view_give_the_curve_on_both_extrusions() {
        let (a, b) = (side(), plan(0.0));
        let curves = two_view(&a, [0.0, 1.0, 0.0], &b, [0.0, 0.0, 2.0]).unwrap();
        assert!(!curves.is_empty());
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in &curves {
            let (Curve::Spline { knots, .. } | Curve::RationalSpline { knots, .. }) = c else {
                panic!("a spline: {c:?}")
            };
            let (t0, t1) = (knots[0], knots[knots.len() - 1]);
            for i in 0..=64 {
                let x = at(c, t0 + (t1 - t0) * f64::from(i) / 64.0);
                assert!(
                    (-APPROX_TOL..=9.0 + APPROX_TOL).contains(&x[0]),
                    "past the side view: {x:?}"
                );
                (lo, hi) = (lo.min(x[0]), hi.max(x[0]));
                // On the side view's extrusion along y: a gap measured along z is at most the
                // distance times sqrt(1 + slope²), below 1 + slope, the slope at most 1.
                let on_a = at(&a, (x[0] / 9.0).clamp(0.0, 1.0));
                assert!(
                    (on_a[2] - x[2]).abs() <= 2.0 * APPROX_TOL,
                    "{x:?} against {on_a:?}"
                );
                // On the plan view's extrusion along z, the slope at most 0.6.
                let on_b = at(&b, (x[0] + 3.0) / 15.0);
                assert!(
                    (on_b[1] - x[1]).abs() <= 1.6 * APPROX_TOL,
                    "{x:?} against {on_b:?}"
                );
            }
        }
        // The whole of the side view's run is seen.
        assert!(
            lo <= APPROX_TOL && hi >= 9.0 - APPROX_TOL,
            "from {lo} to {hi}"
        );
    }

    /// A side view in y = 0 (seen along y) and a plan view in z = 0 (seen along z) at model
    /// scale, about 170 mm across: quadratic Bézier curves, both increasing in x.
    const SIDE_Q: [[f64; 3]; 3] = [
        [-92.06693847750134, 0.0, -39.76423812596792],
        [0.0, 0.0, 18.451591211372346],
        [73.55298836314594, 0.0, 15.024361734430357],
    ];
    const PLAN_Q: [[f64; 3]; 3] = [
        [-16.758544054811637, -82.27159357644084, 0.0],
        [42.459806193714414, -83.42331426400659, 0.0],
        [76.37870944288886, -34.86770213654586, 0.0],
    ];

    /// The point of the quadratic Bézier curve `q` at `t` and its derivative, in Bernstein's
    /// form (none of the kernel's evaluators).
    fn bernstein(q: &[[f64; 3]; 3], t: f64) -> ([f64; 3], [f64; 3]) {
        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
        (
            [0, 1, 2].map(|i| b0 * q[0][i] + b1 * q[1][i] + b2 * q[2][i]),
            [0, 1, 2]
                .map(|i| 2.0 * (1.0 - t) * (q[1][i] - q[0][i]) + 2.0 * t * (q[2][i] - q[1][i])),
        )
    }

    /// The parameter of `q` (increasing in x) where its x is `x`, by bisection on [0, 1].
    fn at_x(q: &[[f64; 3]; 3], x: f64) -> f64 {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..100 {
            let m = 0.5 * (lo + hi);
            if bernstein(q, m).0[0] < x {
                lo = m;
            } else {
                hi = m;
            }
        }
        0.5 * (lo + hi)
    }

    /// The point at `t` of the record spline with `knots` and homogeneous points `pw`, by
    /// de Boor's algorithm (none of the kernel's evaluators).
    fn de_boor(knots: &[f64], pw: &[[f64; 4]], t: f64) -> [f64; 3] {
        let n = pw.len();
        let p = knots.len() - n - 1;
        let k = (p..n).rev().find(|&k| knots[k] <= t).unwrap_or(p);
        let mut d: Vec<[f64; 4]> = (0..=p).map(|j| pw[j + k - p]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let (lo, hi) = (knots[j + k - p], knots[j + 1 + k - r]);
                let alpha = (t - lo) / (hi - lo);
                d[j] = [0, 1, 2, 3].map(|i| (1.0 - alpha) * d[j - 1][i] + alpha * d[j][i]);
            }
        }
        let h = d[p];
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    #[test]
    fn views_at_model_scale_give_the_curve_within_the_tolerance() {
        let side = cv_curve(&SIDE_Q, 2).unwrap();
        let plan = cv_curve(&PLAN_Q, 2).unwrap();
        let curves = two_view(&side, [0.0, 1.0, 0.0], &plan, [0.0, 0.0, 1.0]).unwrap();
        assert!(!curves.is_empty());
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in &curves {
            let (knots, pw): (&[f64], Vec<[f64; 4]>) = match c {
                Curve::Spline { knots, points } => (
                    knots,
                    points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect(),
                ),
                Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
                _ => panic!("a spline: {c:?}"),
            };
            let p = knots.len() - pw.len() - 1;
            let (t0, t1) = (knots[p], knots[pw.len()]);
            for i in 0..=512 {
                let x = de_boor(knots, &pw, t0 + (t1 - t0) * f64::from(i) / 512.0);
                (lo, hi) = (lo.min(x[0]), hi.max(x[0]));
                // A gap along the view's other axis is at most the distance times
                // sqrt(1 + slope²), below 1 + slope; twice the tolerance, as the intersector
                // checks its spans at interior samples.
                let (q, d) = bernstein(&SIDE_Q, at_x(&SIDE_Q, x[0]));
                let allowed = 2.0 * APPROX_TOL * (1.0 + (d[2] / d[0]).abs());
                assert!(
                    (q[2] - x[2]).abs() <= allowed,
                    "off the side view: {x:?} against {q:?}"
                );
                let (q, d) = bernstein(&PLAN_Q, at_x(&PLAN_Q, x[0]));
                let allowed = 2.0 * APPROX_TOL * (1.0 + (d[1] / d[0]).abs());
                assert!(
                    (q[1] - x[1]).abs() <= allowed,
                    "off the plan view: {x:?} against {q:?}"
                );
            }
        }
        // The overlap of the two runs is seen: from the plan view's start to the side view's end.
        assert!(
            (lo - PLAN_Q[0][0]).abs() <= 2.0 * APPROX_TOL
                && (hi - SIDE_Q[2][0]).abs() <= 2.0 * APPROX_TOL,
            "from {lo} to {hi}"
        );
    }

    #[test]
    fn parallel_directions_are_refused_and_views_apart_are_empty() {
        let (a, b) = (side(), plan(0.0));
        let y = [0.0, 1.0, 0.0];
        assert_eq!(two_view(&a, y, &b, [0.0, -2.0, 0.0]), Err(Error::Argument));
        assert_eq!(two_view(&a, y, &b, [0.0; 3]), Err(Error::Argument));
        assert_eq!(
            two_view(&a, y, &b, [f64::NAN, 0.0, 1.0]),
            Err(Error::Argument)
        );
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(
            two_view(&line, y, &b, [0.0, 0.0, 1.0]),
            Err(Error::Argument)
        );
        // The plan view moved to x in [20, 35], past the side view's [0, 9].
        assert_eq!(
            two_view(&a, y, &plan(23.0), [0.0, 0.0, 1.0]),
            Err(Error::Empty)
        );
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
