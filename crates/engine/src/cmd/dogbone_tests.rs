#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_record::Vec3;
use solidcraft_doc::dogbone::MAX_LENGTH;
use solidcraft_doc::{FacePick, FeatureKind};

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// An empty folder in the system's temp folder, removed with what is in it when the test ends,
/// passed or failed.
struct TempFolder(std::path::PathBuf);

impl TempFolder {
    fn new(name: &str) -> TempFolder {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempFolder(dir)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Faces, edges and vertices of body `name`.
fn counts(s: &Session, name: &str) -> [usize; 3] {
    let st = s.model.state();
    let b = &st.body(name).unwrap().body;
    [b.face_count(), b.edge_count(), b.vertex_count()]
}

/// A 40 x 30 x 10 plate (`Plate`) with a 20 x 14 pocket 6 deep from its top: floor at z = 4,
/// corners at x 10 and 30, y 8 and 22.
fn pocketed() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 14, "height": 7, "corner": [10, 8, 4], "operation": "cut"}));
    s
}

const PLATE: f64 = 40.0 * 30.0 * 10.0;
const POCKETED: f64 = PLATE - 20.0 * 14.0 * 6.0;
/// The pocket's four corner edges, a point on each.
const CORNERS: [[f64; 3]; 4] = [[10.0, 8.0, 7.0], [30.0, 8.0, 7.0], [30.0, 22.0, 7.0], [10.0, 22.0, 7.0]];

/// What one normal relief of radius `r` removes from a square corner `depth` deep:
/// (π/2 − 1) r² × depth (two circular segments, each cut off by a wall r/√2 from the centre).
fn normal_relief(r: f64, depth: f64) -> f64 {
    (FRAC_PI_2 - 1.0) * r * r * depth
}

#[test]
fn a_pockets_four_corners_lose_four_derived_reliefs() {
    let mut s = pocketed();
    assert!(rel(volume(&mut s), POCKETED) < 1e-9);
    assert_eq!(counts(&s, "Plate"), [11, 24, 16]);
    let out = run(&mut s, "solid.dogbone", json!({"edges": CORNERS, "diameter": 3, "name": "Bones"}));
    assert_eq!(out["relieved"], 4, "{out}");
    assert_eq!(out["skipped"], json!([]));
    assert_eq!(out["changed_bodies"], json!(["Plate"]));
    assert_eq!(out["relieved_edges"].as_array().unwrap().len(), 4);
    let want = POCKETED - 4.0 * normal_relief(1.5, 6.0);
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
    // Each corner edge becomes a half cylinder between two straight edges on the walls, with an
    // arc on the top and one on the floor: one face, three edges and two vertices more each. A
    // relief cylinder's seam lies along its +x side, so the half cylinders facing +x (at the
    // corners with x = 30, centred on directions 315° and 45°) are split there in two: one face,
    // three edges (the seam and the other halves of the arcs) and two vertices more each.
    assert_eq!(counts(&s, "Plate"), [11 + 4 + 2, 24 + 12 + 6, 16 + 8 + 4]);
}

#[test]
fn a_picked_floor_relieves_the_same_corners_as_its_edges() {
    let mut s = pocketed();
    let out = run(&mut s, "solid.dogbone", json!({"faces": [[20, 15, 4]], "diameter": 3}));
    assert_eq!(out["relieved"], 4, "{out}");
    let by_face = volume(&mut s);
    let mut e = pocketed();
    run(&mut e, "solid.dogbone", json!({"edges": CORNERS, "diameter": 3}));
    assert!(rel(by_face, volume(&mut e)) < 1e-9);
}

/// The pocket with a round bay 5 in radius on its wall at y = 22, centred on (20, 22), floor at
/// z = 4 too.
fn bayed() -> Session {
    let mut s = pocketed();
    run(&mut s, "solid.cylinder", json!({"base": [20, 22, 4], "radius": 5, "height": 7, "operation": "cut"}));
    s
}

