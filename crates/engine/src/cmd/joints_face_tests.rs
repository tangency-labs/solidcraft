//! Joint origins found by their face's name: a part jointed to a hole stays on it, on the same
//! side and turned the same way, as the design's parameters change.

use serde_json::{Value, json};
use solidcraft_doc::joints::{Snap, XRef};
use solidcraft_geom::Vec3;

use super::tests::{occ, place, run};
use crate::Session;

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

#[test]
fn joint_origin_names_in_face_or_circle_explain_the_correct_form() {
    let mut s = Session::default();
    for (name, corner) in [("A", [0, 0, 0]), ("B", [40, 0, 0])] {
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": corner}));
        run(&mut s, "component.activate", json!({"component": "root"}));
    }
    run(&mut s, "joint.origin", json!({"name": "TopA", "occurrence": "A:1", "face": [10, 10, 10]}));
    run(&mut s, "joint.origin", json!({"name": "BotB", "occurrence": "B:1", "face": [50, 10, 0]}));
    for field in ["face", "circle"] {
        for (side, name, occurrence) in [("a", "TopA", "A:1"), ("b", "BotB", "B:1")] {
            let mut params = json!({"type": "rigid", "a": "TopA", "b": "BotB"});
            params[side] = json!({"occurrence": occurrence, field: name});
            let error = s.execute("joint.create", &params).unwrap_err().to_string();
            assert_eq!(
                error,
                format!("invalid parameters for `joint.create`: `{name}` is a joint origin: pass it as the whole origin, not as a `{field}` name")
            );
            assert!(s.doc.assembly.joints.is_empty());
        }
    }
    let error = s.execute("joint.create", &json!({"type": "rigid", "a": {"face": "Missing"}, "b": "BotB"})).unwrap_err().to_string();
    assert!(error.contains("no face is named `Missing`"), "{error}");
    let result = run(&mut s, "joint.create", json!({"type": "rigid", "a": "TopA", "b": "BotB"}));
    assert_eq!(result["errors"].as_array().map(Vec::len), Some(0), "{result}");
    assert_eq!(s.doc.assembly.joints.len(), 1);
}

/// Widths swept: small steps (a hole's wall in a 1 mm plate is a few long facets, and a pick
/// stored at the old width lies beside the moved hole) and large ones.
const WIDTHS: [f64; 9] = [302.0, 304.0, 310.0, 330.0, 400.0, 450.0, 510.0, 301.0, 300.0];

#[test]
fn secondary_alignment_hole_axis_keeps_identity_but_rejects_ambiguous_seam_and_wrong_owner() {
    let (mut s, _, _, mut p) = alignment_boxes();
    run(&mut s, "component.activate", json!({"component":"A"}));
    run(&mut s, "solid.hole", json!({"position":[10,10,10],"diameter":6}));
    let st = s.model.state();
    let body = st.body("Body1").unwrap();
    let names = solidcraft_doc::naming::edge_names(body);
    let seam = names.iter().find(|name| solidcraft_doc::naming::locate_edge(body, name, Vec3::ZERO).is_ok_and(|h| !h.verified)).unwrap();
    p["a_alignment"] = json!({"reference":{"type":"edge","name":seam}});
    assert!(s.execute("joint.create", &p).unwrap_err().to_string().contains("ambiguous"));
    let component = s.doc.find_component("A").unwrap();
    let wall = solidcraft_doc::joints::face_name_at(&s.doc, &st, component, Vec3::new(13.0, 10.0, 5.0), true).unwrap();
    // Use X-facing primaries so the hole's Z axis supplies a valid secondary direction.
    p["a"]["face"] = json!([20, 10, 5]);
    p["b"]["face"] = json!([30, 10, 5]);
    p["a_alignment"] = json!({"reference":{"type":"cylinder","name":wall}});
    p.as_object_mut().unwrap().remove("b_alignment");
    run(&mut s, "joint.create", p);
    let before = s.doc.clone();
    let error =
        s.execute("joint.edit", &json!({"joint":"Aligned","b_alignment":{"reference":{"type":"cylinder","name":wall}}})).unwrap_err().to_string();
    assert!(error.contains("origin's component"), "{error}");
    assert_eq!(s.doc, before);
}

fn alignment_boxes() -> (Session, String, String, Value) {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "jointTurn", "expression": "17 deg"}));
    for (name, x) in [("A", 0), ("B", 30)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": "A:1", "grounded": true}));
    let name = |c, p| solidcraft_doc::joints::face_name_at(&s.doc, &s.model.state(), s.doc.find_component(c).unwrap(), p, false).unwrap();
    let a = name("A", Vec3::new(20.0, 10.0, 5.0));
    let b = name("B", Vec3::new(40.0, 20.0, 5.0));
    let p = json!({"type": "rigid", "name": "Aligned", "angle": "jointTurn",
        "a": {"occurrence": "A", "face": [10, 10, 10]}, "b": {"occurrence": "B", "face": [40, 10, 0]},
        "a_alignment": {"face": a}, "b_alignment": {"face": b}});
    (s, a, b, p)
}

