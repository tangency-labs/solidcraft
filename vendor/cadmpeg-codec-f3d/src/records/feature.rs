// SPDX-License-Identifier: Apache-2.0
//! Typed modeling-feature scopes, operations, and source operands.

pub(crate) mod align;
pub(crate) mod assembly;
pub(crate) mod assembly_features;
pub(crate) mod base_feature;
pub(crate) mod body_ops;
pub(crate) mod coil;
pub(crate) mod combine;
pub(crate) mod direct_face;
pub(crate) mod emboss;
pub(crate) mod extrude;
pub(crate) mod fixed_parameters;
pub(crate) mod hole;
pub(crate) mod joint_motion;
pub(crate) mod mirror;
pub(crate) mod occurrence_transfer;
pub(crate) mod path_features;
pub(crate) mod patterns;
pub(crate) mod primitives;
pub(crate) mod scope;
pub(crate) mod sheet_metal;
pub(crate) mod surface_ops;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
pub(crate) mod thread;
pub(crate) mod work_geometry;
