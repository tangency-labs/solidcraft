use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::{ConstraintKind, CurveKind, LinkKind, Sketch};

use crate::{Sel, Session};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sketch(s: &Session, id: u64) -> &Sketch {
    s.doc.sketch(id).unwrap()
}

fn pos(sk: &Sketch, curve: &str) -> Vec<Vec2> {
    let c = &sk.curves[sk.curve_index(curve).unwrap()];
    c.kind.point_ids().iter().map(|i| sk.point(*i).unwrap()).collect()
}

/// A sketch on XY with a dimensioned, constrained rectangle tied to the origin, a circle with a
/// diameter, an arc, a spline and a point.
fn drawing() -> (Session, Vec<String>, String) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    let rect = strs(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}))["curves"]);
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", rect[0]), "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect[0]], "value": 20}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect[1]], "value": 10}));
    let circle = strs(&run(&mut s, "sketch.circle.center", json!({"center": [40, 5], "diameter": 6}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [circle], "type": "diameter", "value": 6}));
    (s, rect, circle)
}

#[test]
fn copy_paste_keeps_internal_constraints_and_dimensions_and_drops_outside_ones() {
    let (mut s, rect, circle) = drawing();
    let sid = s.active_sketch.unwrap();
    let mut entities = rect.clone();
    entities.push(circle.clone());
    let copied = run(&mut s, "sketch.copy", json!({"entities": entities}));
    assert_eq!(copied["curves"], 5);
    let before = sketch(&s, sid).clone();
    let params_before = s.doc.params.len();
    let undo = s.undo.len();
    let out = run(&mut s, "sketch.paste", json!({"at": [100, 50]}));
    assert_eq!(s.undo.len(), undo + 1, "one paste, one undo step");
    let pasted = strs(&out["curves"]);
    assert_eq!(pasted.len(), 5);
    let sk = sketch(&s, sid);
    // The originals are where they were.
    for c in entities.iter() {
        assert_eq!(pos(sk, c), pos(&before, c));
    }
    // Centred on `at`: the copy's bounding box (0..43 x 0..10) centre goes to (100, 50).
    let p0 = pos(sk, &pasted[0]);
    assert!(p0[0].dist(Vec2::new(100.0 - 21.5, 50.0 - 5.0)) < 1e-6, "{p0:?}");
    // The horizontal/vertical, coincident-corner and parallel constraints and the three dimensions
    // came along; the coincidence with the origin did not.
    let new_cons: Vec<_> = sk.constraints.iter().filter(|c| !before.constraints.iter().any(|b| b.id == c.id)).collect();
    let internal = before.constraints.iter().filter(|c| !c.kind.refers_to_point(0)).count();
    assert_eq!(new_cons.len(), internal, "{new_cons:#?}");
    assert!(!new_cons.iter().any(|c| c.kind.refers_to_point(0)));
    let dims: Vec<_> = new_cons.iter().filter(|c| c.kind.is_dimension()).collect();
    assert_eq!(dims.len(), 3);
    // Each pasted dimension has its own parameter.
    assert_eq!(s.doc.params.len(), params_before + 3);
    assert!(dims.iter().all(|d| d.param.is_some() && !before.constraints.iter().any(|b| b.param == d.param)));
    let length = dims
        .iter()
        .find(|d| matches!(d.kind, ConstraintKind::Length { .. } | ConstraintKind::Distance { .. }))
        .and_then(|d| d.param.clone())
        .unwrap();
    // The paste is selected.
    let sel: Vec<_> = s.selection.iter().filter_map(|x| if let Sel::SketchCurve { id, .. } = x { Some(id.clone()) } else { None }).collect();
    assert_eq!(sel, pasted);
    // The pasted rectangle is free to move (the original is held by the origin): dragging it moves it.
    run(&mut s, "sketch.drag", json!({"entity": pasted[0], "from": [p0[0].x, p0[0].y], "to": [p0[0].x + 5.0, p0[0].y + 5.0]}));
    let moved = pos(sketch(&s, sid), &pasted[0]);
    assert!(moved[0].dist(p0[0]) > 1.0, "{moved:?}");
    assert_eq!(pos(sketch(&s, sid), &rect[0]), pos(&before, &rect[0]));
    // Editing a pasted dimension changes the copy only.
    run(&mut s, "parameters.change", json!({"name": length, "expression": "30"}));
    let sk = sketch(&s, sid);
    let span = |c: &str| {
        let p = pos(sk, c);
        p[0].dist(p[1])
    };
    assert!((span(&rect[0]) - 20.0).abs() < 1e-6 && (span(&rect[1]) - 10.0).abs() < 1e-6);
    assert!(pasted[..4].iter().any(|c| (span(c) - 30.0).abs() < 1e-6), "{:?}", pasted.iter().map(|c| span(c)).collect::<Vec<_>>());
}

