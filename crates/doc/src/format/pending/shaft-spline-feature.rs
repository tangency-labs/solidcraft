//! A design can hold a `shaft_spline` feature (#17693): the tooth spaces of a spline cut into a
//! picked shaft (`shaft`), a picked bore (`bore`) or both, each from the end its start face lies at
//! (`shaft_start`, `bore_start`), or with neither a new splined shaft and hub (`make`), from the
//! numbers of the kernel's tooth outlines (`form`, `teeth`, `length`, the involute or
//! straight-sided numbers, the clearances, `master`, `master_angle`, `cutter_diameter`,
//! `end_chamfer`, `entry_chamfer`, and how far across each chamfer runs, by a second distance or an
//! angle: `end_chamfer_distance2`, `end_chamfer_angle`, `entry_chamfer_distance2`,
//! `entry_chamfer_angle`), each kept as an expression. Older designs have no such feature,
//! so nothing in them is rewritten. The last release opens a design that has one with a warning
//! about the newer format, keeps the feature as raw data it cannot read, and saves it back as it
//! was (`crates/doc/src/newer_file.rs`).
