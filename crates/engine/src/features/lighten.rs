//! Parametric Lighten / Auto-Pocket feature definition, evaluation, and command handler.

use serde_json::Value;
use solidcraft_doc::expr::Kind as ExprKind;
use solidcraft_doc::{FeatureKind, LightenFeature};
use solidcraft_geom::prelude::*;
pub use solidcraft_kernel::lighten::{ChamferLocation, FilletLocation, LightenConfig, ManufacturingMethod, PocketDepthSpec};

use crate::cmd::CommandSpec;
use crate::cmd::features::add_feature;
use crate::params::{bad, bool_, num, str_, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.lighten", "Lighten", lighten).at("SOLID", "MODIFY").icon("lighten").params(
    "target_face?: u64 (planar face index, or picked via faces/selection); \
             faces?: [[x,y,z]]; body?: name; wall_offset?: expr (default 4.0 mm); \
             depth?: expr (default 7.0 mm); draft_angle?: expr (default 0 deg); \
             corner_radius?: expr (default 3.0 mm); \
             auto_fillet?: {radius: expr, location?: bottom|top|all} | bool; \
             auto_chamfer?: {size: expr, location?: top|bottom|all} | bool; \
             method?: cnc_pocket|laser_waterjet|additive_weight_reduction; name?",
)];

/// Evaluate a [`LightenFeature`] directly onto `body`.
pub fn eval_lighten(body: &mut solidcraft_kernel::Body, feat: &LightenFeature) -> std::result::Result<(), String> {
    let config = LightenConfig {
        wall_offset: feat.wall_offset,
        depth_spec: PocketDepthSpec::BlindDepth(feat.depth),
        corner_radius: feat.corner_radius,
        draft_angle: feat.draft_angle,
        auto_fillet_radius: feat.auto_fillet.map(|(r, _)| r),
        auto_chamfer_size: feat.auto_chamfer.map(|(c, _)| c),
        method: ManufacturingMethod::CncPocket,
        island_faces: Vec::new(),
    };
    let fillet_loc = feat.auto_fillet.map(|(_, loc)| loc).unwrap_or_default();
    let chamfer_loc = feat.auto_chamfer.map(|(_, loc)| loc).unwrap_or_default();
    solidcraft_kernel::lighten::auto_pocket_with_locations(body, feat.target_face, &config, fillet_loc, chamfer_loc)
}

fn eval_expr_or_num(s: &Session, p: &Value, keys: &[&str], kind: ExprKind) -> Result<Option<f64>> {
    for &k in keys {
        let Some(v) = p.get(k) else { continue };
        match v {
            Value::Null => continue,
            Value::Number(n) => {
                if let Some(f) = n.as_f64().filter(|x| x.is_finite()) {
                    return Ok(Some(f));
                }
                return Err(bad("solid.lighten", format!("`{k}` must be a finite number")));
            }
            Value::String(text) if !text.trim().is_empty() => {
                let val = s.doc.eval(text.trim(), kind).map_err(|e| bad("solid.lighten", format!("`{k}`: {e}")))?;
                return Ok(Some(val));
            }
            _ => return Err(bad("solid.lighten", format!("`{k}` must be a number or expression"))),
        }
    }
    Ok(None)
}

fn parse_fillet_location(v: Option<&Value>) -> Result<FilletLocation> {
    match v.and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        None | Some("") | Some("bottom") => Ok(FilletLocation::Bottom),
        Some("top") => Ok(FilletLocation::Top),
        Some("all") => Ok(FilletLocation::All),
        Some(other) => Err(bad("solid.lighten", format!("unknown fillet location `{other}`"))),
    }
}

fn parse_chamfer_location(v: Option<&Value>) -> Result<ChamferLocation> {
    match v.and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        None | Some("") | Some("top") => Ok(ChamferLocation::Top),
        Some("bottom") => Ok(ChamferLocation::Bottom),
        Some("all") => Ok(ChamferLocation::All),
        Some(other) => Err(bad("solid.lighten", format!("unknown chamfer location `{other}`"))),
    }
}

