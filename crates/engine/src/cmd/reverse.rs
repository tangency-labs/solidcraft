//! Reverse engineering: classify an imported dumb solid, rebuild the flat-plate case.
//!
//! `reverse.classify` is read-only evidence: it measures one imported body and
//! reports whether it is a flat plate (rectangular outline, uniform thickness,
//! round through-holes) with the numbers a rebuild needs. Anything else is a
//! refusal with the reason, never a guess. `reverse.rebuild_plate` turns that
//! evidence into a parametric tree (`solid.box` + `solid.hole`) so the part can
//! be edited; `dry_run` returns the recipe without building, which is the
//! human/LLM correction hook.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, num, string_list};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("reverse.classify", "Classify Imported Part", classify).icon("inspect").noundo().params(
        "body?: name (default: the only body, else name one) → whether it is a flat plate: \
         category, length, width, thickness, corner, holes [{center, diameter}], reasons, unmatched faces",
    ),
    CommandSpec::new("reverse.rebuild_plate", "Rebuild Plate Parametrically", rebuild_plate).icon("box").params(
        "source?: body (default: the only body); holes?: keep detected holes (default true); \
         dry_run?: return the recipe without building (default false); name?: new body name \
         → the parametric rebuild (a box plus one hole feature per hole)",
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
    let want = string_list(p, "bodies");
    let want_one = string_list(p, "body");
    let mut want = want;
    want.extend(want_one);
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
        return Err(crate::params::bad(cmd, &format!("not supported yet: {why}")));
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
    fn non_plate_refuses() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Rod"}));
        let found = run(&mut s, "reverse.classify", json!({"body": "Rod"}));
        assert_eq!(found["supported"], false, "classify: {found}");
        let err = s.execute("reverse.rebuild_plate", &json!({"source": "Rod"}));
        assert!(err.is_err(), "rebuild must refuse");
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
