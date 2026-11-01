//! STEP (ISO 10303) for SolidCraft, independent of any modelling backend. [`p21`] reads the
//! exchange structure, [`read`] turns its geometry entities into the plain curves and surfaces
//! of [`geom`], and [`write`] writes them back. Written from the public ISO 10303-21 and -42
//! specifications. [`topology`] reads solids (vertices to shells) into the kernel-neutral B-rep,
//! schema 4, and [`units`] the units of a representation context.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod eval;
pub mod geom;
pub mod p21;
pub mod read;
pub mod topology;
pub mod units;
pub mod write;

#[cfg(test)]
mod geom_tests;
