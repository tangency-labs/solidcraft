// SPDX-License-Identifier: Apache-2.0
//! Parse Design metadata and bulk-stream byte frames by domain.

mod assembly;
pub(crate) mod body;
pub(crate) mod body_consumers;
pub(crate) mod body_reference_tails;
pub(crate) mod canvas;
pub(crate) mod component_bodies;
pub(crate) mod components;
pub(crate) mod configuration_root;
pub(crate) mod decal;
pub(crate) mod dimension_frames;
mod image;
pub(crate) mod joint_motion;
pub(crate) mod mesh;
pub(crate) mod meta;
pub(crate) mod operands;
pub(crate) mod parameters;
mod patch;
pub(crate) mod placement_construction;
pub(crate) mod presentation;
pub(crate) mod scopes;
pub(crate) mod sheet_metal_settings;
pub(crate) mod sketch;
pub(crate) mod sketch_owners;
pub(crate) mod surface_trim;
pub(crate) mod text;
pub(crate) mod units;
