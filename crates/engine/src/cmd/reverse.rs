//! Reverse engineering: classify an imported dumb solid, rebuild the flat-plate
//! and turned-shaft cases.
//!
//! `reverse.classify` is read-only evidence: it measures one imported body and
//! reports whether it is a flat plate (rectangular outline, uniform thickness,
//! round through-holes) or a turned shaft (coaxial Z cylinder steps with flat
//! ends), with the numbers a rebuild needs. Anything else is a refusal with
//! the reason, never a guess. `reverse.rebuild_plate` / `reverse.rebuild_turned`
//! turn that evidence into a parametric tree so the part can be edited;
//! `dry_run` returns the recipe without building, which is the human/LLM
//! correction hook.

use super::CommandSpec;
use crate::params::{bad, num, string_list};
use crate::{Result, Session};
use serde_json::{Value, json};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("reverse.classify", "Classify Imported Part", classify).icon("inspect").noundo().params(
        "body?: name (default: the only body, else name one) → flat plate or turned shaft: \
         category, dimensions, holes or segments, reasons, unmatched faces",
    ),
    CommandSpec::new("reverse.rebuild_plate", "Rebuild Plate Parametrically", rebuild_plate).icon("box").params(
        "source?: body (default: the only body); holes?: keep detected holes (default true); \
         dry_run?: return the recipe without building (default false); name?: new body name \
         → the parametric rebuild (a box plus one hole feature per hole)",
    ),
    CommandSpec::new("reverse.rebuild_turned", "Rebuild Shaft Parametrically", rebuild_turned).icon("cylinder").params(
        "source?: body (default: the only body); \
         dry_run?: return the recipe without building (default false); name?: new body name \
         → the parametric rebuild (one cylinder feature per shaft step)",
    ),
];

/// Biggest gap the plate search tolerates, in multiples of `tolerance_mm`.
const COAXIAL_SLACK: f64 = 10.0;
/// How parallel two plate faces must be: |dot| above this.
const PARALLEL_DOT: f64 = 0.999;
/// How square a side face must be to the plate: |dot| below this.
const SQUARE_DOT: f64 = 0.001;

fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, x) in a.iter().enumerate() {
        out[i] = x.as_f64()?;
        if !out[i].is_finite() {
            return None;
        }
    }
    Some(out)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The single body to work on: named, or the only one when the design holds one.
