//! Every design of the corpus rebuilds with the same persistent names: the names of each body's
//! faces and edges, with whether each follows exact history, and every name a design stores
//! (fillet and chamfer edges, faces a feature picks, the face a sketch sits on, sheet metal
//! edges) with the face or edge it finds now, by that face's or edge's name. A rename, a name
//! that becomes geometric, a face or edge more or fewer, or a reference that finds another face
//! or none fails the test.
//!
//! Face and edge indices are left out: they are the kernel's order, which a rebuild need not
//! keep (a fillet's or a boolean's faces come out in another order from one process to the next)
//! and which another kernel changes, while names and references must not change. A body's names
//! are listed sorted.
//!
//! The corpus is the action matrix's (the old design files in `tests/corpus`, the examples, the
//! demo and typical designs in `tests/action_matrix/designs`, the reference recipes), the sample
//! designs, the engine's design fixtures and the older designs kept by the hole matrix, taper
//! and projection tests. Each design's record is `tests/naming/<group>/<design>.txt`; to record
//! an intended change, run the test with `NAMING_CORPUS_BLESS=1` and say why in the pull request.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::{FaceAttachmentStatus, Feature, FeatureKind, ModelBody, ModelState, PlaneRef, naming};
use solidcraft_engine::geom::Vec3;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn golden_dir() -> PathBuf {
    root().join("tests/naming")
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

/// Whether an edge name carries its own number (`~k`): several edges between the same two faces
/// are numbered by where they lie, which is not exact history.
fn numbered(name: &str) -> bool {
    name.rfind('~').and_then(|i| name.get(i + 1..)).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// The names of a body's faces and edges, sorted (with how many of each it has).
fn record_body(out: &mut Vec<String>, b: &ModelBody) {
    if b.body.is_mesh() {
        out.push(format!("body {}: mesh", b.name));
        return;
    }
    let faces = naming::face_names(b);
    let edges = naming::edge_names(b);
    out.push(format!("body {}: {} faces, {} edges", b.name, faces.len(), edges.len()));
    let mut lines: Vec<String> =
        faces.iter().enumerate().map(|(i, n)| format!("{} face {n}{}", b.name, flags(&[(!naming::face_name_exact(b, i), "geometric")]))).collect();
    lines.sort();
    out.append(&mut lines);
    let m = b.mesh();
    let mut lines: Vec<String> = edges
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let exact = m.edge_faces.get(i).is_some_and(|fs| fs.iter().all(|f| naming::face_name_exact(b, *f as usize))) && !numbered(n);
            format!("{} edge {n}{}", b.name, flags(&[(!exact, "geometric")]))
        })
        .collect();
    lines.sort();
    out.append(&mut lines);
}

fn face_label(st: &ModelState, body: usize, face: usize) -> String {
    match st.bodies.get(body) {
        Some(b) => format!("{} face {}", b.name, naming::face_names(b).get(face).map(String::as_str).unwrap_or("?")),
        None => format!("body #{body} face #{face}"),
    }
}

/// Every name feature `f` stores, with what it finds in the model before it, as a feature
/// rebuilding finds it.
fn record_refs(out: &mut Vec<String>, s: &Session, f: &Feature) {
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
            let found = if explicit_emboss {
                "not checked (a cylindrical emboss finds its face on its own body)".to_string()
            } else {
                match naming::locate_face_on_body(&fst, f.kind.face_body(), n, p) {
                    Some(hit) => format!(
                        "{}{}",
                        face_label(&fst, hit.body, hit.face),
                        flags(&[(hit.split, "split"), (hit.merged, "merged"), (!hit.verified, "geometric"), (hit.legacy, "legacy")])
                    ),
                    None => "gone".into(),
                }
            };
            out.push(format!("{who} face {k} `{n}` -> {found}"));
        }
    }
    // Edges a feature picks by point and stores by name.
    if f.edge_names.iter().any(|n| !n.is_empty()) {
        let edge = |out: &mut Vec<String>, b: &ModelBody, k: usize, n: &str, p: Vec3| {
            let found = match naming::locate_edge(b, n, p) {
                Ok(hit) => format!(
                    "{} edge {}{}",
                    b.name,
                    naming::edge_names(b).get(hit.edge).map(String::as_str).unwrap_or("?"),
                    flags(&[(hit.split, "split"), (!hit.verified, "geometric"), (hit.legacy, "legacy")])
                ),
                Err(e) => format!("gone ({e})"),
            };
            out.push(format!("{who} edge {k} `{n}` -> {found}"));
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
                    None => out.push(format!("{who} edges: no body to round")),
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
                None => out.push(format!("{who} bends: body `{body}` gone")),
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
                    out.push(format!("{who} sheet edge {k} `{n}` -> {found}"));
                }
            }
            other => {
                for (k, n) in f.edge_names.iter().enumerate().filter(|(_, n)| !n.is_empty()) {
                    out.push(format!("{who} edge {k} `{n}` -> not checked ({})", other.type_name()));
                }
            }
        }
    }
    // The face a sketch sits on.
    if let FeatureKind::Sketch { plane: PlaneRef::Face { name, anchor, .. }, .. } = &f.kind {
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
                        Err(e) => format!("no faces ({e})"),
                    },
                    None => format!("body `{}` gone", target.body),
                };
                format!("healthy: {faces}")
            }
            FaceAttachmentStatus::Lost { message, .. } => format!("lost: {message}"),
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
        out.push(format!("{who} plane {stored} -> {status}"));
    }
}

