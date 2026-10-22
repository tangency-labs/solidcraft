//! Designs saved by older SolidCraft builds (tests/corpus, see its README) open and evaluate as
//! they did when they were saved; damaged and hostile design files never panic.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solidcraft_engine::Session;

fn corpus() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "solidcraft")).collect();
    v.sort();
    assert!(v.len() >= 7, "corpus files in {}", dir.display());
    v
}

fn open(path: &Path) -> Session {
    let mut s = Session::default();
    s.execute("doc.open", &json!({"path": path.to_string_lossy()})).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    s
}

/// Differences that are deliberate behaviour changes made before the file format had versions
/// (no migration can tell those files apart): (file, check, why).
const KNOWN: [(&str, &str, &str); 2] = [
    (
        "m0-revolve-primitives",
        "Body1 bbox",
        "partial revolves turn toward the sketch normal since ac43365 (as most parametric CAD tools do); M0 turned them the other way",
    ),
    ("components-arm", "body_count", "Measure counted only the root component's bodies then; it measures every placed body now"),
];

fn known(file: &str, check: &str) -> bool {
    KNOWN.iter().any(|(f, c, _)| *f == file && *c == check)
}

/// Volumes and areas then came from coarser tessellation (and curved booleans and fillets have
/// become exact since): the same part within this relative tolerance (the oracle's).
const REL: f64 = 1e-3;

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

#[test]
fn old_designs_open_and_evaluate_as_saved() {
    let mut problems = Vec::new();
    for p in corpus() {
        let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let want: Value = serde_json::from_slice(&std::fs::read(p.with_extension("expected.json")).unwrap()).unwrap();
        let mut s = open(&p);
        for r in &s.model.results {
            if let Some(e) = &r.error {
                problems.push(format!("{name}: feature {}: {e}", r.id));
            }
        }
        assert_eq!(s.doc.format, solidcraft_engine::doc::format::format_string(solidcraft_engine::doc::format::FORMAT), "{name}: upgraded");
        let got = s.execute("inspect.measure", &json!({})).unwrap();
        if got["body_count"] != want["body_count"] && !known(&name, "body_count") {
            problems.push(format!("{name}: {} bodies, saved with {}", got["body_count"], want["body_count"]));
        }
        for w in want["bodies"].as_array().into_iter().flatten() {
            let bname = w["name"].as_str().unwrap_or_default();
            let Some(g) = got["bodies"].as_array().into_iter().flatten().find(|g| g["name"] == w["name"]) else {
                problems.push(format!("{name}: body {bname} missing"));
                continue;
            };
            for k in ["volume_mm3", "area_mm2"] {
                let (a, b) = (g[k].as_f64().unwrap_or(f64::NAN), w[k].as_f64().unwrap_or(f64::NAN));
                if !close(a, b, REL) {
                    problems.push(format!("{name}: {bname} {k} {a} (saved {b})"));
                }
            }
            if known(&name, &format!("{bname} bbox")) {
                continue;
            }
            for (end, i) in [("min", 0), ("min", 1), ("min", 2), ("max", 0), ("max", 1), ("max", 2)] {
                let (a, b) = (g["bbox"][end][i].as_f64().unwrap_or(f64::NAN), w["bbox"][end][i].as_f64().unwrap_or(f64::NAN));
                if (a - b).abs() > 1e-3 * b.abs().max(1.0) {
                    problems.push(format!("{name}: {bname} bbox {end}[{i}] {a} (saved {b})"));
                }
            }
        }
        // Saved again, it reads back the same.
        let again = solidcraft_engine::doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(again, *s.doc, "{name}: round trip");
        let package = solidcraft_engine::io::write_design_container(&s.doc).unwrap();
        let packaged = solidcraft_engine::io::read_design(&package).unwrap();
        assert_eq!(packaged, *s.doc, "{name}: container round trip after migration");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Names saved before format 14 were worked out from geometry, pieces of a split face numbered by
/// where their centres lay (`F2:top#2`). They keep naming the faces they named: marked as such,
/// they are found as the build that saved them found them, without new notes.
#[test]
fn names_saved_before_exact_history_find_the_faces_they_named() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/naming-split-pieces.solidcraft");
    let s = open(&path);
    let feature = |name: &str| s.doc.features.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("{name}"));
    assert_eq!(feature("Round").edge_names, vec!["?F2:side:l2|F2:top#2".to_string()]);
    assert_eq!(feature("Bevel").edge_names, vec!["?F2:side:l4|F2:top#1".to_string()]);
    assert_eq!(feature("Raise").face_names, vec!["?F2:top#1".to_string()]);
    for r in &s.model.results {
        assert!(r.error.is_none() && r.warning.is_none(), "{}: {:?} {:?}", r.name, r.error, r.warning);
    }
    // The raised piece is the left one, as saved (the stored point is on it).
    let state = s.model.state();
    let solid = |p: [f64; 3]| state.bodies.iter().any(|b| b.body.contains(solidcraft_engine::geom::Vec3::new(p[0], p[1], p[2])));
    assert!(solid([7.0, 15.0, 11.0]) && !solid([32.0, 15.0, 11.0]));
}

