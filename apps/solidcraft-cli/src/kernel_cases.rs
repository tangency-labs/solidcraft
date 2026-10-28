//! The kernel regression rig: replay `tests/kernel-cases/<area>/<id>/recipe.json` (an oracle
//! recipe or a command script) and grade the result against the case's own `expected.json`:
//! closed-form numbers where the case has them, and always the invariants (the operation's
//! outcome, one closed valid solid per body, the expected counts). One kernel per run; kernels
//! are never compared with each other. See tests/kernel-cases/README.md.

use serde_json::{Value, json};

use crate::oracle::{ABS_FRAC, Checks, REL_TOL, totals, v3};
use crate::{read_json, recipe};

/// The kernel the cases run on: SMLib, the only one.
fn kernel() -> String {
    "smlib".into()
}

/// How many cases a chain of `base` recipes may go through (a ladder case builds on the one
/// before it, which builds on the one before that).
const MAX_BASE_DEPTH: usize = 32;

/// The case's commands: a script as is, or an oracle recipe translated. In a string, `$CASE` is
/// the case directory (input files such as `input.step`) and `$OUT` a scratch directory for files
/// the case writes (an export), outside the repository. A script with `"base": "<case
/// directory>"` (relative to this case's directory) runs that case's commands first: a ladder
/// case starts from the previous tier's finished part without copying its commands.
fn script(dir: &str, out: &str, recipe: &Value) -> Result<Value, String> {
    script_at(dir, out, recipe, 0)
}

fn script_at(dir: &str, out: &str, recipe: &Value, depth: usize) -> Result<Value, String> {
    let s = if recipe.get("commands").is_some() || recipe.is_array() { recipe.clone() } else { recipe::to_script(recipe)? };
    let text = serde_json::to_string(&s).map_err(|e| e.to_string())?;
    let inner = |p: &str| -> Result<String, String> {
        let j = serde_json::to_string(p).map_err(|e| e.to_string())?;
        Ok(j.get(1..j.len().saturating_sub(1)).unwrap_or("").to_string())
    };
    let own: Value = serde_json::from_str(&text.replace("$CASE", &inner(dir)?).replace("$OUT", &inner(out)?)).map_err(|e| e.to_string())?;
    let Some(base) = recipe.get("base").and_then(Value::as_str) else { return Ok(own) };
    if depth >= MAX_BASE_DEPTH {
        return Err(format!("`base` chain deeper than {MAX_BASE_DEPTH} cases"));
    }
    let base_dir = std::path::Path::new(dir).join(base).to_string_lossy().to_string();
    let base_script = read_json(&format!("{base_dir}/recipe.json"))
        .and_then(|r| script_at(&base_dir, out, &r, depth + 1))
        .map_err(|e| format!("base {base}: {e}"))?;
    let list = |v: &Value| v.get("commands").or(Some(v)).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut commands = list(&base_script);
    commands.extend(list(&own));
    Ok(json!({"commands": commands}))
}

/// A scratch directory for one case's output files.
fn scratch(area: &str, name: &str) -> Result<String, String> {
    let d = std::env::temp_dir().join(format!("solidcraft-kernel-cases-{}", std::process::id())).join(format!("{area}-{name}"));
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d.to_string_lossy().to_string())
}

/// Chord tolerance of the mesh a `watertight` case is checked on (mm).
const MESH_TOL: f64 = 0.01;

/// A mesh's edges used by one triangle only (open) and by more than two (over-shared). Vertices
/// closer than `weld` (mm) are one vertex: a mesh repeats a vertex for each face, and faces
/// meeting at an edge may place it a rounding error apart.
fn open_edges(positions: &[solidcraft_engine::geom::Vec3], triangles: &[[u32; 3]], weld: f64) -> (usize, usize) {
    use std::collections::HashMap;
    let weld = if weld.is_finite() && weld > 0.0 { weld } else { 1e-4 };
    let cell = |p: &solidcraft_engine::geom::Vec3| [p.x, p.y, p.z].map(|x| (x / weld).floor() as i64);
    // Union-find over positions, joining each vertex with any within `weld` in the 27 cells round it.
    let mut parent: Vec<usize> = (0..positions.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while let Some(&p) = parent.get(i) {
            if p == i {
                break;
            }
            let gp = parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = parent.get_mut(i) {
                *slot = gp;
            }
            i = p;
        }
        i
    }
    let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (i, p) in positions.iter().enumerate() {
        let c = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(near) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else { continue };
                    for &j in near {
                        if positions.get(j).is_some_and(|q| q.dist(*p) <= weld) {
                            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                            if let Some(slot) = parent.get_mut(a) {
                                *slot = b;
                            }
                        }
                    }
                }
            }
        }
        grid.entry(c).or_default().push(i);
    }
    let mut uses: HashMap<(usize, usize), usize> = HashMap::new();
    for t in triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (a, b) = (find(&mut parent, a as usize), find(&mut parent, b as usize));
            if a != b {
                *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
    }
    (uses.values().filter(|&&n| n == 1).count(), uses.values().filter(|&&n| n > 2).count())
}

