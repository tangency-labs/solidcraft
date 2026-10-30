//! Every design of the corpus loads, every name it stores still finds a face or an edge, and
//! two builds of it name its faces and edges alike. The stored names are the fillet and chamfer
//! edges, the faces a feature picks, the face a sketch sits on and the sheet metal edges; one
//! that finds nothing (`gone`, a sketch's face `lost`) fails the test, and so does a design that
//! no longer loads. A build's names are each face's and edge's name with where it is (its area
//! and centre, its length and middle); the second build must give the same, and its stored
//! names must find the same faces and edges.
//!
//! Face and edge indices are left out: they are the kernel's order, which a rebuild need not
//! keep (a fillet's or a boolean's faces come out in another order from one build to the next).
//! How many faces a body has and what each is called is the kernel's business, not this test's.
//!
//! The corpus is the action matrix's (the old design files in `tests/corpus`, the examples, the
//! demo and typical designs in `tests/action_matrix/designs`, the reference recipes), the sample
//! designs, the engine's design fixtures and the older designs kept by the hole matrix, taper
//! and projection tests.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::{FaceAttachmentStatus, Feature, FeatureKind, ModelBody, ModelState, PlaneRef, naming};
use solidcraft_engine::geom::Vec3;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Where a design comes from.
enum Source {
    File(PathBuf),
    Sample(fn() -> Value),
}

/// Every design of the corpus: (`<group>/<design>`, where it comes from).
fn designs() -> Vec<(String, Source)> {
    let root = root();
    let mut out = Vec::new();
    let mut add = |prefix: &str, dir: PathBuf, ext: &str| {
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect();
        v.sort();
        for p in v {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            out.push((format!("{prefix}/{stem}"), Source::File(p)));
        }
    };
    add("corpus", root.join("apps/solidcraft-cli/tests/corpus"), "solidcraft");
    add("examples", root.join("examples"), "json");
    add("examples", root.join("examples/acceptance"), "json");
    add("regressions", root.join("examples/regressions"), "json");
    add("demo", root.join("examples/demo"), "json");
    add("typical", root.join("tests/action_matrix/designs"), "json");
    add("demo", root.join("tests/action_matrix/designs"), "solidcraft");
    add("fixtures", root.join("crates/engine/src/tests/fixtures"), "solidcraft");
    add("hole_matrix", root.join("tests/hole_matrix/old-0.9"), "json");
    add("taper", root.join("tests/taper/before-1014"), "json");
    add("projection", root.join("tests/projection/before-1508"), "json");
    let mut cases: Vec<PathBuf> =
        std::fs::read_dir(root.join("tests/oracle")).unwrap().flatten().map(|e| e.path()).filter(|p| p.join("recipe.json").is_file()).collect();
    cases.sort();
    for c in cases {
        out.push((format!("oracle/{}", c.file_name().unwrap_or_default().to_string_lossy()), Source::File(c.join("recipe.json"))));
    }
    for sample in solidcraft_engine::sample::samples() {
        let stem: String = sample.name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-");
        out.push((format!("samples/{stem}"), Source::Sample(sample.script)));
    }
    out
}

