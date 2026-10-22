//! The action matrix: every ordinary action on every face, edge, vertex and body of the final
//! bodies of a corpus of real designs, through the same engine commands the desktop sends (a
//! face pick carries its body, face index, occurrence path and a point on it; never `fixed`).
//!
//! The corpus: the old design files (`tests/corpus`), the examples, the reference recipes
//! (`tests/oracle/*/recipe.json`), the demo designs and a few typical parts
//! (`tests/action_matrix/designs`). Each action is tried on a scratch copy of the design: it
//! passes when the command works, the timeline has no new errors, and a sketch's face attachment
//! is healthy. A sample of passes also gets an edit round: a driving parameter changes and the
//! new feature must still resolve.
//!
//! `tests/action_matrix/baseline.json` is a ratchet: an action that passed there and fails now
//! fails the test (a regression). Newly passing actions are reported; record them with
//! `cargo xtask action-matrix --bless` (or `ACTION_MATRIX_BLESS=1` on [`action_matrix_all`]).
//! Every CI run checks a fixed sample ([`action_matrix_sample`], under a minute); the full matrix
//! is the ignored [`action_matrix_shard`], which the nightly "Action matrix" jobs run one shard
//! each (`ACTION_MATRIX_SHARD=k/n`), cut by what each design costs (`tests/action_matrix/costs.json`).
//! [`action_matrix_determinism`] runs a sample twice and lists any action whose outcome changed.
//! See docs/action-matrix.md.
//!
//! Each design runs in a child process (this test binary, [`action_matrix_worker`]) so an action
//! that hangs or aborts is recorded as such and the rest of the design still runs.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::kernel::{FaceSurface, face_surface};
use solidcraft_engine::doc::{FaceAttachmentStatus, ModelBody, naming};
use solidcraft_engine::geom::Vec3;

/// One in this many actions runs on pull requests (by a stable hash of its key).
const PR_SAMPLE: u64 = 40;
/// One in this many actions runs twice in the nightly determinism check, on the designs whose
/// whole matrix takes at most [`DETERMINISM_MAX_SECS`] on a runner (most of the corpus).
const DETERMINISM_SAMPLE: u64 = 8;
const DETERMINISM_MAX_SECS: f64 = 300.0;
/// Designs that take seconds to build: the full matrix only.
const SLOW: [&str; 7] = [
    "oracle/41-tube-radial-holes",
    "oracle/39-tee-pipe",
    "demo/enclosure_steps",
    "oracle/15-combine-intersect",
    "oracle/35-fillet-curved-curved",
    "oracle/40-box-sphere-torus",
    "oracle/42-overlapping-patterns",
];
/// One in this many passes also gets the edit round.
const EDIT_SAMPLE: u64 = 6;
/// An action that takes longer than this is recorded as a timeout.
const ACTION_TIMEOUT: Duration = Duration::from_secs(90);
/// Building a design and listing its actions, before the first one runs: the biggest designs
/// (oracle 41's tube with radial holes, about 25 s here) take several times that on a loaded CI
/// runner, and a design that never loads loses all its actions, not one.
const LOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// Caps per body, so one large imported or patterned body cannot dominate the run.
const MAX_FACES: usize = 120;
const MAX_EDGES: usize = 240;
const MAX_VERTICES: usize = 120;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn baseline_path() -> PathBuf {
    root().join("tests/action_matrix/baseline.json")
}

// ---------------------------------------------------------------------------------------------
// The corpus

/// Every design in the corpus: (name, where it comes from).
fn designs() -> Vec<(String, PathBuf)> {
    let root = root();
    let mut out = Vec::new();
    let mut add = |prefix: &str, dir: PathBuf, ext: &str| {
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect();
        v.sort();
        for p in v {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            out.push((format!("{prefix}/{stem}"), p));
        }
    };
    add("corpus", root.join("apps/solidcraft-cli/tests/corpus"), "solidcraft");
    add("examples", root.join("examples"), "json");
    add("examples", root.join("examples/acceptance"), "json");
    add("regressions", root.join("examples/regressions"), "json");
    add("demo", root.join("examples/demo"), "json");
    add("typical", root.join("tests/action_matrix/designs"), "json");
    add("demo", root.join("tests/action_matrix/designs"), "solidcraft");
    let mut cases: Vec<PathBuf> =
        std::fs::read_dir(root.join("tests/oracle")).unwrap().flatten().map(|e| e.path()).filter(|p| p.join("recipe.json").is_file()).collect();
    cases.sort();
    for c in cases {
        out.push((format!("oracle/{}", c.file_name().unwrap_or_default().to_string_lossy()), c.join("recipe.json")));
    }
    out
}

