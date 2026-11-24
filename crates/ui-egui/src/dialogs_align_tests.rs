use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::dialogs::{Kind, apply_commands, edge_sel};
use crate::scenario::Harness;
use crate::viewport::Hit;

/// A 60 x 40 x 10 plate with a hole of radius 4 through it at (45, 20), and a peg of the hole's
/// size, 6 tall, standing on XY at (100, 20).
fn plate_and_peg() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 60, "width": 40, "height": 10, "body_name": "Plate"})).unwrap();
    h.app.run("solid.cylinder", json!({"radius": 4, "height": 12, "base": [45, 20, -1], "operation": "cut"})).unwrap();
    h.app.run("solid.cylinder", json!({"radius": 4, "height": 6, "base": [100, 20, 0], "body_name": "Peg"})).unwrap();
    h.app.cam.distance = 200.0;
    h.settle();
    h.app.start("solid.align");
    h.frames(2);
    h
}

/// Clicks the edge at `at` into input `input`, as the viewport hands it to the dialog.
fn pick_edge(h: &mut Harness, input: usize, at: Vec3) {
    let Some(Sel::Edge { body, index, point }) = edge_sel(&h.app.session, at) else { panic!("no edge at {at:?}") };
    let d = h.app.dialog.as_mut().unwrap();
    d.active = input;
    let sel = d.candidate(&h.app.session, &Hit::Edge { body, index, mid: point, point }).expect("the input takes the edge");
    d.pick(&h.app.session, sel);
}

fn set_flip(h: &mut Harness, on: bool) {
    if let Some(Kind::Align { flip, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *flip = on;
    }
}

/// Applies the dialog's command and gives `body`'s bounds after it.
fn apply_and_bound(h: &mut Harness, body: &str) -> (serde_json::Value, [Vec3; 2]) {
    let (cmd, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert_eq!(cmd, "solid.align");
    h.app.run(&cmd, p.clone()).unwrap();
    let st = crate::open_meshing::session_world(&h.app.session);
    let b = st.body(body).unwrap().mesh().bounds();
    (p, [b.min, b.max])
}

fn assert_on_hole_axis([min, max]: [Vec3; 2], z: [f64; 2], why: &str) {
    let (cx, cy) = ((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
    assert!((cx - 45.0).abs() < 0.05 && (cy - 20.0).abs() < 0.05, "{why}: not on the hole's axis: {min:?} {max:?}");
    assert!((min.z - z[0]).abs() < 1e-6 && (max.z - z[1]).abs() < 1e-6, "{why}: z {z:?}: {min:?} {max:?}");
}

/// A round edge picked for From and To is its circle (a frame with an axis), not its centre
/// point: the peg lands concentric on the hole's rim, and Flip turns it over into the hole.
#[test]
fn round_edges_align_as_circles_and_flip_turns_the_body_over() {
    for (flip, z) in [(false, [10.0, 16.0]), (true, [4.0, 10.0])] {
        let mut h = plate_and_peg();
        h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Peg".into() }];
        set_flip(&mut h, flip);
        pick_edge(&mut h, 1, Vec3::new(104.0, 20.0, 0.0));
        pick_edge(&mut h, 2, Vec3::new(49.0, 20.0, 10.0));
        let (p, b) = apply_and_bound(&mut h, "Peg");
        assert!(p["from_frame"]["circle"].is_array() && p["to_frame"]["circle"].is_array(), "{p}");
        assert!(p.get("from").is_none() && p.get("to").is_none(), "{p}");
        assert_on_hole_axis(b, z, &format!("flip {flip}"));
    }
}

/// Clicks the edge at `at` into the input the dialog has active.
fn click_edge(h: &mut Harness, at: Vec3) {
    let active = h.app.dialog.as_ref().unwrap().active;
    pick_edge(h, active, at);
}

/// No Bodies step: the dialog opens on From and goes on to To, and with Bodies left empty what the
/// From pick is on moves. Bodies, when given, overrides it.
#[test]
fn the_body_the_from_pick_is_on_moves_without_picking_bodies() {
    let mut h = plate_and_peg();
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1, "opens on From");
    click_edge(&mut h, Vec3::new(104.0, 20.0, 0.0));
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 2, "goes on to To, not back to Bodies");
    click_edge(&mut h, Vec3::new(49.0, 20.0, 10.0));
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["bodies"], json!(["Peg"]), "{}", commands[0].1);
    let (_, b) = apply_and_bound(&mut h, "Peg");
    assert_on_hole_axis(b, [10.0, 16.0], "the peg the From edge is on");
    // An override moves what it names, From still on the peg.
    let mut h = plate_and_peg();
    h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [120, 0, 0], "body_name": "Cap"})).unwrap();
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Peg".into() }, Sel::Body { name: "Cap".into() }];
    pick_edge(&mut h, 1, Vec3::new(104.0, 20.0, 0.0));
    pick_edge(&mut h, 2, Vec3::new(49.0, 20.0, 10.0));
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["bodies"], json!(["Peg", "Cap"]), "{}", commands[0].1);
    // A To pick on what moves is no frame (the command takes none there): it stays as picked, and
    // as a joint origin it is refused in the dialog's words.
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items.clear();
    pick_edge(&mut h, 2, Vec3::new(104.0, 20.0, 6.0));
    let (_, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert!(p.get("to_frame").is_none() && p["to"]["snap"] == json!("edge_mid"), "{p}");
    h.app.dialog.as_mut().unwrap().extra.insert("to_snap".into(), json!("origin"));
    let refused = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert!(refused.contains("the To pick is on a body that moves"), "{refused}");
}