/// Body names saved before format 14 name the pieces of a split body in the kernel's shell order.
/// The features after the split that refer to a piece by its body name still find that piece:
/// the far one stays `Body1` (chamfered), the near one `Body1 (1)` (filleted, coloured).
#[test]
fn body_names_saved_before_exact_history_keep_their_pieces() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/split-body-pieces.solidcraft");
    let s = open(&path);
    assert!(s.doc.features.iter().all(|f| f.shell_piece_order), "every saved feature keeps the shell order");
    for r in &s.model.results {
        assert!(r.error.is_none() && r.warning.is_none(), "{}: {:?} {:?}", r.name, r.error, r.warning);
    }
    let state = s.model.state();
    let piece = |name: &str| state.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("{name}"));
    let solid = |name: &str, p: [f64; 3]| piece(name).body.contains(solidcraft_engine::geom::Vec3::new(p[0], p[1], p[2]));
    assert!(solid("Body1", [20.0, 24.0, 5.0]) && solid("Body1 (1)", [20.0, 6.0, 5.0]));
    assert!(s.doc.appearances.bodies.contains_key("Body1 (1)"));
}

/// Flat faces that can't take an attached sketch yet, by cause: (design, body, the faces'
/// planes as outward normal and distance from the origin, or every flat face of the body,
/// issue). Face indices are not used: they can change between builds. Each issue has an
/// ignored test below that checks its faces; a listed plane whose face attaches fails the main
/// check until it is removed from here.
type Planes = &'static [([f64; 3], f64)];
const NO_ATTACHMENT_YET: [(&str, &str, Option<Planes>, u32); 1] = [("issue-616-lid-ribs", "Lid", None, 628)];

/// The issue a face is listed under, and whether it is listed by its plane (not as every face).
fn known_without_attachment(design: &str, body: &str, normal: [f64; 3], distance: f64) -> Option<(u32, bool)> {
    let same = |(n, d): &([f64; 3], f64)| (0..3).all(|i| (n[i] - normal[i]).abs() < 1e-6) && (d - distance).abs() < 1e-4;
    NO_ATTACHMENT_YET.iter().find_map(|(d, b, planes, issue)| {
        if *d != design || *b != body {
            return None;
        }
        match planes {
            None => Some((*issue, false)),
            Some(planes) => planes.iter().any(same).then_some((*issue, true)),
        }
    })
}

