//! Sketches on flat faces that no construction certificate names: a Cut's walls and floor, a
//! chamfer, a pattern's copies, a side a Join merged from two features, the pieces of a split
//! face (#491, #492, #493, #557). They attach by the face's exact history name
//! (`FaceCertificate::Lineage`), follow the face through parameter edits and survive a save.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::FaceAttachmentStatus;
use solidcraft_doc::config::FaceCertificate;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn sketch_on(s: &mut Session, at: [f64; 3], name: &str) {
    run(s, "sketch.create", json!({"plane": {"face": at}, "name": name}));
    run(s, "sketch.finish", json!({}));
}

fn status(s: &Session, sketch: &str) -> FaceAttachmentStatus {
    let id = s.doc.find_feature(sketch).map(|f| f.id).unwrap();
    s.placed_document().sketch_face_attachment(id, &s.model)
}

fn healthy(s: &Session, sketch: &str) -> (solidcraft_geom::Plane, Option<FaceCertificate>) {
    match status(s, sketch) {
        FaceAttachmentStatus::Healthy { plane, target, .. } => (plane, target.certificate),
        other => panic!("{sketch}: {other:?}"),
    }
}

fn no_errors(s: &Session) {
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
}

fn reopened(s: &Session) -> Session {
    let mut t = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    t.refresh();
    no_errors(&t);
    t
}

/// A 40 x 30 x 10 block with a slot cut 5 deep across it from x = `slot_x` (a parameter), 10 wide.
fn slotted_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "name": "Block"}));
    run(&mut s, "parameters.add", json!({"name": "slot_x", "expression": "15 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "SlotSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [15, -5], "p1": [25, 35]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "p1"], "type": "horizontal", "value": "slot_x"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "10 mm"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "SlotSketch", "distance": 12, "start_offset": 5, "operation": "cut", "name": "Slot"}));
    no_errors(&s);
    s
}

#[test]
fn a_cut_wall_and_floor_take_a_sketch_that_follows_the_cut() {
    let mut s = slotted_block();
    sketch_on(&mut s, [15., 15., 7.5], "Wall");
    sketch_on(&mut s, [20., 15., 5.], "Floor");
    let (wall, certificate) = healthy(&s, "Wall");
    assert_eq!(certificate, Some(FaceCertificate::Lineage));
    // The wall faces into the slot (+x), its frame's origin over the design origin.
    assert!((wall.normal().x - 1.).abs() < 1e-9 && wall.origin.x.abs() > 14.9 && (wall.origin.x - 15.).abs() < 1e-9, "{wall:?}");
    assert!((wall.origin.y.abs() + wall.origin.z.abs()) < 1e-9, "{wall:?}");
    let (floor, _) = healthy(&s, "Floor");
    assert!((floor.normal().z - 1.).abs() < 1e-9 && (floor.origin.z - 5.).abs() < 1e-9, "{floor:?}");

    // The slot moves: the sketch on its wall moves with it.
    run(&mut s, "parameters.change", json!({"name": "slot_x", "expression": "20 mm"}));
    no_errors(&s);
    let (wall, _) = healthy(&s, "Wall");
    assert!((wall.origin.x - 20.).abs() < 1e-9, "{wall:?}");
    let t = reopened(&s);
    assert!((healthy(&t, "Wall").0.origin.x - 20.).abs() < 1e-9);
    assert!((healthy(&t, "Floor").0.origin.z - 5.).abs() < 1e-9);
}

#[test]
fn the_pieces_of_a_split_top_take_a_sketch() {
    let mut s = slotted_block();
    // The slot splits the block's top in two; each piece takes a sketch (on that face's plane).
    for (name, x) in [("Left", 5.), ("Right", 35.)] {
        sketch_on(&mut s, [x, 15., 10.], name);
        assert!((healthy(&s, name).0.origin.z - 10.).abs() < 1e-9, "{name}");
    }
    run(&mut s, "parameters.change", json!({"name": "slot_x", "expression": "20 mm"}));
    no_errors(&s);
    assert!((healthy(&s, "Left").0.origin.z - 10.).abs() < 1e-9);
}

