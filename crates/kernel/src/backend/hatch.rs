//! solidcore's hatching ([`Hatching`]): the solidcore-ops calls this crate's callers made before
//! the interface existed.

use solidcore_ops::profile;
use solidcore_record::profile::line_family::{Invalid, LineFamily, Mark};
use solidcore_record::{Region2, Vec2};
use solidcraft_kernel_api::hatch::Hatching;

use super::Solidcore;

impl Hatching for Solidcore {
    const MAX_FAMILIES: usize = profile::MAX_FAMILIES;

    fn clip_line_family(&self, families: &[LineFamily], region: &Region2) -> Result<Vec<Mark>, Invalid> {
        profile::clip_line_family(families, region)
    }

    fn dot_region(&self, at: Vec2, width: f64) -> Option<Region2> {
        profile::dot_region(at, width)
    }
}
