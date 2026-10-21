//! The kernel regression rig: replay `tests/kernel-cases/<area>/<id>/recipe.json` (an oracle
//! recipe or a command script) and grade the result against the case's own `expected.json`:
//! closed-form numbers where the case has them, and always the invariants (the operation's
//! outcome, one closed valid solid per body, the expected counts). One kernel per run; kernels
//! are never compared with each other. See tests/kernel-cases/README.md.

use serde_json::{Value, json};

use crate::oracle::{ABS_FRAC, Checks, REL_TOL, totals, v3};
use crate::{read_json, recipe};

/// The kernel this process runs booleans with (`SOLIDCRAFT_BOOLEANS`, set by the caller).
fn kernel() -> String {
    std::env::var("SOLIDCRAFT_BOOLEANS").ok().filter(|k| !k.is_empty()).unwrap_or_else(|| "default".into())
}

/// The case's commands: a script as is, or an oracle recipe translated. In a string, `$CASE` is
/// the case directory (input files such as `input.step`) and `$OUT` a scratch directory for files
/// the case writes (an export), outside the repository.
fn script(dir: &str, out: &str, recipe: &Value) -> Result<Value, String> {
    let s = if recipe.get("commands").is_some() || recipe.is_array() { recipe.clone() } else { recipe::to_script(recipe)? };
    let text = serde_json::to_string(&s).map_err(|e| e.to_string())?;
    let inner = |p: &str| -> Result<String, String> {
        let j = serde_json::to_string(p).map_err(|e| e.to_string())?;
        Ok(j.get(1..j.len().saturating_sub(1)).unwrap_or("").to_string())
    };
    serde_json::from_str(&text.replace("$CASE", &inner(dir)?).replace("$OUT", &inner(out)?)).map_err(|e| e.to_string())
}

/// A scratch directory for one case's output files.
fn scratch(area: &str, name: &str) -> Result<String, String> {
    let d = std::env::temp_dir().join(format!("solidcraft-kernel-cases-{}", std::process::id())).join(format!("{area}-{name}"));
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d.to_string_lossy().to_string())
}

/// An error that means "this kernel can't do it yet", not a wrong answer.
fn unsupported(e: &str) -> bool {
    e.contains("not supported yet")
}

/// A panic caught by the engine or the kernel guard.
fn crashed(e: &str) -> bool {
    e.contains("internal error") || e.contains("panicked")
}

fn case_name(dir: &str) -> (String, String) {
    let p = std::path::Path::new(dir);
    let name = |q: Option<&std::path::Path>| q.and_then(|q| q.file_name()).map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    (name(p.parent()), name(Some(p)))
}

