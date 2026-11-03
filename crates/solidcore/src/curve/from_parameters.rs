//! A curve made from a record's numbers and an interval: the one constructor that turns the
//! plain data of an edge's curve (a line's ends, a conic's frame and radii, a spline's knots and
//! points) and the interval it is wanted over into the kernel's [`Curve`] and that interval in
//! the kernel's parameter, and a pcurve's numbers into the kernel's UV trim over its edge.
//! Beside [`crate::surface::from_parameters`], for every layer that holds records (the
//! conversion of a body's edges).
//!
//! Not upstream: SolidCraft's flat-array import (its edge curves and UV trims), moved into the
//! kernel so the record and conversion layers compute no geometry (docs/layers.md §1). Every
//! geometric step of it is here: a line's direction, a conic's interval from the record's
//! radians to the kernel's degrees (snapped onto a quarter turn within rounding, and moved by
//! whole turns into one turn either way), a polyline's knots, and a trim's knots mapped onto
//! its edge's interval.
//!
//! **Exact classes at bound 0** (owner rule 13564): a circle or ellipse is made as that conic,
//! and is returned only once its NURBS form is proved to lie on the plane and the quadric
//! (sphere or elliptic cylinder) of the record's own numbers ([`curve_on_quadrics`], to the
//! rounding of checking it). A conic given with its NURBS form keeps that NURBS, proved on the
//! record's conic the same way. A line is the record's two points; a spline its own definition.
//!
//! The record's numbers are taken as they are: a frame's axes are the record's (not rebuilt by
//! cross products), a rational spline's homogeneous points are copied (not divided by their
//! weights and multiplied again).
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::curve::curve::Curve;
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::curve::line::Line;
use crate::error::{Result, SolidError};
use crate::eval::{Curve as Record, Curve2 as Record2, Frame};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{CPoint, NOZ};
use crate::surface::implicit::{curve_on_quadrics, QuadricEquation};

const OP: &str = "curve::from_parameters::from_parameters";
const OP_TRIM: &str = "curve::from_parameters::uv_trim";

/// A conic's angle within this many degrees of a quarter turn is that quarter turn
/// (`SnapSMLibAngularBoundaryDeg`): the kernel's conics and their faces meet on the quarter
/// grid, and a record's radians converted to degrees round off it.
const QUARTER_SNAP_DEG: f64 = 1.0e-9;

/// The largest edge or vertex tolerance a body may carry, mm (the flat B-rep import's limit).
pub const MAX_TOLERANCE: f64 = 1.0;

/// The kernel's zone tolerance of an entity whose intersection tolerance is `tolerance`: half
/// of it (`XSectTol3d / 2`, as `SmBrepData` holds it).
#[must_use]
pub fn zone_tolerance(tolerance: f64) -> f64 {
    tolerance / 2.0
}

/// The kernel's approximation tolerance of a body whose intersection tolerance is
/// `tolerance`: a quarter of it.
#[must_use]
pub fn approx_tolerance(tolerance: f64) -> f64 {
    tolerance / 4.0
}

/// An edge's curve in the kernel and its interval in the kernel's parameter.
#[derive(Debug, Clone)]
pub struct EdgeCurve {
    /// The kernel's curve.
    pub curve: Curve,
    /// The edge's interval in the kernel's parameter (degrees on a circle or ellipse).
    pub interval: Extent1d,
}

