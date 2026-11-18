//! Authored sheet fills and the pattern snapshots they use.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::TypedContract;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub outer: Vec<String>,
    pub holes: Vec<Vec<String>>,
}

impl From<solidcraft_doc::WholeProfileBoundary> for Boundary {
    fn from(value: solidcraft_doc::WholeProfileBoundary) -> Self {
        Self { outer: value.outer, holes: value.holes }
    }
}

impl From<Boundary> for solidcraft_doc::WholeProfileBoundary {
    fn from(value: Boundary) -> Self {
        Self { outer: value.outer, holes: value.holes }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Paint {
    Solid,
    Hatch { pattern: String, pen_width_mm: f64 },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub sketch: String,
    pub boundary: Boundary,
    pub paint: Paint,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub fill: String,
    pub boundary: Option<Boundary>,
    pub paint: Option<Paint>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FillId {
    pub fill: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    pub fill: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternId {
    pub pattern: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternInspect {
    pub pattern: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileParams {
    pub sketch: String,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    #[serde(rename = "lines_45")]
    Lines45,
    CrossHatch,
}

#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    #[default]
    Mm,
    Inches,
}

fn one() -> f64 {
    1.0
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternImport {
    Pat {
        source: String,
        #[serde(default)]
        units: Units,
        #[serde(default = "one")]
        scale: f64,
    },
    Builtin {
        pattern: Builtin,
    },
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Current,
    LostBoundary,
    Failed,
}

#[derive(Serialize, JsonSchema)]
pub struct Reading {
    pub fill: String,
    pub sheet: String,
    pub sketch: String,
    pub boundary: Boundary,
    pub paint: Paint,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct Readings {
    pub drawing: String,
    pub fills: Vec<Reading>,
}

#[derive(Serialize, JsonSchema)]
pub struct Family {
    pub angle: f64,
    pub origin: [f64; 2],
    pub delta_x: f64,
    pub delta_y: f64,
    pub dashes: Vec<f64>,
}

#[derive(Serialize, JsonSchema)]
pub struct PatternReading {
    pub id: String,
    pub name: String,
    pub description: String,
    pub families: Vec<Family>,
}

#[derive(Serialize, JsonSchema)]
pub struct Patterns {
    pub drawing: String,
    pub patterns: Vec<PatternReading>,
}

#[derive(Serialize, JsonSchema)]
pub struct DeletedFill {
    pub drawing: String,
    pub fill: String,
}

#[derive(Serialize, JsonSchema)]
pub struct DeletedPattern {
    pub drawing: String,
    pub pattern: String,
}

/// Current certified whole-profile choices; identities are saved instead of indices.
#[derive(Serialize, JsonSchema)]
pub struct Profiles {
    pub boundaries: Vec<Boundary>,
    pub uncertified: usize,
}

#[derive(Serialize, JsonSchema)]
pub struct ProfileChoices {
    pub drawing: String,
    pub sketch: String,
    pub fill_profiles: Profiles,
}

pub static CREATE: TypedContract<Create> = TypedContract::new();
pub static EDIT: TypedContract<Edit> = TypedContract::new();
pub static DELETE: TypedContract<FillId> = TypedContract::new();
pub static INSPECT: TypedContract<Inspect> = TypedContract::new();
pub static PATTERN_IMPORT: TypedContract<PatternImport> = TypedContract::new();
pub static PATTERN_INSPECT: TypedContract<PatternInspect> = TypedContract::new();
pub static PATTERN_DELETE: TypedContract<PatternId> = TypedContract::new();
pub static PROFILES: TypedContract<ProfileParams> = TypedContract::new();
