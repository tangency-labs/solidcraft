//! Exact affine projection of spline controls and certified circular arcs.
//!
//! Not upstream: the projection construction moved from SolidCraft's document layer.
//! Rational Bézier projection preserves weights; line-image extents are certified by
//! the positive-weight control hull primitive in `fit_projection`.
//! Deviations: bounded arc subdivision (at most eight pieces); no recursion.

use crate::curve::fit_projection;
use crate::eval::Frame;
use crate::math::{Vec2, Vec3};
use crate::prism::profile::{to_local, to_world, Seg2};
use crate::prism::read::{PlanarPiece, PlanarSpline};
use crate::{Result, SolidError};

/// A projected geometric image, before an application stores its link state.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    /// An already exact native segment.
    Segment(Seg2),
    /// A point image.
    Point(Vec2),
    /// The complete extent of a linear image.
    Line(Vec2, Vec2),
    /// An exact control spline; `implicit` retains the source's uniform-knot representation.
    Spline {
        /// The exact spline used to evaluate the result.
        spline: PlanarSpline,
        /// The degree requested by the source, before a short uniform curve lowers it.
        degree: usize,
        /// Whether the source omitted its uniform knot vector.
        implicit: bool,
    },
    /// A rational quadratic conic.
    Conic {
        /// Its first point.
        a: Vec2,
        /// Its middle control point.
        apex: Vec2,
        /// Its last point.
        b: Vec2,
        /// Its sketch conic weight ratio.
        rho: f64,
    },
    /// A full ellipse.
    Ellipse {
        /// Its centre.
        center: Vec2,
        /// A point on its major axis.
        major: Vec2,
        /// Its minor radius.
        minor: f64,
    },
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("curve::conic_projection", reason)
}

fn vec3(p: [f64; 3]) -> Vec3 {
    let [x, y, z] = p;
    Vec3::new(x, y, z)
}

fn normal(f: &Frame) -> Vec3 {
    vec3(f.x_axis).cross(vec3(f.y_axis))
}

fn length2(p: Vec2) -> f64 {
    p.x.hypot(p.y)
}

fn distance2(a: Vec2, b: Vec2) -> f64 {
    length2(a - b)
}

fn normalized3(p: Vec3) -> Option<Vec3> {
    let length = p.length();
    (length > 1e-9 && length.is_finite()).then(|| p / length)
}

/// The relationship between two orthonormal sketch frames.
#[derive(Clone, Copy, Debug)]
pub struct PlaneRelation {
    /// Their normals are parallel to the app's existing classification tolerance.
    pub parallel: bool,
    /// Their projected plane has only roundoff width.
    pub edge_on: bool,
    /// The projection reverses orientation.
    pub flip: bool,
}

/// Classify two sketch planes without changing their frames or coordinates.
pub fn plane_relation(source: &Frame, target: &Frame) -> PlaneRelation {
    let a = normal(source);
    let b = normal(target);
    let dot = a.dot(b);
    PlaneRelation {
        parallel: a.cross(b).length() < 1e-9,
        edge_on: dot.abs() <= f64::EPSILON * 64.0,
        flip: dot < 0.0,
    }
}

/// Affinely project points between the two plane frames.
pub fn project_points(source: &Frame, target: &Frame, points: &[Vec2]) -> Vec<Vec2> {
    points
        .iter()
        .map(|p| to_local(target, to_world(source, *p, 0.0)))
        .collect()
}