/// Build a design from its file.
fn load(path: &Path) -> Result<Session, String> {
    let mut s = Session::default();
    if path.extension().is_some_and(|x| x == "solidcraft") {
        s.execute("doc.open", &json!({"path": path.to_string_lossy()})).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let script = if path.file_name().is_some_and(|n| n == "recipe.json") {
        let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("recipe").arg(path).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("recipe: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?
    } else if let Some(steps) = v.get("steps").and_then(Value::as_array) {
        // The demo's steps: an MCP `execute` each.
        json!({"commands": steps.iter().map(|st| json!({"command": st["command"], "params": st["params"]})).collect::<Vec<_>>()})
    } else {
        v
    };
    s.run_script(&script).map_err(|e| e.to_string())?;
    if s.active_sketch.is_some() {
        let _ = s.execute("sketch.finish", &json!({}));
    }
    Ok(s)
}

// ---------------------------------------------------------------------------------------------
// The actions

/// What an action does, given the design (on a scratch copy).
#[derive(Clone, Debug)]
enum Act {
    /// A command; a sketch it creates must be healthy.
    Run(&'static str, Value),
    /// A sketch on an origin plane with this edge projected into it.
    Project { plane: &'static str, edge: Value },
}

#[derive(Clone, Debug)]
struct Action {
    entity: String,
    action: &'static str,
    act: Act,
}

fn pt(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= len * 0.5 && l > 0.0 {
            return w[0].lerp(w[1], (len * 0.5 - acc) / l);
        }
        acc += l;
    }
    pts.first().copied().unwrap_or_default()
}

fn polyline_len(pts: &[Vec3]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// A straight polyline's direction.
fn straight(pts: &[Vec3]) -> Option<Vec3> {
    let (a, z) = (*pts.first()?, *pts.last()?);
    let dir = (z - a).normalized()?;
    let tol = 1e-6 * a.dist(z).max(1.0);
    pts.iter().all(|p| (*p - a).cross(dir).len() < tol).then_some(dir)
}

fn closed(pts: &[Vec3]) -> bool {
    match (pts.first(), pts.last()) {
        (Some(a), Some(z)) => pts.len() > 2 && a.dist(*z) < 1e-9,
        _ => false,
    }
}

/// A point as integers (micrometres), for a stable order.
fn place(p: Vec3) -> [i64; 3] {
    [(p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64, (p.z * 1000.0).round() as i64]
}

fn round3(x: f64) -> f64 {
    let r = (x * 1000.0).round() / 1000.0;
    if r == 0.0 { 0.0 } else { r }
}

/// Every action on every entity of the design's final bodies, in a fixed order.
fn actions(s: &Session) -> Vec<Action> {
    let st = s.world_state();
    let placed = s.placed_document();
    let paths: BTreeMap<String, Vec<u64>> =
        placed.body_frames_with_source(&s.model.state()).into_iter().map(|(world, _, _, path, _)| (world, path)).collect();
    let mut out = Vec::new();
    let mut bodies: Vec<&ModelBody> = st.bodies.iter().filter(|b| !b.body.is_mesh()).collect();
    bodies.sort_by(|a, b| a.name.cmp(&b.name));
    for b in bodies {
        let world = b.name.clone();
        let path = paths.get(&world).cloned().unwrap_or_default();
        let mesh = b.mesh();
        let names = naming::face_names(b);
        let edge_names = naming::edge_names(b);
        let size = b.body.size().max(1e-3);
        let face_name = |i: usize| match names.get(i) {
            Some(n) if !n.is_empty() => format!("{world}/{n}"),
            _ => format!("{world}/face{i}"),
        };
        // A point well inside each face: the middle of its largest triangle.
        let mut inside: BTreeMap<usize, (Vec3, f64)> = BTreeMap::new();
        for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if let Some([a, bb, c]) = mesh.tri(t) {
                let area = (bb - a).cross(c - a).len();
                let e = inside.entry(*f as usize).or_insert(((a + bb + c) * (1.0 / 3.0), -1.0));
                if area > e.1 {
                    *e = ((a + bb + c) * (1.0 / 3.0), area);
                }
            }
        }
        let face_count = b.body.face_count();
        let planes: Vec<Option<(Vec3, Vec3)>> = (0..face_count)
            .map(|i| {
                let p = inside.get(&i)?.0;
                b.body.planar_face(i).map(|pl| (p, pl.normal()))
            })
            .collect();
        let edges_of = |f: usize| mesh.face_edges(f as u32);
        let mut faces: Vec<usize> = (0..face_count).collect();
        faces.sort_by_cached_key(|i| (names.get(*i).cloned().unwrap_or_default(), inside.get(i).map(|(p, _)| place(*p)).unwrap_or_default()));
        for i in faces.into_iter().take(MAX_FACES) {
            let Some(&(p, _)) = inside.get(&i).filter(|(p, _)| p.is_finite()) else { continue };
            let entity = face_name(i);
            let pick = json!({"face": pt(p), "body": world, "index": i, "path": path});
            if let Some(Some((_, n))) = planes.get(i) {
                let n = *n;
                let mut add = |action, act| out.push(Action { entity: entity.clone(), action, act });
                add("face.sketch", Act::Run("sketch.create", json!({"plane": pick})));
                add("face.offset_plane", Act::Run("construct.plane.offset", json!({"base": pick, "offset": round3(size * 0.1)})));
                // A plane through the face's longest straight edge, square to the face.
                let longest = edges_of(i)
                    .into_iter()
                    .filter_map(|e| mesh.edges.get(e).filter(|pts| straight(pts).is_some()))
                    .max_by(|a, b| polyline_len(a).total_cmp(&polyline_len(b)));
                if let Some(e) = longest {
                    add(
                        "face.angle_plane",
                        Act::Run("construct.plane.perpendicular", json!({"plane": {"face": pt(p)}, "line": {"edge": pt(polyline_mid(e))}})),
                    );
                }
                // Midplane with the nearest opposite parallel face of the body.
                let partner = |k: usize| -> Option<usize> {
                    let (pk, nk) = planes.get(k)?.as_ref().copied()?;
                    planes
                        .iter()
                        .enumerate()
                        .filter_map(|(j, q)| q.map(|q| (j, q)))
                        .filter(|(j, (q, m))| *j != k && m.dot(nk) < -1.0 + 1e-9 && (*q - pk).dot(nk).abs() > 1e-6)
                        .min_by(|(j1, (q1, _)), (j2, (q2, _))| {
                            (*q1 - pk).dot(nk).abs().total_cmp(&(*q2 - pk).dot(nk).abs()).then_with(|| face_name(*j1).cmp(&face_name(*j2)))
                        })
                        .map(|(j, _)| j)
                };
                if let Some(j) = partner(i)
                    && (face_name(i) < face_name(j) || partner(j) != Some(i))
                    && let Some(Some((q, _))) = planes.get(j)
                {
                    add("face.midplane", Act::Run("construct.plane.midplane", json!({"a": {"face": pt(p)}, "b": {"face": pt(*q)}})));
                }
                let d = round3((size * 0.05).clamp(0.5, 5.0));
                add("face.press_pull", Act::Run("solid.press_pull", json!({"face": pt(p), "distance": d})));
                add("face.extrude", Act::Run("solid.extrude", json!({"face": pt(p), "distance": d, "operation": "join"})));
                // A hole well inside the face: smaller than the way to its nearest edge.
                let room = edges_of(i)
                    .into_iter()
                    .filter_map(|e| mesh.edges.get(e))
                    .flat_map(|pts| pts.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).collect::<Vec<_>>())
                    .fold(f64::MAX, f64::min);
                if room.is_finite() && room > 0.15 {
                    let dia = round3((room * 0.8).min(6.0));
                    add("face.hole", Act::Run("solid.hole", json!({"position": pt(p), "diameter": dia})));
                }
                // Draft the walls square to the face, the face neutral (as the Draft dialog sends a picked face).
                let walls: Vec<Value> = edges_of(i)
                    .into_iter()
                    .filter_map(|e| mesh.edge_faces.get(e))
                    .flat_map(|fs| fs.iter().map(|f| *f as usize).collect::<Vec<_>>())
                    .filter(|f| *f != i)
                    .collect::<BTreeSet<usize>>()
                    .into_iter()
                    .filter_map(|f| planes.get(f).copied().flatten().filter(|(_, m)| m.dot(n).abs() < 1e-6).map(|(q, _)| pt(q)))
                    .collect();
                if !walls.is_empty() {
                    add(
                        "face.draft_neutral",
                        Act::Run("solid.draft", json!({"faces": walls, "angle": "2 deg", "neutral": {"origin": pt(p), "normal": pt(n)}})),
                    );
                }
                add("face.shell", Act::Run("solid.shell", json!({"faces": [pt(p)], "thickness": round3((size * 0.01).clamp(0.1, 0.5))})));
            } else {
                let curved = matches!(
                    face_surface(&b.body, i),
                    Some(FaceSurface::Cylinder { .. } | FaceSurface::Cone { .. } | FaceSurface::Torus { .. } | FaceSurface::Sphere { .. })
                );
                if !curved {
                    continue;
                }
                let round = !matches!(face_surface(&b.body, i), Some(FaceSurface::Sphere { .. }));
                if round {
                    out.push(Action {
                        entity: entity.clone(),
                        action: "curved.axis",
                        act: Act::Run("construct.axis.through_cylinder", json!({"face": {"face": pt(p)}})),
                    });
                }
                out.push(Action {
                    entity: entity.clone(),
                    action: "curved.tangent_plane",
                    act: Act::Run("construct.plane.tangent", json!({"face": {"face": pt(p)}})),
                });
            }
        }
        // Edges.
        let pickable: Vec<usize> = (0..mesh.edges.len()).filter(|e| mesh.edge_pickable(*e)).collect();
        let ends = |e: usize| -> Vec<Vec3> {
            match mesh.edges.get(e) {
                Some(pts) if !closed(pts) => [pts.first(), pts.last()].into_iter().flatten().copied().collect(),
                _ => Vec::new(),
            }
        };
        let near = |a: Vec3, b: Vec3| a.dist(b) < 1e-6 * size.max(1.0);
        // The surface normal of face `f` at the mesh vertex nearest `p`.
        let normal_near = |f: u32, p: Vec3| -> Option<Vec3> {
            mesh.triangles
                .iter()
                .zip(&mesh.tri_face)
                .filter(|(_, tf)| **tf == f)
                .flat_map(|(t, _)| t.iter().copied())
                .filter_map(|v| Some((*mesh.positions.get(v as usize)?, *mesh.normals.get(v as usize)?)))
                .min_by(|a, b| a.0.dist(p).total_cmp(&b.0.dist(p)))
                .and_then(|(_, n)| n.normalized())
        };
        // The surface's normal at `p` (either way round): exact on analytic faces.
        let normal_at = |f: u32, p: Vec3| -> Option<Vec3> {
            match face_surface(&b.body, f as usize) {
                Some(FaceSurface::Plane { normal, .. }) => Some(*normal),
                Some(FaceSurface::Cylinder { origin, axis, .. }) => {
                    let v = p - origin;
                    (v - axis * v.dot(axis)).normalized()
                }
                Some(FaceSurface::Cone { apex, axis, half_angle }) => {
                    let v = p - apex;
                    let radial = (v - axis * v.dot(axis)).normalized()?;
                    (radial * half_angle.cos() - axis * half_angle.sin()).normalized()
                }
                Some(FaceSurface::Sphere { center, .. }) => (p - center).normalized(),
                Some(FaceSurface::Torus { center, axis, major, .. }) => {
                    let q = p - center;
                    let ring = center + (q - axis * q.dot(axis)).normalized()? * major;
                    (p - ring).normalized()
                }
                None => normal_near(f, p),
            }
        };
        // A smooth (tangent) edge, such as where a round meets a wall: nothing to blend.
        let smooth = |e: usize, mid: Vec3| -> bool {
            let Some([a, b]) = mesh.edge_faces.get(e).and_then(|fs| <[u32; 2]>::try_from(fs.as_slice()).ok()) else { return false };
            matches!((normal_at(a, mid), normal_at(b, mid)), (Some(na), Some(nb)) if na.dot(nb).abs() > (2f64).to_radians().cos())
        };
        let mut ordered = pickable.clone();
        ordered.sort_by_cached_key(|e| {
            (edge_names.get(*e).cloned().unwrap_or_default(), mesh.edges.get(*e).map(|p| place(polyline_mid(p))).unwrap_or_default())
        });
        for &e in ordered.iter().take(MAX_EDGES) {
            let Some(pts) = mesh.edges.get(e) else { continue };
            if !pts.iter().all(|p| p.is_finite()) {
                continue;
            }
            let entity = format!("{world}/{}", edge_names.get(e).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("edge{e}")));
            let mid = polyline_mid(pts);
            let own = if closed(pts) { polyline_len(pts) / std::f64::consts::PI } else { polyline_len(pts) };
            let my_ends = ends(e);
            let shortest = pickable
                .iter()
                .filter(|&&o| o != e && ends(o).iter().any(|q| my_ends.iter().any(|m| near(*m, *q))))
                .filter_map(|&o| mesh.edges.get(o).map(|p| polyline_len(p)))
                .fold(own, f64::min);
            let r = round3((shortest * 0.1).max(0.01));
            let mut add = |action, act| out.push(Action { entity: entity.clone(), action, act });
            if !smooth(e, mid) {
                // The midpoint is interior to this edge, but other bodies (including component
                // copies in their local frames) can have coincident edges. Retain the picked body.
                add("edge.fillet", Act::Run("solid.fillet", json!({"body": world, "edges": [pt(mid)], "radius": r})));
                add("edge.chamfer", Act::Run("solid.chamfer", json!({"body": world, "edges": [pt(mid)], "distance": r})));
            }
            let dir = straight(pts);
            if dir.is_some() {
                add("edge.axis", Act::Run("construct.axis.edge", json!({"edge": {"edge": pt(mid)}})));
            }
            add("edge.normal_plane", Act::Run("construct.plane.along_path", json!({"path": {"edge": pt(mid)}, "t": 0.5})));
            // The origin plane the edge shows longest in.
            let spread = |n: Vec3| pts.windows(2).map(|w| ((w[1] - w[0]) - n * (w[1] - w[0]).dot(n)).len()).sum::<f64>();
            let plane = [("XY", Vec3::Z), ("XZ", Vec3::Y), ("YZ", Vec3::X)]
                .into_iter()
                .max_by(|a, b| spread(a.1).total_cmp(&spread(b.1)))
                .map(|(n, _)| n)
                .unwrap_or("XY");
            add("edge.project", Act::Project { plane, edge: json!({"edge": pt(mid), "body": world}) });
        }
        // Vertices: the ends of open edges, each with the edges at it.
        let mut verts: Vec<(Vec3, Vec<usize>)> = Vec::new();
        for &e in &pickable {
            for q in ends(e) {
                match verts.iter_mut().find(|(v, _)| near(*v, q)) {
                    Some((_, es)) => es.push(e),
                    None => verts.push((q, vec![e])),
                }
            }
        }
        // Named by the faces around it; in a fixed order (name, then place), so the cap and the
        // numbering of repeated names don't depend on the kernel's edge order.
        let mut named: Vec<(String, Vec3, Vec<usize>)> = verts
            .into_iter()
            .map(|(v, mut es)| {
                es.sort_by_cached_key(|e| {
                    (edge_names.get(*e).cloned().unwrap_or_default(), mesh.edges.get(*e).map(|p| place(polyline_mid(p))).unwrap_or_default())
                });
                let mut around: Vec<String> = es
                    .iter()
                    .filter_map(|e| mesh.edge_faces.get(*e))
                    .flatten()
                    .map(|f| names.get(*f as usize).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("face{f}")))
                    .collect();
                around.sort();
                around.dedup();
                (format!("{world}/v[{}]", around.join(" & ")), v, es)
            })
            .collect();
        named.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| place(a.1).cmp(&place(b.1))));
        for (entity, v, es) in named.iter().take(MAX_VERTICES) {
            let (v, entity) = (v, entity.clone());
            out.push(Action {
                entity: entity.clone(),
                action: "vertex.point",
                act: Act::Run("construct.point.vertex", json!({"point": {"vertex": pt(*v)}})),
            });
            // Two neighbours along its edges, not in line with it.
            let others: Vec<Vec3> = es.iter().flat_map(|e| ends(*e)).filter(|q| !near(*q, *v)).collect();
            let pair = others
                .iter()
                .enumerate()
                .find_map(|(k, a)| others.iter().skip(k + 1).find(|b| (*a - *v).cross(**b - *v).len() > 1e-6 * size * size).map(|b| (*a, *b)));
            if let Some((a, c)) = pair {
                out.push(Action {
                    entity,
                    action: "vertex.plane_three_points",
                    act: Act::Run("construct.plane.three_points", json!({"a": {"vertex": pt(*v)}, "b": {"vertex": pt(a)}, "c": {"vertex": pt(c)}})),
                });
            }
        }
        // The body.
        let entity = format!("{world}/body");
        out.push(Action {
            entity: entity.clone(),
            action: "body.move",
            act: Act::Run("solid.move", json!({"bodies": [world], "translate": [10, 0, 0]})),
        });
        out.push(Action {
            entity: entity.clone(),
            action: "body.copy",
            act: Act::Run("solid.move", json!({"bodies": [world], "translate": [round3(size * 1.2), 0, 0], "copy": true})),
        });
        out.push(Action { entity: entity.clone(), action: "body.mirror", act: Act::Run("solid.mirror", json!({"bodies": [world], "plane": "YZ"})) });
        out.push(Action {
            entity,
            action: "body.pattern",
            act: Act::Run("solid.pattern.rectangular", json!({"bodies": [world], "dir1": [1, 0, 0], "count1": 2, "spacing1": round3(size * 1.2)})),
        });
    }
    // Keys must be unique: number repeats (names repeat when naming is ambiguous).
    let mut seen: BTreeMap<(String, &'static str), usize> = BTreeMap::new();
    for a in &mut out {
        let n = seen.entry((a.entity.clone(), a.action)).or_insert(0);
        *n += 1;
        if *n > 1 {
            a.entity = format!("{} ({})", a.entity, n);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Running them

/// Errors in the timeline (feature id → message).
fn errors(s: &Session) -> BTreeMap<u64, String> {
    s.model.results.iter().filter_map(|r| r.error.as_ref().map(|e| (r.id, format!("{}: {e}", r.name)))).collect()
}

fn command_kind(e: &solidcraft_engine::EngineError) -> &'static str {
    use solidcraft_engine::EngineError as E;
    match e {
        E::UnknownCommand(_) => "unknown_command",
        E::Disabled(..) => "disabled",
        E::BadParams { .. } => "bad_params",
        E::Internal(..) => "panic",
        _ => "error",
    }
}

/// Run an action on `t`; Ok, or (error kind, message).
fn perform(t: &mut Session, act: &Act, before: &BTreeMap<u64, String>) -> Result<(), (String, String)> {
    let start = t.doc.features.len();
    let run = |t: &mut Session, id: &str, p: &Value| t.execute(id, p).map_err(|e| (command_kind(&e).to_string(), e.to_string()));
    match act {
        Act::Run(id, p) => {
            run(t, id, p)?;
        }
        Act::Project { plane, edge } => {
            run(t, "sketch.create", &json!({"plane": plane}))?;
            let r = run(t, "sketch.project", &json!({"refs": [edge]}))?;
            let made = r.pointer("/created/curves").and_then(Value::as_array).map_or(0, Vec::len)
                + r.pointer("/created/points").and_then(Value::as_array).map_or(0, Vec::len);
            if made == 0 {
                return Err(("nothing_projected".into(), "sketch.project added nothing".into()));
            }
        }
    }
    if t.active_sketch.is_some() {
        run(t, "sketch.finish", &json!({}))?;
    }
    check(t, start, before)
}

/// The features added since `start` resolve, and the timeline has no new errors.
fn check(t: &Session, start: usize, before: &BTreeMap<u64, String>) -> Result<(), (String, String)> {
    let placed = t.placed_document();
    for f in t.doc.features.iter().skip(start) {
        if matches!(f.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. }) {
            match placed.sketch_face_attachment(f.id, &t.model) {
                FaceAttachmentStatus::Lost { message, .. } => return Err(("attachment_lost".into(), message)),
                FaceAttachmentStatus::Legacy { .. } => {
                    return Err((
                        "attachment_legacy".into(),
                        "its sketch is a legacy face sketch (found again by a point, not by an exact source)".into(),
                    ));
                }
                _ => {}
            }
        }
    }
    let now = errors(t);
    if let Some((_, e)) = now.iter().find(|(id, _)| !before.contains_key(id)) {
        return Err(("timeline_error".into(), e.clone()));
    }
    Ok(())
}

/// A parameter whose change (×1.1) leaves the design without new errors: the edit round's.
fn driving_parameter(s: &Session) -> Option<(String, String)> {
    let before = errors(s);
    let mut params: Vec<_> = s.doc.params.iter().filter(|p| !p.unit.is_empty()).collect();
    // User parameters first, then the ones features made.
    params.sort_by_key(|p| p.model);
    for p in params.into_iter().take(6) {
        let expr = format!("({}) * 1.1", p.expr);
        let mut t = s.scratch();
        if t.execute("parameters.change", &json!({"name": p.name, "expression": expr})).is_ok() && errors(&t).keys().all(|k| before.contains_key(k)) {
            return Some((p.name.clone(), expr));
        }
    }
    None
}

fn stable_hash(s: &str) -> u64 {
    // FNV-1a.
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

fn key(design: &str, entity: &str, action: &str) -> String {
    format!("{design} :: {entity} :: {action}")
}

/// An entity's name without piece numbers (`#2`, `~3`, ` (2)`): pieces that share a name are
/// numbered by where they lie, and the numbers of pieces lying almost level can swap between runs
/// (issue #569), so the ratchet counts passes per family of pieces.
fn family(entity: &str) -> String {
    let mut out = String::with_capacity(entity.len());
    let mut chars = entity.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if (c == '#' || c == '~' || c == '(') && chars.peek().is_some_and(char::is_ascii_digit) {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            out.push('n');
        }
    }
    out
}

/// One outcome, as a JSON line.
fn outcome(design: &str, entity: &str, action: &str, r: &Result<(), (String, String)>, ms: u128) -> Value {
    match r {
        Ok(()) => json!({"design": design, "entity": entity, "action": action, "ok": true, "ms": ms}),
        Err((kind, msg)) => {
            let msg: String = msg.chars().take(400).collect();
            json!({"design": design, "entity": entity, "action": action, "ok": false, "kind": kind, "message": msg, "ms": ms})
        }
    }
}

/// The child: run design `ACTION_MATRIX_DESIGN`'s actions from index `ACTION_MATRIX_FROM`,
/// writing `{"start": i}` before and an outcome after each one to `ACTION_MATRIX_OUT`.
#[test]
#[ignore = "run by the action matrix tests, one design per process"]
fn action_matrix_worker() {
    let (Ok(design), Ok(out)) = (std::env::var("ACTION_MATRIX_DESIGN"), std::env::var("ACTION_MATRIX_OUT")) else { return };
    let from: usize = std::env::var("ACTION_MATRIX_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sample: u64 = std::env::var("ACTION_MATRIX_SAMPLE").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
    let mut out = std::fs::OpenOptions::new().create(true).append(true).open(out).unwrap();
    let mut emit = |v: Value| {
        writeln!(out, "{v}").unwrap();
        out.flush().unwrap();
    };
    let Some((_, path)) = designs().into_iter().find(|(n, _)| *n == design) else { panic!("no design {design}") };
    // A slice of a heavy design (`j/n`): the actions whose name hashes to `j` modulo `n` (by
    // index, slices would take whole kinds of action when n shares a factor with the number of
    // actions per entity, and a slice of only fillets runs three times as long); the first
    // slice records the load.
    let slice: Option<(usize, usize)> = std::env::var("ACTION_MATRIX_SLICE").ok().and_then(|v| parse_part(&v));
    let first = slice.is_none_or(|(j, _)| j == 0);
    let t0 = Instant::now();
    let s = match load(&path) {
        Ok(s) => s,
        Err(e) => {
            if first {
                emit(outcome(&design, "design", "load", &Err(("load".into(), e)), t0.elapsed().as_millis()));
            }
            emit(json!({"done": true}));
            return;
        }
    };
    let before = errors(&s);
    if from == 0 && first {
        let r = if before.is_empty() { Ok(()) } else { Err(("timeline_error".into(), before.values().cloned().collect::<Vec<_>>().join("; "))) };
        emit(outcome(&design, "design", "load", &r, t0.elapsed().as_millis()));
    }
    let list = actions(&s);
    let edit = driving_parameter(&s);
    // Local runs only: `ACTION_MATRIX_ACTIONS=edge.fillet,edge.chamfer` tries just those actions
    // (don't bless such a run), `ACTION_MATRIX_VALIDITY=1` adds each passing action's body check.
    let only: Option<Vec<String>> = std::env::var("ACTION_MATRIX_ACTIONS").ok().map(|v| v.split(',').map(str::to_string).collect());
    let validity = std::env::var_os("ACTION_MATRIX_VALIDITY").is_some();
    for (i, a) in list.iter().enumerate().skip(from) {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == a.action)) {
            continue;
        }
        if slice.is_some_and(|(j, n)| stable_hash(&format!("{} {}", a.entity, a.action)) % n as u64 != j as u64) {
            continue;
        }
        // Whole families of pieces are sampled together (see [`family`]).
        let k = key(&design, &family(&a.entity), a.action);
        if !stable_hash(&k).is_multiple_of(sample) {
            continue;
        }
        emit(json!({"start": i}));
        let t = Instant::now();
        let mut scratch = s.scratch();
        let r = perform(&mut scratch, &a.act, &before);
        let mut line = outcome(&design, &a.entity, a.action, &r, t.elapsed().as_millis());
        if validity && r.is_ok() {
            let st = scratch.model.state();
            let bad: Vec<String> = st.bodies.iter().flat_map(|b| b.body.validity().into_iter().map(|v| format!("{}: {v}", b.name))).collect();
            line["validity"] = json!(bad);
        }
        // What was sent, for reproducing a failure (kept in the results, not the baseline).
        line["command"] = match &a.act {
            Act::Run(id, p) => json!({"command": id, "params": p}),
            Act::Project { plane, edge } => json!({"command": "sketch.project", "params": {"refs": [edge]}, "sketch_plane": plane}),
        };
        emit(line);
        // With ACTION_MATRIX_UNFOLD, sheet metal must still unfold and fold back after the action.
        if r.is_ok() && std::env::var_os("ACTION_MATRIX_UNFOLD").is_some() {
            let t = Instant::now();
            let sheets: Vec<String> = scratch.model.state().sheets.iter().map(|sh| sh.body.clone()).collect();
            let mut r = Ok(());
            for body in &sheets {
                for id in ["sheet.unfold", "sheet.refold"] {
                    if r.is_ok() {
                        r = scratch
                            .execute(id, &json!({"body": body}))
                            .map(|_| ())
                            .map_err(|e| (command_kind(&e).to_string(), format!("{body}: {e}")));
                    }
                }
            }
            if !sheets.is_empty() {
                let unfold: &'static str = Box::leak(format!("{}+unfold", a.action).into_boxed_str());
                emit(outcome(&design, &a.entity, unfold, &r, t.elapsed().as_millis()));
            }
        }
        // The edit round: a driving parameter changes; the new feature still resolves.
        if r.is_ok()
            && let Some((name, expr)) = &edit
            && stable_hash(&format!("{k} edit")).is_multiple_of(EDIT_SAMPLE)
        {
            // Marked like an action, so a hang here is put down to the edit round (not to loading).
            emit(json!({"start": i, "edit": true}));
            let t = Instant::now();
            let start = s.doc.features.len();
            let r = scratch
                .execute("parameters.change", &json!({"name": name, "expression": expr}))
                .map_err(|e| (command_kind(&e).to_string(), e.to_string()))
                .and_then(|_| check(&scratch, start, &before));
            let edited: &'static str = Box::leak(format!("{}+edit", a.action).into_boxed_str());
            emit(outcome(&design, &a.entity, edited, &r, t.elapsed().as_millis()));
        }
    }
    emit(json!({"done": true, "actions": list.len()}));
}