#[test]
fn secondary_alignment_feedback_named_edges_save_reopen_and_reject_missing_identity() {
    let (mut s, _, _, mut p) = alignment_boxes();
    let state = s.model.state();
    let body = state.body("Body1").unwrap();
    let mesh = body.mesh();
    let index = mesh.edges.iter().position(|e| e.len() >= 2 && (e.last().unwrap().x - e[0].x).abs() > 19.0).unwrap();
    let name = solidcraft_doc::naming::edge_names(body)[index].clone();
    p["a_alignment"] = json!({"reference":{"type":"edge","name":name},"reverse":true});
    run(&mut s, "joint.create", p);
    let frame = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &s.doc.assembly.joints[0].a).unwrap();
    assert!(Vec3::new(frame[0][0], frame[0][1], frame[0][2]).dot(Vec3::X).abs() > 0.999);
    let path = std::env::temp_dir().join(format!("solidcraft-directional-alignment-{}.solidcraft", std::process::id()));
    run(&mut s, "file.save", json!({"path":path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path":path}));
    let old = s.doc.assembly.joints[0].clone();
    assert!(matches!(&old.a.snap, Snap::FaceCenter { x: Some(XRef::Reference { reverse: true, .. }), .. }));
    assert!(s.execute("joint.edit", &json!({"joint":"Aligned","a_alignment":{"reference":{"type":"edge","name":"missing"}}})).is_err());
    assert_eq!(s.doc.assembly.joints[0], old);
    run(&mut s, "joint.edit", json!({"joint":"Aligned","a_alignment":null}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.joints[0], old);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn secondary_alignment_feedback_construction_and_component_axes_keep_author_identity() {
    let (mut s, _, _, mut p) = alignment_boxes();
    run(&mut s, "component.activate", json!({"component":"A"}));
    run(&mut s, "construct.axis.two_points", json!({"a":{"point":[0,0,0]},"b":{"point":[20,0,0]},"name":"GuideAxis"}));
    let axis = s.doc.features.iter().find(|f| f.name == "GuideAxis").unwrap().id;
    run(&mut s, "construct.plane.three_points", json!({"a":{"point":[0,0,0]},"b":{"point":[0,10,0]},"c":{"point":[0,0,10]},"name":"GuidePlane"}));
    let plane = s.doc.features.iter().find(|f| f.name == "GuidePlane").unwrap().id;
    run(&mut s, "component.activate", json!({"component":"root"}));
    p["a_alignment"] = json!({"reference":{"type":"construction","feature":axis}});
    run(&mut s, "joint.create", p);
    for reference in
        [json!({"type":"construction","feature":plane}), json!({"type":"origin_axis","axis":"X"}), json!({"type":"origin_plane","plane":"YZ"})]
    {
        run(&mut s, "joint.edit", json!({"joint":"Aligned","a_alignment":{"reference":reference}}));
        let frame = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &s.doc.assembly.joints[0].a).unwrap();
        assert!(Vec3::new(frame[0][0], frame[0][1], frame[0][2]).dot(Vec3::X) > 0.999);
    }
    let old = s.doc.assembly.joints[0].clone();
    for reference in [json!({"type":"construction","feature":99999}), json!({"type":"origin_axis","axis":"Z"})] {
        assert!(s.execute("joint.edit", &json!({"joint":"Aligned","a_alignment":{"reference":reference}})).is_err());
        assert_eq!(s.doc.assembly.joints[0], old);
    }
    assert!(s.execute("joint.edit", &json!({"joint":"Aligned","b_alignment":{"reference":{"type":"construction","feature":axis}}})).is_err());
    assert_eq!(s.doc.assembly.joints[0], old);
}

#[test]
fn secondary_alignment_feedback_sketch_line_is_saved_by_sketch_and_curve_identity() {
    let (mut s, _, _, mut p) = alignment_boxes();
    run(&mut s, "component.activate", json!({"component":"A"}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"AlignmentSketch"}));
    let sketch = s.active_sketch.unwrap();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[20,0]]}));
    let curve = s.model.state().sketch(sketch).unwrap().sketch.curves[0].id.clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    p["a_alignment"] = json!({"reference":{"type":"sketch_line","sketch":sketch,"curve":curve}});
    run(&mut s, "joint.create", p);
    let old = s.doc.assembly.joints[0].clone();
    let frame = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &old.a).unwrap();
    assert!(Vec3::new(frame[0][0], frame[0][1], frame[0][2]).dot(Vec3::X) > 0.999);
    assert!(
        s.execute("joint.edit", &json!({"joint":"Aligned","a_alignment":{"reference":{"type":"sketch_line","sketch":sketch,"curve":"gone"}}}))
            .is_err()
    );
    assert_eq!(s.doc.assembly.joints[0], old);
}

