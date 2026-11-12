//! The stock, facing and rapid-safety parts of the `cam.job` contract (`schema/cam.rs` holds
//! the rest): the job's `stock`, the `face` operation, and the reply's rapid, travel and stock
//! reports. docs/cam-jobs.md describes them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::cam::{Common, FeedsSpec, operation};

/// The raw material the job starts from, in work coordinates: what no rapid may pass through,
/// and what `face` faces by default. One of `box`, `cylinder` or `around_part`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StockSpec {
    /// A box from its lowest to its highest corner.
    Box(BoxStockSpec),
    /// A round bar about work X (the A axis of a 4th axis), from `x_min` to `x_max`.
    Cylinder(CylinderStockSpec),
    /// The box round the part's bodies grown by `margin` on every side.
    AroundPart(AroundPartSpec),
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoxStockSpec {
    /// The lowest corner [x, y, z], mm.
    pub min: [f64; 3],
    /// The highest corner [x, y, z], mm.
    pub max: [f64; 3],
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CylinderStockSpec {
    /// The bar's radius, mm.
    pub radius: f64,
    /// Where it starts and ends along X, mm.
    pub x_min: f64,
    pub x_max: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AroundPartSpec {
    /// How much material there is beyond the part along X, Y and Z, each side, mm (0 or more).
    pub margin: [f64; 3],
    /// Material above the part's top, mm (0 or more), instead of `margin`'s Z. Default: `margin`'s Z.
    #[serde(default)]
    pub top: Option<f64>,
    /// Material below the part's bottom, mm (0 or more), instead of `margin`'s Z. Default: `margin`'s Z.
    #[serde(default)]
    pub bottom: Option<f64>,
}

operation! {
    /// Faces the stock's top (or `top`) flat down to `bottom`, in levels, in straight passes along
    /// `angle_deg` a stepover apart. Every pass starts and ends with the cutter clear of the area,
    /// so the tool goes down beside the stock and enters it at depth; the outermost passes reach
    /// past its edges by `overhang`.
    FaceOp {
        /// The faced height, mm.
        pub bottom: f64,
        /// Where the first level starts from, mm. Default: the stock's top (required without a
        /// stock).
        #[serde(default)]
        pub top: Option<f64>,
        /// Where rapid motion towards the material stops, mm. Default: 2 above the top (or above
        /// the stock's top, if that is higher).
        #[serde(default)]
        pub retract: Option<f64>,
        /// Where the tool travels to and from the operation, mm. Default: 10 above the retract
        /// height.
        #[serde(default)]
        pub clearance: Option<f64>,
        /// Most material per level, mm. Default: the tool's `stepdown`, else 1.
        #[serde(default)]
        pub stepdown: Option<f64>,
        /// Distance between passes, mm, at most the flat of the tool's end. Default: the tool's
        /// `stepover`, else 70% of the flat of its end (its diameter, for a flat end mill).
        #[serde(default)]
        pub stepover: Option<f64>,
        /// Direction of the passes, degrees counter-clockwise from +X. Default: 0.
        #[serde(default)]
        pub angle_deg: Option<f64>,
        /// Every other pass runs back, linked at depth beside the stock; otherwise the tool rises
        /// between passes. Default: true.
        #[serde(default)]
        pub zigzag: Option<bool>,
        /// How far the cutter reaches past the area's edges on the outermost passes, mm.
        /// Default: half the tool's diameter and 0.5.
        #[serde(default)]
        pub overhang: Option<f64>,
        /// The area to face, [[low x, low y], [high x, high y]]. Default: the stock seen from
        /// above, else the part's extent.
        #[serde(default)]
        pub rect: Option<[[f64; 2]; 2]>,
        /// The radius of a vertical lead arc, mm: each pass is entered down a quarter arc in the
        /// vertical plane of the passes from this far above and behind its start, and left up one
        /// beyond its end (`G18`/`G19` arcs when the passes run along X or Y); with `zigzag` the
        /// passes of a level link round half circles outside the area. Default: none (straight
        /// down beside the stock, straight links).
        #[serde(default)]
        pub lead_radius: Option<f64>,
    }
}

/// The stock the job was checked against, as resolved (an `around_part` stock as its box).
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct StockReport {
    /// `box` or `cylinder`.
    pub kind: String,
    /// The box's corners, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<[f64; 3]>,
    /// The cylinder's radius and extent along X, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_max: Option<f64>,
}

/// The rapid check: every `G0` block of the program as written, sampled every half millimetre.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct RapidsReport {
    /// Rapid blocks checked.
    pub checked: usize,
    /// Rapid blocks that run into something (each block once per kind).
    pub hits: usize,
    /// How deep the worst of them goes, mm (0: none).
    pub worst: f64,
    pub pass: bool,
    /// Rapid blocks that start where the machine cannot be followed (after a return to its home,
    /// a move in machine coordinates, a tool change): only their ends are checked.
    pub from_unknown: usize,
    /// The first hits.
    pub first: Vec<RapidHitReport>,
}

/// A rapid block that runs into something.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RapidHitReport {
    /// The program line, from 1.
    pub line: usize,
    /// The operation it belongs to, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// `part` (the part or a fixture), `holder`, `stock` or `turn` (a rotary turning the work
    /// into the tool).
    pub kind: String,
    /// How deep, mm.
    pub depth: f64,
    /// The tool tip there, work coordinates.
    pub at: [f64; 3],
}

/// The linear axes against the machine's travel (machines with `linear_limits` only).
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TravelReport {
    /// Motion blocks checked.
    pub checked: usize,
    /// The first position beyond the travel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub violation: Option<String>,
    pub pass: bool,
}