/// The full image of an analytic ellipse. A rank-deficient image is left to the
/// exact control-curve classifier, which also keeps its complete linear extent.
pub(crate) fn project_ellipse(
    source: &Frame,
    radii: [f64; 2],
    target: &Frame,
    max_coord: f64,
) -> Result<Option<Vec<Projection>>> {
    let x = vec3(source.x_axis) * radii[0];
    let y = vec3(source.y_axis) * radii[1];
    let tx = vec3(target.x_axis);
    let ty = vec3(target.y_axis);
    let u = Vec2::new(x.dot(tx), x.dot(ty));
    let v = Vec2::new(y.dot(tx), y.dot(ty));
    let scale = length2(u).max(length2(v));
    if !(scale > 0.0 && scale.is_finite()) {
        return Ok(None);
    }
    let (u, v) = (u / scale, v / scale);
    let xx = u.x * u.x + v.x * v.x;
    let yy = u.y * u.y + v.y * v.y;
    let xy = u.x * u.y + v.x * v.y;
    let large = ((xx + yy + (xx - yy).hypot(2.0 * xy)) * 0.5).sqrt();
    let small = (u.x * v.y - u.y * v.x).abs() / large;
    let major_radius = scale * large;
    let minor = scale * small;
    if !(minor > major_radius * f64::EPSILON * 128.0 && minor > 1e-7) {
        return Ok(None);
    }
    let angle = (2.0 * xy).atan2(xx - yy) * 0.5;
    let center = to_local(target, vec3(source.origin));
    let coordinate_roundoff =
        (vec3(source.origin).length() + vec3(target.origin).length() + major_radius)
            * f64::EPSILON
            * 128.0;
    if minor <= coordinate_roundoff {
        return Ok(None);
    }
    let major = center + Vec2::new(angle.cos(), angle.sin()) * major_radius;
    if !(max_coord.is_finite()
        && max_coord > 0.0
        && center.x.abs() + scale * u.x.hypot(v.x) < max_coord
        && center.y.abs() + scale * u.y.hypot(v.y) < max_coord
        && major.x.is_finite()
        && major.y.is_finite())
    {
        return Err(invalid("projected ellipse exceeds sketch limits"));
    }
    Ok(Some(vec![Projection::Ellipse {
        center,
        major,
        minor,
    }]))
}

/// Coordinate arithmetic scale for a projection, taken before source spline conversion.
pub fn projection_scale(source: &Frame, target: &Frame, points: &[Vec2]) -> f64 {
    points.iter().fold(
        vec3(source.origin).length() + vec3(target.origin).length(),
        |s, p| s.max(length2(*p)),
    )
}

/// Project a control spline between planes, including the arithmetic scale of both origins.
pub fn project_control_between(
    source: &Frame,
    target: &Frame,
    points: &[Vec2],
    degree: usize,
    metadata: Option<(&[f64], &[f64])>,
    collapse: bool,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let scale = projection_scale(source, target, points);
    classify_control(
        project_points(source, target, points),
        degree,
        metadata,
        scale,
        collapse,
        max_coord,
    )
}

/// Preserve a control spline, or certify its point/line image and complete extent.
///
/// A rank collapse is accepted only within a coordinate-roundoff budget, never a
/// modelling tolerance. The scalar range is the whole-curve certified range.
pub fn classify_control(
    points: Vec<Vec2>,
    degree: usize,
    metadata: Option<(&[f64], &[f64])>,
    world_scale: f64,
    collapse: bool,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let representable = |p: &Vec2| {
        p.x.is_finite() && p.y.is_finite() && p.x.abs() < max_coord && p.y.abs() < max_coord
    };
    if points.iter().any(|p| !representable(p)) {
        return Err(invalid(
            "exact projected spline coordinates exceed sketch limits",
        ));
    }
    let implicit = metadata.is_none();
    let (evaluation_degree, knots, weights) = if let Some((knots, weights)) = metadata {
        (degree, knots.to_vec(), weights.to_vec())
    } else {
        let (d, k) = fit_projection::uniform_knots(points.len(), degree)?;
        (d, k, Vec::new())
    };
    fit_projection::check_control(
        points.len(),
        evaluation_degree,
        &knots,
        &weights,
        points.len(),
    )?;
    if collapse {
        let scale = points
            .iter()
            .fold(world_scale.max(1.0), |s, p| s.max(length2(*p)));
        if let Some([a, b]) = fit_projection::linear_image(
            &points,
            evaluation_degree,
            &knots,
            &weights,
            scale * f64::EPSILON * 64.0,
        )? {
            if !representable(&a) || !representable(&b) {
                return Err(invalid(
                    "exact projected spline extent exceeds sketch limits",
                ));
            }
            return Ok(vec![line_or_point(a, b)]);
        }
    }
    Ok(vec![Projection::Spline {
        spline: PlanarSpline {
            degree: evaluation_degree,
            knots,
            points,
            weights,
        },
        degree,
        implicit,
    }])
}

