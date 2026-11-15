//! A design saves `gears`: for each component made by Insert Gear (`gear.insert`), its module,
//! tooth count, pressure angle, face width, profile shift, backlash and flank points, so the
//! teeth can be rebuilt when a value changes (`gear.edit`). An older design has no gears and
//! opens unchanged. The last release opens a design with gears with a warning about the newer
//! format and keeps the `gears` it cannot read.
