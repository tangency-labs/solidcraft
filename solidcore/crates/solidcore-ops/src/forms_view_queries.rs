//! Record-facing viewport queries for the Form editor.

use solidcore::eval::forms_view_queries as kernel;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use kernel::{Projection, MAX_CURVES};

/// Curve bounds, in source order, for a projected pick query.
pub struct BoundsIndex(kernel::BoundsIndex);

impl BoundsIndex {
    /// Build a projected query index from the lower and upper bounds of each source.
    pub fn new(bounds: impl IntoIterator<Item = [Vec3; 2]>) -> Option<Self> {
        let bounds = bounds
            .into_iter()
            .take(MAX_CURVES + 1)
            .map(|[min, max]| kernel::Bounds {
                min: vec3(min),
                max: vec3(max),
            })
            .collect();
        kernel::BoundsIndex::new(bounds).map(Self)
    }

    /// Indices whose projected bounds lie within the pointer's pick radius.
    pub fn candidates(
        &self,
        projection: Projection,
        pixel: [f32; 2],
        radius: f32,
        limit: usize,
    ) -> Option<Vec<usize>> {
        self.0.candidates(projection, pixel, radius, limit)
    }
}

/// Projected hits in distance order, as (squared screen distance, fraction, source point).
pub fn curve_hits(
    projection: Projection,
    pixel: [f32; 2],
    radius: f32,
    evaluate: impl Fn(f64) -> Option<Vec3>,
) -> impl Iterator<Item = (f32, f64, Vec3)> {
    kernel::curve_hits(projection, pixel, radius, |fraction| {
        evaluate(fraction).map(vec3)
    })
    .into_iter()
    .map(|hit| (hit.distance2, hit.fraction, record_vec3(hit.point)))
}

/// Squared screen distance and the world witness for the projected segment's nearest point.
pub fn segment_hit(
    projection: Projection,
    pixel: [f32; 2],
    segment: [Vec3; 2],
) -> Option<(f32, Vec3)> {
    kernel::segment_hit(projection, pixel, segment.map(vec3))
        .map(|hit| (hit.distance2, record_vec3(hit.point)))
}

/// Depth and ray geometry for occlusion of an exact source point.
pub struct Visibility(kernel::Visibility);

impl Visibility {
    /// Test a hit against the point's own display surface.
    pub fn own_surface_visible(&self, hit: Option<f64>) -> bool {
        kernel::own_surface_visible(self.0.depth, self.0.tolerance, hit)
    }

    /// Exact-point ray and depth, used with independently displayed context bodies.
    pub fn context_ray(&self) -> (Vec3, Vec3, f64) {
        (
            record_vec3(self.0.origin),
            record_vec3(self.0.direction),
            self.0.context_depth,
        )
    }
}

/// Prepare visibility tests using the original ray and the origin of a nearby screen ray.
pub fn visibility(
    point: Vec3,
    origin: Vec3,
    direction: Vec3,
    nearby: Vec3,
    fit: f64,
    perspective: bool,
) -> Option<Visibility> {
    kernel::visibility(
        vec3(point),
        vec3(origin),
        vec3(direction),
        vec3(nearby),
        fit,
        perspective,
    )
    .map(Visibility)
}

/// Whether a context hit precedes the point by more than arithmetic roundoff.
pub fn occludes(origin: Vec3, depth: f64, hit: f64) -> bool {
    kernel::occludes(vec3(origin), depth, hit)
}

/// Squared eye distance used to sort overlapping projected Form points.
pub fn distance2(point: Vec3, eye: Vec3) -> f64 {
    kernel::distance2(vec3(point), vec3(eye))
}