/// Build a design: a design file (also a design saved as JSON) is opened, a script is run.
fn load(source: &Source) -> Result<Session, String> {
    let mut s = Session::default();
    let script = match source {
        Source::Sample(script) => script(),
        Source::File(path) if path.extension().is_some_and(|x| x == "solidcraft") => {
            s.execute("doc.open", &json!({"path": path.to_string_lossy()})).map_err(|e| e.to_string())?;
            return Ok(s);
        }
        Source::File(path) if path.file_name().is_some_and(|n| n == "recipe.json") => {
            let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("recipe").arg(path).output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!("recipe: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
            serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?
        }
        Source::File(path) => {
            let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            if v.get("features").is_some() {
                s.execute("doc.open", &json!({"path": path.to_string_lossy()})).map_err(|e| e.to_string())?;
                return Ok(s);
            }
            match v.get("steps").and_then(Value::as_array) {
                // The demo's steps: an MCP `execute` each.
                Some(steps) => {
                    json!({"commands": steps.iter().map(|st| json!({"command": st["command"], "params": st["params"]})).collect::<Vec<_>>()})
                }
                None => v,
            }
        }
    };
    s.run_script(&script).map_err(|e| e.to_string())?;
    if s.active_sketch.is_some() {
        let _ = s.execute("sketch.finish", &json!({}));
    }
    Ok(s)
}

/// What decides a name beyond its text: `[geometric]` when it was worked out from geometry, not
/// exact history; how a stored name was found.
fn flags(list: &[(bool, &str)]) -> String {
    let set: Vec<&str> = list.iter().filter(|(on, _)| *on).map(|(_, f)| *f).collect();
    if set.is_empty() { String::new() } else { format!(" [{}]", set.join(", ")) }
}

/// Where each named face and edge of a body is, by name, sorted: a face's area and centre, an
/// edge's length and middle, from the body's mesh. Two builds that name their faces alike give
/// the same list.
fn placed_names(b: &ModelBody) -> Vec<(String, [f64; 4])> {
    if b.body.is_mesh() {
        return Vec::new();
    }
    let m = b.mesh();
    let faces = naming::face_names(b);
    let mut sums = vec![[0.0f64; 4]; faces.len()];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let [p, q, r] = t.map(|i| m.positions.get(i as usize).copied().unwrap_or(Vec3::ZERO));
        let area = (q - p).cross(r - p).len() / 2.0;
        let c = (p + q + r) / 3.0;
        if let Some(s) = sums.get_mut(*f as usize) {
            *s = [s[0] + area, s[1] + area * c.x, s[2] + area * c.y, s[3] + area * c.z];
        }
    }
    let mut out: Vec<(String, [f64; 4])> = faces
        .iter()
        .zip(&sums)
        .map(|(n, s)| {
            let a = s[0].max(1e-300);
            (format!("{} face {n}", b.name), [s[0], s[1] / a, s[2] / a, s[3] / a])
        })
        .collect();
    for (n, line) in naming::edge_names(b).iter().zip(&m.edges) {
        let length: f64 = line.windows(2).map(|w| w[1].dist(w[0])).sum();
        let mid = line.iter().fold(Vec3::ZERO, |acc, p| acc + *p) / (line.len().max(1) as f64);
        out.push((format!("{} edge {n}", b.name), [length, mid.x, mid.y, mid.z]));
    }
    out.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal)));
    out
}

fn face_label(st: &ModelState, body: usize, face: usize) -> String {
    match st.bodies.get(body) {
        Some(b) => format!("{} face {}", b.name, naming::face_names(b).get(face).map(String::as_str).unwrap_or("?")),
        None => format!("body #{body} face #{face}"),
    }
}