/// An error that means "this kernel can't do it yet", not a wrong answer.
/// Stations along each seam the `continuity` check samples.
const SEAM_SAMPLES: usize = 65;
/// A seam is tangent (G1), and two seams meeting at a vertex turn there, past this angle (°).
const SEAM_ANGLE_DEG: f64 = 1.0;

/// The `continuity` check's measure of `body`: over the tangent seams lying wholly within
/// `within` of `near`, the worst curvature jump (1/mm) at every station and at the stations at
/// least `clear` (mm, along the seam) from each vertex where two of those seams turn (a corner of
/// the patch's boundary, where its neighbours' own G1 seams end and no surface is G2 to both),
/// with how many seams and stations clear there were. An error where the seams can't be measured.
fn curvature_jumps(body: &solidcraft_engine::kernel::Body, near: [f64; 3], within: f64, clear: f64) -> Result<Value, String> {
    use solidcraft_engine::geom::Vec3;
    let tol =
        solidcraft_engine::kernel::ContinuityTolerances { gap_mm: 1e-3, angle_deg: SEAM_ANGLE_DEG, curvature_per_mm: 0.0, ..Default::default() };
    let report = solidcraft_engine::kernel::seam_diagnostics(body, &[], SEAM_SAMPLES, tol).map_err(|e| e.to_string())?;
    let at = Vec3::new(near[0], near[1], near[2]);
    let seams: Vec<_> = report
        .seams
        .iter()
        .filter(|s| s.normal_angle_deg.value <= SEAM_ANGLE_DEG && s.points.len() >= 2 && s.points.iter().all(|p| p.dist(at) <= within))
        .collect();
    // Each seam's end directions, pointing into it.
    let ends = |s: &solidcraft_engine::kernel::SeamReport| -> Option<[(Vec3, Vec3); 2]> {
        let (p0, p1) = (*s.points.first()?, *s.points.last()?);
        let (q0, q1) = (*s.points.get(1)?, *s.points.get(s.points.len() - 2)?);
        Some([(p0, (q0 - p0).normalized()?), (p1, (q1 - p1).normalized()?)])
    };
    let all_ends: Vec<(Vec3, Vec3)> = seams.iter().filter_map(|s| ends(s)).flatten().collect();
    let weld = 1e-6 * within.max(1.0);
    let cos = SEAM_ANGLE_DEG.to_radians().cos();
    // A vertex turns where another seam leaves it other than straight on.
    let turns = |p: Vec3, d: Vec3| all_ends.iter().any(|(q, e)| q.dist(p) <= weld && (*e - d).len() > 1e-9 && -e.dot(d) < cos);
    let (mut worst, mut worst_clear, mut stations) = (0.0f64, 0.0f64, 0usize);
    for s in &seams {
        let Some(e) = ends(s) else { continue };
        let turning: Vec<Vec3> = e.iter().filter(|(p, d)| turns(*p, *d)).map(|(p, _)| *p).collect();
        // Arc length along the seam to each station, from its start.
        let mut along = vec![0.0f64; s.points.len()];
        for k in 1..s.points.len() {
            along[k] = along[k - 1] + s.points[k].dist(s.points[k - 1]);
        }
        let length = along.last().copied().unwrap_or(0.0);
        for (k, st) in s.stations.iter().enumerate() {
            worst = worst.max(st.curvature_jump_per_mm);
            let a = along.get(k).copied().unwrap_or(0.0);
            let from_turn = turning.iter().map(|p| if p.dist(s.points[0]) <= weld { a } else { length - a }).fold(f64::INFINITY, f64::min);
            if from_turn >= clear {
                worst_clear = worst_clear.max(st.curvature_jump_per_mm);
                stations += 1;
            }
        }
    }
    Ok(json!({"seams": seams.len(), "stations_clear": stations, "worst_per_mm": worst, "worst_clear_per_mm": worst_clear}))
}

