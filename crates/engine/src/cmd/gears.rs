//! Involute spur gears (#6796): a tooth outline from `gear_profile`, sketched and extruded like
//! any other feature, so the gear stays an ordinary component.

use std::sync::Arc;

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_doc::SpurGear;
use solidcraft_sketch::CurveKind;

use super::CommandSpec;
use super::gear_profile::{GearSpec, outline};
use crate::params::{bad, num, str_};
use crate::schema::gears::{GearEditParams, GearInsertParams, InsertedGear};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const EDIT_CONTRACT: TypedContract<GearEditParams> = TypedContract::new();
const INSERT_CONTRACT: TypedContract<GearInsertParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("gear.edit", "Edit Gear", crate::checked!("gear.edit", edit_atomic))
        .icon("gear")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<InsertedGear>)
        .params("component: id|name of an inserted gear; module?, teeth?, pressure_angle?, face_width?, profile_shift?, backlash?, flank_points?, clearance?, root_fillet?, tip_round?, backlash_direction?, internal?, rim_thickness?, bore_diameter?, bore_offset?, helix_angle?, double_helical? (the values given replace the saved ones) → rebuilds the teeth in place; component, occurrence, name, diameters"),
    CommandSpec::new("gear.insert", "Insert Gear", crate::checked!("gear.insert", insert_atomic))
        .at("SOLID", "INSERT")
        .icon("gear")
        .contract(&INSERT_CONTRACT)
        .output(output_schema_for::<InsertedGear>)
        .params("module (mm), teeth (6..1000), pressure_angle? (deg, default 20), face_width? (mm), profile_shift?, backlash? (mm), flank_points? (2..64, default 8: the profile accuracy), clearance? (0..0.6 of the module, default 0.25: how far the roots lie below the dedendum line), root_fillet? (mm, 0..module: rounds where a flank meets the root), tip_round? (mm, 0..module: rounds where a flank meets the tip), backlash_direction? (thinner, the default: the backlash comes off the teeth; thicker: it is added, for a mate that carries the clearance), internal? (bool: a ring with its teeth inward; no profile shift), rim_thickness? (mm, internal only, default 2 × module), bore_diameter? (mm, external only: an axle hole), bore_offset? (mm: the hole's centre along +X), helix_angle? (deg, -45..45, default 0: teeth twisted over the face width, positive right-hand), double_helical? (bool, external only: two mirrored halves), name? → component, occurrence, name, pitch/base/tip/root diameters"),
];

/// A number parameter that may be left out; one that is present but not a usable number is refused.
fn optional(p: &Value, k: &str, cmd: &str) -> Result<Option<f64>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => num(p, k).map(Some).ok_or_else(|| bad(cmd, format!("`{k}` must be a finite number below 1e9"))),
    }
}

/// What a gear needs besides its tooth outline.
#[derive(Clone, Copy)]
struct Extent {
    width: f64,
    /// Internal gears: ring thickness outside the tooth roots.
    rim: f64,
    /// External gears: diameter of the axle hole, 0 for none.
    bore: f64,
    /// External gears: how far the axle hole's centre is from the gear's along +X.
    bore_offset: f64,
    /// Helix angle on the pitch circle (degrees): 0 for straight teeth, positive for a right-hand helix.
    helix: f64,
    /// Two mirrored helical halves, so the thrust of one cancels the other.
    double: bool,
}

/// How far a helical gear's profile turns over `length` along its axis (degrees).
fn twist_over(g: &GearSpec, helix_deg: f64, length: f64) -> f64 {
    (length * helix_deg.to_radians().tan() / g.pitch_radius()).to_degrees()
}

