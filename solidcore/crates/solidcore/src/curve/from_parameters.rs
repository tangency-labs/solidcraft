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

use std::borrow::Cow;

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
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::surface::implicit::{conic_turn, curve_on_quadrics, QuadricEquation};

const OP: &str = "curve::from_parameters::from_parameters";
const OP_TRIM: &str = "curve::from_parameters::uv_trim";
const OP_BACK: &str = "curve::from_parameters::record_of";

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
/// record's conic (proved, [`curve_on_quadrics`]) and turn anticlockwise about the frame's z,
/// at most once round (certified per Bézier segment, [`conic_turn`]); the kernel's conic is
/// over the angles from the form's start through the angle it turns.
///
/// # Errors
///
/// `InvalidInput` where `conic` is no circle or ellipse, `form` no spline, the form is not
/// proved on the conic, or it is not proved to turn anticlockwise at most once round; those
/// of the constructors.
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
    let domain = form_angles(&nurbs, o, x, y, rx, ry)?.ok_or_else(|| {
        bad("a conic's NURBS form does not run anticlockwise round it at most once")
    })?;
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

/// Map a pcurve's entire natural support onto `interval` and scale its UV
/// coordinates. This is for callers that intend to remap the whole support.
/// A B-rep edge may occupy only part of that support: its importer must use
/// [`uv_trim_on_edge`] with the record edge's actual interval instead.
///
/// # Errors
///
/// `InvalidInput` for a spline whose knots do not fit its points or a pcurve or interval of no
/// length; those of the constructors.
pub fn uv_trim(c: &Record2, interval: &Extent1d, scale: [f64; 2]) -> Result<Curve> {
    let source = c
        .interval()
        .ok_or_else(|| SolidError::invalid_input(OP_TRIM, "invalid pcurve interval"))?;
    uv_trim_on_edge(c, source, interval, scale)
}

/// Rebuild a pcurve while mapping the record edge parameter to the kernel edge
/// parameter. The pcurve's own support may extend beyond either edge endpoint.
///
/// # Errors
/// Refuses invalid edge intervals or malformed UV spline data.
pub fn uv_trim_on_edge(
    c: &Record2,
    source: [f64; 2],
    interval: &Extent1d,
    scale: [f64; 2],
) -> Result<Curve> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP_TRIM, reason);
    let [su, sv] = scale;
    let same = su == 1.0 && sv == 1.0;
    let at = |x: f64, y: f64| if same { (x, y) } else { (su * x, sv * y) };
    let (knots, rows): (&[f64], Vec<[f64; 4]>) = match c {
        Record2::Spline { knots, points } => (
            knots,
            points
                .iter()
                .map(|p| {
                    let (x, y) = at(p[0], p[1]);
                    [x, y, NOZ, 1.0]
                })
                .collect(),
        ),
        Record2::RationalSpline { knots, homogeneous } => (
            knots,
            homogeneous
                .iter()
                .map(|h| {
                    let (x, y) = at(h[0], h[1]);
                    [x, y, NOZ, h[2]]
                })
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
    let [from_lo, from_hi] = source;
    if !(s_lo.is_finite()
        && s_hi.is_finite()
        && s_hi > s_lo
        && from_lo.is_finite()
        && from_hi.is_finite()
        && from_hi > from_lo
        && lo.is_finite()
        && hi.is_finite()
        && hi > lo)
    {
        return Err(bad("the pcurve's domain has no length"));
    }
    let mapped: Vec<f64> = if (from_lo, from_hi) == (lo, hi) {
        knots.to_vec()
    } else {
        let by = (hi - lo) / (from_hi - from_lo);
        knots
            .iter()
            .map(|s| {
                if *s == from_lo {
                    lo
                } else if *s == from_hi {
                    hi
                } else {
                    lo + (s - from_lo) * by
                }
            })
            .collect()
    };
    Ok(Curve::from(spline(&mapped, &rows, 2, OP_TRIM)?))
}

/// The intersection tolerance of an entity whose zone tolerance is `zone`: twice it, the
/// inverse of [`zone_tolerance`].
#[must_use]
pub fn intersection_tolerance(zone: f64) -> f64 {
    2.0 * zone
}

/// An edge's curve as the record holds it: what it is (`curve`), the NURBS form its parameter
/// is on when that differs from `curve`'s own (`nurbs`), and the edge's interval in the
/// record's parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordCurve {
    /// What the edge is: a line, circle or ellipse label, or the spline itself.
    pub curve: Record,
    /// The NURBS form of a circle or ellipse, the parameter `interval` and the pcurves are on.
    pub nurbs: Option<Record>,
    /// The edge's interval in the record's parameter.
    pub interval: [f64; 2],
}