/// Classify a spline whose coordinates are already in the target plane.
pub fn classify_control_at_plane(
    target: &Frame,
    spline: PlanarSpline,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    classify_control(
        spline.points,
        spline.degree,
        Some((&spline.knots, &spline.weights)),
        vec3(target.origin).length(),
        true,
        max_coord,
    )
}

/// Classify exact native pieces after splitting any oversized control spline.
pub fn classify_pieces(
    target: &Frame,
    pieces: &[PlanarPiece],
    limit: usize,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            PlanarPiece::Spline(s) => {
                if !(1..=9).contains(&s.degree) {
                    return Err(invalid(
                        "exact projection of spline degrees above 9 is not supported",
                    ));
                }
                for p in crate::curve::spline_projection::split_planar(s, limit)? {
                    out.extend(classify_control_at_plane(target, p, max_coord)?);
                }
            }
            PlanarPiece::Seg(Seg2::Line { a, b }) => out.push(line_or_point(*a, *b)),
            PlanarPiece::Seg(Seg2::Cubic { p0, p1, p2, p3 }) => out.extend(classify_control(
                vec![*p0, *p1, *p2, *p3],
                3,
                None,
                vec3(target.origin).length(),
                true,
                max_coord,
            )?),
            PlanarPiece::Seg(Seg2::Bezier { n, p }) => {
                let points = p
                    .get(..=usize::from(*n))
                    .ok_or_else(|| invalid("invalid projected Bezier degree"))?
                    .to_vec();
                out.extend(classify_control(
                    points,
                    usize::from(*n),
                    None,
                    vec3(target.origin).length(),
                    true,
                    max_coord,
                )?);
            }
            PlanarPiece::Seg(s) => out.push(Projection::Segment(*s)),
        }
    }
    Ok(coalesce_linear(out))
}

/// Keep a representable line, or its single point when it is below the sketch's minimum length.
pub fn line_or_point(a: Vec2, b: Vec2) -> Projection {
    if distance2(a, b) <= 1e-7 {
        Projection::Point(a)
    } else {
        Projection::Line(a, b)
    }
}

/// Keep a single source's complete linear image once, including retracing and split curves.
pub fn coalesce_linear(geometry: Vec<Projection>) -> Vec<Projection> {
    if geometry.len() < 2
        || geometry
            .iter()
            .any(|g| !matches!(g, Projection::Point(_) | Projection::Line(..)))
    {
        return geometry;
    }
    let points: Vec<_> = geometry
        .iter()
        .flat_map(|g| match g {
            Projection::Point(p) => vec![*p],
            Projection::Line(a, b) => vec![*a, *b],
            _ => Vec::new(),
        })
        .collect();
    let Some(a) = points.first().copied() else {
        return geometry;
    };
    let far = points
        .iter()
        .copied()
        .max_by(|p, q| distance2(*p, a).total_cmp(&distance2(*q, a)))
        .unwrap_or(a);
    let delta = far - a;
    let len = length2(delta);
    if !len.is_finite() || len <= 1e-9 {
        return vec![Projection::Point(a)];
    }
    let axis = Vec2::new(delta.x / len, delta.y / len);
    let tol = points.iter().fold(1.0_f64, |s, p| s.max(length2(*p))) * f64::EPSILON * 128.0;
    if points.iter().any(|p| {
        let d = *p - a;
        (axis.x * d.y - axis.y * d.x).abs() > tol
    }) {
        return geometry;
    }
    let range = points
        .iter()
        .map(|p| (*p - a).dot(axis))
        .fold([0.0_f64; 2], |[lo, hi], x| [lo.min(x), hi.max(x)]);
    let [lo, hi] = range;
    if hi - lo <= 1e-7 {
        vec![Projection::Point(a)]
    } else {
        vec![Projection::Line(a + axis * lo, a + axis * hi)]
    }
}