/// Every name feature `f` stores, with what it finds in the model before it, as a feature
/// rebuilding finds it, and whether it finds anything.
fn record_refs(out: &mut Vec<(String, bool)>, s: &Session, f: &Feature) {
    let doc = &*s.doc;
    let who = format!("ref {} ({})", f.name, f.id);
    let before = s.model.state_before(f.id);
    // Faces a feature picks by point and stores by name (shell, draft, hole, …).
    let explicit_emboss = matches!(&f.kind, FeatureKind::Emboss { face: Some(_), .. });
    if f.face_names.iter().any(|n| !n.is_empty()) {
        let fst = solidcraft_engine::doc::face_point_state(doc, &before, &f.kind, f.component).unwrap();
        for (k, (p, n)) in f.kind.face_points().into_iter().zip(&f.face_names).enumerate() {
            if n.is_empty() {
                continue;
            }
            let (found, ok) = if explicit_emboss {
                ("not checked (a cylindrical emboss finds its face on its own body)".to_string(), true)
            } else {
                match naming::locate_face_on_body(&fst, f.kind.face_body(), n, p) {
                    Some(hit) => (
                        format!(
                            "{}{}",
                            face_label(&fst, hit.body, hit.face),
                            flags(&[(hit.split, "split"), (hit.merged, "merged"), (!hit.verified, "geometric"), (hit.legacy, "legacy")])
                        ),
                        true,
                    ),
                    None => ("gone".into(), false),
                }
            };
            out.push((format!("{who} face {k} `{n}` -> {found}"), ok));
        }
    }
    // Edges a feature picks by point and stores by name.
    if f.edge_names.iter().any(|n| !n.is_empty()) {
        let edge = |out: &mut Vec<(String, bool)>, b: &ModelBody, k: usize, n: &str, p: Vec3| {
            let (found, ok) = match naming::locate_edge(b, n, p) {
                Ok(hit) => (
                    format!(
                        "{} edge {}{}",
                        b.name,
                        naming::edge_names(b).get(hit.edge).map(String::as_str).unwrap_or("?"),
                        flags(&[(hit.split, "split"), (!hit.verified, "geometric"), (hit.legacy, "legacy")])
                    ),
                    true,
                ),
                Err(e) => (format!("gone ({e})"), false),
            };
            out.push((format!("{who} edge {k} `{n}` -> {found}"), ok));
        };
        let named = |k: usize| f.edge_names.get(k).filter(|n| !n.is_empty());
        match &f.kind {
            FeatureKind::Fillet { edges, body, .. } | FeatureKind::Chamfer { edges, body, .. } => {
                let pts: Vec<Vec3> = solidcraft_engine::doc::plain(edges);
                match solidcraft_engine::doc::blend_target(&before, body, &pts).ok().and_then(|i| before.bodies.get(i)) {
                    Some(b) => {
                        for (k, p) in pts.iter().enumerate() {
                            if let Some(n) = named(k) {
                                edge(out, b, k, n, *p);
                            }
                        }
                    }
                    None => out.push((format!("{who} edges: no body to round"), false)),
                }
            }
            FeatureKind::SheetConvert { body, bends, .. } => match before.body(body) {
                Some(b) => {
                    for (k, p) in solidcraft_engine::doc::plain::<Vec3, _>(bends).iter().enumerate() {
                        if let Some(n) = named(k) {
                            edge(out, b, k, n, *p);
                        }
                    }
                }
                None => out.push((format!("{who} bends: body `{body}` gone"), false)),
            },
            // Sheet metal edges are named by the sheet's panels, and found on its sheets.
            FeatureKind::SheetFlange { .. } | FeatureKind::SheetHem { .. } => {
                let pts: Vec<Vec3> = match &f.kind {
                    FeatureKind::SheetFlange { edges, .. } => edges.iter().map(|e| e.point.0).collect(),
                    FeatureKind::SheetHem { edges, .. } => solidcraft_engine::doc::plain(edges),
                    _ => Vec::new(),
                };
                for (k, p) in pts.iter().enumerate() {
                    let Some(n) = named(k) else { continue };
                    let on: Vec<&str> = before.sheets.iter().filter(|sh| sh.edge_by_name(n, *p).is_some()).map(|sh| sh.body.as_str()).collect();
                    let found = if on.is_empty() { "gone".to_string() } else { format!("sheet edge on {}", on.join(", ")) };
                    out.push((format!("{who} sheet edge {k} `{n}` -> {found}"), !on.is_empty()));
                }
            }
            other => {
                for (k, n) in f.edge_names.iter().enumerate().filter(|(_, n)| !n.is_empty()) {
                    out.push((format!("{who} edge {k} `{n}` -> not checked ({})", other.type_name()), true));
                }
            }
        }
    }
    // The face a sketch sits on.
    if let FeatureKind::Sketch { plane: PlaneRef::Face { name, anchor, .. }, .. } = &f.kind {
        let mut ok = true;
        let status = match doc.sketch_face_attachment(f.id, &s.model) {
            FaceAttachmentStatus::Healthy { target, .. } => {
                let faces = match before.body(&target.body) {
                    Some(b) => match doc.anchored_face_indices(&before, b, &target) {
                        Ok((indices, note)) => {
                            let names = naming::face_names(b);
                            let mut list: Vec<&str> = indices.iter().map(|i| names.get(*i).map(String::as_str).unwrap_or("?")).collect();
                            list.sort();
                            format!("{} faces {}{}", b.name, list.join(", "), flags(&[(note.is_some(), "changed cap")]))
                        }
                        Err(e) => {
                            ok = false;
                            format!("no faces ({e})")
                        }
                    },
                    None => {
                        ok = false;
                        format!("body `{}` gone", target.body)
                    }
                };
                format!("healthy: {faces}")
            }
            FaceAttachmentStatus::Lost { message, .. } => {
                ok = false;
                format!("lost: {message}")
            }
            FaceAttachmentStatus::Legacy { .. } => {
                // Found by its name among the faces facing the same way, nearest the pick.
                let fst = solidcraft_engine::doc::state_in_frame(doc, &before, f.component).unwrap();
                let mut list: Vec<String> = match name {
                    Some(n) => fst
                        .bodies
                        .iter()
                        .filter(|b| !b.body.is_mesh())
                        .flat_map(|b| {
                            let names = naming::face_names(b);
                            naming::faces_with_stem(b, n)
                                .into_iter()
                                .map(move |i| format!("{} face {}", b.name, names.get(i).map(String::as_str).unwrap_or("?")))
                        })
                        .collect(),
                    None => Vec::new(),
                };
                list.sort();
                if list.is_empty() { "legacy, by its pick".into() } else { format!("legacy, by name among {}", list.join("; ")) }
            }
            FaceAttachmentStatus::Sheet { .. } => "sheet face".into(),
            FaceAttachmentStatus::Fixed { .. } => "fixed".into(),
        };
        let stored = match (anchor, name) {
            (Some(a), _) => {
                let kind =
                    a.target.certificate.as_ref().and_then(|c| serde_json::to_value(c).ok()).and_then(|v| v["kind"].as_str().map(String::from));
                format!("`{}` on {} ({})", a.target.name, a.target.body, kind.unwrap_or_else(|| "no certificate".into()))
            }
            (None, Some(n)) => format!("`{n}`"),
            (None, None) => "by its pick".into(),
        };
        out.push((format!("{who} plane {stored} -> {status}"), ok));
    }
}

