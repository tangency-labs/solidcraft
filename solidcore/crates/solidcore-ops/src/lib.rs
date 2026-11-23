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
//! [`pick`] (the edge or vertex nearest a point), [`boolean`], [`stitch`], [`prism`] (planar
//! region booleans and extrusion, prism sections),
//! [`cell_grid`] (the cells of a grid over a profile).
//!
//! Operations change the body they are given, as the kernel does. An application that keeps
//! its input copies it first ([`Body::copy`]).
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]
#![cfg_attr(not(test), deny(clippy::float_arithmetic))]

pub mod affine;
pub mod axial_section;
pub mod blend;
pub mod blend_definition;
pub mod blend_surface;
pub mod body;
pub mod boolean;
pub mod cancel;
pub mod cap;
pub mod cell_grid;
pub mod check;
pub mod conic_projection;
pub mod contact;
pub mod continuity;
pub mod curve;
pub mod curve_fit;
pub mod curve_gap;
pub mod curve_knots;
pub mod curve_plane;
pub mod cylinder_wall;
pub mod develop;
pub mod error;
pub mod export_geometry;
pub mod extend;
pub mod face_surface;
pub mod faceted;
pub mod facing_trough;
pub mod file;
pub mod finger_joint;
pub mod fit_projection;
pub mod hatch;
pub mod heal;
pub mod history;
pub mod hlr;
pub mod import_geometry;
pub mod joint_turn;
pub mod label;
pub mod measure;
pub mod member_turn;
pub mod network;
pub mod offset;
pub mod order;
pub mod outline;
pub mod path_export;
pub mod pcurve;
pub mod pick;
pub mod point_identity;
pub mod prism;
pub mod provenance;
pub mod query;
pub mod record_check;
pub mod session;
pub mod spline_projection;
pub mod split;
pub mod step_assembly;
pub mod stitch;
pub mod surface;
pub mod surface_cvs;
pub mod sweep;
pub mod tangent;
pub mod trim;
pub mod untrim;
pub mod vertex_meet;

pub use blend::band_room;
pub use blend::blend;
pub use blend::blend_ordered;
pub use blend::blend_with;
pub use blend::carried_faces;
pub use blend::carried_faces_ordered;
pub use blend::chamfer_sides;
pub use blend::chamfer_sides_facing;
pub use blend::chamfer_sides_facing_with;
pub use blend::chamfer_sides_ordered;
pub use blend::chamfer_sides_ordered_with;
pub use blend::chamfer_sides_with;
pub use blend::edge_faces_along;
pub use blend::edge_faces_along_ordered;
pub use blend::edges_share_wedge;
pub use blend::fillet_variable;
pub use blend::fillet_variable_from;
pub use blend::fillet_variable_from_with;
pub use blend::fillet_variable_ordered;
pub use blend::fillet_variable_ordered_with;
pub use blend::fillet_variable_with;
pub use blend::tangent_chain;
pub use blend::BandRoom;
pub use blend::Blend;
pub use blend::BlendOptions;
pub use blend::Blended;
pub use blend::CarriedFace;
pub use blend::ChamferCorner;
pub use blend::ChamferSecond;
pub use blend::Corners;
pub use blend::EndRoom;
pub use blend::Hole;
pub use blend::HoleEdge;
pub use blend::Room;
pub use blend::SetBack;
pub use blend::Turn;
pub use body::make_box;
pub use body::make_cone;
pub use body::make_cylinder;
pub use body::make_sphere;
pub use body::make_torus;
pub use body::Body;
pub use body::Contract;
pub use body::TurnedOver;
pub use boolean::BooleanOp;
pub use check::CheckLevel;
pub use check::CheckReport;
pub use contact::Gap;
pub use error::Error;
pub use error::FailureCode;
pub use error::FitKind;
pub use error::OffsetRefusal;
pub use error::RecordEntity;
pub use error::Result;
pub use error::RoomReason;
pub use error::SolidTest;
pub use error::Unblendable;
pub use error::Unsoundness;
pub use faceted::faceted_volume;
pub use measure::point_displacement;
pub use measure::point_distance;
pub use measure::point_set_summary;
pub use measure::points_meet;
pub use measure::ClassifiedPoint;
pub use measure::Distance;
pub use measure::EdgeCurve;
pub use measure::EdgePoint;
pub use measure::EdgeRayPoint;
pub use measure::FaceCentroid;
pub use measure::FaceRayHit;
pub use measure::GapSample;
pub use measure::MassPropertiesEstimate;
pub use measure::MembersClosest;
pub use measure::Normal;
pub use measure::PointClass;
pub use measure::PointNormalFace;
pub use measure::PointSetSummary;
pub use measure::RayEntity;
pub use measure::RayHit;
pub use measure::SquareFaceLevel;
pub use measure::EDGE_POINTS_MAX;
pub use offset::draft;
pub use offset::offset_faces;
pub use offset::offset_sheet;
pub use offset::offset_surface;
pub use offset::shell;
pub use offset::thicken;
pub use order::Order;
pub use solidcore::mesh_reduce::ReducedMesh;
pub use step_assembly::placed_bodies;
pub use step_assembly::PlacedBody;

/// Reduce a mesh under the current cooperative cancellation context.
pub fn reduce_mesh(
    positions: &[[f64; 3]],
    indices: &[u32],
    corner_normals: Option<&[[f64; 3]]>,
    target_triangles: usize,
) -> Result<ReducedMesh> {
    solidcore::mesh_reduce::reduce_mesh_with_cancel(
        positions,
        indices,
        corner_normals,
        target_triangles,
        cancel::is_cancelled,
    )
    .map_err(error::status)
}

#[cfg(test)]
mod ball_corner_tests;
#[cfg(test)]
mod noisy_round_tests;
#[cfg(test)]
mod test_corpus;
#[cfg(test)]
mod tests;
