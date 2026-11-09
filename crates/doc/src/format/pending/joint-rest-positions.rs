//! Joints save a rest position: `rest`, the values `joint.reset` puts the joint back to (set by
//! `joint.set_rest`). An older design has none, so its joints load with their values as before
//! and a reset leaves them where they are. The last release warns about the newer format number,
//! opens the design and keeps the `rest` values it can't read.
