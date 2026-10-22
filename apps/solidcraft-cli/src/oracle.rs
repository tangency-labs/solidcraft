//! Reference tests: replay `tests/oracle/<case>/recipe.json` through the command engine and
//! compare the part with `expected.json` (closed-form values, or values from an independent
//! kernel; see tests/oracle/README.md), and with `reference.stl` where one exists.

#![allow(clippy::needless_range_loop)]

use serde_json::{Value, json};
use solidcraft_engine::geom::Vec3;

use crate::spatial::{self, Tri};
use crate::{read_json, recipe};

/// Default relative tolerance for volume and area (our curved-face tessellation is within ~2e-4).
pub const REL_TOL: f64 = 1e-3;
/// Default tolerance for centre of mass and bounding box, as a fraction of the bbox diagonal.
pub(crate) const ABS_FRAC: f64 = 1e-3;
/// Chord tolerance used to triangulate our part for the surface-distance check (mm).
const MESH_TOL: f64 = 0.01;
/// Relative tolerance on the principal second moments of the two triangulations.
const INERTIA_REL: f64 = 2e-3;

/// What SolidCraft built: totals over every body.
pub(crate) struct Got {
    pub(crate) body_count: usize,
    pub(crate) volume: f64,
    pub(crate) area: f64,
    pub(crate) com: [f64; 3],
    pub(crate) bbox: ([f64; 3], [f64; 3]),
    pub(crate) faces: u64,
}

pub(crate) fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}

pub(crate) fn totals(m: &Value) -> Got {
    let bodies = m["bodies"].as_array().cloned().unwrap_or_default();
    let (mut volume, mut area, mut faces) = (0.0, 0.0, 0);
    let mut moment = [0.0; 3];
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for b in &bodies {
        let v = b["volume_mm3"].as_f64().unwrap_or(f64::NAN);
        volume += v;
        area += b["area_mm2"].as_f64().unwrap_or(f64::NAN);
        faces += b["faces"].as_u64().unwrap_or(0);
        let c = v3(&b["center_of_mass"]).unwrap_or([f64::NAN; 3]);
        let (bl, bh) = (v3(&b["bbox"]["min"]).unwrap_or([f64::NAN; 3]), v3(&b["bbox"]["max"]).unwrap_or([f64::NAN; 3]));
        for k in 0..3 {
            moment[k] += v * c[k];
            lo[k] = lo[k].min(bl[k]);
            hi[k] = hi[k].max(bh[k]);
        }
    }
    let com = moment.map(|x| x / volume);
    Got { body_count: bodies.len(), volume, area, com, bbox: (lo, hi), faces }
}

pub(crate) struct Checks {
    pub(crate) list: Vec<Value>,
    pub(crate) pass: bool,
}

impl Checks {
    pub(crate) fn push(&mut self, what: &str, got: Value, want: Value, ok: bool) {
        self.pass &= ok;
        self.list.push(json!({"check": what, "got": got, "want": want, "ok": ok}));
    }
    pub(crate) fn rel(&mut self, what: &str, g: f64, w: Option<f64>, rel: f64) {
        let ok = w.is_some_and(|w| g.is_finite() && (g - w).abs() <= rel * w.abs().max(1.0));
        self.push(what, json!(g), json!(w), ok);
    }
    pub(crate) fn vec(&mut self, what: &str, g: [f64; 3], w: Option<[f64; 3]>, tol: f64) {
        let ok = w.is_some_and(|w| g.iter().zip(w).all(|(g, w)| g.is_finite() && (g - w).abs() <= tol));
        self.push(what, json!(g), json!(w), ok);
    }
}