/// `"k/n"` (1-based `k`) as `(k - 1, n)`.
fn parse_part(v: &str) -> Option<(usize, usize)> {
    let (k, n) = v.split_once('/')?;
    let (k, n): (usize, usize) = (k.trim().parse().ok()?, n.trim().parse().ok()?);
    (k >= 1 && k <= n).then_some((k - 1, n))
}

/// What one design costs on a CI runner (`tests/action_matrix/costs.json`, measured by the
/// nightly shards): the whole run and the load before its first action, in seconds.
#[derive(Clone, Copy, Debug)]
struct Cost {
    secs: f64,
    load: f64,
}

fn costs_path() -> PathBuf {
    root().join("tests/action_matrix/costs.json")
}

fn read_costs() -> BTreeMap<String, Cost> {
    let Some(v) = std::fs::read(costs_path()).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return BTreeMap::new() };
    v["designs"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(d, c)| (d.clone(), Cost { secs: c["secs"].as_f64().unwrap_or(0.0), load: c["load"].as_f64().unwrap_or(0.0) }))
        .collect()
}

/// A design new to the corpus (not in costs.json yet) is planned as this.
const UNKNOWN_COST: Cost = Cost { secs: 120.0, load: 5.0 };
/// A design that takes longer than this on a runner is split into slices of about this much, or
/// five loads if that is more (each slice loads the design again), so no shard waits on one design.
const SLICE_SECS: f64 = 600.0;
/// Designs that take longer than this to load on a runner stay out of the pull request sample.
const SAMPLE_MAX_LOAD_SECS: f64 = 8.0;

