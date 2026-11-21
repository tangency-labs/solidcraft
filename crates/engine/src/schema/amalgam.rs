//! Command contracts for Amalgamate (#18799), the commands of `cmd/amalgam.rs`.
use super::{ConstructionCommon, StringList, feature_common};
use schemars::JsonSchema;
use serde::Deserialize;

/// Label bodies of this design as a tool that other designs place with Amalgamate. At least one
/// body in all; a body has one role. Whether each body exists, and that Remove and Add bodies
/// are closed solids, stays the feature's to check. The tag's name is the feature's `name`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AmalgamTagParams {
    /// Bodies cut from the bodies an Amalgamate names for Remove, before any Add is joined.
    /// Closed solids.
    #[serde(default)]
    pub remove: Option<StringList>,
    /// Bodies joined into the bodies an Amalgamate names for Add, after every Remove is cut.
    /// Closed solids.
    #[serde(default)]
    pub add: Option<StringList>,
    /// Bodies brought in as a part of their own.
    #[serde(default)]
    pub new: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

#[cfg(test)]
#[path = "amalgam_tests.rs"]
mod tests;