#[test]
fn a_chamfer_face_takes_a_sketch_that_follows_the_chamfer() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "bevel", "expression": "3 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "name": "Base"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[40, 15, 10]], "distance": "bevel", "name": "Bevel"}));
    no_errors(&s);
    sketch_on(&mut s, [38.5, 15., 8.5], "OnBevel");
    let (plane, certificate) = healthy(&s, "OnBevel");
    assert_eq!(certificate, Some(FaceCertificate::Lineage));
    let n = std::f64::consts::FRAC_1_SQRT_2;
    assert!((plane.normal().x - n).abs() < 1e-9 && (plane.normal().z - n).abs() < 1e-9, "{plane:?}");
    // A bigger chamfer moves the face inward: the sketch goes with it.
    let height = |p: &solidcraft_geom::Plane| (p.origin - solidcraft_geom::Vec3::new(40., 0., 10.)).dot(p.normal());
    assert!((height(&plane) + 3. * n).abs() < 1e-9, "{plane:?}");
    run(&mut s, "parameters.change", json!({"name": "bevel", "expression": "5 mm"}));
    no_errors(&s);
    assert!((height(&healthy(&s, "OnBevel").0) + 5. * n).abs() < 1e-9);
    healthy(&reopened(&s), "OnBevel");
}

/// A plate with a boss patterned 3 times along x, `pitch` apart.
fn patterned_bosses(sketch_on_seed_first: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "pitch", "expression": "20 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [80, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [40, 10, 5]}}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 4}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 6, "operation": "join", "name": "Boss"}));
    if sketch_on_seed_first {
        sketch_on(&mut s, [10., 10., 11.], "Seed");
    }
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Boss"], "dir1": [1, 0, 0], "count1": 3, "spacing1": "pitch", "name": "Row"}));
    no_errors(&s);
    s
}

#[test]
fn each_patterned_copy_takes_its_own_sketch() {
    let mut s = patterned_bosses(true);
    // A sketch made on the seed before the pattern keeps its construction certificate: the
    // copies carry it too, but are named as copies.
    assert!(matches!(healthy(&s, "Seed").1, Some(FaceCertificate::ExtrudeCap { .. })));
    for (name, x) in [("First", 10.), ("Middle", 30.), ("Last", 50.)] {
        sketch_on(&mut s, [x, 10., 11.], name);
        assert!((healthy(&s, name).0.origin.z - 11.).abs() < 1e-9, "{name}");
    }
    run(&mut s, "parameters.change", json!({"name": "pitch", "expression": "25 mm"}));
    no_errors(&s);
    for name in ["Seed", "First", "Middle", "Last"] {
        healthy(&s, name);
    }
    let t = reopened(&s);
    for name in ["Seed", "Middle", "Last"] {
        healthy(&t, name);
    }
}

#[test]
fn a_piece_of_a_face_split_on_purpose_still_needs_a_fixed_plane() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Modified"}));
    run(&mut s, "solid.split_face", json!({"body": "Modified", "faces": [[5, 5, 10]], "plane": {"origin": [5, 0, 0], "normal": [1, 0, 0]}}));
    let error = s.execute("sketch.create", &json!({"plane": {"face": [2.5, 5, 10]}})).unwrap_err().to_string();
    assert!(error.contains("fixed:true"), "{error}");
}

