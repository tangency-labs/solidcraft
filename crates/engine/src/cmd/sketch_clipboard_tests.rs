use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
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

/// Sketch geometry copied in one design pastes into a sketch of another (#9258): at the same
/// coordinates, with its constraints and dimensions. A dimension that named the first design's
/// parameter keeps its value as a number in its unit (the other design's `w` is another thing);
/// one typed as a number keeps it.
#[test]
fn sketch_geometry_pastes_into_another_design_with_values_not_foreign_parameters() {
    let mut a = Session::default();
    run(&mut a, "parameters.change", json!({"name": "w", "expression": "1 in"}));
    run(&mut a, "sketch.create", json!({"plane": "XY"}));
    let rect = strs(&run(&mut a, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [30, 15]}))["curves"]);
    run(&mut a, "sketch.dimension", json!({"entities": [rect[0]], "value": "w"}));
    run(&mut a, "sketch.dimension", json!({"entities": [rect[1]], "value": 10}));
    let sa = a.active_sketch.unwrap();
    let corner = pos(sketch(&a, sa), &rect[0]);
    run(&mut a, "sketch.copy", json!({"entities": rect}));
    let clip = a.sketch_clipboard.clone().unwrap();

    let mut b = Session::default();
    // Another design with its own `w`, and a sketch with the copied one's id.
    run(&mut b, "parameters.change", json!({"name": "w", "expression": "3 mm"}));
    run(&mut b, "sketch.create", json!({"plane": "XZ"}));
    let sb = b.active_sketch.unwrap();
    assert_eq!(sb, sa, "same id: the copy must still not be offset as if pasted back into its own sketch");
    b.sketch_clipboard = Some(clip.for_another_design());
    let pasted = strs(&run(&mut b, "sketch.paste", json!({}))["curves"]);
    assert_eq!(pasted.len(), 4);
    let sk = sketch(&b, sb);
    let at = pos(sk, &pasted[0]);
    assert!(at.iter().zip(&corner).all(|(p, q)| p.dist(*q) < 1e-6), "{at:?} vs {corner:?}");
    let exprs: Vec<String> = sk.constraints.iter().filter_map(|c| c.param.as_ref()).filter_map(|n| b.doc.param(n)).map(|p| p.expr.clone()).collect();
    assert_eq!(exprs.len(), 2, "{exprs:?}");
    assert!(exprs.iter().all(|e| solidcraft_doc::expr::references(e).is_empty()), "{exprs:?}");
    assert!(exprs.iter().any(|e| e == "10"), "{exprs:?}");
    let width = sk.constraints.iter().filter_map(|c| c.kind.value()).fold(0.0_f64, f64::max);
    assert!((width - 25.4).abs() < 1e-6, "the width stays 1 in, not the other design's w: {width}");
    assert!(b.doc.params.iter().filter(|p| p.name == "w").all(|p| p.expr == "3 mm"), "its own w is untouched");
    // Each paste without a position lands at the copied coordinates.
    let again = strs(&run(&mut b, "sketch.paste", json!({}))["curves"]);
    let at = pos(sketch(&b, sb), &again[0]);
    assert!(at.iter().zip(&corner).all(|(p, q)| p.dist(*q) < 1e-6), "{at:?}");
}

fn volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum()
}

/// A design with sketch `Small` (a 10 x 10 square from the origin) extruded 5 mm, then sketch
/// `Big` (a dimensioned 30 x 20 rectangle tied to the origin) on the same plane.
fn two_sketches() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Small"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Small", "distance": 5.0, "operation": "new_body"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Big"}));
    let rect = strs(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}))["curves"]);
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", rect[0]), "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect[0]], "value": 30}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect[1]], "value": 20}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