/// The seams a `continuity: {order, edges}` check measures on `b`, by edge index, with what
/// each lies between: `blend` (the default), every seam of a face a fillet or chamfer made (a
/// round, a chamfer face or a corner patch) with the faces either side of the edge it replaced,
/// and every seam of a corner patch with the rounds it joins: what the blend is built to meet.
/// Where two rounds meet without a patch, their seam is the input's (tangent only where the
/// rounded edges were) and is not one. `corner`, those of them on a corner patch. A list of
/// points `[[x,y,z], …]`, the edges nearest them, as Continuity Analysis picks them. An error for
/// anything else, or a point near no edge.
fn continuity_seams(b: &solidcraft_engine::doc::ModelBody, edges: &Value) -> Result<Vec<(usize, &'static str)>, String> {
    use solidcraft_engine::doc::naming::BlendSeamKind;
    if let Some(points) = edges.as_array() {
        let picks = points
            .iter()
            .map(|p| v3(p).map(|[x, y, z]| solidcraft_engine::kernel::EdgePick::Point(solidcraft_engine::geom::Vec3::new(x, y, z))))
            .collect::<Option<Vec<_>>>()
            .ok_or("continuity `edges` points must each be [x,y,z]")?;
        let picked = solidcraft_engine::kernel::pick_edges(&b.body, &picks).map_err(|e| e.to_string())?;
        return Ok(picked.into_iter().map(|e| (e, "picked")).collect());
    }
    let all = solidcraft_engine::doc::naming::blend_seams(b).into_iter().filter(|s| s.kind == BlendSeamKind::Contact || s.corner);
    let label =
        |s: &solidcraft_engine::doc::naming::BlendSeam| (s.edge, if s.kind == BlendSeamKind::Blend { "with_blends" } else { "with_contact_faces" });
    match edges.as_str().unwrap_or("blend") {
        "blend" => Ok(all.map(|s| label(&s)).collect()),
        "corner" => Ok(all.filter(|s| s.corner).map(|s| label(&s)).collect()),
        other => Err(format!("continuity `edges` must be \"blend\", \"corner\" or a list of points, not {other:?}")),
    }
}