fn target_body(s: &mut Session, cmd: &str, p: &Value) -> Result<String> {
    let mut want = string_list(cmd, p, "bodies")?;
    want.extend(string_list(cmd, p, "body")?);
    let measured = s.execute("inspect.measure", &json!({}))?;
    let mut names: Vec<String> = measured
        .get("bodies")
        .and_then(Value::as_array)
        .map(|bs| bs.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    names.retain(|n| want.is_empty() || want.contains(n));
    if names.is_empty() {
        return Err(bad(cmd, "no bodies to classify"));
    }
    if names.len() > 1 {
        return Err(bad(cmd, format!("name one body: {}", names.join(", "))));
    }
    Ok(names.remove(0))
}

fn tolerance_mm(p: &Value, cmd: &str) -> Result<f64> {
    let t = num(p, "tolerance_mm").unwrap_or(0.01);
    if !t.is_finite() || t < 1e-6 || t > 10.0 {
        return Err(bad(cmd, "`tolerance_mm` must be finite, 1e-6…10"));
    }
    Ok(t)
}

fn classify_body(s: &mut Session, body: &str, tol: f64) -> Result<Value> {
    let mut reasons: Vec<Value> = Vec::new();
    let mut unmatched: Vec<Value> = Vec::new();
    let refuse = |reasons: &[Value], unmatched: &[Value], why: &str| {
        let mut reasons = reasons.to_vec();
        reasons.push(json!(why));
        json!({"body": body, "supported": false, "category": "other",
               "reasons": reasons, "unmatched": unmatched})
    };

    let faces_v = s.execute("model.faces", &json!({"body": body}))?;
    let faces: Vec<&Value> = faces_v.get("faces").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    if faces.is_empty() {
        return Ok(refuse(&reasons, &unmatched, "the body has no faces"));
    }
    let measure = s.execute("inspect.measure", &json!({"bodies": [body]}))?;
    let measured = measure.get("bodies").and_then(Value::as_array).and_then(|a| a.first());
    let bbox_min = measured.and_then(|m| m.get("bbox")).and_then(|b| b.get("min")).and_then(v3);
    let bbox_max = measured.and_then(|m| m.get("bbox")).and_then(|b| b.get("max")).and_then(v3);
    let (Some(bmin), Some(bmax)) = (bbox_min, bbox_max) else {
        return Ok(refuse(&reasons, &unmatched, "the body has no bounding box"));
    };
    let size = [bmax[0] - bmin[0], bmax[1] - bmin[1], bmax[2] - bmin[2]];

    // A shaft is all cylinder walls plus flat ends; check it before the plate
    // branch so a plain rod is not mistaken for a plate with one big hole.
    if let Some(turned) = classify_turned(&faces, bmin, bmax, size, tol, body, &mut reasons) {
        return Ok(turned);
    }

    // Plate faces: the two largest planar faces with opposite normals.
    let mut normals: Vec<([f64; 3], [f64; 3], f64, usize)> = Vec::new();
    for f in &faces {
        if let (Some(n), Some(pt)) = (f.get("plane_normal").and_then(v3), f.get("point").and_then(v3)) {
            let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
            let idx = f.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            normals.push((n, pt, area, idx));
        }
    }
    normals.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let Some((n0, p0, _, _)) = normals.first().copied() else {
        return Ok(refuse(&reasons, &unmatched, "no planar faces"));
    };
    let mate = normals.iter().find(|(n, _, _, _)| dot(*n, n0) < -PARALLEL_DOT);
    let Some((n1, p1, _, _)) = mate.copied() else {
        return Ok(refuse(&reasons, &unmatched, "no opposing parallel face pair"));
    };
    let axis = if dot(n0, [0.0, 0.0, 1.0]).abs() > PARALLEL_DOT {
        [0.0, 0.0, 1.0]
    } else {
        return Ok(refuse(&reasons, &unmatched, "the plate is not Z-aligned"));
    };
    let _ = n1;
    let thickness = ((p1[0] - p0[0]) * axis[0] + (p1[1] - p0[1]) * axis[1] + (p1[2] - p0[2]) * axis[2]).abs();
    if thickness <= 0.0 || (thickness - size[2]).abs() > tol.max(size[2] * 0.001) {
        return Ok(refuse(&reasons, &unmatched, "plate faces disagree with the bounding box"));
    }
    let top_z = bmax[2];
    let mid_z = (bmin[2] + bmax[2]) / 2.0;
    reasons.push(json!(format!("two opposing planar faces {thickness:.3} mm apart, Z-aligned")));

    // Remaining faces: sides must be square, cylinders must be full-height holes.
    let mut holes: Vec<Value> = Vec::new();
    let mut bores: Vec<([f64; 3], [f64; 3], f64, [f64; 3])> = Vec::new();
    for f in &faces {
        let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
        let idx = f.get("index").and_then(Value::as_u64).unwrap_or(0);
        if let Some(n) = f.get("plane_normal").and_then(v3) {
            let dn = dot(n, axis).abs();
            let coplanar = f.get("point").and_then(v3).map(|pt| ((pt[2] - bmin[2]).abs(), (pt[2] - bmax[2]).abs()));
            let on_plate = coplanar.map(|(lo, hi)| lo.min(hi) < tol).unwrap_or(false);
            if dn > PARALLEL_DOT && on_plate {
                continue;
            }
            if dn < SQUARE_DOT {
                continue;
            }
            unmatched.push(json!({"index": idx, "area": area, "why": "non-plate planar face"}));
            continue;
        }
        if let Some(cyl) = f.get("cylinder") {
            let (Some(ax), Some(org), Some(r)) =
                (cyl.get("axis").and_then(v3), cyl.get("origin").and_then(v3), cyl.get("radius").and_then(Value::as_f64))
            else {
                unmatched.push(json!({"index": idx, "area": area, "why": "cylinder without axis"}));
                continue;
            };
            let alen = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
            if alen <= 0.0 || r <= 0.0 {
                unmatched.push(json!({"index": idx, "area": area, "why": "degenerate cylinder"}));
                continue;
            }
            let ax = [ax[0] / alen, ax[1] / alen, ax[2] / alen];
            if (dot(ax, axis).abs()) < PARALLEL_DOT {
                unmatched.push(json!({"index": idx, "area": area, "why": "off-axis cylindrical face"}));
                continue;
            }
            let h = area / (2.0 * std::f64::consts::PI * r);
            let centroid = f.get("centroid").and_then(v3).unwrap_or(org);
            if (h - thickness).abs() > tol.max(thickness * 0.01) || (centroid[2] - mid_z).abs() > thickness * 0.05 + tol {
                unmatched.push(json!({"index": idx, "area": area, "why": "blind or partial cylindrical face"}));
                continue;
            }
            bores.push((ax, org, r, centroid));
            continue;
        }
        unmatched.push(json!({"index": idx, "area": area, "why": "non-planar, non-cylindrical face"}));
    }
    // One hole per axis line: entry and exit walls share it.
    let mut used = vec![false; bores.len()];
    for i in 0..bores.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let (ax, org, r, _) = bores[i];
        let mut members = 1;
        for j in (i + 1)..bores.len() {
            if used[j] || (bores[j].2 - r).abs() > tol {
                continue;
            }
            let w = sub(bores[j].1, org);
            let along = dot(w, ax);
            let d2 = dot(w, w) - along * along;
            if d2 <= (COAXIAL_SLACK * tol) * (COAXIAL_SLACK * tol) {
                used[j] = true;
                members += 1;
            }
        }
        let _ = members;
        // Project the bore centroid onto the top face for the drill point.
        let c = bores[i].3;
        let center = [c[0], c[1], top_z];
        holes.push(json!({"center": center, "diameter": 2.0 * r}));
    }
    if !unmatched.is_empty() {
        return Ok(refuse(&reasons, &unmatched, "faces the flat-plate recipe cannot explain (see unmatched)"));
    }
    reasons.push(json!(format!("{} through-hole(s)", holes.len())));
    Ok(json!({"body": body, "supported": true, "category": "flat_plate",
              "length": size[0], "width": size[1], "thickness": thickness,
              "corner": bmin, "top_z": top_z,
              "holes": holes, "reasons": reasons, "unmatched": unmatched}))
}

