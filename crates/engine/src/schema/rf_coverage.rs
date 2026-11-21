//! Contract and result types for `inspect.rf_coverage` in `cmd/rf_coverage.rs` (#20161). The
//! handler reads its parameters through this contract, so the two always agree.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A band by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
pub enum BandName {
    #[serde(rename = "2.4")]
    G2_4,
    #[serde(rename = "5")]
    G5,
    #[serde(rename = "6")]
    G6,
    /// Every band an emitter has a radio in.
    #[serde(rename = "all")]
    All,
}

/// An emitter's power: one EIRP for every band, or one per band (dBm).
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EirpParam {
    Every(f64),
    PerBand(BTreeMap<BandName, f64>),
}

/// A Wi-Fi router or access point.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct EmitterParam {
    /// Its name in the reply (default: `AP1`, `AP2`…).
    #[serde(default)]
    pub name: Option<String>,
    /// Where it is: world [x, y] in mm, or [x, y, z] with z its height.
    pub at: Vec<f64>,
    /// Its height in mm, world z (default: `client_height`, or the z of `at`).
    #[serde(default)]
    pub height: Option<f64>,
    /// A device from the built-in library or the `devices_file` (a call with no emitters lists
    /// them): its EIRP per band, capped at indoor limits. Not with `eirp_dbm`.
    #[serde(default)]
    pub device: Option<String>,
    /// Its EIRP in dBm: one number for every band, or {"2.4": dBm, "5": dBm, "6": dBm} (a band left
    /// out has no radio). Default with neither: 20 dBm in every band.
    #[serde(default)]
    pub eirp_dbm: Option<EirpParam>,
}

/// A point to report the signal at, such as the middle of a room.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct PointParam {
    pub name: String,
    /// World [x, y] in mm.
    pub at: [f64; 2],
}

/// The floor the statistics count.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct RegionParam {
    /// A closed outline, world [[x, y], …] in mm.
    #[serde(default)]
    pub polygon: Option<Vec<[f64; 2]>>,
    /// A body whose outline from above, cut at the middle of its height, is the floor (a floor slab).
    #[serde(default)]
    pub body: Option<String>,
}

