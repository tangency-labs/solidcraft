//! Sketches on flat faces that no construction certificate names: a Cut's walls and floor, a
//! chamfer, a pattern's copies, a side a Join merged from two features, the pieces of a split
//! face (#491, #492, #493, #557). They attach by the face's exact history name
//! (`FaceCertificate::Lineage`), follow the face through parameter edits and survive a save.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::FaceAttachmentStatus;
use solidcraft_doc::config::FaceCertificate;
use solidcraft_geom::prelude::*;

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

fn healthy(s: &Session, sketch: &str) -> (solidcore_record::Plane, Option<FaceCertificate>) {
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
    let height = |p: &solidcore_record::Plane| (p.origin - solidcore_record::Vec3::new(40., 0., 10.)).dot(p.normal());
    assert!((height(&plane) + 3. * n).abs() < 1e-9, "{plane:?}");
    run(&mut s, "parameters.change", json!({"name": "bevel", "expression": "5 mm"}));
    no_errors(&s);
    assert!((height(&healthy(&s, "OnBevel").0) + 5. * n).abs() < 1e-9);
    healthy(&reopened(&s), "OnBevel");
}

/// #14639: a chamfer face named in the pre-format-52 form (`F2:blend:A|B`) still places a sketch.
#[test]
fn a_sketch_on_a_chamfer_face_saved_in_the_older_name_form_opens() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "name": "Base"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[40, 15, 10]], "distance": 3, "name": "Bevel"}));
    sketch_on(&mut s, [38.5, 15., 8.5], "OnBevel");
    let (saved_plane, _) = healthy(&s, "OnBevel");
    let mut doc: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let sketch = doc["features"].as_array_mut().unwrap().iter_mut().find(|f| f["name"] == "OnBevel").unwrap();
    let name = sketch["plane"]["anchor"]["target"]["name"].as_str().unwrap().to_string();
    let (head, edge) = name.split_once(":blend(").unwrap();
    let older = format!("{head}:blend:{}", edge.strip_suffix(')').unwrap());
    sketch["plane"]["anchor"]["target"]["name"] = json!(older);

    let mut t = Session::new(solidcraft_doc::Document::from_json(&doc.to_string()).unwrap());
    t.refresh();
    no_errors(&t);
    let (plane, certificate) = healthy(&t, "OnBevel");
    assert_eq!(certificate, Some(FaceCertificate::Lineage));
    assert!((plane.origin - saved_plane.origin).len() < 1e-9 && plane.normal().dot(saved_plane.normal()) > 1. - 1e-12, "{plane:?}");
}

/// #7476: a cone's base and the flat top of a cut-off cone take a sketch, as a cylinder's caps
/// do, on the origin and placed along a slanted axis. The sketch on the top follows the height.
#[test]
fn the_flat_faces_of_a_cone_take_a_sketch_that_follows_the_cone() {
    use solidcore_record::Vec3;
    let slant = Vec3::new(1., 1., 0.).normalized().unwrap();
    for (base, axis, along) in [(Vec3::ZERO, json!("Z"), Vec3::Z), (Vec3::new(5., -3., 2.), json!([1, 1, 0]), slant)] {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "tall", "expression": "20 mm"}));
        let at = [base.x, base.y, base.z];
        run(&mut s, "solid.cone", json!({"radius": 10, "top_radius": 4, "height": "tall", "base": at, "axis": axis, "name": "Cut"}));
        run(&mut s, "solid.cone", json!({"radius": 3, "height": 5, "base": [40, 0, 0], "name": "Full"}));
        no_errors(&s);
        let top = |tall: f64| base + along * tall;
        sketch_on(&mut s, [top(20.).x, top(20.).y, top(20.).z], "OnTop");
        sketch_on(&mut s, at, "OnBase");
        sketch_on(&mut s, [40., 0., 0.], "OnFullBase");
        let height = |p: &solidcore_record::Plane| (p.origin - base).dot(along);
        for (name, role) in [("OnTop", ":top"), ("OnBase", ":bottom"), ("OnFullBase", ":bottom")] {
            let FaceAttachmentStatus::Healthy { plane, target, .. } = status(&s, name) else { panic!("{name}: {:?}", status(&s, name)) };
            assert_eq!(target.certificate, Some(FaceCertificate::Lineage), "{name}");
            assert!(target.name.ends_with(role), "{name}: {}", target.name);
            let axis = if name == "OnFullBase" { Vec3::Z } else { along };
            assert!(plane.normal().cross(axis).len() < 1e-9, "{name}: {plane:?}");
        }
        assert!((height(&healthy(&s, "OnTop").0) - 20.).abs() < 1e-9);
        assert!(height(&healthy(&s, "OnBase").0).abs() < 1e-9);
        run(&mut s, "parameters.change", json!({"name": "tall", "expression": "30 mm"}));
        no_errors(&s);
        assert!((height(&healthy(&s, "OnTop").0) - 30.).abs() < 1e-9);
        assert!(height(&healthy(&s, "OnBase").0).abs() < 1e-9);
        let t = reopened(&s);
        assert!((height(&healthy(&t, "OnTop").0) - 30.).abs() < 1e-9);
    }
}

