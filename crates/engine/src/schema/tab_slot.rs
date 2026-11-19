//! Contract of `solid.tab_slot` (`cmd/tab_slot.rs`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::features::FeatureResult;
use super::types::{FeatureCommon, LengthExpr, NumberExpr, Point3, feature_common};

/// A plate: a body by name, or the body and a point that chooses between several contact planes
/// of a pair.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PlateParam {
    /// The body's name.
    Body(String),
    /// The body, and a point near the contact plane to use.
    Picked {
        body: String,
        #[serde(default)]
        near: Option<Point3>,
    },
}

/// Tabs on the tab plates' edges where they meet the slot plates, and matching slots cut through
/// the slot plates, for laser-cut or routed boxes and frames. Every tab plate is tried against
/// every slot plate; a pair that cannot be cut is skipped and named in the reply. Sheet metal and
/// mesh bodies are refused.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabSlotParams {
    /// The plates that get tabs, 1 to 64, each once and not a slot plate.
    pub tab_plates: Vec<PlateParam>,
    /// The plates that get slots, 1 to 64, each once.
    pub slot_plates: Vec<PlateParam>,
    /// Tabs per stretch, a whole number 1 to 100. Not with `spacing`. Default: as many as fit.
    #[serde(default)]
    pub count: Option<NumberExpr>,
    /// Centre to centre between tabs, more than 0. Not with `count`.
    #[serde(default)]
    pub spacing: Option<LengthExpr>,
    /// A tab's width, more than 0. Default: three times the tab plate's thickness.
    #[serde(default)]
    pub tab_width: Option<LengthExpr>,
    /// From a stretch's end to the nearest tab, 0 or more. Default: the tab width.
    #[serde(default)]
    pub end_distance: Option<LengthExpr>,
    /// The room on every side of a tab in its slot, 0 or more. Default: 0.1 mm.
    #[serde(default)]
    pub clearance: Option<LengthExpr>,
    /// How far a tab stands out past the slot plate's far face. Default: 0.
    #[serde(default)]
    pub protrusion: Option<LengthExpr>,
    /// Seed of the tab width variation, a whole number from 0. The same seed gives the same
    /// widths. Default: 1000.
    #[serde(default)]
    pub seed: Option<NumberExpr>,
    /// Vary each tab's width by up to this much either way, 0 or more. Default: 0 (off). A tab is
    /// never under a tenth of the tab width and the layout keeps its end distances and spaces;
    /// the slots follow the tabs, so both plates carry the same layout.
    #[serde(default)]
    pub width_jitter: Option<LengthExpr>,
    /// Chamfer the two outer corners of each tab's free end by this much, so the tabs enter the
    /// slots easily. Default: 0 (off). The slot is not chamfered. Refused at half the tab width
    /// or more, and at the tab's length or more.
    #[serde(default)]
    pub tip_chamfer: Option<LengthExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// A stretch or tab of a cut plate joint that was not made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabSlotLeftOutResult {
    /// The reason id (`too_short`, `spacing_too_small`, `at_edge`, `over_hole`, ...).
    pub reason: String,
    /// Why, in words.
    pub detail: String,
}

/// A plate joint that was cut.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabSlotJointResult {
    pub tab_plate: String,
    pub slot_plate: String,
    /// The tabs it made.
    pub tabs: usize,
    pub left_out: Vec<TabSlotLeftOutResult>,
}

/// A plate pair that met but was not cut.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabSlotSkipResult {
    pub tab_plate: String,
    pub slot_plate: String,
    /// The reason id (`swapped`, `sheet_metal`, `plate_missing`, ...).
    pub reason: String,
    /// Why, in words.
    pub detail: String,
    pub left_out: Vec<TabSlotLeftOutResult>,
}

/// A plate's tool faces over the made joints.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabSlotToolFaces {
    pub body: String,
    pub faces: usize,
}

/// What `solid.tab_slot` returns: the feature, and what its evaluation cut.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabSlotResult {
    #[serde(flatten)]
    pub feature: FeatureResult,
    /// The plate joints cut, in the order they were cut.
    pub plate_joints: Vec<TabSlotJointResult>,
    /// The plate pairs that met but were not cut.
    pub skipped: Vec<TabSlotSkipResult>,
    /// Tabs made.
    pub tabs: usize,
    /// Slots cut (one per tab).
    pub slots: usize,
    /// Each plate's tool faces.
    pub tool_faces: Vec<TabSlotToolFaces>,
}

#[cfg(test)]
#[path = "tab_slot_tests.rs"]
mod tests;
