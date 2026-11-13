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
//! - [`cancel`]: stopping the calls of work nobody waits for any more ([`Error::Cancelled`]);
//! - [`Error`]: why an operation failed, as data (the application words it);
//! - [`Body`]: a kernel body, with primitives, copy and transform, counts and volume, `.smb`
//!   bytes ([`file`]) and the validity check ([`check`]), and the record both ways
//!   ([`Body::from_record`], [`Body::to_record`]) with the [`Order`] that takes record indices to
//!   the kernel's entities;
//! - `finish`: the result contract, from what the input was (crate-internal: the families use it);
//! - [`provenance`]: the hook an application's construction history rides on.
//! - [`affine`]: the maps a body or a curve is moved by, from an application's numbers.
//!
//! The families: [`measure`] (mass properties, area, box, point class, gap samples, the picks'
//! distances and normals), [`contact`] (whether bodies meet, their boundary distance),
//! [`boolean`], [`stitch`], [`prism`] (planar region booleans and extrusion, prism sections),
//! [`cell_grid`] (the cells of a grid over a profile).
//!
//! Operations change the body they are given, as the kernel does. An application that keeps
//! its input copies it first ([`Body::copy`]).
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]
#![cfg_attr(not(test), deny(clippy::float_arithmetic))]

pub mod affine;
pub mod blend;
pub mod blend_definition;
pub mod blend_surface;
pub mod body;
pub mod boolean;
pub mod cancel;
pub mod cell_grid;
pub mod check;
pub mod contact;
pub mod continuity;
pub mod curve;
pub mod curve_fit;
pub mod curve_knots;
pub mod curve_plane;
pub mod error;
pub mod extend;
pub mod face_surface;
pub mod file;
pub mod label;
pub mod measure;
pub mod network;
pub mod offset;
pub mod order;
pub mod pcurve;
pub mod prism;
pub mod provenance;
pub mod record_check;
pub mod session;
pub mod split;
pub mod step_assembly;
pub mod stitch;
pub mod surface;
pub mod surface_cvs;
pub mod sweep;
pub mod tangent;
pub mod trim;
pub mod untrim;

pub use blend::{
    band_room, blend, blend_ordered, carried_faces, carried_faces_ordered, chamfer_sides,
    chamfer_sides_ordered, edge_faces_along, edge_faces_along_ordered, fillet_variable,
    fillet_variable_ordered, tangent_chain, BandRoom, Blend, BlendOptions, Blended, CarriedFace,
    ChamferCorner, ChamferSecond, Corners, EndRoom, Hole, HoleEdge, Room, SetBack, Turn,
};
pub use body::{make_box, make_cone, make_cylinder, make_sphere, make_torus, Body, TurnedOver};
pub use boolean::BooleanOp;
pub use check::{CheckLevel, CheckReport};
pub use contact::Gap;
pub use error::{Error, FitKind, OffsetRefusal, Result, RoomReason, SolidTest, Unblendable};
pub use measure::{
    point_distance, Distance, EdgePoint, EdgeRayPoint, FaceCentroid, GapSample,
    MassPropertiesEstimate, Normal, PointClass,
};
pub use offset::{draft, offset_faces, offset_sheet, shell, thicken};
pub use order::Order;
pub use step_assembly::{placed_bodies, PlacedBody};

#[cfg(test)]
mod noisy_round_tests;
#[cfg(test)]
mod test_corpus;
#[cfg(test)]
mod tests;