/// Stepped-shaft check: every face is a Z-parallel cylinder wall or a Z-normal
/// planar ring (flat ends and shoulders). Returns `Some` when the body has the
/// shaft signature — supported with the step list, or refused with the reason —
/// and `None` when it is not a shaft at all (the plate branch runs next).
#[allow(clippy::too_many_arguments)]
fn classify_turned(
    faces: &[&Value],
    bmin: [f64; 3],
    bmax: [f64; 3],
    size: [f64; 3],
    tol: f64,
    body: &str,
    reasons: &mut Vec<Value>,
) -> Option<Value> {
    let axis = [0.0, 0.0, 1.0];
    let refuse = |reasons: &mut Vec<Value>, why: &str| {
        reasons.push(json!(why));
        Some(json!({"body": body, "supported": false, "category": "other",
                    "reasons": reasons, "unmatched": []}))
    };
    // (radius, z0, z1, centre-x, centre-y) per wall, in face order.
    let mut walls: Vec<(f64, f64, f64, f64, f64)> = Vec::new();
    let mut ends = 0;
    for f in faces {
        if let Some(n) = f.get("plane_normal").and_then(v3) {
            if dot(n, axis).abs() < PARALLEL_DOT {
                return None; // a side face: not a shaft
            }
            ends += 1;
            continue;
        }
        if let Some(cyl) = f.get("cylinder") {
            let (Some(ax), Some(org), Some(r)) =
                (cyl.get("axis").and_then(v3), cyl.get("origin").and_then(v3), cyl.get("radius").and_then(Value::as_f64))
            else {
                return None;
            };
            let alen = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
            let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
            if alen <= 0.0 || r <= 0.0 || area <= 0.0 || dot([ax[0] / alen, ax[1] / alen, ax[2] / alen], axis).abs() < PARALLEL_DOT {
                return None; // degenerate or off-axis wall: not a shaft
            }
            let h = area / (2.0 * std::f64::consts::PI * r);
            let c = f.get("centroid").and_then(v3).unwrap_or(org);
            walls.push((r, c[2] - h / 2.0, c[2] + h / 2.0, c[0], c[1]));
            continue;
        }
        return None; // cone, sphere, anything else: not a shaft
    }
    if walls.is_empty() {
        return None;
    }
    if ends < 2 {
        return refuse(reasons, "shaft walls without both flat end faces");
    }
    // All walls share one axis line.
    let (r0, _, _, cx, cy) = walls[0];
    let _ = r0;
    for (r, _, _, x, y) in &walls {
        let _ = r;
        let dx = x - cx;
        let dy = y - cy;
        let slack = COAXIAL_SLACK * tol;
        if dx * dx + dy * dy > slack * slack {
            return refuse(reasons, "cylinder walls are not coaxial");
        }
    }
    // Steps tile the full height with no gaps. Tessellated wall areas read a
    // touch short, so the coverage slack grows with the part, not just `tol`.
    let slack = tol.max(0.005 * size[2]);
    walls.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    if (walls[0].1 - bmin[2]).abs() > slack {
        return refuse(reasons, "lowest wall does not reach the bottom face");
    }
    if (walls[walls.len() - 1].2 - bmax[2]).abs() > slack {
        return refuse(reasons, "highest wall does not reach the top face");
    }
    for w in walls.windows(2) {
        if w[1].1 - w[0].2 > slack {
            return refuse(reasons, "gap between shaft steps the recipe cannot bridge");
        }
    }
    // Snap the outer bounds to the measured box and split shared joints, so the
    // rebuild volumes match instead of carrying tessellation error.
    walls[0].1 = bmin[2];
    let last = walls.len() - 1;
    walls[last].2 = bmax[2];
    for i in 0..last {
        let mid = (walls[i].2 + walls[i + 1].1) / 2.0;
        walls[i].2 = mid;
        walls[i + 1].1 = mid;
    }
    let segments: Vec<Value> = walls.iter().map(|(r, z0, z1, _, _)| json!({"radius": r, "z0": z0, "z1": z1})).collect();
    reasons.push(json!(format!("{} coaxial Z wall(s), flat ends, {:.3} mm tall", walls.len(), size[2])));
    Some(json!({"body": body, "supported": true, "category": "turned",
                "length": size[2], "center": [cx, cy], "bottom": bmin[2],
                "segments": segments, "reasons": reasons, "unmatched": []}))
}