/// A piece of work: one design, or a slice `(j, n)` of a heavy one, and what it should cost.
#[derive(Clone, Debug)]
struct Unit {
    design: String,
    slice: Option<(usize, usize)>,
    cost: f64,
}

impl Unit {
    fn name(&self) -> String {
        match self.slice {
            Some((j, n)) => format!("{} [{}/{n}]", self.design, j + 1),
            None => self.design.clone(),
        }
    }
}

/// The whole matrix split into `parts` shards of about equal cost: heavy designs in slices, then
/// the largest pieces first, each to the shard with the least so far. The same corpus and
/// costs.json always give the same shards.
fn plan(designs: &[String], costs: &BTreeMap<String, Cost>, parts: usize) -> Vec<Vec<Unit>> {
    let mut units = Vec::new();
    for d in designs {
        let c = costs.get(d).copied().unwrap_or(UNKNOWN_COST);
        let n = ((c.secs / SLICE_SECS.max(5.0 * c.load)).ceil() as usize).max(1);
        if n == 1 {
            units.push(Unit { design: d.clone(), slice: None, cost: c.secs });
        } else {
            let each = c.load + (c.secs - c.load).max(0.0) / n as f64;
            units.extend((0..n).map(|j| Unit { design: d.clone(), slice: Some((j, n)), cost: each }));
        }
    }
    units.sort_by(|a, b| b.cost.total_cmp(&a.cost).then_with(|| a.name().cmp(&b.name())));
    let mut shards: Vec<(f64, Vec<Unit>)> = vec![(0.0, Vec::new()); parts];
    for u in units {
        let i = (0..parts).min_by(|&a, &b| shards[a].0.total_cmp(&shards[b].0)).unwrap_or(0);
        shards[i].0 += u.cost;
        shards[i].1.push(u);
    }
    shards.into_iter().map(|(_, v)| v).collect()
}

/// Every design's name, with `ACTION_MATRIX_FILTER` applied.
fn design_names() -> Vec<String> {
    let designs = designs();
    assert!(designs.len() > 80, "only {} designs", designs.len());
    let filter = std::env::var("ACTION_MATRIX_FILTER").ok();
    designs.into_iter().map(|(n, _)| n).filter(|n| filter.as_deref().is_none_or(|f| n.contains(f))).collect()
}

/// Run the actions (one in `sample`) of these units in child processes, `jobs` at a time.
///
/// `ACTION_MATRIX_WORKDIR` keeps each design's raw outcomes there (one JSON line each, written as
/// they come, so a run cut short still shows what it did) and `ACTION_MATRIX_TIMINGS` appends
/// each unit's time to that file (JSON lines).
fn run_units(sample: u64, queue: &[Unit]) -> Vec<Value> {
    let keep = std::env::var_os("ACTION_MATRIX_WORKDIR").map(PathBuf::from);
    static RUN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = keep.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("action-matrix-{}-{run}", std::process::id())));
    std::fs::create_dir_all(&dir).unwrap();
    let timings = std::env::var_os("ACTION_MATRIX_TIMINGS").map(PathBuf::from);
    let jobs: usize = std::env::var("ACTION_MATRIX_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get()))
        .max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                while let Some(u) = queue.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                    let file = u.name().replace('/', "__").replace([' ', '[', ']'], "_");
                    let out = dir.join(format!("{file}.r{run}.jsonl"));
                    let t = Instant::now();
                    let lines = run_design(&u.design, &out, sample, u.slice);
                    let secs = t.elapsed().as_secs_f64();
                    let load = lines.iter().find(|o| o["entity"] == "design" && o["action"] == "load").and_then(|o| o["ms"].as_f64()).unwrap_or(0.0) / 1000.0;
                    if let Some(p) = &timings {
                        let line = json!({"design": u.design, "slice": u.slice.map(|(j, n)| format!("{}/{n}", j + 1)), "secs": round3(secs), "load": round3(load), "actions": lines.len()});
                        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
                        writeln!(f, "{line}").unwrap();
                    }
                    eprintln!("action matrix: {} took {secs:.1} s ({} outcomes, load {load:.1} s)", u.name(), lines.len());
                    results.lock().unwrap().extend(lines);
                }
            });
        }
    });
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let mut v = results.into_inner().unwrap();
    v.sort_by_key(|o| key(o["design"].as_str().unwrap_or(""), o["entity"].as_str().unwrap_or(""), o["action"].as_str().unwrap_or("")));
    v
}

/// Run the actions (one in `sample`) of every design (the pull request sample leaves out the
/// slow-loading ones).
fn run_matrix(sample: u64) -> Vec<Value> {
    let costs = read_costs();
    let queue: Vec<Unit> = design_names()
        .into_iter()
        .filter(|n| sample == 1 || !(SLOW.contains(&n.as_str()) || costs.get(n).is_some_and(|c| c.load > SAMPLE_MAX_LOAD_SECS)))
        .map(|design| Unit { design, slice: None, cost: 0.0 })
        .collect();
    run_units(sample, &queue)
}

/// One design in child processes, restarting after an action that hangs or aborts.
fn run_design(design: &str, out: &Path, sample: u64, slice: Option<(usize, usize)>) -> Vec<Value> {
    let _ = std::fs::remove_file(out);
    let mut from = 0usize;
    let mut results = Vec::new();
    let mut extra = Vec::new();
    for _restart in 0..200 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["action_matrix_worker", "--exact", "--ignored", "--nocapture", "--test-threads", "1"])
            .env("ACTION_MATRIX_DESIGN", design)
            .env("ACTION_MATRIX_OUT", out)
            .env("ACTION_MATRIX_FROM", from.to_string())
            .env("ACTION_MATRIX_SAMPLE", sample.to_string())
            .env("ACTION_MATRIX_SLICE", slice.map(|(j, n)| format!("{}/{n}", j + 1)).unwrap_or_default())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // Until the child writes, it is loading the design (again, after a restart).
        let loaded_at = std::fs::metadata(out).map(|m| m.len() as usize).unwrap_or(0);
        let mut seen = loaded_at;
        let mut last_change = Instant::now();
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break (Some(status), false);
            }
            let n = std::fs::metadata(out).map(|m| m.len() as usize).unwrap_or(0);
            if n != seen {
                seen = n;
                last_change = Instant::now();
            } else if last_change.elapsed() > if seen == loaded_at { LOAD_TIMEOUT } else { ACTION_TIMEOUT } {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        // Read what it wrote.
        let lines: Vec<Value> = std::fs::File::open(out)
            .map(|f| std::io::BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect())
            .unwrap_or_default();
        let done = lines.iter().any(|l| l.get("done").is_some());
        let last = lines.iter().rev().take_while(|l| l.get("start").is_some() || l.get("design").is_none()).find(|l| l.get("start").is_some());
        let last_start = last.and_then(|l| l["start"].as_u64());
        let in_edit = last.is_some_and(|l| l["edit"] == true);
        results = lines.iter().filter(|l| l.get("design").is_some()).cloned().collect();
        if done {
            break;
        }
        // It stopped inside action `last_start`: record it and go on after it.
        let Some(i) = last_start else {
            if slice.is_some_and(|(j, _)| j > 0) {
                break;
            }
            extra.push(
                json!({"design": design, "entity": "design", "action": "load", "ok": false, "kind": if timed_out { "timeout" } else { "crash" },
                              "message": format!("the design did not load ({status:?})")}),
            );
            break;
        };
        // Which action was it? The child enumerates them; ask it by running from there with a
        // marker: simplest is to record the index and let the next run name it.
        extra.push(json!({"design": design, "index": i, "edit": in_edit, "kind": if timed_out { "timeout" } else { "crash" },
                          "message": if timed_out { format!("no result after {} s", ACTION_TIMEOUT.as_secs()) } else { format!("the process ended: {status:?}") }}));
        from = i as usize + 1;
    }
    // Name the actions that hung or crashed (their index in the design's list).
    if extra.iter().any(|e| e.get("index").is_some()) {
        let names = action_names(design);
        for e in &mut extra {
            if let Some(i) = e.get("index").and_then(Value::as_u64) {
                let (entity, mut action) = names.get(i as usize).cloned().unwrap_or_else(|| (format!("action{i}"), "unknown".into()));
                if e["edit"] == true {
                    action += "+edit";
                }
                *e = json!({"design": design, "entity": entity, "action": action, "ok": false, "kind": e["kind"], "message": e["message"]});
            }
        }
    }
    results.extend(extra);
    results
}