#[test]
fn secondary_alignment_feedback_cylinder_and_circular_edge_follow_geometry_changes() {
    let (mut s, _, _, mut p) = alignment_boxes();
    run(&mut s, "component.activate", json!({"component":"A"}));
    run(&mut s, "parameters.change", json!({"name":"guideRadius","expression":"5 mm"}));
    run(&mut s, "solid.cylinder", json!({"base":[70,0,0],"axis":[1,0,0],"radius":"guideRadius","height":20,"body_name":"Guide"}));
    let component = s.doc.find_component("A").unwrap();
    let face = solidcraft_doc::joints::face_name_at(&s.doc, &s.model.state(), component, Vec3::new(80.0, 5.0, 0.0), true).unwrap();
    p["a_alignment"] = json!({"reference":{"type":"cylinder","name":face}});
    run(&mut s, "joint.create", p);
    run(&mut s, "parameters.change", json!({"name":"guideRadius","expression":"7 mm"}));
    let state = s.model.state();
    let body = state.body("Guide").unwrap();
    let mesh = body.mesh();
    let index = mesh.edges.iter().position(|e| e.len() > 8 && e.iter().all(|p| (p.x - e[0].x).abs() < 1e-6)).unwrap();
    let edge = solidcraft_doc::naming::edge_names(body)[index].clone();
    run(&mut s, "joint.edit", json!({"joint":"Aligned","a_alignment":{"reference":{"type":"edge","name":edge}}}));
    run(&mut s, "parameters.change", json!({"name":"guideRadius","expression":"9 mm"}));
    let frame = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &s.doc.assembly.joints[0].a).unwrap();
    assert!(Vec3::new(frame[0][0], frame[0][1], frame[0][2]).dot(Vec3::X).abs() > 0.999);
    assert!(near(place(&s, "B", Vec3::new(40.0, 10.0, 0.0)), Vec3::new(10.0, 10.0, 10.0)));
}

#[test]
fn secondary_alignment_inverts_each_frame_without_changing_angle_or_anchor() {
    let (mut s, a, b, p) = alignment_boxes();
    run(&mut s, "joint.create", p);
    let old = s.doc.assembly.joints[0].clone();
    for ra in [false, true] {
        for rb in [false, true] {
            for flip in [false, true] {
                run(
                    &mut s,
                    "joint.edit",
                    json!({"joint": "Aligned", "flip": flip,
                    "a_alignment": {"face": a, "reverse": ra}, "b_alignment": {"face": b, "reverse": rb}}),
                );
                let j = &s.doc.assembly.joints[0];
                assert_eq!(j.exprs.angle.as_deref(), Some("jointTurn"));
                assert!((j.angle - 17_f64.to_radians()).abs() < 1e-10);
                for (origin, want) in [(&j.a, if ra { -Vec3::X } else { Vec3::X }), (&j.b, if rb { -Vec3::Y } else { Vec3::Y })] {
                    let m = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), origin).unwrap();
                    assert!(near(Vec3::new(m[0][0], m[0][1], m[0][2]), want));
                }
                assert!(near(place(&s, "B", Vec3::new(40.0, 10.0, 0.0)), Vec3::new(10.0, 10.0, 10.0)));
                // Analytic world direction of B's +Y: mate preserves A's x, Angle turns it.
                let axis = place(&s, "B", Vec3::new(40.0, 11.0, 0.0)) - place(&s, "B", Vec3::new(40.0, 10.0, 0.0));
                let sign = if ra == rb { 1.0 } else { -1.0 };
                let angle = 17_f64.to_radians();
                assert!(near(axis, Vec3::new(angle.cos(), angle.sin(), 0.0) * sign), "{axis:?}, {ra} {rb} {flip}");
            }
        }
    }
    // Primary references stay by name, including after clearing the secondary direction.
    run(&mut s, "joint.edit", json!({"joint": "Aligned", "a_alignment": null}));
    let j = &s.doc.assembly.joints[0];
    assert_eq!(j.a.occurrence, old.a.occurrence);
    assert!(
        matches!((&j.a.snap, &old.a.snap), (Snap::FaceCenter { face: a, x: Some(XRef::Dir(_)), .. }, Snap::FaceCenter { face: b, .. }) if a == b)
    );
    assert!(matches!(&j.b.snap, Snap::FaceCenter { x: Some(XRef::Planar { .. }), .. }));
}