/// The curve the record's numbers `record` describe, over `interval` (in the record's
/// parameter: radians on a circle or ellipse), and that interval in the kernel's parameter.
///
/// - **Line:** through its start along `end - start` (unbounded; the interval as given).
/// - **Circle, ellipse:** on the record's frame, the interval in degrees, a whole number of
///   turns moved into `[-360, 360]` (the record lets an arc run past a full turn; the kernel's
///   conics take one turn either way), its NURBS parameter set to that interval. Proved on the
///   record's conic.
/// - **Polyline:** the degree-1 B-spline with segment `i` on `[i, i + 1]`.
/// - **Spline, rational spline:** the record's points (homogeneous ones copied); a knot vector
///   not clamped is clamped to its domain by the kernel's canonical constructor.
///
/// # Errors
///
/// `InvalidInput` for numbers not finite, an interval not increasing, a radius not positive, a
/// spline whose knots do not fit its points, or a conic whose NURBS is not proved on the
/// record's conic; `Unsupported` for a traced intersection curve; those of the constructors.
pub fn from_parameters(k: &Kernel, record: &Record, interval: [f64; 2]) -> Result<EdgeCurve> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let [lo, hi] = interval;
    if !(lo <= hi) {
        return Err(bad("the interval is not increasing"));
    }
    match record {
        Record::Line { start, end } => {
            let (a, b) = (p3(*start), p3(*end));
            Ok(EdgeCurve {
                curve: Curve::from(Line::create_canonical(a, b - a)),
                interval: Extent1d::from_raw(lo, hi),
            })
        }
        Record::Circle { frame, radius } => conic(k, frame, *radius, *radius, interval, true),
        Record::Ellipse { frame, radii } => conic(k, frame, radii[0], radii[1], interval, false),
        Record::Polyline { points } => {
            // segment i spans [i, i + 1]: a degree-1 B-spline with those knots
            let n = points.len();
            let mut knots = vec![0.0];
            knots.extend((0..n).map(|i| i as f64));
            knots.push(n.saturating_sub(1) as f64);
            let rows: Vec<[f64; 4]> = points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
            Ok(EdgeCurve {
                curve: Curve::from(spline(&knots, &rows, 3, OP)?),
                interval: Extent1d::from_raw(lo, hi),
            })
        }
        Record::Spline { knots, points } => {
            let rows: Vec<[f64; 4]> = points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
            Ok(EdgeCurve {
                curve: Curve::from(spline(knots, &rows, 3, OP)?),
                interval: Extent1d::from_raw(lo, hi),
            })
        }
        Record::RationalSpline { knots, homogeneous } => Ok(EdgeCurve {
            curve: Curve::from(spline(knots, homogeneous, 3, OP)?),
            interval: Extent1d::from_raw(lo, hi),
        }),
        Record::SurfaceIntersection { .. } => Err(SolidError::Unsupported {
            operation: OP,
            reason: "a traced intersection curve has no constructor from its numbers yet",
        }),
    }
}

/// A circle or ellipse edge on its NURBS form `form` (a spline or rational spline, the
/// parameter the edge's interval and trims are in): the conic of the record's numbers
/// (`conic`), keeping `form` as its NURBS, over the form's interval. The form must lie on the
/// record's conic (proved, [`curve_on_quadrics`]) and run once round it anticlockwise about the
/// frame's z; the kernel's conic is over the angles of the form's ends.
///
/// # Errors
///
/// `InvalidInput` where `conic` is no circle or ellipse, `form` no spline, the form is not
/// proved on the conic, or it does not run anticlockwise; those of the constructors.
pub fn conic_on_form(k: &Kernel, conic: &Record, form: &Record) -> Result<Curve> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let (frame, rx, ry, circle) = match conic {
        Record::Circle { frame, radius } => (frame, *radius, *radius, true),
        Record::Ellipse { frame, radii } => (frame, radii[0], radii[1], false),
        _ => return Err(bad("a NURBS form is given for a circle or ellipse only")),
    };
    let nurbs = match form {
        Record::Spline { knots, points } => {
            let rows: Vec<[f64; 4]> = points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
            spline(knots, &rows, 3, OP)?
        }
        Record::RationalSpline { knots, homogeneous } => spline(knots, homogeneous, 3, OP)?,
        _ => return Err(bad("a conic's NURBS form is not a spline")),
    };
    if !(rx > 0.0 && ry > 0.0 && rx.is_finite() && ry.is_finite()) {
        return Err(bad("a radius is not finite and positive"));
    }
    let (o, x, y) = axes(frame)?;
    if !curve_on_quadrics(&nurbs, &quadrics(o, x, y, rx, ry, circle))? {
        return Err(bad("a conic's NURBS form is off the record's conic"));
    }
    let domain = form_angles(&nurbs, o, x, y, rx, ry)
        .ok_or_else(|| bad("a conic's NURBS form does not run anticlockwise round it"))?;
    let opts = EllipseOptions {
        dim: 3,
        nurb: Some(&nurbs),
        inside_out: false,
    };
    let e = if circle {
        Ellipse::new_circle(k, o, x, y, domain, rx, &opts)?
    } else {
        Ellipse::new(k, o, x, y, domain, rx, ry, &opts)?
    };
    Ok(Curve::from(e))
}

