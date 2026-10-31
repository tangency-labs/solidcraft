//! The Loft and Surface Loft dialogs' guide rails (#201): rails picked in the dialog reach the
//! commands with their sketches and the fit, and editing the loft brings them back and keeps them.
//! Their sections follow the picks, two of them from one sketch included (#8221).
use crate::dialogs::Kind;
use crate::dialogs_surface::Surf;
use crate::scenario::Harness;
use crate::viewport::Hit;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, RailFit, SketchId, SweepRail};

/// Circles `Base` (z = 0) and `Top` (z = 40) and in `R` on XZ a rail `out` from (10, 0, 0) out to
/// x = 15 halfway and back to (10, 0, 40); the circles' and the rail's sketch ids.
fn setup(h: &mut Harness) -> (u64, u64, u64) {
    for (name, z) in [("Base", 0), ("Top", 40)] {
        h.app.run("sketch.create", json!({"name": name, "plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 10})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.app.run("sketch.create", json!({"plane": "XZ", "name": "R"})).unwrap();
    h.app.run("sketch.arc.three_point", json!({"start": [10, 0], "end": [10, -40], "through": [15, -20], "id": "out"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let id = |h: &Harness, n: &str| h.app.session.doc.find_feature(n).unwrap().id;
    (id(h, "Base"), id(h, "Top"), id(h, "R"))
}

fn ok(h: &mut Harness) {
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?} {:?}", h.app.dialog.as_ref().map(|d| &d.error), h.app.session.log);
}

#[test]
fn a_rail_picked_in_the_loft_dialog_is_kept_through_an_edit() {
    let mut h = Harness::new();
    let (base, top, rail_sketch) = setup(&mut h);
    h.step(&json!({"call":"ui.start","params":{"command":"solid.loft"}})).unwrap();
    h.frames(3);
    assert!(h.widget("Rail").is_some() && h.widget("Rail Fit").is_some());
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch: base, index: 0 }, Sel::Profile { sketch: top, index: 0 }];
    d.inputs[1].items = vec![Sel::SketchCurve { id: "out".into(), sketch: None }];
    let Kind::Loft { rail_fit, .. } = &mut d.kind else { panic!("{:?}", d.kind) };
    *rail_fit = 1;
    ok(&mut h);
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    let rails = |h: &Harness| match &h.app.session.doc.find_feature(&name).unwrap().kind {
        FeatureKind::Loft { rails, rail_fit, .. } => (rails.clone(), *rail_fit),
        k => panic!("{k:?}"),
    };
    let (stored, fit) = rails(&h);
    assert_eq!(stored, vec![SweepRail { sketch: SketchId(rail_sketch), curves: vec!["out".into()] }]);
    assert_eq!(fit, RailFit::Stretch);
    h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
    h.frames(8);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[1].items, vec![Sel::SketchCurve { id: "out".into(), sketch: Some(rail_sketch) }]);
    assert!(matches!(d.kind, Kind::Loft { rail_fit: 1, .. }), "{:?}", d.kind);
    ok(&mut h);
    assert_eq!(rails(&h), (stored, fit));
}

#[test]
fn a_rail_picked_in_the_surface_loft_dialog_is_kept_through_an_edit() {
    let mut h = Harness::new();
    let (base, top, rail_sketch) = setup(&mut h);
    h.step(&json!({"call":"ui.start","params":{"command":"surface.loft"}})).unwrap();
    h.frames(3);
    assert!(h.widget("Rail").is_some() && h.widget("Rail Fit").is_some());
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch: base, index: 0 }, Sel::Profile { sketch: top, index: 0 }];
    d.inputs[1].items = vec![Sel::SketchCurve { id: "out".into(), sketch: None }];
    ok(&mut h);
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    let rails = |h: &Harness| match &h.app.session.doc.find_feature(&name).unwrap().kind {
        FeatureKind::SurfaceLoft { rails, rail_fit, .. } => (rails.clone(), *rail_fit),
        k => panic!("{k:?}"),
    };
    assert_eq!(rails(&h), (vec![SweepRail { sketch: SketchId(rail_sketch), curves: vec!["out".into()] }], RailFit::Scale));
    h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
    h.frames(8);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(d.kind, Kind::Surface(Surf::Loft { rail_fit: 0 })), "{:?}", d.kind);
    assert_eq!(d.inputs[1].items, vec![Sel::SketchCurve { id: "out".into(), sketch: Some(rail_sketch) }]);
    ok(&mut h);
    assert_eq!(rails(&h).0.len(), 1);
}

