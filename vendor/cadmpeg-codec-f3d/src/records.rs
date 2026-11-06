// SPDX-License-Identifier: Apache-2.0
//! Fusion parametric-design records and links to the solved B-rep.

#![deny(clippy::disallowed_methods)]

#[macro_use]
mod identity_rewrite;

pub(crate) mod act;
pub(crate) mod bodies;
pub(crate) mod body_ownership;
pub(crate) mod body_reference_tail;
pub(crate) mod canvas;
pub(crate) mod component_path;
pub(crate) mod configuration;
pub(crate) mod decal;
pub(crate) mod dimension_locus_arenas;
pub(crate) mod dimension_null_locus_wire;
pub(crate) mod dimensions;
pub(crate) mod entity_header;
pub(crate) mod feature;
mod frame_chain;
pub(crate) mod identity;
pub(crate) mod mesh;
pub(crate) mod parameters;
pub(crate) mod placement_construction;
pub(crate) mod recipes;
pub(crate) mod references;
pub(crate) mod serde_column;
pub(crate) mod sheet_metal_settings;
pub(crate) mod sketch_geometry;
pub(crate) mod sketch_links;
pub(crate) mod sketch_placement;
pub(crate) mod sketch_relations;
#[cfg(test)]
mod tests;
pub(crate) mod topology;
pub(crate) mod xref;