fn parse_method(p: &Value) -> Result<ManufacturingMethod> {
    let raw = str_(p, "method").or_else(|| str_(p, "manufacturing_method")).unwrap_or("cnc_pocket");
    match raw.to_ascii_lowercase().as_str() {
        "cnc_pocket" | "cnc_milling" | "cnc" | "milling" => Ok(ManufacturingMethod::CncPocket),
        "laser_waterjet" | "laser" | "waterjet" | "sheet_metal_punch" | "punch" => Ok(ManufacturingMethod::LaserWaterjet),
        "additive_weight_reduction" | "additive_fdm_sla" | "additive" | "3d_print" => Ok(ManufacturingMethod::AdditiveWeightReduction),
        other => Err(bad("solid.lighten", format!("unknown manufacturing method `{other}`"))),
    }
}

fn resolve_target_face(s: &Session, p: &Value) -> Result<(usize, u64)> {
    let st = s.model.state();
    if let Some(v) = p.get("target_face") {
        let face_id = v.as_u64().ok_or_else(|| bad("solid.lighten", "target_face must be an integer face ID"))?;
        let body_idx = if let Some(bname) = str_(p, "body") {
            st.bodies.iter().position(|b| b.name == bname).unwrap_or(0)
        } else {
            st.bodies.iter().position(|b| (face_id as usize) < b.body.face_count()).unwrap_or(0)
        };
        return Ok((body_idx, face_id));
    }

    let point = p.get("faces").and_then(Value::as_array).and_then(|a| a.first()).and_then(vec3).or_else(|| p.get("face").and_then(vec3));
    if let Some(pt) = point {
        let body_filter = str_(p, "body");
        let mut best: Option<(f64, usize, usize)> = None;
        for (bi, mb) in st.bodies.iter().enumerate() {
            if let Some(target_name) = body_filter
                && mb.name != target_name
            {
                continue;
            }
            if let Ok(faces) = mb.body.faces(0.25) {
                for f in faces {
                    if f.plane_normal.is_some() {
                        let d = f.centroid.dist(pt);
                        if best.is_none_or(|(bd, _, _)| d < bd) {
                            best = Some((d, bi, f.index));
                        }
                    }
                }
            }
        }
        if let Some((_, bi, fi)) = best {
            return Ok((bi, fi as u64));
        }
        return Err(bad("solid.lighten", "could not locate a planar face at the specified point"));
    }

    if let Some(crate::Sel::Face { body, index, .. }) = s.selection.iter().find(|sel| matches!(sel, crate::Sel::Face { .. })) {
        let bi = st.bodies.iter().position(|b| &b.name == body).unwrap_or(0);
        return Ok((bi, *index as u64));
    }

    Err(bad("solid.lighten", "target_face or faces selection is required"))
}

struct ParsedLighten {
    body_idx: usize,
    target_face: u64,
    feature: LightenFeature,
    config: LightenConfig,
}