/// Curve ids repeat from one sketch to the next (#3726). Rails `out` in two sketches each reach
/// the loft from their own sketch, not both from the newest one with an `out`; and two sketches'
/// lines `l` both stay in Surface Loft's sections, the second pick not taking the first back out.
#[test]
fn curves_that_share_an_id_are_picked_from_their_own_sketches() {
    let mut h = Harness::new();
    let (base, top, right) = setup(&mut h);
    h.app.run("sketch.create", json!({"plane": "XZ", "name": "L"})).unwrap();
    h.app.run("sketch.arc.three_point", json!({"start": [-10, 0], "end": [-10, -40], "through": [-15, -20], "id": "out"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let left = h.app.session.doc.find_feature("L").unwrap().id;
    let hit = |sketch: u64, id: &str| Hit::SketchCurve { sketch, id: id.into(), straight: false, point: solidcraft_engine::geom::Vec3::ZERO };
    h.step(&json!({"call":"ui.start","params":{"command":"solid.loft"}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch: base, index: 0 }, Sel::Profile { sketch: top, index: 0 }];
    for (input, sketch) in [(1, right), (2, left)] {
        d.active = input;
        let pick = d.candidate_on(&h.app.session, &hit(sketch, "out"), None).unwrap();
        d.pick(&h.app.session, pick);
    }
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["rails"], json!([{"sketch": right, "curves": ["out"]}, {"sketch": left, "curves": ["out"]}]));
    h.step(&json!({"key":"Escape"})).unwrap();

    for (name, z) in [("Low", 0), ("High", 40)] {
        h.app.run("sketch.create", json!({"name": name, "plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})).unwrap();
        h.app.run("sketch.line", json!({"points": [[-10, 0], [10, 0]], "ids": ["l"]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    let (low, high) = (h.app.session.doc.find_feature("Low").unwrap().id, h.app.session.doc.find_feature("High").unwrap().id);
    h.step(&json!({"call":"ui.start","params":{"command":"surface.loft"}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    for sketch in [low, high] {
        let pick = d.candidate_on(&h.app.session, &hit(sketch, "l"), None).unwrap();
        d.pick(&h.app.session, pick);
    }
    assert_eq!(d.inputs[0].items.len(), 2, "{:?}", d.inputs[0].items);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["sections"], json!([{"sketch": low, "curves": ["l"]}, {"sketch": high, "curves": ["l"]}]));
}

/// A sketch at height `z` named `name`, made by `draw`; its id.
fn sketch_at(h: &mut Harness, name: &str, z: i32, draw: &[(&str, serde_json::Value)]) -> u64 {
    h.app.run("sketch.create", json!({"name": name, "plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})).unwrap();
    for (command, params) in draw {
        h.app.run(command, params.clone()).unwrap();
    }
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.session.doc.find_feature(name).unwrap().id
}

/// Two sections can come from one sketch (#8221): Surface Loft from one line of `A`, over `B`'s,
/// back to `A`'s other line makes three sections in pick order, and an edit keeps them.
#[test]
fn surface_loft_sections_from_one_sketch_stay_apart() {
    let mut h = Harness::new();
    let a = sketch_at(
        &mut h,
        "A",
        0,
        &[
            ("sketch.line", json!({"points": [[-10, 0], [10, 0]], "ids": ["front"]})),
            ("sketch.line", json!({"points": [[-10, 40], [10, 40]], "ids": ["back"]})),
        ],
    );
    let b = sketch_at(&mut h, "B", 30, &[("sketch.line", json!({"points": [[-10, 20], [10, 20]], "ids": ["top"]}))]);
    h.step(&json!({"call":"ui.start","params":{"command":"surface.loft"}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![
        Sel::SketchCurve { id: "front".into(), sketch: Some(a) },
        Sel::SketchCurve { id: "top".into(), sketch: Some(b) },
        Sel::SketchCurve { id: "back".into(), sketch: Some(a) },
    ];
    let expected = json!([{"sketch": a, "curves": ["front"]}, {"sketch": b, "curves": ["top"]}, {"sketch": a, "curves": ["back"]}]);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["sections"], expected);
    ok(&mut h);
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
    h.frames(8);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["params"]["sections"], expected);
    ok(&mut h);
}

/// Curves of one sketch that meet end to end are still one Surface Loft section, whatever order
/// they were picked in.
#[test]
fn surface_loft_curves_that_meet_are_one_section() {
    let mut h = Harness::new();
    let a = sketch_at(&mut h, "A", 0, &[("sketch.line", json!({"points": [[-10, 0], [0, 0], [10, 5]], "ids": ["left", "right"]}))]);
    let b = sketch_at(&mut h, "B", 30, &[("sketch.line", json!({"points": [[-10, 0], [10, 0]], "ids": ["top"]}))]);
    h.step(&json!({"call":"ui.start","params":{"command":"surface.loft"}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![
        Sel::SketchCurve { id: "right".into(), sketch: Some(a) },
        Sel::SketchCurve { id: "top".into(), sketch: Some(b) },
        Sel::SketchCurve { id: "left".into(), sketch: Some(a) },
    ];
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["sections"], json!([{"sketch": a, "curves": ["right", "left"]}, {"sketch": b, "curves": ["top"]}]));
}

/// Loft from one circle of `C`, over `D`'s, back to `C`'s other circle (#8221): each picked
/// profile is a section, in pick order, and an edit keeps them.
#[test]
fn loft_profiles_from_one_sketch_stay_apart() {
    let mut h = Harness::new();
    let circle = |x: i32, y: i32| ("sketch.circle.center", json!({"center": [x, y], "diameter": 12}));
    let c = sketch_at(&mut h, "C", 0, &[circle(0, 0), circle(0, 40)]);
    let d_sketch = sketch_at(&mut h, "D", 30, &[circle(0, 20)]);
    h.step(&json!({"call":"ui.start","params":{"command":"solid.loft"}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch: c, index: 0 }, Sel::Profile { sketch: d_sketch, index: 0 }, Sel::Profile { sketch: c, index: 1 }];
    let expected = json!([{"sketch": c, "profiles": [0]}, {"sketch": d_sketch, "profiles": [0]}, {"sketch": c, "profiles": [1]}]);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["sections"], expected);
    ok(&mut h);
    assert_eq!(h.app.session.world_state().bodies.len(), 1, "{:?}", h.app.session.log);
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
    h.frames(8);
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["params"]["sections"], expected);
    ok(&mut h);
}