#[test]
fn a_face_pick_beside_a_round_wall_relieves_the_face_it_is_exactly_nearest() {
    // The display mesh's facets of the bay's wall lie inside the bay, nearer its axis than the
    // wall. A point `off` in from the wall and 0.8 `off` above the floor is exactly nearer the
    // floor, yet nearer a facet of the wall than the floor where a facet bulges in by more than
    // 0.2 `off`: find one, where the display mesh gives the pick to the wall.
    let s = bayed();
    let pick = {
        let st = s.model.state();
        let b = st.body("Plate").unwrap();
        let mesh_face = |p: [f64; 3]| solidcraft_doc::appearance::nearest_face(b, Vec3::new(p[0], p[1], p[2])).unwrap().1;
        let floor = mesh_face([20.0, 25.0, 4.0]);
        [0.02, 0.05, 0.1]
            .into_iter()
            .flat_map(|off: f64| {
                (0..=480).map(move |i| {
                    let (sin, cos) = (30.0 + f64::from(i) * 0.25).to_radians().sin_cos();
                    [20.0 + (5.0 - off) * cos, 22.0 + (5.0 - off) * sin, 4.0 + 0.8 * off]
                })
            })
            .find(|p| mesh_face(*p) != floor)
            .expect("a point the display mesh gives to the bay's wall")
    };
    let mut mid = bayed();
    let want = run(&mut mid, "solid.dogbone", json!({"faces": [[20, 25, 4]], "diameter": 3}));
    assert!(want["relieved"].as_u64().is_some_and(|n| n >= 1), "{want}");
    // The feature's evaluation decides the face on the exact faces: given the pick as it was
    // made (a design whose feature keeps no face names), it relieves the floor's corners as a
    // pick in the middle of the bay does.
    let mut at = bayed();
    let made = run(&mut at, "solid.dogbone", json!({"faces": [[20, 25, 4]], "diameter": 3}));
    let id = made["feature"].as_u64().unwrap();
    if let Some(f) = at.doc_mut().feature_mut(id)
        && let FeatureKind::Dogbone { faces, .. } = &mut f.kind
    {
        *faces = vec![FacePick(Vec3::new(pick[0], pick[1], pick[2]))];
        f.face_names.clear();
    }
    at.refresh();
    let report = at.model.result(id).and_then(|r| r.state.dogbone_report(id)).cloned().unwrap();
    let relieved: Vec<[f64; 3]> = report.relieved.iter().map(|v| [v.x, v.y, v.z]).collect();
    assert_eq!(json!(relieved), want["relieved_edges"], "{pick:?}");
    assert!(rel(volume(&mut at), volume(&mut mid)) < 1e-12, "{pick:?}");
}

#[test]
fn a_blind_pocket_is_never_cut_below_its_floor() {
    // The reliefs end on the floor: the slab under it (z 0…4) keeps its whole 40 x 30 x 4.
    let mut s = pocketed();
    run(&mut s, "solid.dogbone", json!({"faces": [[20, 15, 4]], "diameter": 6}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 4, "operation": "intersect", "targets": ["Plate"]}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 4.0) < 1e-9, "{}", volume(&mut s));
}

#[test]
fn a_slot_through_the_plate_is_relieved_through_and_its_outside_corners_skipped() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    // 6 wide: the bites of the reliefs at each end of a short wall (√2 r each) don't meet.
    run(&mut s, "solid.box", json!({"length": 20, "width": 6, "height": 12, "corner": [10, 12, -1], "operation": "cut"}));
    let out = run(&mut s, "solid.dogbone", json!({"faces": [[2, 2, 10]], "diameter": 3.175}));
    assert_eq!(out["relieved"], 4, "{out}");
    let skipped = out["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 4);
    assert!(skipped.iter().all(|k| k["reason"] == "convex"), "{out}");
    // The plate's outside corners a picked face reaches are listed, but don't flag the feature.
    assert!(out["warning"].is_null(), "{out}");
    let r = 3.175 / 2.0;
    let want = PLATE - 20.0 * 6.0 * 10.0 - 4.0 * normal_relief(r, 10.0);
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
}