fn classify(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "reverse.classify";
    let tol = tolerance_mm(p, cmd)?;
    let body = target_body(s, cmd, p)?;
    classify_body(s, &body, tol)
}

fn rebuild_plate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "reverse.rebuild_plate";
    let tol = tolerance_mm(p, cmd)?;
    let source = target_body(s, cmd, p)?;
    let keep_holes = p.get("holes").and_then(Value::as_bool).unwrap_or(true);
    let dry_run = p.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let name = p.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{source}_plate"));

    let found = classify_body(s, &source, tol)?;
    if !found.get("supported").and_then(Value::as_bool).unwrap_or(false) {
        let why = found.get("reasons").and_then(Value::as_array).and_then(|r| r.first()).cloned().unwrap_or(json!("unsupported shape"));
        return Err(crate::params::bad(cmd, format!("not supported yet: {why}")));
    }
    let get = |k: &str| found.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let corner = found.get("corner").and_then(v3).unwrap_or([0.0, 0.0, 0.0]);
    let mut recipe: Vec<Value> = vec![json!({"id": "solid.box", "params":
        {"length": get("length"), "width": get("width"), "height": get("thickness"),
         "corner": corner, "body_name": name}})];
    let mut holes: Vec<&Value> = found.get("holes").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    if !keep_holes {
        holes.clear();
    }
    for h in &holes {
        let center = h.get("center").and_then(v3).unwrap_or(corner);
        let diameter = h.get("diameter").and_then(Value::as_f64).unwrap_or(0.0);
        recipe.push(json!({"id": "solid.hole", "params":
            {"position": center, "diameter": diameter, "targets": [name]}}));
    }
    if dry_run {
        return Ok(json!({"dry_run": true, "recipe": recipe, "found": found}));
    }
    let mut built = 0;
    for step in &recipe {
        let id = step.get("id").and_then(Value::as_str).unwrap_or("");
        let params = step.get("params").cloned().unwrap_or(json!({}));
        s.execute(id, &params)?;
        built += 1;
    }
    Ok(json!({"body": name, "holes_built": holes.len(), "steps_built": built, "recipe": recipe}))
}

