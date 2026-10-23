//! Laser mark commands: LightBurn handoff packages (not CNC manufacture).
//!
//! - `laser.profiles` — list built-in machine profiles.
//! - `laser.exportPackage` — build DXF + SVG + laser-job.json from the active sketch
//!   (or explicit polylines) and optionally write them under a path stem.

use serde_json::{Value, json};
use solidcraft_laser::package::{Polyline2, build_package, package_filenames};
use solidcraft_laser::profile::{ProfileId, builtin_profiles};
use solidcraft_sketch::CurveKind;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{EngineError, Result, Session};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("laser.profiles", "Laser Profiles", profiles).icon("list").noundo().doc_only().params("{}"),
    CommandSpec::new("laser.exportPackage", "Export LightBurn Package", export_package).icon("export").noundo().params(
        "profile?: commarker-60w-mopa; field_mm?: number; path?: stem or path without extension \
(writes .dxf .svg .laser-job.json); sketch?: name|id; polylines?: [{points:[[x,y],…], closed?, layer?}]",
    ),
];

fn profiles(s: &mut Session, p: &Value) -> Result<Value> {
    let _ = (s, p);
    let list: Vec<Value> = builtin_profiles()
        .iter()
        .map(|pr| {
            json!({
                "id": pr.id.as_str(),
                "name": pr.name,
                "bed_width_mm": pr.bed_width_mm,
                "bed_height_mm": pr.bed_height_mm,
                "default_field_mm": pr.default_field_mm(),
                "field_options_mm": pr.field_options_mm,
                "lightburn_device": pr.lightburn_device,
                "lightburn_model": pr.lightburn_model,
                "source": pr.source,
                "notes": pr.notes,
            })
        })
        .collect();
    Ok(json!({ "profiles": list }))
}

fn export_package(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "laser.exportPackage";
    let profile_id = match str_(p, "profile") {
        Some(raw) if !raw.trim().is_empty() => ProfileId::parse(raw).ok_or_else(|| bad(CMD, format!("unknown laser profile `{raw}`")))?,
        _ => ProfileId::Commarker60wMopa,
    };
    let field_mm = p.get("field_mm").and_then(Value::as_f64);
    let polylines =
        if let Some(arr) = p.get("polylines").and_then(Value::as_array) { parse_polylines(CMD, arr)? } else { sketch_polylines(s, p, CMD)? };
    let pkg = build_package(profile_id, field_mm, &polylines).map_err(|e| bad(CMD, e.to_string()))?;

    if let Some(path) = str_(p, "path").filter(|x| !x.trim().is_empty()) {
        let (dir, stem) = split_stem(path);
        let (dxf_name, svg_name, job_name) = package_filenames(&stem).map_err(|e| bad(CMD, e.to_string()))?;
        let dxf_path = join_path(&dir, &dxf_name);
        let svg_path = join_path(&dir, &svg_name);
        let job_path = join_path(&dir, &job_name);
        solidcraft_io::vfs::write(&dxf_path, pkg.dxf.as_bytes()).map_err(|e| EngineError::Other(format!("{dxf_path}: {e}")))?;
        solidcraft_io::vfs::write(&svg_path, pkg.svg.as_bytes()).map_err(|e| EngineError::Other(format!("{svg_path}: {e}")))?;
        solidcraft_io::vfs::write(&job_path, pkg.job_json.as_bytes()).map_err(|e| EngineError::Other(format!("{job_path}: {e}")))?;
        return Ok(json!({
            "profile": pkg.meta.profile,
            "field_mm": pkg.meta.field_mm,
            "polylines": polylines.len(),
            "paths": { "dxf": dxf_path, "svg": svg_path, "job": job_path },
        }));
    }

    Ok(json!({
        "profile": pkg.meta.profile,
        "field_mm": pkg.meta.field_mm,
        "polylines": polylines.len(),
        "dxf": pkg.dxf,
        "svg": pkg.svg,
        "job": pkg.job_json,
        "meta": pkg.meta,
    }))
}

