//! Free curves: a B-spline from the control points a designer places, one of its control points
//! moved, the curve under an affine map, the curve seen in two views, its curvature comb, the
//! blend from one curve's end to another's, circles and arcs, a curve extended at an end, two
//! curves joined end to end, the point of a curve nearest a given point, and a planar curve
//! offset in its plane ([`offset_planar`]: the kernel's trimmed offset, its distance measured by
//! [`solidcore::query::deviation::distance_deviation`]).
//! The curve is the record's ([`Curve`]); the knots, the map's carrying of the control points,
//! the views' meeting, the curvature, the blend, the circles, the extension and the nearest point
//! are the kernel's ([`solidcore::nurbs::knots::clamped_uniform`],
//! [`solidcore::nurbs::crv_basic::Curve::transform`],
//! [`solidcore::intersect::views::curves_from_views`],
//! [`solidcore::query::continuity::curve_comb`], [`solidcore::curve::blend::blend_curve`],
//! [`solidcore::curve::ellipse::Ellipse::arc`],
//! [`solidcore::curve::ellipse::Ellipse::circle_from_3_points`],
//! [`solidcore::nurbs::crv_basic::Curve::extend_by_dist`],
//! [`solidcore::nurbs::b_spline_curve::BSplineCurve::join_with`],
//! [`solidcore::curve::curve::Curve::drop_point`]). Ops checks the inputs and
//! converts.

use solidcore::curve::arc::ArcDefinition;
use solidcore::curve::blend::{self as kblend, BlendEnd};
use solidcore::curve::curve::Curve as KCurve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::from_parameters::{from_parameters, record_parameter, EdgeCurve};
use solidcore::geom::certified::Verdict;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::intersect::views::{curves_from_views, views_do_not_meet};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_adv::p1::{Continuity, CurveEnd};
use solidcore::nurbs::crv_geom::ToleranceKind;
use solidcore::nurbs::geom::Interval;
use solidcore::nurbs::knots::{clamped_uniform, MAX_DEGREE};
use solidcore::nurbs::math::{MatrixType, RealMatrix};
use solidcore::query::continuity;
use solidcore::{Point3, SolidError, Vec3};
use solidcore_record::brep::{check_curve, Budget, Curve, Invalid, Natural};

use crate::body::REGULAR_RATIO;
use crate::continuity::{tooth, CombTooth, MAX_COMB_TEETH, MIN_COMB_TEETH};
use crate::curve_knots::{net_of, nurb_of, record_of};
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

/// The full circle about `centre` of `radius` in the plane square to `normal`, starting at
/// `centre + radius * x_dir` (made unit) and running anticlockwise about `normal`: the kernel's
/// circle of that arc definition ([`Ellipse::arc`] of an [`ArcDefinition`] from 0 to a full
/// turn), proved on its plane and sphere ([`solidcore::curve::from_parameters::record_of`]), as
/// its exact rational quadratic on `[0, 1]`, closed (its last control point its first). `normal`
/// and `x_dir` need not be unit vectors; `x_dir` is square to `normal`, as the kernel's
/// definition takes it (it is not projected into the plane).
///
/// # Errors
/// [`Error::Argument`] for a number not finite or past the coordinate bound, a radius not above
/// the kernel's smallest arc ([`solidcore::curve::arc::MIN_ARC`]), a `normal` or `x_dir` of no
/// length, or an `x_dir` not square to `normal` (to the kernel's
/// [`solidcore::curve::arc::SQUARE_TOL`]); [`Error::Record`] for a circle the record cannot
/// hold; the kernel's status otherwise.
pub fn circle(centre: [f64; 3], normal: [f64; 3], x_dir: [f64; 3], radius: f64) -> Result<Curve> {
    if !coord_ok(&centre) || !coord_ok(&normal) || !coord_ok(&x_dir) || !coord_ok(&[radius]) {
        return Err(Error::Argument);
    }
    let definition = ArcDefinition {
        centre: point(centre),
        axis: point(normal),
        reference: point(x_dir),
        radius,
        start: 0.0,
        end: std::f64::consts::TAU,
    };
    let made = run("curve::circle", |k| match Ellipse::arc(k, &definition) {
        Ok(circle) => exact_record(k, &KCurve::from(circle)),
        Err(SolidError::InvalidInput { .. }) => Ok(Err(Error::Argument)),
        Err(e) => Err(e),
    })??;
    on_unit(&made)
}

