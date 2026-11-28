//! Command contracts for a view's automatic centre-mark rule.

use super::drawing_dimension::{CandidateResult, EdgeParam};
use super::drawing_view::ViewIdParam;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SetParams {
    pub view: ViewIdParam,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ViewParams {
    pub view: ViewIdParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ExcludeParams {
    pub view: ViewIdParam,
    pub edge: EdgeParam,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct IncludeParams {
    pub view: ViewIdParam,
    pub reference: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExclusionResult {
    pub reference: Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub exact: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateResult>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RuleResult {
    pub view: String,
    pub enabled: bool,
    pub exclusions: Vec<ExclusionResult>,
    pub changed: bool,
    /// Why automatic marks cannot be scanned from the model as it is now, while the rule is on. A
    /// deferred drawing's sheet may still show its last marks until `drawing.update`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
