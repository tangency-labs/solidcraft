// SPDX-License-Identifier: Apache-2.0
//! Stable Design registration and frame variants of the BREP body map.

/// Stable type family that contains BREP body-map records.
///
/// The family also contains other Body records. A record is a body map only
/// when one of [`BODY_MAP_ZERO_PREFIX_LENGTHS`] gives a complete frame.
pub(crate) const BODY_MAP_CARRIER_TYPE_GUID: &str = "74CA4562-59F8-4C97-8AD1-F8297C21F9AA";
pub(crate) const BODY_MAP_CARRIER_BASE_TYPE_GUID: &str = "E8DCB040-6A0E-4AD1-BCF3-1E1AEDD35EE4";
pub(crate) const BODY_MAP_CARRIER_TYPE_VERSION: u32 = 0;

/// Stable sibling family that binds history-free BREP snapshots to Design bodies.
pub(crate) const SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID: &str = "40DCC3F41D-E94F-B3B1-3E95-9B6ABE572C";
pub(super) const SNAPSHOT_BODY_MAP_CARRIER_TYPE_VERSIONS: [u32; 2] = [0, 1];
pub(crate) const SNAPSHOT_BODY_LIST_TYPE_GUID: &str = "E55D0457-898F-4A8E-B1A5-C85BDBA8794F";

/// Supported reserved-zero lengths between the indexed header and pair count.
pub(super) const BODY_MAP_ZERO_PREFIX_LENGTHS: [usize; 2] = [10, 11];

/// Reserved-zero length emitted by the source-less writer.
pub(crate) const GENERATED_BODY_MAP_ZERO_PREFIX_LEN: usize = BODY_MAP_ZERO_PREFIX_LENGTHS[0];
