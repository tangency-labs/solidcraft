//! The Tab and Slot feature (#17422): tabs on flat plates that fit slots cut in the flat plates
//! they stand on. This file grows with the doc layer's slice 3b: the picks' checks first, then the
//! settings, the joint planes, the joints' order and the cuts.

use std::collections::BTreeMap;

use solidcraft_kernel::Body;
use solidcraft_kernel::tab_slot::{self as kernel_ts, Layout, Plane, TabSlotSpec};

use super::{ModelState, append_warning, val};
use crate::expr::{Kind, Value};
use crate::lineage::Traced;
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

/// The tolerance of the joints' contacts and plans.
const JOINT_TOL: f64 = 1.0e-3;

/// `b`'s extent along `p`'s normal: the slot plate's thickness where a tab stands on it (its
/// box's, so a reach that may be long on a plate at an angle to the axes, never short).
fn extent_along(b: &Body, p: &Plane) -> Result<f64> {
    let bx = b.exact_bounds()?;
    let n = p.normal;
    Ok(n.x.abs() * (bx.max.x - bx.min.x) + n.y.abs() * (bx.max.y - bx.min.y) + n.z.abs() * (bx.max.z - bx.min.z))
}

/// The tool faces each plate's boolean takes (`per_plate`, by index into `names`) against `max`
/// and `slow` (design v2 6.4): past `max` the feature is refused naming the plate
/// (`too_many_tool_faces`), past `slow` it is made with a warning (`slow`).
fn check_tool_faces(per_plate: &[usize], names: &[String], max: usize, slow: usize, warning: &mut Option<String>) -> Result<()> {
    let name = |i: usize| names.get(i).map_or("?", String::as_str);
    match kernel_ts::check_tool_faces(per_plate, max, slow) {
        Some(kernel_ts::ToolFaceCap::TooMany { plate, faces }) => Err(DocError::Invalid(format!(
            "tab and slot: {} would take {faces} tool faces, more than {max}: use fewer or wider tabs (too_many_tool_faces)",
            name(plate)
        ))),
        Some(kernel_ts::ToolFaceCap::Slow { plate, faces }) => {
            append_warning(warning, format!("tab and slot: {} takes {faces} tool faces and may be slow (slow)", name(plate)));
            Ok(())
        }
        None => Ok(()),
    }
}

/// One plate pair of a Tab and Slot feature with a joint (design 5.7), for the feature's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSlotJoint {
    /// The tab plate's body.
    pub tab: String,
    /// The slot plate's body.
    pub slot: String,
    /// Why the joint was not cut, in the user's words (`None`: cut).
    pub skipped: Option<String>,
    /// The skip's reason id of design 11.3 (`swapped`, `meets_other_plate_joint`, ...; `None`: cut).
    pub reason: Option<String>,
}

/// The most skipped joints the feature's warning names before "and N more" (design 11.6).
const SKIPS_NAMED: usize = 6;

/// A pair's skip in the user's words (design 11.3), after "Wall → Bottom".
fn pair_skip_text(skip: &kernel_ts::PairSkip) -> String {
    use kernel_ts::PairSkip as P;
    match skip {
        P::Contact(kernel_ts::Contact::NoContact { distance }) => format!("does not meet it ({distance:.2} mm apart)"),
        P::Contact(kernel_ts::Contact::EndNotFlat) => "meets it only along a line or at a point".into(),
        P::Contact(_) | P::NoPlateFaces(_) => "is not a plate where they meet (no two parallel flat faces)".into(),
        P::NotSquare(Some(angle)) => format!("is not at 90° to it ({angle:.1}°)"),
        P::NotSquare(None) => "lies face to face on it".into(),
        P::Swapped => "is the other way round: its face meets the slot plate's edge (swap them)".into(),
        P::PassesThrough => "runs out past its far face".into(),
        P::SeveralContacts => "meets it in several places: pick the face".into(),
    }
}

