//! A fillet saves `edge_radii` when each picked edge is rounded at its own radius in one blend: a
//! list of lengths, one per picked edge in the order of `edges` (a constant, exact, rolling-corner,
//! mitred round of the picked edges alone). Absent, the default, every edge takes `radius`, as
//! designs before this change were built. An older design has none. The last release warns about
//! this newer design format and keeps the field it cannot read.