/// Offset Face raises one piece of a split top: the pieces no longer share a plane, so each
/// takes a sketch by its own name (the face's, qualified by the faces around it) and follows it.
#[test]
fn a_raised_piece_of_a_split_top_takes_a_sketch_by_its_own_name() {
    let mut s = slotted_block();
    run(&mut s, "parameters.add", json!({"name": "raise", "expression": "2 mm"}));
    run(&mut s, "solid.offset_face", json!({"faces": [[5, 15, 10]], "distance": "raise", "name": "Raise"}));
    no_errors(&s);
    sketch_on(&mut s, [5., 15., 12.], "Raised");
    sketch_on(&mut s, [35., 15., 10.], "Kept");
    let (raised, certificate) = healthy(&s, "Raised");
    assert_eq!(certificate, Some(FaceCertificate::Lineage));
    assert!((raised.origin.z - 12.).abs() < 1e-9 && (raised.normal().z - 1.).abs() < 1e-9, "{raised:?}");
    assert!((healthy(&s, "Kept").0.origin.z - 10.).abs() < 1e-9);
    run(&mut s, "parameters.change", json!({"name": "raise", "expression": "3 mm"}));
    no_errors(&s);
    assert!((healthy(&s, "Raised").0.origin.z - 13.).abs() < 1e-9);
    assert!((healthy(&s, "Kept").0.origin.z - 10.).abs() < 1e-9);
    let t = reopened(&s);
    assert!((healthy(&t, "Raised").0.origin.z - 13.).abs() < 1e-9);
}

/// A plastic Boss keeps the history of the body it stands on (#494): the plate's faces still
/// take attached sketches, and so does the boss's own top, which follows its height.
#[test]
fn faces_after_a_boss_take_a_sketch_that_follows_it() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "boss_h", "expression": "12 mm"}));
    run(&mut s, "solid.box", json!({"length": 80, "width": 50, "height": 2, "name": "Plate"}));
    run(
        &mut s,
        "plastic.boss",
        json!({"position": [20, 20, 2], "diameter": 7, "height": "boss_h", "hole_diameter": 2.5, "fillet": 0.5, "name": "Boss"}),
    );
    no_errors(&s);
    sketch_on(&mut s, [5., 25., 2.], "OnPlate");
    sketch_on(&mut s, [80., 25., 1.], "OnSide");
    sketch_on(&mut s, [22.5, 20., 14.], "OnBoss");
    for name in ["OnPlate", "OnSide"] {
        healthy(&s, name);
    }
    let (top, certificate) = healthy(&s, "OnBoss");
    assert_eq!(certificate, Some(FaceCertificate::Lineage));
    assert!((top.origin.z - 14.).abs() < 1e-9, "{top:?}");
    run(&mut s, "parameters.change", json!({"name": "boss_h", "expression": "15 mm"}));
    no_errors(&s);
    assert!((healthy(&s, "OnBoss").0.origin.z - 17.).abs() < 1e-9);
    healthy(&reopened(&s), "OnBoss");
}

/// A Hole names its faces by the part they cut (a counterbore's floor is `F<hole>:cbore:base`),
/// and a pattern's copies of it as copies: each floor takes its own sketch, which follows the
/// counterbore's depth. The plate's faces stay attachable too.
#[test]
fn counterbore_floors_and_their_pattern_copies_take_a_sketch() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "cb", "expression": "3 mm"}));
    run(&mut s, "solid.box", json!({"length": 80, "width": 30, "height": 10, "name": "Plate"}));
    run(
        &mut s,
        "solid.hole",
        json!({"position": [15, 15, 10], "diameter": 5, "type": "counterbore", "cb_diameter": 10, "cb_depth": "cb", "name": "Hole"}),
    );
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Hole"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 25, "name": "Row"}));
    no_errors(&s);
    for (name, x) in [("Seed", 18.5), ("Copy", 43.5), ("Top", 30.)] {
        sketch_on(&mut s, [x, 15., if name == "Top" { 10. } else { 7. }], name);
    }
    for name in ["Seed", "Copy"] {
        let (floor, certificate) = healthy(&s, name);
        assert_eq!(certificate, Some(FaceCertificate::Lineage), "{name}");
        assert!((floor.origin.z - 7.).abs() < 1e-9, "{name}: {floor:?}");
    }
    healthy(&s, "Top");
    run(&mut s, "parameters.change", json!({"name": "cb", "expression": "4 mm"}));
    no_errors(&s);
    for name in ["Seed", "Copy"] {
        assert!((healthy(&s, name).0.origin.z - 6.).abs() < 1e-9, "{name}");
    }
    healthy(&reopened(&s), "Copy");
}