#[test]
fn an_l_shaped_pocket_has_five_inside_corners_and_one_outside() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    // An L 5 deep: 20 x 6 along x from (10, 6), and 6 x 16 along y.
    run(&mut s, "solid.box", json!({"length": 20, "width": 6, "height": 6, "corner": [10, 6, 5], "operation": "cut"}));
    run(&mut s, "solid.box", json!({"length": 6, "width": 16, "height": 6, "corner": [10, 6, 5], "operation": "cut"}));
    let pocket = PLATE - (20.0 * 6.0 + 6.0 * 10.0) * 5.0;
    assert!(rel(volume(&mut s), pocket) < 1e-9);
    let corners = json!([[10, 6, 7], [30, 6, 7], [30, 12, 7], [16, 12, 7], [16, 22, 7], [10, 22, 7]]);
    let out = run(&mut s, "solid.dogbone", json!({"edges": corners, "diameter": 2}));
    assert_eq!(out["relieved"], 5, "{out}");
    let skipped = out["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["reason"], "convex");
    assert_eq!(skipped[0]["at"], json!([16.0, 12.0, 7.5]));
    assert!(rel(volume(&mut s), pocket - 5.0 * normal_relief(1.0, 5.0)) < 1e-7);
}

#[test]
fn t_bone_and_minimal_reliefs_remove_their_derived_volumes() {
    // A T-bone's centre lies on the wall it bites: half its disc is beyond that wall.
    let mut s = pocketed();
    run(&mut s, "solid.dogbone", json!({"edges": CORNERS, "diameter": 3, "style": "t_bone"}));
    let want = POCKETED - 4.0 * (PI * 1.5 * 1.5 / 2.0) * 6.0;
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
    // Minimal, 0.3 short of the corner: two segments cut off by walls (r + gap)/√2 from the centre.
    let mut s = pocketed();
    run(&mut s, "solid.dogbone", json!({"edges": CORNERS, "diameter": 3, "style": "minimal", "gap": 0.3}));
    let (r, d) = (1.5f64, 1.8 / 2f64.sqrt());
    let segment = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let want = POCKETED - 4.0 * 2.0 * segment * 6.0;
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
    // Without a gap, minimal stops 5% of the diameter short: 0.15 for a 3 mm bit.
    let mut s = pocketed();
    run(&mut s, "solid.dogbone", json!({"edges": CORNERS, "diameter": 3, "style": "minimal"}));
    let d = 1.65 / 2f64.sqrt();
    let segment = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let want = POCKETED - 4.0 * 2.0 * segment * 6.0;
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
    // The T-bone bites the longer (20 mm) wall: the side's choice turns it onto the 14 mm one.
    // Both remove the same; the reliefs' centres differ (checked in the kernel).
    let mut s = pocketed();
    let out = run(&mut s, "solid.dogbone", json!({"edges": CORNERS, "diameter": 3, "style": "t_bone", "side": "shorter"}));
    assert_eq!(out["relieved"], 4);
}

#[test]
fn the_reliefs_follow_a_diameter_parameter() {
    let mut s = pocketed();
    run(&mut s, "parameters.add", json!({"name": "bit", "expression": "3 mm"}));
    run(&mut s, "solid.dogbone", json!({"edges": CORNERS, "diameter": "bit", "name": "Bones"}));
    assert!(rel(volume(&mut s), POCKETED - 4.0 * normal_relief(1.5, 6.0)) < 1e-7);
    run(&mut s, "parameters.change", json!({"name": "bit", "expression": "6 mm"}));
    assert!(rel(volume(&mut s), POCKETED - 4.0 * normal_relief(3.0, 6.0)) < 1e-7, "{}", volume(&mut s));
    // Back to 3 mm, then Edit Feature doubles it.
    run(&mut s, "parameters.change", json!({"name": "bit", "expression": "3 mm"}));
    // Edit Feature redefines it with the dialog's parameters (timeline.redefine).
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Bones", "command": "solid.dogbone", "params": {"edges": CORNERS, "diameter": "bit * 2", "body": "Plate"}}),
    );
    assert!(rel(volume(&mut s), POCKETED - 4.0 * normal_relief(3.0, 6.0)) < 1e-7, "{}", volume(&mut s));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Bones", "command": "solid.dogbone", "params": {"edges": CORNERS, "diameter": "bit", "body": "Plate"}}),
    );
    // Editing the style rebuilds the feature.
    run(&mut s, "timeline.edit", json!({"feature": "Bones", "set": {"style": "t_bone"}}));
    assert!(rel(volume(&mut s), POCKETED - 4.0 * (PI * 1.5 * 1.5 / 2.0) * 6.0) < 1e-7, "{}", volume(&mut s));
}

