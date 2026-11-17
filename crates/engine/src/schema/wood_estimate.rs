//! Read-only timber estimates. Money uses one user-chosen currency; time is minutes.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Supply {
    pub name: String,
    pub quantity: f64,
    pub unit_price: f64,
}

/// Optional per-design supplies under metadata key io.solidcraft.wood.estimate.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesignSupplies {
    pub finishing: Vec<Supply>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rates {
    pub saw_cut_minutes: f64,
    pub router_minutes_per_metre: f64,
    pub glue_up_minutes: f64,
    pub labour_per_hour: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EstimateParams {
    /// Same shared stock file and choices as cutlist.layout.
    pub library: Option<String>,
    pub stock: Option<Vec<String>>,
    #[serde(default = "kerf")]
    pub kerf: f64,
    #[serde(default = "trim")]
    pub trim: f64,
    /// Shared JSON rates file. Omitted uses assets/wood/rates.json.
    pub rates: Option<String>,
    /// Explicit workshop glue-up batches; the model does not infer assembly order.
    #[serde(default)]
    pub glue_ups: u32,
    /// Overrides design or stock-library finishing supplies; [] clears them.
    pub finishing: Option<Vec<Supply>>,
    /// Optional CSV export. Does not save estimates in the design.
    pub path: Option<String>,
}
fn kerf() -> f64 {
    3.2
}
fn trim() -> f64 {
    10.0
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Line {
    pub category: String,
    pub item: String,
    pub quantity: f64,
    pub unit_minutes: f64,
    pub minutes: f64,
    pub unit_cost: Option<f64>,
    pub cost: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct Estimate {
    pub stock_cost: f64,
    /// Stock plus finishing supplies.
    pub material_cost: f64,
    pub finishing_cost: f64,
    pub labour_cost: Option<f64>,
    pub total_cost: f64,
    pub work_minutes: f64,
    /// False when stock prices, placements, or feature results are missing.
    pub complete: bool,
    pub warnings: Vec<String>,
    pub lines: Vec<Line>,
}

#[cfg(test)]
#[path = "wood_estimate_tests.rs"]
mod tests;
