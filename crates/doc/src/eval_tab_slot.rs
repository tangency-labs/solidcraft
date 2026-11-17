//! The Tab and Slot feature (#17422): tabs on flat plates that fit slots cut in the flat plates
//! they stand on. This file grows with the doc layer's slice 3b: the picks' checks first, then the
//! settings, the joint planes, the joints' order and the cuts.

use std::collections::BTreeMap;

use solidcraft_kernel::tab_slot::{Layout, TabSlotSpec};

use super::val;
use crate::expr::{Kind, Value};
use crate::{DocError, FeatureKind, PlatePick, Result};

/// The least land of a slot plate beside a slot, and between two openings (design 0 row 43, 8.4):
/// a floor the layout keeps, not a strength rule.
pub(crate) const MIN_LAND: f64 = 0.5;

/// How far a slot's prism reaches past each face of the slot plate, per thickness of that plate
/// (design 0 row 20, 7.2): a reach, not a tolerance.
pub(crate) const CUT_OVERSHOOT_PER_THICKNESS: f64 = 0.5;

/// The most tabs a stretch is asked for: far past any plate, and small enough for a `u32`.
pub(crate) const MAX_TABS_PER_STRETCH: u32 = 10_000;

/// The recipe's settings for one plate pair: the tab plate `t_tab` and the slot plate `t_slot`
/// thick. The defaults are per pair: the tab width is three times the tab plate's thickness, the
/// end distance is the tab width and the overshoot half the slot plate's thickness.
// The eval arm calls it from the next commit of slice 3b.
#[allow(dead_code)]
pub(crate) fn spec_of(vals: &BTreeMap<String, Value>, k: &FeatureKind, t_tab: f64, t_slot: f64) -> Result<TabSlotSpec> {
    let FeatureKind::TabSlot { count, spacing, tab_width, end_distance, clearance, protrusion, .. } = k else {
        return Err(DocError::Invalid("not a tab and slot".into()));
    };
    let length = |e: &str, what: &str, least: f64| -> Result<f64> {
        let v = val(vals, e, Kind::Length)?;
        if v.is_finite() && v >= least {
            Ok(v)
        } else if least > 0.0 {
            Err(DocError::Invalid(format!("the {what} must be more than 0, not {v}")))
        } else {
            Err(DocError::Invalid(format!("the {what} must be 0 or more, not {v}")))
        }
    };
    let layout = match (count, spacing) {
        (Some(_), Some(_)) => {
            return Err(DocError::Invalid("tabs per stretch and spacing are two ways to lay out the tabs: give one".into()));
        }
        (Some(e), None) => {
            let n = val(vals, e, Kind::Unitless)?;
            if !(n.is_finite() && n.fract() == 0.0 && (1.0..=f64::from(MAX_TABS_PER_STRETCH)).contains(&n)) {
                return Err(DocError::Invalid(format!("tabs per stretch is a whole number from 1 to {MAX_TABS_PER_STRETCH}, not {n}")));
            }
            Layout::Count(n as u32)
        }
        (None, Some(e)) => Layout::Spacing(length(e, "spacing", f64::MIN_POSITIVE)?),
        (None, None) => Layout::Auto,
    };
    let width = match tab_width {
        Some(e) => length(e, "tab width", f64::MIN_POSITIVE)?,
        None => 3.0 * t_tab,
    };
    let end_distance = match end_distance {
        Some(e) => length(e, "end distance", 0.0)?,
        None => width,
    };
    let protrusion = match protrusion {
        Some(e) => {
            let v = val(vals, e, Kind::Length)?;
            if !v.is_finite() {
                return Err(DocError::Invalid(format!("the protrusion must be a length, not {v}")));
            }
            v
        }
        None => 0.0,
    };
    Ok(TabSlotSpec {
        layout,
        width,
        end_distance,
        clearance: length(clearance, "clearance", 0.0)?,
        min_land: MIN_LAND,
        protrusion,
        overshoot: CUT_OVERSHOOT_PER_THICKNESS * t_slot,
    })
}