#[test]
fn the_reliefs_follow_the_pocket_when_it_moves() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "20 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": "w", "width": 14, "height": 7, "corner": [10, 8, 4], "operation": "cut"}));
    run(&mut s, "solid.dogbone", json!({"faces": [[15, 15, 4]], "diameter": 3}));
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "24 mm"}));
    let want = PLATE - 24.0 * 14.0 * 6.0 - 4.0 * normal_relief(1.5, 6.0);
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
}

#[test]
fn the_reliefs_of_picked_edges_follow_the_pocket_when_it_moves() {
    // As above with the four corner edges picked: the pocket widens from 20 to 24, and the two
    // corner edges at x = 30 move to x = 34.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "20 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": "w", "width": 14, "height": 7, "corner": [10, 8, 4], "operation": "cut"}));
    let out = run(&mut s, "solid.dogbone", json!({"edges": CORNERS, "diameter": 3}));
    assert_eq!(out["relieved"], 4, "{out}");
    let id = out["feature"].as_u64().unwrap();
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "24 mm"}));
    let report = s.model.result(id).and_then(|r| r.state.dogbone_report(id)).cloned().unwrap();
    let mut xs: Vec<f64> = report.relieved.iter().map(|v| v.x).collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs, [10.0, 10.0, 34.0, 34.0], "{report:?}");
    let want = PLATE - 24.0 * 14.0 * 6.0 - 4.0 * normal_relief(1.5, 6.0);
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
}

#[test]
fn undo_takes_the_reliefs_away_and_a_saved_design_keeps_them() {
    let mut s = pocketed();
    run(
        &mut s,
        "solid.dogbone",
        json!({"edges": CORNERS, "diameter": 3, "style": "minimal", "gap": 0.2, "max_angle": "100 deg", "along": "Z", "name": "Bones"}),
    );
    let relieved = volume(&mut s);
    run(&mut s, "edit.undo", json!({}));
    assert!(rel(volume(&mut s), POCKETED) < 1e-9);
    run(&mut s, "edit.redo", json!({}));
    assert!(rel(volume(&mut s), relieved) < 1e-12);
    let dir = TempFolder::new("solidcraft-dogbone");
    let path = dir.0.join("bones.sc");
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut back = Session::default();
    run(&mut back, "doc.open", json!({"path": path}));
    assert!(rel(volume(&mut back), relieved) < 1e-9);
    let kinds = |s: &Session| s.doc.features.iter().map(|f| f.kind.clone()).collect::<Vec<_>>();
    assert_eq!(kinds(&back), kinds(&s));
    // The file is a design container: the design read from its bytes names the feature kind.
    let saved = crate::io::read_design(&std::fs::read(&path).unwrap()).unwrap();
    assert!(saved.to_json().contains("\"dogbone\""), "the design names the feature kind");
}

#[test]
fn a_step_export_carries_the_reliefs() {
    let mut s = pocketed();
    run(&mut s, "solid.dogbone", json!({"faces": [[20, 15, 4]], "diameter": 3}));
    let dir = TempFolder::new("solidcraft-dogbone-step");
    let path = dir.0.join("bones.step");
    run(&mut s, "file.export", json!({"path": path, "bodies": ["Plate"]}));
    let step = std::fs::read_to_string(&path).unwrap();
    let mut back = Session::default();
    let r = run(&mut back, "file.insert_step", json!({"path": path}));
    // Four relief walls, two of them split at their seam (see the face counts above).
    assert_eq!(step.matches("CYLINDRICAL_SURFACE").count(), 6, "four relief walls");
    assert!(step.contains("1.5"));
    let name = r["created_bodies"][0].as_str().unwrap().to_string();
    let v = back.model.state().body(&name).unwrap().measure().unwrap().volume;
    assert!(rel(v, POCKETED - 4.0 * normal_relief(1.5, 6.0)) < 1e-6, "{v}");
}