fn rebuild_turned(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "reverse.rebuild_turned";
    let tol = tolerance_mm(p, cmd)?;
    let source = target_body(s, cmd, p)?;
    let dry_run = p.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let name = p.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{source}_shaft"));

    let found = classify_body(s, &source, tol)?;
    if found.get("category").and_then(Value::as_str) != Some("turned") || !found.get("supported").and_then(Value::as_bool).unwrap_or(false) {
        let why = found.get("reasons").and_then(Value::as_array).and_then(|r| r.last()).cloned().unwrap_or(json!("unsupported shape"));
        return Err(crate::params::bad(cmd, format!("not supported yet: {why}")));
    }
    let center = found.get("center").and_then(v3).unwrap_or([0.0, 0.0, 0.0]);
    let segments: Vec<&Value> = found.get("segments").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let mut recipe: Vec<Value> = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let r = seg.get("radius").and_then(Value::as_f64).unwrap_or(0.0);
        let z0 = seg.get("z0").and_then(Value::as_f64).unwrap_or(0.0);
        let z1 = seg.get("z1").and_then(Value::as_f64).unwrap_or(0.0);
        let mut params = json!(
            {"radius": r, "height": z1 - z0, "base": [center[0], center[1], z0],
             "axis": "Z", "body_name": name});
        if i > 0 {
            // Same name alone does not fuse steps: join explicitly.
            params["operation"] = json!("join");
            params["targets"] = json!([name]);
        }
        recipe.push(json!({"id": "solid.cylinder", "params": params}));
    }
    if dry_run {
        return Ok(json!({"dry_run": true, "recipe": recipe, "found": found}));
    }
    let mut built = 0;
    for step in &recipe {
        let id = step.get("id").and_then(Value::as_str).unwrap_or("");
        let params = step.get("params").cloned().unwrap_or(json!({}));
        s.execute(id, &params)?;
        built += 1;
    }
    Ok(json!({"body": name, "segments_built": segments.len(), "steps_built": built, "recipe": recipe}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }

    fn plate_session() -> Session {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 5, "corner": [0, 0, 0], "body_name": "Src"}));
        run(&mut s, "solid.hole", json!({"position": [10, 10, 5], "diameter": 6, "targets": ["Src"]}));
        run(&mut s, "solid.hole", json!({"position": [50, 30, 5], "diameter": 8, "targets": ["Src"]}));
        s
    }

    fn round_trip() -> (Session, String) {
        let mut s = plate_session();
        let dir = std::env::temp_dir().join("reverse_round_trip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.step");
        run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "step"}));
        let mut r = Session::default();
        let _ = run(&mut r, "file.insert_step", json!({"path": path.to_string_lossy()}));
        (r, "plate".to_string())
    }

    #[test]
    fn flat_plate_round_trip() {
        let (mut s, _hint) = round_trip();
        // The import names bodies after the file; find the single solid body.
        let bodies: Vec<String> = s.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert_eq!(bodies.len(), 1, "one imported body, got {bodies:?}");
        let found = run(&mut s, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "flat_plate");
        assert!((found["thickness"].as_f64().unwrap_or(0.0) - 5.0).abs() < 0.05, "thickness: {found}");
        assert_eq!(found["holes"].as_array().map(Vec::len), Some(2), "holes: {found}");
        let dry = run(&mut s, "reverse.rebuild_plate", json!({"source": bodies[0], "dry_run": true}));
        assert_eq!(dry["recipe"].as_array().map(Vec::len), Some(3), "recipe: {dry}");
        let done = run(&mut s, "reverse.rebuild_plate", json!({"source": bodies[0], "name": "Plate2"}));
        assert_eq!(done["holes_built"], 2, "built: {done}");
        let back = s.execute("inspect.measure", &json!({"bodies": ["Plate2"]})).unwrap();
        let b = &back["bodies"][0];
        let vol = b["volume_mm3"].as_f64().unwrap_or(0.0);
        let hole_vol = std::f64::consts::PI * (3.0 * 3.0 + 4.0 * 4.0) * 5.0;
        assert!((vol - (60.0 * 40.0 * 5.0 - hole_vol)).abs() < 1.0, "volume: {b}");
    }

    #[test]
    fn rod_is_turned() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Rod"}));
        let found = run(&mut s, "reverse.classify", json!({"body": "Rod"}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "turned");
        assert_eq!(found["segments"].as_array().map(Vec::len), Some(1), "segments: {found}");
        let dry = run(&mut s, "reverse.rebuild_turned", json!({"source": "Rod", "dry_run": true}));
        assert_eq!(dry["recipe"].as_array().map(Vec::len), Some(1), "recipe: {dry}");
        let done = run(&mut s, "reverse.rebuild_turned", json!({"source": "Rod", "name": "Rod2"}));
        assert_eq!(done["segments_built"], 1, "built: {done}");
        let back = s.execute("inspect.measure", &json!({"bodies": ["Rod2"]})).unwrap();
        let vol = back["bodies"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
        assert!((vol - std::f64::consts::PI * 100.0 * 20.0).abs() < 1.0, "volume: {back}");
    }

    #[test]
    fn stepped_shaft_round_trip() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 10, "base": [0, 0, 0], "body_name": "Shaft"}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 6, "height": 10, "base": [0, 0, 10], "body_name": "Shaft", "operation": "join", "targets": ["Shaft"]}),
        );
        let dir = std::env::temp_dir().join("reverse_shaft_trip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shaft.step");
        run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "step"}));
        let mut r = Session::default();
        let _ = run(&mut r, "file.insert_step", json!({"path": path.to_string_lossy()}));
        let mut bodies: Vec<String> = r.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert!(!bodies.is_empty(), "imported nothing");
        if bodies.len() > 1 {
            // Face-touching steps may import as separate bodies; fuse them.
            let tools: Vec<Value> = bodies[1..].iter().map(|b| json!(b)).collect();
            run(&mut r, "solid.combine", json!({"target": bodies[0], "tools": tools, "touching": "one_body"}));
            bodies = r.execute("inspect.measure", &json!({})).unwrap()["bodies"]
                .as_array()
                .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
        }
        assert_eq!(bodies.len(), 1, "one fused body, got {bodies:?}");
        let found = run(&mut r, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "turned");
        assert_eq!(found["segments"].as_array().map(Vec::len), Some(2), "segments: {found}");
        let done = run(&mut r, "reverse.rebuild_turned", json!({"source": bodies[0], "name": "Shaft2"}));
        assert_eq!(done["segments_built"], 2, "built: {done}");
        // The shared joint re-splits at its measured midpoint: tessellation-scale
        // shift around the exact union volume.
        let back = r.execute("inspect.measure", &json!({})).unwrap();
        let vols: Vec<f64> = back["bodies"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|b| b.get("name").and_then(Value::as_str).unwrap_or("").starts_with("Shaft2"))
                    .filter_map(|b| b.get("volume_mm3").and_then(Value::as_f64))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(vols.len(), 1, "one rebuilt body, got: {back}");
        let want = std::f64::consts::PI * (100.0 * 10.0 + 36.0 * 10.0);
        assert!((vols[0] - want).abs() < 15.0, "volume {}, want ~{want}: {back}", vols[0]);
    }

    #[test]
    fn non_shaft_refuses() {
        let mut s = Session::default();
        run(&mut s, "solid.sphere", json!({"radius": 10, "body_name": "Ball"}));
        let found = run(&mut s, "reverse.classify", json!({"body": "Ball"}));
        assert_eq!(found["supported"], false, "classify: {found}");
        let err = s.execute("reverse.rebuild_plate", &json!({"source": "Ball"}));
        assert!(err.is_err(), "plate rebuild must refuse");
        let err = s.execute("reverse.rebuild_turned", &json!({"source": "Ball"}));
        assert!(err.is_err(), "turned rebuild must refuse");
    }

    #[test]
    fn hostile_params_never_panic() {
        let mut s = Session::default();
        assert!(s.execute("reverse.classify", &json!({"tolerance_mm": -5.0})).is_err());
        assert!(s.execute("reverse.classify", &json!({"body": "Nope"})).is_err());
        assert!(s.execute("reverse.classify", &json!({})).is_err());
        assert!(s.execute("reverse.rebuild_plate", &json!({"source": "Nope"})).is_err());
    }
}
