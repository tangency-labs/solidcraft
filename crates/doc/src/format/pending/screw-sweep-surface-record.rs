//! A design's model cache may hold bodies whose thread flanks and coil walls are saved as a
//! `sweep` surface (`solidcore_record::brep::sweep`): the wall by its definition (the profile, the
//! screw motion, the stations and half angles, the map and the bound the kernel held) beside its
//! B-spline net, where they were saved as the net alone. Nothing in the timeline changes, and a
//! design without modelled threads or coils saves the same cache as before.
//!
//! An older design has no such faces and needs no step.
//!
//! The last release warns about the newer format number and names this change among those it
//! doesn't know (`crates/doc/src/newer_file.rs`). Its record has no `sweep` kind, so a cached body
//! that holds one doesn't decode there, which makes the design's cache a miss
//! (`crates/io/src/design/native_cache.rs`): the design is evaluated again from its timeline, its
//! threads built as splines as that release builds them. Nothing in the design is lost, and
//! saving writes the timeline back as it came.
