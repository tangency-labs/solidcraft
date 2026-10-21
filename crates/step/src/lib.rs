//! STEP (ISO 10303) for SolidCraft, independent of any modelling backend. [`p21`] reads the
//! exchange structure, [`read`] turns its geometry entities into the plain curves and surfaces
//! of [`geom`], and [`write`] writes them back. Written from the public ISO 10303-21 and -42
//! specifications. Topology (vertices to solids) builds on the kernel-neutral B-rep and comes
//! next.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod eval;
pub mod geom;
pub mod p21;
pub mod read;
pub mod write;

#[cfg(test)]
mod geom_tests;
