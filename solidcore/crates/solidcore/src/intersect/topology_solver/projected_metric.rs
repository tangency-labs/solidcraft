//! Interval enclosures in the projected queries' double-cross metric.
//!
//! A projected query measures a difference `V` as `|N x V x N|`, which is
//! `(N . N) |P(V)|` for the orthogonal projection `P` onto the plane normal to
//! `N`. `N` need not be unit length, so tree bounds must use the same scaled
//! metric as the answers they are compared with.
//!
//! Upstream: none (replaces the sphere tests in `SmTopologySolver::MayContainAnswer`
//! for projected and directed operations).
//!
//! Deviations: the upstream projected test compared an unscaled sphere distance with
//! the scaled answers and applied no tolerance (WR-766); the upstream directed test
//! measured the in-plane offset instead of the distance to the direction plane (WR-772).

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::geom::extent3d::Extent3d;
use crate::math::Vec3;

/// The interval cross product `a x b`.
fn cross(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// `N x V x N`, the projected difference in the queries' metric.
fn projected(v: V3<Ivl>, normal: V3<Ivl>) -> V3<Ivl> {
    cross(cross(normal, v), normal)
}

/// The interval square `x^2`. It uses `mig^2 ..= mag^2` because `x * x` on an
/// interval that holds 0 gives a negative lower end.
fn square(x: Ivl) -> Ivl {
    Ivl::checked(x.mig() * x.mig(), x.mag() * x.mag())
}

/// The interval squared length `|v|^2`.
fn norm_squared(v: V3<Ivl>) -> Ivl {
    (square(v.x) + square(v.y)) + square(v.z)
}

/// The interval box of differences `b - a` over every point of `a` and of `b`, or
/// `None` for a non-finite or inverted box (which can never exclude a branch).
fn box_difference(a: &Extent3d, b: &Extent3d) -> Option<V3<Ivl>> {
    if [a, b].iter().any(|extent| {
        let (lo, hi) = (extent.min(), extent.max());
        ![lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]
            .iter()
            .all(|x| x.is_finite())
            || lo.x > hi.x
            || lo.y > hi.y
            || lo.z > hi.z
    }) {
        return None;
    }
    Some(V3 {
        x: Ivl::new(b.min().x, b.max().x) - Ivl::new(a.min().x, a.max().x),
        y: Ivl::new(b.min().y, b.max().y) - Ivl::new(a.min().y, a.max().y),
        z: Ivl::new(b.min().z, b.max().z) - Ivl::new(a.min().z, a.max().z),
    })
}

/// A conservative range of the projected distance between any point of `a` and
/// any point of `b`, in the double-cross metric. A non-finite or inverted box
/// gives the whole line, which can never exclude a branch.
pub(super) fn projected_box_distance(a: &Extent3d, b: &Extent3d, normal: Vec3) -> Ivl {
    match box_difference(a, b) {
        Some(delta) => norm_squared(projected(delta, V3::from_vec3(normal))).sqrt(),
        None => Ivl::whole(),
    }
}

/// Bounds for a directed query between two boxes. A directed answer is a
/// difference whose projection `Q = N x d x N` lies within `ang_tol_deg` of the
/// direction (either sense); its value is `|Q|`, signed by `Q . direction`.
pub(super) struct DirectedRange {
    /// Encloses `|Q|` over every pair of box points.
    pub(super) distance: Ivl,
    /// False only when no pair of box points can be within the angle.
    pub(super) admissible: bool,
}

/// [`DirectedRange`] for boxes `a` and `b`, or `None` for a box that cannot be
/// bounded (the caller keeps the branch).
pub(super) fn directed_box_range(
    a: &Extent3d,
    b: &Extent3d,
    normal: Vec3,
    direction: Vec3,
    ang_tol_deg: f64,
) -> Option<DirectedRange> {
    let delta = box_difference(a, b)?;
    let dir = V3::from_vec3(direction);
    let q = projected(delta, V3::from_vec3(normal));
    let q_sq = norm_squared(q);
    // Within the angle means |Q x dir|^2 <= sin^2(angle) |Q|^2 |dir|^2. The
    // sine is rounded up so the limit can only admit more.
    let sin = ang_tol_deg.to_radians().sin() * (1.0 + 1e-12);
    let limit = Ivl::from(sin * sin) * q_sq * norm_squared(dir);
    let admissible = norm_squared(cross(q, dir)).lo <= limit.hi;
    Some(DirectedRange {
        distance: q_sq.sqrt(),
        admissible,
    })
}