/// Replace Sketch (#11125): the sketch keeps its feature, and the extrude built on it now makes
/// the other sketch's shape (10 x 10 x 5 = 500 mm³ becomes 30 x 20 x 5 = 3000 mm³). The copy's
/// constraints come along, the one to the origin tied to this sketch's origin, its dimensions
/// with their own parameters; the source is untouched; one undo step puts it all back.
#[test]
fn replace_sketch_gives_a_sketch_another_sketchs_contents_and_keeps_its_features() {
    let mut s = two_sketches();
    assert!((volume(&s) - 500.0).abs() < 1e-6, "{}", volume(&s));
    let small = s.doc.find_feature("Small").unwrap().id;
    let big = s.doc.find_feature("Big").unwrap().id;
    let features = s.doc.features.len();
    let params = s.doc.params.len();
    let source = s.doc.sketch(big).unwrap().clone();
    let before = s.doc.to_json();

    let out = run(&mut s, "sketch.replace", json!({"sketch": "Small", "from": "Big"}));
    assert_eq!(out["sketch_id"], json!(small));
    assert_eq!(strs(&out["curves"]).len(), 4);
    assert_eq!(s.doc.features.len(), features, "no feature added or removed");
    assert_eq!(s.doc.find_feature("Small").map(|f| f.id), Some(small), "the sketch keeps its id and name");
    assert!((volume(&s) - 3000.0).abs() < 1e-6, "the extrude follows: {}", volume(&s));
    let sk = sketch(&s, small);
    assert_eq!(sk.constraints.len(), source.constraints.len(), "every constraint comes along");
    assert_eq!(sk.points.iter().filter(|q| q.id == "origin").count(), 1, "one origin");
    let origin = sk.points.iter().position(|q| q.id == "origin").unwrap();
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Coincident { p, q } if p == origin || q == origin)),
        "the corner stays tied to the origin: {:?}",
        sk.constraints
    );
    let names: Vec<String> = sk.constraints.iter().filter_map(|c| c.param.clone()).collect();
    let theirs: Vec<String> = source.constraints.iter().filter_map(|c| c.param.clone()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.iter().all(|n| !theirs.contains(n)), "its own parameters: {names:?} vs {theirs:?}");
    assert_eq!(s.doc.params.len(), params + 2);
    assert_eq!(serde_json::to_value(s.doc.sketch(big).unwrap()).unwrap(), serde_json::to_value(&source).unwrap(), "the source is untouched");
    // Its dimensions drive it: a new width changes this sketch only.
    run(&mut s, "parameters.change", json!({"name": names[0], "expression": "40"}));
    assert!((volume(&s) - 4000.0).abs() < 1e-6, "{}", volume(&s));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before, "one undo step");
}

/// The old contents' dimension parameters go with them, unless something else uses them; a
/// parameter the user made stays.
#[test]
fn replace_sketch_drops_the_old_dimension_parameters_nothing_else_uses() {
    let mut s = two_sketches();
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "12"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Third"}));
    let rect = strs(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [12, 8]}))["curves"]);
    run(&mut s, "sketch.dimension", json!({"entities": [rect[0]], "value": "w"}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect[1]], "value": 8}));
    run(&mut s, "sketch.finish", json!({}));
    let third = s.doc.find_feature("Third").unwrap().id;
    let params: Vec<String> = sketch(&s, third).constraints.iter().filter_map(|c| c.param.clone()).collect();
    assert_eq!(params.len(), 2, "{params:?}");
    run(&mut s, "sketch.replace", json!({"sketch": third, "from": "Small"}));
    for name in &params {
        let kept = s.doc.param(name);
        assert!(kept.is_none_or(|p| !p.model), "`{name}` went with the old dimension: {kept:?}");
    }
    assert!(s.doc.param("w").is_some(), "the user's parameter stays");
}

