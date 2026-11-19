//! A dome on a planar face (#19532): the face raised along its outward normal into a cap of
//! height `h` whose outline is the face's own edges.
//!
//! The cap is one face on the full box of a rational tensor product, the same shape a sphere
//! or a cone has at its pole. With `C` the face's centroid, `n` its outward unit normal, `B(t)`
//! the outline (its edges chained end to end, each trimmed to its interval) and a meridian of
//! three weighted points `(x_j, y_j, w_j)`, the cap is
//!
//! ```text
//! S(t, s) = C + x(s) (B(t) - C) + y(s) n
//! ```
//!
//! so at `s = 0` it is the outline itself (the face's edges), and at `s = 1` every row meets the
//! apex `C + h n`, the degenerate pole the sphere convention drops. The meridian is the quarter
//! ellipse `(1, 0) (1, h) (0, h)` for an elliptical dome, which on a circle is the half
//! ellipsoid, and the arc from the rim to the apex of the sphere through them for a spherical
//! dome on a circle. Each row's weight is `w_i w_j`, so the surface is exact and rational.
//!
//! The outline is star-shaped about `C`: every ray from `C` crosses it once. That is checked on
//! samples of the chained outline: the polar angle about `C` must grow (or shrink) at every
//! sample and turn once in total. The volume of a dome is `(2/3) A h` for any star-shaped
//! outline, because the section at height `h sin s` is the outline scaled by `cos s` about `C`.

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use crate::curve::curve::{Curve, CurveKind};
use crate::geom::extent1d::Extent1d;
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::mass::integrator::properties::face_centroid;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{CPoint, NOW};
use crate::query::api_queries::{edge_curve_copy, face_normal_at_edge};
use crate::surface::surface::Surface;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p4::stitch_faces;
use crate::topology::brep::sheet::face_on;
use crate::topology::face::p2 as face_p2;
use crate::topology::loopuse;
use crate::topology::{Brep, FaceId};
use crate::{Result, SolidError};

const OP: &str = "topology::brep::dome";

/// The relative accuracy the face's centroid is asked for.
const CENTROID_ACCURACY: f64 = 1e-12;

/// Samples of the outline the star check reads, one per turn of its polar angle at most.
const STAR_SAMPLES: usize = 256;

/// The turn of the polar angle a star outline makes about its centroid, in turns (one).
const STAR_TURNS: f64 = 1.0;

/// Relative slack on the star check's turn count and on a circle's radius (rounding only).
const STAR_SLACK: f64 = 1e-6;

/// The v-knots of the cap's rational quadratic meridian.
const MERIDIAN_KNOTS: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// The shape of a dome on a circular or outlined face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomeShape {
    /// The quarter-ellipse meridian: on a circle, the half ellipsoid of revolution.
    Elliptical,
    /// The sphere through the rim and the apex, on a circle: a spherical cap.
    Spherical,
}

/// Why a dome is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomeRefusal {
    /// The face is not planar.
    NotPlanar,
    /// The face has holes; a dome takes one outline.
    Holes,
    /// The outline does not close, or its edges have no B-spline form.
    OpenOutline,
    /// The outline is not star-shaped about its centroid, so the cap would fold.
    NotStar,
    /// The height is zero or negative.
    NonPositiveHeight,
    /// The height is not a finite number.
    NonFiniteHeight,
    /// A spherical dome needs a circular face.
    SphericalNeedsCircle,
    /// A dome cut into the body is not built yet.
    ReverseNotSupported,
}

/// A dome's cap: one face on the full box of its surface, with its outline the face's own.
#[derive(Debug)]
pub struct DomeCap {
    /// The cap as a sheet of one face.
    pub sheet: Brep,
    /// The apex, `h` above the centroid along the outward normal.
    pub apex: [f64; 3],
}

/// The outline of a planar face, chained end to end and trimmed to its edges.
struct Outline {
    /// The chained outline, one curve over its parameter range.
    curve: BSplineCurve,
    /// The samples the star check reads.
    samples: Vec<[f64; 3]>,
}

