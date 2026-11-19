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
//! Star-shaped about `C`. Write `g(t) = ((B(t) - C) x B'(t)) . n`. A simple closed outline
//! whose polar angle about `C` is strictly monotone winds once about `C`, so every ray from `C`
//! meets it once: `g` must keep one strict sign along every edge, the same sign on all of them.
//! That is decided exactly per edge. A conic's `g` is a trigonometric polynomial
//! `K + L cos t + M sin t`, whose extremes are closed forms. A B-spline's `g` is a polynomial
//! on each Bezier span, and its Bernstein coefficients bound it (the convex hull property,
//! Piegl & Tiller, *The NURBS Book*, 2nd ed., sec. 2.4 and A2.2): all coefficients of one sign
//! prove the sign. A span whose coefficients disagree is split in two, down to
//! [`MAX_SUBDIVISIONS`]; a span still undecided there is refused as not star-shaped.
//!
//! The volume of a dome is `(2/3) A h` for any star-shaped outline, because the section at
//! height `h sin s` is the outline scaled by `cos s` about `C`.

use std::f64::consts::PI;

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

/// The v-knots of the cap's rational quadratic meridian.
const MERIDIAN_KNOTS: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// How many times a Bezier span whose sign is not yet proved is split in two, at most.
const MAX_SUBDIVISIONS: usize = 8;

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
    /// The outline does not close, or an edge has no curve the kernel reads.
    OpenOutline,
    /// The outline is not star-shaped about its centroid, so the cap would fold (or the sign
    /// of its turn could not be proved).
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

/// The sign of `g` along an edge, in the edge's direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    Positive,
    Negative,
}

impl Turn {
    fn flipped(self) -> Turn {
        match self {
            Turn::Positive => Turn::Negative,
            Turn::Negative => Turn::Positive,
        }
    }
}

/// The outline of a planar face, chained end to end and trimmed to its edges.
struct Outline {
    /// The chained outline, one curve over its parameter range.
    curve: BSplineCurve,
    /// The radius, when the outline is one whole circle about the centroid.
    circle_radius: Option<f64>,
}