/// The design's action list (entity, action), by index: for naming one that hung.
fn action_names(design: &str) -> Vec<(String, String)> {
    let Some((_, path)) = designs().into_iter().find(|(n, _)| n == design) else { return Vec::new() };
    let Ok(s) = load(&path) else { return Vec::new() };
    actions(&s).into_iter().map(|a| (a.entity, a.action.to_string())).collect()
}

// ---------------------------------------------------------------------------------------------
// The baseline

/// The baseline: design → entity → the actions tried on it, a failing one marked `!`.
fn read_baseline() -> BTreeMap<String, bool> {
    let Some(v) = std::fs::read(baseline_path()).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return BTreeMap::new() };
    let mut out = BTreeMap::new();
    for (design, entities) in v["designs"].as_object().into_iter().flatten() {
        for (entity, actions) in entities.as_object().into_iter().flatten() {
            for a in actions.as_str().unwrap_or("").split_whitespace() {
                let (ok, action) = match a.strip_prefix('!') {
                    Some(action) => (false, action),
                    None => (true, a),
                };
                out.insert(key(design, entity, action), ok);
            }
        }
    }
    out
}

fn write_baseline(results: &[Value]) {
    // Designs this run didn't try (ACTION_MATRIX_FILTER) keep their lines.
    let old: Value = std::fs::read(baseline_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let ran: BTreeSet<&str> = results.iter().filter_map(|o| o["design"].as_str()).collect();
    let mut m: BTreeMap<&str, BTreeMap<&str, Vec<String>>> = BTreeMap::new();
    for (design, entities) in old["designs"].as_object().into_iter().flatten().filter(|(d, _)| !ran.contains(d.as_str())) {
        for (entity, actions) in entities.as_object().into_iter().flatten() {
            m.entry(design).or_default().entry(entity).or_default().extend(actions.as_str().unwrap_or("").split_whitespace().map(str::to_string));
        }
    }
    // A timeout of an action that passed keeps it passing (see [`ratchet`]).
    let base = read_baseline();
    for o in results {
        let (design, entity, action) = (o["design"].as_str().unwrap_or(""), o["entity"].as_str().unwrap_or(""), o["action"].as_str().unwrap_or(""));
        let kept = o["kind"] == "timeout" && base.get(&key(design, entity, action)) == Some(&true);
        let mark = if o["ok"].as_bool() == Some(true) || kept { "" } else { "!" };
        m.entry(design).or_default().entry(entity).or_default().push(format!("{mark}{action}"));
    }
    // One entity per line, sorted: small diffs when a few actions change.
    let mut s = String::from(
        "{\n \"about\": \"The action matrix's ratchet: design, entity, the actions tried on it (failing ones marked !). apps/solidcraft-cli/tests/action_matrix.rs, docs/action-matrix.md. Update with cargo xtask action-matrix --bless.\",\n \"designs\": {\n",
    );
    let designs = m.len();
    for (i, (design, entities)) in m.into_iter().enumerate() {
        s += &format!("  {}: {{\n", json!(design));
        let n = entities.len();
        for (j, (entity, mut actions)) in entities.into_iter().enumerate() {
            actions.sort_by(|a, b| a.trim_start_matches('!').cmp(b.trim_start_matches('!')));
            s += &format!("   {}: {}{}\n", json!(entity), json!(actions.join(" ")), if j + 1 < n { "," } else { "" });
        }
        s += &format!("  }}{}\n", if i + 1 < designs { "," } else { "" });
    }
    s += " }\n}\n";
    std::fs::create_dir_all(baseline_path().parent().unwrap()).unwrap();
    std::fs::write(baseline_path(), s).unwrap();
}

/// Compare with the baseline: fail on regressions, report new passes.
fn ratchet(results: &[Value], full: bool) {
    let base = read_baseline();
    // Passes per family of pieces (design, entity without piece numbers, action): in the
    // baseline, over the same names this run tried, and in this run.
    let mut fam: BTreeMap<String, (usize, usize, Vec<String>, Vec<String>)> = BTreeMap::new();
    let mut fresh = 0usize;
    // An action that passed in the baseline and now runs past ACTION_TIMEOUT is listed but not
    // judged: on a busy runner the slowest actions (40-80 s on oracle 41's tube) land either side
    // of the limit from one night to the next. An error or a crash is judged as usual.
    let mut slow = Vec::new();
    for o in results {
        let (design, entity, action) = (o["design"].as_str().unwrap_or(""), o["entity"].as_str().unwrap_or(""), o["action"].as_str().unwrap_or(""));
        let k = key(design, entity, action);
        let ok = o["ok"].as_bool() == Some(true);
        if o["kind"] == "timeout" && base.get(&k) == Some(&true) {
            slow.push(k);
            continue;
        }
        let f = fam.entry(key(design, &family(entity), action)).or_default();
        match base.get(&k) {
            Some(was) => {
                f.0 += usize::from(*was);
                f.1 += usize::from(ok);
                if !ok {
                    f.2.push(format!("{k}: {}: {}", o["kind"].as_str().unwrap_or(""), o["message"].as_str().unwrap_or("")));
                }
                if !was && ok {
                    f.3.push(k);
                }
            }
            None => fresh += 1,
        }
    }
    let mut regressions = Vec::new();
    let mut fixed = Vec::new();
    for (was, now, failing, passing) in fam.into_values() {
        if now < was {
            regressions.extend(failing);
        } else if now > was {
            fixed.extend(passing);
        }
    }
    let passed = results.iter().filter(|o| o["ok"].as_bool() == Some(true)).count();
    eprintln!("action matrix: {passed} of {} actions pass ({} in the baseline)", results.len(), base.len());
    if std::env::var_os("ACTION_MATRIX_RESULTS").is_some() {
        let p = PathBuf::from(std::env::var_os("ACTION_MATRIX_RESULTS").unwrap_or_default());
        std::fs::write(&p, serde_json::to_string(&results).unwrap()).unwrap();
        eprintln!("results written to {}", p.display());
    }
    if full && std::env::var_os("ACTION_MATRIX_BLESS").is_some() {
        write_baseline(results);
        eprintln!("baseline written: {}", baseline_path().display());
        return;
    }
    if !slow.is_empty() {
        eprintln!(
            "{} actions that pass in the baseline ran past the {} s limit this time (not judged):\n{}",
            slow.len(),
            ACTION_TIMEOUT.as_secs(),
            slow.join("\n")
        );
    }
    if !fixed.is_empty() || fresh > 0 {
        eprintln!(
            "{} actions pass now that failed in the baseline, and {fresh} are not in it: record them with `cargo xtask action-matrix --bless`:\n{}",
            fixed.len(),
            fixed.iter().take(50).cloned().collect::<Vec<_>>().join("\n")
        );
    }
    assert!(
        regressions.is_empty(),
        "{} actions that passed in tests/action_matrix/baseline.json fail now (if that is intended, record it with `cargo xtask action-matrix --bless` and say why in the pull request):\n{}",
        regressions.len(),
        regressions.join("\n")
    );
}

/// Pull requests: a fixed sample of the matrix (one action in [`PR_SAMPLE`]) against the ratchet.
#[test]
fn action_matrix_sample() {
    let results = run_matrix(PR_SAMPLE);
    assert!(results.len() > 300, "only {} actions ran", results.len());
    ratchet(&results, false);
}

/// One shard of the whole matrix against the ratchet: `ACTION_MATRIX_SHARD=k/n` (the nightly
/// "Action matrix" CI jobs run the n shards side by side, `--run-ignored only`). Shards are cut by
/// what each design costs (tests/action_matrix/costs.json), see [`plan`].
/// `ACTION_MATRIX_SHARD_LIST=1` only prints the plan.
#[test]
#[ignore = "heavy: the nightly Action matrix CI jobs run it, one shard each"]
fn action_matrix_shard() {
    let Some((k, n)) = std::env::var("ACTION_MATRIX_SHARD").ok().and_then(|v| parse_part(&v)) else { panic!("set ACTION_MATRIX_SHARD=k/n") };
    let shards = plan(&design_names(), &read_costs(), n);
    for (i, sh) in shards.iter().enumerate() {
        let total: f64 = sh.iter().map(|u| u.cost).sum();
        eprintln!("shard {}/{n}: {} pieces, about {:.0} s of work{}", i + 1, sh.len(), total, if i == k { " (this one)" } else { "" });
    }
    eprintln!("this shard: {}", shards[k].iter().map(Unit::name).collect::<Vec<_>>().join(", "));
    if std::env::var_os("ACTION_MATRIX_SHARD_LIST").is_some() {
        return;
    }
    let results = run_units(1, &shards[k]);
    ratchet(&results, false);
}

/// Determinism: a sample of the matrix (one family in [`DETERMINISM_SAMPLE`], the heaviest
/// designs left out) run twice must give the same outcome for every action. Lists each action
/// whose outcome changed between the runs.
#[test]
#[ignore = "heavy: the nightly Action matrix determinism CI job runs it"]
fn action_matrix_determinism() {
    let outcomes = |v: Vec<Value>| -> BTreeMap<String, String> {
        v.into_iter()
            .map(|o| {
                let k = key(o["design"].as_str().unwrap_or(""), o["entity"].as_str().unwrap_or(""), o["action"].as_str().unwrap_or(""));
                let r = if o["ok"].as_bool() == Some(true) {
                    "pass".to_string()
                } else {
                    format!("{}: {}", o["kind"].as_str().unwrap_or(""), o["message"].as_str().unwrap_or(""))
                };
                (k, r)
            })
            .collect()
    };
    let costs = read_costs();
    let queue: Vec<Unit> = design_names()
        .into_iter()
        .filter(|n| costs.get(n).copied().unwrap_or(UNKNOWN_COST).secs <= DETERMINISM_MAX_SECS)
        .map(|design| Unit { design, slice: None, cost: 0.0 })
        .collect();
    let a = outcomes(run_units(DETERMINISM_SAMPLE, &queue));
    let b = outcomes(run_units(DETERMINISM_SAMPLE, &queue));
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let mut changed = Vec::new();
    // Pieces that share a name can swap numbers between runs (#569): compare passes per family
    // too, and report a family whose pass count differs.
    let mut fam: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for k in &keys {
        let (x, y) = (a.get(*k).map_or("missing", String::as_str), b.get(*k).map_or("missing", String::as_str));
        let mut parts = k.splitn(3, " :: ");
        let (d, e, act) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let f = fam.entry(key(d, &family(e), act)).or_default();
        f.0 += usize::from(x == "pass");
        f.1 += usize::from(y == "pass");
        if x != y {
            changed.push(format!("{k}\n    first:  {x}\n    second: {y}"));
        }
    }
    let families: Vec<String> = fam.into_iter().filter(|(_, (x, y))| x != y).map(|(k, (x, y))| format!("{k}: {x} then {y} pass")).collect();
    eprintln!(
        "action matrix determinism: {} actions, {} changed between two runs, {} families pass a different number of times",
        keys.len(),
        changed.len(),
        families.len()
    );
    for c in &changed {
        eprintln!("  {c}");
    }
    assert!(
        families.is_empty(),
        "{} families of actions pass a different number of times in two runs of the same build:\n{}",
        families.len(),
        families.join("\n")
    );
}

/// The whole matrix at once (`cargo xtask action-matrix`): `ACTION_MATRIX_BLESS=1` rewrites the
/// baseline; `ACTION_MATRIX_RESULTS=file.json` keeps every outcome with its message.
/// `ACTION_MATRIX_MERGE=DIR` runs nothing and takes the outcomes from the `results-*.json` files
/// in DIR instead (the nightly shards' artifacts), and the timings from its `timings-*.jsonl`
/// files, which `ACTION_MATRIX_BLESS` then also writes to tests/action_matrix/costs.json.
#[test]
#[ignore = "cargo xtask action-matrix runs it (CI runs it as the action_matrix_shard jobs)"]
fn action_matrix_all() {
    let Some(dir) = std::env::var_os("ACTION_MATRIX_MERGE").map(PathBuf::from) else {
        let results = run_matrix(1);
        ratchet(&results, true);
        return;
    };
    let mut results: Vec<Value> = Vec::new();
    let mut timings: Vec<Value> = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    for f in files {
        let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        if name.starts_with("results-") && name.ends_with(".json") {
            results.extend(serde_json::from_str::<Vec<Value>>(&text).unwrap_or_else(|e| panic!("{name}: {e}")));
        } else if name.starts_with("timings-") {
            timings.extend(text.lines().filter_map(|l| serde_json::from_str(l).ok()));
        }
    }
    let ran: BTreeSet<&str> = results.iter().filter_map(|o| o["design"].as_str()).collect();
    let missing: Vec<String> = design_names().into_iter().filter(|d| !ran.contains(d.as_str())).collect();
    assert!(missing.is_empty(), "no results for {}", missing.join(", "));
    results.sort_by_key(|o| key(o["design"].as_str().unwrap_or(""), o["entity"].as_str().unwrap_or(""), o["action"].as_str().unwrap_or("")));
    if std::env::var_os("ACTION_MATRIX_BLESS").is_some() && !timings.is_empty() {
        write_costs(&timings);
    }
    ratchet(&results, true);
}

/// tests/action_matrix/costs.json from the shards' timings: per design, its whole time (a sliced
/// design's slices added up, less the loads all but one of them repeat) and its load.
fn write_costs(timings: &[Value]) {
    let mut m: BTreeMap<String, (f64, f64, usize)> = BTreeMap::new();
    for t in timings {
        let e = m.entry(t["design"].as_str().unwrap_or("").to_string()).or_default();
        e.0 += t["secs"].as_f64().unwrap_or(0.0);
        e.1 = e.1.max(t["load"].as_f64().unwrap_or(0.0));
        e.2 += 1;
    }
    // Designs these timings don't cover keep their old costs.
    for (d, c) in read_costs() {
        m.entry(d).or_insert((c.secs, c.load, 1));
    }
    let mut s = String::from(
        "{\n \"about\": \"What each action matrix design costs on a CI runner, in seconds (secs: the whole design, load: before its first action), for cutting the nightly shards (apps/solidcraft-cli/tests/action_matrix.rs, plan). Written with the baseline by cargo xtask action-matrix --bless --from DIR.\",\n \"designs\": {\n",
    );
    let n = m.len();
    for (i, (d, (secs, load, pieces))) in m.into_iter().enumerate() {
        let secs = (secs - load * pieces.saturating_sub(1) as f64).max(load);
        s += &format!("  {}: {{\"secs\": {:.1}, \"load\": {:.1}}}{}\n", json!(d), secs, load, if i + 1 < n { "," } else { "" });
    }
    s += " }\n}\n";
    std::fs::write(costs_path(), s).unwrap();
    eprintln!("costs written: {}", costs_path().display());
}

// ---------------------------------------------------------------------------------------------
// The matrix's causes: one small reproduction per open issue, ignored until it is fixed (the
// issue says how many matrix actions it accounts for). Each runs commands as the desktop sends
// them; every step must work and the timeline must have no errors.

fn reproduces(design: Option<&str>, commands: Value) {
    let mut s = match design {
        Some(d) => load(&root().join(d)).unwrap(),
        None => Session::default(),
    };
    for (i, c) in commands.as_array().unwrap().iter().enumerate() {
        let id = c["command"].as_str().unwrap();
        s.execute(id, &c["params"]).unwrap_or_else(|e| panic!("step {} ({id}): {e}", i + 1));
        if s.active_sketch.is_none() {
            check(&s, 0, &BTreeMap::new()).unwrap_or_else(|(kind, e)| panic!("step {} ({id}): {kind}: {e}", i + 1));
        }
    }
}

fn rectangle(plane: Value, p0: [f64; 2], p1: [f64; 2]) -> [Value; 3] {
    [
        json!({"command": "sketch.create", "params": {"plane": plane}}),
        json!({"command": "sketch.rectangle.two_point", "params": {"p0": p0, "p1": p1}}),
        json!({"command": "sketch.finish", "params": {}}),
    ]
}

#[test]
fn edge_round_actions_keep_the_picked_body() {
    for (corner, length) in [([10, 0, 20], 4), ([0, 0, 0], 20)] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "body_name": "Plate"})).unwrap();
        s.execute(
            "solid.box",
            &json!({"corner": corner, "length": length, "width": length, "height": length, "body_name": "Block", "operation": "new"}),
        )
        .unwrap();
        let mut tried = 0;
        for a in actions(&s) {
            if !a.entity.starts_with("Plate/") || !matches!(a.action, "edge.chamfer" | "edge.fillet") {
                continue;
            }
            let Act::Run(id, ref p) = a.act else { continue };
            let q = &p["edges"][0];
            let x = q[0].as_f64().unwrap();
            if q[1] != 0.0 || q[2] != 20.0 || !(0.0..20.0).contains(&x) {
                continue;
            }
            // Block either meets this midpoint or coincides with the whole edge.
            assert_eq!(p["body"], "Plate", "{}: {p}", a.action);
            assert_eq!(q, &json!([10.0, 0.0, 20.0]));
            let mut unscoped = p.clone();
            unscoped.as_object_mut().unwrap().remove("body");
            let err = s.scratch().execute(id, &unscoped).unwrap_err().to_string();
            assert!(err.contains("edges meet"), "{err}");
            perform(&mut s.scratch(), &a.act, &errors(&s)).unwrap();
            tried += 1;
        }
        assert_eq!(tried, 2);
    }
}

