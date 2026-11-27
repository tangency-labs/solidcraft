//! Hatching ([`Hatching`]): families of parallel dashed lines clipped to a region into marks,
//! and a pen dot's filled disk. The families, marks and regions are the record's
//! (`solidcore_record::profile::line_family`).

use solidcore_record::profile::line_family::{Invalid, LineFamily, Mark};
use solidcore_record::{Region2, Vec2};

use crate::Backend;

/// The hatching of one kernel ([`crate::OpKind::Hatch`]).
pub trait Hatching: Backend {
    /// The most line families one clip takes.
    const MAX_FAMILIES: usize;
    /// Each family's lines clipped, in turn, to `region` under one work budget, or refused without
    /// partial geometry: invalid families, more than [`Hatching::MAX_FAMILIES`], an unsupported
    /// boundary, uncertain arithmetic or an exhausted budget.
    fn clip_line_family(&self, families: &[LineFamily], region: &Region2) -> Result<Vec<Mark>, Invalid>;
    /// A dot's filled disk, when its pen diameter `width` is representable at `at`.
    fn dot_region(&self, at: Vec2, width: f64) -> Option<Region2>;
}
