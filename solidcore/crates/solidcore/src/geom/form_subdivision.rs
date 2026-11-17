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

/// Coordinate geometry of a derived ruled trim-wall preview.
pub struct TrimWallGeometry {
    /// Translated sampled boundary, in the original directed order.
    pub top: Vec<Vec3>,
    /// Unit normals in reverse sampled-boundary order, as used by the ruled strips.
    pub normals: Vec<Vec3>,
    /// Translated logical endpoint controls.
    pub controls: [Vec3; 2],
}

/// Validate a nonzero derived-wall displacement within the Form extent.
pub fn validate_trim_wall_delta(delta: Vec3) -> crate::Result<()> {
    if !delta.is_finite()
        || delta.length() < 1e-9
        || delta.x.abs().max(delta.y.abs()).max(delta.z.abs()) > 200_000.0
    {
        return Err(crate::SolidError::invalid_input(
            "Form trim wall",
            "trim wall needs a finite nonzero displacement within the Form bounds",
        ));
    }
    Ok(())
}

/// Add an editing displacement and validate before publishing the new wall.
pub fn move_trim_wall(delta: Vec3, offset: Vec3) -> crate::Result<Vec3> {
    let moved = delta + offset;
    validate_trim_wall_delta(moved)?;
    Ok(moved)
}

/// Build bounded ruled-wall positions and normals, refusing an along-seam displacement.
pub fn trim_wall(
    base: &[Vec3],
    controls: [Vec3; 2],
    delta: Vec3,
) -> crate::Result<TrimWallGeometry> {
    let bad = |reason| crate::SolidError::invalid_input("Form trim wall", reason);
    validate_trim_wall_delta(delta)?;
    if !(2..=1_000_000).contains(&base.len()) {
        return Err(bad("trim preview path needs bounded stations"));
    }
    let bounded = |p: &Vec3| p.is_finite() && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= 100_000.0;
    let top: Vec<_> = base.iter().map(|&p| p + delta).collect();
    if base.iter().chain(&top).any(|p| !bounded(p)) {
        return Err(bad("trim wall positions must remain within 100000 mm"));
    }
    let controls = controls.map(|p| p + delta);
    if controls.iter().any(|p| !bounded(p)) {
        return Err(bad("trim wall controls must remain within 100000 mm"));
    }
    let normals = (0..base.len())
        .map(|i| {
            let next = base.len() - 1 - i.saturating_sub(1);
            let prev = base.len() - 1 - (i + 1).min(base.len() - 1);
            let a = *base.get(prev).ok_or_else(|| bad("invalid trim station"))?;
            let b = *base.get(next).ok_or_else(|| bad("invalid trim station"))?;
            (a - b)
                .cross(delta)
                .normalized()
                .ok_or_else(|| bad("trim wall displacement is along its seam"))
        })
        .collect::<crate::Result<Vec<_>>>()?;
    Ok(TrimWallGeometry {
        top,
        normals,
        controls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_wall_checks_all_geometry_before_returning_directed_strips() {
        let base = [Vec3::ZERO, Vec3::X, Vec3::X * 2.0];
        let wall = trim_wall(&base, [Vec3::ZERO, Vec3::X * 2.0], Vec3::Z).unwrap();
        assert_eq!(
            wall.top,
            vec![Vec3::Z, Vec3::X + Vec3::Z, Vec3::X * 2.0 + Vec3::Z]
        );
        assert_eq!(wall.controls, [Vec3::Z, Vec3::X * 2.0 + Vec3::Z]);
        assert!(wall
            .normals
            .iter()
            .all(|normal| normal.distance(Vec3::Y) < 1e-12));
        assert!(trim_wall(&base, [Vec3::ZERO, Vec3::X], Vec3::X).is_err());
        assert!(trim_wall(&base, [Vec3::ZERO, Vec3::new(100_000.0, 0.0, 0.0)], Vec3::X).is_err());
        assert!(move_trim_wall(Vec3::Z, -Vec3::Z).is_err());
    }

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
