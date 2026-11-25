//! Display opacity: how opaque a body or a placement is drawn in the modelling view, in percent.
//!
//! It is view state like what is hidden ([`crate::Visibility`]), apart from the appearances
//! ([`crate::appearance`]): it never changes a look, so 100% shows a body exactly as its
//! appearance says. Renders and exports read appearances alone.
//!
//! A body's percentage, those of the placements above it and its appearance's opacity multiply.
//! 100% is no entry at all.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS};

/// Fully opaque, in percent: what a body or placement without an entry is.
pub const OPAQUE: u8 = 100;

/// The percentages set, by target.
#[derive(Clone, Debug, Default, Hash, PartialEq)]
pub struct DisplayOpacity {
    /// By the body's name in the placed design (`Lid (K:2)`), as hidden bodies are named.
    bodies: BTreeMap<String, u8>,
    /// By exact placement: it and everything in it.
    placements: BTreeMap<Vec<u64>, u8>,
}

/// One placement's percentage, as a design file and a command's reply list it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacementOpacity {
    /// Occurrence ids from the design down.
    pub path: Vec<u64>,
    /// 0 (not drawn) to 99.
    pub percent: u8,
}

/// Is nothing set? (What the saved view leaves out.)
pub(crate) fn unset(opacity: &std::sync::Arc<DisplayOpacity>) -> bool {
    opacity.is_empty()
}

fn set<K: Ord>(map: &mut BTreeMap<K, u8>, key: K, percent: u8) {
    if percent >= OPAQUE {
        map.remove(&key);
    } else {
        map.insert(key, percent);
    }
}

impl DisplayOpacity {
    /// Is every body and placement at 100%?
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.placements.is_empty()
    }

    /// How many bodies and placements are below 100%.
    pub fn len(&self) -> usize {
        self.bodies.len().saturating_add(self.placements.len())
    }

    /// The bodies below 100%.
    pub fn bodies(&self) -> &BTreeMap<String, u8> {
        &self.bodies
    }

    /// The placements below 100%, in path order.
    pub fn placements(&self) -> impl Iterator<Item = PlacementOpacity> + '_ {
        self.placements.iter().map(|(path, percent)| PlacementOpacity { path: path.clone(), percent: *percent })
    }

    /// A body's own percentage.
    pub fn body(&self, name: &str) -> u8 {
        self.bodies.get(name).copied().unwrap_or(OPAQUE)
    }

    /// A placement's own percentage.
    pub fn placement(&self, path: &[u64]) -> u8 {
        self.placements.get(path).copied().unwrap_or(OPAQUE)
    }

    /// Set a body's percentage; 100 and above clears it.
    pub fn set_body(&mut self, name: &str, percent: u8) {
        set(&mut self.bodies, name.to_string(), percent);
    }

    /// Set a placement's percentage; 100 and above clears it.
    pub fn set_placement(&mut self, path: &[u64], percent: u8) {
        set(&mut self.placements, path.to_vec(), percent);
    }

    /// How much of their own opacity the bodies placed at `path` keep (0 to 1): the product over
    /// the placement and each one above it.
    pub fn placement_factor(&self, path: &[u64]) -> f64 {
        if self.placements.is_empty() {
            return 1.0;
        }
        (1..=path.len()).filter_map(|n| path.get(..n)).map(|above| f64::from(self.placement(above)) / 100.0).product()
    }

    /// Keep only the placements `kept` says are still there.
    pub fn retain_placements(&mut self, kept: impl Fn(&Vec<u64>) -> bool) {
        self.placements.retain(|path, _| kept(path));
    }

    /// Follow each placement to where `moved` says it went (none: it stays).
    pub fn move_placements(&mut self, moved: impl Fn(&[u64]) -> Option<Vec<u64>>) {
        self.placements = std::mem::take(&mut self.placements).into_iter().map(|(path, percent)| (moved(&path).unwrap_or(path), percent)).collect();
    }

    /// Follow each body to its new name. An entry named like one of the new names that is not
    /// itself renamed is a deleted body's: it goes, so it does not land on the renamed body.
    pub fn rename_bodies(&mut self, names: &BTreeMap<String, String>, destinations: &std::collections::BTreeSet<String>) {
        self.bodies = std::mem::take(&mut self.bodies)
            .into_iter()
            .filter_map(|(name, percent)| match names.get(&name) {
                Some(new) => Some((new.clone(), percent)),
                None => (!destinations.contains(&name)).then_some((name, percent)),
            })
            .collect();
    }
}