#[test]
fn a_fillet_or_chamfer_on_one_edge_ending_at_a_cylinder() {
    for (cmd, size) in [("solid.fillet", "radius"), ("solid.chamfer", "distance")] {
        reproduces(
            None,
            json!([
                {"command": "solid.box", "params": {"length": 40, "width": 40, "height": 20}},
                {"command": "solid.cylinder", "params": {"base": [40, 20, 0], "radius": 12, "height": 40, "operation": "join"}},
                {"command": cmd, "params": {"edges": [[40, 4, 0]], size: 0.5}}
            ]),
        );
    }
}

// #1255: a round on a knuckle's cap circle (oracle/63's hinge), 0.73 wide on a ring of radius
// 4. The run past each end of the circle followed the end tangent so far that its sections
// wrapped round the ring and the blend was refused; it is shortened now while it still
// crosses the end face. Before #1101 a 0.13 mm station on the leaf's top edge kept the
// matrix's radius at 0.013, which never got that far.
#[test]
fn a_round_on_a_knuckle_cap_circle() {
    for (cmd, size, removed) in [("solid.fillet", "radius", (2.4, 2.8)), ("solid.chamfer", "distance", (5.6, 6.3))] {
        let mut s = load(&root().join("tests/oracle/63-hinge-knuckle/recipe.json")).unwrap();
        let volume = |s: &Session| s.model.state().bodies.iter().map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.001)).unwrap().measure().volume).sum::<f64>();
        let before = volume(&s);
        s.execute(cmd, &json!({"edges": [[40.0, 29.837_722, 6.449_490]], size: 0.728})).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        check(&s, 0, &BTreeMap::new()).unwrap_or_else(|(kind, e)| panic!("{cmd}: {kind}: {e}"));
        let st = s.model.state();
        let body = &st.bodies.first().unwrap().body;
        assert!(body.validity().is_empty(), "{cmd}: {:?}", body.validity());
        let gone = before - volume(&s);
        assert!(gone > removed.0 && gone < removed.1, "{cmd} took {gone} mm³");
    }
}

// #1255: a hole 0.133 mm across on the 0.5 mm ledge a loft cut leaves in a pocket floor of a
// 900 mm slab. The Boolean needed a retry, and its guard meshed the drill as coarsely as the
// slab's size allows (0.4 mm, three times the drill's width): the mesh had no inside, the drill
// measured −30272 mm³ and the right cut was refused. Volumes are measured finer than each
// body's smallest radius now.
#[test]
fn a_hair_thin_hole_on_a_ledge_beside_a_loft_cut() {
    let mut s = load(&root().join("examples/regressions/loft_cut_beside_round_hole.json")).unwrap();
    let volume = |s: &Session| s.model.state().bodies.iter().map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.005)).unwrap().measure().volume).sum::<f64>();
    let before = volume(&s);
    s.execute("solid.hole", &json!({"diameter": 0.133, "position": [357.22, -150.993_333, 730.0]})).unwrap();
    check(&s, 0, &BTreeMap::new()).unwrap_or_else(|(kind, e)| panic!("{kind}: {e}"));
    // The drill goes through the 4 mm ledge and on through the slab below: 690 to 730.
    let gone = before - volume(&s);
    let expected = std::f64::consts::PI * 0.0665 * 0.0665 * 40.0;
    assert!((gone - expected).abs() < 0.2, "the hole took {gone} mm³, not {expected}");
}