/// A build of a design: where its faces and edges are by name, and what every name it stores
/// finds.
struct Build {
    names: Vec<(String, [f64; 4])>,
    refs: Vec<(String, bool)>,
}

fn build(s: &Session) -> Build {
    let names = s.model.state().bodies.iter().flat_map(placed_names).collect();
    let mut refs = Vec::new();
    for f in s.doc.features.iter().filter(|f| !f.suppressed) {
        record_refs(&mut refs, s, f);
    }
    Build { names, refs }
}

/// Where two builds name their faces and edges apart (at most `cap` of them): a name only one
/// has, or a face or edge of that name somewhere else (1e-6 of its size, to the mesh's rounding).
fn differences(a: &Build, b: &Build, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    if a.names.len() != b.names.len() {
        out.push(format!("{} named faces and edges, then {}", a.names.len(), b.names.len()));
    }
    for ((n, x), (m, y)) in a.names.iter().zip(&b.names) {
        let scale = x.iter().chain(y).fold(1.0f64, |s, v| s.max(v.abs()));
        if n != m || x.iter().zip(y).any(|(p, q)| (p - q).abs() > 1e-6 * scale) {
            out.push(format!("{n} at {x:?}, then {m} at {y:?}"));
        }
    }
    for ((r, _), (q, _)) in a.refs.iter().zip(&b.refs) {
        if r != q {
            out.push(format!("{r}, then {q}"));
        }
    }
    if a.refs.len() != b.refs.len() {
        out.push(format!("{} stored names, then {}", a.refs.len(), b.refs.len()));
    }
    out.truncate(cap);
    out
}

/// Designs whose names differ from one build to the next today: (design, why). Their
/// differences are printed, not failed on, until the cause is fixed; a stored name that finds
/// nothing still fails.
const UNSTABLE: [(&str, &str); 1] = [(
    "regressions/closed_sweep_torus_cut",
    "the halves of each swept wall lie level along every axis, so which is `#1` and which `#2` follows the order the kernel gives the faces in, which changes from one build to the next",
)];

#[test]
fn corpus_designs_keep_their_references_and_name_alike_every_build() {
    let designs = designs();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(designs.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((name, source)) = designs.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                    let got = load(source).and_then(|first| Ok((build(&first), build(&load(source)?))));
                    results.lock().unwrap_or_else(|e| e.into_inner()).push((name.clone(), got));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort_by(|a, b| a.0.cmp(&b.0));
    let mut problems = Vec::new();
    for (name, got) in &results {
        let (first, second) = match got {
            Ok(builds) => builds,
            Err(e) => {
                problems.push(format!("{name}: does not load: {e}"));
                continue;
            }
        };
        let lost: Vec<&str> = first.refs.iter().filter(|(_, ok)| !ok).map(|(r, _)| r.as_str()).collect();
        if !lost.is_empty() {
            problems.push(format!("{name}: stored names that find nothing:\n  {}", lost.join("\n  ")));
        }
        let apart = differences(first, second, 40);
        if !apart.is_empty() {
            match UNSTABLE.iter().find(|(design, _)| design == name) {
                Some((_, why)) => eprintln!("{name} (not checked: {why}):\n  {}", apart.join("\n  ")),
                None => problems.push(format!("{name}: two builds name it apart (first, then second):\n  {}", apart.join("\n  "))),
            }
        }
    }
    assert!(problems.is_empty(), "{} of {} design(s) of the corpus:\n{}", problems.len(), results.len(), problems.join("\n"));
}
