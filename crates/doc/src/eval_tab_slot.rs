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

/// The largest length the feature takes (mm), as the command's: a hand-edited file past it is
/// refused here too, and a protrusion is at least its negative.
pub(crate) const MAX_LENGTH: f64 = 100_000.0;

/// The recipe's settings for one plate pair: the tab plate `t_tab` and the slot plate `t_slot`
/// thick. The defaults are per pair: the tab width is three times the tab plate's thickness, the
/// end distance is the tab width and the overshoot half the slot plate's thickness.
pub(crate) fn spec_of(vals: &BTreeMap<String, Value>, k: &FeatureKind, t_tab: f64, t_slot: f64) -> Result<TabSlotSpec> {
    let FeatureKind::TabSlot { count, spacing, tab_width, end_distance, clearance, protrusion, .. } = k else {
        return Err(DocError::Invalid("not a tab and slot".into()));
    };
    let length = |e: &str, what: &str, least: f64| -> Result<f64> {
        let v = val(vals, e, Kind::Length)?;
        if v.is_finite() && v >= least && v <= MAX_LENGTH {
            Ok(v)
        } else if v > MAX_LENGTH {
            Err(DocError::Invalid(format!("the {what} must be at most {MAX_LENGTH} mm, not {v}")))
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
            if v.abs() > MAX_LENGTH {
                return Err(DocError::Invalid(format!("the protrusion must be from -{MAX_LENGTH} to {MAX_LENGTH} mm, not {v}")));
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
    /// The tabs the joint made (0 when skipped).
    pub tabs: usize,
    /// The stretches and tabs of a cut joint left out (design 11.3: `too_short`, `at_edge`,
    /// `over_hole`, ...), in strip order. A cut joint that made no tab is skipped, named by its
    /// first.
    pub left_out: Vec<TabSlotLeftOut>,
}

/// A stretch or tab of a cut joint that is not made (design 11.3, stretch and tab level).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSlotLeftOut {
    /// The reason id of design 11.3 (`too_short`, `spacing_too_small`, `at_edge`, `over_hole`,
    /// `uneven_far_side`, `no_far_side`, `tab_too_short`).
    pub reason: String,
    /// Why, in the user's words ("the tab at 12.0 mm: its slot would open the slot plate's edge").
    pub text: String,
}

/// A stretch's skip in the user's words (design 11.3).
fn stretch_skip_text(stretch: (f64, f64), skip: &kernel_ts::StretchSkip) -> String {
    use kernel_ts::StretchSkip as S;
    let at = format!("the stretch from {:.1} to {:.1} mm", stretch.0, stretch.1);
    match skip {
        S::TooShort { length, need, most: 0 } => format!("{at} is too short for a tab ({length:.1} mm, needs {need:.1} mm)"),
        S::TooShort { length, need, most } => format!(
            "{at} is too short for the tabs asked ({length:.1} mm, needs {need:.1} mm, {most} tab{} would fit)",
            if *most == 1 { "" } else { "s" }
        ),
        S::SpacingTooSmall { space, least } => {
            format!("{at}: the spacing leaves {space:.2} mm between tabs, less than {least:.2} mm")
        }
    }
}

/// A slot's skip in the user's words (design 11.3, 7.4).
fn slot_skip_text(skip: &kernel_ts::SlotSkip) -> String {
    use kernel_ts::SlotSkip as S;
    match skip {
        S::AtEdge => "its slot would open the slot plate's edge".into(),
        S::OverHole { web } => format!("its slot comes {web:.2} mm from a hole of the slot plate"),
        S::UnevenFarSide => "the slot plate steps under its slot".into(),
        S::NoFarSide => "no flat far face of the slot plate lies under its slot".into(),
    }
}

/// A tab's skip in the user's words (design 11.3, 7.1).
fn tab_skip_text(skip: &kernel_ts::TabSkip) -> String {
    match skip {
        kernel_ts::TabSkip::TooShort { length } => format!("it would be {length:.2} mm long, too short to make"),
    }
}

/// The tabs a cut joint made and what it left out (stretch by stretch, then tab by tab), from
/// its plan.
fn left_out_of(plan: &[kernel_ts::StretchPlan]) -> (usize, Vec<TabSlotLeftOut>) {
    let mut made = 0;
    let mut out = Vec::new();
    let mut push = |reason: &str, text: String| out.push(TabSlotLeftOut { reason: reason.into(), text });
    for st in plan {
        match &st.tabs {
            Err(skip) => push(skip.reason_id(), stretch_skip_text(st.stretch, skip)),
            Ok(tabs) => {
                for t in tabs {
                    let at = format!("the tab at {:.1} mm", t.start);
                    match (t.skip, &t.tab) {
                        (Some(skip), _) => push(skip.reason_id(), format!("{at}: {}", slot_skip_text(&skip))),
                        (None, Err(skip)) => push(skip.reason_id(), format!("{at}: {}", tab_skip_text(skip))),
                        (None, Ok(_)) => made += 1,
                    }
                }
            }
        }
    }
    (made, out)
}

/// Up to [`SKIPS_NAMED`] of `items` joined by `sep`, then "and N more".
fn named_items(items: &[String], sep: &str) -> String {
    let mut s = items.iter().take(SKIPS_NAMED).cloned().collect::<Vec<_>>().join(sep);
    if items.len() > SKIPS_NAMED {
        s = format!("{s} and {} more", items.len() - SKIPS_NAMED);
    }
    s
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

/// The route a plate pair takes (design 4.4): the plate route here, or the sheet metal route when
/// a plate is sheet metal in the state before the feature. One Tab and Slot, two routes, chosen by
/// what is picked (#17424).
enum Route {
    Plate,
    /// The sheet metal route, for the named sheet metal plate. Not built yet.
    Sheet(String),
}

/// The route of the pair of plates `names`.
fn route_of(st: &ModelState, names: [&str; 2]) -> Route {
    match names.into_iter().find(|n| st.sheets.iter().any(|s| s.body == *n)) {
        Some(sheet) => Route::Sheet(sheet.to_string()),
        None => Route::Plate,
    }
}

/// The sheet metal route (not built yet): a pair it takes is skipped as
/// `sheet_route_not_built`, with the sheet metal plate named. When the route is built, it takes
/// these pairs (and the feature's sheet picks) here, and the plate route is unchanged.
fn sheet_route(sheet: &str) -> (&'static str, String) {
    ("sheet_route_not_built", format!("needs the sheet metal route, not built yet: {sheet} is sheet metal"))
}

/// Why the plate `name` cannot take a tab or a slot in the plate route (design v3 4.4 / 11.3), as
/// (reason id, text): a mesh body (`not_solid`).
fn plate_unusable(body: Option<&Body>, name: &str) -> Option<(&'static str, String)> {
    body.is_some_and(Body::is_mesh).then(|| ("not_solid", format!("cannot be cut: {name} is a mesh body")))
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
/// `no_contact` skips are left out (the reply names them). Then the cut joints that left
/// stretches or tabs out: "left out in 1 plate joint: Wall → Bottom the tab at 12.0 mm: its slot
/// would open the slot plate's edge".
fn skipped_note(joints: &[TabSlotJoint]) -> Option<String> {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let skipped: Vec<String> = joints
        .iter()
        .filter(|j| j.reason.as_deref().is_some_and(|r| r != "no_contact"))
        .map(|j| format!("{} → {} {}", j.tab, j.slot, j.skipped.as_deref().unwrap_or("")))
        .collect();
    let partial: Vec<String> = joints
        .iter()
        .filter(|j| j.reason.is_none() && !j.left_out.is_empty())
        .map(|j| {
            let texts: Vec<String> = j.left_out.iter().map(|l| l.text.clone()).collect();
            format!("{} → {} {}", j.tab, j.slot, named_items(&texts, "; "))
        })
        .collect();
    let mut parts = Vec::new();
    if !skipped.is_empty() {
        parts.push(format!("skipped {} plate joint{}: {}", skipped.len(), plural(skipped.len()), named_items(&skipped, ", ")));
    }
    if !partial.is_empty() {
        parts.push(format!("left out in {} plate joint{}: {}", partial.len(), plural(partial.len()), named_items(&partial, ", ")));
    }
    (!parts.is_empty()).then(|| format!("tab and slot: {}", parts.join("; ")))
}

/// The feature's failure when every pair is skipped (design v2 11.2, `no_plate_joint`): "tab and
/// slot: no plate joint to make (Wall → Bottom needs the sheet metal route, not built
/// yet: Bottom is sheet metal; Far → Bottom
/// does not meet it (0.50 mm apart)) (no_plate_joint)".
fn no_plate_joint(skipped: &[TabSlotJoint]) -> DocError {
    let items: Vec<String> = skipped.iter().map(|j| format!("{} → {} {}", j.tab, j.slot, j.skipped.as_deref().unwrap_or(""))).collect();
    if items.is_empty() {
        return DocError::Invalid("tab and slot: no tab plate stands on a slot plate (no_plate_joint)".into());
    }
    DocError::Invalid(format!("tab and slot: no plate joint to make ({}) (no_plate_joint)", named_items(&items, "; ")))
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
/// takes the previous joint's result), the plates replaced in place. A pair that does not meet is
/// named in the report only; a joint skipped, or a pair the cut refuses, in `warning` and the report.
pub(crate) fn apply(vals: &BTreeMap<String, Value>, k: &FeatureKind, st: &mut ModelState, warning: &mut Option<String>) -> Result<TabSlotReport> {
    let FeatureKind::TabSlot { tab_plates, slot_plates, sheet_picks, .. } = k else {
        return Err(DocError::Invalid("not a tab and slot".into()));
    };
    if let Some(bad) = bad_plate_key(tab_plates, slot_plates) {
        return Err(DocError::Invalid(format!("tab and slot: {bad}")));
    }
    // The plates, each once, as the state has them. A plate whose body is gone skips its pairs
    // (`plate_missing`, design v2 3.2 / 11.3).
    let mut names: Vec<String> = Vec::new();
    for p in tab_plates.iter().chain(slot_plates) {
        let name = p.body.to_string();
        if st.body(&name).is_some() && !names.contains(&name) {
            names.push(name);
        }
    }
    let index = |n: &str| names.iter().position(|m| m == n).unwrap_or(0);
    let mut plates: Vec<Body> = names.iter().filter_map(|n| st.body(n)).map(|b| b.body.clone()).collect();
    // The pairs with a joint, in pick order, with their settings.
    let mut list: Vec<(usize, usize, Plane)> = Vec::new();
    let mut specs: Vec<TabSlotSpec> = Vec::new();
    // Pairs the plate route does not take (design v3 4.4 / 11.3): a sheet metal plate (the sheet
    // metal route's, not built yet) or a mesh body in the state before the feature; and pairs with no joint (no contact, ...). Every such pair is
    // skipped and named.
    let mut unusable: Vec<TabSlotJoint> = Vec::new();
    for t in tab_plates {
        for s in slot_plates {
            let (tn, sn) = (t.body.to_string(), s.body.to_string());
            if tn == sn {
                continue;
            }
            if let Some(gone) = [&tn, &sn].into_iter().find(|n| !names.contains(*n)) {
                unusable.push(TabSlotJoint {
                    tab: tn.clone(),
                    slot: sn.clone(),
                    skipped: Some(format!("cannot be cut: {gone} is gone")),
                    reason: Some("plate_missing".into()),
                    tabs: 0,
                    left_out: Vec::new(),
                });
                continue;
            }
            let (ti, si) = (index(&tn), index(&sn));
            let routed = match route_of(st, [&names[ti], &names[si]]) {
                Route::Sheet(sheet) => Some(sheet_route(&sheet)),
                Route::Plate => [ti, si].into_iter().find_map(|i| plate_unusable(plates.get(i), &names[i])),
            };
            if let Some((reason, text)) = routed {
                unusable.push(TabSlotJoint {
                    tab: names[ti].clone(),
                    slot: names[si].clone(),
                    skipped: Some(text),
                    reason: Some(reason.into()),
                    tabs: 0,
                    left_out: Vec::new(),
                });
                continue;
            }
            let near = s.near.or(t.near).map(Into::into);
            let Some((tab, slot)) = plates.get(ti).zip(plates.get(si)) else { continue };
            let (p, joint) = match kernel_ts::joint_plane(tab, slot, near, JOINT_TOL)? {
                Ok(found) => found,
                Err(skip) => {
                    // A pair with no joint (no contact, ...) is skipped and named in the report,
                    // never silent; the warning leaves `no_contact` out (design 11.6).
                    unusable.push(TabSlotJoint {
                        tab: tn,
                        slot: sn,
                        skipped: Some(pair_skip_text(&skip)),
                        reason: Some(skip.reason_id().into()),
                        tabs: 0,
                        left_out: Vec::new(),
                    });
                    continue;
                }
            };
            specs.push(spec_of(vals, k, joint.faces.thickness, extent_along(slot, &p)?)?);
            list.push((ti, si, p));
        }
    }
    // The feature's sheet picks belong to the sheet metal route, not built yet: named, never silent.
    if !sheet_picks.is_empty() {
        let n = sheet_picks.len();
        let note = format!(
            "tab and slot: {n} sheet metal joint{} skipped: the sheet metal route is not built yet (sheet_route_not_built)",
            if n == 1 { "" } else { "s" }
        );
        if specs.is_empty() && unusable.is_empty() {
            return Err(DocError::Invalid(note));
        }
        append_warning(warning, note);
    }
    if specs.is_empty() {
        return Err(no_plate_joint(&unusable));
    }
    let refs: Vec<&Body> = plates.iter().collect();
    let (skips, tool_faces) = kernel_ts::joints_each(&refs, &list, &specs, JOINT_TOL)?;
    let pair_names: Vec<String> = list.iter().map(|(ti, si, _)| format!("{} → {}", names[*ti], names[*si])).collect();
    check_tool_faces(&tool_faces, &names, kernel_ts::MAX_TOOL_FACES, kernel_ts::SLOW_TOOL_FACES, warning)?;
    let mut report = TabSlotReport { joints: unusable, tool_faces: names.iter().cloned().zip(tool_faces).collect() };
    for (j, ((ti, si, p), spec)) in list.iter().zip(&specs).enumerate() {
        let (tab_name, slot_name) = (names[*ti].clone(), names[*si].clone());
        let (mut tabs, mut left_out) = (0, Vec::new());
        let skipped = if let Some(Some(skip)) = skips.get(j) {
            Some((skip.reason_id().to_string(), joint_skip_text(skip, &pair_names)))
        } else if let Some((tab, slot)) = plates.get(*ti).zip(plates.get(*si)) {
            match kernel_ts::cut(tab, slot, *p, *spec, JOINT_TOL)? {
                Ok(c) => {
                    (tabs, left_out) = left_out_of(&c.plan);
                    plates[*ti] = c.tab_plate;
                    plates[*si] = c.slot_plate;
                    // A cut joint that made no tab is skipped: never silent (design 11.6).
                    match left_out.first() {
                        Some(first) if tabs == 0 => {
                            let texts: Vec<String> = left_out.iter().map(|l| l.text.clone()).collect();
                            Some((first.reason.clone(), format!("made no tab: {}", named_items(&texts, "; "))))
                        }
                        _ => None,
                    }
                }
                Err(skip) => Some((skip.reason_id().to_string(), pair_skip_text(&skip))),
            }
        } else {
            continue;
        };
        let (reason, skipped) = skipped.map_or((None, None), |(id, text)| (Some(id), Some(text)));
        report.joints.push(TabSlotJoint { tab: tab_name, slot: slot_name, skipped, reason, tabs, left_out });
    }
    if report.joints.iter().all(|j| j.reason.is_some()) {
        return Err(no_plate_joint(&report.joints));
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
        TabSlotJoint {
            tab: tab.into(),
            slot: "Bottom".into(),
            skipped: reason.map(|_| text.to_string()),
            reason: reason.map(Into::into),
            tabs: 0,
            left_out: Vec::new(),
        }
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

    /// Design 11.3 and 11.6: stretches and tabs a cut joint leaves out are named in the warning
    /// in the user's words, after the skipped joints; a joint that made some tabs is not counted
    /// as skipped.
    #[test]
    fn tab_slot_left_out_tabs_read_as_user_text() {
        let left = |reason: &str, text: &str| TabSlotLeftOut { reason: reason.into(), text: text.into() };
        let mut wall = joint("Wall", None, "");
        wall.tabs = 2;
        wall.left_out = vec![left("at_edge", "the tab at 12.0 mm: its slot would open the slot plate's edge")];
        let note = skipped_note(&[wall.clone()]).unwrap();
        assert_eq!(note, "tab and slot: left out in 1 plate joint: Wall → Bottom the tab at 12.0 mm: its slot would open the slot plate's edge");
        let note = skipped_note(&[joint("Left", Some("swapped"), "is the other way round"), wall]).unwrap();
        assert!(
            note.starts_with("tab and slot: skipped 1 plate joint: Left → Bottom is the other way round; left out in 1 plate joint: Wall"),
            "{note}"
        );
        let short = kernel_ts::StretchSkip::TooShort { length: 120.0, need: 120.4, most: 1 };
        let text = stretch_skip_text((20.0, 140.0), &short);
        assert!(text.contains("from 20.0 to 140.0 mm") && text.contains("too short") && text.contains("1 tab would fit"), "{text}");
        assert!(slot_skip_text(&kernel_ts::SlotSkip::OverHole { web: 0.25 }).contains("0.25 mm from a hole"));
        assert!(tab_skip_text(&kernel_ts::TabSkip::TooShort { length: -1.0 }).contains("too short"));
    }

    /// Design 11.6: a wall whose every tab is dropped is never silent. Thick (10 thick, 120 long)
    /// on Bottom with two tabs of the default width 30 and the default end distance 30: the
    /// stretch is too short, the joint made no tab, and the feature's failure says so.
    #[test]
    fn tab_slot_joint_with_every_tab_dropped_is_named() {
        use solidcore_record::Vec3;
        use solidcraft_kernel as kernel;
        let mut st = ModelState::default();
        let bottom = kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(160.0, 100.0, 6.0)).unwrap();
        let thick = kernel::box_solid(Vec3::new(20.0, 60.0, 6.0), Vec3::new(140.0, 70.0, 66.0)).unwrap();
        st.bodies.push(super::super::ModelBody::new("Bottom".into(), bottom, 0));
        st.bodies.push(super::super::ModelBody::new("Thick".into(), thick, 0));
        let k: FeatureKind = serde_json::from_value(serde_json::json!({
            "type": "tab_slot",
            "tab_plates": [{"key": 1, "body": "Thick"}],
            "slot_plates": [{"key": 2, "body": "Bottom"}],
            "clearance": "0.1",
            "count": "2",
        }))
        .unwrap();
        let mut warning = None;
        // Its only joint skipped, the feature fails `no_plate_joint` naming it (design v2 11.2).
        let err = apply(&BTreeMap::new(), &k, &mut st, &mut warning).unwrap_err().to_string();
        assert!(err.contains("Thick → Bottom made no tab: the stretch from") && err.contains("too short"), "{err}");
        assert!(err.ends_with("(no_plate_joint)"), "{err}");
        let volume = |n: &str| kernel::measure_mass(&st.body(n).unwrap().body).unwrap().volume;
        assert!((volume("Thick") - 72_000.0).abs() < 1e-6 && (volume("Bottom") - 96_000.0).abs() < 1e-6);
    }

    #[test]
    fn tab_slot_skips_every_pair_with_a_sheet_metal_plate() {
        use crate::sheet::{AngleDisplay, ReliefShape, RuleValues, base_flange};
        use solidcore_record::{Loop2, Region2, Vec2, Vec3};
        use solidcraft_kernel as kernel;
        let mut st = ModelState::default();
        let bottom = kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(160.0, 100.0, 6.0)).unwrap();
        let wall = kernel::box_solid(Vec3::new(20.0, 60.0, 6.0), Vec3::new(140.0, 66.0, 66.0)).unwrap();
        st.bodies.push(super::super::ModelBody::new("Bottom".into(), bottom, 0));
        st.bodies.push(super::super::ModelBody::new("Wall".into(), wall, 0));
        let rv = RuleValues {
            thickness: 6.0,
            k_factor: 0.44,
            bend_radius: 6.0,
            relief_width: 6.0,
            relief_depth: 3.0,
            corner_relief: 10.0,
            corner_relief_shape: ReliefShape::Notch,
            hem_gap: 0.01,
            gap: 6.0,
            angle_display: AngleDisplay::Bend,
        };
        let outline = [Vec2::new(0.0, 0.0), Vec2::new(160.0, 0.0), Vec2::new(160.0, 100.0), Vec2::new(0.0, 100.0)];
        let region = Region2 { outer: Loop2::polygon(&outline), holes: Vec::new() };
        let frame = solidcore_record::Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
        st.sheets.push(base_flange("Bottom", "Steel (mm)", &rv, frame, region));
        let mut warning = None;
        let err = apply(&BTreeMap::new(), &kind(serde_json::json!({"clearance": "0.1"})), &mut st, &mut warning).unwrap_err().to_string();
        assert_eq!(
            err,
            "tab and slot: no plate joint to make (Wall → Bottom needs the sheet metal route, not built yet: Bottom is sheet metal) (no_plate_joint)"
        );
        let wall = report_reason(&st);
        assert_eq!(wall.as_deref(), Some("sheet_route_not_built"));
        let volume = |n: &str| kernel::measure_mass(&st.body(n).unwrap().body).unwrap().volume;
        assert!((volume("Wall") - 43_200.0).abs() < 1e-6 && (volume("Bottom") - 96_000.0).abs() < 1e-6);
    }

    /// The reason id of the only pair of fixture `st`, as the plate loop names it.
    fn report_reason(st: &ModelState) -> Option<String> {
        let names = ["Wall", "Bottom"];
        match route_of(st, names) {
            Route::Sheet(sheet) => Some(sheet_route(&sheet).0.to_string()),
            Route::Plate => None,
        }
    }

    #[test]
    fn tab_slot_sheet_picks_wait_for_the_sheet_metal_route() {
        let v = serde_json::json!({"type": "tab_slot", "tab_plates": [], "slot_plates": [], "sheet_picks": [{"kind": "joint", "key": 1, "name": "Joint 1"}], "clearance": "0.1"});
        let k: FeatureKind = serde_json::from_value(v.clone()).unwrap();
        let mut back = serde_json::to_value(&k).unwrap();
        back["next_plate_key"] = 0.into();
        let mut want = v;
        want["next_plate_key"] = 0.into();
        assert_eq!(back, want);
        let (mut st, mut warning) = (ModelState::default(), None);
        let err = apply(&BTreeMap::new(), &k, &mut st, &mut warning).unwrap_err().to_string();
        assert_eq!(err, "tab and slot: 1 sheet metal joint skipped: the sheet metal route is not built yet (sheet_route_not_built)");
    }

    /// Fixture F's Bottom and Wall, and `more` (name, min, max) boxes, in the state.
    fn fixture_f_with(more: &[(&str, [f64; 3], [f64; 3])]) -> ModelState {
        use solidcore_record::Vec3;
        use solidcraft_kernel as kernel;
        let mut st = ModelState::default();
        let mut add = |n: &str, a: [f64; 3], b: [f64; 3]| {
            let body = kernel::box_solid(Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2])).unwrap();
            st.bodies.push(super::super::ModelBody::new(n.into(), body, 0));
        };
        add("Bottom", [0.0, 0.0, 0.0], [160.0, 100.0, 6.0]);
        add("Wall", [20.0, 30.0, 6.0], [140.0, 36.0, 66.0]);
        for (n, a, b) in more {
            add(n, *a, *b);
        }
        st
    }

    fn tab_slot_of(tabs: &[&str], slots: &[&str]) -> FeatureKind {
        let picks = |names: &[&str], from: usize| -> Vec<serde_json::Value> {
            names.iter().enumerate().map(|(i, n)| serde_json::json!({"key": from + i, "body": n})).collect()
        };
        serde_json::from_value(serde_json::json!({
            "type": "tab_slot",
            "tab_plates": picks(tabs, 1),
            "slot_plates": picks(slots, 1 + tabs.len()),
            "clearance": "0.1",
            "count": "3",
            "tab_width": "20",
            "end_distance": "10",
        }))
        .unwrap()
    }

    /// Design v2 3.2 / 11.3: a plate whose body is gone skips its pairs `plate_missing`; the
    /// other pairs are cut as before (Wall 45 360, Bottom 93 745.68 mm³ as fixture F).
    #[test]
    fn tab_slot_skips_a_pair_whose_plate_is_gone() {
        use solidcraft_kernel as kernel;
        let mut st = fixture_f_with(&[]);
        let mut warning = None;
        let report = apply(&BTreeMap::new(), &tab_slot_of(&["Wall", "Ghost"], &["Bottom"]), &mut st, &mut warning).unwrap();
        assert_eq!(report.joints.len(), 2, "{report:?}");
        let gone = report.joints.iter().find(|j| j.tab == "Ghost").unwrap();
        assert_eq!((gone.tabs, gone.reason.as_deref(), gone.skipped.as_deref()), (0, Some("plate_missing"), Some("cannot be cut: Ghost is gone")));
        let wall = report.joints.iter().find(|j| j.tab == "Wall").unwrap();
        assert_eq!((wall.tabs, wall.reason.as_deref()), (3, None), "{report:?}");
        let warning = warning.unwrap_or_default();
        assert!(warning.contains("skipped 1 plate joint: Ghost → Bottom cannot be cut: Ghost is gone"), "{warning}");
        let volume = |n: &str| kernel::measure_mass(&st.body(n).unwrap().body).unwrap().volume;
        assert!((volume("Wall") - 45_360.0).abs() < 1e-6, "{}", volume("Wall"));
        assert!((volume("Bottom") - 93_745.68).abs() < 1e-6, "{}", volume("Bottom"));
    }

    /// Design v2 11.2: when every pair is skipped, a gone plate or plates that do not meet, the
    /// feature fails `no_plate_joint` naming each pair, and the plates are left as they were.
    #[test]
    fn tab_slot_with_every_pair_skipped_fails_no_plate_joint() {
        use solidcraft_kernel as kernel;
        let mut st = fixture_f_with(&[("Far", [20.0, 60.0, 10.0], [140.0, 66.0, 70.0])]);
        let mut warning = None;
        let err = apply(&BTreeMap::new(), &tab_slot_of(&["Ghost"], &["Bottom"]), &mut st, &mut warning).unwrap_err().to_string();
        assert_eq!(err, "tab and slot: no plate joint to make (Ghost → Bottom cannot be cut: Ghost is gone) (no_plate_joint)");
        let err = apply(&BTreeMap::new(), &tab_slot_of(&["Far", "Ghost"], &["Bottom"]), &mut st, &mut warning).unwrap_err().to_string();
        assert!(err.contains("Ghost → Bottom cannot be cut: Ghost is gone") && err.contains("Far → Bottom "), "{err}");
        assert!(err.ends_with("(no_plate_joint)"), "{err}");
        let volume = |n: &str| kernel::measure_mass(&st.body(n).unwrap().body).unwrap().volume;
        assert!((volume("Wall") - 43_200.0).abs() < 1e-6 && (volume("Bottom") - 96_000.0).abs() < 1e-6);
    }

    /// Design v3 4.4 item 3: a hand-edited file with a length past 1e5 mm is refused by the eval
    /// too, as by the command; the cap itself is taken.
    #[test]
    fn tab_slot_spec_of_refuses_lengths_past_the_cap() {
        let s = spec(serde_json::json!({"clearance": "0.1", "tab_width": "100000", "protrusion": "-100000"})).unwrap();
        assert_eq!((s.width, s.protrusion), (100_000.0, -100_000.0));
        for (bad, what) in [
            (serde_json::json!({"clearance": "0.1", "tab_width": "100001"}), "tab width"),
            (serde_json::json!({"clearance": "0.1", "spacing": "1000000"}), "spacing"),
            (serde_json::json!({"clearance": "0.1", "end_distance": "200000"}), "end distance"),
            (serde_json::json!({"clearance": "100001"}), "clearance"),
            (serde_json::json!({"clearance": "0.1", "protrusion": "100001"}), "protrusion"),
            (serde_json::json!({"clearance": "0.1", "protrusion": "-100001"}), "protrusion"),
        ] {
            let e = spec(bad.clone()).unwrap_err().to_string();
            assert!(e.contains(what) && e.contains("100000"), "{bad}: {e}");
        }
    }
}