fn spec(p: &Value, cmd: &str) -> Result<(GearSpec, Extent)> {
    let module = num(p, "module").ok_or_else(|| bad(cmd, "`module` is required (a positive number of mm)"))?;
    let teeth = p
        .get("teeth")
        .and_then(Value::as_u64)
        .and_then(|t| u32::try_from(t).ok())
        .ok_or_else(|| bad(cmd, "`teeth` is required (a whole number, 6 to 1000)"))?;
    let mut g = GearSpec::new(module, teeth, optional(p, "pressure_angle", cmd)?.unwrap_or(20.0));
    g.shift = optional(p, "profile_shift", cmd)?.unwrap_or(0.0);
    g.backlash = optional(p, "backlash", cmd)?.unwrap_or(0.0);
    g.clearance = optional(p, "clearance", cmd)?.unwrap_or(0.25);
    g.root_fillet = optional(p, "root_fillet", cmd)?.unwrap_or(0.0);
    g.tip_round = optional(p, "tip_round", cmd)?.unwrap_or(0.0);
    g.thicker = match p.get("backlash_direction") {
        None | Some(Value::Null) => false,
        Some(Value::String(d)) if d == "thinner" => false,
        Some(Value::String(d)) if d == "thicker" => true,
        Some(_) => return Err(bad(cmd, "`backlash_direction` must be `thinner` or `thicker`")),
    };
    match p.get("flank_points") {
        None | Some(Value::Null) => {}
        Some(v) => match v.as_u64() {
            Some(n) => g.flank_points = u32::try_from(n).unwrap_or(u32::MAX),
            None => return Err(bad(cmd, "`flank_points` must be a whole number")),
        },
    }
    g.internal = match p.get("internal") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`internal` must be true or false")),
    };
    let width = optional(p, "face_width", cmd)?.unwrap_or(module * 10.0);
    if !(width > 0.0) {
        return Err(bad(cmd, "`face_width` must be above 0"));
    }
    // 0 means none: it is what a gear that is not internal, or has no axle hole, saves.
    let rim = optional(p, "rim_thickness", cmd)?.filter(|r| *r != 0.0);
    let bore = optional(p, "bore_diameter", cmd)?;
    if g.internal && bore.is_some_and(|b| b != 0.0) {
        return Err(bad(cmd, "`bore_diameter` is for external gears"));
    }
    if !g.internal && rim.is_some() {
        return Err(bad(cmd, "`rim_thickness` is for internal gears"));
    }
    let rim = if g.internal { rim.unwrap_or(module * 2.0) } else { 0.0 };
    if g.internal && !(rim > 0.0) {
        return Err(bad(cmd, "`rim_thickness` must be above 0"));
    }
    let bore = bore.unwrap_or(0.0);
    let bore_offset = optional(p, "bore_offset", cmd)?.unwrap_or(0.0);
    // The hole must leave a rim of material inside the root circle.
    let room = 0.9 * g.root_radius().min(g.base_radius());
    if bore < 0.0 || (bore > 0.0 && bore / 2.0 + bore_offset.abs() >= room) {
        let limit = if g.base_radius() < g.root_radius() { "base" } else { "root" };
        return Err(bad(
            cmd,
            format!(
                "the axle hole, with its offset, must stay inside 90% of the gear's {limit} circle (a radius of {room:.3} mm from the gear's centre)"
            ),
        ));
    }
    if bore == 0.0 && bore_offset != 0.0 {
        return Err(bad(cmd, "`bore_offset` needs a `bore_diameter`"));
    }
    let helix = optional(p, "helix_angle", cmd)?.unwrap_or(0.0);
    if helix.abs() > 45.0 {
        return Err(bad(cmd, "`helix_angle` must be from -45 to 45 degrees"));
    }
    let double = match p.get("double_helical") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`double_helical` must be true or false")),
    };
    if double && helix == 0.0 {
        return Err(bad(cmd, "`double_helical` needs a `helix_angle`"));
    }
    if double && g.internal {
        return Err(bad(cmd, "`double_helical` is for external gears"));
    }
    // The sweep that twists the teeth turns at most four times.
    if twist_over(&g, helix, width).abs() > 1440.0 {
        return Err(bad(cmd, "the teeth would turn more than four times over the face width: use a smaller helix angle or a thinner gear"));
    }
    Ok((g, Extent { width, rim, bore, bore_offset, helix, double }))
}

fn insert_atomic(s: &mut Session, p: &Value) -> Result<Value> {
    let mut computed = s.scratch();
    let result = insert(&mut computed, p)?;
    computed.undo.clear();
    computed.active_component = s.active_component;
    computed.active_occurrence_path = s.active_occurrence_path.clone();
    computed.active_sketch = s.active_sketch.filter(|id| computed.doc.feature(*id).is_some());
    s.accept_computed(computed)?;
    Ok(result)
}

