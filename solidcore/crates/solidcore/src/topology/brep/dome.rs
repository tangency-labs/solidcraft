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

use crate::curve::curve::{Curve, CurveKind};
use crate::error::EntityRef;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
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
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
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
    /// The chained outline's pieces, each trimmed to its edge and turned to run on from the one
    /// before.
    pieces: Vec<BSplineCurve>,
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
    let (curve, pieces) = join_pieces(outline.pieces)?;
    let surface = cap_surface(&curve, centre, normal, &meridian)?;
    let seam = seam_curve(&curve, centre, normal, &meridian)?;
    let mut sheet = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    cap_face(k, &mut sheet, surface, &pieces, &seam)?;
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
    Ok(Ok(Outline {
        pieces: parts,
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
    let turn = bezier_turn(&mut part.clone(), centre, normal)?;
    Ok(Some((part, turn)))
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
                .map(|&m| cap_point(h, w, centre, normal, m))
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

/// The cap's control point on meridian point `[x, y, mw]` for the outline's control point with
/// homogeneous coordinates `h` and weight `w`: `mw (w C + x (h - w C) + y w n)`, weight `mw w`.
fn cap_point(h: [f64; 3], w: f64, centre: [f64; 3], normal: [f64; 3], [x, y, mw]: [f64; 3]) -> CPoint {
    let xyz: [f64; 3] = std::array::from_fn(|d| {
        mw * (w * centre[d] + x * (h[d] - w * centre[d]) + y * w * normal[d])
    });
    CPoint {
        x: xyz[0],
        y: xyz[1],
        z: xyz[2],
        w: mw * w,
    }
}

/// The cap's meridian at the outline's start: the iso curve `u = 0` of the cap, the seam.
fn seam_curve(
    curve: &BSplineCurve,
    centre: [f64; 3],
    normal: [f64; 3],
    meridian: &[[f64; 3]; 3],
) -> Result<BSplineCurve> {
    let (h, w) = homogeneous(&curve.control_points()?[0]);
    let points: Vec<CPoint> = meridian
        .iter()
        .map(|&m| cap_point(h, w, centre, normal, m))
        .collect();
    bspline(&points, 2, &MERIDIAN_KNOTS)
}

/// The outline's pieces joined into one curve over `[0, m]` (the piece `i` on `[i, i+1]`),
/// by an affine reparametrisation of each piece and the knot and control point layout of a
/// clamped B-spline: the joints are knots of multiplicity `degree` (continuous), and each
/// later piece drops its first control point, which the joint shares. Returns the joined curve
/// and the pieces, reparametrised the same way.
fn join_pieces(mut pieces: Vec<BSplineCurve>) -> Result<(BSplineCurve, Vec<BSplineCurve>)> {
    let mut degree = 0;
    for piece in &pieces {
        degree = degree.max(piece.degree()?);
    }
    for (i, piece) in pieces.iter_mut().enumerate() {
        if piece.degree()? < degree {
            piece.degree_elevate(degree)?;
        }
        piece.edit_parameterization(&Extent1d::from_min_max(i as f64, (i + 1) as f64))?;
    }
    let count = pieces.len();
    let mut points: Vec<CPoint> = Vec::new();
    let mut knots: Vec<f64> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        let full = full_knots(piece)?;
        let first_knot = if i == 0 { 0 } else { degree + 1 };
        let end_knot = if i + 1 == count { full.len() } else { full.len() - 1 };
        knots.extend_from_slice(&full[first_knot..end_knot]);
        let first_point = if i == 0 { 0 } else { 1 };
        points.extend(piece.control_points()?[first_point..].iter().map(rational));
    }
    let joined = bspline(&points, degree, &knots)?;
    Ok((joined, pieces))
}

/// A curve's knot vector with each knot repeated by its multiplicity.
fn full_knots(curve: &BSplineCurve) -> Result<Vec<f64>> {
    let (unique, multiplicities) = curve.knots(None)?;
    Ok(unique
        .iter()
        .zip(multiplicities)
        .flat_map(|(&value, mult)| std::iter::repeat_n(value, mult))
        .collect())
}

/// A control point with its weight made explicit (`1` for a non-rational point).
fn rational(p: &CPoint) -> CPoint {
    if p.w == NOW {
        CPoint {
            x: p.x,
            y: p.y,
            z: p.z,
            w: 1.0,
        }
    } else {
        *p
    }
}

/// A straight parameter-space segment from `a` to `b` over `[t0, t1]`, as a pcurve.
fn uv_line(a: [f64; 2], b: [f64; 2], t0: f64, t1: f64) -> Result<Curve> {
    let points = vec![
        CPoint { x: a[0], y: a[1], z: 0.0, w: 1.0 },
        CPoint { x: b[0], y: b[1], z: 0.0, w: 1.0 },
    ];
    Ok(Curve::from(bspline(&points, 1, &[t0, t0, t1, t1])?))
}

/// The cap's one face, made from explicit loop uses: the outline's pieces along `v = 0`, the seam
/// up at `u = m`, a degenerate pole use with no 3D curve (its neighbours share the apex vertex,
/// as the STEP reader's collapsed side does), and the seam down at `u = 0`, `v` from 1 to 0.
/// The seam curve is used twice, once in each direction.
fn cap_face(
    k: &mut Kernel,
    b: &mut Brep,
    surface: BSplineSurface,
    pieces: &[BSplineCurve],
    seam: &BSplineCurve,
) -> Result<()> {
    let surface = Surface::from(surface);
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let top = pieces.len() as f64;
    let mut curves: Vec<Option<Curve>> = Vec::with_capacity(pieces.len() + 3);
    let mut uvs: Vec<Curve> = Vec::with_capacity(pieces.len() + 3);
    let mut orientations = Vec::with_capacity(pieces.len() + 3);
    for (i, piece) in pieces.iter().enumerate() {
        let (lo, hi) = (i as f64, (i + 1) as f64);
        curves.push(Some(Curve::from(piece.clone())));
        uvs.push(uv_line([lo, 0.0], [hi, 0.0], lo, hi)?);
        orientations.push(OrientType::Same);
    }
    curves.push(Some(Curve::from(seam.clone())));
    uvs.push(uv_line([top, 0.0], [top, 1.0], 0.0, 1.0)?);
    orientations.push(OrientType::Same);
    curves.push(None);
    uvs.push(uv_line([top, 1.0], [0.0, 1.0], 0.0, 1.0)?);
    orientations.push(OrientType::Same);
    curves.push(Some(Curve::from(seam.clone())));
    uvs.push(uv_line([0.0, 0.0], [0.0, 1.0], 0.0, 1.0)?);
    orientations.push(OrientType::Opposite);
    let input = FaceCurvesInput {
        loops: vec![pieces.len() + 3],
        curves_3d: Some(curves),
        uv_curves: Some(uvs),
        orientations,
        loop_points: Vec::new(),
    };
    let region = b.infinite_region();
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    make_face_with_curves(
        k,
        b,
        region,
        input,
        surface,
        &bx,
        OrientType::Same,
        None,
        &this_face,
        &mut env,
    )?;
    Ok(())
}

/// The B-spline of `points` (homogeneous, `dim` 3) of `degree` over `knots`, clamped.
fn bspline(points: &[CPoint], degree: usize, knots: &[f64]) -> Result<BSplineCurve> {
    BSplineCurve::from_data(3, points.len() - 1, knots.len() - 1, degree, points, knots)
}