/// The middle of the largest triangle of each flat face of the first body.
fn flat_faces(s: &Session) -> Vec<(usize, solidcraft_geom::Vec3)> {
    let state = s.model.state();
    let body = &state.bodies[0].body;
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
    (0..body.face_count())
        .filter(|index| body.planar_face(*index).is_some())
        .filter_map(|index| {
            let point = mesh
                .triangles
                .iter()
                .zip(&mesh.tri_face)
                .filter(|(_, f)| **f as usize == index)
                .filter_map(|(t, _)| mesh.tri(t))
                .max_by(|[a, b, c], [d, e, f]| (*b - *a).cross(*c - *a).len().total_cmp(&(*e - *d).cross(*f - *d).len()))
                .map(|[a, b, c]| (a + b + c) * (1.0 / 3.0))?;
            Some((index, point))
        })
        .collect()
}

/// Every flat face of the first body takes an attached sketch (#681: a flat face is a flat face,
/// whichever features it was merged from).
fn every_flat_face_takes_a_sketch(s: &mut Session) {
    no_errors(s);
    let faces = flat_faces(s);
    assert!(faces.len() >= 8, "{}", faces.len());
    let mut failed = Vec::new();
    for (index, point) in faces {
        match s.execute("sketch.create", &json!({"plane": {"face": point, "body": "Body1", "index": index}})) {
            Ok(made) => {
                run(s, "sketch.finish", json!({}));
                let id = made["sketch"].as_u64().unwrap();
                if !matches!(s.placed_document().sketch_face_attachment(id, &s.model), FaceAttachmentStatus::Healthy { .. }) {
                    failed.push(format!("face {index} at {point:?}: not healthy"));
                }
            }
            Err(e) => failed.push(format!("face {index} at {point:?}: {e}")),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// #681: a block with a lower step joined flush beside it. The step's side walls continue the
/// block's (a Join merged them), and they take a sketch like every other flat face.
#[test]
fn every_flat_face_of_a_block_and_a_flush_step_takes_a_sketch() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "A", "distance": 10, "name": "Block"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "B"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 10], "p1": [20, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "B", "distance": 5, "operation": "join", "name": "Step"}));
    every_flat_face_takes_a_sketch(&mut s);
}

/// #681 as reported: one sketch of a 2 x 2 grid, three cells extruded up (an elbow) and three
/// joined down (the inverse elbow).
#[test]
fn every_flat_face_of_an_elbow_up_and_an_elbow_down_takes_a_sketch() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Grid"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0], [20, 20], [0, 20]], "closed": true}));
    run(&mut s, "sketch.line", json!({"points": [[10, 0], [10, 20]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 10], [20, 10]]}));
    run(&mut s, "sketch.finish", json!({}));
    let cells = |pts: [[f64; 2]; 3]| pts.iter().map(|p| json!({"point": p})).collect::<Vec<_>>();
    run(&mut s, "solid.extrude", json!({"sketch": "Grid", "distance": 10, "profiles": cells([[5., 5.], [15., 5.], [5., 15.]]), "name": "Up"}));
    let down = cells([[15., 5.], [5., 15.], [15., 15.]]);
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Grid", "distance": 10, "direction": "negative", "operation": "join", "profiles": down, "name": "Down"}),
    );
    every_flat_face_takes_a_sketch(&mut s);
}

