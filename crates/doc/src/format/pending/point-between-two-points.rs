//! A construction point can sit halfway between two points: its `def` saves
//! `{type: "midpoint", a, b}`, two point references found again on every evaluation, so the point
//! follows both (#17396). Older designs never have this definition, so no step is needed. The last
//! release warns about the newer format number, opens what it can read and keeps the rest
//! (`crates/doc/src/newer_file.rs`): such a point keeps its place on the timeline as a feature it
//! can't read, reports an error when evaluated, and is saved back as it came.
