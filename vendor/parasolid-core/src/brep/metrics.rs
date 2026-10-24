//! Kernel-free metrics for fully mapped planar topology.

use super::model::{BodyKind, BoundingBox, BrepMetrics, BrepModel, CurveKind, SurfaceKind, Vector3};

pub(super) fn derive_metrics(model: &BrepModel) -> BrepMetrics {
    let bounding_box = bounding_box(model);
    let (surface_area, signed_volume) = planar_metrics(model).unwrap_or((None, None));
    let volume = if model.bodies.iter().all(|body| body.kind == BodyKind::Solid) { signed_volume.map(f64::abs) } else { None };
    BrepMetrics { bounding_box, surface_area, volume }
}

fn bounding_box(model: &BrepModel) -> Option<BoundingBox> {
    let mut points = model.vertices.iter().filter_map(|vertex| model.points.get(usize::try_from(vertex.point).ok()?).map(|point| point.position));
    let first = points.next()?;
    let mut minimum = first;
    let mut maximum = first;
    for point in points {
        minimum.x = minimum.x.min(point.x);
        minimum.y = minimum.y.min(point.y);
        minimum.z = minimum.z.min(point.z);
        maximum.x = maximum.x.max(point.x);
        maximum.y = maximum.y.max(point.y);
        maximum.z = maximum.z.max(point.z);
    }
    Some(BoundingBox { minimum, maximum })
}

fn planar_metrics(model: &BrepModel) -> Option<(Option<f64>, Option<f64>)> {
    if model.faces.is_empty() {
        return Some((Some(0.0), None));
    }
    // A planar support with curved trim edges is not a vertex polygon.
    if model.curves.iter().any(|curve| !matches!(curve.kind, CurveKind::Line { .. })) {
        return None;
    }
    let mut area = 0.0;
    let mut signed_volume = 0.0;
    for face in &model.faces {
        let surface = model.surfaces.get(usize::try_from(face.surface?).ok()?)?;
        let SurfaceKind::Plane { normal, .. } = surface.kind else {
            return None;
        };
        let oriented_normal = normalize(scale(normal, surface.sense.multiplier()? * face.sense.multiplier()?))?;
        let mut signed_face_area = 0.0;
        if face.loops.is_empty() {
            return None;
        }
        for loop_id in &face.loops {
            let loop_value = model.loops.get(usize::try_from(*loop_id).ok()?)?;
            let polygon = loop_value
                .half_edges
                .iter()
                .map(|half_edge_id| {
                    let half_edge = model.half_edges.get(usize::try_from(*half_edge_id).ok()?)?;
                    let vertex = model.vertices.get(usize::try_from(half_edge.vertex?).ok()?)?;
                    model.points.get(usize::try_from(vertex.point).ok()?).map(|point| point.position)
                })
                .collect::<Option<Vec<_>>>()?;
            if polygon.len() < 3 {
                return None;
            }
            let vector_area = polygon_vector_area(&polygon);
            signed_face_area += dot(vector_area, oriented_normal);
            signed_volume += polygon_signed_volume(&polygon);
        }
        area += signed_face_area.abs();
    }
    if !area.is_finite() || !signed_volume.is_finite() {
        return None;
    }
    Some((Some(area), Some(signed_volume)))
}

fn polygon_vector_area(points: &[Vector3]) -> Vector3 {
    let mut result = Vector3 { x: 0.0, y: 0.0, z: 0.0 };
    for (left, right) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
        result = add(result, scale(cross(*left, *right), 0.5));
    }
    result
}

fn polygon_signed_volume(points: &[Vector3]) -> f64 {
    let origin = points[0];
    points[1..].windows(2).map(|pair| dot(origin, cross(pair[0], pair[1])) / 6.0).sum()
}

fn normalize(value: Vector3) -> Option<Vector3> {
    let length = dot(value, value).sqrt();
    (length.is_finite() && length > 0.0).then(|| scale(value, 1.0 / length))
}

const fn add(left: Vector3, right: Vector3) -> Vector3 {
    Vector3 { x: left.x + right.x, y: left.y + right.y, z: left.z + right.z }
}

const fn scale(value: Vector3, factor: f64) -> Vector3 {
    Vector3 { x: value.x * factor, y: value.y * factor, z: value.z * factor }
}

const fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

const fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3 { x: left.y * right.z - left.z * right.y, y: left.z * right.x - left.x * right.z, z: left.x * right.y - left.y * right.x }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_oriented_polygon_area_and_volume() {
        let points = [
            Vector3 { x: 0.0, y: 0.0, z: 1.0 },
            Vector3 { x: 1.0, y: 0.0, z: 1.0 },
            Vector3 { x: 1.0, y: 1.0, z: 1.0 },
            Vector3 { x: 0.0, y: 1.0, z: 1.0 },
        ];
        assert_eq!(polygon_vector_area(&points), Vector3 { x: 0.0, y: 0.0, z: 1.0 });
        assert!((polygon_signed_volume(&points) - (1.0 / 3.0)).abs() < 1.0e-15);
    }

    #[test]
    fn normalizes_nonzero_vectors_only() {
        assert_eq!(normalize(Vector3 { x: 0.0, y: 0.0, z: 2.0 }), Some(Vector3 { x: 0.0, y: 0.0, z: 1.0 }));
        assert_eq!(normalize(Vector3 { x: 0.0, y: 0.0, z: 0.0 }), None);
    }
}
