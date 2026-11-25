//! Area-weighted plane quadrics for endpoint-only mesh reduction.
//!
//! A face contributes `area * (n · x + d)^2`, with a unit normal `n`.
//! As in Garland and Heckbert, *Surface Simplification Using Quadric Error
//! Metrics* (SIGGRAPH 1997), the sum is a symmetric homogeneous 4×4 matrix.
//! Each quadric stores its ten independent `f64` coefficients (80 bytes),
//! and merging is coefficient-wise addition; no original position is moved.
//!
//! All positions used with a set of quadrics must be expressed relative to
//! the same fixed mesh-local origin, subtracted in `f64` before construction
//! and evaluation. This keeps a large world translation out of the plane
//! offsets and the expanded polynomial. It cannot recover precision already
//! lost in the input coordinates. The score is a heuristic, not an error bound.

/// The upper triangle of the symmetric homogeneous error matrix, in the
/// order xx, xy, xz, xw, yy, yz, yw, zz, zw, ww.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Quadric([f64; 10]);

impl Quadric {
    /// Construct the area-weighted squared-distance quadric for a face.
    /// Degenerate, non-finite, underflowed or overflowed faces are refused.
    pub(super) fn from_triangle(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<Self> {
        if !a.into_iter().chain(b).chain(c).all(f64::is_finite) {
            return None;
        }
        let [ax, ay, az] = a;
        let [bx, by, bz] = b;
        let [cx, cy, cz] = c;
        let [ux, uy, uz] = [bx - ax, by - ay, bz - az];
        let [vx, vy, vz] = [cx - ax, cy - ay, cz - az];
        let [nx, ny, nz] = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
        let length = nx.hypot(ny).hypot(nz);
        let area = length * 0.5;
        if !length.is_finite() || !area.is_normal() {
            return None;
        }
        let [nx, ny, nz] = [nx / length, ny / length, nz / length];
        let d = -(nx * ax + ny * ay + nz * az);
        let q = Self([
            area * nx * nx,
            area * nx * ny,
            area * nx * nz,
            area * nx * d,
            area * ny * ny,
            area * ny * nz,
            area * ny * d,
            area * nz * nz,
            area * nz * d,
            area * d * d,
        ]);
        q.0.into_iter().all(f64::is_finite).then_some(q)
    }

    /// Sum two vertex quadrics, refusing overflow without modifying either.
    pub(super) fn merge(&self, other: &Self) -> Option<Self> {
        let mut sum = *self;
        for (value, rhs) in sum.0.iter_mut().zip(other.0) {
            *value += rhs;
            if !value.is_finite() {
                return None;
            }
        }
        Some(sum)
    }

    /// Score a collapse at the retained endpoint, or refuse non-finite
    /// arithmetic. Roundoff can make the expanded positive-semidefinite form
    /// slightly negative, so finite negative results are clamped to zero.
    pub(super) fn evaluate_pair_at(&self, other: &Self, point: [f64; 3]) -> Option<f64> {
        if !point.into_iter().all(f64::is_finite) {
            return None;
        }
        let Self([xx, xy, xz, xw, yy, yz, yw, zz, zw, ww]) = self.merge(other)?;
        let [x, y, z] = point;
        let value = xx * x * x
            + 2.0 * xy * x * y
            + 2.0 * xz * x * z
            + 2.0 * xw * x
            + yy * y * y
            + 2.0 * yz * y * z
            + 2.0 * yw * y
            + zz * z * z
            + 2.0 * zw * z
            + ww;
        value.is_finite().then_some(value.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::Quadric;

    #[test]
    fn area_weighted_distance_and_accumulation() {
        // Area 3, plane z=2; distance 4 produces 3*4² = 48.
        let q = Quadric::from_triangle([0.0, 0.0, 2.0], [3.0, 0.0, 2.0], [0.0, 2.0, 2.0]).unwrap();
        let zero = Quadric::default();
        assert_eq!(q.evaluate_pair_at(&zero, [1.0, 1.0, 2.0]), Some(0.0));
        assert_eq!(q.evaluate_pair_at(&zero, [1.0, 1.0, 6.0]), Some(48.0));
        assert_eq!(q.evaluate_pair_at(&q, [1.0, 1.0, 6.0]), Some(96.0));
        assert_eq!(
            q.merge(&q)
                .unwrap()
                .evaluate_pair_at(&zero, [1.0, 1.0, 6.0]),
            Some(96.0)
        );
        let reversed =
            Quadric::from_triangle([0.0, 0.0, 2.0], [0.0, 2.0, 2.0], [3.0, 0.0, 2.0]).unwrap();
        assert_eq!(
            reversed.evaluate_pair_at(&zero, [1.0, 1.0, 6.0]),
            Some(48.0)
        );
    }

    #[test]
    fn common_local_origin_removes_large_world_translation() {
        let origin = [1.0e12, -1.0e12, 1.0e12];
        let local = |p: [f64; 3]| [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
        let q = Quadric::from_triangle(
            local(origin),
            local([origin[0] + 3.0, origin[1], origin[2]]),
            local([origin[0], origin[1] + 2.0, origin[2]]),
        )
        .unwrap();
        assert_eq!(
            q.evaluate_pair_at(
                &Quadric::default(),
                local([origin[0], origin[1], origin[2] + 4.0])
            ),
            Some(48.0)
        );
    }

    #[test]
    fn rejects_degenerate_nonfinite_and_overflow() {
        assert!(Quadric::from_triangle([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]).is_none());
        assert!(
            Quadric::from_triangle([f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
                .is_none()
        );
        assert!(
            Quadric::from_triangle([0.0; 3], [f64::MAX, 0.0, 0.0], [0.0, f64::MAX, 0.0]).is_none()
        );
        let q = Quadric::from_triangle([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        assert!(q.evaluate_pair_at(&q, [0.0, 0.0, f64::MAX]).is_none());
        assert!(q.evaluate_pair_at(&q, [f64::INFINITY, 0.0, 0.0]).is_none());
        assert!(Quadric([f64::MAX; 10])
            .merge(&Quadric([f64::MAX; 10]))
            .is_none());
    }
}