/// The arc about `centre`, right-handed about `normal`, from `start` to the direction of `end`
/// (the kernel's [`ArcDefinition::from_points`]: its radius `start`'s distance from the axis, its
/// sweep the anticlockwise angle to `end`'s direction in `(0, 2 pi]`, a full turn where `end` is in
/// `start`'s direction; `start` and `end` taken into the plane through `centre` square to
/// `normal`), as [`circle`] gives it: the exact rational quadratic on `[0, 1]`, from the start.
///
/// # Errors
/// [`Error::Argument`] for a number not finite or past the coordinate bound, a `normal` of no
/// length, or a start or end on the axis; [`Error::Record`] for an arc the record cannot hold;
/// the kernel's status otherwise.
pub fn arc_about(
    centre: [f64; 3],
    normal: [f64; 3],
    start: [f64; 3],
    end: [f64; 3],
) -> Result<Curve> {
    if ![centre, normal, start, end].iter().all(|p| coord_ok(p)) {
        return Err(Error::Argument);
    }
    let made = run("curve::arc_about", |k| {
        let definition = match ArcDefinition::from_points(
            point(centre),
            point(normal),
            point(start),
            point(end),
        ) {
            Ok(d) => d,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
        match Ellipse::arc(k, &definition) {
            Ok(arc) => exact_record(k, &KCurve::from(arc)),
            Err(SolidError::InvalidInput { .. }) => Ok(Err(Error::Argument)),
            Err(e) => Err(e),
        }
    })??;
    on_unit(&made)
}

/// The full circle through `points`, starting at `points[0]` and running from it through
/// `points[1]` to `points[2]` ([`Ellipse::circle_from_3_points`], closed), as [`circle`] gives
/// it: the exact rational quadratic on `[0, 1]`, closed.
///
/// # Errors
/// [`Error::Argument`] for a coordinate not finite or past the coordinate bound, two points
/// that coincide, or three on one line (the kernel's solve for the centre singular or nearly
/// so: any geometry failure of [`Ellipse::circle_from_3_points`], whose failures are all of its
/// input);
/// [`Error::Record`] for a circle the record cannot hold; the kernel's status otherwise.
pub fn circle_through(points: [[f64; 3]; 3]) -> Result<Curve> {
    through(points, true)
}

/// The arc from `points[0]` through `points[1]` to `points[2]`
/// ([`Ellipse::circle_from_3_points`], open), as [`circle`] gives it: the exact rational
/// quadratic on `[0, 1]`, from `points[0]` at 0 to `points[2]` at 1.
///
/// # Errors
/// As [`circle_through`].
pub fn arc_through(points: [[f64; 3]; 3]) -> Result<Curve> {
    through(points, false)
}

/// [`circle_through`] (`closed`) and [`arc_through`].
fn through(points: [[f64; 3]; 3], closed: bool) -> Result<Curve> {
    let [a, b, c] = points;
    if !points.iter().all(|p| coord_ok(p)) || a == b || b == c || c == a {
        return Err(Error::Argument);
    }
    let made = run(
        "curve::circle_through",
        |k| match Ellipse::circle_from_3_points(k, point(a), point(b), point(c), 3, closed) {
            Ok(circle) => exact_record(k, &KCurve::from(circle)),
            // every geometry failure of the routine is its input's: points on one line (a
            // centre solve that is singular or nearly so) or on one another
            Err(e) if e.geom_failure().is_some() => Ok(Err(Error::Argument)),
            Err(e) => Err(e),
        },
    )??;
    on_unit(&made)
}

/// The kernel's point (or vector) of `p`.
fn point([x, y, z]: [f64; 3]) -> Point3 {
    Point3::new(x, y, z)
}

/// The record of the kernel's circle or arc `conic`: its NURBS form, proved on the conic's
/// plane and sphere ([`solidcore::curve::from_parameters::record_of`]), over its whole
/// interval.
fn exact_record(k: &Kernel, conic: &KCurve) -> solidcore::Result<Result<Curve>> {
    let made = solidcore::curve::from_parameters::record_of(k, conic, &conic.natural_interval())?;
    // a conic's record is its label beside its NURBS form; a B-spline's is the spline itself
    let nurbs = made.nurbs.unwrap_or(made.curve);
    Ok(match solidcore_convert::record_curve(nurbs) {
        Ok(curve) => Ok(curve),
        Err(Invalid(why)) => Err(Error::Record(why.to_owned())),
    })
}

/// `curve` (a clamped spline or rational spline) with its knots mapped onto `[0, 1]`
/// ([`solidcore::nurbs::crv_basic::Curve::reparam_to_interval`]): the same shape.
fn on_unit(curve: &Curve) -> Result<Curve> {
    let mut nurb = net_of(curve)?;
    run("curve::on_unit", |_| {
        nurb.reparam_to_interval(Interval { ul: 0.0, ur: 1.0 })?;
        Ok(record_of(&nurb))
    })?
}

/// Which end of a curve [`extend_curve`] grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveEndAt {
    /// Back from its start.
    Start,
    /// On from its end.
    End,
    /// Both: the start first, then the end, each by the length.
    Both,
}

/// How [`extend_curve`] continues a curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurveExtension {
    /// The end span's own polynomial (or rational) continued, with no knot left at the old
    /// end: the curve's derivatives of every order carry on across it (the kernel's
    /// [`Continuity::Cmax`]).
    Natural,
    /// A straight piece along the end tangent, joined to the curve as one spline of its
    /// degree (G1; the kernel's [`Continuity::G1`]).
    Linear,
    /// The same knots and control points, the control polygon stretched along itself from the
    /// other end so each point keeps its share of it (the kernel's
    /// [`solidcore::nurbs::crv_basic::Curve::extend_keeping_proportion`]): the curve grows by the
    /// length to the kernel's stop, its shape slides along the polygon.
    Proportion,
}

/// `curve` made longer by `length` (mm, of arc length) at `at`, continued as `how` says
/// ([`solidcore::nurbs::crv_basic::Curve::extend_by_dist`]): the curve over its old domain is
/// the curve as it was, the extension runs on past it (below its start, above its end). A
/// linear extension is `length` long to the rounding; a natural one to the kernel's stop
/// (its secant steps end within 3 % of `length`, or after five). A clamped spline or rational
/// spline, open.
///
/// # Errors
/// [`Error::Argument`] for a `length` not finite and above 0, a curve that is not a valid
/// clamped spline or rational spline, a closed one (a periodic curve, or one whose ends meet
/// to the kernel's model tolerance), or an extension the record cannot hold; the kernel's
/// status otherwise (a natural extension of a rational curve whose weights leave the
/// kernel's range).
pub fn extend_curve(
    curve: &Curve,
    at: CurveEndAt,
    length: f64,
    how: CurveExtension,
) -> Result<Curve> {
    if !(length.is_finite() && length > 0.0) {
        return Err(Error::Argument);
    }
    let nurb = nurb_of(curve)?;
    if nurb.is_closed() {
        return Err(Error::Argument);
    }
    let ends: &[CurveEnd] = match at {
        CurveEndAt::Start => &[CurveEnd::Start],
        CurveEndAt::End => &[CurveEnd::End],
        CurveEndAt::Both => &[CurveEnd::Start, CurveEnd::End],
    };
    let continuity = match how {
        CurveExtension::Natural => Continuity::Cmax,
        CurveExtension::Linear => Continuity::G1,
        CurveExtension::Proportion => {
            return run("curve::extend_curve", |_| {
                let mut extended = nurb;
                for &end in ends {
                    extended = match extended.extend_keeping_proportion(length, end) {
                        Ok((curve, _)) => curve,
                        Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
                        Err(e) => return Err(e),
                    };
                }
                Ok(record_of(&extended))
            })?;
        }
    };
    run("curve::extend_curve", |_| {
        let mut extended = nurb;
        for &end in ends {
            extended = extended.extend_by_dist(length, ToleranceKind::Absolute, end, continuity)?;
        }
        Ok(record_of(&extended))
    })?
}

