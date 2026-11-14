//! The Sweep and Surface Sweep dialogs' options (#771): orientation, twist and end scale reach
//! the commands, and editing a sweep keeps them.
use crate::dialogs::Kind;
use crate::dialogs_surface::Surf;
use crate::scenario::Harness;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, SweepOrientation};

/// A square `Q` on XY and a path `P` 40 up the Z axis; the square's sketch id.
fn setup(h: &mut Harness) -> u64 {
    h.app.run("sketch.create", json!({"plane":"XY","name":"Q"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-5,-5],"p1":[5,5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XZ","name":"P"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,-40]],"ids":["up"]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.session.doc.find_feature("Q").unwrap().id
}

#[test]
fn a_new_sweep_takes_its_options_from_the_dialog() {
    let mut h = Harness::new();
    let sketch = setup(&mut h);
    h.step(&json!({"call":"ui.start","params":{"command":"solid.sweep"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:section:Advanced"}})).unwrap();
    assert!(h.widget("Twist").is_some() && h.widget("End Scale").is_some() && h.widget("Orientation").is_some());
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    d.inputs[1].items = vec![Sel::SketchCurve { id: "up".into(), sketch: None }];
    d.inputs[2].items = vec![Sel::Axis { name: "X".into() }];
    let Kind::Sweep { opts, .. } = &mut d.kind else { panic!("{:?}", d.kind) };
    opts.orientation = 2;
    opts.twist = "90 deg".into();
    opts.scale = "0.5".into();
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let f = h.app.session.doc.features.last().unwrap();
    let FeatureKind::Sweep { orientation, twist, scale, .. } = &f.kind else { panic!("{:?}", f.kind) };
    assert_eq!(*orientation, SweepOrientation::Direction(solidcraft_engine::doc::Dir(Vec3::X)));
    assert_eq!(twist.as_ref().map(|t| t.as_str()), Some("90 deg"));
    assert_eq!(scale.as_ref().map(|k| k.as_str()), Some("0.5"));
}

#[test]
fn a_plain_sweep_from_the_dialog_stores_no_options() {
    let mut h = Harness::new();
    let sketch = setup(&mut h);
    h.step(&json!({"call":"ui.start","params":{"command":"solid.sweep"}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    d.inputs[1].items = vec![Sel::SketchCurve { id: "up".into(), sketch: None }];
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let f = h.app.session.doc.features.last().unwrap();
    let FeatureKind::Sweep { orientation, twist, scale, .. } = &f.kind else { panic!("{:?}", f.kind) };
    assert!(orientation.is_perpendicular() && twist.is_none() && scale.is_none());
}

#[test]
fn editing_sweeps_keeps_their_options() {
    let mut h = Harness::new();
    setup(&mut h);
    h.app
        .run(
            "solid.sweep",
            json!({"sketch":"Q","path_sketch":"P","path":["up"],"orientation":[1,1,0],"twist":"30 deg","scale":"2","operation":"new","name":"Solid"}),
        )
        .unwrap();
    h.app
        .run(
            "surface.sweep",
            json!({"sketch":"Q","path_sketch":"P","path":["up"],"orientation":"parallel","twist":"45 deg","scale":"0.5","name":"Sheet"}),
        )
        .unwrap();
    h.frames(4);
    for name in ["Solid", "Sheet"] {
        let before = h.app.session.doc.find_feature(name).unwrap().clone();
        h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
        h.frames(8);
        match &h.app.dialog.as_ref().unwrap().kind {
            Kind::Sweep { opts, .. } | Kind::Surface(Surf::Sweep(opts)) => assert_eq!(opts.twist, if name == "Solid" { "30 deg" } else { "45 deg" }),
            k => panic!("{k:?}"),
        }
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
        // The options come back as they were (the profile pick is stored by its curves).
        let options = |k: &FeatureKind| match k {
            FeatureKind::Sweep { orientation, twist, scale, .. } | FeatureKind::SurfaceSweep { orientation, twist, scale, .. } => {
                (*orientation, twist.clone(), scale.clone())
            }
            k => panic!("{k:?}"),
        };
        let after = h.app.session.doc.find_feature(name).unwrap();
        assert_eq!(options(&after.kind), options(&before.kind), "{name}");
        assert_eq!(after.param_names, before.param_names, "{name}");
    }
}

/// #2216: the path of a sweep is the path the user chose: an edge of a body can be the path, as
/// a sketch's curve is. The pick goes into the Path input and the sweep is built with it.
#[test]
fn a_sweep_takes_a_body_edge_as_its_path() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    // A block whose upright corner edge is the path, and a square profile at its foot.
    h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [-40, 0, 0]})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Q"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-5,-5],"p1":[5,5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let sketch = h.app.session.doc.find_feature("Q").unwrap().id;
    h.step(&json!({"call":"ui.start","params":{"command":"solid.sweep"}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    // The Path input is the one the click goes to.
    d.active = 1;
    let Kind::Sweep { operation, .. } = &mut d.kind else { panic!("{:?}", d.kind) };
    *operation = 0;
    h.frames(2);
    // The corner edge of the block, where the cursor is.
    let at = h.at(&json!({"world": [0.0, 0.0, 5.0]})).unwrap();
    h.step(&json!({"move": at})).unwrap();
    h.step(&json!({"click": at})).unwrap();
    let path = h.app.dialog.as_ref().unwrap().inputs[1].items.clone();
    assert!(matches!(path.as_slice(), [Sel::Edge { .. }]), "the path took the edge: {path:?}");
    h.step(&json!({"widget":"OK"})).unwrap();
    let refused = h.app.dialog.as_ref().and_then(|d| d.error.clone());
    assert!(refused.is_none(), "the sweep was refused: {refused:?}");
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let f = h.app.session.doc.features.last().unwrap();
    assert!(matches!(f.kind, FeatureKind::Sweep { .. }), "{:?}", f.kind);
}

/// A rail picked in the dialog reaches the command with its sketch, the fit with it; editing the
/// sweep brings both back and keeps them.
#[test]
fn a_rail_picked_in_the_dialog_is_kept_through_an_edit() {
    let mut h = Harness::new();
    let sketch = setup(&mut h);
    h.app.run("sketch.create", json!({"plane":"XZ","name":"R"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[5,0],[10,-40]],"ids":["out"]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let rail_sketch = h.app.session.doc.find_feature("R").unwrap().id;
    h.step(&json!({"call":"ui.start","params":{"command":"solid.sweep"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:section:Advanced"}})).unwrap();
    assert!(h.widget("Rail Fit").is_some());
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    d.inputs[1].items = vec![Sel::SketchCurve { id: "up".into(), sketch: None }];
    d.inputs[3].items = vec![Sel::SketchCurve { id: "out".into(), sketch: None }];
    let Kind::Sweep { opts, .. } = &mut d.kind else { panic!("{:?}", d.kind) };
    opts.rail_fit = 1;
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    let rails = |h: &Harness| match &h.app.session.doc.find_feature(&name).unwrap().kind {
        FeatureKind::Sweep { rails, rail_fit, .. } => (rails.clone(), *rail_fit),
        k => panic!("{k:?}"),
    };
    let (stored, fit) = rails(&h);
    assert_eq!(stored, vec![solidcraft_engine::doc::SweepRail { sketch: solidcraft_engine::doc::SketchId(rail_sketch), curves: vec!["out".into()] }]);
    assert_eq!(fit, solidcraft_engine::doc::RailFit::Stretch);
    h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
    h.frames(8);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[3].items, vec![Sel::SketchCurve { id: "out".into(), sketch: Some(rail_sketch) }]);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    assert_eq!(rails(&h), (stored, fit));
}

/// #2216: a sweep whose path is a body's edge comes back on the body it was picked on, even
/// when another body has an edge at the same place: the path must not change on an edit.
#[test]
fn a_sweep_path_edge_is_restored_on_the_body_it_was_picked_on() {
    let mut h = Harness::new();
    // Two blocks that share the upright edge at (0, 20): the path is on the second one.
    h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [-40, 0, 0], "body_name": "B"})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [-40, 20, 0], "body_name": "A"})).unwrap();
    // A square profile at the origin.
    h.app.run("sketch.create", json!({"plane":"XY","name":"Q"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-5,-5],"p1":[5,5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    // The sweep's path is the shared edge, on the body A.
    h.app.run("solid.sweep", json!({"sketch":"Q","path_body":"A","path_edges":[[0,20,5]],"operation":"new","name":"Swept"})).unwrap();
    h.step(&json!({"call":"ui.editFeature","params":{"feature":"Swept"}})).unwrap();
    h.frames(8);
    let path = h.app.dialog.as_ref().unwrap().inputs[1].items.clone();
    match path.as_slice() {
        [Sel::Edge { body, .. }] => assert_eq!(body, "A", "the path was restored on another body"),
        other => panic!("the path was not restored as an edge: {other:?}"),
    }
}

/// A 40 x 20 x 10 block `B` whose four upright edges are rounded 4: its top outline is eight
/// edges, each running on smoothly into the next. The display index of its top front edge.
fn rounded_block(h: &mut Harness) -> (usize, Vec3) {
    h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "B"})).unwrap();
    h.app.run("solid.fillet", json!({"edges": [[0, 0, 5], [40, 0, 5], [0, 20, 5], [40, 20, 5]], "radius": 4, "tangent_chain": false})).unwrap();
    let world = crate::open_meshing::session_world(&h.app.session);
    let b = world.body("B").unwrap();
    let at = Vec3::new(20.0, 0.0, 10.0);
    // The straight top front edge: every point of it at y = 0, z = 10.
    let index = b.mesh().edges.iter().position(|e| e.len() > 1 && e.iter().all(|p| p.y.abs() < 1e-6 && (p.z - 10.0).abs() < 1e-6)).unwrap();
    (index, at)
}

/// The Sweep dialog's Path has a Tangent Chain switch: ticked, one pick of a body edge brings
/// every edge that continues it smoothly (the block's whole rounded top outline); a listed
/// row's remove button takes the whole chain out; unticked, the path is the picked edge again.
#[test]
fn sweep_path_tangent_chain_brings_the_smooth_outline() {
    let mut h = Harness::new();
    let (index, at) = rounded_block(&mut h);
    h.step(&json!({"call":"ui.start","params":{"command":"solid.sweep"}})).unwrap();
    h.frames(3);
    let top = |h: &Harness| -> Vec<Sel> { h.app.dialog.as_ref().unwrap().inputs[1].items.clone() };
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].chain, Some(false), "off by default");
    h.app.dialog.as_mut().unwrap().active = 1;
    let pick = Sel::Edge { body: "B".into(), index, point: at };
    h.app.dialog.as_mut().unwrap().pick(&h.app.session, pick.clone());
    h.frames(2);
    assert_eq!(top(&h), vec![pick.clone()], "off: only the picked edge");
    h.step(&json!({"click":{"handle":"dialog:Tangent Chain"}})).unwrap();
    h.frames(2);
    let chain = top(&h);
    assert_eq!(chain.len(), 8, "{chain:?}");
    assert!(chain.iter().all(|x| matches!(x, Sel::Edge { point, .. } if (point.z - 10.0).abs() < 1e-6)), "{chain:?}");
    // Picking the same edge again takes the chain out, and again brings it back.
    h.app.dialog.as_mut().unwrap().pick(&h.app.session, pick.clone());
    h.frames(2);
    assert!(top(&h).is_empty(), "{:?}", top(&h));
    h.app.dialog.as_mut().unwrap().pick(&h.app.session, pick.clone());
    h.frames(2);
    assert_eq!(top(&h).len(), 8);
    // A listed row of the chain (not the picked edge) takes the whole chain with it.
    let other = top(&h)[3].clone();
    let d = h.app.dialog.as_mut().unwrap();
    d.remove_listed(&h.app.session, 1, &other);
    h.frames(2);
    assert!(top(&h).is_empty(), "{:?}", top(&h));
    // Unticked: back to the picked edges.
    h.app.dialog.as_mut().unwrap().pick(&h.app.session, pick.clone());
    h.frames(2);
    assert_eq!(top(&h).len(), 8);
    h.step(&json!({"click":{"handle":"dialog:Tangent Chain"}})).unwrap();
    h.frames(2);
    assert_eq!(top(&h), vec![pick]);
}

/// Every input that takes a chain of edges offers Tangent Chain, off: both Sweeps' Path, the
/// Surface Loft's sections, Blend's two edge sets, Boundary's curves, Patch's boundary and
/// Ruled's edges.
#[test]
fn edge_chain_inputs_offer_tangent_chain() {
    let mut h = Harness::new();
    rounded_block(&mut h);
    for (command, labels) in [
        ("solid.sweep", &["Path"][..]),
        ("surface.sweep", &["Path"][..]),
        ("surface.loft", &["Sections"][..]),
        ("surface.blend", &["From edges", "To edges"][..]),
        ("surface.boundary", &["Curves", "Cross curves"][..]),
        ("surface.patch", &["Boundary"][..]),
        ("surface.ruled", &["Edges"][..]),
    ] {
        h.step(&json!({"call":"ui.start","params":{"command":command}})).unwrap();
        h.frames(2);
        let d = h.app.dialog.as_ref().unwrap_or_else(|| panic!("{command}: no dialog"));
        for inp in &d.inputs {
            let want = if labels.contains(&inp.label) { Some(false) } else { None };
            assert_eq!(inp.chain, want, "{command}: {}", inp.label);
        }
        h.app.dialog = None;
    }
}