fn undo_to(s: &mut Session, features: usize) {
    for _ in 0..4 {
        if s.doc.features.len() <= features {
            return;
        }
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(s.doc.features.len(), features, "undo did not remove the added feature");
}

/// Try a sketch and an offset plane on every flat face of every final body of every corpus
/// design, through the same face pick the desktop sends (body, face index, occurrence path, a
/// point on it), without `fixed`. Undo leaves the design as it was for the next face. Returns
/// (design, body, face, issue it is known under, what went wrong or None).
fn flat_face_attachments() -> Vec<(String, String, usize, Option<(u32, bool)>, Option<String>)> {
    use solidcraft_engine::doc::{FaceAttachmentStatus, apply_point};
    let mut out = Vec::new();
    for p in corpus() {
        let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let mut s = open(&p);
        let state = s.model.state();
        let placed = s.placed_document();
        let mut picks = Vec::new();
        for (world, definition, component, path, frame) in placed.body_frames_with_source(&state) {
            let Some(body) = state.bodies.iter().find(|b| b.definition_name() == definition && placed.model_body_component(b) == component) else {
                continue;
            };
            let mesh = body.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
            for index in 0..body.body.face_count() {
                if body.body.planar_face(index).is_none() {
                    continue;
                }
                // The middle of the face's largest triangle: well inside the face.
                let point = mesh
                    .triangles
                    .iter()
                    .zip(&mesh.tri_face)
                    .filter(|(_, f)| **f as usize == index)
                    .filter_map(|(t, _)| mesh.tri(t))
                    .max_by(|[a, b, c], [d, e, f]| (*b - *a).cross(*c - *a).len().total_cmp(&(*e - *d).cross(*f - *d).len()))
                    .map(|[a, b, c]| (a + b + c) * (1.0 / 3.0));
                let Some(point) = point else { continue };
                let point = apply_point(&frame, point);
                let normal =
                    body.body.planar_face(index).map(|plane| solidcraft_engine::doc::apply_vector(&frame, plane.normal())).unwrap_or_default();
                let key = ([normal.x, normal.y, normal.z], normal.dot(point));
                picks.push((world.clone(), index, key, json!({"face": point, "body": world, "index": index, "path": path})));
            }
        }
        let before = s.doc.features.len();
        for (body, index, (normal, distance), plane) in picks {
            let mut problem = None;
            match s.execute("sketch.create", &json!({"plane": plane})) {
                Ok(created) => {
                    let id = created["sketch"].as_u64().unwrap_or(0);
                    match s.placed_document().sketch_face_attachment(id, &s.model) {
                        FaceAttachmentStatus::Healthy { .. } | FaceAttachmentStatus::Sheet { .. } => {}
                        other => problem = Some(format!("sketch attachment {other:?}")),
                    }
                    s.execute("sketch.finish", &json!({})).unwrap();
                    undo_to(&mut s, before);
                }
                Err(e) => problem = Some(format!("sketch.create: {e}")),
            }
            match s.execute("construct.plane.offset", &json!({"base": plane, "offset": 5})) {
                Ok(_) => undo_to(&mut s, before),
                Err(e) => problem = Some(problem.map_or(String::new(), |p| p + "; ") + &format!("construct.plane.offset: {e}")),
            }
            let known = known_without_attachment(&name, &body, normal, distance);
            out.push((name.clone(), body, index, known, problem));
        }
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{name}: left with errors after undo");
    }
    out
}

/// Every flat face of every corpus design takes a durable attachment (issue #489: the top face
/// of an extrude sketched over a face's projected boundary had "no exact planar source"),
/// except the faces listed in [`NO_ATTACHMENT_YET`] under their open issues.
#[test]
fn every_flat_face_of_every_corpus_design_takes_an_attached_sketch() {
    let all = flat_face_attachments();
    assert!(all.len() > 50, "only {} faces checked", all.len());
    let problems: Vec<String> = all
        .iter()
        .filter_map(|(design, body, face, known, problem)| match (known, problem) {
            (None, Some(problem)) => Some(format!("{design}: {body} face {face}: {problem}")),
            (Some((issue, true)), None) => Some(format!("{design}: {body} face {face} attaches now: remove it from NO_ATTACHMENT_YET (#{issue})")),
            _ => None,
        })
        .collect();
    assert!(problems.is_empty(), "{} of {} faces:\n{}", problems.len(), all.len(), problems.join("\n"));
}

fn known_faces_attach(issue: u32) {
    let problems: Vec<String> = flat_face_attachments()
        .into_iter()
        .filter(|(.., known, problem)| known.is_some_and(|(known, _)| known == issue) && problem.is_some())
        .map(|(design, body, face, _, problem)| format!("{design}: {body} face {face}: {}", problem.unwrap_or_default()))
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
#[ignore = "fails until #628 is fixed"]
fn faces_of_a_lid_with_joined_ribs_take_an_attached_sketch() {
    known_faces_attach(628);
}

/// A small deterministic generator (xorshift).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const HOSTILE: [&str; 14] =
    ["0", "-1", "1e308", "-1e308", "1e9", "4294967297", "\"\"", "\"1/0\"", "\"((((1\"", "null", "true", "[]", "{}", "\"nope\""];

/// Every JSON value in a document, as a path of keys and indices.
fn paths(v: &Value, at: &mut Vec<Value>, out: &mut Vec<Vec<Value>>) {
    out.push(at.clone());
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                at.push(json!(k));
                paths(x, at, out);
                at.pop();
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                at.push(json!(i));
                paths(x, at, out);
                at.pop();
            }
        }
        _ => {}
    }
}

