//! Involute spur gears (#6796): a tooth outline from `gear_profile`, sketched and extruded like
//! any other feature, so the gear stays an ordinary component.

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_sketch::CurveKind;

use super::CommandSpec;
use super::gear_profile::{GearSpec, outline};
use crate::params::{bad, num, str_};
use crate::schema::gears::{GearInsertParams, InsertedGear};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const INSERT_CONTRACT: TypedContract<GearInsertParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("gear.insert", "Insert Gear", crate::checked!("gear.insert", insert_atomic))
        .at("SOLID", "INSERT")
        .icon("fastener")
        .contract(&INSERT_CONTRACT)
        .output(output_schema_for::<InsertedGear>)
        .params("module (mm), teeth (6..1000), pressure_angle? (deg, default 20), face_width? (mm), profile_shift?, backlash? (mm), flank_points? (2..64, default 8), name? → component, occurrence, name, pitch/base/tip/root diameters"),
];

/// A number parameter that may be left out; one that is present but not a usable number is refused.
fn optional(p: &Value, k: &str, cmd: &str) -> Result<Option<f64>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => num(p, k).map(Some).ok_or_else(|| bad(cmd, format!("`{k}` must be a finite number below 1e9"))),
    }
}

fn spec(p: &Value, cmd: &str) -> Result<(GearSpec, f64)> {
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
    let width = optional(p, "face_width", cmd)?.unwrap_or(module * 10.0);
    if !(width > 0.0) {
        return Err(bad(cmd, "`face_width` must be above 0"));
    }
    Ok((g, width))
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

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "gear.insert (Insert Gear)";
    let (g, width) = spec(p, cmd)?;
    let points = outline(&g).map_err(|e| bad(cmd, e))?;
    let name = str_(p, "name").map(str::to_string).unwrap_or_else(|| format!("Spur Gear M{} Z{}", g.module, g.teeth));
    let active = s.active_component;
    let c = s.execute("component.create", &json!({"name": name, "grounded": false}))?;
    let (comp, occ) = (c["component"].as_u64().unwrap_or(0), c["occurrence"].as_u64().unwrap_or(0));
    s.execute("sketch.create", &json!({"plane": "XY"}))?;
    // Each flank is one spline through its points, each tip and root one line: 4 edges a tooth.
    // All go in with one sketch edit: a command per curve would copy the design for each.
    let flank = points.len() / (g.teeth as usize * 2);
    super::sketch::edit(s, &json!({}), cmd, false, |sk, _| {
        let mut ids = Vec::with_capacity(points.len());
        for q in &points {
            ids.push(sk.add_point(Vec2::new(q[0], q[1]), None).map_err(|e| bad(cmd, e.to_string()))?);
        }
        let chunks: Vec<&[usize]> = ids.chunks(flank).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let kind = CurveKind::Spline { pts: chunk.to_vec(), control: false, degree: 3, periodic: false, nurbs: None };
            sk.add_curve(kind, None).map_err(|e| bad(cmd, e.to_string()))?;
            // After a right flank comes the tip; after a left flank, the root.
            let (from, to) = (chunk[chunk.len() - 1], chunks[(i + 1) % chunks.len()][0]);
            sk.add_line_pts(from, to, None).map_err(|e| bad(cmd, e.to_string()))?;
        }
        Ok(())
    })?;
    s.execute("sketch.finish", &json!({}))?;
    s.execute("solid.extrude", &json!({"distance": format!("{width} mm"), "direction": "positive", "operation": "new"}))?;
    s.execute("component.activate", &json!({"component": active}))?;
    let out = InsertedGear {
        component: comp,
        occurrence: occ,
        name,
        pitch_diameter: 2.0 * g.pitch_radius(),
        base_diameter: 2.0 * g.base_radius(),
        tip_diameter: 2.0 * g.tip_radius(),
        root_diameter: 2.0 * g.root_radius(),
    };
    serde_json::to_value(out).map_err(|e| bad(cmd, e.to_string()))
}
