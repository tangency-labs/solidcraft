//! Saved weld graphics: manufacturing settings and optional symbols, never features or bodies.
use serde::{Deserialize, Serialize};
use solidcraft_geom::weld::{MAX_DIMENSION_MM, MIN_DIMENSION_MM, Weld};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_WELDS: usize = 1000;
pub const MAX_PICKS: usize = 256;
pub const TYPES: &[&str] = &[
    "fillet",
    "groove",
    "groove_fillet",
    "root_gap",
    "edge",
    "flanged_butt",
    "flanged_corner",
    "bead_line",
    "seam_only",
    "rip",
    "spot",
    "projection",
    "fusion_spot",
    "stake",
    "plug",
    "stud",
    "res_seam",
    "fusion_seam",
    "overlay",
    "friction",
    "fsw",
    "ultrasonic",
    "explosion",
    "diffusion",
    "magnetic",
    "cold",
];
pub const PICK_KINDS: &[&str] = &["line", "groove", "point", "hole", "ring", "area"];
pub const SIZE_KEYS: &[&str] = &["a", "z", "z1", "h", "s", "d", "fill", "collar", "t", "w"];
/// Fixed points or exact topology references in the body/index form used by other commands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeldPick {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}
impl WeldPick {
    pub fn validate(&self) -> Result<(), String> {
        let points = [self.point, self.edge, self.face];
        if points.iter().flatten().count() > 1 || points.iter().flatten().flatten().any(|v| !v.is_finite() || v.abs() >= 1e9) {
            return Err("a pick accepts at most one finite point, edge or face coordinate".into());
        }
        if self.point.is_some() {
            if self.body.is_some() || self.index.is_some() {
                return Err("a fixed point has no body or index".into());
            }
        } else if self.body.as_ref().is_none_or(|b| b.trim().is_empty() || b.len() > 1024) || self.index.is_none_or(|i| i > 1_000_000) {
            return Err("an edge or face pick needs body and index".into());
        }
        Ok(())
    }
}
/// Values evaluated at creation: millimetres except count and density (kg/m³).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeldSettings {
    pub weld_type: String,
    pub picks: BTreeMap<String, Vec<WeldPick>>,
    pub size: BTreeMap<String, f64>,
    pub face: String,
    pub size_by_leg: bool,
    pub blend_toes: bool,
    pub sheet_picked: bool,
    pub tangent_chain: bool,
    pub flip_side: bool,
    pub spans: bool,
    pub both_sides: bool,
    pub staggered: bool,
    pub sharp_mitre: bool,
    pub merge_crossing: bool,
    pub different_halves: bool,
    pub tack: bool,
    pub field_weld: bool,
    pub all_around: bool,
    pub passes_shown: bool,
    pub penetration_shown: bool,
    pub root_side: usize,
    pub ends: usize,
    pub process: usize,
    pub texture: usize,
    pub spot_look: usize,
    pub reference_plane: usize,
    pub pattern_count: f64,
    pub pattern_pitch: f64,
    pub edge_distance: f64,
    pub density: f64,
    pub overlap: f64,
}
impl Default for WeldSettings {
    fn default() -> Self {
        Self {
            weld_type: "fillet".into(),
            picks: BTreeMap::new(),
            size: [("a", 3.), ("z", 4.2), ("z1", 3.), ("h", 1.), ("d", 5.), ("fill", 20.), ("collar", 3.), ("t", 2.), ("w", 6.)]
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
            face: "convex".into(),
            size_by_leg: false,
            blend_toes: false,
            sheet_picked: false,
            tangent_chain: false,
            flip_side: false,
            spans: false,
            both_sides: false,
            staggered: false,
            sharp_mitre: false,
            merge_crossing: true,
            different_halves: false,
            tack: false,
            field_weld: false,
            all_around: false,
            passes_shown: false,
            penetration_shown: false,
            root_side: 0,
            ends: 0,
            process: 0,
            texture: 0,
            spot_look: 0,
            reference_plane: 0,
            pattern_count: 4.0,
            pattern_pitch: 40.0,
            edge_distance: 12.0,
            density: 7850.0,
            overlap: 1.5,
        }
    }
}
impl WeldSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !TYPES.contains(&self.weld_type.as_str()) {
            return Err("unknown weld type".into());
        }
        if !["flat", "convex", "concave"].contains(&self.face.as_str()) {
            return Err("unknown weld face".into());
        }
        if self.size.is_empty()
            || self.size.len() > SIZE_KEYS.len()
            || self.size.iter().any(|(k, v)| !SIZE_KEYS.contains(&k.as_str()) || !dimension(*v))
        {
            return Err("size values must be between 1e-6 and 100000 mm with known size keys".into());
        }
        if self.picks.len() > PICK_KINDS.len()
            || self.picks.keys().any(|k| !PICK_KINDS.contains(&k.as_str()))
            || self.picks.values().map(Vec::len).sum::<usize>() > MAX_PICKS
        {
            return Err("invalid weld pick groups or too many picks".into());
        }
        for pick in self.picks.values().flatten() {
            pick.validate()?;
        }
        if self.root_side > 1 {
            return Err("invalid root_side".into());
        }
        if self.ends > 2 {
            return Err("invalid ends".into());
        }
        if self.process > 6 {
            return Err("invalid process".into());
        }
        if self.texture > 6 {
            return Err("invalid texture".into());
        }
        if self.spot_look > 5 {
            return Err("invalid spot_look".into());
        }
        if self.reference_plane != 0 {
            return Err("invalid reference_plane".into());
        }
        if !self.pattern_count.is_finite() || self.pattern_count.fract() != 0. || !(1.0..=10000.0).contains(&self.pattern_count) {
            return Err("pattern count must be an integer between 1 and 10000".into());
        }
        if ![self.pattern_pitch, self.edge_distance, self.overlap, self.density].into_iter().all(dimension) {
            return Err("weld values must be finite and between 1e-6 and 100000".into());
        }
        Ok(())
    }
}
fn dimension(v: f64) -> bool {
    v.is_finite() && (MIN_DIMENSION_MM..=MAX_DIMENSION_MM).contains(&v)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "WrittenRecord")]
