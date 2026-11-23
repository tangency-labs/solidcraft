//! Frame Member: `pairs` (members between two points, each `{id, a, b}` with GeoRef ends),
//! `next_pair`, and `path_sketch` may be absent (a Frame Member of pairs only) (#15705). An older
//! design has no pairs and its `path_sketch` reads as before, so it opens and saves unchanged. The
//! last release refuses a design that has them (it asks for an update) instead of failing to read
//! the feature.