fn at_mut<'a>(v: &'a mut Value, path: &[Value]) -> Option<&'a mut Value> {
    let mut cur = v;
    for k in path {
        cur = match k {
            Value::String(s) => cur.get_mut(s.as_str())?,
            Value::Number(n) => cur.get_mut(n.as_u64()? as usize)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// Open a file's bytes as File → Open does and evaluate them; any panic is a failure.
fn try_bytes(bytes: &[u8]) {
    if let Ok(doc) = solidcraft_engine::io::read_design(bytes) {
        let mut s = Session::new(doc);
        let _ = s.execute("inspect.measure", &json!({}));
        let _ = s.execute("document.inspect", &json!({}));
    }
}

/// Hostile shapes that no mutation of a saved file produces: plane references that close a
/// cycle, which resolve through the model and back into the document.
fn generated() -> Vec<(String, Vec<u8>)> {
    let to = |name: &str| json!({"type": "plane", "plane": {"type": "construction", "name": name}});
    let plane = |id: u64, name: &str, side: Value| {
        json!({"id": id, "name": name, "suppressed": false, "type": "construction_plane",
               "plane": {"type": "midplane", "a": side, "b": {"type": "plane", "plane": {"type": "origin", "name": "XZ"}}}})
    };
    let design = |name: &str, features: Value| {
        json!({"format": "solidcraft/11", "name": name, "units": "mm", "params": [], "next_id": 9, "features": features}).to_string().into_bytes()
    };
    // A sketch past the caps interactive editing enforces: solving it would size dense matrices
    // by the point count and abort on the allocation.
    let point = json!({"id": "p1", "pos": [0.0, 0.0], "fixed": false});
    let oversized = json!([{"id": 1, "name": "Sketch1", "suppressed": false, "type": "sketch",
                            "plane": {"type": "origin", "name": "XY"},
                            "sketch": {"points": vec![point; 200_000], "curves": [], "constraints": []}}]);
    vec![
        ("construction plane is its own side".into(), design("selfplane", json!([plane(1, "P1", to("P1"))]))),
        ("two construction planes refer to each other".into(), design("twoplanes", json!([plane(1, "P1", to("P2")), plane(2, "P2", to("P1"))]))),
        ("sketch with 200,000 points".into(), design("bigsketch", oversized)),
    ]
}

#[test]
fn damaged_and_hostile_design_files_never_panic() {
    let mut rng = Rng(0x5eed_cafe_f00d_d00d);
    // The cases come from one seeded sequence; trying them is independent, so threads share it.
    let mut all_cases: Vec<(String, String, Vec<u8>)> = Vec::new();
    for p in corpus() {
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        let bytes = std::fs::read(&p).unwrap();
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        let mut all = Vec::new();
        paths(&doc, &mut Vec::new(), &mut all);
        let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
        // Truncated and bit-flipped files.
        for _ in 0..8 {
            let cut = rng.below(bytes.len());
            cases.push((format!("truncated at {cut}"), bytes[..cut].to_vec()));
            let mut b = bytes.clone();
            let i = rng.below(b.len());
            b[i] ^= 1 << rng.below(8);
            cases.push((format!("bit flip at {i}"), b));
        }
        // Values replaced by hostile ones, and keys removed.
        for _ in 0..25 {
            let path = &all[rng.below(all.len())];
            let mut v = doc.clone();
            let h = HOSTILE[rng.below(HOSTILE.len())];
            if let Some(slot) = at_mut(&mut v, path) {
                *slot = serde_json::from_str(h).unwrap();
            }
            cases.push((format!("{path:?} = {h}"), v.to_string().into_bytes()));
            let mut v = doc.clone();
            if let Some((last, parent)) = path.split_last()
                && let (Some(Value::Object(m)), Value::String(k)) = (at_mut(&mut v, parent), last)
            {
                m.remove(k);
            }
            cases.push((format!("{path:?} removed"), v.to_string().into_bytes()));
        }
        // Features repeated many times; deep nesting; not JSON at all.
        let mut v = doc.clone();
        if let Some(Value::Array(fs)) = v.get_mut("features") {
            let copy = fs.clone();
            for _ in 0..2 {
                fs.extend(copy.iter().cloned());
            }
        }
        cases.push(("features repeated".into(), v.to_string().into_bytes()));
        cases.push(("deep nesting".into(), format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)).into_bytes()));
        cases.push(("binary".into(), (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect()));
        all_cases.extend(cases.into_iter().map(|(what, b)| (name.clone(), what, b)));
    }
    all_cases.extend(generated().into_iter().map(|(what, b)| ("generated".to_string(), what, b)));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let failures: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut failures = Vec::new();
                    while let Some((name, what, b)) = all_cases.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                        if std::env::var_os("CORPUS_TRACE").is_some() {
                            eprintln!("{name}: {what}");
                        }
                        if std::panic::catch_unwind(|| try_bytes(b)).is_err() {
                            failures.push(format!("{name}: {what}"));
                        }
                    }
                    failures
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap()).collect()
    });
    assert!(failures.is_empty(), "panics:\n{}", failures.join("\n"));
}