/// A pcurve's numbers as the kernel's UV trim of an edge over `interval` (the edge's interval
/// in the kernel's parameter): the record's pcurve runs along the edge over the edge's own
/// interval in the record, and the kernel's trim runs along the edge over the edge's kernel
/// interval, so its knots are mapped affinely from the pcurve's domain onto `interval`.
///
/// # Errors
///
/// `InvalidInput` for a spline whose knots do not fit its points or a pcurve or interval of no
/// length; those of the constructors.
pub fn uv_trim(c: &Record2, interval: &Extent1d) -> Result<Curve> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP_TRIM, reason);
    let (knots, rows): (&[f64], Vec<[f64; 4]>) = match c {
        Record2::Spline { knots, points } => (
            knots,
            points.iter().map(|p| [p[0], p[1], NOZ, 1.0]).collect(),
        ),
        Record2::RationalSpline { knots, homogeneous } => (
            knots,
            homogeneous
                .iter()
                .map(|h| [h[0], h[1], NOZ, h[2]])
                .collect(),
        ),
    };
    let n = rows.len();
    let degree = knots
        .len()
        .checked_sub(n + 1)
        .ok_or_else(|| bad("too few knots"))?;
    let (Some(&s_lo), Some(&s_hi)) = (knots.get(degree), knots.get(n)) else {
        return Err(bad("too few knots"));
    };
    let (lo, hi) = (interval.min(), interval.max());
    if !(s_hi > s_lo) {
        return Err(bad("the pcurve's domain has no length"));
    }
    let scale = (hi - lo) / (s_hi - s_lo);
    let mapped: Vec<f64> = knots.iter().map(|s| lo + (s - s_lo) * scale).collect();
    Ok(Curve::from(spline(&mapped, &rows, 2, OP_TRIM)?))
}

fn p3(a: [f64; 3]) -> Point3 {
    Point3::new(a[0], a[1], a[2])
}

/// A record frame's origin and axes, the axes checked unit and square to rounding.
fn axes(f: &Frame) -> Result<(Point3, Vec3, Vec3)> {
    let ulps = 8.0 * f64::EPSILON;
    let (x, y) = (p3(f.x_axis), p3(f.y_axis));
    let unit = |v: Vec3| (v.length() - 1.0).abs() <= ulps;
    if !(unit(x) && unit(y) && x.dot(y).abs() <= ulps) {
        return Err(SolidError::invalid_input(
            OP,
            "the frame's axes are not unit and square",
        ));
    }
    Ok((p3(f.origin), x, y))
}

