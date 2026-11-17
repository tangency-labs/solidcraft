//! A design can hold a `living_hinge` feature (#17484): the area's sketch and profiles, the slot
//! length, bridge, row spacing and slot width, the angle, the anchor, the clip, the margin and the
//! plate (`targets`), whose slots are cut through the plate. Older designs have no such feature, so
//! nothing in them is rewritten. The last release opens a design that has one with a warning about
//! the newer format, keeps the feature as raw data it cannot read, and saves it back as it was
//! (`crates/doc/src/newer_file.rs`).
