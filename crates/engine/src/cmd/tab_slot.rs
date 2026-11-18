//! `solid.tab_slot`: tabs on the tab plates' edges where they meet the slot plates, and matching
//! slots cut through the slot plates (#17422). The feature is `FeatureKind::TabSlot`, evaluated in
//! `crates/doc/src/eval_tab_slot.rs`; every refusal made before evaluation (design 11.1) is in
//! [`check`], which every writer of the feature runs.

use serde_json::{Map, Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{Expr, FeatureKind, ModelState};

use super::CommandSpec;
use super::features::{add_feature, check_expr};
use crate::params::{bad, expr, expr_problem, vec3};
use crate::schema::tab_slot::{TabSlotParams, TabSlotResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const TAB_SLOT_CONTRACT: TypedContract<TabSlotParams> = TypedContract::new();

/// The most plates in one list.
const MAX_PLATES: usize = 64;
/// The largest length the command takes (mm), as Dogbone's.
const MAX_LENGTH: f64 = 100_000.0;
/// The most tabs per stretch.
const MAX_COUNT: f64 = 100.0;
/// The clearance written when none is given.
const DEFAULT_CLEARANCE: &str = "0.1 mm";

const HELP: &str = "tab_plates: [plate] (the plates that get tabs; a plate is a body name or {body, near: [x,y,z], key?}, near choosing between several contact planes, key a whole number from 1 that names the plate's tabs or slots and is kept on an edit; plates without one get the next free key); slot_plates: [plate] (the plates that get slots); count?: expr (tabs per stretch, a whole number 1 to 100; default: as many as fit); spacing?: expr (centre to centre between tabs, instead of count); tab_width?: expr (default 3 x the tab plate's thickness); end_distance?: expr (from a stretch's end to the nearest tab; default the tab width); clearance?: expr (the room on every side of a tab in its slot; default 0.1 mm); protrusion?: expr (how far a tab stands out past the slot plate's far face; default 0); next_plate_key?: integer (the lowest key a new plate may get; stored by the feature so an edit never reuses a removed plate's key); name?. Every tab plate is tried against every slot plate; a pair that cannot be cut is skipped and named. A pair with a sheet metal plate waits for the sheet metal route, not built yet (sheet_route_not_built). Not for mesh bodies";

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.tab_slot", "Tab and Slot", crate::checked!("solid.tab_slot", tab_slot))
    .icon("tab_slot")
    .contract(&TAB_SLOT_CONTRACT)
    .output(output_schema_for::<TabSlotResult>)
    .params(HELP)];

/// The expression keys of the feature, in the order the command reads them.
const EXPR_KEYS: [&str; 6] = ["count", "spacing", "tab_width", "end_distance", "clearance", "protrusion"];

/// One plate as the stored pick (`{key?, body, near?}`). A `key` is the stored key Edit Feature
/// sends back, so the plate keeps its names; a plate without one gets a new key later.
fn plate(cmd: &str, list: &str, v: &Value) -> Result<Value> {
    let want = || bad(cmd, format!("`{list}`: a plate is a body name or {{body, near}}"));
    let mut key = None;
    let (body, near) = match v {
        Value::String(b) => (b.clone(), None),
        Value::Object(o) => {
            if let Some(k) = o.keys().find(|k| !["key", "body", "near"].contains(&k.as_str())) {
                return Err(bad(cmd, format!("`{list}`: a plate takes `body` and `near`, not `{k}`")));
            }
            key = match o.get("key").filter(|k| !k.is_null()) {
                None => None,
                Some(k) => Some(
                    k.as_u64()
                        .and_then(|k| u32::try_from(k).ok())
                        .filter(|k| *k > 0)
                        .ok_or_else(|| bad(cmd, format!("`{list}`: a plate's `key` must be a whole number from 1")))?,
                ),
            };
            let body = o.get("body").and_then(Value::as_str).ok_or_else(want)?.to_string();
            let near = match o.get("near").filter(|n| !n.is_null()) {
                None => None,
                Some(n) => Some(vec3(n).ok_or_else(|| bad(cmd, format!("`{list}`: `near` must be a point [x, y, z] of numbers")))?),
            };
            (body, near)
        }
        _ => return Err(want()),
    };
    if body.trim().is_empty() {
        return Err(want());
    }
    let mut out = json!({"body": body});
    if let Some(k) = key {
        out["key"] = json!(k);
    }
    if let Some(n) = near {
        out["near"] = json!([n.x, n.y, n.z]);
    }
    Ok(out)
}

/// The plates of `list`, with the keys they were given (none for a new plate).
fn plates(cmd: &str, p: &Value, list: &str) -> Result<Vec<Value>> {
    let a = match p.get(list) {
        None | Some(Value::Null) => return Err(bad(cmd, "Pick the plates that get tabs and the plates that get slots")),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(bad(cmd, format!("`{list}` must be a list of plates"))),
    };
    if a.len() > MAX_PLATES {
        return Err(bad(cmd, format!("at most {MAX_PLATES} {}", list.replace('_', " "))));
    }
    a.iter().map(|v| plate(cmd, list, v)).collect()
}

/// Gives every plate without a key one: from past the largest key given and `next_plate_key`
/// (which Edit Feature sends back, so a key is never handed out twice). Returns the next key.
fn key_plates(cmd: &str, p: &Value, lists: [&mut Vec<Value>; 2]) -> Result<u32> {
    let [tabs, slots] = lists;
    let given: Vec<u64> = tabs.iter().chain(slots.iter()).filter_map(|v| v.get("key").and_then(Value::as_u64)).collect();
    if let Some(k) = given.iter().enumerate().find_map(|(i, k)| given[..i].contains(k).then_some(*k)) {
        return Err(bad(cmd, format!("two plates have the key {k}")));
    }
    let floor = p.get("next_plate_key").and_then(Value::as_u64).and_then(|k| u32::try_from(k).ok()).unwrap_or(1);
    let largest = given.iter().max().map_or(0, |k| *k as u32);
    let mut next = largest.saturating_add(1).max(floor).max(1);
    for v in tabs.iter_mut().chain(slots.iter_mut()).filter(|v| v.get("key").is_none()) {
        v["key"] = json!(next);
        next = next.checked_add(1).ok_or_else(|| bad(cmd, "the plate key space of this tab and slot is exhausted"))?;
    }
    Ok(next)
}

/// The feature the parameters `p` ask for. Lengths are stored as given (a bare number is the
/// design's unit); the clearance is written out when none is given.
fn feature_kind(cmd: &str, p: &Value) -> Result<FeatureKind> {
    let mut tab_plates = plates(cmd, p, "tab_plates")?;
    let mut slot_plates = plates(cmd, p, "slot_plates")?;
    let next = key_plates(cmd, p, [&mut tab_plates, &mut slot_plates])?;
    let mut o = Map::new();
    o.insert("type".into(), "tab_slot".into());
    o.insert("tab_plates".into(), tab_plates.into());
    o.insert("slot_plates".into(), slot_plates.into());
    o.insert("next_plate_key".into(), next.into());
    for key in EXPR_KEYS {
        match expr(p, key) {
            Some(e) => {
                o.insert(key.into(), Value::String(e.to_string()));
            }
            None if p.get(key).is_some_and(|v| !v.is_null()) => {
                return Err(bad(cmd, format!("`{key}` must be a number or an expression ({})", expr_problem(p.get(key)))));
            }
            None => {}
        }
    }
    o.entry("clearance").or_insert_with(|| DEFAULT_CLEARANCE.into());
    serde_json::from_value(Value::Object(o)).map_err(|e| bad(cmd, format!("invalid tab and slot: {e}")))
}

/// A length's value now, in mm (a bare number means the design's unit).
fn length_now(s: &Session, e: &Expr, key: &str, cmd: &str) -> Result<f64> {
    check_expr(s, e, Kind::Length, cmd, key)?;
    let typed: Expr = s.doc.with_design_unit(e, Kind::Length).into();
    let v = s.doc.eval(&typed, Kind::Length).map_err(|err| bad(cmd, format!("{key}: {err}")))?;
    if !v.is_finite() {
        return Err(bad(cmd, format!("`{key}` must be a finite length")));
    }
    Ok(v)
}

/// A length in `[least, MAX_LENGTH]` (`open`: more than `least`).
fn length_in(s: &Session, e: Option<&Expr>, key: &str, cmd: &str, least: f64, open: bool) -> Result<()> {
    let Some(e) = e else { return Ok(()) };
    let v = length_now(s, e, key, cmd)?;
    let low = if open { v <= least } else { v < least };
    if low || v > MAX_LENGTH {
        let from = if open { format!("more than {least}") } else { format!("{least} or more") };
        return Err(bad(cmd, format!("`{key}` must be {from} and at most {MAX_LENGTH} mm, not {v} mm")));
    }
    Ok(())
}

/// Every refusal of design 11.1 for the feature `kind` against the model `st` it would be added
/// to (the command: the model now; `timeline.edit`: the state before the feature). Changes
/// nothing.
pub(crate) fn check(s: &Session, st: &ModelState, kind: &FeatureKind) -> Result<()> {
    let cmd = "solid.tab_slot";
    let FeatureKind::TabSlot { tab_plates, slot_plates, count, spacing, tab_width, end_distance, clearance, protrusion, .. } = kind else {
        return Ok(());
    };
    if tab_plates.is_empty() || slot_plates.is_empty() {
        return Err(bad(cmd, "Pick the plates that get tabs and the plates that get slots"));
    }
    for (list, what) in [(tab_plates, "tab plate"), (slot_plates, "slot plate")] {
        if list.len() > MAX_PLATES {
            return Err(bad(cmd, format!("at most {MAX_PLATES} {what}s")));
        }
        for (i, pick) in list.iter().enumerate() {
            let name = pick.body.to_string();
            if list[..i].iter().any(|q| q.body == pick.body) {
                return Err(bad(cmd, format!("{name} is picked twice as a {what}")));
            }
            let Some(body) = st.body(&name) else {
                return Err(bad(cmd, format!("no body named {name}")));
            };
            if body.body.is_mesh() {
                return Err(bad(cmd, format!("{name} is a mesh body: tab and slot needs a solid")));
            }
            if st.surface_sheets.iter().any(|sh| sh.body == name) {
                return Err(bad(
                    cmd,
                    format!("{name} is a sheet metal body: Tab and Slot works on solid plates (sheet metal tab and slot is #11398)"),
                ));
            }
        }
    }
    if let Some(both) = tab_plates.iter().find(|t| slot_plates.iter().any(|q| q.body == t.body)) {
        return Err(bad(cmd, format!("{} is both a tab plate and a slot plate: pick it in one box", both.body)));
    }
    if count.is_some() && spacing.is_some() {
        return Err(bad(cmd, "give `count` or `spacing`, not both"));
    }
    if let Some(c) = count {
        let (vals, _) = s.doc.param_values();
        let n = solidcraft_doc::expr::eval_count(c, &vals).map_err(|err| bad(cmd, format!("count: {err}")))?;
        if !(1.0..=MAX_COUNT).contains(&n) {
            return Err(bad(cmd, format!("`count` must be a whole number from 1 to {MAX_COUNT}, not {n}")));
        }
    }
    length_in(s, spacing.as_ref(), "spacing", cmd, 0.0, true)?;
    length_in(s, tab_width.as_ref(), "tab_width", cmd, 0.0, true)?;
    length_in(s, end_distance.as_ref(), "end_distance", cmd, 0.0, false)?;
    length_in(s, Some(clearance), "clearance", cmd, 0.0, false)?;
    length_in(s, protrusion.as_ref(), "protrusion", cmd, -MAX_LENGTH, false)?;
    Ok(())
}

fn tab_slot(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.tab_slot";
    let kind = feature_kind(cmd, p)?;
    let st = s.model.state();
    check(s, &st, &kind)?;
    drop(st);
    let mut reply = add_feature(s, p, kind)?;
    with_report(s, &mut reply);
    Ok(reply)
}

/// Adds the feature's report (its plate joints, skips, left-out stretches and tabs, and each
/// plate's tool faces) to its reply. A feature that failed has no report.
fn with_report(s: &Session, reply: &mut Value) {
    let Some(id) = reply.get("feature").and_then(Value::as_u64) else { return };
    let st = s.model.state();
    let Some(r) = st.tab_slot_report(id) else { return };
    let left_out = |j: &solidcraft_doc::TabSlotJoint| -> Value {
        j.left_out.iter().map(|l| json!({"reason": l.reason, "detail": l.text})).collect::<Vec<_>>().into()
    };
    let mut joints = Vec::new();
    let mut skipped = Vec::new();
    for j in &r.joints {
        match (&j.reason, &j.skipped) {
            (Some(reason), detail) => skipped.push(json!({
                "tab_plate": j.tab,
                "slot_plate": j.slot,
                "reason": reason,
                "detail": detail.clone().unwrap_or_default(),
                "left_out": left_out(j),
            })),
            (None, _) => joints.push(json!({"tab_plate": j.tab, "slot_plate": j.slot, "tabs": j.tabs, "left_out": left_out(j)})),
        }
    }
    let tabs: usize = r.joints.iter().map(|j| j.tabs).sum();
    let tool_faces: Vec<Value> = r.tool_faces.iter().map(|(body, faces)| json!({"body": body, "faces": faces})).collect();
    let Some(o) = reply.as_object_mut() else { return };
    for (k, v) in [
        ("plate_joints", Value::from(joints)),
        ("skipped", skipped.into()),
        ("tabs", tabs.into()),
        ("slots", tabs.into()),
        ("tool_faces", tool_faces.into()),
    ] {
        o.insert(k.into(), v);
    }
}

#[cfg(test)]
#[path = "tab_slot_tests.rs"]
mod tests;
