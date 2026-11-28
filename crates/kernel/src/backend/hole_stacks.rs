//! solidcore's hole stacks ([`HoleStacks`]): the solidcore-ops call this crate's callers made
//! before the interface existed, its refusal's words kept whole.

use solidcore_record::brep::Brep;
use solidcore_record::hole_stacks::{Options, Stacks};
use solidcraft_kernel_api::hole_stacks::{HoleStacks, HoleStacksRefused};

use super::Solidcore;

impl HoleStacks for Solidcore {
    fn hole_stacks(&self, records: &[&Brep], o: &Options) -> Result<Stacks, HoleStacksRefused> {
        solidcore_ops::hole_stacks::hole_stacks(records, o).map_err(|e| HoleStacksRefused(e.to_string()))
    }
}