/// The plane of a conic and its sphere (a circle) or elliptic cylinder (an ellipse), as
/// quadrics `X.A.X + 2 b.X + c = 0`.
fn quadrics(o: Point3, x: Vec3, y: Vec3, rx: f64, ry: f64, circle: bool) -> [QuadricEquation; 2] {
    let n = x.cross(y);
    let oa = [o.x, o.y, o.z];
    let plane: QuadricEquation = ([[0.0; 3]; 3], [0.5 * n.x, 0.5 * n.y, 0.5 * n.z], -n.dot(o));
    if circle {
        let a = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let c = o.dot(o) - rx * rx;
        return [plane, (a, [-o.x, -o.y, -o.z], c)];
    }
    let (xa, ya) = ([x.x, x.y, x.z], [y.x, y.y, y.z]);
    let (sx, sy) = (1.0 / (rx * rx), 1.0 / (ry * ry));
    let mut a = [[0.0; 3]; 3];
    for ((row, xi), yi) in a.iter_mut().zip(xa).zip(ya) {
        for ((aij, xj), yj) in row.iter_mut().zip(xa).zip(ya) {
            *aij = sx * xi * xj + sy * yi * yj;
        }
    }
    let ao = a.map(|row| row.iter().zip(oa).map(|(p, q)| p * q).sum::<f64>());
    let c = oa.iter().zip(ao).map(|(p, q)| p * q).sum::<f64>() - 1.0;
    [plane, (a, [-ao[0], -ao[1], -ao[2]], c)]
}

/// The angles, degrees, of a conic's NURBS form from its start to its end about the frame's z
/// (`ConicDomainOfNurbs`): the angle at sixteen even steps of the form's parameter must turn
/// anticlockwise at each; a closed form is the whole turn. `None` where it turns back.
fn form_angles(
    nurbs: &BSplineCurve,
    o: Point3,
    x: Vec3,
    y: Vec3,
    rx: f64,
    ry: f64,
) -> Option<Extent1d> {
    let ivl = nurbs.natural_interval();
    let angle = |t: f64| -> Option<f64> {
        let d = nurbs.evaluate_point(t).ok()? - o;
        Some((d.dot(y) / ry).atan2(d.dot(x) / rx).to_degrees())
    };
    let n = 16;
    let (mut a0, mut prev) = (0.0, 0.0);
    for step in 0..=n {
        let a = angle(ivl.min() + (ivl.max() - ivl.min()) * f64::from(step) / f64::from(n))?;
        if step == 0 {
            a0 = a;
        } else if !(ieee_remainder(a - prev, 360.0) > 0.0) {
            return None;
        }
        prev = a;
    }
    let mut sweep = (prev - a0 + 720.0).rem_euclid(360.0);
    if sweep <= QUARTER_SNAP_DEG {
        sweep = 360.0;
    }
    if a0 + sweep > 360.0 {
        Some(Extent1d::from_raw(a0 - 360.0, a0 - 360.0 + sweep))
    } else {
        Some(Extent1d::from_raw(a0, a0 + sweep))
    }
}

/// `std::remainder`: `x - n y` with `n` the integer nearest `x / y`, ties to even.
fn ieee_remainder(x: f64, y: f64) -> f64 {
    let q = x / y;
    let mut n = q.round();
    if (q - q.trunc()).abs() == 0.5 && n.rem_euclid(2.0) != 0.0 {
        n -= q.signum();
    }
    x - n * y
}

/// Radians to the kernel's degrees, snapped onto a quarter turn within [`QUARTER_SNAP_DEG`].
fn degrees(radians: f64) -> f64 {
    let d = radians.to_degrees();
    for quarter in -4..=4 {
        let boundary = 90.0 * f64::from(quarter);
        if (d - boundary).abs() <= QUARTER_SNAP_DEG {
            return boundary;
        }
    }
    d
}

/// An arc's interval (radians) a whole number of turns moved into `[-2 pi, 2 pi]`.
fn turned([lo, hi]: [f64; 2]) -> [f64; 2] {
    use core::f64::consts::TAU;
    let shift = if hi > TAU {
        -TAU * ((hi - TAU) / TAU).ceil()
    } else if lo < -TAU {
        TAU * ((-TAU - lo) / TAU).ceil()
    } else {
        0.0
    };
    [lo + shift, hi + shift]
}