/// `a` and `b` joined end to end into one spline (the kernel's `JoinWith`): the end of `a` that
/// meets an end of `b` within `tolerance` (mm) is joined to it, and the joined curve runs from
/// `a`'s other end to `b`'s other end. Both are brought to the higher degree and `b` follows on
/// in parameter, so the joined curve is both curves exactly, with a knot of full multiplicity at
/// the joint: it is as smooth there as the two curves meet (G1 where their tangents line up, as
/// a half section and its mirror do). Two lines that continue each other become one line. The
/// kernel tries `a`'s end first, then its start, each against `b`'s start, then its end.
/// A clamped spline or rational spline each.
///
/// # Errors
/// [`Error::Argument`] for a `tolerance` not finite and above 0, a curve that is not a valid
/// clamped spline or rational spline, or a joined curve the record cannot hold (a spline joined
/// to a rational spline, past the record's size); [`Error::Unsupported`] when no end of `a`
/// meets an end of `b` within `tolerance` (or the kernel could not join the two there); the
/// kernel's unported and cancelled statuses.
pub fn join_curves(a: &Curve, b: &Curve, tolerance: f64) -> Result<Curve> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(Error::Argument);
    }
    let (na, nb) = (nurb_of(a)?, nurb_of(b)?);
    run("curve::join_curves", |k| {
        for (a_end, b_end) in [(true, false), (true, true), (false, false), (false, true)] {
            let mut first = BSplineCurve::from_nurb(na.clone(), 3, false);
            let mut second = BSplineCurve::from_nurb(nb.clone(), 3, false);
            match first.join_with(k, a_end, &mut second, b_end, Some(tolerance)) {
                Ok(()) => {
                    let Some(joined) = first.gw_nurb() else {
                        return Ok(Err(Error::Unsupported(
                            "the joined curve has no NURBS form",
                        )));
                    };
                    return Ok(record_of(joined));
                }
                Err(e) if e.is_unported() || e.is_cancelled() => return Err(e),
                Err(_) => {}
            }
        }
        Ok(Err(Error::Unsupported(
            "no end of one curve meets an end of the other within the tolerance",
        )))
    })?
}

/// How far one curve strays from another curve or from a sheet ([`curve_deviation`],
/// [`curve_surface_deviation`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Deviation {
    /// The largest distance found at the stations.
    pub max: f64,
    /// Where on the first curve it is.
    pub at: [f64; 3],
    /// Its foot on the other curve or the sheet.
    pub to: [f64; 3],
    /// Each station's point and its foot, in order along the first curve: a comb.
    pub comb: Vec<([f64; 3], [f64; 3])>,
    /// The kernel's certified bound of the largest distance, where it applies (two
    /// non-rational splines on one domain); `None` elsewhere.
    pub bound: Option<f64>,
}

fn deviation_of(d: solidcore::query::deviation::Deviation) -> Deviation {
    let a = |p: Point3| [p.x, p.y, p.z];
    Deviation {
        max: d.max,
        at: a(d.at),
        to: a(d.to),
        comb: d.comb.into_iter().map(|(p, q)| (a(p), a(q))).collect(),
        bound: d.bound,
    }
}

/// How far curve `a` strays from curve `b`: `samples` stations at even steps of `a`'s range,
/// each dropped on `b` (the kernel's [`solidcore::query::deviation::curve_deviation`]), the
/// largest distance found with where it is, the station pairs, and the certified bound where it
/// applies. The curves are splines, rational splines or polylines, each over its whole range.
///
/// # Errors
/// [`Error::Argument`] for another kind of curve, one the record refuses, or a station count
/// outside 2..=10000; the kernel's status otherwise (no station dropping onto `b`).
pub fn curve_deviation(a: &Curve, b: &Curve, samples: usize) -> Result<Deviation> {
    run("curve::curve_deviation", |k| {
        let ka = match blend_input(k, a)? {
            Ok(made) => made,
            Err(e) => return Ok(Err(e)),
        };
        let kb = match blend_input(k, b)? {
            Ok(made) => made,
            Err(e) => return Ok(Err(e)),
        };
        Ok(
            match solidcore::query::deviation::curve_deviation(k, &ka.curve, &kb.curve, samples) {
                Ok(d) => Ok(deviation_of(d)),
                Err(SolidError::InvalidInput { .. }) => Err(Error::Argument),
                Err(e) => return Err(e),
            },
        )
    })?
}

/// How far curve `c` strays from the one-face `sheet`: as [`curve_deviation`], each station
/// dropped on the face's surface over its parameter box (the kernel's
/// [`solidcore::query::deviation::curve_surface_deviation`]); no certified bound.
///
/// # Errors
/// As [`curve_deviation`]; [`Error::Argument`] for a sheet that is not one face; those of
/// [`crate::Body::from_record`].
pub fn curve_surface_deviation(
    c: &Curve,
    sheet: &solidcore_record::brep::Brep,
    samples: usize,
) -> Result<Deviation> {
    let (body, _) = crate::Body::from_record(sheet, true)?;
    crate::session::read_body("curve::curve_surface_deviation", &body, |k, b| {
        let [f] = b.face_list() else {
            return Ok(Err(Error::Argument));
        };
        let face = b.face(*f)?;
        let Some(surface) = face.surface.as_ref() else {
            return Ok(Err(Error::Argument));
        };
        let kc = match blend_input(k, c)? {
            Ok(made) => made,
            Err(e) => return Ok(Err(e)),
        };
        Ok(
            match solidcore::query::deviation::curve_surface_deviation(
                k,
                &kc.curve,
                surface,
                &face.uv_domain,
                samples,
            ) {
                Ok(d) => Ok(deviation_of(d)),
                Err(SolidError::InvalidInput { .. }) => Err(Error::Argument),
                Err(e) => return Err(e),
            },
        )
    })?
}