/// #14897: a pipe's end faces take a sketch, as a sweep's caps do: a solid pipe and a hollow one
/// (its ends are rings), each along a slanted line. The sketches follow the pipe when its path
/// gets longer and keep their faces after a save.
#[test]
fn the_end_faces_of_a_pipe_take_a_sketch_that_follows_the_pipe() {
    use solidcore_record::Vec3;
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [25, 25]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 40], [25, 65]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.pipe", json!({"path_sketch": "Path", "path": ["l1"], "diameter": 5, "operation": "new", "name": "Solid"}));
    run(&mut s, "solid.pipe", json!({"path_sketch": "Path", "path": ["l2"], "diameter": 5, "wall": 1, "operation": "new", "name": "Hollow"}));
    no_errors(&s);
    // The solid pipe's ends hold the path's ends; the hollow pipe's rings hold points 2 mm above
    // its path's ends (between the 1.5 mm hole and the 2.5 mm outside).
    sketch_on(&mut s, [0., 0., 0.], "SolidStart");
    sketch_on(&mut s, [25., 25., 0.], "SolidEnd");
    sketch_on(&mut s, [0., 40., 2.], "HollowStart");
    sketch_on(&mut s, [25., 65., 2.], "HollowEnd");
    let along = Vec3::new(1., 1., 0.).normalized().unwrap();
    let ends = [("SolidStart", ":start", Vec3::ZERO), ("SolidEnd", ":end", Vec3::new(25., 25., 0.))];
    let rings = [("HollowStart", ":start", Vec3::new(0., 40., 0.)), ("HollowEnd", ":end", Vec3::new(25., 65., 0.))];
    for (name, role, at) in ends.into_iter().chain(rings) {
        let FaceAttachmentStatus::Healthy { plane, target, .. } = status(&s, name) else { panic!("{name}: {:?}", status(&s, name)) };
        assert_eq!(target.certificate, Some(FaceCertificate::Lineage), "{name}");
        assert!(target.name.ends_with(role), "{name}: {}", target.name);
        assert!(plane.normal().cross(along).len() < 1e-9, "{name}: {plane:?}");
        assert!((plane.origin - at).dot(along).abs() < 1e-9, "{name}: {plane:?}");
    }
    // The solid pipe's path gets longer: the sketch on its end goes with it.
    run(&mut s, "sketch.edit", json!({"sketch": "Path"}));
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [30, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    no_errors(&s);
    let end = |s: &Session| (healthy(s, "SolidEnd").0.origin - Vec3::new(30., 30., 0.)).dot(along);
    assert!(end(&s).abs() < 1e-9, "{:?}", healthy(&s, "SolidEnd"));
    let t = reopened(&s);
    assert!(end(&t).abs() < 1e-9);
    for name in ["SolidStart", "HollowStart", "HollowEnd"] {
        healthy(&t, name);
    }
}