/// A sketch on the XY plane holding the tooth outline (`teeth`), a circle round the centre, or both.
fn sketch_profile(s: &mut Session, g: &GearSpec, teeth: bool, circle: Option<(f64, f64)>, cmd: &str) -> Result<u64> {
    let sketch = s.execute("sketch.create", &json!({"plane": "XY"}))?["sketch"].as_u64().unwrap_or(0);
    // Each flank is one spline through its points, each tip and root one line: 4 edges a tooth.
    // All go in with one sketch edit: a command per curve would copy the design for each.
    let points = if teeth { outline(g).map_err(|e| bad(cmd, e))? } else { Vec::new() };
    let flank = (points.len() / (g.teeth as usize * 2)).max(1);
    super::sketch::edit(s, &json!({}), cmd, false, |sk, _| {
        let mut ids = Vec::with_capacity(points.len());
        for q in &points {
            ids.push(sk.add_point(Vec2::new(q[0], q[1]), None).map_err(|e| bad(cmd, e.to_string()))?);
        }
        let chunks: Vec<&[usize]> = ids.chunks(flank).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let kind = CurveKind::Spline { pts: chunk.to_vec(), control: false, degree: 3, periodic: false, nurbs: None, handles: Vec::new() };
            sk.add_curve(kind, None).map_err(|e| bad(cmd, e.to_string()))?;
            // After a right flank comes the tip; after a left flank, the root.
            let (from, to) = (chunk[chunk.len() - 1], chunks[(i + 1) % chunks.len()][0]);
            sk.add_line_pts(from, to, None).map_err(|e| bad(cmd, e.to_string()))?;
        }
        if let Some((r, x)) = circle {
            sk.add_circle(Vec2::new(x, 0.0), r, None, None).map_err(|e| bad(cmd, e.to_string()))?;
        }
        Ok(())
    })?;
    s.execute("sketch.finish", &json!({}))?;
    Ok(sketch)
}

/// Turn a profile sketch into material: straight for a spur gear; for a helical one, a sweep along
/// the gear's axis that turns the profile. A double helical gear is one half and its mirror.
fn solidify(s: &mut Session, g: &GearSpec, e: &Extent, profile: u64, operation: &str) -> Result<()> {
    if e.helix == 0.0 {
        let distance = format!("{} mm", e.width);
        s.execute("solid.extrude", &json!({"sketch": profile, "distance": distance, "direction": "positive", "operation": operation}))?;
        return Ok(());
    }
    let length = if e.double { e.width / 2.0 } else { e.width };
    // The path runs up +Z from the middle of the profile.
    let path = s.execute("sketch.create", &json!({"plane": "YZ"}))?["sketch"].as_u64().unwrap_or(0);
    let line = s.execute("sketch.line", &json!({"points": [[0, 0], [0, length]]}))?;
    let curve = line["created"]["curves"][0].clone();
    s.execute("sketch.finish", &json!({}))?;
    let twist = format!("{} deg", twist_over(g, e.helix, length));
    let swept =
        s.execute("solid.sweep", &json!({"sketch": profile, "path_sketch": path, "path": [curve], "twist": twist, "operation": operation}))?;
    if e.double {
        let body = swept["bodies"][0].clone();
        let plane = json!({"origin": [0, 0, length], "normal": [0, 0, 1]});
        s.execute("solid.mirror", &json!({"bodies": [body], "plane": plane, "combine": true}))?;
    }
    Ok(())
}

/// The gear's teeth as sketch and extrusion in the active component. An external gear is its tooth
/// outline, with an axle hole cut through it. An internal gear is the ring's disc with the tooth
/// outline cut through it. Helical teeth are a twisted sweep in place of the extrusion.
fn build(s: &mut Session, g: &GearSpec, e: &Extent, cmd: &str) -> Result<()> {
    let straight = Extent { helix: 0.0, double: false, ..*e };
    if g.internal {
        let disc = sketch_profile(s, g, false, Some((g.root_radius() + e.rim, 0.0)), cmd)?;
        solidify(s, g, &straight, disc, "new")?;
        let teeth = sketch_profile(s, g, true, None, cmd)?;
        solidify(s, g, e, teeth, "cut")?;
    } else {
        let teeth = sketch_profile(s, g, true, None, cmd)?;
        solidify(s, g, e, teeth, "new")?;
        if e.bore > 0.0 {
            let hole = sketch_profile(s, g, false, Some((e.bore / 2.0, e.bore_offset)), cmd)?;
            solidify(s, g, &straight, hole, "cut")?;
        }
    }
    Ok(())
}