fn check_case(dir: &str) -> Value {
    let name = std::path::Path::new(dir).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    let recipe = match read_json(&format!("{dir}/recipe.json")) {
        Ok(r) => r,
        Err(e) => return json!({"case": name, "pass": false, "error": e}),
    };
    let want = read_json(&format!("{dir}/expected.json")).ok();
    let script = match recipe::to_script(&recipe) {
        Ok(s) => s,
        Err(e) => return json!({"case": name, "pass": false, "skipped": true, "error": e}),
    };
    let mut s = solidcraft_engine::Session::default();
    if let Err(e) = s.run_script(&script) {
        return json!({"case": name, "pass": false, "error": e.to_string(), "source": want.as_ref().map(|w| w["source"].clone())});
    }
    let got = match s.execute("inspect.measure", &json!({})) {
        Ok(m) => totals(&m),
        Err(e) => return json!({"case": name, "pass": false, "error": e.to_string()}),
    };
    let got_json = json!({
        "body_count": got.body_count, "volume_mm3": got.volume, "area_mm2": got.area,
        "center_of_mass": got.com, "bbox": {"min": got.bbox.0, "max": got.bbox.1}, "faces": got.faces,
    });
    let Some(want) = want else {
        return json!({"case": name, "pass": false, "skipped": true, "error": "no expected.json yet", "got": got_json});
    };
    let only: Option<Vec<String>> = want["compare"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect());
    let wanted = |k: &str| want.get(k).is_some_and(|v| !v.is_null()) && only.as_ref().is_none_or(|o| o.iter().any(|x| x == k));
    let rel = want["tolerance"]["rel"].as_f64().unwrap_or(REL_TOL);
    let diag = |b: ([f64; 3], [f64; 3])| ((b.1[0] - b.0[0]).powi(2) + (b.1[1] - b.0[1]).powi(2) + (b.1[2] - b.0[2]).powi(2)).sqrt();
    let want_bbox = v3(&want["bbox"]["min"]).zip(v3(&want["bbox"]["max"]));
    let abs = want["tolerance"]["abs_mm"].as_f64().unwrap_or_else(|| ABS_FRAC * want_bbox.map(diag).unwrap_or_else(|| diag(got.bbox)).max(1.0));
    let mut c = Checks { list: Vec::new(), pass: true };
    if wanted("body_count") {
        let ok = want["body_count"].as_u64() == Some(got.body_count as u64);
        c.push("body_count", json!(got.body_count), want["body_count"].clone(), ok);
    }
    if wanted("volume_mm3") {
        c.rel("volume_mm3", got.volume, want["volume_mm3"].as_f64(), rel);
    }
    if wanted("area_mm2") {
        c.rel("area_mm2", got.area, want["area_mm2"].as_f64(), rel);
    }
    if wanted("center_of_mass") {
        c.vec("center_of_mass", got.com, v3(&want["center_of_mass"]), abs);
    }
    if wanted("bbox") {
        c.vec("bbox.min", got.bbox.0, want_bbox.map(|b| b.0), abs);
        c.vec("bbox.max", got.bbox.1, want_bbox.map(|b| b.1), abs);
    }
    if wanted("faces") {
        let ok = want["faces"].as_u64() == Some(got.faces);
        c.push("faces", json!(got.faces), want["faces"].clone(), ok);
    }
    let stl = format!("{dir}/reference.stl");
    if std::path::Path::new(&stl).is_file() && only.as_ref().is_none_or(|o| o.iter().any(|x| x == "surface")) {
        let tol = want["tolerance"]["surface_mm"].as_f64().unwrap_or(0.05);
        reference_checks(&s, &stl, tol, &mut c);
    }
    // Our STEP export of the rebuilt part: well formed, and read back with no warnings and the
    // same volume.
    let (ok, note) = step_export_check(&s, got.volume);
    c.push("step_export", json!(note), json!("valid, re-imports cleanly"), ok);
    let mut out = json!({"case": name, "pass": c.pass, "source": want["source"], "checks": c.list, "got": got_json});
    if let Some(n) = want["note"].as_str() {
        out["note"] = json!(n);
    }
    out
}

/// Export the session's bodies as STEP, validate the file and import it again.
fn step_export_check(s: &solidcraft_engine::Session, volume: f64) -> (bool, String) {
    let st = s.model.state();
    let bytes = match solidcraft_engine::io::export(&st, &[], solidcraft_engine::io::Format::Step, "oracle") {
        Ok(b) => b,
        Err(e) => return (false, format!("export: {e}")),
    };
    let text = String::from_utf8_lossy(&bytes);
    if let Err(e) = solidcraft_engine::kernel::step_validate(&text) {
        return (false, format!("invalid: {e}"));
    }
    match solidcraft_engine::kernel::step_orientation_errors(&text) {
        Ok(errs) if !errs.is_empty() => return (false, format!("faces inside out: {}", errs.join("; "))),
        Err(e) => return (false, format!("orientation check: {e}")),
        Ok(_) => {}
    }
    match solidcraft_engine::kernel::step_import(&text) {
        Ok(imp) if !imp.warnings.is_empty() => (false, format!("warnings: {}", imp.warnings.join("; "))),
        Ok(imp) if imp.bodies.len() != st.bodies.len() => (false, format!("{} bodies back of {}", imp.bodies.len(), st.bodies.len())),
        Ok(imp) => {
            let m: Vec<_> = imp.bodies.iter().filter_map(|b| solidcraft_engine::kernel::measure(&b.body).ok()).collect();
            let m0: Vec<_> = st.bodies.iter().filter_map(|b| solidcraft_engine::kernel::measure(&b.body).ok()).collect();
            let v: f64 = m.iter().map(|x| x.volume).sum();
            let (a, a0): (f64, f64) = (m.iter().map(|x| x.area).sum(), m0.iter().map(|x| x.area).sum());
            let faces: usize = imp.bodies.iter().map(|b| b.file_faces).sum();
            let faces0: usize = m0.iter().map(|x| x.merged.faces).sum();
            let close = |x: f64, y: f64| (x - y).abs() <= REL_TOL * y.abs().max(1.0);
            let faces_raw: usize = st.bodies.iter().map(|b| b.body.face_count()).sum();
            // (Fewer is fine: pieces of one plane, cylinder, cone, sphere or torus are written as one face.)
            let ok = close(v, volume) && close(a, a0) && faces <= faces_raw;
            (ok, format!("re-imported volume {v:.3} (was {volume:.3}), area {a:.3} (was {a0:.3}), faces {faces} (was {faces0})"))
        }
        Err(e) => (false, format!("import: {e}")),
    }
}

