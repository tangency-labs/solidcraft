//! Planning a living hinge (design 15): the slots of a kerf-bending pattern over a sketch area in a
//! flat plate.
//!
//! Recipes of kernel calls (`solidcore::living_hinge`, which holds the lattice, the edge rules and
//! the checks of its inputs): the record's spec and slots are converted by
//! [`solidcore_convert::living_hinge`]. It computes no geometric quantity.

use solidcore::living_hinge as kernel;
use solidcore_convert as convert;
use solidcore_record::living_hinge::{HingeSlot, LivingHingeSpec};
use solidcore_record::Region2;

pub use solidcore::living_hinge::{LivingHingeError, MAX_SLOTS};

/// The pattern a new living hinge starts from (the kernel's defaults).
#[must_use]
pub fn default_spec() -> LivingHingeSpec {
    convert::living_hinge::record_spec(&kernel::LivingHingeSpec::default())
}

/// The region of `slot` in the hinge frame: a stadium `slot_width` wide around its centre line.
#[must_use]
pub fn slot_region(slot: &HingeSlot, slot_width: f64) -> Region2 {
    convert::record_region2(&kernel::slot_region(
        &convert::living_hinge::slot(slot),
        slot_width,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::living_hinge::HingeClip;

    #[test]
    fn the_default_spec_is_the_designs() {
        let s = default_spec();
        assert_eq!(
            (s.slot_length, s.bridge, s.row_spacing, s.slot_width, s.margin),
            (20.0, 3.0, 1.5, 0.2, 0.0)
        );
        assert_eq!(s.clip, HingeClip::RunOut);
    }
}