fn parse_lighten_params(s: &Session, p: &Value) -> Result<ParsedLighten> {
    let (body_idx, target_face) = resolve_target_face(s, p)?;

    let wall_offset = eval_expr_or_num(s, p, &["wall_offset", "wall_clearance_mm"], ExprKind::Length)?.unwrap_or(4.0);
    let corner_radius = eval_expr_or_num(s, p, &["corner_radius", "corner_fillet"], ExprKind::Length)?.unwrap_or(3.0);
    let draft_angle = eval_expr_or_num(s, p, &["draft_angle", "draft_angle_deg"], ExprKind::Angle)?.unwrap_or(0.0);
    let method = parse_method(p)?;

    let (depth_val, depth_spec) = if let Some(depth_obj) = p.get("depth_spec") {
        if let Some(d) = depth_obj.as_f64() {
            (d, PocketDepthSpec::BlindDepth(d))
        } else if let Some(text) = depth_obj.as_str() {
            if text.eq_ignore_ascii_case("through") || text.eq_ignore_ascii_case("through_hole") {
                (100.0, PocketDepthSpec::ThroughHole)
            } else {
                let d = s.doc.eval(text, ExprKind::Length).map_err(|e| bad("solid.lighten", format!("depth_spec: {e}")))?;
                (d, PocketDepthSpec::BlindDepth(d))
            }
        } else if let Some(b) =
            depth_obj.get("blind_depth").or_else(|| depth_obj.get("blind")).or_else(|| depth_obj.get("BlindDepth")).and_then(Value::as_f64)
        {
            (b, PocketDepthSpec::BlindDepth(b))
        } else if let Some(f) = depth_obj
            .get("floor_thickness")
            .or_else(|| depth_obj.get("floor_offset"))
            .or_else(|| depth_obj.get("FloorThickness"))
            .and_then(Value::as_f64)
        {
            (f, PocketDepthSpec::FloorThickness(f))
        } else if depth_obj.get("through_hole").or_else(|| depth_obj.get("ThroughHole")).is_some() {
            (100.0, PocketDepthSpec::ThroughHole)
        } else {
            return Err(bad("solid.lighten", "invalid depth_spec"));
        }
    } else if let Some(d) = eval_expr_or_num(s, p, &["depth", "pocket_depth", "blind"], ExprKind::Length)? {
        (d, PocketDepthSpec::BlindDepth(d))
    } else if let Some(f) = num(p, "floor_thickness").or_else(|| num(p, "floor_offset")) {
        (f, PocketDepthSpec::FloorThickness(f))
    } else if bool_(p, "through").unwrap_or(false) {
        (100.0, PocketDepthSpec::ThroughHole)
    } else {
        (7.0, PocketDepthSpec::BlindDepth(7.0))
    };

    let auto_fillet = match p.get("auto_fillet") {
        Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => {
            let r = eval_expr_or_num(s, p, &["auto_fillet_radius", "fillet_radius"], ExprKind::Length)?.unwrap_or(2.0);
            let loc = parse_fillet_location(p.get("auto_fillet_location").or_else(|| p.get("fillet_location")))?;
            Some((r, loc))
        }
        Some(Value::Array(arr)) if arr.len() == 2 => {
            let r = arr[0].as_f64().ok_or_else(|| bad("solid.lighten", "auto_fillet[0] must be a number"))?;
            let loc = parse_fillet_location(arr.get(1))?;
            Some((r, loc))
        }
        Some(Value::Object(obj)) => {
            let r = match obj.get("radius") {
                Some(Value::Number(n)) => n.as_f64().ok_or_else(|| bad("solid.lighten", "auto_fillet.radius invalid"))?,
                Some(Value::String(txt)) => {
                    s.doc.eval(txt.trim(), ExprKind::Length).map_err(|e| bad("solid.lighten", format!("auto_fillet.radius: {e}")))?
                }
                _ => 2.0,
            };
            let loc = parse_fillet_location(obj.get("location"))?;
            Some((r, loc))
        }
        Some(Value::Number(n)) => {
            let r = n.as_f64().ok_or_else(|| bad("solid.lighten", "auto_fillet must be a finite number"))?;
            let loc = parse_fillet_location(p.get("auto_fillet_location"))?;
            Some((r, loc))
        }
        None => eval_expr_or_num(s, p, &["auto_fillet_radius"], ExprKind::Length)?
            .map(|r| Ok::<_, crate::EngineError>((r, parse_fillet_location(p.get("auto_fillet_location"))?)))
            .transpose()?,
        _ => return Err(bad("solid.lighten", "invalid auto_fillet parameter")),
    };

    let auto_chamfer = match p.get("auto_chamfer") {
        Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => {
            let c = eval_expr_or_num(s, p, &["auto_chamfer_size", "chamfer_size"], ExprKind::Length)?.unwrap_or(1.0);
            let loc = parse_chamfer_location(p.get("auto_chamfer_location").or_else(|| p.get("chamfer_location")))?;
            Some((c, loc))
        }
        Some(Value::Array(arr)) if arr.len() == 2 => {
            let c = arr[0].as_f64().ok_or_else(|| bad("solid.lighten", "auto_chamfer[0] must be a number"))?;
            let loc = parse_chamfer_location(arr.get(1))?;
            Some((c, loc))
        }
        Some(Value::Object(obj)) => {
            let c = match obj.get("size") {
                Some(Value::Number(n)) => n.as_f64().ok_or_else(|| bad("solid.lighten", "auto_chamfer.size invalid"))?,
                Some(Value::String(txt)) => {
                    s.doc.eval(txt.trim(), ExprKind::Length).map_err(|e| bad("solid.lighten", format!("auto_chamfer.size: {e}")))?
                }
                _ => 1.0,
            };
            let loc = parse_chamfer_location(obj.get("location"))?;
            Some((c, loc))
        }
        Some(Value::Number(n)) => {
            let c = n.as_f64().ok_or_else(|| bad("solid.lighten", "auto_chamfer must be a finite number"))?;
            let loc = parse_chamfer_location(p.get("auto_chamfer_location"))?;
            Some((c, loc))
        }
        None => eval_expr_or_num(s, p, &["auto_chamfer_size"], ExprKind::Length)?
            .map(|c| Ok::<_, crate::EngineError>((c, parse_chamfer_location(p.get("auto_chamfer_location"))?)))
            .transpose()?,
        _ => return Err(bad("solid.lighten", "invalid auto_chamfer parameter")),
    };

    let config = LightenConfig {
        wall_offset,
        depth_spec,
        corner_radius,
        draft_angle,
        auto_fillet_radius: auto_fillet.map(|(r, _)| r),
        auto_chamfer_size: auto_chamfer.map(|(c, _)| c),
        method,
        island_faces: Vec::new(),
    };
    config.validate().map_err(|msg| bad("solid.lighten", msg))?;

    let feature = LightenFeature { target_face, wall_offset, depth: depth_val, draft_angle, corner_radius, auto_fillet, auto_chamfer };

    Ok(ParsedLighten { body_idx, target_face, feature, config })
}

