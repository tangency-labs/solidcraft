//! A fillet may save `turn: "auto"`: each turn of two rounds at a sharp side edge swept where the
//! swept turn builds (the rolling ball can pass the side edge) and mitred where it does not.
//! `solid.fillet` gives every new fillet this turn unless it asks for another (or rounds its
//! edges at per-edge radii, which are mitred).
//!
//! An older design is not rewritten: a fillet saved without `turn`, as every mitred fillet was
//! and still is, reads as mitred, so it rebuilds as it did. The last release opens a design with
//! an `auto` fillet with a warning about the newer format, and keeps what it cannot read.
