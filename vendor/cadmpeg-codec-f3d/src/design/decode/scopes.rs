// SPDX-License-Identifier: Apache-2.0
//! Parse parameter scopes and exact feature-construction frames.

pub(crate) mod align;
mod assembly_alignment;
mod assembly_carrier_paths;
mod assembly_operand_frames;
pub(crate) mod assembly_v3;
pub(crate) use assembly_operand_frames::wide_family_frame_offsets;
mod assembly_operand_paths;
mod axial_assembly;
mod base_feature;
mod coil;
mod combine;
mod component_constructions;
mod copy_paste_bodies;
pub(crate) mod direct_face;
mod draft;
mod emboss;
pub(crate) mod extrude;
mod fixed_parameters;
pub(super) mod hole;
pub(crate) mod legacy_class_397;
pub(crate) mod legacy_class_415;
mod legacy_operand_paths;
pub(crate) mod mirror;
mod occurrence_transfer;
pub(crate) mod parameter_scope;
mod path_feature;
mod pattern;
pub(crate) mod pipe;
pub(super) mod point_data;
pub(super) mod shared_frames;
pub(crate) mod sheet_metal;
mod solid_primitive;
mod surfaces;
mod thicken_shell;
mod thread;
mod work_geometry;

#[cfg(test)]
mod tests;