/// The first key that names more than one of `tab_plates` and `slot_plates` together, or a pick
/// with no key (a key is minted on load, so that is a damaged design). Keys name every tool of a
/// pick's joints: two picks with one key would share their names.
// The eval arm calls it from the next commit of slice 3b.
#[allow(dead_code)]
pub(crate) fn bad_plate_key(tab_plates: &[PlatePick], slot_plates: &[PlatePick]) -> Option<String> {
    let mut seen = std::collections::BTreeSet::new();
    for p in tab_plates.iter().chain(slot_plates) {
        let Some(k) = p.key else {
            return Some(format!("the pick of {} has no key", p.body.0));
        };
        if !seen.insert(k) {
            return Some(format!("two plate picks share key {k}"));
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn kind(v: serde_json::Value) -> FeatureKind {
        let mut v = v;
        v["type"] = "tab_slot".into();
        v["tab_plates"] = serde_json::json!([{"key": 1, "body": "Wall"}]);
        v["slot_plates"] = serde_json::json!([{"key": 2, "body": "Bottom"}]);
        serde_json::from_value(v).unwrap()
    }

    fn spec(v: serde_json::Value) -> Result<TabSlotSpec> {
        spec_of(&BTreeMap::new(), &kind(v), 6.0, 4.0)
    }

    #[test]
    fn tab_slot_spec_of_fills_the_per_pair_defaults() {
        let s = spec(serde_json::json!({"clearance": "0.1"})).unwrap();
        assert!(matches!(s.layout, Layout::Auto));
        assert_eq!((s.width, s.end_distance, s.protrusion), (18.0, 18.0, 0.0));
        assert_eq!((s.clearance, s.min_land, s.overshoot), (0.1, 0.5, 2.0));
        let s = spec(serde_json::json!({"clearance": "0", "tab_width": "10", "protrusion": "-1"})).unwrap();
        assert_eq!((s.width, s.end_distance, s.clearance, s.protrusion), (10.0, 10.0, 0.0, -1.0));
        let s = spec(serde_json::json!({"clearance": "0.1", "tab_width": "10", "end_distance": "0"})).unwrap();
        assert_eq!(s.end_distance, 0.0);
    }

    #[test]
    fn tab_slot_spec_of_lays_out_by_count_or_spacing_never_both() {
        assert!(matches!(spec(serde_json::json!({"clearance": "0.1", "count": "3"})).unwrap().layout, Layout::Count(3)));
        assert!(matches!(spec(serde_json::json!({"clearance": "0.1", "spacing": "40"})).unwrap().layout, Layout::Spacing(s) if s == 40.0));
        let both = spec(serde_json::json!({"clearance": "0.1", "count": "3", "spacing": "40"})).unwrap_err();
        assert!(both.to_string().contains("give one"), "{both}");
        for bad in [
            serde_json::json!({"clearance": "0.1", "count": "0"}),
            serde_json::json!({"clearance": "0.1", "count": "2.5"}),
            serde_json::json!({"clearance": "0.1", "spacing": "0"}),
            serde_json::json!({"clearance": "0.1", "tab_width": "-2"}),
            serde_json::json!({"clearance": "0.1", "end_distance": "-1"}),
            serde_json::json!({"clearance": "-0.1"}),
        ] {
            assert!(spec(bad.clone()).is_err(), "{bad}");
        }
    }

    fn pick(key: Option<u32>, body: &str) -> PlatePick {
        PlatePick { key, body: crate::names::BodyName(body.into()), near: None }
    }

    #[test]
    fn tab_slot_bad_plate_key_finds_duplicates_and_missing_keys() {
        assert_eq!(bad_plate_key(&[pick(Some(1), "A")], &[pick(Some(2), "B")]), None);
        assert!(bad_plate_key(&[pick(Some(1), "A")], &[pick(Some(1), "B")]).unwrap().contains("key 1"));
        assert!(bad_plate_key(&[pick(Some(1), "A"), pick(Some(1), "C")], &[]).is_some());
        assert!(bad_plate_key(&[pick(None, "A")], &[]).unwrap().contains("no key"));
    }
}
