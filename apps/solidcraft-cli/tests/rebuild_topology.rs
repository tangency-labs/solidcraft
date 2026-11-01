//! A parameter change gives the same topology as building at the new value from scratch, and
//! changing it back gives the original topology: no stale splits, no faces added to regions
//! nothing touched. Over the design corpus (`tests/corpus`, `tests/action_matrix/designs`), and
//! the README demo's Raspberry Pi case pinned step by step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::kernel::{FaceSurface, face_surface};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// A folder of its own for test `name` (tests run side by side).
fn scratch_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("solidcraft-rebuild-topology-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A body's topology: its face and edge counts, and how many faces lie in each plane.
#[derive(Debug, PartialEq, Eq)]
struct Topology {
    faces: usize,
    edges: usize,
    planes: BTreeMap<[i64; 4], usize>,
}

/// Every solid body's topology, by name.
fn topology(s: &Session) -> BTreeMap<String, Topology> {
    let st = s.world_state();
    st.bodies
        .iter()
        .filter(|b| !b.body.is_mesh())
        .map(|b| {
            let mut planes = BTreeMap::new();
            for i in 0..b.body.face_count() {
                if let Some(FaceSurface::Plane { normal, point }) = face_surface(&b.body, i) {
                    let key = [normal.x, normal.y, normal.z, normal.dot(point) / 100.].map(|x| (x * 1e4).round() as i64);
                    *planes.entry(key).or_insert(0) += 1;
                }
            }
            (b.name.clone(), Topology { faces: b.body.face_count(), edges: b.body.edge_count(), planes })
        })
        .collect()
}

fn open(path: &Path) -> Session {
    let mut s = Session::default();
    s.execute("doc.open", &json!({"path": path.to_string_lossy()})).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    s
}

/// The design built from scratch: saved as it is (timeline only, no cached geometry) and
/// opened again.
fn rebuilt(s: &Session, path: &Path) -> Session {
    let mut t = s.scratch();
    t.execute("file.save", &json!({"path": path.to_string_lossy(), "format": "json"})).unwrap();
    open(path)
}

fn errors(s: &Session) -> Vec<u64> {
    s.model.results.iter().filter(|r| r.error.is_some()).map(|r| r.id).collect()
}

/// The corpus's designs, as sessions.
fn designs() -> Vec<(String, Session)> {
    let root = root();
    let mut out = Vec::new();
    for dir in ["apps/solidcraft-cli/tests/corpus", "tests/action_matrix/designs"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(dir))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                (solidcraft_engine::io::is_design_container_path(p) || p.extension().is_some_and(|x| x == "json"))
                    && !p.to_string_lossy().ends_with(".expected.json")
            })
            .collect();
        files.sort();
        for p in files {
            let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let s = if solidcraft_engine::io::is_design_container_path(&p) {
                open(&p)
            } else {
                let mut s = Session::default();
                let script: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
                s.run_script(&script).unwrap_or_else(|e| panic!("{name}: {e}"));
                if s.active_sketch.is_some() {
                    let _ = s.execute("sketch.finish", &json!({}));
                }
                s
            };
            out.push((name, s));
        }
    }
    out
}