#[test]
fn corners_it_cannot_relieve_are_refused_with_the_reason() {
    let mut s = pocketed();
    let features = s.doc.features.len();
    let refused = |s: &mut Session, p: Value| -> String {
        let e = s.execute("solid.dogbone", &p).unwrap_err().to_string();
        assert_eq!(s.doc.features.len(), features, "{p}: {e}");
        assert!(rel(volume(s), POCKETED) < 1e-9);
        e
    };
    // An outside edge alone: nothing to relieve.
    let e = refused(&mut s, json!({"edges": [[0, 0, 5]], "diameter": 3}));
    // A picked edge skipped beside relieved ones is named in the feature's warning.
    let mut t = pocketed();
    let out = run(&mut t, "solid.dogbone", json!({"edges": [CORNERS[0], [0, 0, 5]], "diameter": 3}));
    assert_eq!(out["relieved"], 1);
    assert!(out["warning"].as_str().is_some_and(|w| w.contains("convex")), "{out}");
    assert!(e.contains("no inside corner") && e.contains("convex"), "{e}");
    // A floor edge (along x) with the bit along Z.
    let e = refused(&mut s, json!({"edges": [[20, 8, 4]], "diameter": 3, "along": "Z"}));
    assert!(e.contains("not along axis"), "{e}");
    // Corners more open than the largest angle.
    let e = refused(&mut s, json!({"edges": CORNERS, "diameter": 3, "max_angle": "60 deg"}));
    assert!(e.contains("too open"), "{e}");
    // Without an axis, that floor edge is a 90° inside corner and is relieved along it.
    let out = run(&mut s, "solid.dogbone", json!({"edges": [[20, 8, 4]], "diameter": 2}));
    assert_eq!(out["relieved"], 1);
    assert!(rel(volume(&mut s), POCKETED - normal_relief(1.0, 20.0)) < 1e-7, "{}", volume(&mut s));
}

#[test]
fn hostile_parameters_are_refused_and_change_nothing() {
    let mut s = pocketed();
    let features = s.doc.features.len();
    let bad = [
        json!({"edges": CORNERS, "diameter": 0}),
        json!({"edges": CORNERS, "diameter": -3}),
        json!({"edges": CORNERS, "diameter": "nan"}),
        json!({"edges": CORNERS, "diameter": "1e400"}),
        json!({"edges": CORNERS, "diameter": "3 deg"}),
        json!({"edges": CORNERS}),
        json!({"diameter": 3}),
        json!({"edges": [], "diameter": 3}),
        json!({"edges": [[1000, 1000, 1000]], "diameter": 3}),
        json!({"faces": [[1000, 1000, 1000]], "diameter": 3}),
        json!({"edges": [[f64::MAX, 0, 0]], "diameter": 3}),
        json!({"edges": CORNERS, "diameter": 3, "style": "bogus"}),
        json!({"edges": CORNERS, "diameter": 3, "gap": 0.2}),
        json!({"edges": CORNERS, "diameter": 3, "style": "minimal", "gap": -1}),
        json!({"edges": CORNERS, "diameter": 3, "side": "shorter"}),
        json!({"edges": CORNERS, "diameter": 3, "style": "t_bone", "side": "left"}),
        json!({"edges": CORNERS, "diameter": 3, "max_angle": "0 deg"}),
        json!({"edges": CORNERS, "diameter": 3, "max_angle": "270 deg"}),
        json!({"edges": CORNERS, "diameter": 3, "along": [0, 0, 0]}),
        json!({"edges": CORNERS, "diameter": 3, "along": "W"}),
        json!({"edges": CORNERS, "diameter": 3, "body": "Nope"}),
        json!({"edges": CORNERS, "diameter": 1e9}),
    ];
    for p in bad {
        assert!(s.execute("solid.dogbone", &p).is_err(), "{p}");
        assert_eq!(s.doc.features.len(), features, "{p}");
    }
    // A diameter or a gap past the largest one (100 m) is a bad parameter that names the range.
    let range = format!("at most {MAX_LENGTH} mm");
    for p in [
        json!({"edges": CORNERS, "diameter": MAX_LENGTH + 1.0}),
        json!({"edges": CORNERS, "diameter": 10.0 * MAX_LENGTH}),
        json!({"edges": CORNERS, "diameter": 3, "style": "minimal", "gap": MAX_LENGTH + 1.0}),
        json!({"edges": CORNERS, "diameter": 3, "style": "minimal", "gap": 10.0 * MAX_LENGTH}),
    ] {
        match s.execute("solid.dogbone", &p) {
            Err(EngineError::BadParams { msg, .. }) => assert!(msg.contains(&range), "{p}: {msg}"),
            other => panic!("{p}: {other:?}"),
        }
        assert_eq!(s.doc.features.len(), features, "{p}");
    }
    assert!(rel(volume(&mut s), POCKETED) < 1e-9);
    // No body at all.
    let mut empty = Session::default();
    assert!(empty.execute("solid.dogbone", &json!({"edges": [[0, 0, 0]], "diameter": 3})).is_err());
    assert!(empty.doc.features.is_empty());
}