/// #20622: a rib's walls take a sketch, as a thin extrude's walls do: a 2 mm rib along a line
/// across a shelled box (the issue's steps). Each wall is named by the line and its side, so the
/// sketches follow the rib when its line moves and keep their faces after a save.
#[test]
fn the_walls_of_a_rib_take_a_sketch_that_follows_the_rib() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.shell", json!({"faces": [[30, 20, 10]], "thickness": "1mm"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "RibLine"}));
    run(&mut s, "sketch.line", json!({"points": [[30, 1], [30, 39]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.rib", json!({"sketch": "RibLine", "curves": ["l1"], "thickness": "2 mm", "direction": "normal"}));
    no_errors(&s);
    sketch_on(&mut s, [31., 20., 5.], "PlusX");
    sketch_on(&mut s, [29., 20., 5.], "MinusX");
    let wall_x = |s: &Session, name: &str| {
        let (plane, certificate) = healthy(s, name);
        assert_eq!(certificate, Some(FaceCertificate::Lineage), "{name}");
        plane.origin.x
    };
    assert!((wall_x(&s, "PlusX") - 31.).abs() < 1e-9 && (wall_x(&s, "MinusX") - 29.).abs() < 1e-9);
    // The rib's line moves 5 mm along x: the sketches on its walls go with it.
    run(&mut s, "sketch.edit", json!({"sketch": "RibLine"}));
    run(&mut s, "sketch.move_point", json!({"point": "l1.start", "to": [35, 1]}));
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [35, 39]}));
    run(&mut s, "sketch.finish", json!({}));
    no_errors(&s);
    for t in [&s, &reopened(&s)] {
        assert!((wall_x(t, "PlusX") - 36.).abs() < 1e-9, "{:?}", healthy(t, "PlusX"));
        assert!((wall_x(t, "MinusX") - 34.).abs() < 1e-9, "{:?}", healthy(t, "MinusX"));
    }
}

