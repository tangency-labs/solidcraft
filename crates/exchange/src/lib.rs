//! SolidCraft exchange: the one boundary through which analysis tools (electromagnetics,
//! structural, CFD) read SolidCraft designs.
//!
//! Open a design, take a snapshot of it, and get plain, typed data:
//!
//! ```no_run
//! # fn main() -> Result<(), solidcraft_exchange::Error> {
//! use solidcraft_exchange::{Edit, FeatureId, Mm, Options, Quantity};
//! let bytes = std::fs::read("part.sc").unwrap_or_default();
//! let design = solidcraft_exchange::open(&bytes)?;
//! let options = Options::default()
//!     .with_override("width", Quantity::Length(Mm(30.0)))
//!     .with_edits(vec![Edit::Suppress(FeatureId(4))]);
//! let snapshot = design.snapshot(&options)?;
//! let json = solidcraft_exchange::to_json(&snapshot)?;
//! # let _ = json;
//! # Ok(())
//! # }
//! ```
//!
//! A [`Snapshot`] holds the parameters, the physical materials, the timeline's features and per
//! body one watertight, welded triangle mesh (the tagged 3MF export's tessellation) with the
//! B-rep's faces (persistent name, exact surface), edges (name, exact curve, the mesh vertices
//! along it) and corners on it. No kernel or document type appears in it, so tools depend on
//! this crate alone, or read its JSON. The compatibility rules are in [`schema`] and
//! docs/formats/exchange-snapshot.md.
//!
//! Everything is bounded: the design's size, the overrides, edits, bodies, faces, triangles and
//! control points, the chord tolerance. The same design and options give the same bytes.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod convert;
mod design;
mod error;
mod options;
pub mod schema;

use std::sync::Mutex;

use solidcraft_doc::Model;

pub use design::Design;
pub use error::{Error, FeatureFailure, Result};
pub use options::{Options, Override, Tolerance};
pub use schema::*;

/// Largest design file [`open`] reads (the design reader's own limit).
pub const MAX_DESIGN_BYTES: usize = solidcraft_io::MAX_DESIGN_BYTES;
/// The newest design format [`open`] reads (`"format": "solidcraft/N"`), the one SolidCraft writes.
pub const DESIGN_FORMAT: u32 = solidcraft_doc::format::FORMAT;
/// Most parameter overrides one snapshot takes.
pub const MAX_OVERRIDES: usize = 1024;
/// Longest override expression (bytes).
pub const MAX_EXPRESSION_BYTES: usize = 4096;
/// Largest magnitude of an override value or an edit's coordinate.
pub const MAX_VALUE: f64 = 1e12;
/// Most edits one snapshot takes.
pub const MAX_EDITS: usize = 1024;
/// Longest edit name (bytes).
pub const MAX_NAME_BYTES: usize = 256;
/// Most bodies in one snapshot.
pub const MAX_BODIES: usize = 100_000;
/// Most faces in one snapshot (all bodies together).
pub const MAX_FACES: usize = 1_000_000;
/// Most triangles in one snapshot (all bodies together).
pub const MAX_TRIANGLES: usize = 20_000_000;
/// Most NURBS control points in one snapshot (all faces together).
pub const MAX_CONTROL_POINTS: usize = 10_000_000;
/// Finest chord tolerance (mm); the export meshes never go below it either.
pub const MIN_TOLERANCE_MM: f64 = 1e-3;
/// Coarsest absolute chord tolerance (mm).
pub const MAX_TOLERANCE_MM: f64 = 1e4;
/// Range of a relative chord tolerance (a share of each body's size).
pub const RELATIVE_TOLERANCE: (f64, f64) = (1e-6, 0.1);
/// The default relative chord tolerance: the export's standard quality.
pub const DEFAULT_RELATIVE_TOLERANCE: f64 = 2e-4;

/// Open a design file's bytes (`.sc`, a legacy `.solidcraft`/`.solvecraft` container, or the
/// plain JSON design).
pub fn open(bytes: &[u8]) -> Result<Design> {
    if bytes.len() > MAX_DESIGN_BYTES {
        return Err(Error::Limit(format!("the design is {} MB, over the {} MB a design file may hold", bytes.len() >> 20, MAX_DESIGN_BYTES >> 20)));
    }
    let doc = solidcraft_io::read_design(bytes).map_err(|e| Error::Read(e.to_string()))?;
    Ok(Design { doc, model: Mutex::new(Model::new()) })
}

/// The snapshot as JSON: deterministic bytes (fixed field and element order, shortest
/// round-trip numbers).
pub fn to_json(snapshot: &Snapshot) -> Result<String> {
    serde_json::to_string(snapshot).map_err(|e| Error::Invalid(format!("snapshot as JSON: {e}")))
}

/// A snapshot read from JSON, checked ([`Snapshot::validate`]). A schema above [`SCHEMA`] is
/// refused; unknown fields are ignored.
pub fn from_json(text: &str) -> Result<Snapshot> {
    let s: Snapshot = serde_json::from_str(text).map_err(|e| Error::Read(format!("snapshot JSON: {e}")))?;
    if s.schema > SCHEMA {
        return Err(Error::NotSupported(format!("snapshot schema {} (this reader knows up to {SCHEMA})", s.schema)));
    }
    s.validate()?;
    Ok(s)
}