#[test]
fn paste_is_one_undo_step() {
    let (mut s, rect, circle) = drawing();
    let sid = s.active_sketch.unwrap();
    let before = s.doc.to_json();
    run(&mut s, "sketch.copy", json!({"entities": [rect[0], rect[1], circle]}));
    run(&mut s, "sketch.paste", json!({}));
    assert_ne!(s.doc.to_json(), before);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.active_sketch, Some(sid));
}

#[test]
fn paste_without_a_position_steps_away_from_the_original_each_time() {
    let (mut s, rect, _) = drawing();
    let sid = s.active_sketch.unwrap();
    run(&mut s, "sketch.copy", json!({"entities": [rect[0]]}));
    let a = strs(&run(&mut s, "sketch.paste", json!({}))["curves"]);
    let b = strs(&run(&mut s, "sketch.paste", json!({}))["curves"]);
    let sk = sketch(&s, sid);
    let (o, pa, pb) = (pos(sk, &rect[0])[0], pos(sk, &a[0])[0], pos(sk, &b[0])[0]);
    // A 20 mm line: steps of 2 mm.
    assert!(pa.dist(o + Vec2::new(2.0, -2.0)) < 1e-6, "{o:?} {pa:?}");
    assert!(pb.dist(o + Vec2::new(4.0, -4.0)) < 1e-6, "{o:?} {pb:?}");
}

#[test]
fn every_kind_of_sketch_geometry_pastes() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let line = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))["curves"]);
    let arc = strs(&run(&mut s, "sketch.arc.three_point", json!({"start": [20, 0], "end": [30, 0], "through": [25, 4]}))["curves"]);
    let circle = strs(&run(&mut s, "sketch.circle.center", json!({"center": [40, 0], "radius": 3}))["curves"]);
    let spline = strs(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 10], [5, 14], [10, 10], [15, 13]]}))["curves"]);
    let point = run(&mut s, "sketch.point", json!({"point": [50, 5]}));
    let point_id = strs(&point["points"]).first().cloned().unwrap_or_else(|| point["point"].as_str().unwrap().to_string());
    let text = run(&mut s, "sketch.text", json!({"text": "Hi", "at": [0, 20], "height": 5}));
    let text_curve = strs(&text["curves"])[0].clone();
    let sid = s.active_sketch.unwrap();
    let before = sketch(&s, sid).clone();
    let mut all: Vec<String> = [line, arc, circle, spline].concat();
    all.push(point_id.clone());
    all.push(text_curve);
    // Copy from the selection, as Ctrl+C does.
    s.selection = all
        .iter()
        .map(|id| if *id == point_id { Sel::SketchPoint { id: id.clone() } } else { Sel::SketchCurve { id: id.clone(), sketch: None } })
        .collect();
    run(&mut s, "sketch.copy", json!({}));
    let out = run(&mut s, "sketch.paste", json!({"translate": [0, 100]}));
    let sk = sketch(&s, sid);
    let new_curves: Vec<_> = sk.curves.iter().filter(|c| !before.curves.iter().any(|b| b.id == c.id)).collect();
    let kinds = |pred: fn(&CurveKind) -> bool| new_curves.iter().filter(|c| pred(&c.kind)).count();
    assert_eq!(kinds(|k| matches!(k, CurveKind::Arc { .. })), 1);
    assert_eq!(kinds(|k| matches!(k, CurveKind::Circle { .. })), 1);
    assert_eq!(
        kinds(|k| matches!(k, CurveKind::Spline { .. }))
            - before.curves.iter().filter(|c| c.link.is_some() && matches!(c.kind, CurveKind::Spline { .. })).count(),
        1
    );
    // The text came as a new, editable text 100 mm up.
    let texts: Vec<_> = sk.links.iter().filter(|l| l.kind == LinkKind::Text).collect();
    assert_eq!(texts.len(), 2);
    let Some(solidcraft_sketch::LinkSource::Text { at, text, .. }) =
        texts.iter().map(|l| &l.source).find(|src| !before.links.iter().any(|b| &b.source == *src))
    else {
        panic!()
    };
    assert_eq!(text, "Hi");
    assert!(at.dist(Vec2::new(0.0, 120.0)) < 1e-9);
    // The lone point.
    assert_eq!(strs(&out["points"]).len(), 1);
    let q = sk.resolve_point(&strs(&out["points"])[0]).and_then(|i| sk.point(i)).unwrap();
    assert!(q.dist(Vec2::new(50.0, 105.0)) < 1e-9);
    // Every pasted curve is selected.
    assert_eq!(s.selection.len(), strs(&out["curves"]).len() + 1);
}