// #1255: a 0.8 mm hole through the side wall of the plastic part's rest. Meshed at the
// tolerance the 200 mm drill sets, the part with its rounds, bosses and draft measured 6.8 mm³
// more after the cut than before; ten times finer it is 1.44 mm³ less, the drill's volume
// through the 2.9 mm wall. A volume out of bounds is measured again finer before it refuses.
#[test]
fn a_small_hole_through_the_wall_of_a_plastic_rest() {
    let mut s = load(&root().join("apps/solidcraft-cli/tests/corpus/modern-plastic.solidcraft")).unwrap();
    let volume = |s: &Session| s.model.state().bodies.iter().map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.002)).unwrap().measure().volume).sum::<f64>();
    let before = volume(&s);
    s.execute("solid.hole", &json!({"diameter": 0.8, "position": [63.069_841_5, 28.333_333, 4.0]})).unwrap();
    check(&s, 0, &BTreeMap::new()).unwrap_or_else(|(kind, e)| panic!("{kind}: {e}"));
    let gone = before - volume(&s);
    assert!(gone > 1.2 && gone < 1.7, "the hole took {gone} mm³");
}

// #1255: a hole square to a flange bent at 45°. The drill's cap, evaluated at that angle, lay
// 2.9e-11 mm off the plane it was made on against a bound of 2.2e-11, so the sheet didn't take
// the hole as a cut-out of its flat pattern and the cut was refused. The sheet keeps the hole
// and still unfolds and folds back.
#[test]
fn a_hole_square_to_a_45_degree_flange() {
    let mut s = load(&root().join("tests/oracle/70-sm-flange-45/recipe.json")).unwrap();
    let volume = |s: &Session| s.model.state().bodies.iter().map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.002)).unwrap().measure().volume).sum::<f64>();
    let before = volume(&s);
    s.execute("solid.hole", &json!({"diameter": 3.448, "position": [66.666_667, -4.511_845, 4.511_845]})).unwrap();
    check(&s, 0, &BTreeMap::new()).unwrap_or_else(|(kind, e)| panic!("{kind}: {e}"));
    // Through the 2.5 mm flange.
    let gone = before - volume(&s);
    let expected = std::f64::consts::PI * 1.724 * 1.724 * 2.5;
    assert!((gone - expected).abs() < 0.1, "the hole took {gone} mm³, not {expected}");
    let sheets: Vec<String> = s.model.state().sheets.iter().map(|sh| sh.body.clone()).collect();
    assert!(!sheets.is_empty());
    for body in sheets {
        for id in ["sheet.unfold", "sheet.refold"] {
            s.execute(id, &json!({"body": body})).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
}

// #1255: an axis through a corner round of the sensor enclosure's lid, then a parameter that
// widens the lid. The round is found again by name, but its point nearest the old pick is the
// end of its arc, on the edge it shares with the flat side, and picking again by that point
// took the side: "the face is not a cylinder". The named face is used as found now.
#[test]
fn an_axis_through_a_corner_round_that_a_parameter_moves() {
    let mut s = load(&root().join("examples/acceptance/sensor_enclosure.json")).unwrap();
    s.execute("construct.axis.through_cylinder", &json!({"face": {"face": [36.242_617, 29.977_740, 27.0]}})).unwrap();
    let start = s.doc.features.len();
    s.execute("parameters.change", &json!({"name": "board_w", "expression": "(60 mm) * 1.1"})).unwrap();
    check(&s, start, &BTreeMap::new()).unwrap_or_else(|(kind, e)| panic!("{kind}: {e}"));
    let st = s.model.state();
    let axis = st.construct.iter().find_map(|c| match &c.geom {
        solidcraft_engine::doc::construct::ConstructGeom::Axis { origin, dir } => Some((*origin, *dir)),
        _ => None,
    });
    let (origin, dir) = axis.expect("the axis");
    // The round's axis moved with the lid's corner, from x = 36 to 39.
    let off = origin - Vec3::new(39.0, 26.0, 0.0);
    assert!((off - dir * off.dot(dir)).len() < 1e-6, "the axis passes {origin:?} along {dir:?}");
}

#[test]
fn a_sketch_on_a_patterned_boss_top() {
    let mut c: Vec<Value> = rectangle(json!("XY"), [0., 0.], [60., 20.]).into();
    c.extend([
        json!({"command": "solid.extrude", "params": {"distance": 5}}),
        json!({"command": "sketch.create", "params": {"plane": {"face": [30, 10, 5]}}}),
        json!({"command": "sketch.circle.center", "params": {"center": [10, 10], "radius": 4}}),
        json!({"command": "sketch.finish", "params": {}}),
        json!({"command": "solid.extrude", "params": {"distance": 6, "operation": "join", "name": "Boss"}}),
        json!({"command": "solid.pattern.rectangular", "params": {"features": ["Boss"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 20}}),
        json!({"command": "sketch.create", "params": {"plane": {"face": [30, 10, 11]}}}),
        json!({"command": "sketch.finish", "params": {}}),
    ]);
    reproduces(None, Value::Array(c));
}

#[test]
fn construction_geometry_and_holes_on_a_moved_component() {
    let moved = || {
        vec![
            json!({"command": "component.create", "params": {"name": "Arm", "grounded": false}}),
            json!({"command": "solid.box", "params": {"corner": [20, 0, 0], "length": 10, "width": 4, "height": 4, "body_name": "ArmBody"}}),
            json!({"command": "component.activate", "params": {"component": 0}}),
            json!({"command": "occurrence.move", "params": {"occurrence": "Arm:1", "translate": [0, 50, 0]}}),
        ]
    };
    for step in [
        json!({"command": "construct.axis.edge", "params": {"edge": {"edge": [25, 50, 0]}}}),
        json!({"command": "construct.plane.three_points", "params": {"a": {"vertex": [20, 50, 0]}, "b": {"vertex": [30, 50, 0]}, "c": {"vertex": [20, 54, 0]}}}),
        json!({"command": "solid.hole", "params": {"position": [25, 52, 4], "diameter": 1}}),
        json!({"command": "solid.press_pull", "params": {"face": [25, 52, 4], "distance": 1}}),
        json!({"command": "solid.extrude", "params": {"face": [25, 52, 4], "distance": 1, "operation": "join"}}),
        json!({"command": "construct.plane.along_path", "params": {"path": {"edge": [25, 50, 0]}, "t": 0.5}}),
    ] {
        let mut c = moved();
        c.push(step);
        reproduces(None, Value::Array(c));
    }
}

#[test]
fn a_through_hole_into_a_shelled_box_rim() {
    reproduces(
        None,
        json!([
            {"command": "solid.box", "params": {"corner": [-35, -25, 0], "length": 70, "width": 50, "height": 30}},
            {"command": "solid.shell", "params": {"faces": [[0, 0, 30]], "thickness": 2}},
            {"command": "solid.hole", "params": {"position": [0, 24, 30], "diameter": 1}}
        ]),
    );
}

/// #559: a 6 mm hole drilled at an angle through a 920 mm prism, its through-all tool nearly
/// 1.9 m long beside a thin bar it misses. The bar's cut came out empty: the tool filled too
/// little of its own box for the sampled shortcut, and the boolean's tolerances (from the
/// tool's length) were wider than the hole.
#[test]
fn an_oblique_through_hole_beside_a_thin_bar() {
    reproduces(
        Some("examples/regressions/oblique_cut_of_cylinder.json"),
        json!([{"command": "solid.hole", "params": {"diameter": 6.0, "position": [296.66666666666663, -458.66666666666663, 848.7633333333333]}}]),
    );
}

#[test]
fn a_hole_through_a_sheet_metal_base() {
    let mut c: Vec<Value> = rectangle(json!("XY"), [0., 0.], [60., 40.]).into();
    c.extend([
        json!({"command": "sheet.flange", "params": {"type": "base"}}),
        json!({"command": "solid.hole", "params": {"position": [30, 20, 0], "direction": [0, 0, 1], "diameter": 5}}),
    ]);
    reproduces(None, Value::Array(c));
}

/// `face.hole` on a rim of the sheet metal corpus (#560): drilled along the panel, the hole is a
/// bore of that panel, and the part still unfolds and folds back.
#[test]
fn a_hole_drilled_into_a_sheet_metal_rim() {
    // What the matrix sends: the face's point, the hole square into it.
    for (design, at, diameter) in [
        // The base's end rim of the L bracket, through to the other end.
        ("typical/sheet-metal-bracket", [60.0, 7.777777777777779, 0.8333333333333333], 0.667),
        // The end rim of a tray's wall, beside the corner.
        ("oracle/69-sm-tray", [99.58333333333333, 4.267766952966369, 15.0], 0.333),
    ] {
        let Some((_, path)) = designs().into_iter().find(|(n, _)| n == design) else { panic!("no design {design}") };
        let mut s = load(&path).unwrap();
        let before = errors(&s);
        let start = s.doc.features.len();
        s.execute("solid.hole", &json!({"position": at, "diameter": diameter})).unwrap_or_else(|e| panic!("{design}: {e}"));
        check(&s, start, &before).unwrap_or_else(|(kind, e)| panic!("{design}: {kind}: {e}"));
        for id in ["sheet.unfold", "sheet.refold"] {
            s.execute(id, &json!({})).unwrap_or_else(|e| panic!("{design} {id}: {e}"));
        }
    }
}

#[test]
fn sketches_on_loft_and_sweep_faces() {
    let mut loft: Vec<Value> = rectangle(json!("XY"), [-20., -20.], [20., 20.]).into();
    loft.extend([
        json!({"command": "construct.plane.offset", "params": {"base": "XY", "offset": 40, "name": "P40"}}),
        json!({"command": "sketch.create", "params": {"plane": "P40"}}),
        json!({"command": "sketch.circle.center", "params": {"center": [0, 0], "radius": 10}}),
        json!({"command": "sketch.finish", "params": {}}),
        json!({"command": "solid.loft", "params": {"sections": [{"sketch": "Sketch1"}, {"sketch": "Sketch2"}]}}),
        json!({"command": "sketch.create", "params": {"plane": {"face": [0, 0, 40]}}}),
        json!({"command": "sketch.finish", "params": {}}),
    ]);
    reproduces(None, Value::Array(loft));
    reproduces(
        None,
        json!([
            {"command": "sketch.create", "params": {"plane": "XZ", "name": "Path"}},
            {"command": "sketch.line", "params": {"points": [[0, 0], [0, 30]], "ids": ["l1"]}},
            {"command": "sketch.finish", "params": {}},
            {"command": "sketch.create", "params": {"plane": "XY", "name": "Section"}},
            {"command": "sketch.rectangle.center", "params": {"center": [0, 0], "corner": [4, 2.5]}},
            {"command": "sketch.finish", "params": {}},
            {"command": "solid.sweep", "params": {"sketch": "Section", "path_sketch": "Path", "path": ["l1"]}},
            {"command": "sketch.create", "params": {"plane": {"face": [4, 0, -15]}}},
            {"command": "sketch.finish", "params": {}}
        ]),
    );
}

#[test]
fn press_pull_on_the_pi_case_floor() {
    reproduces(
        Some("tests/action_matrix/designs/pi-case.solidcraft"),
        json!([{"command": "solid.press_pull", "params": {"face": [30, 20, 0], "distance": 1}}]),
    );
}

#[test]
fn sketches_after_a_draft_and_on_a_tapered_extrude() {
    reproduces(
        None,
        json!([
            {"command": "solid.box", "params": {"corner": [-20, -20, 0], "length": 40, "width": 40, "height": 30}},
            {"command": "solid.draft", "params": {"faces": [[20, 0, 15], [-20, 0, 15], [0, 20, 15], [0, -20, 15]], "angle": "3 deg", "neutral": "XY"}},
            {"command": "sketch.create", "params": {"plane": {"face": [0, 0, 30]}}},
            {"command": "sketch.finish", "params": {}}
        ]),
    );
    let mut taper: Vec<Value> = rectangle(json!("XY"), [-20., -20.], [20., 20.]).into();
    taper.extend([
        json!({"command": "solid.extrude", "params": {"distance": 30, "taper": "5 deg"}}),
        json!({"command": "sketch.create", "params": {"plane": {"face": [0, 0, 30]}}}),
        json!({"command": "sketch.finish", "params": {}}),
    ]);
    reproduces(None, Value::Array(taper));
}

#[test]
fn a_sketch_on_the_rim_a_shell_leaves() {
    let mut c: Vec<Value> = rectangle(json!("XY"), [-35., -25.], [35., 25.]).into();
    c.extend([
        json!({"command": "solid.extrude", "params": {"distance": 30}}),
        json!({"command": "solid.shell", "params": {"faces": [[0, 0, 30]], "thickness": 2}}),
        json!({"command": "sketch.create", "params": {"plane": {"face": [0, 24, 30]}}}),
        json!({"command": "sketch.finish", "params": {}}),
    ]);
    reproduces(None, Value::Array(c));
}

#[test]
fn sketches_after_split_body_and_intersect() {
    reproduces(
        None,
        json!([
            {"command": "solid.box", "params": {"corner": [0, 0, 0], "length": 40, "width": 20, "height": 10, "body_name": "Block"}},
            {"command": "solid.split_body", "params": {"body": "Block", "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}},
            {"command": "sketch.create", "params": {"plane": {"face": [10, 10, 10]}}},
            {"command": "sketch.finish", "params": {}}
        ]),
    );
    reproduces(
        None,
        json!([
            {"command": "solid.box", "params": {"corner": [0, 0, 0], "length": 40, "width": 20, "height": 10, "body_name": "A"}},
            {"command": "solid.box", "params": {"corner": [10, -5, 0], "length": 20, "width": 30, "height": 20, "operation": "new_body", "body_name": "B"}},
            {"command": "solid.combine", "params": {"target": "A", "tools": ["B"], "operation": "intersect"}},
            {"command": "sketch.create", "params": {"plane": {"face": [20, 10, 10]}}},
            {"command": "sketch.finish", "params": {}}
        ]),
    );
}

#[test]
#[ignore = "fails until #566 is fixed"]
fn a_shell_of_a_swept_bar() {
    reproduces(
        None,
        json!([
            {"command": "sketch.create", "params": {"plane": "XZ", "name": "Path"}},
            {"command": "sketch.line", "params": {"points": [[0, 0], [0, 30]], "ids": ["l1"]}},
            {"command": "sketch.arc.three_point", "params": {"start": "l1.end", "through": [8.787, 51.213], "end": [30, 60], "id": "a1"}},
            {"command": "sketch.line", "params": {"points": ["a1.end", [60, 60]], "ids": ["l2"]}},
            {"command": "sketch.finish", "params": {}},
            {"command": "sketch.create", "params": {"plane": "XY", "name": "Section"}},
            {"command": "sketch.rectangle.center", "params": {"center": [0, 0], "corner": [4, 2.5]}},
            {"command": "sketch.finish", "params": {}},
            {"command": "solid.sweep", "params": {"sketch": "Section", "path_sketch": "Path", "path": ["l1", "a1", "l2"]}},
            {"command": "solid.shell", "params": {"faces": [[0, 0, 0]], "thickness": 0.5}}
        ]),
    );
}

/// The construction a drill-tip script ends with.
struct TipConstruction(solidcraft_engine::doc::construct::ConstructGeom);

impl TipConstruction {
    fn axis(&self) -> (Vec3, Vec3) {
        match &self.0 {
            solidcraft_engine::doc::construct::ConstructGeom::Axis { origin, dir } => (*origin, *dir),
            other => panic!("expected an axis, got {other:?}"),
        }
    }
    fn plane(&self) -> solidcraft_engine::geom::Plane {
        match &self.0 {
            solidcraft_engine::doc::construct::ConstructGeom::Plane(p) => *p,
            other => panic!("expected a plane, got {other:?}"),
        }
    }
}

/// Replays the script and returns its last construction.
fn drill_tip_construction(commands: Vec<Value>) -> TipConstruction {
    let mut s = Session::default();
    for c in &commands {
        let id = c["command"].as_str().unwrap();
        s.execute(id, &c["params"]).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let state = s.model.state();
    TipConstruction(state.construct.last().map(|c| c.geom.clone()).expect("a construction"))
}

#[test]
fn an_axis_through_a_drill_tip() {
    let mut c: Vec<Value> = rectangle(json!("XY"), [0., 0.], [60., 40.]).into();
    c.extend([
        json!({"command": "solid.extrude", "params": {"distance": 15}}),
        json!({"command": "solid.hole", "params": {"position": [50, 20, 15], "direction": [0, 0, -1], "diameter": 6, "depth": 10, "tip_angle": 118}}),
        json!({"command": "construct.axis.through_cylinder", "params": {"face": {"face": [48.44468907075447, 21.22448979591836, 4.399139380972439]}}}),
    ]);
    reproduces(None, Value::Array(c.clone()));
    // Not just "no error": the axis runs along the drill, through the hole's centre line.
    let (origin, dir) = drill_tip_construction(c).axis();
    assert!((origin.x - 50.).abs() < 1e-6 && (origin.y - 20.).abs() < 1e-6, "{origin:?}");
    assert!(dir.z.abs() > 1. - 1e-9, "{dir:?}");
}

#[test]
fn a_tangent_plane_on_a_drill_tip() {
    let mut c: Vec<Value> = rectangle(json!("XY"), [0., 0.], [60., 40.]).into();
    c.extend([
        json!({"command": "solid.extrude", "params": {"distance": 15}}),
        json!({"command": "solid.hole", "params": {"position": [50, 20, 15], "direction": [0, 0, -1], "diameter": 6, "depth": 10, "tip_angle": 118}}),
        json!({"command": "construct.plane.tangent", "params": {"face": {"face": [48.44468907075447, 21.22448979591836, 4.399139380972439]}}}),
    ]);
    reproduces(None, Value::Array(c.clone()));
    // The plane touches the cone at the picked point and leans like the cone does: a 118 degree tip
    // puts the wall 59 degrees off the drill axis, so its normal is 31 degrees off it.
    let plane = drill_tip_construction(c).plane();
    let picked = Vec3::new(48.44468907075447, 21.22448979591836, 4.399139380972439);
    let distance = (picked - plane.origin).dot(plane.normal());
    // The pick lies on the tessellated cone and the plane is the exact one, so allow for chord error.
    assert!(distance.abs() < 0.05, "the pick is {distance} mm off the plane");
    assert!((plane.normal().z.abs() - 31f64.to_radians().cos()).abs() < 1e-2, "{:?}", plane.normal());
}

#[test]
fn a_plane_on_a_patterned_counterbore_after_a_thickness_change() {
    let mut c = vec![json!({"command": "parameters.change", "params": {"name": "thick", "expression": "10 mm"}})];
    c.extend(rectangle(json!("XY"), [0., 0.], [80., 30.]));
    c.extend([
        json!({"command": "solid.extrude", "params": {"distance": "thick"}}),
        json!({"command": "solid.hole", "params": {"position": [15, 15, 10], "diameter": 5, "type": "counterbore", "cb_diameter": 9, "cb_depth": 3, "name": "Hole1"}}),
        json!({"command": "solid.pattern.rectangular", "params": {"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 25}}),
        json!({"command": "construct.plane.along_path", "params": {"path": {"edge": [44.5, 15, 7]}, "t": 0.5}}),
        json!({"command": "parameters.change", "params": {"name": "thick", "expression": "11 mm"}}),
    ]);
    reproduces(None, Value::Array(c));
}

/// #707: the same design builds the same body every time, in one process too. Faces were put in
/// the hash order of their addresses in a few places (truck's connected components, the planar
/// boolean's rebuild, healing's merges), so a later boolean on the ribbed lid passed or failed
/// from run to run.
#[test]
fn the_ribbed_lid_builds_the_same_body_every_time() {
    let path = root().join("examples/regressions/lid_ribs_flush_with_window.json");
    let fingerprint = || {
        let s = load(&path).unwrap();
        let st = s.model.state();
        st.bodies
            .iter()
            .map(|b| {
                let mesh = b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.1)).unwrap();
                let bits = mesh.positions.iter().fold(0u64, |acc, p| {
                    acc.rotate_left(7).wrapping_add(p.x.to_bits() ^ p.y.to_bits().rotate_left(21) ^ p.z.to_bits().rotate_left(42))
                });
                format!("{} {} {bits:x}", b.name, b.body.face_count())
            })
            .collect::<Vec<_>>()
    };
    let first = fingerprint();
    for _ in 0..5 {
        assert_eq!(fingerprint(), first);
    }
}