/// #21724: builds before #20622 named every face of a rib by its number among the faces the rib
/// made (`F5:face:2`). A design saved so, with a fillet on the edge between a rib wall and the
/// floor, opens with the fillet on that edge, and a face saved so is the rib's face its saved
/// point lies on: the saved points say which face of the rib each name was.
#[test]
fn a_fillet_and_a_sketch_on_rib_walls_saved_under_numbered_names_open() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.shell", json!({"faces": [[30, 20, 10]], "thickness": "1mm"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "RibLine"}));
    run(&mut s, "sketch.line", json!({"points": [[30, 1], [30, 39]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.rib", json!({"sketch": "RibLine", "curves": ["l1"], "thickness": "2 mm", "direction": "normal"}));
    sketch_on(&mut s, [31., 20., 5.], "PlusX");
    sketch_on(&mut s, [29., 20., 5.], "MinusX");
    run(&mut s, "solid.fillet", json!({"edges": [[31, 20, 1]], "radius": 0.5, "name": "Round"}));
    no_errors(&s);
    let volume = s.model.state().bodies[0].volume().unwrap();

    // Save, then name the walls as the earlier builds did.
    let mut doc: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let anchor = |doc: &Value, sketch: &str| {
        let f = doc["features"].as_array().unwrap().iter().find(|f| f["name"] == sketch).unwrap();
        f["plane"]["anchor"]["target"]["name"].as_str().unwrap().to_string()
    };
    let (plus, minus) = (anchor(&doc, "PlusX"), anchor(&doc, "MinusX"));
    let rib = plus.split_once(':').unwrap().0.to_string();
    assert!(minus.starts_with(&format!("{rib}:")) && plus != minus, "{plus} {minus}");
    let (old_plus, old_minus) = (format!("{rib}:face:3"), format!("{rib}:face:4"));
    let features = doc["features"].as_array_mut().unwrap();
    let round = features.iter_mut().find(|f| f["name"] == "Round").unwrap();
    let edge = round["edge_names"][0].as_str().unwrap().to_string();
    assert!(edge.contains(plus.as_str()), "the fillet's edge is on the +x wall: {edge}");
    round["edge_names"][0] = json!(edge.replace(plus.as_str(), &old_plus));

    let mut t = Session::new(solidcraft_doc::Document::from_json(&doc.to_string()).unwrap());
    t.refresh();
    no_errors(&t);
    let round = t.doc.find_feature("Round").map(|f| f.id).unwrap();
    let result = t.model.results.iter().find(|r| r.id == round).unwrap();
    assert!(result.warning.is_none(), "{:?}", result.warning);
    assert!((t.model.state().bodies[0].volume().unwrap() - volume).abs() < 1e-6, "the same edge is rounded");
    let state = t.model.state();
    let hit = solidcraft_doc::naming::locate_face(&state, &old_minus, solidcore_record::Vec3::new(29., 20., 5.)).expect("the -x wall");
    assert_eq!(solidcraft_doc::naming::face_names(&state.bodies[hit.body])[hit.face], minus);
    assert!(
        solidcraft_doc::naming::locate_face(&state, &old_minus, solidcore_record::Vec3::new(10., 20., 1.)).is_none(),
        "the floor is no face of the rib"
    );
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

/// A piece of a face split on purpose takes a sketch by the name Split Face gave it (the face's,
/// with the piece's side of the tool), and follows that piece when the split moves.
#[test]
fn a_piece_of_a_face_split_on_purpose_takes_a_sketch_by_its_own_name() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "at", "expression": "5 mm"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Modified"}));
    run(&mut s, "solid.split_face", json!({"body": "Modified", "faces": [[5, 5, 10]], "plane": {"origin": ["at", 0, 0], "normal": [1, 0, 0]}}));
    sketch_on(&mut s, [2.5, 5., 10.], "Low");
    let (plane, certificate) = healthy(&s, "Low");
    assert_eq!(certificate, Some(FaceCertificate::Lineage));
    assert!((plane.origin.z - 10.).abs() < 1e-9 && (plane.normal().z - 1.).abs() < 1e-9, "{plane:?}");
    run(&mut s, "parameters.change", json!({"name": "at", "expression": "7 mm"}));
    no_errors(&s);
    healthy(&s, "Low");
    healthy(&reopened(&s), "Low");
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

#[test]
fn transform_face_moves_only_the_picked_piece_and_rebuilds_through_zero() {
    let mut s = slotted_block();
    run(&mut s, "parameters.add", json!({"name":"raise","expression":"2 mm"}));
    run(&mut s, "solid.transform_face", json!({"face":[5,15,10],"translate":[0,0,"raise"],"name":"Raise"}));
    no_errors(&s);
    sketch_on(&mut s, [5., 15., 12.], "Raised");
    sketch_on(&mut s, [35., 15., 10.], "Kept");
    for height in [0, -1, 3] {
        run(&mut s, "parameters.change", json!({"name":"raise","expression":format!("{height} mm")}));
        no_errors(&s);
        assert!((healthy(&s, "Raised").0.origin.z - f64::from(10 + height)).abs() < 1e-9);
        assert!((healthy(&s, "Kept").0.origin.z - 10.).abs() < 1e-9);
    }
    assert!((healthy(&reopened(&s), "Raised").0.origin.z - 13.).abs() < 1e-9);
}

#[test]
fn transform_face_patterns_carry_the_pivot_and_mirror_the_rotation() {
    for (command, params) in [
        ("solid.pattern.rectangular", json!({"features":["Tilt"],"dir1":[1,0,0],"count1":2,"spacing1":25})),
        ("solid.mirror", json!({"features":["Tilt"],"plane":{"origin":[20,0,0],"normal":[1,0,0]}})),
    ] {
        let mut s = slotted_block();
        run(&mut s, "solid.transform_face", json!({"face":[5,15,10],"axis":[0,-1,0],"angle":"10 deg","origin":[0,0,10],"name":"Tilt"}));
        no_errors(&s);
        run(&mut s, command, params);
        no_errors(&s);
        let measured = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        // Stock minus the slot, plus two triangular prisms (width 15, depth 30).
        let want = 40. * 30. * 10. - 10. * 30. * 5. + 30. * 15. * 15. * 10_f64.to_radians().tan();
        assert!((measured - want).abs() < 1e-5, "{command}: {measured} vs {want}");
    }
}

#[test]
fn transform_face_edits_each_rebuilt_body_not_the_source_or_a_coincident_neighbour() {
    for (command, params, count) in [
        ("solid.pattern.rectangular", json!({"features":["Stock","Tilt"],"dir1":[1,0,0],"count1":3,"spacing1":30}), 4),
        ("solid.mirror", json!({"features":["Tilt","Stock"],"plane":{"origin":[20,0,0],"normal":[1,0,0]}}), 3),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"operation":"new","body_name":"Block","name":"Stock"}));
        run(&mut s, "solid.box", json!({"corner":[30,0,0],"length":10,"width":10,"height":10,"operation":"new","body_name":"Neighbour"}));
        run(
            &mut s,
            "solid.transform_face",
            json!({"body":"Block","face":[5,5,10],"translate":[0,0,2],"axis":[0,-1,0],"origin":[0,0,10],"angle":"45 deg","name":"Tilt"}),
        );
        run(&mut s, command, params);
        no_errors(&s);
        let measured = run(&mut s, "inspect.measure", json!({}));
        let bodies = measured["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), count, "{command}: {measured}");
        for body in bodies {
            // Each copy has the same area 100 and mean roof height (12 + 22)/2. The
            // neighbour also holds the copied pick but must not receive any part of the edit.
            let want = if body["name"] == "Neighbour" { 1000. } else { 1700. };
            assert!((body["volume_mm3"].as_f64().unwrap() - want).abs() < 1e-6, "{command}: {body}");
        }
    }
}

#[test]
fn transform_face_on_a_placed_move_copy_is_not_applied_twice_by_a_mirror() {
    use solidcore_record::Vec3;

    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Blank"}));
    run(&mut s, "solid.move", json!({"bodies":["Blank"],"translate":[20,0,0],"copy":true,"name":"Duplicate"}));
    let duplicate = s.model.state().bodies.last().unwrap().name.clone();
    run(&mut s, "solid.transform_face", json!({"body":duplicate,"face":[30,5,5],"translate":[2,0,0],"name":"Extend"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence":"Part:1","axis":[0,0,1],"angle":"90 deg","origin":[0,0,0],"translate":[50,0,0],"capture":true}),
    );
    run(&mut s, "solid.mirror", json!({"features":["Duplicate","Extend"],"plane":"YZ","name":"Echo"}));
    no_errors(&s);
    let echo = s.doc.find_feature("Echo").unwrap().id;
    let world = s.world_state().unwrap();
    let copy = world.bodies.iter().find(|b| b.feature == echo).unwrap();
    // The extended face is at local x=32, world y=32. The mirror changes x, not y.
    assert!(world.body(&duplicate).unwrap().body.face_at_point(Vec3::new(45., 32., 5.)).unwrap().is_some());
    assert!(copy.body.face_at_point(Vec3::new(-45., 32., 5.)).unwrap().is_some());
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
fn flat_faces(s: &Session) -> Vec<(usize, solidcore_record::Vec3)> {
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

/// A case's wall (a block with a rib across its top) and a cover extruded above it from an
/// offset start, cut by the wall so the rib leaves a groove across the cover's underside, with a
/// pocket in the cover mirrored twice. Both caps of the cover lie above its sketch plane.
fn grooved_cover() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "wall_h", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "WallSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-20, -15], "p1": [20, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "WallSketch", "distance": "wall_h", "name": "Wall"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "RibSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-2, -15], "p1": [2, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "RibSketch", "distance": 1, "start_offset": "wall_h", "operation": "join", "targets": ["Body1"], "name": "Rib"}),
    );
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "CoverSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-20, -15], "p1": [20, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "CoverSketch", "distance": 5, "start_offset": "wall_h", "operation": "new_body", "name": "Cover"}));
    run(&mut s, "solid.combine", json!({"target": "Body2", "tools": ["Body1"], "operation": "cut", "keep_tools": true, "name": "Groove"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PocketSketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 8], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "PocketSketch", "distance": 3, "start_offset": "wall_h + 2 mm", "operation": "cut", "targets": ["Body2"], "name": "Pocket"}),
    );
    run(&mut s, "solid.mirror", json!({"features": ["Pocket"], "plane": "YZ", "name": "MirrorX"}));
    run(&mut s, "solid.mirror", json!({"features": ["Pocket", "MirrorX"], "plane": "XZ", "name": "MirrorY"}));
    no_errors(&s);
    s
}

/// Both caps of an extrude from an offset start lie on one side of its sketch plane, so their
/// height gives both one role (`top`, numbered `top#1` and `top#2`), and the pieces of its
/// underside the groove left are named after that shared role. Each cap also answers to the end
/// it closes (`cap:low`, `cap:high`): the pieces take a sketch by it that keeps its attachment,
/// without a fixed plane, and follows them when the offset changes (T-1986). The faces' names
/// stay as they were.
#[test]
fn the_split_underside_of_an_extrude_from_an_offset_start_takes_a_sketch() {
    let mut s = grooved_cover();
    let cover = s.doc.find_feature("Cover").map(|f| f.id).unwrap();
    let state = s.model.state();
    let body = state.bodies.iter().find(|b| b.name == "Body2").unwrap();
    let names = solidcraft_doc::naming::face_names(body);
    assert!(names.iter().any(|n| n == &format!("F{cover}:top#2")), "{names:?}");
    let under: Vec<&String> = names.iter().filter(|n| n.starts_with(&format!("F{cover}:top#[")) && n.ends_with("]#1")).collect();
    assert_eq!(under.len(), 2, "the groove splits the underside in two: {names:?}");

    let faces = run(&mut s, "model.faces", json!({"body": "Body2"}));
    let piece = |x: f64| {
        let face = faces["faces"].as_array().unwrap().iter().find(|f| {
            let c = &f["centroid"];
            (c[2].as_f64().unwrap() - 10.).abs() < 1e-6 && c[0].as_f64().unwrap() * x > 0.
        });
        let face = face.unwrap_or_else(|| panic!("no underside piece at x {x}: {faces}"));
        (face["index"].as_u64().unwrap(), face["point"].clone())
    };
    for (name, x) in [("UnderLeft", -10.), ("UnderRight", 10.)] {
        let (index, point) = piece(x);
        run(&mut s, "sketch.create", json!({"plane": {"body": "Body2", "face": point, "index": index}, "name": name}));
        run(&mut s, "sketch.finish", json!({}));
        let (plane, certificate) = healthy(&s, name);
        assert_eq!(certificate, Some(FaceCertificate::Lineage), "{name}");
        let FaceAttachmentStatus::Healthy { target, .. } = status(&s, name) else { panic!("{name}") };
        assert_eq!(target.name, format!("F{cover}:cap:low"), "{name}");
        assert!((plane.normal().z + 1.).abs() < 1e-9 && (plane.origin.z - 10.).abs() < 1e-9, "{name}: {plane:?}");
    }
    sketch_on(&mut s, [0., 0., 15.], "OnTop");
    run(&mut s, "parameters.change", json!({"name": "wall_h", "expression": "12 mm"}));
    no_errors(&s);
    for (name, z) in [("UnderLeft", 12.), ("UnderRight", 12.), ("OnTop", 17.)] {
        assert!((healthy(&s, name).0.origin.z - z).abs() < 1e-9, "{name}");
    }
    let t = reopened(&s);
    for (name, z) in [("UnderLeft", 12.), ("UnderRight", 12.), ("OnTop", 17.)] {
        assert!((healthy(&t, name).0.origin.z - z).abs() < 1e-9, "{name}");
    }
}