/// Build before/after model states with fast pocket boolean cuts (no 3D SMLib blends) for live preview.
pub fn preview_cut_state_for_command(
    s: &Session,
    p: &Value,
) -> std::result::Result<(solidcraft_doc::ModelState, solidcraft_doc::ModelState), String> {
    let parsed = parse_lighten_params(s, p).map_err(|e| e.to_string())?;
    let before = (*s.model.state()).clone();
    let mb = before.bodies.get(parsed.body_idx).ok_or_else(|| "no target body for lighten preview".to_string())?;
    let mut cut_body = mb.body.clone();
    let fillet_loc = parsed.feature.auto_fillet.map(|(_, loc)| loc).unwrap_or_default();
    let chamfer_loc = parsed.feature.auto_chamfer.map(|(_, loc)| loc).unwrap_or_default();
    solidcraft_kernel::lighten::preview_cut_body_with_locations(&mut cut_body, parsed.target_face, &parsed.config, fillet_loc, chamfer_loc)?;
    let mut after = before.clone();
    let mut next_mb = solidcraft_doc::ModelBody::new(mb.name.clone(), cut_body, mb.feature);
    next_mb.component = mb.component;
    after.bodies[parsed.body_idx] = next_mb;
    Ok((before, after))
}

/// Build cutter prism preview bodies (prior to committing the boolean cut) for `solid.lighten`.
pub fn preview_pockets_for_command(s: &Session, p: &Value) -> std::result::Result<Vec<solidcraft_kernel::Body>, String> {
    let parsed = parse_lighten_params(s, p).map_err(|e| e.to_string())?;
    let st = s.model.state();
    let mb = st.bodies.get(parsed.body_idx).ok_or_else(|| "no target body for lighten preview".to_string())?;
    let fillet_loc = parsed.feature.auto_fillet.map(|(_, loc)| loc).unwrap_or_default();
    let chamfer_loc = parsed.feature.auto_chamfer.map(|(_, loc)| loc).unwrap_or_default();
    solidcraft_kernel::lighten::preview_pockets_with_locations(&mb.body, parsed.target_face, &parsed.config, fillet_loc, chamfer_loc)
}

