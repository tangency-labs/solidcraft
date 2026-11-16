//! Involute spur gears (#6796): a tooth outline from `gear_profile`, sketched and extruded like
//! any other feature, so the gear stays an ordinary component.

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
        .icon("fastener")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<InsertedGear>)
        .params("component: id|name of an inserted gear; module?, teeth?, pressure_angle?, face_width?, profile_shift?, backlash?, flank_points?, internal?, rim_thickness?, bore_diameter? (the values given replace the saved ones) → rebuilds the teeth in place; component, occurrence, name, diameters"),
    CommandSpec::new("gear.insert", "Insert Gear", crate::checked!("gear.insert", insert_atomic))
        .at("SOLID", "INSERT")
        .icon("fastener")
        .contract(&INSERT_CONTRACT)
        .output(output_schema_for::<InsertedGear>)
        .params("module (mm), teeth (6..1000), pressure_angle? (deg, default 20), face_width? (mm), profile_shift?, backlash? (mm), flank_points? (2..64, default 8), internal? (bool: a ring with its teeth inward; no profile shift), rim_thickness? (mm, internal only, default 2 × module), bore_diameter? (mm, external only: an axle hole), name? → component, occurrence, name, pitch/base/tip/root diameters"),
];

/// A number parameter that may be left out; one that is present but not a usable number is refused.
fn optional(p: &Value, k: &str, cmd: &str) -> Result<Option<f64>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => num(p, k).map(Some).ok_or_else(|| bad(cmd, format!("`{k}` must be a finite number below 1e9"))),
    }
}

/// What a gear needs besides its tooth outline.
struct Extent {
    width: f64,
    /// Internal gears: ring thickness outside the tooth roots.
    rim: f64,
    /// External gears: diameter of the axle hole, 0 for none.
    bore: f64,
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
    if let Some(n) = p.get("flank_points").and_then(Value::as_u64) {
        g.flank_points = u32::try_from(n).unwrap_or(u32::MAX);
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
    // The hole must leave a rim of material inside the root circle.
    if bore < 0.0 || (bore > 0.0 && bore >= 1.8 * g.root_radius().min(g.base_radius())) {
        return Err(bad(cmd, "`bore_diameter` must be from 0 up to 90% of the gear's root circle"));
    }
    Ok((g, Extent { width, rim, bore }))
}

fn insert_atomic(s: &mut Session, p: &Value) -> Result<Value> {
    let mut computed = s.scratch();
    let result = insert(&mut computed, p)?;
    computed.undo.clear();
    computed.active_component = s.active_component;
    computed.active_occurrence_path = s.active_occurrence_path.clone();
    computed.active_sketch = s.active_sketch.filter(|id| computed.doc.feature(*id).is_some());
    s.accept_computed(computed);
    Ok(result)
}

/// A sketch on the XY plane holding the tooth outline (`teeth`), a circle round the centre, or both.
fn sketch_profile(s: &mut Session, g: &GearSpec, teeth: bool, circle: Option<f64>, cmd: &str) -> Result<()> {
    s.execute("sketch.create", &json!({"plane": "XY"}))?;
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
        if let Some(r) = circle {
            sk.add_circle(Vec2::new(0.0, 0.0), r, None, None).map_err(|e| bad(cmd, e.to_string()))?;
        }
        Ok(())
    })?;
    s.execute("sketch.finish", &json!({})).map(|_| ())
}

/// The gear's teeth as sketch and extrusion in the active component. An external gear is its tooth
/// outline, with an axle hole cut through it. An internal gear is the ring's disc
/// with the tooth outline cut through it.
fn build(s: &mut Session, g: &GearSpec, e: &Extent, cmd: &str) -> Result<()> {
    let distance = format!("{} mm", e.width);
    if g.internal {
        sketch_profile(s, g, false, Some(g.root_radius() + e.rim), cmd)?;
        s.execute("solid.extrude", &json!({"distance": distance, "direction": "positive", "operation": "new"}))?;
        sketch_profile(s, g, true, None, cmd)?;
        s.execute("solid.extrude", &json!({"distance": distance, "direction": "positive", "operation": "cut"}))?;
    } else {
        sketch_profile(s, g, true, None, cmd)?;
        s.execute("solid.extrude", &json!({"distance": distance, "direction": "positive", "operation": "new"}))?;
        if e.bore > 0.0 {
            sketch_profile(s, g, false, Some(e.bore / 2.0), cmd)?;
            s.execute("solid.extrude", &json!({"distance": distance, "direction": "positive", "operation": "cut"}))?;
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
        internal: g.internal,
        rim_thickness: e.rim,
        bore_diameter: e.bore,
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
    computed.undo.clear();
    computed.active_component = s.active_component;
    computed.active_occurrence_path = s.active_occurrence_path.clone();
    computed.active_sketch = s.active_sketch.filter(|id| computed.doc.feature(*id).is_some());
    s.accept_computed(computed);
    Ok(result)
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
    let mut merged = json!({
        "module": was.module, "teeth": was.teeth, "pressure_angle": was.pressure_angle, "face_width": was.face_width,
        "profile_shift": was.profile_shift, "backlash": was.backlash, "flank_points": was.flank_points,
        "internal": was.internal, "rim_thickness": was.rim_thickness, "bore_diameter": was.bore_diameter,
    });
    for k in
        ["module", "teeth", "pressure_angle", "face_width", "profile_shift", "backlash", "flank_points", "internal", "rim_thickness", "bore_diameter"]
    {
        if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
            merged[k] = v.clone();
        }
    }
    // Turning a gear inside out drops what only the other kind has, unless it is given too.
    let absent = |k: &str| p.get(k).is_none_or(Value::is_null);
    if merged["internal"] == json!(true) && absent("bore_diameter") {
        merged["bore_diameter"] = json!(0);
    } else if merged["internal"] != json!(true) && absent("rim_thickness") {
        merged["rim_thickness"] = json!(0);
    }
    let (g, ext) = spec(&merged, cmd)?;
    outline(&g).map_err(|e| bad(cmd, e))?;
    let ids: Vec<u64> = s.doc.features.iter().filter(|f| f.component == comp).map(|f| f.id).collect();
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
    if let Some(e) = s.model.results.iter().find_map(|r| r.error.clone()) {
        return Err(crate::EngineError::Other(e));
    }
    let occ = s.doc.occurrences.iter().find(|o| o.component == comp).map(|o| o.id).unwrap_or(0);
    reply(&g, comp, occ, name)
}