/// The point of a free curve nearest a given point ([`closest_point`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    /// The curve's parameter there, the record's.
    pub parameter: f64,
    /// The curve's point there.
    pub point: [f64; 3],
    /// Its distance from the given point: the kernel solve's (not certified). A solution
    /// within the tolerance of an end is snapped to that end and keeps the solve's distance, so
    /// it is the distance to `point` within the tolerance.
    pub distance: f64,
}

/// The record parameter range [`closest_point`] drops over: a line's segment from its start
/// (0) to its end (1), a circle's or ellipse's one turn from its frame's x axis, any other
/// curve's own range; `None` for an unbounded range.
fn own_range(curve: &Curve) -> Option<[f64; 2]> {
    match (curve, curve.natural()) {
        (Curve::Line { .. }, _) => Some([0.0, 1.0]),
        (_, Natural::Periodic) => Some([0.0, std::f64::consts::TAU]),
        (_, Natural::Range(lo, hi)) => (lo.is_finite() && hi.is_finite()).then_some([lo, hi]),
    }
}

/// The point of `curve` nearest `p` over the record's own range (a line between its two
/// points, a circle or ellipse its whole turn, a polyline or spline its whole range): the
/// curve as the kernel's ([`from_parameters`]), the kernel's global minimising drop of `p` onto
/// it ([`KCurve::drop_point`], no guess; a solution within `tolerance` of an end snapped to
/// that end), the curve's point at the parameter it finds ([`KCurve::evaluate_point`]) and that
/// parameter as the record's
/// ([`solidcore::curve::from_parameters::record_parameter`]: radians on a circle or ellipse).
/// `None` where the drop finds no point.
///
/// # Errors
/// [`Error::Argument`] for a point not finite or past the coordinate bound, a tolerance not
/// finite and positive, or a curve the record or the kernel's constructors refuse; the kernel's
/// status otherwise (a traced intersection curve, which has no constructor yet).
pub fn closest_point(curve: &Curve, p: [f64; 3], tolerance: f64) -> Result<Option<CurvePoint>> {
    if !coord_ok(&p) || !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(Error::Argument);
    }
    if check_curve(curve, &mut Budget::input()).is_err() {
        return Err(Error::Argument);
    }
    let range = own_range(curve).ok_or(Error::Argument)?;
    let record = solidcore_convert::curve(curve).map_err(|_| Error::Argument)?;
    run("curve::closest_point", |k| {
        let made = match from_parameters(k, &record, range) {
            Ok(made) => made,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
        let dropped = match made.curve.drop_point(
            k,
            &made.interval,
            point(p),
            None,
            tolerance,
            None,
            SolverOperation::Minimize,
        ) {
            Ok(dropped) => dropped,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
        if !dropped.success {
            return Ok(Ok(None));
        }
        let at = made.curve.evaluate_point(k, dropped.parameter)?;
        Ok(Ok(Some(CurvePoint {
            parameter: record_parameter(&record, range, dropped.parameter, at),
            point: [at.x, at.y, at.z],
            distance: dropped.distance,
        })))
    })?
}

pub use solidcore::curve::stretch::Stretch;

/// `curve` with the control points `picked` stretched by a drag `delta` of point `handle` from
/// point `pivot` (the kernel's [`solidcore::curve::stretch::stretch_points`]): with
/// [`Stretch::Linear`] each moves by `delta` times the control polygon's length from the pivot to
/// it over its length from the pivot to the handle (the pivot stays, the handle moves by `delta`
/// whole); with [`Stretch::None`] each by `delta`. With `plane` (as
/// [`crate::curve_plane::plane_of`] gives it), `delta` is first taken into the plane, so a
/// planar curve stays planar. A closed periodic spline's repeated points move with their twins.
///
/// # Errors
/// [`Error::Argument`] for another kind of curve than a spline or polyline, an index past its
/// points, the handle at the pivot, no polygon length from the pivot to the handle, a number not
/// finite or a moved point past the coordinate bound.
#[allow(clippy::too_many_arguments)]
pub fn stretch_cvs(
    curve: &Curve,
    picked: &[usize],
    pivot: usize,
    handle: usize,
    delta: [f64; 3],
    how: Stretch,
    plane: Option<crate::curve_plane::CurvePlane>,
) -> Result<Curve> {
    let (Curve::Spline { points, .. } | Curve::Polyline { points }) = curve else {
        return Err(Error::Argument);
    };
    let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let given: Vec<Point3> = points.iter().map(|p| v(*p)).collect();
    let stretched = solidcore::curve::stretch::stretch_points(
        &given,
        picked,
        pivot,
        handle,
        v(delta),
        how,
        plane.map(|p| v(p.normal)),
    )
    .map_err(|_| Error::Argument)?;
    let mut out = curve.clone();
    for &i in picked {
        let to = stretched
            .get(i)
            .map(|p| [p.x, p.y, p.z])
            .ok_or(Error::Argument)?;
        if !coord_ok(&to) {
            return Err(Error::Argument);
        }
        out = move_cv(&out, i, to)?;
    }
    Ok(out)
}

#[cfg(test)]
// The tests measure the circles' radii; ops itself computes none.
#[allow(clippy::disallowed_methods)]
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

    /// The distance from `a` to `b`.
    fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f64>()
            .sqrt()
    }

    /// `c`'s degree, control point count and knots, for a rational spline.
    fn rational(c: &Curve) -> (usize, &[[f64; 4]], &[f64]) {
        let Curve::RationalSpline { knots, homogeneous } = c else {
            panic!("a rational spline: {c:?}")
        };
        (knots.len() - homogeneous.len() - 1, homogeneous, knots)
    }

    /// Every one of 257 points of `c` across `[0, 1]` is `radius` from `centre` and in the
    /// plane through it square to `normal` (a unit vector), to `tol`.
    fn on_circle(c: &Curve, centre: [f64; 3], normal: [f64; 3], radius: f64, tol: f64) {
        for i in 0..=256 {
            let p = at(c, f64::from(i) / 256.0);
            let r = dist(p, centre);
            assert!(
                (r - radius).abs() <= tol,
                "at {i}: radius {r}, not {radius}"
            );
            let off: f64 = (0..3).map(|j| (p[j] - centre[j]) * normal[j]).sum();
            assert!(off.abs() <= tol, "at {i}: {off} off the plane");
        }
    }

    #[test]
    fn a_circle_is_an_exact_closed_rational_quadratic_on_its_plane() {
        let centre = [1.0, 2.0, 3.0];
        let c = circle(centre, [0.0, 0.0, 2.0], [3.0, 0.0, 0.0], 5.0).unwrap();
        let (degree, cvs, knots) = rational(&c);
        assert_eq!(degree, 2);
        assert_eq!((knots[0], knots[knots.len() - 1]), (0.0, 1.0));
        let (first, last) = (cvs[0], cvs[cvs.len() - 1]);
        assert!(
            first.iter().zip(last).all(|(a, b)| (a - b).abs() < 1e-12),
            "closed: {first:?} and {last:?}"
        );
        on_circle(&c, centre, [0.0, 0.0, 1.0], 5.0, 1e-12 * 10.0);
        // It starts on the reference and turns anticlockwise about the normal: a quarter of
        // the way round it is on y.
        let start = at(&c, 0.0);
        assert!(dist(start, [6.0, 2.0, 3.0]) < 1e-12 * 10.0, "{start:?}");
        let quarter = at(&c, 0.25);
        assert!(quarter[1] > 2.0 + 2.5, "{quarter:?}");
    }

    #[test]
    fn a_tilted_circle_lies_in_the_plane_square_to_its_normal() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let centre = [-4.0, 7.0, 0.5];
        let c = circle(centre, [0.0, 1.0, 1.0], [1.0, 0.0, 0.0], 12.0).unwrap();
        on_circle(&c, centre, [0.0, s, s], 12.0, 1e-12 * 20.0);
        assert!(dist(at(&c, 0.0), [8.0, 7.0, 0.5]) < 1e-12 * 20.0);
    }

    #[test]
    fn a_circle_with_a_bad_radius_or_frame_is_refused() {
        let (o, z, x) = ([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        for r in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(circle(o, z, x, r), Err(Error::Argument), "{r}");
        }
        assert_eq!(circle(o, [0.0; 3], x, 1.0), Err(Error::Argument));
        assert_eq!(circle(o, z, [0.0; 3], 1.0), Err(Error::Argument));
        assert_eq!(circle(o, z, [0.0, 0.0, -3.0], 1.0), Err(Error::Argument));
        // the reference is not projected into the plane
        assert_eq!(circle(o, z, [1.0, 0.0, 1.0], 1.0), Err(Error::Argument));
        assert_eq!(
            circle([f64::NAN, 0.0, 0.0], z, x, 1.0),
            Err(Error::Argument)
        );
    }

    #[test]
    fn a_circle_through_three_points_has_their_centre_and_starts_on_the_first() {
        // (0, 0), (10, 0) and (0, 10) are on the circle about (5, 5) of radius sqrt 50.
        let picks = [[0.0, 0.0, 2.0], [10.0, 0.0, 2.0], [0.0, 10.0, 2.0]];
        let c = circle_through(picks).unwrap();
        let (degree, cvs, _) = rational(&c);
        assert_eq!(degree, 2);
        let (first, last) = (cvs[0], cvs[cvs.len() - 1]);
        assert!(
            first.iter().zip(last).all(|(a, b)| (a - b).abs() < 1e-12),
            "closed: {first:?} and {last:?}"
        );
        on_circle(&c, [5.0, 5.0, 2.0], [0.0, 0.0, 1.0], 50f64.sqrt(), 1e-11);
        assert!(dist(at(&c, 0.0), picks[0]) < 1e-11);
        assert!(dist(at(&c, 1.0), picks[0]) < 1e-11);
    }

    #[test]
    fn an_arc_through_three_points_runs_from_the_first_through_the_second_to_the_third() {
        // A half circle over the top, and three quarters round through the top and the left.
        let (east, north, west, south) = (
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [-10.0, 0.0, 0.0],
            [0.0, -10.0, 0.0],
        );
        let half = arc_through([east, north, west]).unwrap();
        assert_eq!(rational(&half).0, 2);
        on_circle(&half, [0.0; 3], [0.0, 0.0, 1.0], 10.0, 1e-11);
        assert!(dist(at(&half, 0.0), east) < 1e-11);
        assert!(dist(at(&half, 1.0), west) < 1e-11);
        // the kernel's half circle is two quarters: the joint is the top
        assert!(dist(at(&half, 0.5), north) < 1e-11, "{:?}", at(&half, 0.5));
        assert!((0..=64).all(|i| at(&half, f64::from(i) / 64.0)[1] > -1e-11));

        let most = arc_through([east, north, south]).unwrap();
        on_circle(&most, [0.0; 3], [0.0, 0.0, 1.0], 10.0, 1e-11);
        assert!(dist(at(&most, 0.0), east) < 1e-11);
        assert!(dist(at(&most, 1.0), south) < 1e-11);
        let samples: Vec<[f64; 3]> = (0..=256).map(|i| at(&most, f64::from(i) / 256.0)).collect();
        assert!(
            samples.iter().any(|p| dist(*p, west) < 0.5),
            "through the west"
        );
        assert!(
            samples.iter().all(|p| p[0] < 1e-11 || p[1] > -1e-11),
            "not the south-east"
        );
    }

    #[test]
    fn three_points_on_a_line_or_two_on_one_another_make_no_circle() {
        let line = [[0.0; 3], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]];
        assert_eq!(circle_through(line), Err(Error::Argument));
        assert_eq!(arc_through(line), Err(Error::Argument));
        let twice = [[0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]];
        assert_eq!(circle_through(twice), Err(Error::Argument));
        assert_eq!(arc_through(twice), Err(Error::Argument));
        let nan = [[f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        assert_eq!(circle_through(nan), Err(Error::Argument));
    }

    /// A cubic of six control points, two interior knots, bending both ways.
    const WAVE: [[f64; 3]; 6] = [
        [0.0, 0.0, 0.0],
        [1.0, 2.0, 0.0],
        [2.0, -1.0, 0.0],
        [3.0, 3.0, 0.0],
        [4.0, 0.0, 1.0],
        [5.0, 1.0, 0.0],
    ];

    /// The domain of a clamped spline: its first and last knot.
    fn domain(c: &Curve) -> [f64; 2] {
        let (Curve::Spline { knots, .. } | Curve::RationalSpline { knots, .. }) = c else {
            panic!("a spline: {c:?}")
        };
        [knots[0], knots[knots.len() - 1]]
    }

    /// The kernel's arc length bracket (control polygon over chord, halved to a depth of 20)
    /// is asked for [`MEASURE`] of the piece's size: what that depth reaches on these pieces.
    /// A straight piece is measured exactly (the kernel takes it as a line).
    const MEASURE: f64 = 1e-6;

    /// The arc length of `c` from `a` to `b`, by the kernel, within [`MEASURE`] of the piece's
    /// box diagonal.
    fn length(c: &Curve, a: f64, b: f64) -> solidcore::Result<f64> {
        net_of(c)
            .unwrap()
            .arc_length(a, b, MEASURE, ToleranceKind::Relative)
    }

    /// The point, first and second derivative of `c` at `t`, from the side `side`.
    fn jet(c: &Curve, t: f64, side: solidcore::nurbs::knots::Side) -> Vec<Vec3> {
        net_of(c).unwrap().derivs(t, side, 2).unwrap()
    }

    /// `a` and `b` are the same curve over `[0, 1]`: the same point at 201 parameters.
    fn same_on_unit(a: &Curve, b: &Curve, tol: f64) {
        for i in 0..=200 {
            let t = f64::from(i) / 200.0;
            let (p, q) = (at(a, t), at(b, t));
            assert!(dist(p, q) <= tol, "at {t}: {p:?} and {q:?}");
        }
    }

    #[test]
    fn a_linear_extension_is_straight_along_the_end_tangent_and_as_long_as_asked() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let longer = extend_curve(&c, CurveEndAt::End, 2.0, CurveExtension::Linear).unwrap();
        // the old curve is the old domain's: only the extension adds length
        same_on_unit(&c, &longer, 1e-12);
        let [lo, hi] = domain(&longer);
        assert_eq!(lo, 0.0);
        assert!(hi > 1.0, "{hi}");
        let grown = length(&longer, 1.0, hi);
        assert!(
            matches!(grown, Ok(l) if (l - 2.0).abs() < 1e-9),
            "{grown:?}"
        );
        // Straight on from the end along P5 - P4 = (1, 1, -1) / sqrt 3, the clamped end's tangent.
        let end = WAVE[5];
        let s = 1.0 / 3f64.sqrt();
        for i in 1..=20 {
            let t = 1.0 + (hi - 1.0) * f64::from(i) / 20.0;
            let p = at(&longer, t);
            let d = [p[0] - end[0], p[1] - end[1], p[2] - end[2]];
            let along = (d[0] + d[1] - d[2]) * s;
            let off = dist(d, [along * s, along * s, -along * s]);
            assert!(along > 0.0 && off < 1e-12, "at {t}: {p:?}");
        }
        let far = at(&longer, hi);
        assert!((dist(far, end) - 2.0).abs() < 1e-12, "{far:?}");
    }

    #[test]
    fn a_natural_extension_keeps_the_ends_curvature() {
        use solidcore::nurbs::knots::Side;
        let c = cv_curve(&WAVE, 3).unwrap();
        let longer = extend_curve(&c, CurveEndAt::Start, 1.5, CurveExtension::Natural).unwrap();
        same_on_unit(&c, &longer, 1e-12);
        let [lo, hi] = domain(&longer);
        assert!(lo < 0.0 && hi == 1.0, "{lo} {hi}");
        // the kernel's stop: within 3 % of the length asked; the measure within MEASURE of
        // the piece's diagonal, at most its length
        let grown = length(&longer, lo, 0.0);
        assert!(
            matches!(grown, Ok(l) if (l - 1.5).abs() <= 0.03 * 1.5 + MEASURE * 1.6),
            "{grown:?}"
        );
        // The start span's polynomial carries on: the point and its first two derivatives at
        // the old start are the same from the extension's side as on the curve.
        let (was, now) = (jet(&c, 0.0, Side::Right), jet(&longer, 0.0, Side::Left));
        for (a, b) in was.iter().zip(&now) {
            let gap = dist([a.x, a.y, a.z], [b.x, b.y, b.z]);
            assert!(gap < 1e-9 * (1.0 + a.length()), "{a:?} and {b:?}");
        }
    }

    #[test]
    fn both_ends_grow_and_the_curve_between_stays() {
        let c = cv_curve(&WAVE, 3).unwrap();
        let longer = extend_curve(&c, CurveEndAt::Both, 1.0, CurveExtension::Linear).unwrap();
        same_on_unit(&c, &longer, 1e-12);
        let [lo, hi] = domain(&longer);
        assert!(lo < 0.0 && hi > 1.0, "{lo} {hi}");
        for (a, b) in [(lo, 0.0), (1.0, hi)] {
            let grown = length(&longer, a, b);
            assert!(
                matches!(grown, Ok(l) if (l - 1.0).abs() < 1e-9),
                "{grown:?}"
            );
        }
        // a rational arc keeps its circle under the extension
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        let more = extend_curve(&arc, CurveEndAt::End, 3.0, CurveExtension::Natural).unwrap();
        assert!(matches!(more, Curve::RationalSpline { .. }));
        same_on_unit(&arc, &more, 1e-12);
        let [_, hi] = domain(&more);
        for i in 0..=20 {
            let p = at(&more, 1.0 + (hi - 1.0) * f64::from(i) / 20.0);
            assert!(
                (dist(p, [0.0; 3]) - 10.0).abs() < 1e-9,
                "{p:?} off the circle"
            );
        }
    }

    #[test]
    fn a_closed_curve_or_a_bad_length_is_not_extended() {
        let c = cv_curve(&WAVE, 3).unwrap();
        for l in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                extend_curve(&c, CurveEndAt::End, l, CurveExtension::Linear),
                Err(Error::Argument),
                "{l}"
            );
        }
        let periodic = crate::curve_fit::closed_cv_curve(&SQUARE, 3).unwrap();
        assert_eq!(
            extend_curve(&periodic, CurveEndAt::End, 1.0, CurveExtension::Natural),
            Err(Error::Argument)
        );
        let mut ring = SQUARE.to_vec();
        ring.push(SQUARE[0]);
        let clamped_ring = cv_curve(&ring, 3).unwrap();
        assert_eq!(
            extend_curve(
                &clamped_ring,
                CurveEndAt::Start,
                1.0,
                CurveExtension::Linear
            ),
            Err(Error::Argument)
        );
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0; 3],
        };
        assert_eq!(
            extend_curve(&line, CurveEndAt::End, 1.0, CurveExtension::Linear),
            Err(Error::Argument)
        );
    }
}

