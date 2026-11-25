//! Constraint recipes over the kernel's derivative-aware geometric primitives.
//! The caller owns operand lookup and constraint state; ops only selects and converts.

use solidcore::eval::constraint as k;
use solidcore_record::{Plane, Vec2, Vec3};

pub use k::{
    circumcenter, circumradius, plane_angle, plane_angle_residual, plane_distance, plane_reversed,
    size, unit, weight, Dual, Num, N, V,
};

/// A constant solver vector from a record point or direction.
pub fn of<T: Num>(p: Vec3) -> V<T> {
    V::of([p.x, p.y, p.z])
}

/// Point-on-curve residuals: two operands define a line, three a circle.
/// Malformed operand lists return a nonfinite residual for the solver to reject.
pub fn point_on_curve<T: Num>(p: V<T>, curve: &[V<T>]) -> Vec<T> {
    match curve {
        [a, b] => k::point_on_line(p, *a, *b).to_vec(),
        [a, b, c] => k::point_on_circle(p, *a, *b, *c).to_vec(),
        _ => vec![T::num(f64::NAN)],
    }
}

/// The three solver operands of a circle in the given orthonormal frame.
pub fn circle_points(c: Vec3, x: Vec3, y: Vec3, radius: f64, from: f64) -> [Vec3; 3] {
    k::circle_points(of(c), of(x), of(y), radius, from).map(|[x, y, z]| Vec3::new(x, y, z))
}

/// The three solver operands of a 2D arc's circle through its sketch frame.
pub fn arc_circle_points(c: [f64; 2], start: [f64; 2], plane: &Plane) -> [Vec3; 3] {
    k::arc_circle_points(c, start, &solidcore_convert::plane(plane))
        .map(|[x, y, z]| Vec3::new(x, y, z))
}

/// Whether two planes lie on the same support, independent of their orientation.
pub fn same_plane(a: &Plane, b: &Plane) -> bool {
    k::same_plane(&solidcore_convert::plane(a), &solidcore_convert::plane(b))
}

/// The normal of an orthonormal plane.
pub fn plane_normal(plane: &Plane) -> Vec3 {
    let [x, y, z] = k::plane_normal(&solidcore_convert::plane(plane));
    Vec3::new(x, y, z)
}

/// A point of the sketch through its orthonormal plane.
pub fn plane_point(plane: &Plane, point: Vec2) -> Vec3 {
    let [x, y, z] = k::plane_point(&solidcore_convert::plane(plane), [point.x, point.y]);
    Vec3::new(x, y, z)
}