/// A candidate source plane through samples of a circular edge.
///
/// This proposal does not certify a circle or decide its interval. The caller must
/// certify the native curve in the returned frame before projecting its arcs.
pub fn circle_plane(points: &[Vec3]) -> Option<Frame> {
    let n = points.len();
    if n < 4 {
        return None;
    }
    let first = *points.first()?;
    let last = *points.last()?;
    let closed = first.distance(last) <= 1e-9 * first.length().max(1.0);
    let (a, b, c) = if closed {
        (
            first,
            *points.get(n.checked_div(3)?)?,
            *points.get(n.checked_mul(2)?.checked_div(3)?)?,
        )
    } else {
        (first, *points.get(n.checked_div(2)?)?, last)
    };
    let (ab, ac) = (b - a, c - a);
    let nrm = ab.cross(ac);
    let l2 = nrm.length_squared();
    if l2 < 1e-24 {
        return None;
    }
    let center = a
        + (nrm.cross(ab) * ac.length_squared() + ac.cross(nrm) * ab.length_squared()) / (2.0 * l2);
    let radius = center.distance(a);
    let normal = normalized3(nrm)?;
    let tol = 1e-7 * radius.max(1.0);
    if !points
        .iter()
        .all(|p| (p.distance(center) - radius).abs() < tol && (*p - center).dot(normal).abs() < tol)
    {
        return None;
    }
    let delta = first - center;
    if delta.length() <= 1e-9 {
        return None;
    }
    let x = normalized3(delta)?;
    let y = normal.cross(x);
    Some(Frame {
        origin: [center.x, center.y, center.z],
        x_axis: [x.x, x.y, x.z],
        y_axis: [y.x, y.y, y.z],
    })
}