/// A feature that names the old contents' curves (here a profile picked by its curve ids) never
/// takes the new contents' curves that happen to get the same ids: the new ids carry on from the
/// old ones, so the reference finds nothing and the feature says so, instead of quietly building
/// from unrelated geometry (30 x 20 x 5 = 3000 mm³ here).
#[test]
fn replace_sketch_never_rebinds_a_reference_to_an_old_curve_onto_a_new_one() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Small"}));
    let square = strs(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}))["curves"]);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Small", "distance": 5.0, "operation": "new_body", "profiles": [{"curves": square}]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Big"}));
    let rect = strs(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}))["curves"]);
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(square, rect, "both sketches numbered their curves alike: {square:?} {rect:?}");
    assert!((volume(&s) - 500.0).abs() < 1e-6, "{}", volume(&s));
    let before = s.doc.to_json();
    match s.execute("sketch.replace", &json!({"sketch": "Small", "from": "Big"})) {
        Ok(out) => {
            let new = strs(&out["curves"]);
            assert!(new.iter().all(|id| !square.contains(id)), "new ids, not the old ones again: {new:?} vs {square:?}");
            assert!(s.model.results.iter().any(|r| r.error.is_some()), "the extrude says its curves are gone");
        }
        Err(_) => assert_eq!(s.doc.to_json(), before, "a refused replace changes nothing"),
    }
    assert!((volume(&s) - 3000.0).abs() > 1.0, "never built from the unrelated new curves: {}", volume(&s));
}

/// Sketch contents come from a sketch in another design file, at the same sketch coordinates; a
/// dimension that named that design's parameter keeps its value, without the parameter.
#[test]
fn replace_sketch_takes_a_sketch_from_another_design_file() {
    let dir = std::env::temp_dir().join(format!("solidcraft-replace-sketch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Other.sc");
    let mut other = Session::default();
    run(&mut other, "parameters.change", json!({"name": "w", "expression": "1 in"}));
    run(&mut other, "sketch.create", json!({"plane": "XZ", "name": "Plate"}));
    let rect = strs(&run(&mut other, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [25.4, 10]}))["curves"]);
    run(&mut other, "sketch.dimension", json!({"entities": [rect[0]], "value": "w"}));
    run(&mut other, "sketch.finish", json!({}));
    run(&mut other, "file.save_as", json!({"path": path.to_string_lossy()}));

    let mut s = two_sketches();
    let file = path.to_string_lossy().to_string();
    let out = run(&mut s, "sketch.replace", json!({"sketch": "Small", "from": "Plate", "file": file}));
    assert_eq!(out["file"], json!(file));
    assert!((volume(&s) - 25.4 * 10.0 * 5.0).abs() < 1e-6, "{}", volume(&s));
    assert!(s.doc.param("w").is_none(), "the other design's parameter is not brought in");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn replace_sketch_refuses_bad_requests_and_leaves_the_design_as_it_was() {
    let mut s = two_sketches();
    let before = s.doc.to_json();
    let extrude = s.doc.features.iter().find(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).unwrap().id;
    for bad in [
        json!({"sketch": "Small"}),
        json!({"sketch": "Small", "from": "Small"}),
        json!({"sketch": "Small", "from": "Nope"}),
        json!({"sketch": "Small", "from": 1.5}),
        json!({"sketch": "Small", "from": [1]}),
        json!({"sketch": "Small", "from": extrude}),
        json!({"sketch": "Nope", "from": "Big"}),
        json!({"from": "Big"}),
        json!({"sketch": "Small", "from": "Big", "file": 3}),
        json!({"sketch": "Small", "from": "Big", "file": "Z:/no/such/file.solidcraft"}),
        json!([1, 2]),
        json!("text"),
    ] {
        assert!(s.execute("sketch.replace", &bad).is_err(), "{bad}");
        assert_eq!(s.doc.to_json(), before, "{bad}");
    }
}