/// The curves where the tube of `radius` round `curve` meets the one-face `sheet`: a chamfer's
/// two boundaries on the sheet, each lying `radius` from the curve in space. The kernel's pipe of
/// that radius swept along the curve
/// ([`solidcore::sweep::api_brep::create_pipe_sweep`], uncapped), each of its walls intersected
/// with the sheet's surface over the two faces' parameter boxes (the kernel's
/// `Surface::global_surface_intersect`, traced to `tolerance`; a trimmed sheet is met over its
/// whole box), and every curve found measured from the curve
/// ([`solidcore::query::deviation::distance_deviation`] at [`OFFSET_STATIONS`] stations).
///
/// # Errors
/// [`Error::Argument`] for a radius or a tolerance not finite and positive, a radius past the
/// coordinate bound, a traced intersection curve, a curve the record or the kernel's
/// constructors refuse, or a sheet that is not one face with a surface; [`Error::Unsupported`]
/// for a curve whose kernel form runs past its record's range (a line); [`Error::Empty`] when
/// the tube does not meet the sheet; [`Error::OffsetPastTolerance`] for a curve whose measured
/// distance from `curve` strays past `tolerance`; [`Error::Record`] for a curve the record cannot
/// hold; the kernel's refusals otherwise (a radius that reaches the centre of a bend: the pipe's
/// wall would fold through itself); those of [`crate::Body::from_record`].
pub fn tube_on_surface(
    curve: &Curve,
    sheet: &solidcore_record::brep::Brep,
    radius: f64,
    tolerance: f64,
) -> Result<Vec<Curve>> {
    use solidcore::query::deviation::distance_deviation;
    use solidcore::sweep::api_brep::create_pipe_sweep;
    let positive = |x: f64| x.is_finite() && x > 0.0;
    if !positive(radius) || !coord_ok(&[radius]) || !positive(tolerance) {
        return Err(Error::Argument);
    }
    if matches!(curve, Curve::SurfaceIntersection { .. })
        || check_curve(curve, &mut Budget::input()).is_err()
    {
        return Err(Error::Argument);
    }
    let range = own_range(curve).ok_or(Error::Argument)?;
    let record = solidcore_convert::curve(curve).map_err(|_| Error::Argument)?;
    let (body, _) = crate::Body::from_record(sheet, true)?;
    crate::session::read_body("curve::tube_on_surface", &body, |k, b| {
        let [f] = b.face_list() else {
            return Ok(Err(Error::Argument));
        };
        let face = b.face(*f)?;
        // copies, held by no face: the two surfaces belong to different bodies
        let Some(surface) = face.surface.clone() else {
            return Ok(Err(Error::Argument));
        };
        let domain = face.uv_domain;
        let made = match from_parameters(k, &record, range) {
            Ok(made) => made,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
        let natural = made.curve.natural_interval();
        if natural.min() != made.interval.min() || natural.max() != made.interval.max() {
            return Ok(Err(Error::Unsupported(
                "a curve whose kernel form runs past its record's interval (a line: give it as a spline)",
            )));
        }
        let path = made.curve;
        let pipe = match create_pipe_sweep(k, radius, Some(&path), false) {
            Ok(pipe) => pipe,
            Err(e) => return Ok(Err(crate::error::status(e))),
        };
        let mut found = Vec::new();
        for &wall in pipe.face_list() {
            let wall_face = pipe.face(wall)?;
            let Some(wall_surface) = wall_face.surface.clone() else {
                continue;
            };
            let met = match wall_surface.global_surface_intersect(
                k,
                &wall_face.uv_domain,
                &surface,
                &domain,
                [true, true],
                Some(tolerance),
                None,
            ) {
                Ok(met) => met,
                Err(e) => return Ok(Err(crate::error::status(e))),
            };
            found.extend(met.curves_3d);
        }
        if found.is_empty() {
            return Ok(Err(Error::Empty));
        }
        let mut out = Vec::new();
        for c in &found {
            let measured = distance_deviation(k, c, &path, radius, OFFSET_STATIONS)?;
            if measured.worst > tolerance {
                return Ok(Err(Error::OffsetPastTolerance {
                    tolerance,
                    worst: measured.worst,
                }));
            }
            let held = solidcore::curve::from_parameters::record_of(k, c, &c.natural_interval())?;
            match solidcore_convert::record_curve(held.curve) {
                Ok(r) => out.push(r),
                Err(Invalid(why)) => return Ok(Err(Error::Record(why.to_owned()))),
            }
        }
        Ok(Ok(out))
    })?
}

/// Which side of a planar curve an offset lies on, looking down the plane's normal
/// ([`PlanarOffset::normal`]): left of the curve's direction of travel, or right of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetSide {
    /// To the left: the side `normal x tangent` points to.
    Left,
    /// To the right.
    Right,
}

