//! Least-squares line/plane projection for Form controls or evaluated surface points.
use super::{Result, bad};
use solidcore_record::Vec3;

pub use solidcore_ops::form_cage::Projection;

/// With no direction, use the principal line or least-squares plane. A supplied direction
/// is the line direction or plane normal; an omitted origin uses the selection centroid.
pub fn project(points: &[Vec3], tool: Projection, direction: Option<Vec3>, origin: Option<Vec3>) -> Result<Vec<Vec3>> {
    solidcore_ops::form_cage::project(points, tool, direction, origin).map_err(|error| bad(&error.to_string()))
}
