//! A design saves `gears`: for each component made by Insert Gear (`gear.insert`), its module,
//! tooth count, pressure angle, face width, profile shift, backlash and flank points, and for an internal gear its ring thickness or for an external one its axle hole diameter and offset, and its helix angle and whether it is double helical, its clearance (0.25 when not saved), and whether the backlash thickens the teeth, so the
//! teeth can be rebuilt when a value changes (`gear.edit`). An older design has no gears and
//! opens unchanged. The last release opens a design with gears with a warning about the newer
//! format and keeps the `gears` it cannot read.