fn record(g: &GearSpec, e: &Extent) -> SpurGear {
    SpurGear {
        module: g.module,
        teeth: g.teeth,
        pressure_angle: g.pressure_angle.to_degrees(),
        face_width: e.width,
        profile_shift: g.shift,
        backlash: g.backlash,
        flank_points: g.flank_points,
        clearance: g.clearance,
        root_fillet: g.root_fillet,
        tip_round: g.tip_round,
        backlash_thicker: g.thicker,
        internal: g.internal,
        rim_thickness: e.rim,
        bore_diameter: e.bore,
        bore_offset: e.bore_offset,
        helix_angle: e.helix,
        double_helical: e.double,
    }
}

fn reply(g: &GearSpec, comp: u64, occ: u64, name: String) -> Result<Value> {
    let out = InsertedGear {
        component: comp,
        occurrence: occ,
        name,
        pitch_diameter: 2.0 * g.pitch_radius(),
        base_diameter: 2.0 * g.base_radius(),
        tip_diameter: 2.0 * g.tip_radius(),
        root_diameter: 2.0 * g.root_radius(),
    };
    serde_json::to_value(out).map_err(|e| bad("gear.insert", e.to_string()))
}

/// Pitch, base, tip and root diameters (mm) for the values a gear form holds, or `None` when they
/// make no gear. The form shows these as the values change.
pub fn gear_diameters(module: f64, teeth: u32, pressure_angle_deg: f64, shift: f64, clearance: f64, internal: bool) -> Option<[f64; 4]> {
    let mut g = GearSpec::new(module, teeth, pressure_angle_deg);
    g.clearance = clearance;
    g.shift = if internal { 0.0 } else { shift };
    g.internal = internal;
    let d = [2.0 * g.pitch_radius(), 2.0 * g.base_radius(), 2.0 * g.tip_radius(), 2.0 * g.root_radius()];
    d.iter().all(|v| v.is_finite() && *v > 0.0).then_some(d)
}

fn gear_name(internal: bool, module: f64, teeth: u32) -> String {
    format!("{} Gear M{module} Z{teeth}", if internal { "Internal" } else { "Spur" })
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "gear.insert (Insert Gear)";
    let (g, ext) = spec(p, cmd)?;
    outline(&g).map_err(|e| bad(cmd, e))?;
    let name = str_(p, "name").map(str::to_string).unwrap_or_else(|| gear_name(g.internal, g.module, g.teeth));
    let active = s.active_component;
    let c = s.execute("component.create", &json!({"name": name, "grounded": false}))?;
    let (comp, occ) = (c["component"].as_u64().unwrap_or(0), c["occurrence"].as_u64().unwrap_or(0));
    build(s, &g, &ext, cmd)?;
    s.doc_mut().gears.insert(comp, record(&g, &ext));
    s.execute("component.activate", &json!({"component": active}))?;
    reply(&g, comp, occ, name)
}

fn edit_atomic(s: &mut Session, p: &Value) -> Result<Value> {
    let mut computed = s.scratch();
    let result = edit(&mut computed, p)?;
    // Nothing was rebuilt: the document and its revision stay as they are.
    if computed.revision == s.revision && Arc::ptr_eq(&computed.doc, &s.doc) {
        return Ok(result);
    }
    computed.undo.clear();
    computed.active_component = s.active_component;
    computed.active_occurrence_path = s.active_occurrence_path.clone();
    computed.active_sketch = s.active_sketch.filter(|id| computed.doc.feature(*id).is_some());
    s.accept_computed(computed)?;
    Ok(result)
}

/// The command values a saved gear stands for.
fn saved_values(was: &SpurGear) -> Value {
    json!({
        "module": was.module, "teeth": was.teeth, "pressure_angle": was.pressure_angle, "face_width": was.face_width,
        "profile_shift": was.profile_shift, "backlash": was.backlash, "flank_points": was.flank_points,
        "internal": was.internal, "rim_thickness": was.rim_thickness, "bore_diameter": was.bore_diameter,
        "bore_offset": was.bore_offset, "helix_angle": was.helix_angle, "double_helical": was.double_helical,
        "clearance": was.clearance, "root_fillet": was.root_fillet, "tip_round": was.tip_round,
        "backlash_direction": if was.backlash_thicker { "thicker" } else { "thinner" },
    })
}

