//! Input and reply contracts for router edge profiles.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{AngleExpr, EdgeRefs, FeatureCommon, IdOrName, LengthExpr, feature_common};

/// Cut a router profile or update an existing routed-edge feature.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EdgeProfileParams {
    /// Existing router feature, by id or name. Omit to create one.
    pub feature: Option<IdOrName>,
    /// One to 256 edges of one body. Required when creating; omitted when retaining an edit's picks.
    pub edges: Option<EdgeRefs>,
    /// Restrict all picks to this body.
    pub body: Option<String>,
    /// The router section. None restores the input geometry while keeping the feature.
    pub profile: Profile,
    /// Roundover, cove and bead radius; default 6 mm.
    pub radius: Option<LengthExpr>,
    /// Chamfer/bevel setback (default 3 mm), rabbet/ogee width (default 12 mm).
    pub width: Option<LengthExpr>,
    /// Rabbet/ogee depth; default 12 mm.
    pub depth: Option<LengthExpr>,
    /// Bevel angle from the top support, strictly between 0 and 90 degrees; default 45 degrees.
    pub angle: Option<AngleExpr>,
    /// Bead's flat step on each side; default 1 mm.
    pub step: Option<LengthExpr>,
    /// Roundover's flat top step; default 0 mm.
    pub fillet: Option<LengthExpr>,
    /// Include the selected edges' tangent chains; default false.
    pub tangent_chain: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Router section identifiers.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Bead,
    Bevel,
    Chamfer,
    Cove,
    None,
    Ogee,
    Rabbet,
    Roundover,
}

/// A routed edge's length on the feature's input body, in mm.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RoutedEdge {
    /// Index within this feature's resolved edges, including any tangent propagation.
    pub edge: usize,
    pub length_mm: f64,
}

/// Router feature identity and the work length of each edge.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EdgeProfileResult {
    pub feature: u64,
    pub edges: Vec<RoutedEdge>,
}
