//! Saved weld graphics: manufacturing settings and optional symbols, never features or bodies.
use serde::{Deserialize, Serialize};
use solidcraft_geom::weld::{MAX_DIMENSION_MM, MIN_DIMENSION_MM, Weld};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_WELDS: usize = 1000;
pub const MAX_PICKS: usize = 256;
pub const TYPES: &[&str] = &["fillet", "butt", "edge", "plug", "spot", "seam", "stud", "surfacing", "tack"];
pub const PICK_KINDS: &[&str] = &["line", "groove", "point", "hole", "ring", "area"];
pub const SIZE_KEYS: &[&str] = &["a", "z", "z1", "h", "s", "d", "fill", "w", "t", "l", "e"];
/// Size names belonging to the chosen type.
pub fn size_keys(weld_type: &str) -> &'static [&'static str] {
    match weld_type {
        "fillet" => &["a", "z", "z1"],
        "butt" => &["h", "s"],
        "edge" | "seam" => &["w"],
        "plug" => &["fill"],
        "spot" | "stud" => &["d"],
        "surfacing" => &["t"],
        "tack" => &["l", "e"],
        _ => &[],
    }
}
/// Intermittent elements; pitch is measured centre to centre in millimetres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeldSpans {
    pub count: u32,
    pub length_mm: f64,
    pub pitch_mm: f64,
}
impl WeldSpans {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=10000).contains(&self.count) || ![self.length_mm, self.pitch_mm].into_iter().all(|v| v.is_finite() && v > 0.) {
            return Err("spans need count 1..=10000 and positive finite length and pitch".into());
        }
        Ok(())
    }
}
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
    pub spans: Option<WeldSpans>,
    pub both_sides: bool,
    pub staggered: bool,
    pub sharp_mitre: bool,
    pub merge_crossing: bool,
    pub different_halves: bool,
    pub field_weld: bool,
    pub all_around: bool,
    pub passes_shown: bool,
    pub penetration_shown: bool,
    pub side: String,
    pub ends: String,
    pub process: Option<String>,
    pub spot_look: String,
    pub reference_plane: String,
    pub density: Option<f64>,
    pub overlap: Option<f64>,
}
impl Default for WeldSettings {
    fn default() -> Self {
        Self {
            weld_type: "fillet".into(),
            picks: BTreeMap::new(),
            size: [("a".into(), 3.)].into_iter().collect(),
            face: "convex".into(),
            size_by_leg: false,
            blend_toes: false,
            sheet_picked: false,
            tangent_chain: false,
            spans: None,
            both_sides: false,
            staggered: false,
            sharp_mitre: false,
            merge_crossing: true,
            different_halves: false,
            field_weld: false,
            all_around: false,
            passes_shown: false,
            penetration_shown: false,
            side: "arrow".into(),
            ends: "square".into(),
            process: None,
            spot_look: "marks".into(),
            reference_plane: "sketch".into(),
            density: None,
            overlap: None,
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
            || self.size.iter().any(|(k, v)| !size_keys(&self.weld_type).contains(&k.as_str()) || !dimension(*v))
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
        for (name, value, allowed) in [
            ("side", self.side.as_str(), &["arrow", "other"][..]),
            ("ends", self.ends.as_str(), &["square", "tapered", "rounded"][..]),
            ("spot_look", self.spot_look.as_str(), &["marks", "ball", "marks_ball", "marks_pin", "pin", "dent"][..]),
            ("reference_plane", self.reference_plane.as_str(), &["sketch"][..]),
        ] {
            if !allowed.contains(&value) {
                return Err(format!("unknown {name}"));
            }
        }
        if self.process.as_ref().is_some_and(|p| !(1..=3).contains(&p.len()) || !p.bytes().all(|b| b.is_ascii_digit())) {
            return Err("process must be 1-3 ASCII digits".into());
        }
        if let Some(spans) = &self.spans {
            spans.validate()?;
        }
        if [self.density, self.overlap].into_iter().flatten().any(|v| !dimension(v)) {
            return Err("density and overlap must be finite and between 1e-6 and 100000".into());
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
