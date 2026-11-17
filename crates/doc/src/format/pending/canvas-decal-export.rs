//! A canvas saves `decal: true` when it is a decal: part of the model's look, which a URDF
//! export writes as a textured visual of its component's link (`canvas.decal` sets it;
//! `canvas.edit` turns it on or off). A reference canvas, made to trace, saves nothing new.
//!
//! An older design has no such field: its decals are not exported to URDF until
//! `canvas.edit {decal: true}` marks them.
//!
//! The last release opens a design with a marked decal with a warning about the newer format,
//! shows the decal as before, and keeps the field it can't read for the next save
//! (`crates/doc/src/newer_file.rs`).
