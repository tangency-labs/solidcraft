//! Dogbone reliefs: round bites a router's bit cuts into inside corners so that square parts
//! seat in them (`solid.dogbone`).
use serde_json::Value;
use solidcraft_doc::dogbone::{DogboneSide, DogboneStyle, MAX_LENGTH};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{BodyName, Dir, FacePick, FeatureKind, Position};

use super::CommandSpec;
use crate::params::{bad, expr, req_expr, str_};
use crate::schema::dogbone::{DogboneParams, DogboneResult, DogboneSkip};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<DogboneParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.dogbone", "Dogbone", crate::checked!("solid.dogbone", dogbone))
    .at("SOLID", "MODIFY")
    .icon("dogbone")
    .contract(&CONTRACT)
    .output(output_schema_for::<DogboneResult>)
    .params(
        "edges?: [[x,y,z] point on an inside corner edge | {body, index}]; faces?: [[x,y,z] points on planar faces, or face names from model.faces] (a pocket's floor, a plate's face a slot runs through: every inside corner edge along the face's normal at its corners); give edges, faces or both; diameter: expr (the router bit's diameter; each relief is a round hole of it, running the corner's length: from a blind pocket's floor, never below it, out past an open end); style?: normal|minimal|t_bone (normal, the default: centred on the corner's bisector, the circle through the sharp corner; of all reliefs that reach the sharp corner, it removes the least material. minimal: on the bisector, stopping `gap` short of the corner, for mating parts with slightly broken corners. t_bone: centred on one wall's line, the longer or the shorter one; at a square corner it bites that wall only, at a sharper corner it also bites the other wall); gap?: expr (minimal only; default 5% of the diameter); side?: longer|shorter (t_bone only: which wall each relief bites; default longer); max_angle?: angle expr (corners more open are skipped; default 90 deg); along?: X|Y|Z|[x,y,z] (the bit's axis: picked edges not along it are skipped; default any; a face's corners are along its normal); body?. Only inside (concave) corners between planar faces are relieved: the reply's `relieved` counts them and `skipped` lists every other edge with its reason (convex, too_open, not_along_axis, not_straight, …); none relieved is refused.",
    )];

fn dogbone(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.dogbone";
    let given = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    if !given("edges") && !given("faces") {
        return Err(bad(cmd, "pick the inside corner edges (`edges`) or the faces whose inside corners to relieve (`faces`)"));
    }
    let picks = if given("edges") { Some(super::features::edge_points(s, p, cmd)?) } else { None };
    let faces = if given("faces") { super::features::face_points(s, p, cmd)? } else { Vec::new() };
    let picked = super::features::faces_body(s, p, &faces);
    let body = match (picks.as_ref().and_then(|e| e.body.clone()), picked.body.clone()) {
        (Some(a), Some(b)) if a != b => return Err(bad(cmd, format!("the edges are on `{a}` and the faces on `{b}`: relieve one body at a time"))),
        (a, b) => a.or(b),
    };
    let diameter = req_expr(cmd, p, "diameter")?;
    super::features::check_expr(s, &diameter, Kind::Length, cmd, "diameter")?;
    let d = s.doc.eval(&diameter, Kind::Length).map_err(|e| bad(cmd, format!("diameter: {e}")))?;
    if !(d.is_finite() && d > 0.0 && d <= MAX_LENGTH) {
        return Err(bad(cmd, format!("`diameter` must be more than 0 and at most {MAX_LENGTH} mm")));
    }
    let style = match str_(p, "style") {
        None => DogboneStyle::Normal,
        Some(x) => {
            let x = x.to_ascii_lowercase();
            DogboneStyle::parse(&x).ok_or_else(|| bad(cmd, format!("unknown style `{x}` (normal, minimal or t_bone)")))?
        }
    };
    let gap = expr(p, "gap");
    if let Some(g) = &gap {
        if style != DogboneStyle::Minimal {
            return Err(bad(cmd, "`gap` is for the minimal style"));
        }
        super::features::check_expr(s, g, Kind::Length, cmd, "gap")?;
        let v = s.doc.eval(g, Kind::Length).map_err(|e| bad(cmd, format!("gap: {e}")))?;
        if !(v.is_finite() && (0.0..=MAX_LENGTH).contains(&v)) {
            return Err(bad(cmd, format!("`gap` must be at least 0 and at most {MAX_LENGTH} mm")));
        }
    }
    let side = match str_(p, "side") {
        None => DogboneSide::Longer,
        Some(_) if style != DogboneStyle::TBone => return Err(bad(cmd, "`side` is for the t_bone style")),
        Some(x) => {
            let x = x.to_ascii_lowercase();
            DogboneSide::parse(&x).ok_or_else(|| bad(cmd, format!("unknown side `{x}` (longer or shorter)")))?
        }
    };
    let max_angle = expr(p, "max_angle");
    if let Some(a) = &max_angle {
        super::features::check_expr(s, a, Kind::Angle, cmd, "max_angle")?;
        let v = s.doc.eval(a, Kind::Angle).map_err(|e| bad(cmd, format!("max_angle: {e}")))?;
        if !(v.is_finite() && v > 0.0 && v <= std::f64::consts::PI) {
            return Err(bad(cmd, "`max_angle` must be between 0 and 180 deg"));
        }
    }
    let along = match p.get("along") {
        None | Some(Value::Null) => None,
        Some(v) => Some(Dir(super::features::axis_dir_form(v).map_err(|_| bad(cmd, crate::schema::dogbone::ALONG_MESSAGE))?)),
    };
    let kind = FeatureKind::Dogbone {
        edges: picks.map(|e| e.points.into_iter().map(Position).collect()).unwrap_or_default(),
        faces: faces.into_iter().map(FacePick).collect(),
        diameter,
        style,
        gap,
        side,
        max_angle,
        along,
        body: body.map(BodyName::from),
    };
    let mut out = super::features::add_face_feature(&picked, s, p, kind)?;
    // What the feature relieved and skipped.
    let report = out
        .get("feature")
        .and_then(Value::as_u64)
        .and_then(|id| s.model.result(id).and_then(|res| res.state.dogbone_report(id)).cloned())
        .unwrap_or_default();
    let skipped: Vec<DogboneSkip> =
        report.skipped.iter().map(|(at, why)| DogboneSkip { at: [at.x, at.y, at.z], reason: (*why).to_string() }).collect();
    out["relieved"] = serde_json::json!(report.relieved.len());
    out["relieved_edges"] = serde_json::json!(report.relieved.iter().map(|v| [v.x, v.y, v.z]).collect::<Vec<_>>());
    out["skipped"] = serde_json::to_value(skipped).map_err(|e| EngineError::Internal(cmd.into(), e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
#[path = "dogbone_tests.rs"]
mod tests;
