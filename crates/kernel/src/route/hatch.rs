//! The router's hatching ([`solidcraft_kernel_api::hatch::Hatching`]), as
//! `solidcraft_kernel::hatch`: line families clipped to a region into marks, and a pen dot.

use solidcore_record::profile::line_family::{Invalid, LineFamily};
use solidcore_record::{Region2, Vec2};
use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::hatch::Hatching;

pub use solidcore_record::profile::line_family::Mark;

use super::{Which, which};
use crate::backend::Solidcore;

/// The most line families one clip takes: the kernel's. This build holds solidcore only.
pub const MAX_FAMILIES: usize = <Solidcore as Hatching>::MAX_FAMILIES;

/// Each family's lines clipped, in turn, to `region` under one work budget, or refused without
/// partial geometry.
///
/// # Errors
/// Invalid families, more than [`MAX_FAMILIES`], an unsupported boundary, uncertain arithmetic or
/// an exhausted budget.
pub fn clip_line_family(families: &[LineFamily], region: &Region2) -> Result<Vec<Mark>, Invalid> {
    match which(OpKind::Hatch) {
        Which::Solidcore => Solidcore.clip_line_family(families, region),
    }
}

/// A dot's filled disk, when its pen diameter `width` is representable at `at`.
#[must_use]
pub fn dot_region(at: Vec2, width: f64) -> Option<Region2> {
    match which(OpKind::Hatch) {
        Which::Solidcore => Solidcore.dot_region(at, width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::profile;
    use solidcore_record::Loop2;

    #[test]
    fn routed_hatching_is_solidcores() {
        assert_eq!(MAX_FAMILIES, profile::MAX_FAMILIES);
        let p = Vec2::new;
        let square = Region2 { outer: Loop2::polygon(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]), holes: Vec::new() };
        let family = LineFamily { angle: 0.5, origin: Vec2::ZERO, delta_x: 0.0, delta_y: 1.5, dashes: Vec::new() };
        let families = [family];
        assert_eq!(clip_line_family(&families, &square), profile::clip_line_family(&families, &square));
        assert_eq!(dot_region(p(1.0, 2.0), 0.5), profile::dot_region(p(1.0, 2.0), 0.5));
    }
}