/// Execute the `solid.lighten` command, pushing a non-destructive parametric `FeatureKind::Lighten` step onto the timeline.
pub fn lighten(s: &mut Session, p: &Value) -> Result<Value> {
    let parsed = parse_lighten_params(s, p)?;
    add_feature(s, p, FeatureKind::Lighten(parsed.feature))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lighten_config_validation() {
        let mut cfg = LightenConfig::default();
        assert!(cfg.validate().is_ok());

        cfg.wall_offset = -1.0;
        assert!(cfg.validate().is_err());
        cfg.wall_offset = 4.0;

        cfg.depth_spec = PocketDepthSpec::BlindDepth(-2.0);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_eval_lighten_directly() {
        let mut body = solidcraft_kernel::box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(50.0, 50.0, 20.0)).unwrap();
        let top =
            body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3)).unwrap();
        let feat = LightenFeature {
            target_face: top.index as u64,
            wall_offset: 4.0,
            depth: 7.0,
            draft_angle: 0.0,
            corner_radius: 3.0,
            auto_fillet: None,
            auto_chamfer: None,
        };
        let vol_before = solidcraft_kernel::measure(&body).unwrap().volume;
        eval_lighten(&mut body, &feat).unwrap();
        let vol_after = solidcraft_kernel::measure(&body).unwrap().volume;
        assert!(vol_after < vol_before);
    }

    #[test]
    fn test_command_execution_on_session() {
        let mut s = Session::new(solidcraft_doc::Document::new("Untitled"));
        s.execute("solid.box", &serde_json::json!({"length": 50, "width": 50, "height": 20})).unwrap();
        let st = s.model.state();
        let top = st.bodies[0]
            .body
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3))
            .unwrap();
        let res = s.execute(
            "solid.lighten",
            &serde_json::json!({
                "target_face": top.index,
                "wall_offset": "4.0 mm",
                "depth": "7.0 mm",
                "draft_angle": "0 deg",
                "corner_radius": "3.0 mm",
            }),
        );
        assert!(res.is_ok(), "solid.lighten failed: {res:?}");
    }

    #[test]
    fn test_command_auto_fillet_chamfer_and_preview() {
        let mut s = Session::new(solidcraft_doc::Document::new("Untitled"));
        s.execute("solid.box", &serde_json::json!({"length": 50, "width": 50, "height": 20})).unwrap();
        let top = s.model.state().bodies[0]
            .body
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3))
            .unwrap();

        let params = serde_json::json!({
            "target_face": top.index,
            "wall_offset": "4.0 mm",
            "depth": "7.0 mm",
            "draft_angle": "0 deg",
            "corner_radius": "3.0 mm",
            "auto_fillet": { "radius": "2.0 mm", "location": "bottom" },
            "auto_chamfer": { "size": "1.0 mm", "location": "top" },
        });
        let parsed = parse_lighten_params(&s, &params).unwrap();
        assert_eq!(parsed.config.auto_fillet_radius, Some(2.0));
        assert_eq!(parsed.config.auto_chamfer_size, Some(1.0));
        assert_eq!(parsed.feature.auto_fillet, Some((2.0, FilletLocation::Bottom)));
        assert_eq!(parsed.feature.auto_chamfer, Some((1.0, ChamferLocation::Top)));

        let preview = preview_pockets_for_command(&s, &params).unwrap();
        assert_eq!(preview.len(), 1);
        assert!(preview[0].face_count() > 6, "preview prism should include rounded corner & edge blend faces");

        let res = s.execute("solid.lighten", &params);
        assert!(res.is_ok(), "solid.lighten with auto blends failed: {res:?}");
    }
}