/// The record of the kernel's edge curve `c` over `interval` (its kernel parameter), the
/// inverse of [`from_parameters`] and [`conic_on_form`]:
///
/// - **Line:** the line as the kernel holds it, its point and its point plus its vector, over
///   the STEP parameters of the interval's ends (`ConvertTFromNURBSToSTEP`); where those run
///   down, the vector and the parameters are negated so the interval increases. This is what
///   SolidCraft's flat export wrote (`FlatFromBrep`'s line origin and direction, its range in
///   STEP parameters), so a line comes back as it went in, not reparameterised onto `[0, 1]`.
/// - **Circle, ellipse:** the label on the conic's own frame (copied, not rebuilt by a cross
///   product), its y axis reversed where the conic's NURBS runs against its angle (its own
///   `inside_out`, no sampling), with that NURBS as its form, over the kernel's interval.
///   Exact class at bound 0: returned only once the form is proved on the label's plane and
///   sphere or elliptic cylinder ([`curve_on_quadrics`]).
/// - **B-spline, hyperbola, parabola:** its NURBS as a spline or rational spline, homogeneous
///   points copied, over the kernel's interval; an interval past the knots by the rounding of
///   the kernel's parameter (10⁻¹² of its size) ends on them.
///
/// # Errors
///
/// `InvalidInput` for an interval not finite and increasing, a curve without its NURBS, or a
/// conic's form not proved on its label; `Unsupported` for a curve with no B-spline form (an
/// offset, projected or surface curve), which the record cannot carry exactly.
pub fn record_of(k: &Kernel, c: &Curve, interval: &Extent1d) -> Result<RecordCurve> {
    use crate::curve::curve::CurveKind;
    let bad = |reason: &'static str| SolidError::invalid_input(OP_BACK, reason);
    let (lo, hi) = (interval.min(), interval.max());
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(bad("the interval is not finite and increasing"));
    }
    match c.kind() {
        CurveKind::Line(l) => {
            let can = l.canonical();
            let (mut s0, mut s1) = (
                c.convert_t_from_nurbs_to_step(k, lo)?,
                c.convert_t_from_nurbs_to_step(k, hi)?,
            );
            let mut v = can.vector;
            // a line whose parameter runs against its vector over the edge: the vector
            // reversed, the parameters negated, so the record's interval increases
            if s0 > s1 {
                (s0, s1) = (-s0, -s1);
                v = -v;
            }
            if !(s0.is_finite() && s1.is_finite() && s1 > s0) {
                return Err(bad("a line's parameters over the edge are not increasing"));
            }
            let p = can.point;
            return Ok(RecordCurve {
                curve: Record::Line {
                    start: [p.x, p.y, p.z],
                    end: [p.x + v.x, p.y + v.y, p.z + v.z],
                },
                nurbs: None,
                interval: [s0, s1],
            });
        }
        CurveKind::Ellipse(e) => {
            let can = e.canonical();
            let (o, x, y0) = can.position.canonical();
            let y = if can.inside_out { -y0 } else { y0 };
            let circle = e.is_circle_class();
            let (rx, ry) = (
                can.radius_x,
                if circle { can.radius_x } else { can.radius_y },
            );
            let made;
            let nurbs = if e.base().gw_nurb().is_some() {
                e.base()
            } else {
                let mut copy = e.clone();
                copy.make_nurb(k)?;
                made = copy;
                made.base()
            };
            if !curve_on_quadrics(nurbs, &quadrics(o, x, y, rx, ry, circle))? {
                return Err(bad("a conic's NURBS is off its own frame and radii"));
            }
            let frame = Frame {
                origin: [o.x, o.y, o.z],
                x_axis: [x.x, x.y, x.z],
                y_axis: [y.x, y.y, y.z],
            };
            let curve = if circle {
                Record::Circle { frame, radius: rx }
            } else {
                Record::Ellipse {
                    frame,
                    radii: [rx, ry],
                }
            };
            return Ok(RecordCurve {
                curve,
                nurbs: Some(spline_record(nurbs)?),
                interval: [lo, hi],
            });
        }
        _ => (),
    }
    let base = b_spline_part(k, c)?.ok_or(SolidError::Unsupported {
        operation: OP_BACK,
        reason: "an edge curve with no B-spline form has no exact record",
    })?;
    let knots = base.natural_interval();
    let slack = 1e-12 * (1.0 + lo.abs().max(hi.abs()));
    let onto = |t: f64| {
        if t < knots.min() && t >= knots.min() - slack {
            knots.min()
        } else if t > knots.max() && t <= knots.max() + slack {
            knots.max()
        } else {
            t
        }
    };
    Ok(RecordCurve {
        curve: spline_record(&base)?,
        nurbs: None,
        interval: [onto(lo), onto(hi)],
    })
}

