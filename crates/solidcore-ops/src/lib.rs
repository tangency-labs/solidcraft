//! # solidcore-ops
//!
//! The modelling operations applications built on solidcore call: each a sequence of kernel
//! calls, with the record converted through `solidcore-convert` (docs/layers.md). Ops chooses
//! (entities, parameters, defaults), sequences kernel calls, converts, and compares the numbers
//! the kernel returns with tolerances. It computes no geometric quantity of its own: no square
//! root, trigonometry, product, normalising or solve (clippy `disallowed-methods`, this crate's
//! `clippy.toml`). Arithmetic operators cannot be banned by clippy: review is the guard.
//!
//! What every family shares:
//! - [`session`]: the one process kernel, the fixes the application names, the panic guard, the
//!   call running now and the failure callback;
//! - [`Error`]: why an operation failed, as data (the application words it);
//! - [`Body`]: a kernel body, with primitives, copy and transform, counts and volume, `.smb`
//!   bytes ([`file`]) and the validity check ([`check`]), and the record both ways
//!   ([`Body::from_record`], [`Body::to_record`]) with the [`Order`] that takes record indices to
//!   the kernel's entities;
//! - `finish`: the result contract, from what the input was (crate-internal: the families use it);
//! - [`provenance`]: the hook an application's construction history rides on.
//!
//! The families: [`measure`] (mass properties, area, box, point class, gap samples, the picks'
//! distances and normals), [`boolean`], [`stitch`], [`prism`] (planar region booleans and
//! extrusion, prism sections).
//!
//! Operations change the body they are given, as the kernel does. An application that keeps
//! its input copies it first ([`Body::copy`]).
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]

pub mod blend;
pub mod body;
pub mod boolean;
pub mod check;
pub mod error;
pub mod extend;
pub mod file;
pub mod measure;
pub mod offset;
pub mod order;
pub mod prism;
pub mod provenance;
pub mod session;
pub mod split;
pub mod stitch;
pub mod sweep;
pub mod untrim;

pub use blend::{
    band_room, blend, chamfer_sides, chamfer_sides_ordered, edge_faces_along,
    edge_faces_along_ordered, fillet_variable, tangent_chain, BandRoom, Blend, Blended,
    ChamferSecond, Corners, EndRoom, Hole, HoleEdge, Room, SetBack,
};
pub use body::{make_box, make_cone, make_cylinder, make_sphere, make_torus, Body};
pub use boolean::BooleanOp;
pub use check::{CheckLevel, CheckReport};
pub use error::{Error, FitKind, Result, RoomReason, Unblendable};
pub use measure::{
    point_distance, Distance, EdgePoint, GapSample, MassPropertiesEstimate, Normal, PointClass,
};
pub use offset::{draft, offset_faces, offset_sheet, shell, thicken};
pub use order::Order;

#[cfg(test)]
mod test_corpus;
#[cfg(test)]
mod tests;
