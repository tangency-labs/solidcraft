//! The router's hole stacks ([`solidcraft_kernel_api::HoleStacks`]).

use solidcore_record::brep::Brep;
use solidcore_record::hole_stacks::{Options, Stacks};
use solidcraft_kernel_api::hole_stacks::HoleStacksRefused;
use solidcraft_kernel_api::{HoleStacks, OpKind};

use super::{Which, which};
use crate::backend::Solidcore;

/// The hole stacks among `records`: every set of coaxial holes through two or more touching
/// bodies, and the holes in no stack, shaped by `o`.
///
/// # Errors
/// The kernel's refusal, in its words.
pub fn hole_stacks(records: &[&Brep], o: &Options) -> Result<Stacks, HoleStacksRefused> {
    match which(OpKind::HoleStacks) {
        Which::Solidcore => Solidcore.hole_stacks(records, o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

    #[test]
    fn routed_hole_stacks_are_solidcores() {
        let a = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 2.0)).unwrap();
        let b = crate::box_solid(Vec3::new(0.0, 0.0, 2.0), Vec3::new(10.0, 10.0, 4.0)).unwrap();
        let records = [a.record(), b.record()];
        let o = Options { touching: 0.1, axis_tolerance: 0.01, slots: true, diameters: Vec::new() };
        let routed = hole_stacks(&records, &o).unwrap();
        assert_eq!(routed, solidcore_ops::hole_stacks::hole_stacks(&records, &o).unwrap());
        assert!(routed.stacks.is_empty() && routed.lone.is_empty(), "two plain blocks have no holes: {routed:?}");
    }
}
