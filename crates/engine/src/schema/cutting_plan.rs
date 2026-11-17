//! Derived workshop plans and the machine-wide stock JSON contract (millimetres).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StockChoice {
    pub id: String,
    pub name: String,
    /// panel, board or length. Board and length use cut-to-length packing.
    pub kind: String,
    /// Empty material matches any material, on separate stock pieces.
    #[serde(default)]
    pub material: String,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    pub nominal: Option<String>,
    pub price: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StockLibrary {
    pub stock: Vec<StockChoice>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Guillotine,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutParams {
    /// Stock ids from the library. Omitted selects all compatible choices.
    pub stock: Option<Vec<String>>,
    /// Shared JSON stock file; omitted uses the shipped metric preset.
    pub library: Option<String>,
    #[serde(default = "kerf")]
    pub kerf: f64,
    #[serde(default = "trim")]
    pub trim: f64,
    #[serde(default)]
    pub mode: Mode,
    /// Plan export: .svg, .json, .csv or .dxf.
    pub path: Option<String>,
    /// One SVG label for every physical piece, including unplaced pieces.
    pub labels_path: Option<String>,
}
fn kerf() -> f64 {
    3.2
}
fn trim() -> f64 {
    10.0
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub length: f64,
    pub width: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Placement {
    pub item: usize,
    pub mark: String,
    pub x: f64,
    pub y: f64,
    pub length: f64,
    pub width: f64,
    pub rotated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Sheet {
    pub stock: String,
    pub material: String,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    pub linear: bool,
    pub placements: Vec<Placement>,
    /// Kerf bands in edge-to-edge cutting order.
    pub cuts: Vec<Rectangle>,
    pub offcuts: Vec<Rectangle>,
    #[serde(rename = "yield")]
    pub yield_ratio: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Label {
    pub item: usize,
    pub mark: String,
    pub name: String,
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    pub material: String,
    pub grain: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Unplaced {
    pub item: usize,
    pub mark: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Purchase {
    pub stock: String,
    pub material: String,
    pub count: usize,
    pub price: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct Layout {
    pub sheets: Vec<Sheet>,
    pub labels: Vec<Label>,
    pub to_buy: Vec<Purchase>,
    pub unplaced: Vec<Unplaced>,
    pub available_stock: Vec<StockChoice>,
    pub excluded: Vec<super::cutlist::Excluded>,
}

#[cfg(test)]
#[path = "cutting_plan_tests.rs"]
mod tests;