#[test]
fn a_bit_too_big_for_its_pocket_still_cuts_what_it_reaches() {
    // A 4 mm slot and an 8 mm bit: the reliefs overlap across the slot; the cut is still a solid.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 4, "height": 12, "corner": [10, 13, -1], "operation": "cut"}));
    let out = run(&mut s, "solid.dogbone", json!({"faces": [[2, 2, 10]], "diameter": 8}));
    assert_eq!(out["relieved"], 4);
    let v = volume(&mut s);
    assert!(v < PLATE - 800.0 && v > PLATE - 800.0 - 4.0 * PI * 16.0 * 10.0, "{v}");
    assert_eq!(s.model.state().bodies.len(), 1);
}

/// A plate with a polygon pocket `depth` deep from its top: the polygon's `n` corners on a circle
/// of `radius` about (20, 15).
fn polygon_pocket(n: usize, radius: f64, depth: f64) -> (Session, f64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));
    let pts: Vec<[f64; 2]> = (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64 + FRAC_PI_2;
            [20.0 + radius * a.cos(), 15.0 + radius * a.sin()]
        })
        .collect();
    run(&mut s, "sketch.line", json!({"points": pts, "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": depth, "direction": "negative", "operation": "cut"}));
    // The polygon's area: n triangles from its centre.
    let area = n as f64 / 2.0 * radius * radius * (2.0 * PI / n as f64).sin();
    let v = PLATE - area * depth;
    assert!(rel(volume(&mut s), v) < 1e-9, "{} vs {v}", volume(&mut s));
    (s, v)
}

#[test]
fn sixty_and_hundred_and_twenty_degree_corners_lose_their_closed_form_reliefs() {
    // One relief removes r² (π − θ − sin θ) of the corner's section: two circular segments.
    let removed = |r: f64, theta: f64, depth: f64| r * r * (PI - theta - theta.sin()) * depth;
    // An equilateral triangle: three 60° corners.
    let (mut s, before) = polygon_pocket(3, 13.0, 6.0);
    let out = run(&mut s, "solid.dogbone", json!({"faces": [[20, 15, 4]], "diameter": 3}));
    assert_eq!(out["relieved"], 3, "{out}");
    let want = before - 3.0 * removed(1.5, PI / 3.0, 6.0);
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
    // A regular hexagon: six 120° corners, skipped by default (more open than 90°).
    let (mut s, before) = polygon_pocket(6, 9.0, 5.0);
    let e = s.execute("solid.dogbone", &json!({"faces": [[20, 15, 5]], "diameter": 3})).unwrap_err().to_string();
    assert!(e.contains("too open"), "{e}");
    let out = run(&mut s, "solid.dogbone", json!({"faces": [[20, 15, 5]], "diameter": 3, "max_angle": "120 deg"}));
    assert_eq!(out["relieved"], 6, "{out}");
    let want = before - 6.0 * removed(1.5, 2.0 * PI / 3.0, 5.0);
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
}
