//! `FeatureKind::SurfaceMorph` (`surface.morph`): morphs a solid or sheet body (or selected
//! faces) by mapping a base reference surface onto a target reference surface.
//!
//! Older designs do not contain `SurfaceMorph` features and open unchanged. The last release
//! opens a design carrying `SurfaceMorph` with a newer-format warning, reads what it knows, and
//! preserves the unknown feature record on save (`crates/doc/src/newer_file.rs`).
