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

/// Where `got` differs from `want`, a part of it: every key of a `want` object in `got` and
/// matching, an array element by element and as long, a whole number exactly, any other number
/// within `rel` of it (relative, at least 1 absolute), anything else equal. `None` where it
/// matches; else the path to the first difference with both values.
fn json_mismatch(got: &Value, want: &Value, rel: f64, path: &str) -> Option<String> {
    let differ = || Some(format!("{path}: got {got}, want {want}"));
    match (got, want) {
        (_, Value::Object(w)) => {
            let Value::Object(g) = got else { return differ() };
            w.iter().find_map(|(k, wv)| match g.get(k) {
                Some(gv) => json_mismatch(gv, wv, rel, &format!("{path}.{k}")),
                None => Some(format!("{path}.{k}: missing, want {wv}")),
            })
        }
        (_, Value::Array(w)) => {
            let Value::Array(g) = got else { return differ() };
            if g.len() != w.len() {
                return Some(format!("{path}: {} items, want {}", g.len(), w.len()));
            }
            g.iter().zip(w).enumerate().find_map(|(i, (gv, wv))| json_mismatch(gv, wv, rel, &format!("{path}[{i}]")))
        }
        (_, Value::Number(w)) if w.is_i64() || w.is_u64() => (got.as_f64() != w.as_f64()).then(differ).flatten(),
        (_, Value::Number(w)) => {
            let (g, w) = (got.as_f64(), w.as_f64().unwrap_or(f64::NAN));
            (!g.is_some_and(|g| g.is_finite() && (g - w).abs() <= rel * w.abs().max(1.0))).then(differ).flatten()
        }
        _ => (got != want).then(differ).flatten(),
    }
}

/// The rows of a CSV text (RFC 4180: quoted fields may hold commas, quotes and line breaks) as
/// objects keyed by the header row; a field that reads as a number is one.
fn csv_rows(text: &str) -> Vec<Value> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let (mut row, mut field, mut quoted, mut chars) = (Vec::new(), String::new(), false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            ('"', _) => quoted = !quoted,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        records.push(row);
    }
    let Some((head, rows)) = records.split_first() else { return Vec::new() };
    rows.iter()
        .map(|r| {
            let cell = |v: &String| v.parse::<f64>().ok().filter(|x| x.is_finite()).map_or_else(|| json!(v), |x| json!(x));
            Value::Object(head.iter().cloned().zip(r.iter().map(cell)).collect())
        })
        .collect()
}

