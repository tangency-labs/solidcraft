//! A torus surface may save `tube_start`: the tube angle (radians) its v counts from, so that a
//! face's form starts its tube where the face begins (a face covering most of the tube, off the
//! tube's quarter lines, imported from STEP). An older design has none and counts from the outer
//! equator, as before. The last release refuses a design with this change (the newer format
//! number).