/// A joint's skip in the user's words (design 11.3), naming the earlier joint (`earlier[j]`,
/// "Back → Bottom") a skip refers to.
fn joint_skip_text(skip: &kernel_ts::JointSkip, earlier: &[String]) -> String {
    let named = |j: &usize| earlier.get(*j).map_or("another plate joint", String::as_str).to_string();
    match skip {
        kernel_ts::JointSkip::Pair(p) => pair_skip_text(p),
        kernel_ts::JointSkip::MeetsOtherJoint(j) => format!("its slots come too close to those of {}", named(j)),
        kernel_ts::JointSkip::OtherDirection(j) => format!("a plate already takes {} in another direction", named(j)),
    }
}

/// The feature's warning for its skipped joints (design 11.6): "skipped 2 plate joints: Back →
/// Bottom too short, Left → Bottom swapped", up to [`SKIPS_NAMED`] named, then "and N more".
/// `no_contact` skips are left out (the reply names them).
fn skipped_note(joints: &[TabSlotJoint]) -> Option<String> {
    let skipped: Vec<String> = joints
        .iter()
        .filter(|j| j.reason.as_deref().is_some_and(|r| r != "no_contact"))
        .map(|j| format!("{} → {} {}", j.tab, j.slot, j.skipped.as_deref().unwrap_or("")))
        .collect();
    if skipped.is_empty() {
        return None;
    }
    let n = skipped.len();
    let mut note = format!("tab and slot: skipped {n} plate joint{}: ", if n == 1 { "" } else { "s" });
    note.push_str(&skipped.iter().take(SKIPS_NAMED).cloned().collect::<Vec<_>>().join(", "));
    if n > SKIPS_NAMED {
        note = format!("{note} and {} more", n - SKIPS_NAMED);
    }
    Some(note)
}

/// What a Tab and Slot feature did (design 5.7): its joints in pick order and the tool faces each
/// plate's boolean took (by body, in pick order).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabSlotReport {
    /// The plate pairs with a joint, in the order they were cut.
    pub joints: Vec<TabSlotJoint>,
    /// Each plate's tool faces over the made joints.
    pub tool_faces: Vec<(String, usize)>,
}