#[test]
fn paste_into_another_sketch_lands_on_its_plane() {
    let (mut s, rect, circle) = drawing();
    run(&mut s, "sketch.copy", json!({"entities": [rect[0], rect[1], rect[2], rect[3], circle]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "B"}));
    let b = s.active_sketch.unwrap();
    let out = run(&mut s, "sketch.paste", json!({}));
    let sk = sketch(&s, b);
    let pasted = strs(&out["curves"]);
    assert_eq!(pasted.len(), 5);
    // Same sketch coordinates, on B's plane; constraints and dimensions came along.
    let p = pos(sk, &pasted[0]);
    assert!(p[0].dist(Vec2::new(0.0, 0.0)) < 1e-6 && p[1].dist(Vec2::new(20.0, 0.0)) < 1e-6, "{p:?}");
    assert_eq!(sk.constraints.iter().filter(|c| c.kind.is_dimension()).count(), 3);
    assert!(!sk.constraints.iter().any(|c| c.kind.refers_to_point(0)));
    let st = s.world_state().unwrap();
    let ss = st.sketch(b).unwrap();
    let w = ss.plane.to_world(Vec2::new(20.0, 10.0));
    assert!(w.y.abs() < 1e-9 && (w.x - 20.0).abs() < 1e-9, "{w:?}");
    // A profile closes on B.
    run(&mut s, "sketch.finish", json!({}));
    assert!(!s.model.state().sketches.iter().find(|x| x.feature == b).unwrap().profiles.is_empty());
}

#[test]
fn copy_and_paste_refuse_politely() {
    let mut s = Session::default();
    assert!(s.execute("sketch.copy", &json!({"entities": ["l1"]})).is_err());
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.execute("sketch.paste", &json!({})).is_err(), "nothing copied");
    assert!(s.execute("sketch.copy", &json!({})).is_err(), "nothing selected");
    assert!(s.execute("sketch.copy", &json!({"entities": ["nope"]})).is_err());
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    run(&mut s, "sketch.copy", json!({"entities": ["l1"]}));
    for bad in [json!({"at": [f64::MAX, 0]}), json!({"at": "x"}), json!({"translate": [1e308, 1e308]})] {
        let doc = s.doc.to_json();
        assert!(s.execute("sketch.paste", &bad).is_err(), "{bad}");
        assert_eq!(s.doc.to_json(), doc);
    }
}

#[test]
fn cut_copies_and_deletes_in_one_step_and_pastes_back_in_place() {
    let (mut s, rect, circle) = drawing();
    let sid = s.active_sketch.unwrap();
    let before = sketch(&s, sid).clone();
    let undo = s.undo.len();
    run(&mut s, "sketch.cut", json!({"entities": [circle]}));
    assert_eq!(s.undo.len(), undo + 1);
    assert!(sketch(&s, sid).curve_index(&circle).is_none());
    // Pasted back without a position: where it was, with its diameter.
    let pasted = strs(&run(&mut s, "sketch.paste", json!({}))["curves"]);
    let sk = sketch(&s, sid);
    assert_eq!(pos(sk, &pasted[0]), pos(&before, &circle));
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Diameter { .. })));
    assert_eq!(pos(sk, &rect[0]), pos(&before, &rect[0]));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sketch(&s, sid), &before);
}

/// Parameters that are not a JSON object (a number, text, a list) are an error, not a panic:
/// `sketch.cut` and `sketch.paste` add keys to a copy of the parameters, and serde_json panics
/// when a key is set on anything but an object (#1536, found by `hostile_params_never_panic`).
#[test]
fn clipboard_commands_refuse_parameters_that_are_not_an_object() {
    let (mut s, rect, _) = drawing();
    run(&mut s, "sketch.copy", json!({"entities": [rect[0].clone()]}));
    for id in ["sketch.cut", "sketch.copy", "sketch.paste"] {
        for hostile in [json!(true), json!(-1), json!(1e308), json!("nope"), json!([]), json!([0, 0, 0]), json!([{"x": 1}])] {
            let result = s.execute(id, &hostile);
            assert!(!matches!(result, Err(crate::EngineError::Internal(..))), "{id} {hostile} panicked inside the command: {result:?}");
            assert!(result.is_err(), "{id} {hostile} should be refused");
        }
    }
    // Nothing was cut or pasted by the refused calls.
    assert!(s.execute("sketch.paste", &json!({})).is_ok(), "the clipboard still holds the copy");
}