/// The B-spline a curve is (every kernel line, conic and spline is one, its NURB made on a
/// copy where it has none yet); `None` for the curves that are not.
fn b_spline_part<'c>(k: &Kernel, c: &'c Curve) -> Result<Option<Cow<'c, BSplineCurve>>> {
    use crate::curve::curve::CurveKind;
    Ok(Some(match c.kind() {
        CurveKind::BSpline(b) => Cow::Borrowed(b),
        CurveKind::Line(l) => l.base_with_nurb()?,
        CurveKind::Ellipse(e) if e.base().gw_nurb().is_none() => {
            let mut made = e.clone();
            made.make_nurb(k)?;
            Cow::Owned(made.base().clone_exact())
        }
        CurveKind::Ellipse(e) => Cow::Borrowed(e.base()),
        CurveKind::Hyperbola(h) if h.base().gw_nurb().is_none() => {
            let mut made = h.clone();
            made.make_nurb(None)?;
            Cow::Owned(made.base().clone_exact())
        }
        CurveKind::Hyperbola(h) => Cow::Borrowed(h.base()),
        CurveKind::Parabola(p) if p.base().gw_nurb().is_none() => {
            let mut made = p.clone();
            made.make_nurb(None)?;
            Cow::Owned(made.base().clone_exact())
        }
        CurveKind::Parabola(p) => Cow::Borrowed(p.base()),
        _ => return Ok(None),
    }))
}

/// A B-spline curve's NURBS as the record's spline (every weight 1) or rational spline
/// (homogeneous points copied as NLib holds them).
fn spline_record(c: &BSplineCurve) -> Result<Record> {
    let nurb = c
        .gw_nurb()
        .ok_or_else(|| SolidError::invalid_input(OP_BACK, "a B-spline without its NURB"))?;
    let knots = nurb.knots().to_vec();
    let polynomial = nurb.cpts().iter().all(|p| p.w == NOW || p.w == 1.0);
    Ok(if polynomial {
        Record::Spline {
            knots,
            points: nurb.cpts().iter().map(|p| [p.x, p.y, p.z]).collect(),
        }
    } else {
        Record::RationalSpline {
            knots,
            homogeneous: nurb
                .cpts()
                .iter()
                .map(|p| {
                    let w = if p.w == NOW { 1.0 } else { p.w };
                    [p.x, p.y, p.z, w]
                })
                .collect(),
        }
    })
}

/// The record's pcurve of the kernel's UV trim `trim`, the inverse of [`uv_trim`]: the trim
/// runs along its edge over `from` (the edge's kernel interval), the pcurve over `to` (the
/// edge's record interval), so the knots are mapped affinely from one onto the other (copied
/// as they are where the two are the same, the knots at the trim's ends set to `to`'s where
/// they are `from`'s); the points are scaled by `scale` in u and v (the record's parameters
/// of a plane face, distances along its unit axes, are its kernel parameters times its
/// scales; `[1, 1]` elsewhere, copied).
///
/// # Errors
///
/// `InvalidInput` for a trim that is no 2D B-spline or has no NURB, or an interval of no
/// length.
pub fn record_trim(
    k: &Kernel,
    trim: &Curve,
    from: &Extent1d,
    to: [f64; 2],
    scale: [f64; 2],
) -> Result<Record2> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP_BACK, reason);
    let b = b_spline_part(k, trim)?.ok_or_else(|| bad("a UV trim with no B-spline form"))?;
    let nurb = b
        .gw_nurb()
        .ok_or_else(|| bad("a UV trim without its NURB"))?;
    let (f0, f1) = (from.min(), from.max());
    let [t0, t1] = to;
    if !(f1 > f0 && t1 > t0) {
        return Err(bad("an interval has no length"));
    }
    let knots: Vec<f64> = if (f0, f1) == (t0, t1) {
        nurb.knots().to_vec()
    } else {
        let s = (t1 - t0) / (f1 - f0);
        nurb.knots()
            .iter()
            .map(|&t| {
                if t == f0 {
                    t0
                } else if t == f1 {
                    t1
                } else {
                    t0 + (t - f0) * s
                }
            })
            .collect()
    };
    let [su, sv] = scale;
    let same = su == 1.0 && sv == 1.0;
    let polynomial = nurb.cpts().iter().all(|p| p.w == NOW || p.w == 1.0);
    Ok(if polynomial {
        Record2::Spline {
            knots,
            points: nurb
                .cpts()
                .iter()
                .map(|p| {
                    if same {
                        [p.x, p.y]
                    } else {
                        [su * p.x, sv * p.y]
                    }
                })
                .collect(),
        }
    } else {
        Record2::RationalSpline {
            knots,
            homogeneous: nurb
                .cpts()
                .iter()
                .map(|p| {
                    let w = if p.w == NOW { 1.0 } else { p.w };
                    if same {
                        [p.x, p.y, w]
                    } else {
                        [su * p.x, sv * p.y, w]
                    }
                })
                .collect(),
        }
    })
}