/// The feature `k` applied to `st` (design 5.7): every tab plate against every slot plate, the
/// pairs with a joint in pick order, each made joint cut in that order (a plate in several joints
/// takes the previous joint's result), the plates replaced in place. Pairs that do not meet are
/// passed by; a joint skipped, or a pair the cut refuses, is named in `warning` and the report.
pub(crate) fn apply(vals: &BTreeMap<String, Value>, k: &FeatureKind, st: &mut ModelState, warning: &mut Option<String>) -> Result<TabSlotReport> {
    let FeatureKind::TabSlot { tab_plates, slot_plates, .. } = k else {
        return Err(DocError::Invalid("not a tab and slot".into()));
    };
    if let Some(bad) = bad_plate_key(tab_plates, slot_plates) {
        return Err(DocError::Invalid(format!("tab and slot: {bad}")));
    }
    // The plates, each once, as the state has them.
    let mut names: Vec<String> = Vec::new();
    for p in tab_plates.iter().chain(slot_plates) {
        let name = p.body.to_string();
        if st.body(&name).is_none() {
            return Err(DocError::Invalid(format!("tab and slot: no body {name}")));
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let index = |n: &str| names.iter().position(|m| m == n).unwrap_or(0);
    let mut plates: Vec<Body> = names.iter().filter_map(|n| st.body(n)).map(|b| b.body.clone()).collect();
    // The pairs with a joint, in pick order, with their settings.
    let mut list: Vec<(usize, usize, Plane)> = Vec::new();
    let mut specs: Vec<TabSlotSpec> = Vec::new();
    for t in tab_plates {
        for s in slot_plates {
            let (ti, si) = (index(&t.body), index(&s.body));
            if ti == si {
                continue;
            }
            let near = s.near.or(t.near).map(Into::into);
            let Some((tab, slot)) = plates.get(ti).zip(plates.get(si)) else { continue };
            let Ok((p, joint)) = kernel_ts::joint_plane(tab, slot, near, JOINT_TOL)? else { continue };
            specs.push(spec_of(vals, k, joint.faces.thickness, extent_along(slot, &p)?)?);
            list.push((ti, si, p));
        }
    }
    if specs.is_empty() {
        append_warning(warning, "tab and slot: no tab plate stands on a slot plate".into());
        return Ok(TabSlotReport::default());
    }
    let refs: Vec<&Body> = plates.iter().collect();
    let (skips, tool_faces) = kernel_ts::joints_each(&refs, &list, &specs, JOINT_TOL)?;
    let pair_names: Vec<String> = list.iter().map(|(ti, si, _)| format!("{} → {}", names[*ti], names[*si])).collect();
    check_tool_faces(&tool_faces, &names, kernel_ts::MAX_TOOL_FACES, kernel_ts::SLOW_TOOL_FACES, warning)?;
    let mut report = TabSlotReport { joints: Vec::new(), tool_faces: names.iter().cloned().zip(tool_faces).collect() };
    for (j, ((ti, si, p), spec)) in list.iter().zip(&specs).enumerate() {
        let (tab_name, slot_name) = (names[*ti].clone(), names[*si].clone());
        let skipped = if let Some(Some(skip)) = skips.get(j) {
            Some((skip.reason_id(), joint_skip_text(skip, &pair_names)))
        } else if let Some((tab, slot)) = plates.get(*ti).zip(plates.get(*si)) {
            match kernel_ts::cut(tab, slot, *p, *spec, JOINT_TOL)? {
                Ok(c) => {
                    plates[*ti] = c.tab_plate;
                    plates[*si] = c.slot_plate;
                    None
                }
                Err(skip) => Some((skip.reason_id(), pair_skip_text(&skip))),
            }
        } else {
            continue;
        };
        let (reason, skipped) = skipped.map_or((None, None), |(id, text)| (Some(id.to_string()), Some(text)));
        report.joints.push(TabSlotJoint { tab: tab_name, slot: slot_name, skipped, reason });
    }
    if let Some(note) = skipped_note(&report.joints) {
        append_warning(warning, note);
    }
    for (name, made) in names.iter().zip(plates) {
        if let Some(mb) = st.bodies.iter_mut().find(|b| &b.name == name)
            && !mb.body.same(&made)
        {
            *mb = mb.remade(Traced::unnamed(made));
        }
    }
    Ok(report)
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

    /// Fixture F (design 16): Bottom (0,0,0)-(160,100,6), Wall (20,30,6)-(140,36,66) on its top;
    /// E1 laser (count 3, W 20, e 10, c 0.1) through the eval: Wall 45 360 and Bottom 93 745.68 mm³.
    #[test]
    fn tab_slot_apply_cuts_one_pair_on_fixture_f() {
        use solidcore_record::Vec3;
        use solidcraft_kernel as kernel;
        let mut st = ModelState::default();
        let bottom = kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(160.0, 100.0, 6.0)).unwrap();
        let wall = kernel::box_solid(Vec3::new(20.0, 30.0, 6.0), Vec3::new(140.0, 36.0, 66.0)).unwrap();
        st.bodies.push(super::super::ModelBody::new("Bottom".into(), bottom, 0));
        st.bodies.push(super::super::ModelBody::new("Wall".into(), wall, 0));
        let k = kind(serde_json::json!({"clearance": "0.1", "count": "3", "tab_width": "20", "end_distance": "10"}));
        let mut warning = None;
        apply(&BTreeMap::new(), &k, &mut st, &mut warning).unwrap();
        assert!(warning.is_none(), "{warning:?}");
        let volume = |n: &str| kernel::measure_mass(&st.body(n).unwrap().body).unwrap().volume;
        assert!((volume("Wall") - 45_360.0).abs() < 1e-6, "{}", volume("Wall"));
        assert!((volume("Bottom") - 93_745.68).abs() < 1e-6, "{}", volume("Bottom"));
    }

    /// Design v2 6.4: past the most tool faces the feature is refused naming the plate, past the
    /// slow count it is made with a warning, below both nothing is said.
    #[test]
    fn tab_slot_tool_face_caps_refuse_or_warn_naming_the_plate() {
        let names = vec!["Bottom".to_string(), "Wall".to_string()];
        let mut warning = None;
        check_tool_faces(&[24, 12], &names, 2_000, 800, &mut warning).unwrap();
        assert!(warning.is_none(), "{warning:?}");
        check_tool_faces(&[12, 900], &names, 2_000, 800, &mut warning).unwrap();
        let said = warning.unwrap();
        assert!(said.contains("Wall") && said.contains("900") && said.contains("slow"), "{said}");
        let mut warning = None;
        let err = check_tool_faces(&[2_150, 12], &names, 2_000, 800, &mut warning).unwrap_err().to_string();
        assert!(err.contains("Bottom") && err.contains("2150") && err.contains("too_many_tool_faces"), "{err}");
        assert!(warning.is_none(), "{warning:?}");
    }

    /// Fixture F through the eval: the report names the one joint as cut and counts tool faces on
    /// both plates, in pick order (tab plate first).
    #[test]
    fn tab_slot_apply_reports_the_joint_and_tool_faces_on_fixture_f() {
        use solidcore_record::Vec3;
        use solidcraft_kernel as kernel;
        let mut st = ModelState::default();
        let bottom = kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(160.0, 100.0, 6.0)).unwrap();
        let wall = kernel::box_solid(Vec3::new(20.0, 30.0, 6.0), Vec3::new(140.0, 36.0, 66.0)).unwrap();
        st.bodies.push(super::super::ModelBody::new("Bottom".into(), bottom, 0));
        st.bodies.push(super::super::ModelBody::new("Wall".into(), wall, 0));
        let k = kind(serde_json::json!({"clearance": "0.1", "count": "3", "tab_width": "20", "end_distance": "10"}));
        let mut warning = None;
        let report = apply(&BTreeMap::new(), &k, &mut st, &mut warning).unwrap();
        assert_eq!(report.joints.len(), 1, "{report:?}");
        let joint = &report.joints[0];
        assert_eq!((joint.tab.as_str(), joint.slot.as_str(), joint.skipped.as_deref()), ("Wall", "Bottom", None));
        assert_eq!(report.tool_faces.len(), 2, "{report:?}");
        assert!(report.tool_faces.iter().all(|(_, n)| *n > 0), "{report:?}");
    }

    fn joint(tab: &str, reason: Option<&str>, text: &str) -> TabSlotJoint {
        TabSlotJoint { tab: tab.into(), slot: "Bottom".into(), skipped: reason.map(|_| text.to_string()), reason: reason.map(Into::into) }
    }

    /// Design 11.3 and 11.6: skips read as the user's words keyed by the reason id; the warning
    /// names up to six ("Back → Bottom ..."), then "and N more", and leaves `no_contact` out.
    #[test]
    fn tab_slot_skips_read_as_user_text_by_reason_id() {
        use kernel_ts::{JointSkip, PairSkip};
        let earlier = vec!["Back → Bottom".to_string()];
        assert_eq!(JointSkip::Pair(PairSkip::Swapped).reason_id(), "swapped");
        assert!(joint_skip_text(&JointSkip::Pair(PairSkip::Swapped), &earlier).contains("swap"));
        assert!(joint_skip_text(&JointSkip::Pair(PairSkip::NotSquare(Some(45.0))), &earlier).contains("45.0°"));
        assert!(joint_skip_text(&JointSkip::Pair(PairSkip::NotSquare(None)), &earlier).contains("face to face"));
        let meets = joint_skip_text(&JointSkip::MeetsOtherJoint(0), &earlier);
        assert!(meets.contains("Back → Bottom") && !meets.contains("MeetsOtherJoint"), "{meets}");
        assert!(joint_skip_text(&JointSkip::OtherDirection(0), &earlier).contains("another direction"));
        assert_eq!(skipped_note(&[joint("Wall", None, ""), joint("Far", Some("no_contact"), "does not meet it")]), None);
        let note = skipped_note(&[joint("Wall", None, ""), joint("Left", Some("swapped"), "is the other way round")]).unwrap();
        assert_eq!(note, "tab and slot: skipped 1 plate joint: Left → Bottom is the other way round");
        let many: Vec<TabSlotJoint> = (0..8).map(|i| joint(&format!("W{i}"), Some("passes_through"), "runs out")).collect();
        let note = skipped_note(&many).unwrap();
        assert!(note.starts_with("tab and slot: skipped 8 plate joints: W0 → Bottom runs out") && note.ends_with("and 2 more"), "{note}");
        assert!(!note.contains("W6"), "{note}");
    }

    /// Two plate pairs of different thickness in one feature: each joint is laid out with its own
    /// default tab width (3 × its tab plate's thickness), for the skips and the cut alike. Bottom
    /// 160 × 100 × 6; Thin 6 and Thick 10 thick, 120 long, 60 high, two tabs each 10 from the
    /// ends, clearance 0.1: Thin gains 2 × 18 × 6 × 6, Thick 2 × 30 × 10 × 6, Bottom
    /// loses 2 × 18.2 × 6.2 × 6 + 2 × 30.2 × 10.2 × 6.
    #[test]
    fn tab_slot_apply_lays_out_each_pair_by_its_own_thickness() {
        use solidcore_record::Vec3;
        use solidcraft_kernel as kernel;
        let mut st = ModelState::default();
        let bottom = kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(160.0, 100.0, 6.0)).unwrap();
        let thin = kernel::box_solid(Vec3::new(20.0, 30.0, 6.0), Vec3::new(140.0, 36.0, 66.0)).unwrap();
        let thick = kernel::box_solid(Vec3::new(20.0, 60.0, 6.0), Vec3::new(140.0, 70.0, 66.0)).unwrap();
        st.bodies.push(super::super::ModelBody::new("Bottom".into(), bottom, 0));
        st.bodies.push(super::super::ModelBody::new("Thin".into(), thin, 0));
        st.bodies.push(super::super::ModelBody::new("Thick".into(), thick, 0));
        let k: FeatureKind = serde_json::from_value(serde_json::json!({
            "type": "tab_slot",
            "tab_plates": [{"key": 1, "body": "Thin"}, {"key": 2, "body": "Thick"}],
            "slot_plates": [{"key": 3, "body": "Bottom"}],
            "clearance": "0.1",
            "count": "2",
            "end_distance": "10",
        }))
        .unwrap();
        let mut warning = None;
        let report = apply(&BTreeMap::new(), &k, &mut st, &mut warning).unwrap();
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(report.joints.len(), 2, "{report:?}");
        assert!(report.joints.iter().all(|j| j.skipped.is_none() && j.reason.is_none()), "{report:?}");
        let volume = |n: &str| kernel::measure_mass(&st.body(n).unwrap().body).unwrap().volume;
        let want = [
            ("Thin", 43_200.0 + 2.0 * 18.0 * 6.0 * 6.0),
            ("Thick", 72_000.0 + 2.0 * 30.0 * 10.0 * 6.0),
            ("Bottom", 96_000.0 - 2.0 * 18.2 * 6.2 * 6.0 - 2.0 * 30.2 * 10.2 * 6.0),
        ];
        for (n, v) in want {
            assert!((volume(n) - v).abs() < 1e-6, "{n}: {} not {v}", volume(n));
        }
    }
}