#[test]
fn secondary_alignment_keeps_motion_values_limits_and_degrees_of_freedom() {
    for kind in [
        "rigid",
        "revolute",
        "slider",
        "cylindrical",
        "pin_slot",
        "planar",
        "ball",
        "parallel",
        "perpendicular",
        "angle",
        "point_on_plane",
        "point_on_line",
        "line_on_plane",
    ] {
        let (mut s, a, b, mut p) = alignment_boxes();
        p["type"] = json!(kind);
        p.as_object_mut().unwrap().remove("a_alignment");
        p.as_object_mut().unwrap().remove("b_alignment");
        run(&mut s, "joint.create", p);
        let n = s.doc.assembly.joints[0].values.len();
        for i in 0..n {
            run(&mut s, "joint.limits", json!({"joint": "Aligned", "index": i, "min": -20, "max": 20}));
        }
        let before = s.doc.assembly.joints[0].clone();
        let dof = run(&mut s, "joint.list", json!({}))["dof"].clone();
        run(&mut s, "joint.edit", json!({"joint": "Aligned", "a_alignment": {"face": a, "reverse": true}, "b_alignment": {"face": b}}));
        let after = &s.doc.assembly.joints[0];
        assert_eq!(
            (&after.values, &after.limits, &after.exprs, after.angle, after.offset),
            (&before.values, &before.limits, &before.exprs, before.angle, before.offset),
            "{kind}"
        );
        assert_eq!(run(&mut s, "joint.list", json!({}))["dof"], dof, "{kind}");
    }
}

#[test]
fn secondary_alignment_invalid_edits_are_atomic() {
    let (mut s, a, b, p) = alignment_boxes();
    run(&mut s, "joint.create", p);
    let top = solidcraft_doc::joints::face_name_at(&s.doc, &s.model.state(), s.doc.find_component("A").unwrap(), Vec3::new(10.0, 10.0, 10.0), false)
        .unwrap();
    for alignment in [
        json!({"face": b}),
        json!({"face": "missing"}),
        json!({"face": top}),
        json!({"face": a, "reverse": "yes"}),
        json!({"face": a, "occurrence": "B"}),
        json!([0, 0, 0]),
    ] {
        let before = serde_json::to_value(&*s.doc).unwrap();
        assert!(s.execute("joint.edit", &json!({"joint": "Aligned", "angle": 90, "a_alignment": alignment})).is_err(), "{alignment}");
        assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
    }
    let component = s.doc.find_component("A").unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [100,0,0]}))["occurrence"].clone();
    assert!(
        s.execute("joint.edit", &json!({"joint":"Aligned", "a_alignment":{"face":a,"occurrence":second}})).is_err(),
        "same definition, different occurrence is still a mismatch"
    );
    // An explicit point's x support remains independent; alignment UI doesn't turn it into a face.
    assert!(s.execute("joint.create", &json!({"type": "rigid", "a": {"occurrence": "A", "point": [0,0,0]}, "b": {"occurrence": "B", "point": [0,0,0]}, "a_alignment": {"face": a}})).is_err());
}