/// Our part as a triangle soup (curved faces refined onto the surface).
fn our_mesh(s: &solidcraft_engine::Session) -> Result<Vec<Tri>, String> {
    let mut ours: Vec<Tri> = Vec::new();
    for b in &s.world_state().bodies {
        let m = b.body.mesh(&solidcraft_engine::kernel::MeshTolerance::chord(MESH_TOL)).map_err(|e| e.to_string())?;
        let p = |i: u32| m.positions.get(i as usize).map(|v: &Vec3| [v.x, v.y, v.z]);
        for t in &m.triangles {
            if let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) {
                ours.push([a, b, c]);
            }
        }
    }
    if ours.is_empty() { Err("empty mesh".into()) } else { Ok(ours) }
}

/// Compare our part with the reference triangulation: the symmetric surface distance, and the
/// principal second moments of volume of both meshes (the shape's inertia, independent of how it
/// sits in space).
fn reference_checks(s: &solidcraft_engine::Session, stl: &str, tol: f64, c: &mut Checks) {
    let (ours, reference) = match (our_mesh(s), spatial::read_stl(stl)) {
        (Ok(o), Ok(r)) if !r.is_empty() => (o, r),
        (Err(e), _) | (_, Err(e)) => return c.push("surface_distance_mm", json!(e), json!(format!("<= {tol}")), false),
        _ => return c.push("surface_distance_mm", json!("empty reference.stl"), json!(format!("<= {tol}")), false),
    };
    let (lo, hi) = spatial::bounds(&reference);
    let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
    let h = spatial::hausdorff(&ours, &reference, size.max(1.0));
    let worst = h.a_to_b.max(h.b_to_a);
    c.push(
        "surface_distance_mm",
        json!({"max": worst, "mean": h.mean, "ours_to_reference": h.a_to_b, "reference_to_ours": h.b_to_a, "worst_at": h.worst_at}),
        json!(format!("<= {tol}")),
        worst <= tol,
    );
    let ((_, _, pa), (_, _, pb)) = (spatial::mass_properties(&ours), spatial::mass_properties(&reference));
    let ok = pa.iter().zip(pb).all(|(a, b)| (a - b).abs() <= INERTIA_REL * b.abs().max(1.0));
    c.push("principal_moments_mm5", json!(pa), json!(pb), ok);
}

fn report(args: &[String], results: &[Value]) {
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&Value::Array(results.to_vec())).unwrap_or_default());
        return;
    }
    for r in results {
        let status = if r["pass"].as_bool().unwrap_or(false) {
            "PASS"
        } else if r["skipped"].as_bool().unwrap_or(false) {
            "SKIP"
        } else {
            "FAIL"
        };
        let detail: Vec<String> = r["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| !c["ok"].as_bool().unwrap_or(false))
            .map(|c| format!("{} got {} want {}", c["check"].as_str().unwrap_or(""), c["got"], c["want"]))
            .collect();
        let err = r["error"].as_str().map(|e| format!(" ({e})")).unwrap_or_default();
        println!(
            "{status} {}{err}{}",
            r["case"].as_str().unwrap_or(""),
            if detail.is_empty() { String::new() } else { format!(": {}", detail.join("; ")) }
        );
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let dirs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if dirs.is_empty() {
        return Err("oracle: give one or more case directories (each with recipe.json and expected.json)".into());
    }
    let results: Vec<Value> = dirs.iter().map(|d| check_case(d)).collect();
    report(args, &results);
    let failed = results.iter().filter(|r| !r["pass"].as_bool().unwrap_or(false) && !r["skipped"].as_bool().unwrap_or(false)).count();
    if failed > 0 { Err(format!("{failed} oracle case(s) failed")) } else { Ok(()) }
}