/// Predict Wi-Fi signal strength over a floor plan from the walls the model's bodies make at a
/// height, and show it as a heatmap. View only: the design doesn't change.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct RfCoverageParams {
    /// The routers or access points (at least one, at most 64).
    #[serde(default)]
    pub emitters: Option<Vec<EmitterParam>>,
    /// The band to work out (default "all": each band an emitter has). The heatmap shows this
    /// band, or 5 GHz with "all".
    #[serde(default)]
    pub band: Option<BandName>,
    /// The height the bodies are cut at to find the walls, mm (default 1000).
    #[serde(default)]
    pub slice_height: Option<f64>,
    /// The height of the receiving phone or laptop, mm (default 1000).
    #[serde(default)]
    pub client_height: Option<f64>,
    /// The grid's cell size, mm: 10 to 2000 (default 75).
    #[serde(default)]
    pub grid_step: Option<f64>,
    /// The path-loss exponent: 1.5 to 6 (default 2.2; 2 is free space).
    #[serde(default)]
    pub exponent: Option<f64>,
    /// A JSON file of devices `device` can name, besides the built-in ones: a list of
    /// {id, name?, eirp_dbm: {"2.4": dBm, "5": dBm, "6": dBm}} (other keys are ignored). Its ids
    /// replace built-in ones of the same id.
    #[serde(default)]
    pub devices_file: Option<String>,
    /// The floor the statistics count (default: the floor the walls enclose; when they enclose
    /// nothing, every cell not in a wall).
    #[serde(default)]
    pub region: Option<RegionParam>,
    /// Without `region`: openings narrower than this (mm) close the floor the walls enclose, so
    /// open doorways don't let the outside in: 0 to 5000 (default 1000).
    #[serde(default)]
    pub close_gaps: Option<f64>,
    /// Points to report the signal at.
    #[serde(default)]
    pub points: Option<Vec<PointParam>>,
    /// Only these bodies make walls (names).
    #[serde(default)]
    pub bodies: Option<Vec<String>>,
    /// Only the bodies of these components, their subcomponents included, make walls (names).
    #[serde(default)]
    pub components: Option<Vec<String>>,
    /// With `bodies` or `components`: bodies whose material has an RF loss make walls too, wherever
    /// they are (default false).
    #[serde(default)]
    pub rf_materials: Option<bool>,
    /// Bodies or components (names) left out of the analysis: furniture, plants.
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
    /// Hidden bodies make walls too (default false: only those whose material has an RF loss).
    #[serde(default)]
    pub include_hidden: Option<bool>,
    /// Cap each emitter's EIRP at the indoor limits (default true): 30 dBm at 2.4 and 5 GHz, 27 dBm
    /// at 6 GHz.
    #[serde(default)]
    pub cap_eirp: Option<bool>,
    /// Write the heatmap as a PNG here (a path ending in .png).
    #[serde(default)]
    pub png: Option<String>,
    /// Return the grid's values too (default false: the statistics only).
    #[serde(default)]
    pub grid: Option<bool>,
    /// Show the heatmap in the 3D view (default true).
    #[serde(default)]
    pub show: Option<bool>,
    /// Turn the heatmap off; nothing else may be given.
    #[serde(default)]
    pub clear: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GridReply {
    /// The corner of the first cell, world [x, y] in mm.
    pub origin: [f64; 2],
    pub step: f64,
    pub columns: usize,
    pub rows: usize,
    /// The cells the statistics count.
    pub floor_cells: usize,
    /// The floor's area, m².
    pub floor_m2: f64,
    /// Where the floor came from: "enclosed" (by the walls), "open" (every cell not in a wall:
    /// the walls enclose nothing), "polygon" or "body".
    pub region: String,
    /// With `grid: true`: each band's RSSI by cell, row by row from the lowest y, null outside the floor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<BTreeMap<String, Vec<Option<f32>>>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WallsReply {
    /// The bodies the slice cut.
    pub bodies: usize,
    pub regions: usize,
    /// How many regions each material accounts for; guessed ones end in "(guessed)".
    pub materials: BTreeMap<String, usize>,
    /// Bodies left out because they couldn't be cut, with why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EmitterReply {
    pub name: String,
    pub at: [f64; 2],
    pub height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The EIRP used per band, after any cap (dBm).
    pub eirp_dbm: BTreeMap<String, f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BandStats {
    pub band: String,
    /// The share of the floor at −60 dBm or better (fast), 0–100.
    pub pct_at_least_60: f64,
    /// The share at −67 dBm or better (video calls), 0–100.
    pub pct_at_least_67: f64,
    /// The weakest 5% of the floor gets this or less (dBm).
    pub p5_dbm: f64,
    pub median_dbm: f64,
    pub min_dbm: f64,
    pub max_dbm: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PointReply {
    pub name: String,
    pub at: [f64; 2],
    /// The best signal by band (dBm).
    pub rssi_dbm: BTreeMap<String, f64>,
    /// The emitter that gives it, by band.
    pub from: BTreeMap<String, String>,
    /// The wall losses on the way from that emitter, by band (dB).
    pub walls_db: BTreeMap<String, f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeviceReply {
    pub id: String,
    pub name: String,
    /// EIRP per band before the cap (dBm).
    pub eirp_dbm: BTreeMap<String, f64>,
    /// "built-in" or "file" (the `devices_file`).
    pub source: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RfCoverageReply {
    /// False after `clear`.
    pub shown: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridReply>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub walls: Option<WallsReply>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub emitters: Vec<EmitterReply>,
    /// The coverage figures by band.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stats: Vec<BandStats>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<PointReply>,
    /// The band the heatmap shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub showing: Option<String>,
    /// The PNG written, when `png` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub png: Option<String>,
    /// The built-in devices `device` takes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<DeviceReply>,
    /// Whether the walls came from the last call's slice (an emitter moved, nothing else).
    pub walls_reused: bool,
    pub ms: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
