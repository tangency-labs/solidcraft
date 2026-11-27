//! Joint origins can snap persistently to a sketch point (including a sketch
//! circle, arc or ellipse centre such as `c1.center`).
//!
//! Adds `Snap::SketchPoint` (`"type": "sketch_point"`) to joint snap variants.
//! Older releases do not know this snap type and ask for an update when opening
//! a design that uses it (`crates/doc/src/newer_file.rs`). Older designs
//! without sketch-point joint origins open unchanged.