/// #565: Split Body by a plane a parameter moves (a raw plane, a construction plane or a sketch
/// line). The face the split cuts on each part is named after the half-space that cut it
/// (`F<split>:section`): a sketch and an offset plane attach to it and follow the plane.
#[test]
fn the_face_a_split_cuts_takes_a_sketch_and_an_offset_plane_that_follow_the_plane() {
    for tool in ["plane", "construction", "line"] {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "split_x", "expression": "15 mm"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Block"}));
        let split = match tool {
            "plane" => json!({"body": "Block", "plane": {"origin": ["split_x", 0, 0], "normal": [1, 0, 0]}}),
            "construction" => {
                run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": "split_x", "name": "Cut"}));
                json!({"body": "Block", "plane": "Cut"})
            }
            _ => {
                let id = run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Guide"}))["sketch"].as_u64().unwrap();
                run(&mut s, "sketch.line", json!({"points": [[15, -10], [15, 30]], "ids": ["cut"], "connect": false}));
                run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "cut", "mode": "vertical"}));
                run(&mut s, "sketch.dimension", json!({"entities": ["origin", "cut.start"], "type": "horizontal", "value": "split_x"}));
                run(&mut s, "sketch.finish", json!({}));
                json!({"body": "Block", "sketch": id, "line": "cut"})
            }
        };
        run(&mut s, "solid.split_body", split);
        no_errors(&s);
        // Each part's cut face (the two lie on one another, so each is picked by body and index).
        let cut_face = |s: &Session, body: &str, x: f64| {
            let state = s.model.state();
            let b = &state.body(body).unwrap().body;
            let index = (0..b.face_count())
                .find(|i| b.planar_face(*i).is_some_and(|p| p.normal().x.abs() > 0.999 && (p.origin.x - x).abs() < 1e-9))
                .unwrap_or_else(|| panic!("{tool}: no cut face on `{body}`"));
            json!({"face": [x, 10, 5], "body": body, "index": index})
        };
        let mut normals = Vec::new();
        for (k, body) in ["Block", "Block (1)"].into_iter().enumerate() {
            let pick = cut_face(&s, body, 15.);
            run(&mut s, "sketch.create", json!({"plane": pick, "name": format!("OnCut{k}")}));
            run(&mut s, "sketch.finish", json!({}));
            let (plane, certificate) = healthy(&s, &format!("OnCut{k}"));
            assert_eq!(certificate, Some(FaceCertificate::Lineage), "{tool} {body}");
            assert!((plane.origin.x - 15.).abs() < 1e-9 && plane.normal().x.abs() > 0.999, "{tool} {body}: {plane:?}");
            normals.push(plane.normal().x);
            let pick = cut_face(&s, body, 15.);
            run(&mut s, "construct.plane.offset", json!({"base": pick, "offset": 2, "name": format!("Off{k}")}));
        }
        assert!(normals[0] * normals[1] < 0., "{tool}: the two cut faces face each other's way: {normals:?}");
        no_errors(&s);

        run(&mut s, "parameters.change", json!({"name": "split_x", "expression": "20 mm"}));
        no_errors(&s);
        let offset = |s: &Session, name: &str| {
            let feature = s.doc.find_feature(name).map(|f| f.id).unwrap();
            let state = s.model.state();
            match state.construct.iter().find(|c| c.feature == feature).map(|c| &c.geom) {
                Some(solidcraft_doc::construct::ConstructGeom::Plane(p)) => *p,
                other => panic!("{tool} {name}: {other:?}"),
            }
        };
        for (k, n) in normals.iter().enumerate() {
            let (plane, _) = healthy(&s, &format!("OnCut{k}"));
            assert!((plane.origin.x - 20.).abs() < 1e-9 && (plane.normal().x - n).abs() < 1e-9, "{tool} OnCut{k}: {plane:?}");
            let off = offset(&s, &format!("Off{k}"));
            assert!((off.origin.x - (20. + 2. * n)).abs() < 1e-9, "{tool} Off{k}: {off:?}");
        }
        let t = reopened(&s);
        for k in 0..2 {
            assert!((healthy(&t, &format!("OnCut{k}")).0.origin.x - 20.).abs() < 1e-9, "{tool} reopened OnCut{k}");
        }
    }
}