/// A design's record: its features that failed, every body's names, every stored reference.
fn record(name: &str, s: &Session) -> String {
    let mut out = vec![format!("# {name}: persistent names after a rebuild (apps/solidcraft-cli/tests/naming_corpus.rs)")];
    for r in s.model.results.iter().filter(|r| r.error.is_some()) {
        out.push(format!("failed {} ({})", r.name, r.id));
    }
    let st = s.model.state();
    for b in &st.bodies {
        record_body(&mut out, b);
    }
    for f in s.doc.features.iter().filter(|f| !f.suppressed) {
        record_refs(&mut out, s, f);
    }
    out.push(String::new());
    out.join("\n")
}

/// The lines of `want` and `got` that differ, as `-`/`+` lines (the longest common run of
/// lines kept), at most `cap` of them.
fn diff(want: &str, got: &str, cap: usize) -> Vec<String> {
    let (a, b): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut out = Vec::new();
    if a.len().saturating_mul(b.len()) > 25_000_000 {
        out.extend(a.iter().map(|l| format!("- {l}")));
        out.extend(b.iter().map(|l| format!("+ {l}")));
    } else {
        // lcs[i][j]: the longest common run of a[i..] and b[j..].
        let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                i += 1;
                j += 1;
            } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
                out.push(format!("- {}", a[i]));
                i += 1;
            } else {
                out.push(format!("+ {}", b[j]));
                j += 1;
            }
        }
    }
    let n = out.len();
    out.truncate(cap);
    if n > cap {
        out.push(format!("… and {} more", n - cap));
    }
    out
}

/// Designs whose names differ from one run to the next today: (design, why). Their records are
/// kept and their differences printed, not failed on, until the cause is fixed.
const UNSTABLE: [(&str, &str); 1] = [(
    "regressions/closed_sweep_torus_cut",
    "the halves of each swept wall lie level along every axis, so which is `#1` and which `#2` follows the order the kernel gives the faces in, which changes from one process to the next",
)];

#[test]
fn corpus_designs_rebuild_with_the_same_names_and_references() {
    let bless = std::env::var_os("NAMING_CORPUS_BLESS").is_some();
    let designs = designs();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(designs.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((name, source)) = designs.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                    let got = match load(source) {
                        Ok(s) => record(name, &s),
                        Err(e) => format!("# {name}: does not load: {e}\n"),
                    };
                    results.lock().unwrap_or_else(|e| e.into_inner()).push((name.clone(), got));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort();
    let dir = golden_dir();
    let mut problems = Vec::new();
    for (name, got) in &results {
        let path = dir.join(format!("{name}.txt"));
        let want = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        if bless {
            if want != *got {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, got).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
            continue;
        }
        if want.is_empty() {
            problems.push(format!("{name}: no record (tests/naming/{name}.txt)"));
        } else if want != *got {
            let lines = diff(&want, got, 40).join("\n  ");
            match UNSTABLE.iter().find(|(design, _)| design == name) {
                Some((_, why)) => eprintln!("{name} (not checked: {why}):\n  {lines}"),
                None => problems.push(format!("{name}:\n  {lines}")),
            }
        }
    }
    // A record whose design left the corpus.
    let known: std::collections::BTreeSet<PathBuf> = results.iter().map(|(name, _)| dir.join(format!("{name}.txt"))).collect();
    for group in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        for file in std::fs::read_dir(group.path()).into_iter().flatten().flatten() {
            if !known.contains(&file.path()) {
                if bless {
                    let _ = std::fs::remove_file(file.path());
                } else {
                    problems.push(format!("{}: a record without a design", file.path().display()));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} design(s) rebuild with other names than tests/naming records (if that is intended, record it with NAMING_CORPUS_BLESS=1 and say why in the pull request; - recorded, + now):\n{}",
        problems.len(),
        problems.join("\n")
    );
}
