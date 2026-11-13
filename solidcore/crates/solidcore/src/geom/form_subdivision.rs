//! Coordinate stencils used by an editable Catmull–Clark cage.
use crate::eval::vec::VecExt;
use crate::Vec3;

/// Limit point of a corner along a boundary or a pair of sharp edges.
pub fn crease_limit(point: Vec3, [a, b]: [Vec3; 2]) -> Vec3 {
    (point * 4.0 + a + b) / 6.0
}

/// Limit point of a smooth quad fan. Supply radial and opposite vertices
/// in source order so the stencil keeps the evaluator's rounding order.
pub fn smooth_limit(
    point: Vec3,
    neighbors: impl IntoIterator<Item = Vec3>,
    diagonals: impl IntoIterator<Item = Vec3>,
    valence: usize,
) -> Vec3 {
    let neighbors = neighbors.into_iter().fold(Vec3::ZERO, |sum, p| sum + p);
    let diagonals = diagonals.into_iter().fold(Vec3::ZERO, |sum, p| sum + p);
    let n = valence as f64;
    (point * (n * n) + neighbors * 4.0 + diagonals) / (n * (n + 5.0))
}

/// Whether a semi-sharp corner's successive values have converged.
pub fn converged_limit(current: Vec3, previous: Vec3) -> bool {
    current.distance(previous) < 1e-12 * current.length().max(1.0)
}

/// Uniform cubic B-spline span to Bézier in both directions, U fastest.
pub fn regular_bezier(net: [Vec3; 16]) -> [Vec3; 16] {
    let weights = [
        [1.0, 4.0, 1.0, 0.0],
        [0.0, 4.0, 2.0, 0.0],
        [0.0, 2.0, 4.0, 0.0],
        [0.0, 1.0, 4.0, 1.0],
    ];
    let mut output = [Vec3::ZERO; 16];
    for (point, (u, v)) in output.iter_mut().zip(
        weights
            .into_iter()
            .flat_map(|v| weights.into_iter().map(move |u| (u, v))),
    ) {
        let product = v
            .into_iter()
            .flat_map(|v| u.into_iter().map(move |u| u * v / 36.0));
        for (control, weight) in net.into_iter().zip(product) {
            *point += control * weight;
        }
    }
    output
}

/// Project displacement onto its reflection plane and refuse a zero or nonfinite offset.
pub fn mirror_tangent(delta: Vec3, axis: usize) -> Option<Vec3> {
    let mut projected = delta.to_array();
    *projected.get_mut(axis)? = 0.0;
    let result = Vec3::from_array(projected);
    (delta.is_finite() && result.len() >= 1e-9).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_planar_net_converts_to_uniform_bezier_coordinates() {
        let net = std::array::from_fn(|i| Vec3::new((i % 4) as f64, (i / 4) as f64, 5.0));
        for (i, p) in regular_bezier(net).into_iter().enumerate() {
            let expected = Vec3::new(1.0 + (i % 4) as f64 / 3.0, 1.0 + (i / 4) as f64 / 3.0, 5.0);
            assert!(p.distance(expected) < 1e-14);
        }
        assert_eq!(
            mirror_tangent(Vec3::new(6.0, 2.0, -5.0), 0),
            Some(Vec3::new(0.0, 2.0, -5.0))
        );
        assert_eq!(mirror_tangent(Vec3::X, 0), None);
    }
}