/// The stations along each piece of an offset at which its distance from the curve is measured
/// ([`offset_planar`]).
pub const OFFSET_STATIONS: usize = 129;

/// A planar curve's offset ([`offset_planar`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PlanarOffset {
    /// The offset's pieces, in order along the curve.
    pub curves: Vec<Curve>,
    /// The normal of the curve's plane the side is read about (the kernel's
    /// [`crate::curve_plane::plane_of`]).
    pub normal: [f64; 3],
    /// The largest difference found, at [`OFFSET_STATIONS`] stations along each piece, between a
    /// station's distance from the curve and the distance asked: measured, not proved between
    /// the stations.
    pub deviation: f64,
}

/// `curve` offset `distance` in its own plane to `side` (the kernel's trimmed offset of a planar
/// curve, `CompositeCurve::create_trimmed_offsets`): each point moved `distance` square to the
/// curve in the plane, where the offset would cross itself (past a bend tighter than the
/// distance) the loops trimmed off, the pieces approximated by splines within `tolerance`. Each
/// piece is then measured ([`solidcore::query::deviation::distance_deviation`] at
/// [`OFFSET_STATIONS`] stations) and the offset refused when a station lies farther than
/// `tolerance` from `distance`. The plane is the one the kernel proves the curve lies in
/// ([`crate::curve_plane::plane_of`]); a line is in no one plane and is refused.
///
/// # Errors
/// [`Error::Argument`] for a distance or a tolerance not finite and positive, a distance past the
/// coordinate bound, a traced intersection curve or a curve the record or the kernel's
/// constructors refuse; [`Error::Unsupported`] for a curve in no one plane;
/// [`Error::Empty`] when no offset is left; [`Error::OffsetPastTolerance`] for an offset whose
/// measured distance strays past `tolerance`; [`Error::Record`] for a piece the record cannot
/// hold; the kernel's refusals otherwise.
pub fn offset_planar(
    curve: &Curve,
    distance: f64,
    side: OffsetSide,
    tolerance: f64,
) -> Result<PlanarOffset> {
    use solidcore::curve::composite_curve::{
        CompositeCurve, OffsetCornerType, OffsetDirection, OffsetParams,
    };
    use solidcore::query::deviation::distance_deviation;
    let positive = |x: f64| x.is_finite() && x > 0.0;
    if !positive(distance) || !coord_ok(&[distance]) || !positive(tolerance) {
        return Err(Error::Argument);
    }
    if matches!(curve, Curve::SurfaceIntersection { .. })
        || check_curve(curve, &mut Budget::input()).is_err()
    {
        return Err(Error::Argument);
    }
    let Some(plane) = crate::curve_plane::plane_of(std::slice::from_ref(curve))? else {
        return Err(Error::Unsupported(
            "the curve lies in no one plane: a planar offset needs a curve that spans a plane",
        ));
    };
    let range = own_range(curve).ok_or(Error::Argument)?;
    let record = solidcore_convert::curve(curve).map_err(|_| Error::Argument)?;
    let [nx, ny, nz] = plane.normal;
    let direction = match side {
        OffsetSide::Left => OffsetDirection::LeftHandSide,
        OffsetSide::Right => OffsetDirection::RightHandSide,
    };
    let params = OffsetParams {
        xsect_tol_3d: tolerance,
        approx_tol_3d: tolerance,
        plane_normal: Vec3::new(nx, ny, nz),
        corner: OffsetCornerType::FilletCorner,
        trim_results: true,
        offset_distance: distance,
        match_parameterization: false,
    };
    run("curve::offset_planar", |k| {
        let made = match from_parameters(k, &record, range) {
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
        let source = made.curve;
        let composite = CompositeCurve::with_curves(3, vec![source.clone()], false, None, None)?;
        let pieces = composite.create_trimmed_offsets(k, direction, &params)?;
        if pieces.is_empty() {
            return Ok(Err(Error::Empty));
        }
        let mut worst = 0.0_f64;
        let mut curves = Vec::with_capacity(pieces.len());
        for piece in &pieces {
            let measured = distance_deviation(k, &piece.curve, &source, distance, OFFSET_STATIONS)?;
            worst = worst.max(measured.worst);
            let held = solidcore::curve::from_parameters::record_of(
                k,
                &piece.curve,
                &piece.curve.natural_interval(),
            )?;
            match solidcore_convert::record_curve(held.curve) {
                Ok(c) => curves.push(c),
                Err(Invalid(why)) => return Ok(Err(Error::Record(why.to_owned()))),
            }
        }
        if worst > tolerance {
            return Ok(Err(Error::OffsetPastTolerance { tolerance, worst }));
        }
        Ok(Ok(PlanarOffset {
            curves,
            normal: plane.normal,
            deviation: worst,
        }))
    })?
}