/// For each design and its first length parameter that edits cleanly (the user's first): the parameter
/// scaled by 1.1 gives what a fresh build at that value gives, and setting it back gives the
/// original topology.
#[test]
fn a_parameter_edit_rebuilds_like_a_fresh_build_and_reverts_exactly() {
    let dir = scratch_dir("corpus");
    let mut problems = Vec::new();
    let mut checked = 0;
    for (name, s) in designs() {
        let original = topology(&s);
        let before = errors(&s);
        let mut params: Vec<_> = s.doc.params.iter().filter(|p| p.unit == "mm").cloned().collect();
        params.sort_by_key(|p| p.model);
        let mut tried = 0;
        for p in params {
            if tried == 1 {
                break;
            }
            let mut t = s.scratch();
            let expr = format!("({}) * 1.1", p.expr);
            if t.execute("parameters.change", &json!({"name": p.name, "expression": expr})).is_err() || errors(&t) != before {
                continue;
            }
            tried += 1;
            checked += 1;
            let edited = topology(&t);
            let fresh = topology(&rebuilt(&t, &dir.join(format!("{name}-{}.solidcraft", p.name))));
            if edited != fresh {
                problems.push(format!("{name}: {} = {expr}: edited {edited:?}\n  fresh {fresh:?}", p.name));
            }
            t.execute("parameters.change", &json!({"name": p.name, "expression": p.expr})).unwrap();
            let back = topology(&t);
            if back != original {
                problems.push(format!("{name}: {} back to {}: {back:?}\n  original {original:?}", p.name, p.expr));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(checked >= 8, "only {checked} parameter edits checked");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The README demo's Raspberry Pi case (`examples/demo/enclosure_ui_mcp.py --story pi`): its
/// face count after each feature, and each outer side (the four walls and the bottom) one face
/// throughout: (a) built at height 24, (b) built at 24 and changed to 32 in Parameters, (c)
/// built at 32. The port cut-outs and the floor vents used to split the long wall (or the end
/// wall at 24) into strips along the openings' tops and bottoms.
#[test]
fn pi_case_faces_per_step_at_both_heights() {
    // Sketch1 (1) .. RectangularPattern2 (14): the faces once features 1..=k are built.
    const FACES: [(usize, usize); 13] =
        [(2, 6), (3, 10), (4, 19), (5, 19), (6, 25), (7, 43), (8, 43), (9, 55), (10, 55), (11, 59), (12, 59), (13, 63), (14, 79)];
    let dir = scratch_dir("pi");
    let design: Value = serde_json::from_slice(&std::fs::read(root().join("tests/action_matrix/designs/pi-case.sc")).unwrap()).unwrap();
    let at = |height: &str, file: &str| {
        let mut d = design.clone();
        for p in d["params"].as_array_mut().unwrap() {
            if p["name"] == "height" {
                p["expr"] = json!(height);
            }
        }
        let path = dir.join(file);
        std::fs::write(&path, serde_json::to_vec(&d).unwrap()).unwrap();
        open(&path)
    };
    let a = at("24 mm", "pi-24.solidcraft");
    let mut b = at("24 mm", "pi-24-to-32.solidcraft");
    b.execute("parameters.change", &json!({"name": "height", "expression": "32 mm"})).unwrap();
    let c = at("32 mm", "pi-32.solidcraft");
    let _ = std::fs::remove_dir_all(&dir);
    let mut per_case = Vec::new();
    for (case, s, h) in [("(a) at 24", a, 24.), ("(b) 24 changed to 32", b, 32.), ("(c) at 32", c, 32.)] {
        assert!(errors(&s).is_empty(), "{case}: features fail: {:?}", errors(&s));
        let mut steps = Vec::new();
        for (k, want) in FACES {
            let mut t = s.scratch();
            t.execute("timeline.rollback", &json!({"position": k})).unwrap();
            let st = t.world_state();
            let body = st.bodies.iter().find(|b| b.name == "Body1").unwrap();
            assert_eq!(body.body.face_count(), want, "{case}: faces after feature {k}");
            // Each outer side: the faces in its plane.
            for (n, d) in [([0., -1., 0.], 33.), ([0., 1., 0.], 33.), ([-1., 0., 0.], 47.5), ([1., 0., 0.], 47.5), ([0., 0., -1.], 0.)] {
                let on = (0..body.body.face_count())
                    .filter(|&i| match face_surface(&body.body, i) {
                        Some(FaceSurface::Plane { normal, point }) => {
                            (normal.x - n[0]).abs() + (normal.y - n[1]).abs() + (normal.z - n[2]).abs() < 1e-6 && (normal.dot(point) - d).abs() < 1e-6
                        }
                        _ => false,
                    })
                    .count();
                assert_eq!(on, 1, "{case}: after feature {k} the outer side facing {n:?} is {on} faces");
            }
            let bbox_top = body.mesh().bounds().max.z;
            assert!((bbox_top - h).abs() < 1e-6, "{case}: height {bbox_top}");
            steps.push(topology(&t));
        }
        per_case.push((case, steps));
    }
    // (b) and (c): the same topology after every feature.
    let (b, c) = (&per_case[1].1, &per_case[2].1);
    for (k, (x, y)) in b.iter().zip(c).enumerate() {
        assert_eq!(x, y, "after feature {}: (b) differs from (c)", k + 2);
    }
}