impl Serialize for DisplayOpacity {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(skip_serializing_if = "BTreeMap::is_empty")]
            bodies: &'a BTreeMap<String, u8>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            placements: Vec<PlacementOpacity>,
        }
        Wire { bodies: &self.bodies, placements: self.placements().collect() }.serialize(s)
    }
}

/// Read from any design file: what is not a percentage of a body or placement is left out, and
/// never stops the design from opening.
impl<'de> Deserialize<'de> for DisplayOpacity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let percent = |v: &Value| v.as_u64().map(|p| u8::try_from(p).unwrap_or(OPAQUE).min(OPAQUE));
        let mut out = DisplayOpacity::default();
        for (name, v) in value.get("bodies").and_then(Value::as_object).into_iter().flatten().take(MAX_PLACEMENTS) {
            if let Some(p) = percent(v) {
                out.set_body(name, p);
            }
        }
        for v in value.get("placements").and_then(Value::as_array).into_iter().flatten().take(MAX_PLACEMENTS) {
            let path = v
                .get("path")
                .and_then(Value::as_array)
                .filter(|path| !path.is_empty() && path.len() <= MAX_PLACEMENT_DEPTH)
                .and_then(|path| path.iter().map(Value::as_u64).collect::<Option<Vec<_>>>());
            if let (Some(path), Some(p)) = (path, v.get("percent").and_then(percent)) {
                out.set_placement(&path, p);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn percentages_multiply_down_a_path_and_100_is_no_entry() {
        let mut o = DisplayOpacity::default();
        o.set_placement(&[1], 50);
        o.set_placement(&[1, 2], 40);
        o.set_body("Lid", 0);
        assert!((o.placement_factor(&[1, 2, 3]) - 0.2).abs() < 1e-12);
        assert_eq!(o.placement_factor(&[2]), 1.0);
        assert_eq!((o.body("Lid"), o.body("Base"), o.placement(&[1]), o.placement(&[9])), (0, 100, 50, 100));
        o.set_placement(&[1], 100);
        o.set_placement(&[1, 2], 100);
        o.set_body("Lid", 100);
        assert!(o.is_empty() && o == DisplayOpacity::default());
    }

    #[test]
    fn a_saved_opacity_reads_back_and_hostile_entries_are_left_out() {
        let mut o = DisplayOpacity::default();
        o.set_placement(&[4, 9], 30);
        o.set_body("Lid (K:2)", 70);
        let saved = serde_json::to_value(&o).unwrap();
        assert_eq!(saved, json!({"bodies": {"Lid (K:2)": 70}, "placements": [{"path": [4, 9], "percent": 30}]}));
        assert_eq!(serde_json::from_value::<DisplayOpacity>(saved).unwrap(), o);
        let deep: Vec<u64> = (0..65).collect();
        let hostile = json!({
            "bodies": {"A": 101, "B": -1, "C": 1.5, "D": "50", "E": null, "F": u64::MAX, "G": 100, "H": 0},
            "placements": [{"path": [], "percent": 5}, {"path": deep, "percent": 5}, {"path": [-1], "percent": 5},
                {"path": "x", "percent": 5}, {"percent": 5}, 7, {"path": [1], "percent": 20}, {"path": [1], "percent": 60}],
        });
        let read: DisplayOpacity = serde_json::from_value(hostile).unwrap();
        assert_eq!(read.bodies().keys().collect::<Vec<_>>(), ["H"]);
        assert_eq!(read.placements().collect::<Vec<_>>(), [PlacementOpacity { path: vec![1], percent: 60 }], "the last one of a path counts");
        for junk in [json!(null), json!(5), json!([]), json!({"bodies": [], "placements": {}})] {
            assert!(serde_json::from_value::<DisplayOpacity>(junk).unwrap().is_empty());
        }
    }

    /// An entry left by a deleted body does not land on a body renamed to its name.
    #[test]
    fn a_rename_takes_the_entry_along_and_drops_a_stale_one() {
        let mut o = DisplayOpacity::default();
        o.set_body("Old", 30);
        o.set_body("Gone", 80);
        o.set_body("Other", 60);
        o.rename_bodies(&[("Old".to_string(), "Gone".to_string())].into(), &["Gone".to_string()].into());
        assert_eq!(o.bodies().iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>(), [("Gone", 30), ("Other", 60)]);
    }
}
