//! STEP (ISO 10303) for solidcore. [`p21`] (`solidcore-record`'s) reads the exchange structure,
//! [`read`] turns its geometry entities into the plain curves and surfaces of [`geom`], and
//! [`write`] writes them back, with [`write::product`] writing whole AP242 files of products,
//! assemblies and solidcore bodies. Written from the public ISO 10303-21 and -42 specifications.
//! [`topology`] reads solids (vertices to shells) into the kernel-neutral B-rep, schema 4, and
//! [`units`] the units of a representation context. Nothing here names an application: a file's
//! header says what its caller gives.
//!
//! Origin: SolidCraft's `crates/step` (MIT OR Apache-2.0), moved here with its history (RFC 0003,
//! 1.5). Curves and surfaces of the record are evaluated, and their exact forms made, by the
//! kernel's `solidcore::eval`, converting through `solidcore-convert` (the private `kernel`
//! module).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod eval;
pub mod geom;
mod kernel;
pub use solidcore_record::step::p21;
pub mod read;
pub mod topology;
pub mod units;
mod vec;
pub mod write;

#[cfg(test)]
mod geom_tests;