/// The cap of a dome of height `height` on the planar face `f` of `b`, outward when `reverse`
/// is false. `Ok(Err(refusal))` for a face or height the dome cannot take.
///
/// # Errors
///
/// The kernel's errors for a face or curve the kernel cannot read.
pub fn dome_cap(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    height: f64,
    shape: DomeShape,
    reverse: bool,
) -> Result<std::result::Result<DomeCap, DomeRefusal>> {
    if reverse {
        return Ok(Err(DomeRefusal::ReverseNotSupported));
    }
    if height.is_nan() || height.is_infinite() {
        return Ok(Err(DomeRefusal::NonFiniteHeight));
    }
    if height <= 0.0 {
        return Ok(Err(DomeRefusal::NonPositiveHeight));
    }
    let planar = {
        let surface = face_p2::surface(b, f)?
            .ok_or_else(|| SolidError::invalid_input(OP, "a face without its surface"))?;
        surface.is_planar(k, face_p2::tolerance(b, f)?)?.is_planar
    };
    if !planar {
        return Ok(Err(DomeRefusal::NotPlanar));
    }
    if face_p2::loops(b, f, false, None)?.len() != 1 {
        return Ok(Err(DomeRefusal::Holes));
    }
    let outline = match outline(k, b, f)? {
        Ok(outline) => outline,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let (_area, centre) = face_centroid(k, b, f, CENTROID_ACCURACY)?;
    let centre = point_of(centre);
    let normal = outward_normal(k, b, f)?;
    if !is_star(&outline.samples, centre, normal) {
        return Ok(Err(DomeRefusal::NotStar));
    }
    let meridian = match shape {
        DomeShape::Elliptical => elliptical_meridian(height),
        DomeShape::Spherical => match spherical_meridian(&outline.samples, centre, height) {
            Ok(meridian) => meridian,
            Err(refusal) => return Ok(Err(refusal)),
        },
    };
    let surface = cap_surface(&outline.curve, centre, normal, &meridian)?;
    let mut sheet = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    let surface = Surface::from(surface);
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    face_on(k, &mut sheet, surface, &bx)?;
    sheet.refresh_cached_topology();
    let tol = sheet.tolerance();
    stitch_faces(k, &mut sheet, tol, None, false)?;
    sheet.refresh_cached_topology();
    let apex: [f64; 3] = std::array::from_fn(|i| centre[i] + height * normal[i]);
    Ok(Ok(DomeCap { sheet, apex }))
}

/// The outline of the outer loop of `f`, its edges chained in order, each trimmed to its
/// interval and turned to run on from the one before.
fn outline(k: &mut Kernel, b: &mut Brep, f: FaceId) -> Result<std::result::Result<Outline, DomeRefusal>> {
    let loops = face_p2::loops(b, f, false, None)?;
    let Some(&lp) = loops.first() else {
        return Ok(Err(DomeRefusal::OpenOutline));
    };
    let mut edges = Vec::new();
    for e in loopuse::loop_edges(b, lp)? {
        let Some(e) = e else {
            return Ok(Err(DomeRefusal::OpenOutline));
        };
        edges.push(e);
    }
    let tol = b.tolerance();
    let mut parts: Vec<BSplineCurve> = Vec::with_capacity(edges.len());
    for e in edges {
        let (curve, interval) = edge_curve_copy(k, b, e)?;
        let mut part = match bspline_of(&curve)? {
            Some(part) => part,
            None => return Ok(Err(DomeRefusal::OpenOutline)),
        };
        let mut interval: Extent1d = interval;
        part.trim(k, &mut interval, TrimMode::Silent)?;
        if let Some(last) = parts.last() {
            let end = last.ends()?.end;
            let ends = part.ends()?;
            if distance(end, ends.start) > tol {
                if distance(end, ends.end) > tol {
                    return Ok(Err(DomeRefusal::OpenOutline));
                }
                let natural = part.natural_interval();
                part.reverse_parameterization(&natural)?;
            }
        }
        parts.push(part);
    }
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else {
        return Ok(Err(DomeRefusal::OpenOutline));
    };
    if distance(last.ends()?.end, first.ends()?.start) > tol {
        return Ok(Err(DomeRefusal::OpenOutline));
    }
    let curve = BSplineCurve::create_by_joining(&mut parts, None)?;
    let domain = curve.natural_interval();
    let span = domain.max() - domain.min();
    let samples = (0..STAR_SAMPLES)
        .map(|i| {
            let t = domain.min() + span * i as f64 / STAR_SAMPLES as f64;
            Ok(point_of(curve.evaluate_point(t)?))
        })
        .collect::<Result<Vec<[f64; 3]>>>()?;
    Ok(Ok(Outline { curve, samples }))
}

/// The outward unit normal of the planar face `f`, as the body's own sense gives it.
fn outward_normal(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<[f64; 3]> {
    let e = *crate::topology::face::edges(b, f, None, false, None)?
        .first()
        .ok_or_else(|| SolidError::invalid_input(OP, "a planar face with no edge"))?;
    let fu = face_p2::outer_faceuse(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP, "a face without its outer use"))?;
    let frame = face_normal_at_edge(k, b, e, f, fu)?;
    let v = frame.normal.value;
    Ok([v.x, v.y, v.z])
}

/// The curve as a B-spline: a line, an ellipse or a circle's B-spline form, or a B-spline.
fn bspline_of(curve: &Curve) -> Result<Option<BSplineCurve>> {
    Ok(match curve.kind() {
        CurveKind::BSpline(b) => Some(b.clone()),
        CurveKind::Line(l) => Some(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Some(e.base().clone()),
        _ => None,
    })
}

/// Whether the samples of a closed outline wind once about `centre` in the plane of `normal`,
/// monotonically: every step turns the same way, and the total is one turn.
fn is_star(samples: &[[f64; 3]], centre: [f64; 3], normal: [f64; 3]) -> bool {
    let Some(&first) = samples.first() else {
        return false;
    };
    let radial = sub(first, centre);
    let Some(e1) = unit(radial) else {
        return false;
    };
    let e2 = cross(normal, e1);
    let angle = |p: [f64; 3]| {
        let d = sub(p, centre);
        dot(d, e2).atan2(dot(d, e1))
    };
    let mut total = 0.0;
    let mut sign = 0.0;
    for i in 0..samples.len() {
        let a = angle(samples[i]);
        let b = angle(samples[(i + 1) % samples.len()]);
        let step = wrap(b - a);
        if step == 0.0 || (sign != 0.0 && step.signum() != sign) {
            return false;
        }
        sign = step.signum();
        total += step;
    }
    (total.abs() / TAU - STAR_TURNS).abs() <= STAR_SLACK
}

/// `angle` wrapped into (-π, π].
fn wrap(angle: f64) -> f64 {
    angle - TAU * ((angle + PI) / TAU).floor()
}

/// The meridian of the elliptical dome: the quarter ellipse from the rim to the apex, as the
/// control points `(x, y, w)` of the cap (`x` scales the outline about the centroid, `y` is the
/// height along the normal).
fn elliptical_meridian(height: f64) -> [[f64; 3]; 3] {
    [
        [1.0, 0.0, 1.0],
        [1.0, height, FRAC_1_SQRT_2],
        [0.0, height, 1.0],
    ]
}

/// The meridian of the spherical dome on a circle: the arc of the sphere through the rim and
/// the apex, as a rational quadratic whose middle weight is `cos(alpha / 2)` for the arc's
/// span `alpha`. `Err` when the face is not a circle about the centroid.
fn spherical_meridian(
    samples: &[[f64; 3]],
    centre: [f64; 3],
    height: f64,
) -> std::result::Result<[[f64; 3]; 3], DomeRefusal> {
    let radius = distance(point_at(samples[0]), point_at(centre));
    let circle = samples
        .iter()
        .all(|&p| (distance(point_at(p), point_at(centre)) - radius).abs() <= STAR_SLACK * radius);
    if !circle || radius <= 0.0 {
        return Err(DomeRefusal::SphericalNeedsCircle);
    }
    // the sphere through the rim (radius, 0) and the apex (0, height) with its centre on the axis
    let big = (radius * radius + height * height) / (2.0 * height);
    let c = [0.0, height - big];
    let v0 = [radius, -c[1]];
    let v1 = [0.0, big];
    let cos_span = ((v0[0] * v1[0] + v0[1] * v1[1]) / (big * big)).clamp(-1.0, 1.0);
    let half = (0.5 * (1.0 + cos_span)).sqrt();
    let mid = [
        v0[0] / big + v1[0] / big,
        v0[1] / big + v1[1] / big,
    ];
    let mid_len = (mid[0] * mid[0] + mid[1] * mid[1]).sqrt();
    let control = [
        c[0] + big * mid[0] / mid_len / half,
        c[1] + big * mid[1] / mid_len / half,
    ];
    Ok([
        [1.0, 0.0, 1.0],
        [control[0] / radius, control[1], half],
        [0.0, height, 1.0],
    ])
}

/// The cap surface: the rational tensor product of the outline with the meridian. Row `i` of
/// the net is the outline's control point `P_i` (weight `w_i`), placed on each meridian point
/// `j` as `C + x_j (P_i - C) + y_j n`, with weight `w_i w_j`.
fn cap_surface(
    outline: &BSplineCurve,
    centre: [f64; 3],
    normal: [f64; 3],
    meridian: &[[f64; 3]; 3],
) -> Result<BSplineSurface> {
    let points = outline.control_points()?;
    let degree = outline.degree()?;
    let (knots, _) = outline.knots(None)?;
    let rows: Vec<Vec<CPoint>> = points
        .iter()
        .map(|p| {
            let w = if p.w == NOW { 1.0 } else { p.w };
            let h = [p.x, p.y, p.z];
            meridian
                .iter()
                .map(|&[x, y, mw]| {
                    let xyz: [f64; 3] = std::array::from_fn(|d| {
                        mw * (w * centre[d] + x * (h[d] - w * centre[d]) + y * w * normal[d])
                    });
                    CPoint {
                        x: xyz[0],
                        y: xyz[1],
                        z: xyz[2],
                        w: mw * w,
                    }
                })
                .collect()
        })
        .collect();
    let n = points.len() - 1;
    let r = knots.len() - 1;
    BSplineSurface::from_data(n, 2, degree, 2, r, 5, &rows, &knots, &MERIDIAN_KNOTS)
}

fn point_of(p: crate::geom::vector3d::Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn point_at(p: [f64; 3]) -> crate::geom::vector3d::Point3 {
    crate::geom::vector3d::Point3::new(p[0], p[1], p[2])
}

fn distance(a: crate::geom::vector3d::Point3, b: crate::geom::vector3d::Point3) -> f64 {
    let (a, b) = (point_of(a), point_of(b));
    dot(sub(a, b), sub(a, b)).sqrt()
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let len = dot(a, a).sqrt();
    (len > 0.0).then(|| a.map(|x| x / len))
}