pub struct WeldRecord {
    pub id: u64,
    pub number: u32,
    pub settings: WeldSettings,
    pub symbol: Option<Weld>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenRecord {
    id: u64,
    number: u32,
    settings: WeldSettings,
    #[serde(default)]
    symbol: Option<Weld>,
}
impl TryFrom<WrittenRecord> for WeldRecord {
    type Error = String;
    fn try_from(r: WrittenRecord) -> Result<Self, String> {
        if r.id == 0 || r.id > crate::MAX_FEATURE_ID || r.number == 0 || r.number > 1_000_000 {
            return Err("invalid weld identity".into());
        }
        r.settings.validate()?;
        if let Some(symbol) = &r.symbol {
            symbol.validate().map_err(|e| e.to_string())?;
        }
        Ok(Self { id: r.id, number: r.number, settings: r.settings, symbol: r.symbol })
    }
}
/// Stream the list with a cap rather than allocating an unbounded saved list first.
pub(crate) fn deserialize_records<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<WeldRecord>, D::Error> {
    struct Records;
    impl<'de> serde::de::Visitor<'de> for Records {
        type Value = Vec<WeldRecord>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a bounded list of weld graphics")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut records = Vec::new();
            let mut ids = BTreeSet::new();
            let mut numbers = BTreeSet::new();
            while let Some(record) = seq.next_element::<WeldRecord>()? {
                if records.len() >= MAX_WELDS || !ids.insert(record.id) || !numbers.insert(record.number) {
                    return Err(serde::de::Error::custom("too many welds or repeated weld identity"));
                }
                records.push(record);
            }
            Ok(records)
        }
    }
    d.deserialize_seq(Records)
}
