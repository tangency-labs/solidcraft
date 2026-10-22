//! The area vector of a loop of points (Newell's method): its normal and the area it encloses.

use solidcraft_geom::Vec3;

/// The area vector of a closed polygon (Newell's method): normal to the loop, as long as the area
/// it encloses, pointing the way the loop runs round by the right-hand rule.
pub(crate) fn loop_area(points: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for (i, p) in points.iter().enumerate() {
        let Some(q) = points.get((i + 1) % points.len()) else { continue };
        n += Vec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y));
    }
    n * 0.5
}
