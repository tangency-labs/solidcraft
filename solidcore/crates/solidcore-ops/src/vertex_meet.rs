//! Whether two tolerant points are one point ([`solidcore::query::vertex_meet::points_meet`]),
//! on record points, so a reader joining two vertices written at one corner names no kernel
//! type. Only the call: the test and the joined tolerance are the kernel's.

use solidcore::math::Point3;

/// Whether the tolerant point `a` (tolerance `a_tol`) and `b` (tolerance `b_tol`) are one, and
/// if so the tolerance `a` takes as that one point (its ball then holds `b`'s); `None` when they
/// are apart or a number is not finite.
#[must_use]
pub fn points_meet(a: [f64; 3], a_tol: f64, b: [f64; 3], b_tol: f64) -> Option<f64> {
    solidcore::query::vertex_meet::points_meet(
        Point3::from_array(a),
        a_tol,
        Point3::from_array(b),
        b_tol,
    )
}