/// Copying a shape that uses `Equal` constraints to tie its edges to an earlier shape in the same
/// sketch keeps those `Equal` constraints on paste in that sketch, while positional outside
/// constraints (like coincidence to the origin) are still dropped so the copy moves freely (#15664).
#[test]
fn copy_paste_keeps_external_equal_constraints_within_same_sketch() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    let rect1 = strs(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}))["curves"]);
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", rect1[0]), "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": [rect1[0]], "value": 20}));
    let width_param = sketch(&s, s.active_sketch.unwrap()).constraints.iter().find(|c| c.kind.is_dimension()).and_then(|c| c.param.clone()).unwrap();
    run(&mut s, "sketch.dimension", json!({"entities": [rect1[1]], "value": 10}));

    // Copy rect1 -> paste rect2, delete rect2's dimensions, and tie rect2 to rect1 via Equal
    // (covering both `a` external and `b` external) plus a coincident anchor to the origin.
    run(&mut s, "sketch.copy", json!({"entities": rect1}));
    let paste2 = run(&mut s, "sketch.paste", json!({"at": [50, 0]}));
    let rect2 = strs(&paste2["curves"]);
    let sid = s.active_sketch.unwrap();
    let rect2_dims: Vec<String> = sketch(&s, sid)
        .constraints
        .iter()
        .filter(|c| strs(&paste2["constraints"]).contains(&c.id) && c.kind.is_dimension())
        .map(|c| c.id.clone())
        .collect();
    run(&mut s, "sketch.delete", json!({"entities": rect2_dims}));
    run(&mut s, "sketch.constraint.equal", json!({"a": rect1[0], "b": rect2[0]}));
    run(&mut s, "sketch.constraint.equal", json!({"a": rect2[1], "b": rect1[1]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", rect2[0]), "b": "origin"}));

    // Copy rect2 -> paste rect3: both external Equals come along, while the origin coincidence drops.
    let copied = run(&mut s, "sketch.copy", json!({"entities": rect2}));
    let before = sketch(&s, sid).clone();
    let paste3 = run(&mut s, "sketch.paste", json!({"at": [120, 40]}));
    let rect3 = strs(&paste3["curves"]);
    assert_eq!(rect3.len(), 4);
    assert_eq!(strs(&paste3["constraints"]).len(), copied["constraints"].as_u64().unwrap() as usize);

    let sk = sketch(&s, sid);
    let new_cons: Vec<_> = sk.constraints.iter().filter(|c| !before.constraints.iter().any(|b| b.id == c.id)).collect();
    assert!(!new_cons.iter().any(|c| c.kind.refers_to_point(0)), "origin anchor stays behind: {new_cons:#?}");
    let r1_0 = sk.curve_index(&rect1[0]).unwrap();
    let r1_1 = sk.curve_index(&rect1[1]).unwrap();
    let r3_0 = sk.curve_index(&rect3[0]).unwrap();
    let r3_1 = sk.curve_index(&rect3[1]).unwrap();
    assert!(new_cons.iter().any(|c| c.kind == ConstraintKind::Equal { a: r1_0, b: r3_0 }), "{new_cons:#?}");
    assert!(new_cons.iter().any(|c| c.kind == ConstraintKind::Equal { a: r3_1, b: r1_1 }), "{new_cons:#?}");

    // The pasted shape moves freely, and resizing rect1 drives both rect2 and rect3.
    let p0 = pos(sk, &rect3[0])[0];
    run(&mut s, "sketch.drag", json!({"entity": rect3[0], "from": [p0.x, p0.y], "to": [p0.x + 10.0, p0.y + 5.0]}));
    assert!(pos(sketch(&s, sid), &rect3[0])[0].dist(p0) > 1.0);
    run(&mut s, "parameters.change", json!({"name": width_param, "expression": "35"}));
    let span = |c: &str| {
        let p = pos(sketch(&s, sid), c);
        p[0].dist(p[1])
    };
    assert!((span(&rect1[0]) - 35.0).abs() < 1e-6);
    assert!((span(&rect2[0]) - 35.0).abs() < 1e-6);
    assert!((span(&rect3[0]) - 35.0).abs() < 1e-6);
}

/// `sketch.cut` + `sketch.paste` in the same sketch keeps external `Equal` constraints when the
/// referenced curve still exists, and drops them cleanly when that curve was deleted.
#[test]
fn cut_paste_keeps_external_equal_while_referenced_curve_exists() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let l1 = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let l2 = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 10], [20, 10]]}))["curves"])[0].clone();
    let l3 = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [15, 20]]}))["curves"])[0].clone();
    let l4 = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 30], [15, 30]]}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [l1], "value": 20}));
    let sid = s.active_sketch.unwrap();
    let param = sketch(&s, sid).constraints.iter().find(|c| c.kind.is_dimension()).and_then(|c| c.param.clone()).unwrap();
    run(&mut s, "sketch.constraint.equal", json!({"a": l1, "b": l2}));
    run(&mut s, "sketch.constraint.equal", json!({"a": l3, "b": l4}));

    run(&mut s, "sketch.cut", json!({"entities": [l2]}));
    let pasted = strs(&run(&mut s, "sketch.paste", json!({}))["curves"])[0].clone();
    let sk = sketch(&s, sid);
    let i1 = sk.curve_index(&l1).unwrap();
    let ip = sk.curve_index(&pasted).unwrap();
    assert!(sk.constraints.iter().any(|c| c.kind == ConstraintKind::Equal { a: i1, b: ip }));
    run(&mut s, "parameters.change", json!({"name": param, "expression": "28"}));
    let p = pos(sketch(&s, sid), &pasted);
    assert!((p[0].dist(p[1]) - 28.0).abs() < 1e-6);

    // If the referenced curve is deleted before paste, the dangling Equal is dropped without error.
    run(&mut s, "sketch.copy", json!({"entities": [l4]}));
    run(&mut s, "sketch.delete", json!({"entities": [l3]}));
    let out = run(&mut s, "sketch.paste", json!({"translate": [0, 10]}));
    assert_eq!(strs(&out["curves"]).len(), 1);
    assert!(strs(&out["constraints"]).is_empty());
}