/// Per layer of a DXF text's ENTITIES section, how many of each entity it has and the total
/// length of its curves (`length_mm`: lines, arcs, circles, ellipses and polylines; a spline makes
/// the layer's length null, as not measured): `{layer: {"LINE": n, …, "length_mm": l}}`.
fn dxf_layers(text: &str) -> Value {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|p| Some((*p.first()?, *p.get(1)?))).collect();
    let start = pairs.windows(2).position(|w| w[0] == ("0", "SECTION") && w[1] == ("2", "ENTITIES")).map_or(pairs.len(), |i| i + 2);
    let mut layers = serde_json::Map::new();
    let mut i = start;
    while let Some(&(code, kind)) = pairs.get(i) {
        if code != "0" || kind == "ENDSEC" {
            break;
        }
        let end = pairs.get(i + 1..).and_then(|r| r.iter().position(|p| p.0 == "0")).map_or(pairs.len(), |k| i + 1 + k);
        let group = pairs.get(i + 1..end).unwrap_or(&[]);
        let num = |c: &str| group.iter().find(|p| p.0 == c).and_then(|p| p.1.parse::<f64>().ok()).unwrap_or(0.0);
        let all = |c: &str| group.iter().filter(|p| p.0 == c).filter_map(|p| p.1.parse::<f64>().ok()).collect::<Vec<_>>();
        let layer = group.iter().find(|p| p.0 == "8").map_or("0", |p| p.1);
        let length = match kind {
            "LINE" => Some((num("11") - num("10")).hypot(num("21") - num("20"))),
            "CIRCLE" => Some(std::f64::consts::TAU * num("40")),
            "ARC" => Some(num("40") * (num("51") - num("50")).rem_euclid(360.0).to_radians()),
            "LWPOLYLINE" => {
                let mut pts: Vec<(f64, f64)> = all("10").into_iter().zip(all("20")).collect();
                if (num("70") as i64) & 1 == 1
                    && let Some(&p) = pts.first()
                {
                    pts.push(p);
                }
                Some(pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum())
            }
            "ELLIPSE" => {
                // Arc length of c + cos t·major + sin t·ratio·minor, t from 41 to 42, by Simpson's rule.
                let (a, b) = (num("11").hypot(num("21")), num("11").hypot(num("21")) * num("40"));
                let (t0, t1) = (num("41"), num("42"));
                let sweep = if t1 > t0 { t1 - t0 } else { t1 - t0 + std::f64::consts::TAU };
                let n = 4096;
                let speed = |t: f64| (a * t.sin()).hypot(b * t.cos());
                let h = sweep / n as f64;
                let sum: f64 = (0..=n).map(|k| speed(t0 + k as f64 * h) * if k == 0 || k == n { 1.0 } else if k % 2 == 1 { 4.0 } else { 2.0 }).sum();
                Some(sum * h / 3.0)
            }
            _ => None,
        };
        let entry = layers.entry(layer.to_string()).or_insert_with(|| json!({"length_mm": 0.0}));
        entry[kind] = json!(entry[kind].as_u64().unwrap_or(0) + 1);
        entry["length_mm"] = match (entry["length_mm"].as_f64(), length) {
            (Some(l), Some(more)) => json!(l + more),
            _ => Value::Null,
        };
        i = end;
    }
    Value::Object(layers)
}

