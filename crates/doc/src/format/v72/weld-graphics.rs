//! Save weld graphics and their symbols together, using nine stable type ids, named choices,
//! optional ISO 4063 process numbers and density, type-specific millimetre sizes and optional
//! intermittent spans (count, length and centre pitch). Texture is derived from the process.
//! Settings also save symbol_standard (ISO A by default), groove (v), penetration (full),
//! root (as_welded), tack_mode (pitch), optional root_gap and add_fillet (false).
//! Fillet legs have separate z1/z2 keys and optional deep throat s; plug fill depth is h.
//! Tack size n is a positive integer, with l/e, l/n or l selected by tack_mode.
//! Older designs have no welds; this pre-merge shape needs no migration. The last release
//! warns about the newer format and preserves the weld data it cannot read when saving.