fn parse_polylines(cmd: &str, arr: &[Value]) -> Result<Vec<Polyline2>> {
    let mut out = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let points = v.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, format!("polylines[{i}].points required")))?;
        let mut pts = Vec::with_capacity(points.len());
        for (j, pt) in points.iter().enumerate() {
            let xy = pt.as_array().ok_or_else(|| bad(cmd, format!("polylines[{i}].points[{j}] must be [x,y]")))?;
            if xy.len() < 2 {
                return Err(bad(cmd, format!("polylines[{i}].points[{j}] must be [x,y]")));
            }
            let x = xy[0].as_f64().ok_or_else(|| bad(cmd, format!("polylines[{i}].points[{j}].x")))?;
            let y = xy[1].as_f64().ok_or_else(|| bad(cmd, format!("polylines[{i}].points[{j}].y")))?;
            pts.push([x, y]);
        }
        out.push(Polyline2 {
            points: pts,
            closed: v.get("closed").and_then(Value::as_bool).unwrap_or(false),
            layer: v.get("layer").and_then(Value::as_str).unwrap_or("mark").to_string(),
        });
    }
    Ok(out)
}

fn sketch_polylines(s: &Session, p: &Value, cmd: &str) -> Result<Vec<Polyline2>> {
    let id = match p.get("sketch") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(x)) => s.doc.find_feature(x).map(|f| f.id),
        _ => s.active_sketch,
    }
    .ok_or_else(|| bad(cmd, "no such sketch (and none is active); pass polylines or activate a sketch"))?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
    let sk = &ss.sketch;
    let mut out = Vec::new();
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction {
            continue;
        }
        let pt = |k: usize| {
            let p = sk.point(k).unwrap_or_default();
            [p.x, p.y]
        };
        match &c.kind {
            CurveKind::Line { a, b } => out.push(Polyline2 { points: vec![pt(*a), pt(*b)], closed: false, layer: "mark".into() }),
            CurveKind::Circle { c, r } => {
                let center = sk.point(*c).unwrap_or_default();
                let n = 64usize;
                let mut points = Vec::with_capacity(n);
                for k in 0..n {
                    let t = (k as f64) * std::f64::consts::TAU / (n as f64);
                    points.push([center.x + r * t.cos(), center.y + r * t.sin()]);
                }
                out.push(Polyline2 { points, closed: true, layer: "mark".into() });
            }
            CurveKind::Arc { .. } => {
                let poly = sk.polyline(i);
                if poly.len() >= 2 {
                    out.push(Polyline2 { points: poly.iter().map(|p| [p.x, p.y]).collect(), closed: false, layer: "mark".into() });
                }
            }
            _ => {
                let poly = sk.polyline(i);
                if poly.len() >= 2 {
                    let closed = poly.first().zip(poly.last()).is_some_and(|(a, b)| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
                    out.push(Polyline2 { points: poly.iter().map(|p| [p.x, p.y]).collect(), closed, layer: "mark".into() });
                }
            }
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, "sketch has no drawable curves for laser export"));
    }
    Ok(out)
}

fn split_stem(path: &str) -> (String, String) {
    let path = path.trim().trim_end_matches(['/', '\\']);
    let (dir, file) = match path.rfind(['/', '\\']) {
        Some(i) => (path[..=i].to_string(), path[i + 1..].to_string()),
        None => (String::new(), path.to_string()),
    };
    let stem = file
        .trim_end_matches(".dxf")
        .trim_end_matches(".DXF")
        .trim_end_matches(".svg")
        .trim_end_matches(".SVG")
        .trim_end_matches(".laser-job.json")
        .trim_end_matches(".json")
        .to_string();
    (dir, stem)
}

fn join_path(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_string() } else { format!("{dir}{name}") }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn lists_commarker_profile() {
        let mut s = Session::default();
        let out = s.execute("laser.profiles", &json!({})).unwrap();
        let ids: Vec<&str> = out["profiles"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"commarker-60w-mopa"));
    }

    #[test]
    fn export_package_from_sketch_returns_dxf() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [30, 20]})).unwrap();
        let out = s.execute("laser.exportPackage", &json!({"profile": "commarker-60w-mopa", "field_mm": 110.0})).unwrap();
        assert_eq!(out["profile"], "commarker-60w-mopa");
        assert!(out["dxf"].as_str().unwrap().contains("LINE"));
        assert!(out["job"].as_str().unwrap().contains("JCZFiber"));
    }

    #[test]
    fn export_package_from_explicit_polylines() {
        let mut s = Session::default();
        let out = s
            .execute(
                "laser.exportPackage",
                &json!({
                    "polylines": [{
                        "points": [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0]],
                        "closed": true,
                        "layer": "engrave"
                    }]
                }),
            )
            .unwrap();
        assert!(out["polylines"].as_u64().unwrap() >= 1);
        let job: serde_json::Value = serde_json::from_str(out["job"].as_str().unwrap()).unwrap();
        assert_eq!(job["layers"][0]["intent"], "engrave");
    }
}