/// The cap of a dome of height `height` on the planar face `f` of `b`. `reverse` is refused
/// (not built yet). `Ok(Err(refusal))` for a face or height the dome cannot take.
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
    let (_area, centre) = face_centroid(k, b, f, CENTROID_ACCURACY)?;
    let centre = point_of(centre);
    let normal = outward_normal(k, b, f)?;
    let outline = match outline(k, b, f, centre, normal)? {
        Ok(outline) => outline,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let meridian = match shape {
        DomeShape::Elliptical => elliptical_meridian(height),
        DomeShape::Spherical => match outline.circle_radius {
            Some(radius) => spherical_meridian(radius, height),
            None => return Ok(Err(DomeRefusal::SphericalNeedsCircle)),
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
/// interval and turned to run on from the one before. Refused as not star-shaped when `g` does
/// not keep one strict sign over every edge.
fn outline(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    centre: [f64; 3],
    normal: [f64; 3],
) -> Result<std::result::Result<Outline, DomeRefusal>> {
    let Some(&lp) = face_p2::loops(b, f, false, None)?.first() else {
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
    let mut turns: Vec<Option<Turn>> = Vec::with_capacity(edges.len());
    let mut circle_radius = None;
    for &e in &edges {
        let (curve, interval) = edge_curve_copy(k, b, e)?;
        let (mut part, turn) = match natural_turn(k, &curve, interval, centre, normal)? {
            Some((part, turn)) => (part, turn),
            None => return Ok(Err(DomeRefusal::OpenOutline)),
        };
        if parts.len() == 1 {
            // the second edge may meet the first at the first's start: the first then runs the
            // other way round the loop
            let head = parts[0].ends()?;
            let ends = part.ends()?;
            let meets_start = distance(head.start, ends.start) <= tol || distance(head.start, ends.end) <= tol;
            let meets_end = distance(head.end, ends.start) <= tol || distance(head.end, ends.end) <= tol;
            if meets_start && !meets_end {
                let natural = parts[0].natural_interval();
                parts[0].reverse_parameterization(&natural)?;
                turns[0] = turns[0].map(Turn::flipped);
            }
        }
        if let Some(last) = parts.last() {
            let end = last.ends()?.end;
            let ends = part.ends()?;
            if distance(end, ends.start) > tol {
                if distance(end, ends.end) > tol {
                    return Ok(Err(DomeRefusal::OpenOutline));
                }
                let natural = part.natural_interval();
                part.reverse_parameterization(&natural)?;
                turns.push(turn.map(Turn::flipped));
            } else {
                turns.push(turn);
            }
        } else {
            turns.push(turn);
        }
        parts.push(part);
    }
    if edges.len() == 1 {
        circle_radius = circle_about(&edges, k, b, centre)?;
    }
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else {
        return Ok(Err(DomeRefusal::OpenOutline));
    };
    if distance(last.ends()?.end, first.ends()?.start) > tol {
        return Ok(Err(DomeRefusal::OpenOutline));
    }
    let Some(sign) = turns.first().copied().flatten() else {
        return Ok(Err(DomeRefusal::NotStar));
    };
    if turns.iter().any(|t| *t != Some(sign)) {
        return Ok(Err(DomeRefusal::NotStar));
    }
    let curve = BSplineCurve::create_by_joining(&mut parts, None)?;
    Ok(Ok(Outline {
        curve,
        circle_radius,
    }))
}

/// The radius of the one edge of the outline when it is a whole circle about `centre`.
fn circle_about(
    edges: &[crate::topology::EdgeId],
    k: &Kernel,
    b: &Brep,
    centre: [f64; 3],
) -> Result<Option<f64>> {
    let (curve, _) = edge_curve_copy(k, b, edges[0])?;
    let CurveKind::Ellipse(ell) = curve.kind() else {
        return Ok(None);
    };
    let origin = point_of(ell.position.origin());
    let radius = ell.radius_x;
    let off = sub(origin, centre);
    let round = (ell.radius_y - radius).abs() <= radius * RADIUS_SLACK
        && dot(off, off).sqrt() <= radius * RADIUS_SLACK;
    Ok(round.then_some(radius))
}

/// Relative slack on a circle's radius and centre, for rounding only.
const RADIUS_SLACK: f64 = 1e-9;

/// The edge's curve trimmed to its interval, as a B-spline, and the sign of `g` along it in its
/// own direction. `None` for an edge the kernel reads as no curve it can check.
fn natural_turn(
    k: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    centre: [f64; 3],
    normal: [f64; 3],
) -> Result<Option<(BSplineCurve, Option<Turn>)>> {
    let mut part = match curve.kind() {
        CurveKind::BSpline(b) => b.clone(),
        CurveKind::Line(l) => l.base_with_nurb()?.into_owned(),
        CurveKind::Ellipse(e) => e.base().clone(),
        _ => return Ok(None),
    };
    let mut interval = interval;
    part.trim(k, &mut interval, TrimMode::Silent)?;
    let turn = match curve.kind() {
        CurveKind::Ellipse(ell) => ellipse_turn(ell, interval, centre, normal),
        _ => bezier_turn(&mut part.clone(), centre, normal)?,
    };
    Ok(Some((part, turn)))
}

/// The sign of `g` on an elliptical arc over `interval`, in closed form. With `(e1, e2, n)`
/// right-handed, the arc's points in the plane are `(x0 + A cos t + B sin t, y0 + C cos t +
/// D sin t)` about the centroid, and `g = K + L cos t + M sin t` with
/// `K = AD - BC`, `L = x0 D - y0 B`, `M = y0 A - x0 C`. Its extremes lie at the endpoints and
/// at `phi + k pi`, `phi = atan2(M, L)`.
fn ellipse_turn(
    ell: &crate::curve::ellipse::Ellipse,
    interval: Extent1d,
    centre: [f64; 3],
    normal: [f64; 3],
) -> Option<Turn> {
    let (e1, e2) = plane_frame(normal);
    let origin = point_of(ell.position.origin());
    let (ax, ay) = (point_of_vec(ell.position.x_axis()), point_of_vec(ell.position.y_axis()));
    let d = sub(origin, centre);
    let (x0, y0) = (dot(d, e1), dot(d, e2));
    let (rx, ry) = (ell.radius_x, ell.radius_y);
    let (a, b, c, dd) = (rx * dot(ax, e1), ry * dot(ay, e1), rx * dot(ax, e2), ry * dot(ay, e2));
    let k = a * dd - b * c;
    let l = x0 * dd - y0 * b;
    let m = y0 * a - x0 * c;
    let g = |t: f64| k + l * t.cos() + m * t.sin();
    let (t0, t1) = (interval.min(), interval.max());
    let phi = m.atan2(l);
    let mut values = vec![g(t0), g(t1)];
    let first = ((t0 - phi) / PI).ceil();
    let last = ((t1 - phi) / PI).floor();
    let mut step = first;
    while step <= last {
        values.push(g(phi + step * PI));
        step += 1.0;
    }
    let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if lo > 0.0 {
        Some(Turn::Positive)
    } else if hi < 0.0 {
        Some(Turn::Negative)
    } else {
        None
    }
}

/// The sign of `g` on a B-spline, from the Bernstein coefficients of `g` on each Bezier span.
/// The spans are made by inserting every interior knot up to the curve's degree; `None` when a
/// span stays undecided, or when the curve is not clamped, so it has no Bezier spans to read.
fn bezier_turn(part: &mut BSplineCurve, centre: [f64; 3], normal: [f64; 3]) -> Result<Option<Turn>> {
    let degree = part.degree()?;
    let (knots, multiplicities) = part.knots(None)?;
    let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
        return Ok(None);
    };
    let mut new_knots = Vec::new();
    for (&value, &mult) in knots.iter().zip(&multiplicities) {
        if value == first || value == last {
            if mult != degree + 1 {
                return Ok(None);
            }
        } else {
            new_knots.extend(std::iter::repeat_n(value, degree.saturating_sub(mult)));
        }
    }
    part.insert_knots(&new_knots)?;
    let points: Vec<([f64; 3], f64)> = part
        .control_points()?
        .iter()
        .map(homogeneous)
        .collect();
    let mut sign = None;
    for span in points.windows(degree + 1).step_by(degree) {
        match piece_turn(span, centre, normal, 0) {
            Some(turn) if sign.is_none_or(|s| s == turn) => sign = Some(turn),
            _ => return Ok(None),
        }
    }
    Ok(sign)
}

/// A control point as `(H, w)`: its weighted coordinates and weight (`1` for a non-rational
/// point).
fn homogeneous(p: &CPoint) -> ([f64; 3], f64) {
    let w = if p.w == NOW { 1.0 } else { p.w };
    ([p.x, p.y, p.z], w)
}

/// The sign of `g` on a Bezier piece of homogeneous control points `pts` (degree `p = len - 1`),
/// from the Bernstein coefficients of its numerator, of degree `3p - 1`:
/// `N(t) = sum b_i H_i`, `W(t) = sum b_i w_i`, `g = (N - C W) x (N' W - N W') / W^3`, and
/// `N' W - N W' = p sum_j b^{p-1}_j sum_m b_m [(H_{j+1} - H_j) w_m - H_m (w_{j+1} - w_j)]`.
/// The triple product of Bernstein bases is a Bernstein basis of degree `3p - 1` with the
/// coefficient `C(p,i) C(p-1,j) C(p,m) / C(3p-1,k)`, `k = i + j + m`; the convex hull property
/// gives the sign. Undecided pieces are split in two, to [`MAX_SUBDIVISIONS`].
fn piece_turn(
    pts: &[([f64; 3], f64)],
    centre: [f64; 3],
    normal: [f64; 3],
    depth: usize,
) -> Option<Turn> {
    let p = pts.len().checked_sub(1)?;
    if p == 0 || pts.iter().any(|&(_, w)| w <= 0.0) {
        return None;
    }
    let total = 3 * p - 1;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for k in 0..=total {
        let mut sum = 0.0;
        for i in 0..=p {
            for j in 0..p {
                if k < i + j || k - i - j > p {
                    continue;
                }
                let m = k - i - j;
                let coeff = binomial(p, i) * binomial(p - 1, j) * binomial(p, m) / binomial(total, k);
                let (hi, wi) = pts[i];
                let (h0, w0) = pts[j];
                let (h1, w1) = pts[j + 1];
                let (hm, wm) = pts[m];
                let rel: [f64; 3] = std::array::from_fn(|d| hi[d] - centre[d] * wi);
                let diff: [f64; 3] = std::array::from_fn(|d| (h1[d] - h0[d]) * wm - hm[d] * (w1 - w0));
                sum += coeff * dot(cross(rel, diff), normal);
            }
        }
        let value = p as f64 * sum;
        lo = lo.min(value);
        hi = hi.max(value);
    }
    if lo > 0.0 {
        return Some(Turn::Positive);
    }
    if hi < 0.0 {
        return Some(Turn::Negative);
    }
    if depth >= MAX_SUBDIVISIONS {
        return None;
    }
    let (left, right) = split_piece(pts);
    let a = piece_turn(&left, centre, normal, depth + 1)?;
    let b = piece_turn(&right, centre, normal, depth + 1)?;
    (a == b).then_some(a)
}

/// The two Bezier pieces of `pts` split at the middle of its parameter range (de Casteljau).
/// Homogeneous points split linearly, so the weights go with the points.
fn split_piece(pts: &[([f64; 3], f64)]) -> (Vec<([f64; 3], f64)>, Vec<([f64; 3], f64)>) {
    let mut row = pts.to_vec();
    let mut left = vec![row[0]];
    let mut right = vec![row[row.len() - 1]];
    while row.len() > 1 {
        row = row
            .windows(2)
            .map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let point = std::array::from_fn(|d| 0.5 * (a.0[d] + b.0[d]));
                (point, 0.5 * (a.1 + b.1))
            })
            .collect();
        left.push(row[0]);
        right.push(row[row.len() - 1]);
    }
    right.reverse();
    (left, right)
}

/// The binomial coefficient `C(n, k)` as a float, for the small degrees a dome uses.
fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let mut out = 1.0;
    for i in 0..k {
        out = out * (n - i) as f64 / (i + 1) as f64;
    }
    out
}