/// In Components mode the component the From pick is on moves when none is picked.
#[test]
fn the_component_the_from_pick_is_on_moves_without_picking_components() {
    let mut h = Harness::new();
    let comp = h.app.run("component.create", json!({"name": "Extrusion"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 60, "body_name": "Beam"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"component": comp, "translate": [40, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.start("solid.align");
    h.frames(2);
    let from = crate::dialogs::face_sel(&h.app.session, Vec3::new(45.0, 5.0, 0.0)).unwrap();
    let to = crate::dialogs::face_sel(&h.app.session, Vec3::new(10.0, 5.0, 30.0)).unwrap();
    let d = h.app.dialog.as_mut().unwrap();
    d.kind = Kind::Align { flip: false, components: true, axes: [true; 3], angle: String::new() };
    d.inputs[0].label = "Components";
    d.inputs[1].items = vec![from];
    d.inputs[2].items = vec![to];
    let (cmd, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert_eq!((cmd.as_str(), &p["paths"]), ("occurrence.align", &json!([[second]])), "{p}");
}

/// Clicks the face at `at` into input `input`.
fn pick_face(h: &mut Harness, input: usize, at: Vec3) {
    let Some(Sel::Face { body, index, point }) = crate::dialogs::face_sel(&h.app.session, at) else { panic!("no face at {at:?}") };
    let d = h.app.dialog.as_mut().unwrap();
    d.active = input;
    let sel = d.candidate(&h.app.session, &Hit::Face { body, index, point }).expect("the input takes the face");
    d.pick(&h.app.session, sel);
}

/// No snap to choose first: Auto takes what is under the cursor. A flat face is a face frame (the
/// faces meet centre on centre), a round face its circle, a straight edge its middle and a vertex
/// itself; the Snap rows still choose otherwise.
#[test]
fn auto_takes_the_kind_of_pick_from_what_is_under_the_cursor() {
    for (flip, z) in [(false, [4.0, 10.0]), (true, [-2.0, 4.0])] {
        let mut h = plate_and_peg();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 4, "corner": [0, 50, 0], "body_name": "Pad"})).unwrap();
        set_flip(&mut h, flip);
        pick_face(&mut h, 1, Vec3::new(101.0, 21.0, 0.0));
        pick_face(&mut h, 2, Vec3::new(5.0, 55.0, 4.0));
        let (p, [min, max]) = apply_and_bound(&mut h, "Peg");
        assert!(p["from_frame"]["face"].is_array() && p["to_frame"]["face"].is_array(), "{p}");
        // The peg's bottom centre on the pad's top centre, (10, 60, 4).
        let (cx, cy) = ((min.x + max.x) / 2.0, (min.y + max.y) / 2.0);
        assert!((cx - 10.0).abs() < 0.05 && (cy - 60.0).abs() < 0.05, "flip {flip}: {min:?} {max:?}");
        assert!((min.z - z[0]).abs() < 1e-6 && (max.z - z[1]).abs() < 1e-6, "flip {flip}: {min:?} {max:?}");
    }
    let mut h = plate_and_peg();
    pick_face(&mut h, 1, Vec3::new(104.0, 20.0, 3.0));
    pick_edge(&mut h, 2, Vec3::new(30.0, 0.0, 10.0));
    let (_, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert!(p["from_frame"]["circle"].is_array(), "a round face is its circle: {p}");
    assert_eq!(p["to"]["snap"], json!("edge_mid"), "a straight edge is its middle: {p}");
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[2].items = vec![Sel::Vertex { body: "Plate".into(), point: Vec3::new(0.0, 0.0, 10.0) }];
    let (_, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert_eq!(p["to"], json!([0.0, 0.0, 10.0]), "a vertex is itself: {p}");
    // As Picked keeps a flat face as the picked point and its normal.
    let d = h.app.dialog.as_mut().unwrap();
    d.extra.insert("from_snap".into(), json!("picked"));
    d.extra.insert("to_snap".into(), json!("picked"));
    pick_face(&mut h, 1, Vec3::new(101.0, 21.0, 0.0));
    pick_face(&mut h, 2, Vec3::new(10.0, 10.0, 10.0));
    let (_, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert!(p["from_face"].is_array() && p["to_face"].is_array(), "{p}");
}

/// Whether the dialog's Flip is drawn disabled.
fn flip_disabled(h: &Harness) -> bool {
    let tree = h.output.accesskit_update.as_ref().expect("an accessibility tree");
    let flip = tree.nodes.iter().find(|(_, n)| n.label() == Some("Flip"));
    flip.expect("a Flip option").1.is_disabled()
}

fn flip_why(h: &Harness) -> Option<&'static str> {
    crate::dialogs_align::flip_blocked(&h.app, h.app.dialog.as_ref().unwrap())
}

/// Flip never silently does nothing, and is never off where the command would take it: as the
/// command decides, it is off, saying why, only where neither side faces a way (two points). One
/// side that does (a face, a face or circle centre, a circle, a plane) is enough.
#[test]
fn flip_is_disabled_with_the_reason_only_where_neither_side_faces_a_way() {
    let mut h = plate_and_peg();
    h.frames(2);
    assert!(!flip_disabled(&h), "nothing picked yet: nothing to say");
    let peg_rim = Sel::Vertex { body: "Peg".into(), point: Vec3::new(104.0, 20.0, 0.0) };
    let plate_corner = Sel::Vertex { body: "Plate".into(), point: Vec3::new(0.0, 0.0, 10.0) };
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[1].items = vec![peg_rim.clone()];
    d.inputs[2].items = vec![plate_corner.clone()];
    let why = flip_why(&h);
    assert!(why.is_some_and(|id| solidcraft_ui_text::tr(id).contains("two points")), "two vertices face no way: {why:?}");
    h.frames(2);
    assert!(flip_disabled(&h), "drawn disabled");
    pick_edge(&mut h, 2, Vec3::new(30.0, 0.0, 10.0));
    assert!(flip_why(&h).is_some(), "an edge's middle faces no way either");
    // One side facing a way is enough, whichever side it is.
    for to in [Vec3::new(5.0, 5.0, 10.0), Vec3::new(49.0, 20.0, 10.0)] {
        if to.x > 40.0 {
            pick_edge(&mut h, 2, to);
        } else {
            pick_face(&mut h, 2, to);
        }
        assert_eq!(flip_why(&h), None, "To at {to:?}");
        h.frames(2);
        assert!(!flip_disabled(&h), "To at {to:?}");
    }
    pick_face(&mut h, 1, Vec3::new(101.0, 21.0, 0.0));
    h.app.dialog.as_mut().unwrap().inputs[2].items = vec![plate_corner];
    assert_eq!(flip_why(&h), None, "a face From against a vertex To");
    // A face's or a circle's centre faces a way as the command takes it.
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[1].items = vec![peg_rim];
    for (snap, at) in [("center", Vec3::new(5.0, 5.0, 10.0)), ("circle", Vec3::new(49.0, 20.0, 10.0))] {
        h.app.dialog.as_mut().unwrap().extra.insert("to_snap".into(), json!(snap));
        if snap == "center" {
            pick_face(&mut h, 2, at);
        } else {
            pick_edge(&mut h, 2, at);
        }
        assert_eq!(flip_why(&h), None, "a {snap} To");
    }
}

/// Whether the dialog's Flip is drawn ticked.
fn flip_shown_on(h: &Harness) -> bool {
    let tree = h.output.accesskit_update.as_ref().expect("an accessibility tree");
    let flip = tree.nodes.iter().find(|(_, n)| n.label() == Some("Flip")).expect("a Flip option");
    flip.1.toggled() == Some(egui::accesskit::Toggled::True)
}

/// A remembered Flip never blocks two points: there it is shown off and sent off, so the command
/// applies; the remembered value comes back once a side faces a way again.
#[test]
fn a_remembered_flip_is_sent_off_where_neither_side_faces_a_way_and_comes_back() {
    let mut h = plate_and_peg();
    h.app.preferences.remember_dialog_values = true;
    set_flip(&mut h, true);
    let applied = h.app.dialog.take().unwrap();
    crate::dialogs::remember(&mut h.app, &applied);
    h.app.start("solid.align");
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::Align { flip: true, .. }), "Flip is remembered");
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[1].items = vec![Sel::Vertex { body: "Peg".into(), point: Vec3::new(104.0, 20.0, 0.0) }];
    d.inputs[2].items = vec![Sel::Vertex { body: "Plate".into(), point: Vec3::new(0.0, 0.0, 10.0) }];
    h.frames(2);
    assert!(flip_disabled(&h) && !flip_shown_on(&h), "two points: Flip shown off and disabled");
    let (p, [min, _]) = apply_and_bound(&mut h, "Peg");
    assert_eq!(p["flip"], json!(false), "{p}");
    assert!((min.x + 8.0).abs() < 0.05 && (min.z - 10.0).abs() < 1e-6, "the rim point moved onto the corner: {min:?}");
    pick_face(&mut h, 2, Vec3::new(5.0, 5.0, 10.0));
    h.frames(2);
    assert!(!flip_disabled(&h) && flip_shown_on(&h), "a face To: the remembered Flip is back");
    let (_, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert_eq!(p["flip"], json!(true), "{p}");
}