/// External `Equal` constraints are never carried into another sketch or another design, even when
/// the target sketch has a curve with the same id.
#[test]
fn external_equal_is_dropped_when_pasting_to_another_sketch_or_design() {
    let mut a = Session::default();
    run(&mut a, "sketch.create", json!({"plane": "XY", "name": "A"}));
    let l1 = strs(&run(&mut a, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let l2 = strs(&run(&mut a, "sketch.line", json!({"points": [[0, 10], [20, 10]]}))["curves"])[0].clone();
    run(&mut a, "sketch.constraint.equal", json!({"a": l1, "b": l2}));
    run(&mut a, "sketch.copy", json!({"entities": [l2]}));
    let clip = a.sketch_clipboard.clone().unwrap();

    // Pasting into a second sketch in the same design drops the external Equal.
    run(&mut a, "sketch.finish", json!({}));
    run(&mut a, "sketch.create", json!({"plane": "XZ", "name": "B"}));
    let sb = a.active_sketch.unwrap();
    run(&mut a, "sketch.line", json!({"points": [[0, 0], [50, 0]]}));
    let out_b = run(&mut a, "sketch.paste", json!({}));
    assert_eq!(strs(&out_b["curves"]).len(), 1);
    assert!(strs(&out_b["constraints"]).is_empty());
    assert!(!sketch(&a, sb).constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Equal { .. })));

    // Pasting into another design (even with matching sketch and curve ids) drops the external Equal.
    let mut b = Session::default();
    run(&mut b, "sketch.create", json!({"plane": "XY", "name": "A"}));
    let sb2 = b.active_sketch.unwrap();
    assert_eq!(sb2, 1);
    let other_l1 = strs(&run(&mut b, "sketch.line", json!({"points": [[0, 0], [50, 0]]}))["curves"])[0].clone();
    assert_eq!(other_l1, l1);
    b.sketch_clipboard = Some(clip.for_another_design());
    let out_other = run(&mut b, "sketch.paste", json!({}));
    assert_eq!(strs(&out_other["curves"]).len(), 1);
    assert!(strs(&out_other["constraints"]).is_empty());
    assert!(!sketch(&b, sb2).constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Equal { .. })));
}
