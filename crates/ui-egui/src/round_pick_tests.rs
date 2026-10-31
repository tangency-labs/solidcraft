//! A round edge or face picked in 3D stands for its circle: an axis input (Revolve, Circular
//! Pattern, Plane at Angle, Move's Rotate) takes a circular edge or a cylinder wall as its axis,
//! kept as a reference so the feature follows it; a point input takes a circle's centre; and
//! hovering one draws the axis or centre as construction geometry.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{AxisRef, FeatureKind, PatternKind, PlaneRef};
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::scenario::Harness;
use crate::viewport::Hit;

/// A Ø10 × 10 shaft at x = `cx` (40 mm) and a 2 × 6 profile on XZ beside it.
fn shaft() -> Harness {
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name": "cx", "expression": "40 mm"})).unwrap();
    h.app.run("solid.cylinder", json!({"base": ["cx", 0, 0], "radius": 5, "height": 10, "body_name": "Shaft"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XZ", "name": "Profile"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [46, -2], "p1": [48, -8]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.frames(2);
    h
}

/// The shaft's wall, top rim and top cap, as picks. The rim is picked at the middle of its
/// polyline: its first point is the vertex where the wall's seam meets it, as near the seam as
/// the rim.
fn picks(h: &Harness) -> (Sel, Sel, Sel) {
    use solidcraft_engine::kernel::{FaceSurface, face_surface};
    let st = h.app.session.world_state().unwrap();
    let b = st.body("Shaft").unwrap();
    let face = |want: fn(&FaceSurface) -> bool, point: Vec3| {
        let index = (0..b.body.face_count()).find(|i| face_surface(&b.body, *i).is_some_and(|f| want(&f))).unwrap();
        Sel::Face { body: "Shaft".into(), index, point }
    };
    let wall = face(|f| matches!(f, FaceSurface::Cylinder { .. }), Vec3::new(45.0, 0.0, 5.0));
    let cap = face(|f| matches!(f, FaceSurface::Plane { normal, .. } if normal.z > 0.5), Vec3::new(40.0, 0.0, 10.0));
    let rim = b
        .mesh()
        .edges
        .iter()
        .enumerate()
        .find_map(|(index, e)| {
            let c = solidcraft_engine::kernel::edge_circle(&b.body, index)?;
            (c.center.z > 9.0).then(|| Sel::Edge { body: "Shaft".into(), index, point: crate::viewport::polyline_mid(e) })
        })
        .unwrap();
    (wall, rim, cap)
}

fn hit(sel: &Sel) -> Hit {
    match sel {
        Sel::Face { body, index, point } => Hit::Face { body: body.clone(), index: *index, point: *point },
        Sel::Edge { body, index, point } => Hit::Edge { body: body.clone(), index: *index, mid: *point, point: *point },
        _ => panic!("not a model pick"),
    }
}

fn along_z_through(axis: (Vec3, Vec3), x: f64) -> bool {
    let (o, d) = axis;
    d.cross(Vec3::Z).len() < 1e-9 && (o.x - x).abs() < 1e-6 && o.y.abs() < 1e-6
}

/// The command the open dialog runs, with `sel` in input `input`.
fn command_with(h: &mut Harness, cmd: &str, input: usize, sel: &Sel, setup: impl Fn(&mut crate::dialogs::Dialog)) -> Value {
    h.app.start(cmd);
    let mut d = h.app.dialog.clone().unwrap();
    setup(&mut d);
    d.active = input;
    d.inputs[input].items = vec![sel.clone()];
    let cmds = crate::dialogs::apply_commands(&h.app, &d).unwrap();
    h.app.dialog = None;
    cmds.into_iter().find(|(c, _)| c == cmd).map(|(_, p)| p).unwrap()
}

fn profile(h: &Harness) -> Sel {
    Sel::Profile { sketch: h.app.session.doc.find_feature("Profile").unwrap().id, index: 0 }
}

#[test]
fn a_circular_edge_or_cylinder_wall_is_an_axis_and_a_flat_face_is_not() {
    let h = shaft();
    let (wall, rim, cap) = picks(&h);
    for sel in [&wall, &rim] {
        let axis = crate::dialogs::axis_of(&h.app, sel).unwrap();
        assert!(along_z_through(axis, 40.0), "{sel:?}: {axis:?}");
        assert!(axis.1.z > 0.0, "oriented the way the command finds it");
    }
    assert!(crate::dialogs::axis_of(&h.app, &cap).is_none());
}

#[test]
fn axis_inputs_take_only_edges_and_faces_that_have_an_axis() {
    let mut h = shaft();
    let (wall, rim, cap) = picks(&h);
    for cmd in ["solid.revolve", "solid.pattern.circular", "construct.plane.at_angle", "surface.revolve"] {
        h.app.start(cmd);
        let mut d = h.app.dialog.clone().unwrap();
        d.active = 1;
        assert_eq!(d.inputs[1].accept, crate::selection::AXIS_PICKS, "{cmd}");
        assert_eq!(d.candidate(&h.app.session, &hit(&wall)), Some(wall.clone()), "{cmd} takes the wall");
        assert_eq!(d.candidate(&h.app.session, &hit(&rim)), Some(rim.clone()), "{cmd} takes the rim");
        assert_eq!(d.candidate(&h.app.session, &hit(&cap)), None, "{cmd} refuses the flat cap");
        h.app.dialog = None;
    }
    // Move's Rotate too.
    h.app.start("solid.move");
    let mut d = h.app.dialog.clone().unwrap();
    d.kind = crate::dialogs_move::kind(crate::dialogs_move::ROTATE, &h.app.session.doc);
    d.inputs = crate::dialogs_move::inputs(crate::dialogs_move::ROTATE);
    d.active = 1;
    assert_eq!(d.candidate(&h.app.session, &hit(&wall)), Some(wall));
    assert_eq!(d.candidate(&h.app.session, &hit(&cap)), None);
}

#[test]
fn revolve_keeps_a_round_pick_as_a_reference_and_reopens_it() {
    let mut h = shaft();
    let (_, rim, _) = picks(&h);
    let p = profile(&h);
    let mut params = command_with(&mut h, "solid.revolve", 1, &rim, |d| d.inputs[0].items = vec![p.clone()]);
    assert!(params["axis"].get("edge").is_some(), "{params}");
    params["name"] = json!("Turn");
    h.app.run("solid.revolve", params).unwrap();
    let revolve_axis = |h: &Harness| match &h.app.session.doc.find_feature("Turn").unwrap().kind {
        FeatureKind::Revolve { axis, .. } => axis.clone(),
        _ => panic!("revolve"),
    };
    assert!(matches!(revolve_axis(&h), AxisRef::Model { .. }));
    // The cylinder moves; editing the revolve shows the rim where it is now and keeps the reference.
    h.app.run("parameters.change", json!({"name": "cx", "expression": "41 mm"})).unwrap();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Turn"}})).unwrap();
    h.frames(4);
    let d = h.app.dialog.as_ref().unwrap();
    let picked = d.inputs[1].items.first().cloned().unwrap();
    assert!(along_z_through(crate::dialogs::axis_of(&h.app, &picked).unwrap(), 41.0), "{picked:?}");
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(matches!(revolve_axis(&h), AxisRef::Model { .. }));
}

#[test]
fn circular_pattern_and_angle_plane_keep_a_cylinder_wall() {
    let mut h = shaft();
    let (wall, ..) = picks(&h);
    let params = command_with(&mut h, "solid.pattern.circular", 1, &wall, |d| d.inputs[0].items = vec![Sel::Body { name: "Shaft".into() }]);
    assert!(params["axis"].get("face").is_some(), "{params}");
    h.app.run("solid.pattern.circular", params).unwrap();
    assert!(
        h.app
            .session
            .doc
            .features
            .iter()
            .any(|f| matches!(f.kind, FeatureKind::Pattern { pattern: PatternKind::Circular { axis_ref: Some(_), .. }, .. }))
    );
    let params = command_with(&mut h, "construct.plane.at_angle", 1, &wall, |d| d.inputs[0].items = vec![Sel::Plane { name: "XZ".into() }]);
    assert!(params["axis"].get("face").is_some(), "{params}");
    h.app.run("construct.plane.at_angle", params).unwrap();
    assert!(
        h.app
            .session
            .doc
            .features
            .iter()
            .any(|f| matches!(&f.kind, FeatureKind::ConstructionPlane { plane: PlaneRef::AtAngle { line: Some(_), .. } }))
    );
}

#[test]
fn move_rotates_about_a_cylinder_wall_and_moves_from_a_circle_centre() {
    use crate::dialogs_move::{POINT_TO_POINT, ROTATE, inputs, kind, params};
    let mut h = shaft();
    h.app.run("solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [60, 0, 0], "body_name": "Lug"})).unwrap();
    let (wall, rim, _) = picks(&h);
    h.app.start("solid.move");
    let mut d = h.app.dialog.clone().unwrap();
    d.kind = kind(ROTATE, &h.app.session.doc);
    d.inputs = inputs(ROTATE);
    d.inputs[0].items = vec![Sel::Body { name: "Lug".into() }];
    d.inputs[1].items = vec![wall.clone()];
    let p = params(&h.app, &d).unwrap();
    let a: Vec<f64> = serde_json::from_value(p["axis"].clone()).unwrap();
    assert!((a[2].abs() - 1.0).abs() < 1e-9, "about the shaft's axis: {a:?}");
    // The chips say what the round picks stand for.
    let label = |d: &crate::dialogs::Dialog, i: usize, sel: &Sel| {
        crate::dialogs::selection_label(&h.app.session, crate::prefs::OriginPlaneNames::Xyz, d, &d.inputs[i], sel)
    };
    let wall_label = label(&d, 1, &wall);
    assert!(wall_label.starts_with("Shaft · face ") && wall_label.ends_with(" axis"), "{wall_label}");
    assert!(label(&d, 1, &rim).ends_with("· circle axis"), "{}", label(&d, 1, &rim));
    // Point to Point from the rim: from its centre.
    d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
    d.inputs = inputs(POINT_TO_POINT);
    d.inputs[0].items = vec![Sel::Body { name: "Lug".into() }];
    assert!(label(&d, 1, &rim).ends_with("· circle centre"), "{}", label(&d, 1, &rim));
    d.inputs[1].items = vec![rim];
    d.inputs[2].items = vec![Sel::Vertex { body: String::new(), point: Vec3::ZERO }];
    let p = params(&h.app, &d).unwrap();
    assert_eq!(p["translate"], json!(["-40 mm", "0 mm", "-10 mm"]), "the rim's centre onto the origin");
}

#[test]
fn move_takes_the_centre_of_a_circle_in_a_placed_component_where_it_is_shown() {
    use crate::dialogs_move::{POINT_TO_POINT, inputs, kind, params};
    let mut h = shaft();
    h.app.run("solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [60, 0, 0], "body_name": "Lug"})).unwrap();
    h.app.run("component.from_bodies", json!({"bodies": ["Shaft"]})).unwrap();
    let occ = h.app.session.doc.occurrences[0].id;
    // A design's first part is grounded when it is made.
    h.app.run("occurrence.ground", json!({"occurrence": occ, "grounded": false})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": occ, "translate": [0, 30, 0]})).unwrap();
    h.frames(2);
    // The rim as picked in the viewport: on the shaft where the occurrence puts it.
    let (_, rim, _) = picks(&h);
    let Sel::Edge { point, .. } = &rim else { panic!("{rim:?}") };
    assert!((point.y - 30.0).abs() < 5.0 + 1e-6, "picked in the world: {point:?}");
    h.app.start("solid.move");
    let mut d = h.app.dialog.clone().unwrap();
    d.kind = kind(POINT_TO_POINT, &h.app.session.doc);
    d.inputs = inputs(POINT_TO_POINT);
    d.inputs[0].items = vec![Sel::Body { name: "Lug".into() }];
    d.inputs[1].items = vec![rim];
    d.inputs[2].items = vec![Sel::Vertex { body: String::new(), point: Vec3::ZERO }];
    let p = params(&h.app, &d).unwrap();
    assert_eq!(p["translate"], json!(["-40 mm", "-30 mm", "-10 mm"]), "the centre where the component shows it");
}

#[test]
fn point_inputs_take_a_circles_centre() {
    let mut h = shaft();
    let (_, rim, _) = picks(&h);
    // A construction point keeps the edge, and lands on its centre.
    h.app.start("construct.point.vertex");
    let d = h.app.dialog.clone().unwrap();
    let got = d.candidate(&h.app.session, &hit(&rim)).unwrap();
    assert!(matches!(got, Sel::Edge { .. }), "{got:?}");
    let mut d = d;
    d.inputs[0].items = vec![got];
    let cmds = crate::dialogs::apply_commands(&h.app, &d).unwrap();
    h.app.dialog = None;
    for (c, p) in cmds {
        h.app.run(&c, p).unwrap();
    }
    let points = solidcraft_engine::view::construction_points(&h.app.session);
    assert!(points.iter().any(|p| p.2.dist(Vec3::new(40.0, 0.0, 10.0)) < 1e-6), "{points:?}");
    // Revolve's To Object takes the centre as a point.
    h.app.start("solid.revolve");
    let mut d = h.app.dialog.clone().unwrap();
    d.inputs.push(crate::selection::SelInput::new("To Object", crate::selection::FACES | crate::selection::VERTICES, false));
    d.active = d.inputs.len() - 1;
    let got = d.candidate(&h.app.session, &hit(&rim));
    let Some(Sel::Vertex { body, point }) = &got else { panic!("{got:?}") };
    assert_eq!(body, "Shaft");
    assert!(point.dist(Vec3::new(40.0, 0.0, 10.0)) < 1e-6, "{point:?}");
    h.app.dialog = None;
    // A hole placed from the top rim goes at its centre, on the cap under the cursor.
    let (_, _, cap) = picks(&h);
    h.app.start("solid.hole");
    let d = h.app.dialog.clone().unwrap();
    let hits = [hit(&rim), hit(&cap)];
    let snap = d.face_snap(&h.app.session, &hits);
    let Some(Sel::Face { point, .. }) = d.candidate_on(&h.app.session, &hit(&rim), snap.as_ref()) else { panic!("no place") };
    assert!(point.dist(Vec3::new(40.0, 0.0, 10.0)) < 1e-6, "{point:?}");
}

#[test]
fn hovering_a_round_pick_draws_its_axis_or_centre() {
    let mut h = shaft();
    let (wall, rim, cap) = picks(&h);
    h.app.start("solid.revolve");
    if let Some(d) = h.app.dialog.as_mut() {
        d.active = 1;
    }
    for (sel, lines) in [(&wall, 1), (&rim, 1), (&cap, 0)] {
        h.app.viewport.hover = Some(hit(sel));
        let (axes, centres) = crate::viewport::round_pick_feedback(&h.app);
        assert_eq!(axes.len(), lines, "{sel:?}");
        assert!(centres.is_empty());
        for (a, b) in axes {
            assert!(along_z_through((a, (b - a).normalized().unwrap()), 40.0), "{a:?} {b:?}");
        }
    }
    // With the axis picked, it stays drawn.
    h.app.viewport.hover = None;
    if let Some(d) = h.app.dialog.as_mut() {
        d.inputs[1].items = vec![rim.clone()];
    }
    assert_eq!(crate::viewport::round_pick_feedback(&h.app).0.len(), 1);
    // In a point input a circle shows its centre.
    h.app.dialog = None;
    h.app.start("construct.point.vertex");
    h.app.viewport.hover = Some(hit(&rim));
    let (axes, centres) = crate::viewport::round_pick_feedback(&h.app);
    assert!(axes.is_empty());
    assert_eq!(centres.len(), 1);
    assert!(centres[0].dist(Vec3::new(40.0, 0.0, 10.0)) < 1e-6);
    assert!(!matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Revolve { .. })));
}
