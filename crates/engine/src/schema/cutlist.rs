//! Input and output of the read-only cut list command. All sizes are millimetres.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Include {
    #[default]
    Bodies,
    Components,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Material,
    Stock,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Component {
    Id(u64),
    Name(String),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Csv,
    Json,
}

/// List stock parts, grouping equal sizes and materials. Purchased and reference subtrees
/// are excluded. Phantom and inseparable components pass their cut parts through.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CutListParams {
    #[serde(default)]
    pub include: Include,
    /// Only this component and its descendants; omitted means the whole design.
    pub component: Option<Component>,
    /// The two grouping fields, material and stock (the default).
    pub group_by: Option<Vec<GroupBy>>,
    /// Optionally export a .csv or .json file. CSV lengths are mm.
    pub path: Option<String>,
    pub format: Option<Format>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Stock {
    pub kind: String,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    pub exact: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Row {
    pub item: usize,
    pub name: String,
    pub count: u64,
    pub material: String,
    pub stock: Stock,
    /// Empty when unset, otherwise length, width, thickness or oblique relative to the stock box.
    pub grain: String,
    pub bodies: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Excluded {
    pub body: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct MaterialTotal {
    pub material: String,
    pub count: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct CutList {
    pub rows: Vec<Row>,
    pub excluded: Vec<Excluded>,
    pub totals: Vec<MaterialTotal>,
    pub total_occurrences: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[cfg(test)]
#[path = "cutlist_tests.rs"]
mod tests;