/// A circle or ellipse on the record's frame over `interval` (radians), proved on its conic.
fn conic(
    k: &Kernel,
    frame: &Frame,
    rx: f64,
    ry: f64,
    interval: [f64; 2],
    circle: bool,
) -> Result<EdgeCurve> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    if !(rx > 0.0 && ry > 0.0 && rx.is_finite() && ry.is_finite()) {
        return Err(bad("a radius is not finite and positive"));
    }
    if !(interval[0].is_finite() && interval[1].is_finite()) {
        return Err(bad("a conic's interval is not finite"));
    }
    let (o, x, y) = axes(frame)?;
    let [lo, hi] = turned(interval);
    let deg = Extent1d::from_min_max(degrees(lo), degrees(hi));
    let placement = Axis2Placement::new(o, x, y);
    let mut e = if circle {
        Ellipse::create_canonical_circle(k, &placement, rx, Some(&deg), None)?
    } else {
        Ellipse::create_canonical(k, &placement, rx, ry, Some(&deg), None)?
    };
    e.edit_parameterization(k, &deg, true, None)?;
    if !curve_on_quadrics(e.base(), &quadrics(o, x, y, rx, ry, circle))? {
        return Err(bad("the conic's NURBS is off the record's conic"));
    }
    Ok(EdgeCurve {
        curve: Curve::from(e),
        interval: deg,
    })
}

/// A B-spline of `dim` 2 or 3 from its knots and homogeneous rows `(w x, w y, w z, w)` (`z`
/// [`NOZ`] in 2D, `w = 1` for a polynomial one): filled directly when its knots are clamped,
/// else clamped to its domain by the kernel's canonical constructor.
fn spline(knots: &[f64], rows: &[[f64; 4]], dim: u64, op: &'static str) -> Result<BSplineCurve> {
    let bad = |reason: &'static str| SolidError::invalid_input(op, reason);
    let n = rows.len();
    let degree = knots
        .len()
        .checked_sub(n + 1)
        .filter(|d| *d >= 1)
        .ok_or_else(|| bad("the knots do not fit the points"))?;
    if rows.iter().any(|&[x, y, z, w]| {
        !(x.is_finite() && y.is_finite() && (z.is_finite() || z == NOZ) && w.is_finite() && w > 0.0)
    }) {
        return Err(bad("a point is not finite or a weight not positive"));
    }
    if !knots.iter().all(|t| t.is_finite()) {
        return Err(bad("a knot is not finite"));
    }
    if is_clamped(knots, degree) {
        let pw: Vec<CPoint> = rows
            .iter()
            .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
            .collect();
        let nurb = NurbCurve::new(pw, degree, knots.to_vec())?;
        let mut c = BSplineCurve::new(dim);
        c.set_from_gw_nurb(Some(&nurb))?;
        return Ok(c);
    }
    let (unique, mults) = compress_knots(knots);
    let points: Vec<Point3> = rows
        .iter()
        .map(|&[x, y, z, w]| {
            if dim == 3 {
                Point3::new(x / w, y / w, z / w)
            } else {
                Point3::new(x / w, y / w, 0.0)
            }
        })
        .collect();
    let weights: Vec<f64> = rows.iter().map(|r| r[3]).collect();
    let data = CanonicalData {
        dim,
        degree,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &unique,
        knot_type: KnotType::Unspecified,
        weights: Some(&weights),
        trim: None,
    };
    BSplineCurve::create_canonical(&data)
}

/// The first and last knots each repeat `degree + 1` times.
fn is_clamped(knots: &[f64], degree: usize) -> bool {
    let n = knots.len();
    let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
        return false;
    };
    n >= 2 * (degree + 1)
        && (1..=degree).all(|i| {
            knots.get(i) == Some(&first)
                && n.checked_sub(1 + i).and_then(|j| knots.get(j)) == Some(&last)
        })
}

/// The distinct knots and their multiplicities.
fn compress_knots(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let (mut unique, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &t in knots {
        match (unique.last(), mults.last_mut()) {
            (Some(&u), Some(m)) if u == t => *m += 1,
            _ => {
                unique.push(t);
                mults.push(1);
            }
        }
    }
    (unique, mults)
}

#[cfg(test)]
mod tests;