/// The outward unit normal of the planar face `f`, as the body's own sense gives it.
fn outward_normal(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<[f64; 3]> {
    let e = *crate::topology::face::edges(b, f, None, false, None)?
        .first()
        .ok_or_else(|| SolidError::invalid_input(OP, "a planar face with no edge"))?;
    let fu = face_p2::outer_faceuse(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP, "a face without its outer use"))?;
    let frame = face_normal_at_edge(k, b, e, f, fu)?;
    Ok(point_of_vec(frame.normal.value))
}

/// Two unit vectors in the plane of `normal` with `e1 x e2 = normal`.
fn plane_frame(normal: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let helper = if normal[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let e1 = scale(cross(normal, helper), 1.0 / dot(cross(normal, helper), cross(normal, helper)).sqrt());
    let e2 = cross(normal, e1);
    (e1, e2)
}

/// The meridian of the elliptical dome: the quarter ellipse from the rim to the apex, as the
/// control points `(x, y, w)` of the cap (`x` scales the outline about the centroid, `y` is the
/// height along the normal).
fn elliptical_meridian(height: f64) -> [[f64; 3]; 3] {
    [
        [1.0, 0.0, 1.0],
        [1.0, height, std::f64::consts::FRAC_1_SQRT_2],
        [0.0, height, 1.0],
    ]
}

/// The meridian of the spherical dome on a circle of radius `radius`: the arc of the sphere
/// through the rim and the apex, as a rational quadratic whose middle weight is `cos(alpha/2)`
/// for the arc's span `alpha`.
fn spherical_meridian(radius: f64, height: f64) -> [[f64; 3]; 3] {
    // the sphere through the rim (radius, 0) and the apex (0, height), centred on the axis
    let big = (radius * radius + height * height) / (2.0 * height);
    let centre = [0.0, height - big];
    let v0 = [radius, -centre[1]];
    let v1 = [0.0, big];
    let cos_span = ((v0[0] * v1[0] + v0[1] * v1[1]) / (big * big)).clamp(-1.0, 1.0);
    let half = (0.5 * (1.0 + cos_span)).sqrt();
    let mid = [v0[0] / big + v1[0] / big, v0[1] / big + v1[1] / big];
    let mid_len = (mid[0] * mid[0] + mid[1] * mid[1]).sqrt();
    let control = [
        centre[0] + big * mid[0] / mid_len / half,
        centre[1] + big * mid[1] / mid_len / half,
    ];
    [
        [1.0, 0.0, 1.0],
        [control[0] / radius, control[1], half],
        [0.0, height, 1.0],
    ]
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
            let (h, w) = homogeneous(p);
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

fn point_of_vec(v: crate::geom::vector3d::Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn distance(a: crate::geom::vector3d::Point3, b: crate::geom::vector3d::Point3) -> f64 {
    let (a, b) = (point_of(a), point_of(b));
    dot(sub(a, b), sub(a, b)).sqrt()
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
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
