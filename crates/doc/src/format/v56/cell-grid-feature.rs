//! A design can hold a `cell_grid` feature (#14044): a sketch's closed profiles filled with
//! cells of one shape (`shape`, `sides`, `dot_grid`, `size`, `web`, `angle`, `anchor`, `clip`) and
//! cut from the `targets` `depth` deep or through all (`mode: "cut"`). Older designs have no such
//! feature, so nothing in them is rewritten. The last release opens a design that has one with a
//! warning about the newer format, keeps the feature as raw data it cannot read, and saves it back
//! as it was (`crates/doc/src/newer_file.rs`).