fn check_case(dir: &str) -> Value {
    let (area, name) = case_name(dir);
    let mut out = json!({"case": name, "area": area, "kernel": kernel()});
    let want = match read_json(&format!("{dir}/expected.json")) {
        Ok(w) => w,
        Err(e) => {
            out["status"] = json!("fail");
            out["error"] = json!(format!("expected.json: {e}"));
            return out;
        }
    };
    out["sources"] = want["sources"].clone();
    let commands = match scratch(&area, &name).and_then(|o| read_json(&format!("{dir}/recipe.json")).and_then(|r| script(dir, &o, &r))) {
        Ok(s) => s,
        Err(e) => {
            out["status"] = json!("fail");
            out["error"] = json!(format!("recipe.json: {e}"));
            return out;
        }
    };
    if kernel() == "smlib" && !solidcraft_engine::kernel::smlib_built() {
        out["status"] = json!("unsupported");
        out["error"] = json!("this build has no SMLib: build solidcraft-cli with --features smlib");
        return out;
    }
    let running = solidcraft_engine::kernel::boolean_kernel();
    if kernel() != running {
        out["status"] = json!("unsupported");
        out["error"] = json!(format!("this build runs booleans on the {running} kernel, not {}", kernel()));
        return out;
    }
    // `refused`: only a clean refusal is right; `solid_or_refused`: a clean refusal, or a result
    // graded as usual (where the geometry has no manifold answer, such as bodies touching along a
    // curve only).
    let outcome = want["outcome"].as_str().unwrap_or("solid");
    let refused = outcome == "refused";
    let may_refuse = refused || outcome == "solid_or_refused";
    let mut s = solidcraft_engine::Session::default();
    let ran = s.run_script(&commands);
    match (ran, may_refuse) {
        (Err(e), _) if crashed(&e.to_string()) => {
            out["status"] = json!("crash");
            out["error"] = json!(e.to_string());
            return out;
        }
        (Err(e), true) => {
            // The right answer is a clean refusal.
            out["status"] = json!("pass");
            out["refused_with"] = json!(e.to_string());
            return out;
        }
        (Err(e), false) => {
            let e = e.to_string();
            out["status"] = json!(if unsupported(&e) { "unsupported" } else { "fail" });
            out["error"] = json!(e);
            return out;
        }
        (Ok(_), true) if refused => {
            out["status"] = json!("fail");
            out["error"] = json!("succeeded where the case expects a refusal");
            return out;
        }
        (Ok(_), _) => {}
    }

    // Every body in the world, imported components included.
    let world = s.world_state();
    let mut bodies = Vec::new();
    let mut c = Checks { list: Vec::new(), pass: true };
    let want_valid = want["valid"].as_bool().unwrap_or(true);
    // Shells: the total where the case gives it (a body with a void, or several lumps in one
    // body), else one per body.
    let want_shells = want["shells"].as_u64();
    let mut shells = 0u64;
    for b in &world.bodies {
        let m = match solidcraft_engine::kernel::measure(&b.body) {
            Ok(m) => m,
            Err(e) => {
                c.push("measure", json!(format!("{}: {e}", b.name)), json!("measurable"), false);
                continue;
            }
        };
        bodies.push(json!({
            "volume_mm3": m.volume, "area_mm2": m.area, "center_of_mass": m.centroid,
            "bbox": {"min": m.bbox.min, "max": m.bbox.max},
            "faces": m.merged.faces, "edges": m.merged.edges, "vertices": m.merged.vertices,
        }));
        shells += m.shells as u64;
        if want_valid {
            // Closed shells, and every validity check this build has.
            if want_shells.is_none() {
                c.push("shells", json!(format!("{}: {}", b.name, m.shells)), json!(1), m.shells == 1);
            }
            let v = solidcraft_engine::kernel::validity(&b.body);
            c.push("record_valid", json!(v.record.clone().err().unwrap_or_else(|| "ok".into())), json!("ok"), v.record.is_ok());
            match &v.smlib {
                Some(solidcraft_engine::kernel::SmlibCheck::Valid) => c.push("smlib_check", json!("ok"), json!("ok"), true),
                Some(solidcraft_engine::kernel::SmlibCheck::Invalid(e)) => c.push("smlib_check", json!(e), json!("ok"), false),
                // No verdict: the record can't be built in SMLib yet. Shown, not graded.
                Some(solidcraft_engine::kernel::SmlibCheck::NotSupported(e)) => {
                    c.push("smlib_check", json!(format!("no verdict: {e}")), json!("ok"), true)
                }
                None => {}
            }
        }
    }
    let got = totals(&json!({"bodies": bodies}));
    let (edges, vertices): (u64, u64) =
        bodies.iter().fold((0, 0), |(e, v), b| (e + b["edges"].as_u64().unwrap_or(0), v + b["vertices"].as_u64().unwrap_or(0)));
    let has = |k: &str| want.get(k).is_some_and(|v| !v.is_null());
    let rel = want["tolerance"]["rel"].as_f64().unwrap_or(REL_TOL);
    let diag = |b: ([f64; 3], [f64; 3])| ((b.1[0] - b.0[0]).powi(2) + (b.1[1] - b.0[1]).powi(2) + (b.1[2] - b.0[2]).powi(2)).sqrt();
    let want_bbox = v3(&want["bbox"]["min"]).zip(v3(&want["bbox"]["max"]));
    let abs = want["tolerance"]["abs_mm"].as_f64().unwrap_or_else(|| ABS_FRAC * want_bbox.map(diag).unwrap_or_else(|| diag(got.bbox)).max(1.0));
    let count = |c: &mut Checks, k: &str, g: u64| {
        if has(k) {
            c.push(k, json!(g), want[k].clone(), want[k].as_u64() == Some(g));
        }
    };
    count(&mut c, "body_count", got.body_count as u64);
    count(&mut c, "shells", shells);
    count(&mut c, "faces", got.faces);
    count(&mut c, "edges", edges);
    count(&mut c, "vertices", vertices);
    if has("volume_mm3") {
        c.rel("volume_mm3", got.volume, want["volume_mm3"].as_f64(), rel);
    }
    if has("area_mm2") {
        c.rel("area_mm2", got.area, want["area_mm2"].as_f64(), rel);
    }
    if has("center_of_mass") {
        c.vec("center_of_mass", got.com, v3(&want["center_of_mass"]), abs);
    }
    if has("bbox") {
        c.vec("bbox.min", got.bbox.0, want_bbox.map(|b| b.0), abs);
        c.vec("bbox.max", got.bbox.1, want_bbox.map(|b| b.1), abs);
    }
    out["status"] = json!(if c.pass { "pass" } else { "fail" });
    out["checks"] = json!(c.list);
    out
}

fn report(args: &[String], results: &[Value]) {
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&Value::Array(results.to_vec())).unwrap_or_default());
        return;
    }
    for r in results {
        let detail: Vec<String> = r["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| !c["ok"].as_bool().unwrap_or(false))
            .map(|c| format!("{} got {} want {}", c["check"].as_str().unwrap_or(""), c["got"], c["want"]))
            .collect();
        let err = r["error"].as_str().map(|e| format!(" ({e})")).unwrap_or_default();
        println!(
            "{} {}/{} [{}]{err}{}",
            r["status"].as_str().unwrap_or("?").to_uppercase(),
            r["area"].as_str().unwrap_or(""),
            r["case"].as_str().unwrap_or(""),
            r["kernel"].as_str().unwrap_or(""),
            if detail.is_empty() { String::new() } else { format!(": {}", detail.join("; ")) }
        );
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let dirs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if dirs.is_empty() {
        return Err("kernel-cases: give one or more case directories (each with recipe.json and expected.json)".into());
    }
    let results: Vec<Value> = dirs.iter().map(|d| check_case(d)).collect();
    report(args, &results);
    let bad = results.iter().filter(|r| matches!(r["status"].as_str(), Some("fail" | "crash"))).count();
    if bad > 0 { Err(format!("{bad} kernel case(s) failed")) } else { Ok(()) }
}