#[test]
fn secondary_alignment_roundtrips_and_reports_a_lost_reference() {
    let (mut s, _, b, p) = alignment_boxes();
    run(&mut s, "joint.create", p);
    let path = std::env::temp_dir().join(format!("solidcraft-secondary-alignment-{}.solidcraft", std::process::id()));
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    run(&mut s, "parameters.change", json!({"name": "jointTurn", "expression": "32 deg"}));
    assert_eq!(s.doc.assembly.joints[0].exprs.angle.as_deref(), Some("jointTurn"));
    assert!((s.doc.assembly.joints[0].angle - 32_f64.to_radians()).abs() < 1e-10);
    let before = s.doc.assembly.joints[0].clone();
    run(&mut s, "joint.edit", json!({"joint": "Aligned", "b_alignment": {"face": b, "reverse": true}}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.joints[0], before);
    run(&mut s, "edit.redo", json!({}));
    let placed = place(&s, "B", Vec3::new(40.0, 10.0, 0.0));
    // A serialized reference which later disappears is reported, never replaced by nearby geometry.
    if let Snap::FaceCenter { x, .. } = &mut s.doc_mut().assembly.joints[0].a.snap {
        *x = Some(XRef::Planar { planar_face: "missing".into(), reverse: false });
    }
    let r = run(&mut s, "joint.solve", json!({}));
    assert_eq!(r["errors"].as_array().unwrap().len(), 1);
    assert!(near(place(&s, "B", Vec3::new(40.0, 10.0, 0.0)), placed));
    let r = run(&mut s, "joint.edit", json!({"joint":"Aligned", "a_alignment":null}));
    assert_eq!(r["errors"].as_array().unwrap().len(), 0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn secondary_alignment_follows_a_parametric_guide_face_instead_of_a_captured_vector() {
    let (mut s, _, _, mut p) = alignment_boxes();
    run(&mut s, "component.activate", json!({"component":"A"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":[70,0,0],"body_name":"Guide","operation":"new"}));
    run(&mut s, "parameters.change", json!({"name":"guideTurn","expression":"0 deg"}));
    run(&mut s, "solid.move", json!({"bodies":["Guide"],"axis":[0,0,1],"angle":"guideTurn"}));
    let component = s.doc.find_component("A").unwrap();
    let face = solidcraft_doc::joints::face_name_at(&s.doc, &s.model.state(), component, Vec3::new(80.0, 5.0, 5.0), false).unwrap();
    p["a_alignment"] = json!({"face":face});
    run(&mut s, "joint.create", p);
    for degrees in [40.0, -25.0, 80.0] {
        run(&mut s, "parameters.change", json!({"name":"guideTurn","expression":format!("{degrees} deg")}));
        let r = run(&mut s, "joint.solve", json!({}));
        assert_eq!(r["errors"].as_array().unwrap().len(), 0, "{r}");
        let angle = (degrees + 17.0_f64).to_radians();
        let axis = place(&s, "B", Vec3::new(40.0, 11.0, 0.0)) - place(&s, "B", Vec3::new(40.0, 10.0, 0.0));
        assert!(near(axis, Vec3::new(angle.cos(), angle.sin(), 0.0)), "{degrees}: {axis:?}");
        assert!(near(place(&s, "B", Vec3::new(40.0, 10.0, 0.0)), Vec3::new(10.0, 10.0, 10.0)));
    }
}

#[test]
fn secondary_alignment_on_round_origins_keeps_the_named_end_as_geometry_changes() {
    let mut s = tray();
    let named = |component, point| {
        solidcraft_doc::joints::face_name_at(&s.doc, &s.model.state(), s.doc.find_component(component).unwrap(), point, false).unwrap()
    };
    let a = named("Tray", Vec3::new(300.0, 50.0, 0.5));
    let b = named("Bracket", Vec3::new(120.0, 210.0, 10.0));
    run(
        &mut s,
        "joint.create",
        json!({"type":"rigid","angle":0,"name":"RoundAligned",
        "a":{"occurrence":"Tray","circle":[283,50,0.8]},"b":{"occurrence":"Bracket","circle":[113,210,0.4]},
        "a_alignment":{"face":a},"b_alignment":{"face":b}}),
    );
    on_the_hole(&s, 300.0);
    let old = s.doc.assembly.joints[0].clone();
    let Snap::CircleCenter { face: Some(wall), .. } = &old.a.snap else { panic!("named round origin") };
    assert!(
        s.execute("joint.edit", &json!({"joint":"RoundAligned", "a_alignment":{"face":wall}})).is_err(),
        "a cylindrical alignment face must not become a captured direction"
    );
    width(&mut s, 330.0);
    on_the_hole(&s, 330.0);
    for (before, after) in [(&old.a, &s.doc.assembly.joints[0].a), (&old.b, &s.doc.assembly.joints[0].b)] {
        assert!(
            matches!((&before.snap,&after.snap),(Snap::CircleCenter{face:a,end:ae,..},Snap::CircleCenter{face:b,end:be,x:Some(XRef::Planar{..}),..}) if a==b && ae==be)
        );
    }
}

/// A tray: a 1 mm plate `W` wide (a parameter, 300 mm to start) and 100 deep, with a 6 mm hole
/// 20 mm in from its right edge (an expression); grounded. A bracket: a 20 x 20 x 2 foot with a
/// 6 mm hole in its middle and a 15 mm upright along its +X edge, so a turn shows.
fn tray() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "300 mm"}));
    run(&mut s, "component.create", json!({"name": "Tray"}));
    run(&mut s, "solid.box", json!({"length": "W", "width": 100, "height": 1, "corner": [0, 0, 0]}));
    run(&mut s, "solid.hole", json!({"position": ["W - 20 mm", 50, 1], "diameter": 6}));
    let tray = occ(&s, "Tray");
    run(&mut s, "occurrence.ground", json!({"occurrence": tray, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 2, "corner": [100, 200, 0]}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 20, "height": 15, "corner": [118, 200, 0], "operation": "join"}));
    run(&mut s, "solid.hole", json!({"position": [110, 210, 2], "diameter": 6}));
    s
}

fn width(s: &mut Session, w: f64) {
    run(s, "parameters.change", json!({"name": "W", "expression": format!("{w} mm")}));
    let r = run(s, "joint.solve", json!({}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "W = {w}: {r}");
}

/// The bracket sits on the tray's top, its hole on the tray's hole and its upright toward +X.
fn on_the_hole(s: &Session, w: f64) {
    let hole = place(s, "Bracket", Vec3::new(110.0, 210.0, 0.0));
    assert!(near(hole, Vec3::new(w - 20.0, 50.0, 1.0)), "W = {w}: the bracket's hole is at {hole:?}");
    let upright = place(s, "Bracket", Vec3::new(119.0, 210.0, 15.0));
    assert!(near(upright, Vec3::new(w - 11.0, 50.0, 16.0)), "W = {w}: the upright's top is at {upright:?}");
}

fn origin(s: &Session, joint: &str, a: bool) -> Snap {
    let j = s.doc.assembly.joints.iter().find(|j| j.name == joint).map(|j| if a { &j.a } else { &j.b });
    j.map(|o| o.snap.clone()).unwrap_or(Snap::Point { point: Vec3::ZERO, z: None, x: None })
}

#[test]
fn a_bracket_on_a_hole_in_thin_plate_stays_on_it_as_the_plate_widens() {
    let mut s = tray();
    // Picks inside the 1 mm wall of the tray's hole (nearer its top) and the 2 mm wall of the
    // bracket's (nearer its bottom); both x along +X.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Mount",
               "a": {"occurrence": "Tray", "circle": [283, 50, 0.8], "x": [1, 0, 0]},
               "b": {"occurrence": "Bracket", "circle": [113, 210, 0.4], "x": [1, 0, 0]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    on_the_hole(&s, 300.0);
    // Both origins are kept by their wall's name and their end's (the tray's top face, the
    // bracket's bottom one).
    for a in [true, false] {
        let snap = origin(&s, "Mount", a);
        assert!(matches!(&snap, Snap::CircleCenter { face: Some(_), end: Some(_), x: Some(XRef::Dir(_)), .. }), "{snap:?}");
    }
    for w in WIDTHS {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
    // Saved and opened again, it still follows.
    let dir = std::env::temp_dir().join(format!("solidcraft-joint-faces-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tray.json").to_string_lossy().to_string();
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    for w in [330.0, 510.0] {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_circle_given_by_its_centre_follows_its_hole() {
    let mut s = tray();
    // The centres of the tray hole's top rim and the bracket hole's bottom rim.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Mount",
               "a": {"occurrence": "Tray", "circle": [280, 50, 1], "x": [1, 0, 0]},
               "b": {"occurrence": "Bracket", "circle": [110, 210, 0], "x": [1, 0, 0]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    let snap = origin(&s, "Mount", true);
    assert!(matches!(&snap, Snap::CircleCenter { face: Some(_), end: Some(_), .. }), "a centre names its face and end: {snap:?}");
    on_the_hole(&s, 300.0);
    for w in WIDTHS {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
}

/// The name `model.faces` gives the face of `component`'s body holding `p` (component coordinates).
fn face_name(s: &mut Session, component: &str, p: Vec3) -> String {
    let st = s.model.state();
    let c = s.doc.find_component(component).unwrap_or(0);
    solidcraft_doc::joints::face_name_at(&s.doc, &st, c, p, false).unwrap_or_default()
}

#[test]
fn origins_take_faces_by_name_and_an_end_and_turn_by_name() {
    let mut s = tray();
    let wall = face_name(&mut s, "Tray", Vec3::new(283.0, 50.0, 0.5));
    let top = face_name(&mut s, "Tray", Vec3::new(150.0, 50.0, 1.0));
    let bottom = face_name(&mut s, "Tray", Vec3::new(150.0, 50.0, 0.0));
    let right = face_name(&mut s, "Tray", Vec3::new(300.0, 50.0, 0.5));
    let foot = face_name(&mut s, "Bracket", Vec3::new(113.0, 210.0, 1.0));
    let under = face_name(&mut s, "Bracket", Vec3::new(105.0, 205.0, 0.0));
    assert!(![&wall, &top, &bottom, &right, &foot, &under].iter().any(|n| n.is_empty()));
    // A round end by the wall's name and the face it opens onto; x along the tray's right end
    // face's normal (+X).
    run(&mut s, "joint.origin", json!({"name": "Hole", "circle": wall, "end": top, "x": right}));
    let r = run(&mut s, "joint.create", json!({"type": "rigid", "name": "Mount", "a": "Hole", "b": {"circle": foot, "end": under, "x": [1, 0, 0]}}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    on_the_hole(&s, 300.0);
    for w in [304.0, 400.0, 510.0] {
        width(&mut s, w);
        on_the_hole(&s, w);
    }
    // The other end: the bracket hangs under the plate (its foot's underside on the plate's).
    run(&mut s, "joint.origin", json!({"name": "Under", "circle": wall, "end": bottom, "x": right}));
    run(&mut s, "joint.delete", json!({"joint": "Mount"}));
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Hung", "a": "Under", "b": {"circle": foot, "end": under, "x": [1, 0, 0]}}));
    let hole = place(&s, "Bracket", Vec3::new(110.0, 210.0, 0.0));
    assert!(near(hole, Vec3::new(490.0, 50.0, 0.0)), "{hole:?}");
    assert!(place(&s, "Bracket", Vec3::new(119.0, 210.0, 15.0)).z < -10.0);
    // An end the hole doesn't have, a missing face and a bad x are refused.
    for bad in [
        json!({"circle": wall, "end": right}),
        json!({"circle": "F999:nothing"}),
        json!({"circle": wall, "x": "F999:nothing"}),
        json!({"circle": wall, "x": [0, 0, 0]}),
        json!({"face": [10, 10, 1], "end": top}),
    ] {
        assert!(s.execute("joint.origin", &bad).is_err(), "{bad}");
    }
}

#[test]
fn a_face_centre_by_name_follows_its_face() {
    let mut s = tray();
    let right = face_name(&mut s, "Tray", Vec3::new(300.0, 50.0, 0.5));
    run(&mut s, "joint.origin", json!({"name": "End", "face": right, "x": [0, 1, 0]}));
    let frame = |s: &Session| {
        let o = s.doc.assembly.origins.iter().find(|o| o.name == "End").map(|o| o.origin.clone());
        o.and_then(|o| solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &o).ok())
    };
    for w in [300.0, 330.0, 510.0] {
        width(&mut s, w);
        let m = frame(&s).unwrap_or(solidcraft_doc::IDENTITY);
        let (o, x, z) = (Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[0][0], m[0][1], m[0][2]), Vec3::new(m[2][0], m[2][1], m[2][2]));
        assert!(near(o, Vec3::new(w, 50.0, 0.5)) && near(z, Vec3::X) && near(x, Vec3::Y), "W = {w}: {o:?} {x:?} {z:?}");
    }
}

#[test]
fn an_origin_without_an_end_or_turn_opens_as_before() {
    // A design saved before origins named their end and turn: the snap reads without them.
    let snap: Snap = serde_json::from_value(json!({"type": "circle_center", "pick": [1, 2, 3], "face": "F1:side"})).unwrap_or(Snap::Point {
        point: Vec3::ZERO,
        z: None,
        x: None,
    });
    assert_eq!(snap, Snap::CircleCenter { pick: Vec3::new(1.0, 2.0, 3.0), face: Some("F1:side".into()), end: None, x: None });
    let back: Value = serde_json::to_value(&snap).unwrap_or_default();
    assert_eq!(back, json!({"type": "circle_center", "pick": [1.0, 2.0, 3.0], "face": "F1:side"}));
    let turned = Snap::FaceCenter { pick: Vec3::ZERO, face: None, x: Some(XRef::Face { face: "F2:+x".into(), pick: None }) };
    let v = serde_json::to_value(&turned).unwrap_or_default();
    assert_eq!(v["x"], json!({"face": "F2:+x"}));
    assert_eq!(serde_json::from_value::<Snap>(v).ok(), Some(turned));
}

/// The frame a named joint origin resolves to: its origin and z.
fn origin_frame(s: &Session, name: &str) -> Option<(Vec3, Vec3)> {
    let o = s.doc.assembly.origins.iter().find(|o| o.name == name)?;
    let m = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &o.origin).ok()?;
    Some((Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[2][0], m[2][1], m[2][2])))
}

#[test]
fn a_round_origin_saved_without_an_end_takes_the_end_it_took_before() {
    // A plate `T` thick (5 mm) with a 6 mm hole through it, and two round origins as builds
    // before `end` saved them (no `end`, no `x`): picks on the hole's wall 4 mm and 1 mm up.
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "5 mm"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": "T", "corner": [0, 0, 0]}));
    run(&mut s, "solid.hole", json!({"position": [20, 20, "T"], "diameter": 6}));
    let wall = solidcraft_doc::joints::face_name_at(&s.doc, &s.model.state(), 0, Vec3::new(23.0, 20.0, 2.5), true).unwrap_or_default();
    assert!(!wall.is_empty());
    for (name, z) in [("High", 4.0), ("Low", 1.0)] {
        let saved = json!({"name": name, "origin": {"occurrence": 0, "snap": {"face": wall, "pick": [23.0, 20.0, z], "type": "circle_center"}}});
        let o: solidcraft_doc::joints::NamedOrigin = serde_json::from_value(saved).unwrap_or_else(|e| panic!("{e}"));
        s.doc_mut().assembly.origins.push(o);
    }
    let dir = std::env::temp_dir().join(format!("solidcraft-joint-end-tie-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("plate.json").to_string_lossy().to_string();
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    let _ = std::fs::remove_dir_all(&dir);
    let (top, bottom) = (|t: f64| (Vec3::new(20.0, 20.0, t), Vec3::Z), (Vec3::new(20.0, 20.0, 0.0), -Vec3::Z));
    // Where the pick is nearer one end, that end; midway along the wall (the plate 8 mm thick
    // for the pick 4 mm up, 2 mm for the one 1 mm up), the bottom end, as those builds took.
    for (t, high, low) in [(5.0, Some(top(5.0)), bottom), (8.0, Some(bottom), bottom), (2.0, None, bottom), (5.0, Some(top(5.0)), bottom)] {
        run(&mut s, "parameters.change", json!({"name": "T", "expression": format!("{t} mm")}));
        for (name, want) in [("High", high), ("Low", Some(low))] {
            let Some((o, z)) = want else { continue };
            let got = origin_frame(&s, name);
            assert!(got.is_some_and(|(go, gz)| near(go, o) && near(gz, z)), "T = {t}, {name}: {got:?}, expected {o:?} {z:?}");
        }
    }
}

#[test]
fn origins_on_a_body_placed_by_an_offset_agree_named_or_not() {
    // A plate made at the origin, then placed by an offset (turned a quarter about x, moved):
    // a body moved in from another component keeps its geometry where it was made.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "corner": [0, 0, 0], "body_name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [20, 20, 5], "diameter": 6}));
    let offset = solidcraft_doc::rigid(Vec3::new(7.0, 0.0, 3.0), Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_2);
    s.doc_mut().body_offsets.insert("Plate".into(), offset);
    s.refresh();
    let at = |p: Vec3| solidcraft_doc::apply_point(&offset, p);
    let up = solidcraft_doc::apply_vector(&offset, Vec3::Z);
    let st = s.model.state();
    let (top_pick, wall_pick) = (at(Vec3::new(10.0, 10.0, 5.0)), at(Vec3::new(23.0, 20.0, 4.0)));
    let top = solidcraft_doc::joints::face_name_at(&s.doc, &st, 0, top_pick, false);
    let wall = solidcraft_doc::joints::face_name_at(&s.doc, &st, 0, wall_pick, true);
    assert!(top.is_some() && wall.is_some(), "{top:?} {wall:?}");
    let resolve = |snap: Snap| {
        let o = solidcraft_doc::joints::JointOrigin { occurrence: 0, snap };
        let m = solidcraft_doc::joints::resolve_origin(&s.doc, &st, &o).unwrap_or_else(|e| panic!("{e}"));
        (Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(m[2][0], m[2][1], m[2][2]))
    };
    // The top face's centre: by its name, and by the point alone.
    let named = resolve(Snap::FaceCenter { pick: top_pick, face: top, x: None });
    let unnamed = resolve(Snap::FaceCenter { pick: top_pick, face: None, x: None });
    assert!(near(named.0, unnamed.0) && near(named.1, unnamed.1), "{named:?} {unnamed:?}");
    assert!(near(named.1, up) && (named.0 - at(Vec3::new(0.0, 0.0, 5.0))).dot(up).abs() < 1e-6, "{named:?}");
    // The hole's top end (the pick nearer it): by the wall's name, and by the point alone.
    let named = resolve(Snap::CircleCenter { pick: wall_pick, face: wall, end: None, x: None });
    let unnamed = resolve(Snap::CircleCenter { pick: wall_pick, face: None, end: None, x: None });
    assert!(near(named.0, unnamed.0) && near(named.1, unnamed.1), "{named:?} {unnamed:?}");
    assert!(near(named.0, at(Vec3::new(20.0, 20.0, 5.0))) && near(named.1, up), "{named:?}");
}

#[test]
fn a_round_origin_keeps_its_end_when_the_face_it_opens_onto_gets_other_neighbours() {
    // A plate `T` thick with a groove across its top (so the top face is in pieces, each named
    // by the faces round it), a small hole and a 6 mm hole in the left piece. The origin is
    // picked on the 6 mm hole's wall near the top, and names the top piece as its end.
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "2 mm"}));
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 60, "width": 100, "height": "T", "corner": [0, 0, 0]}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 100, "height": 1, "corner": [28, 0, "T - 1 mm"], "operation": "cut"}));
    run(&mut s, "solid.hole", json!({"position": [10, 50, "T"], "diameter": 4}));
    let small = s.doc.features.last().map_or(0, |f| f.id);
    run(&mut s, "solid.hole", json!({"position": [20, 50, "T"], "diameter": 6}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "joint.origin", json!({"occurrence": "Plate", "circle": [23, 50, 1.6], "name": "O"}));
    let top = |s: &Session| {
        let o = s.doc.assembly.origins.iter().find(|o| o.name == "O").map(|o| o.origin.clone());
        o.and_then(|o| solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &o).ok()).map(|m| (m[3][2], m[2][2]))
    };
    assert_eq!(top(&s), Some((2.0, 1.0)));
    // Thicker, the pick is nearer the bottom; without the small hole the top piece is named by
    // fewer faces. The origin stays on the top.
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "4 mm"}));
    run(&mut s, "timeline.suppress", json!({"feature": small, "suppressed": true}));
    let (z, dz) = top(&s).unwrap_or_default();
    assert!((z - 4.0).abs() < 1e-9 && (dz - 1.0).abs() < 1e-9, "{z} {dz}");
}