/// How many features building this gear makes: built in a throwaway copy of the session.
fn own_feature_count(s: &Session, g: &GearSpec, e: &Extent, cmd: &str) -> Result<usize> {
    let mut probe = s.scratch();
    let c = probe.execute("component.create", &json!({"name": "probe", "grounded": false}))?;
    let comp = c["component"].as_u64().unwrap_or(0);
    build(&mut probe, g, e, cmd)?;
    Ok(probe.doc.features.iter().filter(|f| f.component == comp).count())
}

/// Change a gear's values and rebuild its teeth in place; the component, its placement and its
/// joints stay.
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "gear.edit";
    let key = match p.get("component") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`component` must be an id or a name")),
    };
    let comp = s.doc.find_component(&key).ok_or_else(|| bad(cmd, format!("no component `{key}`")))?;
    let was = s.doc.gears.get(&comp).cloned().ok_or_else(|| bad(cmd, "that component is not a gear"))?;
    // The values given replace the saved ones.
    let mut merged = saved_values(&was);
    for k in [
        "module",
        "teeth",
        "pressure_angle",
        "face_width",
        "profile_shift",
        "backlash",
        "flank_points",
        "internal",
        "rim_thickness",
        "bore_diameter",
        "bore_offset",
        "helix_angle",
        "double_helical",
        "clearance",
        "root_fillet",
        "tip_round",
        "backlash_direction",
    ] {
        if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
            merged[k] = v.clone();
        }
    }
    // Turning a gear inside out drops what only the other kind has, unless it is given too.
    let absent = |k: &str| p.get(k).is_none_or(Value::is_null);
    if merged["internal"] == json!(true) && absent("profile_shift") {
        merged["profile_shift"] = json!(0);
    }
    if merged["internal"] == json!(true) && absent("bore_diameter") {
        merged["bore_diameter"] = json!(0);
    } else if merged["internal"] != json!(true) && absent("rim_thickness") {
        merged["rim_thickness"] = json!(0);
    }
    let (g, ext) = spec(&merged, cmd)?;
    outline(&g).map_err(|e| bad(cmd, e))?;
    // The values it already has are no edit: nothing is rebuilt and no undo step is made.
    if record(&g, &ext) == was {
        let name = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_default();
        let occ = s.doc.occurrences.iter().find(|o| o.component == comp).map(|o| o.id).unwrap_or(0);
        return reply(&g, comp, occ, name);
    }
    let ids: Vec<u64> = s.doc.features.iter().filter(|f| f.component == comp).map(|f| f.id).collect();
    // The gear's own features are all this deletes. Anything else made in the component (a sketch,
    // a cut) would go with them and take its dependents along, so refuse instead.
    let (was_g, was_ext) = spec(&saved_values(&was), cmd)?;
    if ids.len() != own_feature_count(s, &was_g, &was_ext, cmd)? {
        return Err(bad(
            cmd,
            "this gear's component holds features that are not the gear's own; edit it by hand, or insert a new gear",
        ));
    }
    s.refresh();
    let before: Vec<(u64, String)> = s.model.results.iter().filter_map(|r| r.error.clone().map(|e| (r.id, e))).collect();
    for id in ids.iter().rev() {
        let _ = s.doc_mut().delete_feature(*id);
    }
    let active = s.active_component;
    s.execute("component.activate", &json!({"component": comp}))?;
    build(s, &g, &ext, cmd)?;
    s.doc_mut().gears.insert(comp, record(&g, &ext));
    // A name the user chose stays; the default one follows the values.
    let name = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_default();
    let name = if name == gear_name(was.internal, was.module, was.teeth) { gear_name(g.internal, g.module, g.teeth) } else { name };
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == comp) {
        c.name = name.clone();
    }
    s.execute("component.activate", &json!({"component": active}))?;
    s.refresh();
    // An error the design already had elsewhere is not this edit's.
    if let Some(e) = s.model.results.iter().find_map(|r| r.error.clone().filter(|e| !before.contains(&(r.id, e.clone())))) {
        return Err(crate::EngineError::Other(e));
    }
    let occ = s.doc.occurrences.iter().find(|o| o.component == comp).map(|o| o.id).unwrap_or(0);
    reply(&g, comp, occ, name)
}