/// The `files` check of the files a case wrote to `$OUT` (`out`): each `{path, json | csv | dxf}`
/// read and matched as [`json_mismatch`] does: `json` against the file's JSON, `csv` against its
/// rows ([`csv_rows`]), `dxf` against its layers ([`dxf_layers`]), a count there exact or an
/// inclusive range `[min, max]`. And the `results` check of the commands' own answers: each
/// `{step, json}`, the answer of the `step`th command (from 1) matched against `json`.
fn output_checks(c: &mut Checks, want: &Value, out: &str, results: &[Value], rel: f64) {
    for f in want["files"].as_array().into_iter().flatten() {
        let path = f["path"].as_str().unwrap_or_default();
        let what = format!("file {path}");
        let text = match std::fs::read_to_string(std::path::Path::new(out).join(path)) {
            Ok(t) => t,
            Err(e) => {
                c.push(&what, json!(e.to_string()), json!("written"), false);
                continue;
            }
        };
        let (got, wanted) = if let Some(w) = f.get("json") {
            (serde_json::from_str::<Value>(&text).unwrap_or_else(|e| json!(format!("not JSON: {e}"))), w.clone())
        } else if let Some(w) = f.get("csv") {
            (Value::Array(csv_rows(&text)), w.clone())
        } else if let Some(w) = f.get("dxf") {
            let layers = dxf_layers(&text);
            // A count given as a range is in it: compare the count itself with the range's bounds.
            let mut w = w.clone();
            let mut ranged = Vec::new();
            for (layer, kinds) in w.as_object_mut().into_iter().flatten() {
                for (kind, n) in kinds.as_object_mut().into_iter().flatten() {
                    if let Some([lo, hi]) = n.as_array().and_then(|r| <[Value; 2]>::try_from(r.clone()).ok()) {
                        let g = layers[layer.as_str()][kind.as_str()].as_u64().unwrap_or(0);
                        ranged.push((format!("{layer}.{kind}"), g, lo.clone(), hi.clone()));
                        *n = json!(g);
                    }
                }
            }
            for (k, g, lo, hi) in ranged {
                let ok = lo.as_u64().is_some_and(|lo| g >= lo) && hi.as_u64().is_some_and(|hi| g <= hi);
                c.push(&format!("{what} {k}"), json!(g), json!([lo, hi]), ok);
            }
            // An entity the case names but the layer lacks counts 0.
            let mut got = layers;
            for (layer, kinds) in w.as_object().into_iter().flatten() {
                for kind in kinds.as_object().into_iter().flatten().map(|(k, _)| k) {
                    if got[layer.as_str()].get(kind.as_str()).is_none() && kind != "length_mm" {
                        got[layer.as_str()][kind.as_str()] = json!(0);
                    }
                }
            }
            (got, w)
        } else {
            c.push(&what, json!(f), json!("json, csv or dxf"), false);
            continue;
        };
        let miss = json_mismatch(&got, &wanted, rel, "");
        c.push(&what, json!(miss.clone().unwrap_or_else(|| "matches".into())), wanted, miss.is_none());
    }
    for r in want["results"].as_array().into_iter().flatten() {
        let step = r["step"].as_u64().unwrap_or(0);
        let what = format!("result of step {step}");
        let got = step.checked_sub(1).and_then(|i| results.get(i as usize));
        let miss = match got {
            Some(g) => json_mismatch(g, &r["json"], rel, ""),
            None => Some(format!("no step {step} of {}", results.len())),
        };
        c.push(&what, json!(miss.clone().unwrap_or_else(|| "matches".into())), r["json"].clone(), miss.is_none());
    }
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
    let scratch_dir = match scratch(&area, &name) {
        Ok(o) => o,
        Err(e) => {
            out["status"] = json!("fail");
            out["error"] = json!(e);
            return out;
        }
    };
    let commands = match read_json(&format!("{dir}/recipe.json")).and_then(|r| script(dir, &scratch_dir, &r)) {
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
    let answers = match (ran, may_refuse) {
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
        (Ok(r), _) => r,
    };

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
    output_checks(&mut c, &want, &scratch_dir, &answers, rel);
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
    use std::f64::consts::PI;

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
    fn json_parts_match_within_tolerance_and_whole_numbers_exactly() {
        let got = json!({"bends": [{"angle_deg": 90.00001, "direction": "up", "count": 3}], "extra": 1});
        assert_eq!(json_mismatch(&got, &json!({"bends": [{"angle_deg": 90.0, "direction": "up"}]}), 1e-3, ""), None);
        assert!(json_mismatch(&got, &json!({"bends": [{"count": 4}]}), 1e-3, "").is_some_and(|m| m.contains(".bends[0].count")));
        assert!(json_mismatch(&got, &json!({"bends": []}), 1e-3, "").is_some_and(|m| m.contains("1 items, want 0")));
        assert!(json_mismatch(&got, &json!({"missing": 1}), 1e-3, "").is_some_and(|m| m.contains("missing")));
        assert!(json_mismatch(&got, &json!({"bends": [{"direction": "down"}]}), 1e-3, "").is_some());
    }

    #[test]
    fn csv_rows_are_keyed_by_the_header_and_read_quoted_fields() {
        let rows = csv_rows("body,angle_deg,note\r\n\"a, \"\"b\"\"\",90.5,x\r\n\"c\",180,\r\n");
        assert_eq!(rows, vec![json!({"body": "a, \"b\"", "angle_deg": 90.5, "note": "x"}), json!({"body": "c", "angle_deg": 180.0, "note": ""})]);
    }

    #[test]
    fn dxf_layers_count_entities_and_measure_their_length() {
        let pair = |c: &str, v: &str| format!("{c}\n{v}\n");
        let mut t = String::new();
        // A circle in the BLOCKS section is not an entity of the drawing.
        for (c, v) in [("0", "SECTION"), ("2", "BLOCKS"), ("0", "CIRCLE"), ("8", "CUT"), ("40", "9"), ("0", "ENDSEC")] {
            t += &pair(c, v);
        }
        t += &pair("0", "SECTION");
        t += &pair("2", "ENTITIES");
        for e in [
            vec![("0", "LINE"), ("8", "OUTLINE"), ("10", "0"), ("20", "0"), ("11", "3"), ("21", "4")],
            vec![("0", "ARC"), ("8", "OUTLINE"), ("10", "0"), ("20", "0"), ("40", "2"), ("50", "270"), ("51", "0")],
            vec![("0", "CIRCLE"), ("8", "CUT"), ("10", "5"), ("20", "5"), ("40", "1")],
            vec![("0", "LWPOLYLINE"), ("8", "BEND_UP"), ("90", "2"), ("70", "128"), ("10", "0"), ("20", "1"), ("10", "10"), ("20", "1")],
            vec![("0", "ELLIPSE"), ("8", "CUT"), ("10", "0"), ("20", "0"), ("11", "10"), ("21", "0"), ("40", "1"), ("41", "0"), ("42", "6.283185307179586")],
        ] {
            for (c, v) in e {
                t += &pair(c, v);
            }
        }
        t += &pair("0", "ENDSEC");
        t += &pair("0", "EOF");
        let l = dxf_layers(&t);
        assert_eq!(l["OUTLINE"]["LINE"], 1);
        assert_eq!(l["OUTLINE"]["ARC"], 1);
        assert!((l["OUTLINE"]["length_mm"].as_f64().unwrap() - (5.0 + PI)).abs() < 1e-9, "{l}");
        assert_eq!(l["CUT"]["CIRCLE"], 1);
        assert_eq!(l["CUT"]["ELLIPSE"], 1);
        // A circle of radius 1 and an ellipse of ratio 1 (a circle of radius 10).
        assert!((l["CUT"]["length_mm"].as_f64().unwrap() - 22.0 * PI).abs() < 1e-6, "{l}");
        assert!((l["BEND_UP"]["length_mm"].as_f64().unwrap() - 10.0).abs() < 1e-9, "{l}");
    }

    #[test]
    fn output_checks_read_the_files_and_answers_a_case_names() {
        let out = std::env::temp_dir().join(format!("kernel-cases-files-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("t.json"), r#"{"bends": [{"angle_deg": 90}]}"#).unwrap();
        std::fs::write(out.join("t.csv"), "bend,angle_deg\r\n1,90\r\n").unwrap();
        std::fs::write(out.join("t.dxf"), "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nOUTLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDSEC\n").unwrap();
        let dir = out.to_string_lossy().to_string();
        let run = |want: Value| {
            let mut c = Checks { list: Vec::new(), pass: true };
            output_checks(&mut c, &want, &dir, &[json!({"count": 3})], 1e-3);
            c.pass
        };
        assert!(run(json!({"files": [{"path": "t.json", "json": {"bends": [{"angle_deg": 90.0}]}}]})));
        assert!(run(json!({"files": [{"path": "t.csv", "csv": [{"bend": 1, "angle_deg": 90.0}]}]})));
        assert!(run(json!({"files": [{"path": "t.dxf", "dxf": {"OUTLINE": {"LINE": [1, 8], "ARC": 0, "length_mm": 1.0}}}]})));
        assert!(!run(json!({"files": [{"path": "t.dxf", "dxf": {"OUTLINE": {"LINE": [2, 8]}}}]})));
        assert!(!run(json!({"files": [{"path": "t.dxf", "dxf": {"CUT": {"CIRCLE": 1}}}]})));
        assert!(!run(json!({"files": [{"path": "none.json", "json": {}}]})));
        assert!(run(json!({"results": [{"step": 1, "json": {"count": 3}}]})));
        assert!(!run(json!({"results": [{"step": 2, "json": {}}]})));
        let _ = std::fs::remove_dir_all(&out);
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