fn p3(a: [f64; 3]) -> Point3 {
    Point3::new(a[0], a[1], a[2])
}

/// A record frame's origin and axes, the axes checked unit and square to rounding.
/// Unit and square axes, each within 8 ulps of 1: the record construction's
/// and the exact-form check's shared frame test.
fn unit_square(x: Vec3, y: Vec3) -> bool {
    let ulps = 8.0 * f64::EPSILON;
    let unit = |v: Vec3| (v.length() - 1.0).abs() <= ulps;
    unit(x) && unit(y) && x.dot(y).abs() <= ulps
}

fn axes(f: &Frame) -> Result<(Point3, Vec3, Vec3)> {
    let (x, y) = (p3(f.x_axis), p3(f.y_axis));
    if !unit_square(x, y) {
        return Err(SolidError::invalid_input(
            OP,
            "the frame's axes are not unit and square",
        ));
    }
    Ok((p3(f.origin), x, y))
}

/// Check an ellipse's attached rational form against its analytic support using
/// the same exact-class certificate as record construction. Public curve
/// parameters remain the attached form's parameters.
pub(crate) fn ellipse_form_is_exact(e: &Ellipse) -> Result<bool> {
    let placement = e.position();
    let (o, x, y) = (placement.origin(), placement.x_axis(), placement.y_axis());
    let (rx, ry) = (e.x_radius(), e.y_radius());
    if !(rx > 0.0
        && ry > 0.0
        && rx.is_finite()
        && ry.is_finite()
        && [o.x, o.y, o.z].iter().all(|x| x.is_finite())
        && unit_square(x, y))
    {
        return Ok(false);
    }
    let mut form = e.base().clone_exact();
    for point in form.get_or_create_gw_nurb()?.cpts_mut() {
        if point.z == NOZ {
            point.z = 0.0;
        }
    }
    curve_on_quadrics(&form, &quadrics(o, x, y, rx, ry, false))
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
/// (`ConicDomainOfNurbs`): its start's angle and the angle it turns, certified anticlockwise
/// segment by segment from its Bernstein coefficients ([`conic_turn`]). A form turning back
/// anywhere, or more than a whole turn (past [`QUARTER_SNAP_DEG`]), is `None`; a form turning
/// a whole turn to that snap is the whole turn.
///
/// # Errors
///
/// Those of [`conic_turn`] and of evaluating the form's start.
fn form_angles(
    nurbs: &BSplineCurve,
    o: Point3,
    x: Vec3,
    y: Vec3,
    rx: f64,
    ry: f64,
) -> Result<Option<Extent1d>> {
    let d = nurbs.evaluate_point(nurbs.natural_interval().min())? - o;
    let a0 = (d.dot(y) / ry).atan2(d.dot(x) / rx).to_degrees();
    let Some(mut sweep) = conic_turn(nurbs, o, x, y, rx, ry)? else {
        return Ok(None);
    };
    if (sweep - 360.0).abs() <= QUARTER_SNAP_DEG {
        sweep = 360.0;
    }
    if !(sweep > 0.0 && sweep <= 360.0) {
        return Ok(None);
    }
    Ok(Some(if a0 + sweep > 360.0 {
        Extent1d::from_raw(a0 - 360.0, a0 - 360.0 + sweep)
    } else {
        Extent1d::from_raw(a0, a0 + sweep)
    }))
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