/// The `continuity: {order, edges}` check of `b`: its `edges` seams measured as Continuity
/// Analysis (`inspect.continuity`) measures them, at its default stations and tolerances, and the
/// lowest order among them with the seam that has it (the worst: of those, the one whose first
/// failing metric is furthest past its tolerance), its faces' names and that metric's worst value.
/// `Ok((report, ok))`, `ok` where every seam meets `want` (`exact`: reaches it and no more).
fn continuity_order(
    b: &solidcraft_engine::doc::ModelBody,
    want: solidcraft_engine::kernel::Continuity,
    edges: &Value,
    exact: bool,
) -> Result<(Value, bool), String> {
    use solidcraft_engine::kernel::{self as k, Continuity};
    let seams = continuity_seams(b, edges)?;
    let selected: Vec<usize> = seams.iter().map(|s| s.0).collect();
    if selected.is_empty() {
        return Ok((json!(format!("{}: no {edges} seams", b.name)), false));
    }
    let tol = k::ContinuityTolerances::default();
    let report = k::seam_diagnostics(&b.body, &selected, k::DEFAULT_SAMPLES, tol).map_err(|e| e.to_string())?;
    let limits = [tol.gap_mm, tol.angle_deg, tol.curvature_per_mm, tol.curvature_change_per_mm2, tol.curvature_change_rate_per_mm3];
    let metric = ["gap_mm", "normal_angle_deg", "curvature_jump_per_mm", "curvature_change_jump_per_mm2", "curvature_change_rate_jump_per_mm3"];
    // The metric a seam of this order fails first (none past G4), and how far past its tolerance.
    let first_failing = |s: &k::SeamReport| -> Option<(usize, f64, k::SeamWorst)> {
        let i = s.order.map_or(0, |o| o.order() + 1);
        let w = [s.gap_mm, s.normal_angle_deg, s.curvature_jump_per_mm, s.curvature_change_jump_per_mm2, s.curvature_change_rate_jump_per_mm3];
        let (worst, limit) = (*w.get(i)?, *limits.get(i)?);
        Some((i, if limit > 0.0 { worst.value / limit } else { f64::INFINITY }, worst))
    };
    let worst = report.seams.iter().min_by(|a, b| {
        a.order.cmp(&b.order).then_with(|| {
            let past = |s: &k::SeamReport| first_failing(s).map_or(0.0, |(_, r, _)| r);
            past(b).total_cmp(&past(a))
        })
    });
    let edge_names = solidcraft_engine::doc::naming::edge_names(b);
    let order_word = |o: Option<Continuity>| o.map_or("none (the faces do not meet)", Continuity::as_str);
    // How many seams reach each order, by what the blend meets there.
    let kind = |edge: usize| seams.iter().find(|s| s.0 == edge).map_or("picked", |s| s.1);
    let mut orders = json!({});
    for s in &report.seams {
        let n = &mut orders[kind(s.edge)][order_word(s.order)];
        *n = json!(n.as_u64().unwrap_or(0) + 1);
    }
    let mut got = json!({
        "body": b.name, "edges": if edges.is_array() { "picked" } else { edges.as_str().unwrap_or("blend") }, "seams": report.seams.len(), "unmeasured": report.unmeasured, "orders": orders,
    });
    if let Some(s) = worst {
        got["order"] = json!(order_word(s.order));
        got["worst_edge"] = json!({"edge": s.edge, "faces": edge_names.get(s.edge), "meets": kind(s.edge)});
        if let Some((i, _, w)) = first_failing(s) {
            got["worst_edge"]["next_order_metric"] =
                json!({"metric": metric.get(i), "value": w.value, "tolerance": limits.get(i), "at": w.at, "edge_fraction": w.edge_fraction});
        }
    }
    let ok = report.unmeasured == 0
        && !report.seams.is_empty()
        && report.seams.iter().all(|s| s.order.is_some_and(|o| if exact { o == want } else { o >= want }));
    Ok((got, ok))
}

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

