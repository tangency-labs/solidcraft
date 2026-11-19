//! Portable data adapter for exact support-based planar-face replacement.
use solidcore::eval::plan::EvalSurface;
use solidcore::geom::form_surface_replace as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::{Plane, Vec3};

fn plane(plane: &Plane) -> kernel::HalfSpace {
    kernel::HalfSpace::from_axes(vec3(plane.origin), vec3(plane.x), vec3(plane.y))
}

/// Cartesian controls of a bounded positive-weight hull.
pub fn points(controls: &[[f64; 4]]) -> Option<Vec<Vec3>> {
    kernel::points(controls).map(|points| points.into_iter().map(record_vec3).collect())
}
/// Full support height bound for a native evaluator.
pub fn maximum_height(
    surface: &EvalSurface,
    boundary: &[[f64; 4]],
    support: &Plane,
) -> Option<f64> {
    kernel::maximum_height(surface, boundary, plane(support))
}
/// Supported open hull of a native evaluator.
pub fn hull(surface: &EvalSurface, boundary: &[[f64; 4]]) -> Option<Vec<Vec3>> {
    kernel::hull(surface, boundary).map(|points| points.into_iter().map(record_vec3).collect())
}
/// All hull coefficients stay in the supplied half-spaces.
pub fn inside(hull: &[Vec3], planes: &[Plane], tolerance: f64) -> bool {
    kernel::inside(
        &hull.iter().copied().map(vec3).collect::<Vec<_>>(),
        &planes.iter().map(plane).collect::<Vec<_>>(),
        tolerance,
    )
}
/// All hull coefficients lie on one plane.
pub fn coplanar(hull: &[Vec3], support: &Plane, tolerance: f64) -> bool {
    kernel::coplanar(
        &hull.iter().copied().map(vec3).collect::<Vec<_>>(),
        plane(support),
        tolerance,
    )
}
/// Every span leaves every contact plane.
pub fn strictly_inside(
    surface: &EvalSurface,
    hull: &[Vec3],
    planes: &[Plane],
    epsilon: f64,
) -> bool {
    kernel::strictly_inside(
        surface,
        &hull.iter().copied().map(vec3).collect::<Vec<_>>(),
        &planes.iter().map(plane).collect::<Vec<_>>(),
        epsilon,
    )
}
/// Scale-aware qualification epsilon.
pub fn epsilon(size: f64, relative: f64) -> f64 {
    kernel::epsilon(size, relative)
}
/// The same support with reversed orientation.
pub fn opposite(support: Plane) -> Plane {
    Plane {
        y: record_vec3(kernel::reverse_axis(vec3(support.y))),
        ..support
    }
}
