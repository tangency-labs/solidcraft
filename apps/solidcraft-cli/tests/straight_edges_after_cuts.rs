//! Straight edges stay one edge after many cuts (#1049). The owner's design
//! (`tests/action_matrix/designs/crazy-geom-cuts.sc`, saved by 0.10.0): five extrudes,
//! joins and cuts, over four sketches, three of them on faces of the body with the faces'
//! edges projected. Cuts used to leave the straight edges of the walls they sliced split at
//! vertices nothing else used, so an edge that is one straight line was drawn (and lit on hover)
//! as many pieces, and a sketch on such a face projected every piece.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::doc::kernel::{FaceSurface, face_surface};
use solidcraft_engine::geom::Vec3;

fn design() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/action_matrix/designs/crazy-geom-cuts.sc")
}

fn open(path: &Path) -> Session {
    let mut s = Session::default();
    s.execute("doc.open", &json!({"path": path.to_string_lossy()})).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    s
}

/// The design built from scratch: saved without cached geometry and opened again.
fn rebuilt() -> Session {
    let s = open(&design());
    let dir = std::env::temp_dir().join(format!("solidcraft-straight-edges-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crazy-geom-cuts.json");
    let mut t = s.scratch();
    t.execute("file.save", &json!({"path": path.to_string_lossy(), "format": "json"})).unwrap();
    let out = open(&path);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn errors(s: &Session) -> Vec<(u64, String)> {
    s.model.results.iter().filter_map(|r| r.error.clone().map(|e| (r.id, e))).collect()
}

fn key(v: Vec3) -> [i64; 3] {
    [v.x, v.y, v.z].map(|c| (c * 1e5).round() as i64)
}

#[test]
fn straight_edges_are_one_edge_after_many_cuts() {
    for (what, s) in [("as saved", open(&design())), ("built from scratch", rebuilt())] {
        assert_eq!(errors(&s), vec![], "{what}: every feature evaluates");
        let st = s.world_state();
        let bodies: Vec<_> = st.bodies.iter().filter(|b| !b.body.is_mesh()).collect();
        assert_eq!(bodies.len(), 1, "{what}: one body");
        let body = &bodies[0].body;
        let mesh = body.tessellate(0.01).unwrap();
        // Every edge is drawn as one polyline; a straight one as one segment.
        let mut at: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, e) in mesh.edges.iter().enumerate() {
            let (Some(a), Some(z)) = (e.first(), e.last()) else { panic!("{what}: edge {i} is empty") };
            at.entry(key(*a)).or_default().push(i);
            at.entry(key(*z)).or_default().push(i);
        }
        let straight = |i: usize| mesh.edges.get(i).filter(|e| e.len() == 2).and_then(|e| (e[1] - e[0]).normalized());
        let split: Vec<_> = at
            .iter()
            .filter(|(_, es)| match es.as_slice() {
                [a, b] => straight(*a).zip(straight(*b)).is_some_and(|(u, w)| u.dot(w).abs() > 1. - 1e-9),
                _ => false,
            })
            .map(|(k, _)| k.map(|c| c as f64 / 1e5))
            .collect();
        assert_eq!(split, Vec::<[f64; 3]>::new(), "{what}: straight edges split where nothing else meets them");
        // No two faces side by side in one plane.
        let plane = |f: usize| match face_surface(body, f) {
            Some(FaceSurface::Plane { normal, point }) => Some((normal, normal.dot(point))),
            _ => None,
        };
        for (i, faces) in mesh.edge_faces.iter().enumerate() {
            if let [a, b] = faces.as_slice()
                && a != b
                && let (Some((na, da)), Some((nb, db))) = (plane(*a as usize), plane(*b as usize))
            {
                assert!(na.dot(nb) < 1. - 1e-9 || (da - db).abs() > 1e-6, "{what}: faces {a} and {b} meet at edge {i} in one plane");
            }
        }
        assert_eq!((body.face_count(), body.edge_count(), body.vertex_count()), (35, 107, 71), "{what}: faces, edges and vertices");
    }
}
