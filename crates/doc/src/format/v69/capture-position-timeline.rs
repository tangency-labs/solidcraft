//! Designs save `capture_position` timeline features (`FeatureKind::CapturePosition`: captured
//! occurrence moves with their coordinate space, parametric translation/origin/axis/angle
//! expressions, optional rigid rotation matrix and placement path, and captured non-rigid joint
//! values and expressions), `Occurrence::base_transform` (pre-capture placement when timeline
//! position capture is used), and `Joint::base_values` / `Joint::base_exprs` (pre-capture DOF
//! values and expressions when `CapturePosition` captures or overrides a joint). Older designs have
//! none of these, so they load with `base_transform: None`, `base_values: None`, and
//! `base_exprs: []` and evaluate identically. The last release warns about the newer format number,
//! opens what it can read, and keeps `capture_position` features, `base_transform`, `base_values`,
//! and `base_exprs` for the next save (`crates/doc/src/newer_file.rs`).
