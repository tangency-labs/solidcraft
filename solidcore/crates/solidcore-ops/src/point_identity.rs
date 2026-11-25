//! Point-identity witnesses for application-owned links, without geometry in the application.

use solidcore_record::Vec2;

use crate::error::status;
use crate::Result;

/// Check ordered point candidates after the application establishes their sharing graph.
///
/// False includes ambiguous identity and exhausted work; it never means that the two
/// geometries are equal. Accepted coordinates must still replace the stored coordinates.
/// `within` scales with one plus the larger point norm, as for the legacy link nearness.
pub fn ordered_nearby_points(old: &[Vec2], new: &[Vec2], within: f64, work: usize) -> Result<bool> {
    let old: Vec<_> = old.iter().copied().map(solidcore_convert::vec2).collect();
    let new: Vec<_> = new.iter().copied().map(solidcore_convert::vec2).collect();
    solidcore::query::point_identity::ordered_nearby_points(&old, &new, within, work)
        .map_err(status)
}