/// Project certified circular arcs as ellipses or rational quadratic conics.
/// Only the certificate's centre, radius and selected sweep determine the result.
pub fn project_arcs(
    target: &Frame,
    source: &Frame,
    certified: &[PlanarPiece],
    max_coord: f64,
) -> Result<Option<Vec<Projection>>> {
    let Some(arcs): Option<Vec<_>> = certified
        .iter()
        .map(|p| match p {
            PlanarPiece::Seg(Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            }) => Some((*center, *radius, *start, *sweep)),
            _ => None,
        })
        .collect()
    else {
        return Ok(None);
    };
    let Some(&(c, r, _, _)) = arcs.first() else {
        return Ok(None);
    };
    let sweep: f64 = arcs.iter().map(|a| a.3).sum();
    let cosang = normal(source).dot(normal(target)).abs().min(1.0);
    if arcs.iter().all(|a| a.0 == c && a.1 == r)
        && (sweep.abs() - std::f64::consts::TAU).abs() <= f64::EPSILON * 64.0
        && r * cosang > 1e-9
    {
        let world = to_world(source, c, 0.0);
        let cross = normal(source).cross(normal(target));
        let major = normalized3(cross).unwrap_or_else(|| vec3(source.x_axis));
        return Ok(Some(vec![Projection::Ellipse {
            center: to_local(target, world),
            major: to_local(target, world + major * r),
            minor: r * cosang,
        }]));
    }
    let mut out = Vec::new();
    for (center, radius, start, sweep) in arcs {
        if !sweep.is_finite() || !radius.is_finite() {
            return Err(invalid("invalid circular projection parameters"));
        }
        let count = (sweep.abs() / std::f64::consts::FRAC_PI_2)
            .ceil()
            .clamp(1.0, 8.0) as u32;
        let step = sweep / f64::from(count);
        let w = (step / 2.0).cos();
        let at = |angle: f64, scale: f64| {
            to_local(
                target,
                to_world(
                    source,
                    center + Vec2::new(angle.cos(), angle.sin()) * scale,
                    0.0,
                ),
            )
        };
        for i in 0..count {
            let a = start + step * f64::from(i);
            let b = start + step * f64::from(i + 1);
            let points = vec![at(a, radius), at((a + b) / 2.0, radius / w), at(b, radius)];
            let g = classify_control(
                points.clone(),
                2,
                Some((&[0., 0., 0., 1., 1., 1.], &[1., w, 1.])),
                vec3(source.origin).length() + vec3(target.origin).length(),
                true,
                max_coord,
            )?;
            if matches!(g.as_slice(), [Projection::Spline { .. }])
                && points
                    .iter()
                    .enumerate()
                    .all(|(i, p)| points.iter().skip(i + 1).all(|q| distance2(*p, *q) > 1e-7))
            {
                let [Some(a), Some(apex), Some(b)] = [points.first(), points.get(1), points.get(2)]
                else {
                    return Err(invalid("invalid conic controls"));
                };
                out.push(Projection::Conic {
                    a: *a,
                    apex: *apex,
                    b: *b,
                    rho: w / (1.0 + w),
                });
            } else {
                out.extend(g);
            }
        }
    }
    Ok(Some(coalesce_linear(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(origin: [f64; 3], x_axis: [f64; 3], y_axis: [f64; 3]) -> Frame {
        Frame {
            origin,
            x_axis,
            y_axis,
        }
    }

    #[test]
    fn a_short_certified_arc_far_from_the_origin_keeps_its_selected_sweep() {
        let source = frame([1e6, 0., 0.], [1., 0., 0.], [0., 1., 0.]);
        let target = frame([1e6, 0., 0.], [0.8, 0., 0.6], [0., 1., 0.]);
        let arc = PlanarPiece::Seg(Seg2::Arc {
            center: Vec2::ZERO,
            radius: 1.,
            start: 0.,
            sweep: 0.0005,
        });
        let projected = project_arcs(&target, &source, &[arc], 1e9)
            .unwrap()
            .unwrap();
        assert_eq!(projected.len(), 1);
        let points = match &projected[0] {
            Projection::Conic { a, apex, b, .. } => vec![*a, *apex, *b],
            Projection::Spline { spline, .. } => spline.points.clone(),
            Projection::Line(a, b) => vec![*a, *b],
            other => panic!("a tiny selected arc must not become a full ellipse: {other:?}"),
        };
        let expected =
            |t: f64| to_local(&target, to_world(&source, Vec2::new(t.cos(), t.sin()), 0.));
        assert!(points.first().unwrap().distance(expected(0.)) < 1e-7);
        assert!(points.last().unwrap().distance(expected(0.0005)) < 1e-7);
        assert!(points.iter().all(|p| p.distance(expected(0.)) < 0.001));
    }

    #[test]
    fn a_full_certified_circle_projects_to_an_ellipse() {
        let source = frame([0., 0., 0.], [1., 0., 0.], [0., 1., 0.]);
        let target = frame([0., 0., 0.], [0.8, 0., 0.6], [0., 1., 0.]);
        let arc = PlanarPiece::Seg(Seg2::Arc {
            center: Vec2::ZERO,
            radius: 5.,
            start: 0.,
            sweep: std::f64::consts::TAU,
        });
        let projected = project_arcs(&target, &source, &[arc], 1e9)
            .unwrap()
            .unwrap();
        assert!(
            matches!(projected.as_slice(), [Projection::Ellipse { center, major, minor }] if *center == Vec2::ZERO && (major.length() - 5.).abs() < 1e-14 && (*minor - 4.).abs() < 1e-14)
        );
    }

    #[test]
    fn a_retraced_linear_image_keeps_its_complete_extent_once() {
        let projected = coalesce_linear(vec![
            Projection::Line(Vec2::new(1., 0.), Vec2::new(4., 0.)),
            Projection::Line(Vec2::new(4., 0.), Vec2::new(-2., 0.)),
        ]);
        assert_eq!(
            projected,
            vec![Projection::Line(Vec2::new(4., 0.), Vec2::new(-2., 0.))]
        );
    }

    #[test]
    fn representability_is_checked_before_a_projection_is_stored() {
        let pts = vec![Vec2::new(1e10, 0.), Vec2::new(1e10, 5.)];
        assert!(classify_control(pts, 1, None, 1e10, true, 1e9).is_err());
    }
}