fn check_case(dir: &str, dumps: Option<&std::path::Path>) -> Value {
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
    // A capability SolidCraft doesn't have yet (no command or parameter for it): recorded as a
    // case so the gap is visible, never run against a stand-in.
    if let Some(missing) = want["missing"].as_str() {
        out["status"] = json!("unsupported");
        out["error"] = json!(missing);
        return out;
    }
    let commands = match scratch(&area, &name).and_then(|o| read_json(&format!("{dir}/recipe.json")).and_then(|r| script(dir, &o, &r))) {
        Ok(s) => s,
        Err(e) => {
            out["status"] = json!("fail");
            out["error"] = json!(format!("recipe.json: {e}"));
            return out;
        }
    };
    if !solidcraft_engine::kernel::smlib_built() {
        out["status"] = json!("unsupported");
        out["error"] = json!("this build has no SMLib: build solidcraft-cli with --features smlib");
        return out;
    }
    // `refused`: only a clean refusal is right; `solid_or_refused`: a clean refusal, or a result
    // graded as usual (where the geometry has no manifold answer, such as bodies touching along a
    // curve only).
    let outcome = want["outcome"].as_str().unwrap_or("solid");
    let refused = outcome == "refused";
    let may_refuse = refused || outcome == "solid_or_refused";
    // The SMLib calls that fail in this case leave their inputs in its own folder.
    if let Some(root) = dumps {
        let folder = root.join(format!("{area}-{name}"));
        if let Err(e) = std::fs::create_dir_all(&folder)
            .map_err(|e| e.to_string())
            .and_then(|()| solidcraft_engine::kernel::smlib_dump_dir(Some(&folder)).map_err(|e| e.to_string()))
        {
            out["status"] = json!("fail");
            out["error"] = json!(format!("--dumps: {}: {e}", folder.display()));
            return out;
        }
    }
    let mut s = solidcraft_engine::Session::default();
    let clock = std::time::Instant::now();
    let ran = s.run_script(&commands);
    // The commands' own time, without process start or grading: the ladder's timing golden.
    out["seconds"] = json!(clock.elapsed().as_secs_f64());
    match (ran, may_refuse) {
        (Err(e), _) if crashed(&e.to_string()) => {
            out["status"] = json!("crash");
            out["error"] = json!(e.to_string());
            return out;
        }
        (Err(e), true) => {
            // The right answer is a clean refusal, for the reason the case gives: a `refused`
            // case names it (`refusal`, a part of the message), so a failure for another reason
            // (an SMLib status) is not taken for it.
            let e = e.to_string();
            out["refused_with"] = json!(e);
            match want["refusal"].as_str() {
                Some(why) if !e.contains(why) => {
                    out["status"] = json!("fail");
                    out["error"] = json!(format!("refused, but not because {why:?}: {e}"));
                }
                None if refused => {
                    out["status"] = json!("fail");
                    out["error"] = json!(format!("refused ({e}), but expected.json gives no `refusal`: the reason it must be refused for"));
                }
                _ => out["status"] = json!("pass"),
            }
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
    let world = match s.world_state() {
        Ok(w) => w,
        Err(e) => {
            out["status"] = json!("fail");
            out["error"] = json!(e.to_string());
            return out;
        }
    };
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
        if want["watertight"].as_bool() == Some(true) {
            let tol = want["tolerance"]["mesh_mm"].as_f64().unwrap_or(MESH_TOL);
            let (open, over) = match b.body.mesh(&solidcraft_engine::kernel::MeshTolerance::chord(tol)) {
                Ok(mesh) => open_edges(&mesh.positions, &mesh.triangles, want["tolerance"]["weld_mm"].as_f64().unwrap_or(1e-4)),
                Err(e) => {
                    c.push("watertight", json!(format!("{}: {e}", b.name)), json!("no open edges"), false);
                    continue;
                }
            };
            c.push(
                "watertight",
                json!(format!("{}: {open} open, {over} over-shared mesh edges", b.name)),
                json!("no open edges"),
                open == 0 && over == 0,
            );
        }
        // The order every blend seam reaches, as Continuity Analysis measures it.
        if let Some(order) = want["continuity"].get("order").filter(|o| !o.is_null()) {
            let edges = &want["continuity"]["edges"];
            let exact = want["continuity"]["exact"].as_bool().unwrap_or(false);
            let which = if edges.is_array() { "picked" } else { edges.as_str().unwrap_or("blend") };
            let need = format!("{} {} at every {which} seam", if exact { "exactly" } else { ">=" }, order.as_str().unwrap_or("?"));
            match order.as_str().and_then(solidcraft_engine::kernel::Continuity::parse) {
                Some(o) => match continuity_order(b, o, edges, exact) {
                    Ok((got, ok)) => c.push("continuity_order", got, json!(need), ok),
                    Err(e) => c.push("continuity_order", json!(format!("{}: {e}", b.name)), json!(need), false),
                },
                None => c.push("continuity_order", json!(format!("continuity `order` {order} is not g0…g4")), json!("an order"), false),
            }
        }
        // Curvature continuity across the seams round a point: the worst jump clear of the
        // seams' turns must be within the case's bound.
        if let Some(cont) = want.get("continuity").filter(|c| !c.is_null() && c.get("order").is_none()) {
            let (near, within, clear, bound) =
                (v3(&cont["near"]), cont["within_mm"].as_f64(), cont["clear_mm"].as_f64(), cont["curvature_jump_per_mm"].as_f64());
            match (near, within, clear, bound) {
                (Some(near), Some(within), Some(clear), Some(bound)) => match curvature_jumps(&b.body, near, within, clear) {
                    Ok(got) => {
                        let ok = got["seams"].as_u64().unwrap_or(0) > 0
                            && got["stations_clear"].as_u64().unwrap_or(0) > 0
                            && got["worst_clear_per_mm"].as_f64().is_some_and(|w| w <= bound);
                        c.push("curvature_jump", got, json!({"worst_clear_per_mm": format!("<= {bound}")}), ok);
                    }
                    Err(e) => c.push("curvature_jump", json!(format!("{}: {e}", b.name)), json!("measurable"), false),
                },
                _ => c.push("curvature_jump", json!("continuity needs near, within_mm, clear_mm and curvature_jump_per_mm"), json!("a bound"), false),
            }
        }
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
    // What the kernel made, graded or not: the ladder's golden records it per kernel.
    out["measured"] = json!({
        "body_count": got.body_count, "faces": got.faces, "edges": edges, "vertices": vertices,
        "volume_mm3": got.volume, "area_mm2": got.area,
    });
    let has = |k: &str| want.get(k).is_some_and(|v| !v.is_null());
    let rel = want["tolerance"]["rel"].as_f64().unwrap_or(REL_TOL);
    let diag = |b: ([f64; 3], [f64; 3])| ((b.1[0] - b.0[0]).powi(2) + (b.1[1] - b.0[1]).powi(2) + (b.1[2] - b.0[2]).powi(2)).sqrt();
    let want_bbox = v3(&want["bbox"]["min"]).zip(v3(&want["bbox"]["max"]));
    let abs = want["tolerance"]["abs_mm"].as_f64().unwrap_or_else(|| ABS_FRAC * want_bbox.map(diag).unwrap_or_else(|| diag(got.bbox)).max(1.0));
    // A count is exact, or an inclusive range `[min, max]` where the geometry only bounds it.
    let count = |c: &mut Checks, k: &str, g: u64| {
        if has(k) {
            let ok = match want[k].as_array() {
                Some(r) => r.first().and_then(Value::as_u64).is_some_and(|lo| g >= lo) && r.get(1).and_then(Value::as_u64).is_some_and(|hi| g <= hi),
                None => want[k].as_u64() == Some(g),
            };
            c.push(k, json!(g), want[k].clone(), ok);
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
    // `--dumps=DIR`: each case's failing SMLib calls leave their inputs in DIR/<area>-<case>.
    let dumps = args.iter().find_map(|a| a.strip_prefix("--dumps=")).map(std::path::PathBuf::from);
    if dumps.as_ref().is_some_and(|d| d.as_os_str().is_empty()) {
        return Err("kernel-cases: --dumps= needs a folder".into());
    }
    let results: Vec<Value> = dirs.iter().map(|d| check_case(d, dumps.as_deref())).collect();
    if dumps.is_some() {
        solidcraft_engine::kernel::smlib_dump_dir(None).map_err(|e| format!("kernel-cases: --dumps: {e}"))?;
    }
    report(args, &results);
    let bad = results.iter().filter(|r| matches!(r["status"].as_str(), Some("fail" | "crash"))).count();
    if bad > 0 { Err(format!("{bad} kernel case(s) failed")) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case_dir(root: &std::path::Path, name: &str, recipe: &Value) -> String {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("recipe.json"), recipe.to_string()).unwrap();
        d.to_string_lossy().to_string()
    }

    #[test]
    fn base_commands_run_first_through_the_chain() {
        let root = std::env::temp_dir().join(format!("kernel-cases-base-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        case_dir(&root, "a", &json!({"commands": [{"command": "a", "params": {"file": "$OUT/a.step"}}]}));
        case_dir(&root, "b", &json!({"base": "../a", "commands": [{"command": "b"}]}));
        let c = case_dir(&root, "c", &json!({"base": "../b", "commands": [{"command": "c", "params": {"file": "$CASE/x"}}]}));
        let r = read_json(&format!("{c}/recipe.json")).unwrap();
        let s = script(&c, "/scratch", &r).unwrap();
        let ids: Vec<&str> = s["commands"].as_array().unwrap().iter().map(|c| c["command"].as_str().unwrap()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(s["commands"][2]["params"]["file"], json!(format!("{c}/x")));
        // `$OUT` is the running case's scratch directory, in its bases too.
        assert_eq!(s["commands"][0]["params"]["file"], json!("/scratch/a.step"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_base_cycle_is_an_error_not_a_hang() {
        let root = std::env::temp_dir().join(format!("kernel-cases-cycle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let a = case_dir(&root, "a", &json!({"base": "../a", "commands": []}));
        let r = read_json(&format!("{a}/recipe.json")).unwrap();
        assert!(script(&a, "/scratch", &r).unwrap_err().contains("deeper than"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
