use serde_json::json;

use super::*;
use crate::schema::tests::conforms;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(FROM_MODEL, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

/// `drawing.dimension.from_model` on `view`, with `more` params, its reply held to its schema.
fn brought(s: &mut Session, view: &Value, more: Value) -> Value {
    let mut p = json!({"view": view});
    if let (Some(p), Some(more)) = (p.as_object_mut(), more.as_object()) {
        p.extend(more.clone());
    }
    let reply = run(s, FROM_MODEL, p);
    let schema = output_schema_for::<FromModelResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    reply
}

/// What a model dimension is called in a reply: its constraint's id, or its parameter's label.
fn name(from: &Value) -> String {
    from["constraint"].as_str().or(from["param"].as_str()).unwrap().to_string()
}

/// What a reply added: each model dimension's name and what its new dimension reads now (model mm,
/// or degrees for an angle).
fn added(s: &mut Session, reply: &Value) -> Vec<(String, f64)> {
    let list = reply["added"].as_array().unwrap().clone();
    let read = |s: &mut Session, d: &Value| run(s, "drawing.dimension.inspect", json!({"dimension": d}))["value"].as_f64().unwrap();
    list.iter().map(|a| (name(&a["from"]), read(s, &a["dimension"]))).collect()
}

/// What a reply skipped, as (name, reason).
fn skipped(reply: &Value) -> Vec<(String, String)> {
    let one = |k: &Value| (name(&k["from"]), k["reason"].as_str().unwrap().to_string());
    reply["skipped"].as_array().unwrap().iter().map(one).collect()
}

/// What a reply skipped for `reason`, by name.
fn skipped_for(reply: &Value, reason: &str) -> Vec<String> {
    skipped(reply).into_iter().filter(|(_, r)| r == reason).map(|(n, _)| n).collect()
}

/// The ids of sketch `sketch`'s dimensions, in its own order.
fn dimensions_of(s: &Session, sketch: &str) -> Vec<String> {
    let f = s.doc.find_feature(sketch).unwrap();
    let FeatureKind::Sketch { sketch, .. } = &f.kind else { panic!("{} is not a sketch", f.name) };
    sketch.constraints.iter().filter(|c| c.kind.is_dimension()).map(|c| c.id.clone()).collect()
}

/// A new view of the open drawing's first sheet (a drawing is made first if none is open), at 1:1
/// and `at` on the sheet: its id.
fn view(s: &mut Session, direction: Value, at: [f64; 2]) -> Value {
    if s.open_drawing.is_none() {
        run(s, "drawing.create", json!({}));
    }
    run(s, "drawing.view.base", json!({"sheet": "Sheet 1", "view": direction, "scale": "1:1", "position_mm": at}))["view"].clone()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The issue's plate: a sketch `Plate` on XY, a 100 x 60 rectangle from the origin with its bottom
/// dimensioned 100 and its right side 60, extruded 10 into the body `Plate`: the session and the
/// extrude's id.
fn plate() -> (Session, u64) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    let sides = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}))["sides"].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [sides["bottom"]], "value": 100}));
    run(&mut s, "sketch.dimension", json!({"entities": [sides["right"]], "value": 60}));
    run(&mut s, "sketch.finish", json!({}));
    let extrude = run(&mut s, "solid.extrude", json!({"sketch": "Plate", "distance": 10, "body_name": "Plate"}))["feature"].as_u64().unwrap();
    (s, extrude)
}

/// The issue's case. In the top view the sketch's two dimensions come in, reading 100 and 60, and
/// the extrude's distance, between two faces the top view sees face on, can't be measured there. In
/// a front view only the distance comes in, reading 10: the sketch's two are already shown, each
/// by its dimension. Again in the top view, and in a second top view, nothing comes in.
#[test]
fn each_model_dimension_shows_in_one_view_only() {
    let (mut s, extrude) = plate();
    let [width, height]: [String; 2] = dimensions_of(&s, "Plate").try_into().unwrap();
    let top = view(&mut s, json!("top"), [120.0, 180.0]);
    let first = brought(&mut s, &top, json!({}));
    assert_eq!(added(&mut s, &first).len(), 2, "{first}");
    let read = added(&mut s, &first);
    assert!(read[0].0 == width && near(read[0].1, 100.0) && read[1].0 == height && near(read[1].1, 60.0), "{read:?}");
    assert_eq!(first["added"][0]["from"], json!({"kind": "sketch", "sketch": s.doc.find_feature("Plate").unwrap().id, "constraint": width}));
    assert_eq!(skipped_for(&first, "unmeasurable"), ["Distance"], "{first}");
    assert!(skipped(&first).iter().all(|(n, r)| r == "unmeasurable" || (r == "not_supported" && n != "Distance")), "{first}");

    let front = view(&mut s, json!("front"), [300.0, 180.0]);
    let second = brought(&mut s, &front, json!({}));
    let read = added(&mut s, &second);
    assert!(read.len() == 1 && read[0].0 == "Distance" && near(read[0].1, 10.0), "{second}");
    assert_eq!(second["added"][0]["from"], json!({"kind": "feature", "feature": extrude, "param": "Distance"}));
    assert_eq!(skipped_for(&second, "already_shown"), [width.clone(), height.clone()], "{second}");
    let shown_as: Vec<&Value> =
        second["skipped"].as_array().unwrap().iter().filter(|k| k["reason"] == "already_shown").map(|k| &k["shown_as"]).collect();
    assert_eq!(shown_as, [&first["added"][0]["dimension"], &first["added"][1]["dimension"]]);

    let again = view(&mut s, json!("top"), [120.0, 80.0]);
    for v in [&top, &again] {
        let reply = brought(&mut s, v, json!({}));
        assert_eq!(reply["added"], json!([]), "{reply}");
        assert_eq!(skipped_for(&reply, "already_shown"), [width.clone(), height.clone(), "Distance".into()], "{reply}");
    }
    assert_eq!(s.drawing("test").unwrap().dimensions.len(), 3);
}

/// Each kind brought in, worked out by hand on one sketch seen from the top: a quadrilateral from
/// (0, 0) through (100, 0) and (80, 40) to (0, 40), and a circle 20 across at (150, 20), extruded 10.
/// The lines are drawn with nothing inferred, so each dimension drives. The top's length 80; the
/// distances 80 across and 40 up from the origin corner, horizontal and vertical on paper; the
/// diagonal to (80, 40), sqrt(80² + 40²) = 89.4427191 aligned; the angle between the bottom and the
/// slanted side, as the sketch has it; the circle's diameter 20; and the radius 10 of a half circle
/// round (150, -40) from (160, -40) to (140, -40), closed by a line, picked at its sweep's middle,
/// (150, -30): its ends are corners the line shares.
#[test]
fn each_kind_of_sketch_dimension_comes_in_as_its_drawing_dimension() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Quad"}));
    let quad = run(
        &mut s,
        "sketch.line",
        json!({"points": [[0, 0], [100, 0], [80, 40], [0, 40]], "closed": true, "infer": false, "ids": ["bottom", "slant", "top", "left"]}),
    );
    let points = quad["created"]["points"].clone();
    let p = |i: usize| points[i].clone();
    run(&mut s, "sketch.dimension", json!({"entities": ["top"], "value": 80}));
    run(&mut s, "sketch.dimension", json!({"entities": [p(0), p(2)], "type": "horizontal", "value": 80}));
    run(&mut s, "sketch.dimension", json!({"entities": [p(0), p(3)], "type": "vertical", "value": 40}));
    run(&mut s, "sketch.dimension", json!({"entities": [p(0), p(2)], "type": "distance"}));
    // The obtuse one of the two angles between the lines, given as it is, so nothing moves.
    let angle =
        run(&mut s, "sketch.dimension", json!({"entities": ["bottom", "slant"], "value": "116.56505117707799 deg"}))["measured"].as_f64().unwrap();
    let circle = run(&mut s, "sketch.circle.center", json!({"center": [150, 20], "radius": 10}))["created"]["curves"][0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [circle], "type": "diameter", "value": 20}));
    let arc =
        run(&mut s, "sketch.arc.center_point", json!({"center": [150, -40], "start": [160, -40], "end": [140, -40]}))["created"]["curves"][0].clone();
    run(&mut s, "sketch.line", json!({"points": [[140, -40], [160, -40]]}));
    run(&mut s, "sketch.dimension", json!({"entities": [arc], "type": "radius", "value": 10}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Quad", "distance": 10}));
    assert!(near(angle, 116.56505117707799), "{angle}");
    let top = view(&mut s, json!("top"), [150.0, 150.0]);
    let reply = brought(&mut s, &top, json!({"sketch": "Quad"}));
    let read: Vec<f64> = added(&mut s, &reply).into_iter().map(|(_, v)| v).collect();
    assert_eq!((read.len(), reply["skipped"].clone()), (7, json!([])), "{reply}");
    let expected = [80.0, 80.0, 40.0, 89.44271909999159, angle, 20.0, 10.0];
    assert!(read.iter().zip(expected).all(|(r, e)| near(*r, e)), "{read:?} {reply}");
    let kind = |s: &mut Session, i: usize| {
        run(s, "drawing.dimension.inspect", json!({"dimension": reply["added"][i]["dimension"]}))["record"]["kind"].clone()
    };
    assert_eq!(kind(&mut s, 3)["linear"]["direction"], json!("aligned"));
    assert!(kind(&mut s, 4).get("angle").is_some());
    let record =
        |s: &mut Session, i: usize| run(s, "drawing.dimension.inspect", json!({"dimension": reply["added"][i]["dimension"]}))["record"].clone();
    assert_eq!(record(&mut s, 1)["kind"]["linear"]["direction"], json!("horizontal"));
    assert_eq!(record(&mut s, 2)["kind"]["linear"]["direction"], json!("vertical"));
    assert_eq!(record(&mut s, 5)["kind"]["radial"]["diameter"], json!(true));
    assert_eq!(record(&mut s, 6)["kind"]["radial"].get("diameter"), None, "a radius");
    assert_eq!(record(&mut s, 4)["kind"]["angle"]["other_side"], json!(true), "the obtuse side");

    // An acute angle reads on the usual side: a wedge from (200, 0) through (260, 0) to (200, 40),
    // its bottom and its long side atan(40 / 60) = 33.690068° apart. The model changes only with the
    // drawing closed.
    let open = json!({"drawing": ids::encode(s.open_drawing.unwrap())});
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Wedge"}));
    run(
        &mut s,
        "sketch.line",
        json!({"points": [[200, 0], [260, 0], [200, 40]], "closed": true, "infer": false, "ids": ["w_bottom", "w_long", "w_side"]}),
    );
    run(&mut s, "sketch.dimension", json!({"entities": ["w_bottom", "w_long"], "value": "33.690067525979785 deg"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Wedge", "distance": 10}));
    run(&mut s, "drawing.open", open);
    let wedge = brought(&mut s, &top, json!({"sketch": "Wedge"}));
    let read = added(&mut s, &wedge);
    assert!(read.len() == 1 && near(read[0].1, 33.690067525979785), "{wedge}");
    let acute = run(&mut s, "drawing.dimension.inspect", json!({"dimension": wedge["added"][0]["dimension"]}))["record"].clone();
    assert_eq!(acute["kind"]["angle"].get("other_side"), None, "the usual side: {acute}");
}

/// A view turned 45° about its direction still sees the sketch face on: a sketch's distance across
/// comes in along the sketch's x as the view shows it, not horizontal, and reads 80. The bottom's
/// length, which the rectangle and that distance already fix, is a driven dimension: it measures the
/// sketch and doesn't define the part, so it is skipped. A front view doesn't look along the
/// sketch's normal, so the distance is skipped there.
#[test]
fn a_turned_view_measures_along_the_sketchs_axis_and_a_side_view_skips_it() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Block"}));
    let made = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [80, 30]}));
    let corners = made["corners"].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [corners["bottom_left"], corners["top_right"]], "type": "horizontal", "value": 80}));
    let driven = run(&mut s, "sketch.dimension", json!({"entities": [made["sides"]["bottom"]], "if_redundant": "driven"}));
    assert_eq!(driven["driven"], json!(true), "{driven}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Block", "distance": 5}));
    let front = view(&mut s, json!("front"), [100.0, 200.0]);
    let side = brought(&mut s, &front, json!({"sketch": "Block"}));
    assert_eq!((side["added"].clone(), skipped_for(&side, "not_face_on").len(), skipped_for(&side, "driven").len()), (json!([]), 1, 1), "{side}");
    let turned = view(&mut s, json!({"direction": [0, 0, -1], "up": [1, 1, 0]}), [250.0, 150.0]);
    let reply = brought(&mut s, &turned, json!({"sketch": "Block"}));
    let read = added(&mut s, &reply);
    assert!(read.len() == 1 && near(read[0].1, 80.0) && skipped_for(&reply, "driven").len() == 1, "{reply}");
    let record = run(&mut s, "drawing.dimension.inspect", json!({"dimension": reply["added"][0]["dimension"]}))["record"].clone();
    assert!(record["kind"]["linear"]["direction"].get("along").is_some(), "{record}");
}

/// A sketch line on a longer edge of an older box: its middle finds the box's 100 mm edge, which
/// reads another value than the sketch's 40, so it is skipped with both values, never shown as 40.
#[test]
fn a_dimension_that_reads_another_value_is_skipped() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Box"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Short"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]], "ids": ["short"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["short"], "value": 40}));
    run(&mut s, "sketch.finish", json!({}));
    let top = view(&mut s, json!("top"), [150.0, 150.0]);
    let reply = brought(&mut s, &top, json!({"sketch": "Short"}));
    assert_eq!(reply["added"], json!([]), "{reply}");
    assert_eq!(skipped_for(&reply, "value_differs").len(), 1, "{reply}");
    let why = reply["skipped"][0]["why"].as_str().unwrap();
    assert!(why.contains("100.00 mm") && why.contains("40.00 mm"), "{why}");
    assert!(s.drawing("test").unwrap().dimensions.is_empty());
}

/// The filters narrow together: a sketch's own dimensions; a feature's own and its sketch's; a
/// body's, which leave out a sketch no feature of it uses; and a sketch with a body it doesn't
/// make, nothing. A sketch filter naming another feature, and names the design hasn't, are refused.
#[test]
fn the_filters_narrow_together() {
    let (mut s, extrude) = plate();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Other"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 100], [30, 100]], "ids": ["other"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["other"], "value": 30}));
    run(&mut s, "sketch.finish", json!({}));
    let front = view(&mut s, json!("front"), [150.0, 150.0]);
    let names = |s: &mut Session, more: Value| -> Vec<String> {
        let reply = brought(s, &front, more);
        let all = reply["added"].as_array().unwrap().iter().chain(reply["skipped"].as_array().unwrap()).map(|x| x["from"].clone());
        let mut names: Vec<String> = all.map(|f| format!("{}:{}", f["sketch"].as_u64().or(f["feature"].as_u64()).unwrap(), name(&f))).collect();
        names.sort();
        names.dedup();
        names
    };
    let plate_sketch = s.doc.find_feature("Plate").unwrap().id;
    let other = s.doc.find_feature("Other").unwrap().id;
    let of = |feature: u64, names: Vec<String>| names.into_iter().filter(|n| n.starts_with(&format!("{feature}:"))).count();
    // Seen from the front, nothing is added: the sketches are skipped, and the distance is added
    // only on the first call; so each call lists the same model dimensions.
    let only_plate = names(&mut s, json!({"sketch": "Plate"}));
    assert_eq!((only_plate.len(), of(plate_sketch, only_plate.clone())), (2, 2), "{only_plate:?}");
    let by_feature = names(&mut s, json!({"feature": extrude.to_string()}));
    assert_eq!((of(plate_sketch, by_feature.clone()), of(other, by_feature.clone())), (2, 0), "{by_feature:?}");
    assert!(of(extrude, by_feature.clone()) >= 1, "{by_feature:?}");
    let by_body = names(&mut s, json!({"body": "Plate"}));
    assert_eq!(by_body, by_feature, "the body's are the extrude's and its sketch's");
    assert_eq!(of(other, names(&mut s, json!({}))), 1, "with no filter the other sketch is there");
    assert_eq!(names(&mut s, json!({"sketch": "Other", "body": "Plate"})), Vec::<String>::new());
    assert_eq!(names(&mut s, json!({"feature": "Plate"})), only_plate, "a sketch as the feature is its own dimensions");
    assert!(refused(&mut s, json!({"view": front, "sketch": extrude.to_string()})).contains("is not a sketch"));
    assert!(refused(&mut s, json!({"view": front, "feature": "Nothing"})).contains("has no feature `Nothing`"));
    assert!(refused(&mut s, json!({"view": front, "body": "Nothing"})).contains("has no body `Nothing`"));
}

/// The record: a brought-in dimension saves which model dimension it shows, edit keeps it, add
/// never sets it, a copy of its view doesn't carry it (or it would show twice), and deleting the
/// dimension frees its model dimension to come in again. It follows the model: the sketch's 100
/// set to 120 reads 120, and still shows that model dimension.
#[test]
fn the_record_says_which_model_dimension_a_dimension_shows() {
    let (mut s, _) = plate();
    let width = dimensions_of(&s, "Plate")[0].clone();
    let top = view(&mut s, json!("top"), [120.0, 180.0]);
    let reply = brought(&mut s, &top, json!({"sketch": "Plate"}));
    let dimension = reply["added"][0]["dimension"].clone();
    let record = |s: &mut Session, d: &Value| run(s, "drawing.dimension.inspect", json!({"dimension": d}))["record"].clone();
    let sketch = s.doc.find_feature("Plate").unwrap().id;
    assert_eq!(record(&mut s, &dimension)["from_model"], json!({"sketch": {"sketch": sketch, "constraint": width}}));
    run(&mut s, "drawing.dimension.edit", json!({"dimension": dimension, "set": {"offset_mm": 15}}));
    assert_eq!(record(&mut s, &dimension)["from_model"], json!({"sketch": {"sketch": sketch, "constraint": width}}));
    let by_hand = run(&mut s, "drawing.dimension.add", json!({"view": top, "kind": {"length": {"edge": [50, 60, 10]}}}))["dimension"].clone();
    assert_eq!(record(&mut s, &by_hand).get("from_model"), None);

    let sheet = run(&mut s, "drawing.sheet.add", json!({"name": "Sheet 2"}))["sheet"].clone();
    let copy = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": top, "sheet": sheet, "position_mm": [150, 150]}))["view"].clone();
    let copied: Vec<Arc<Dimension>> = s.drawing("test").unwrap().dimensions.iter().filter(|d| json!(ids::encode(d.view)) == copy).cloned().collect();
    assert_eq!(copied.len(), 3, "the copy has the three dimensions");
    assert!(copied.iter().all(|d| d.from_model.is_none()), "{copied:?}");
    assert_eq!(skipped_for(&brought(&mut s, &copy, json!({"sketch": "Plate"})), "already_shown").len(), 2);

    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "selection.delete", json!({"items": [{"type": "drawing", "drawing": drawing, "object": dimension}]}));
    let again = brought(&mut s, &top, json!({"sketch": "Plate"}));
    assert_eq!(added(&mut s, &again).iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(), std::slice::from_ref(&width), "{again}");

    let back = again["added"][0]["dimension"].clone();
    let open = json!({"drawing": drawing});
    run(&mut s, "drawing.close", json!({}));
    let param = s.doc.find_feature("Plate").and_then(|f| match &f.kind {
        FeatureKind::Sketch { sketch, .. } => sketch.constraints.iter().find(|c| c.id == width).and_then(|c| c.param.clone()),
        _ => None,
    });
    run(&mut s, "parameters.change", json!({"name": param.unwrap(), "expression": "120 mm"}));
    run(&mut s, "drawing.open", open);
    let now = run(&mut s, "drawing.dimension.inspect", json!({"dimension": back}));
    assert!(near(now["value"].as_f64().unwrap(), 120.0), "{now}");
    assert_eq!(brought(&mut s, &top, json!({"sketch": "Plate"}))["added"], json!([]));
}

/// Everything one call brings in is one undo step, and a call that brings in nothing makes none: one
/// undo after both takes the drawing back to before the first.
#[test]
fn a_call_is_one_undo_step_or_none() {
    let (mut s, _) = plate();
    let top = view(&mut s, json!("top"), [120.0, 180.0]);
    let before = s.drawing("test").unwrap().to_json().unwrap();
    assert_eq!(brought(&mut s, &top, json!({}))["added"].as_array().map(Vec::len), Some(2));
    let after = s.drawing("test").unwrap().to_json().unwrap();
    assert_eq!(brought(&mut s, &top, json!({}))["added"], json!([]));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), after);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// Hostile input in every parameter is refused, never a panic, and changes nothing.
#[test]
fn hostile_input_is_refused() {
    let (mut s, _) = plate();
    let top = view(&mut s, json!("top"), [120.0, 180.0]);
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let long = "x".repeat(100_000);
    let cases = [
        json!({}),
        json!({"view": ""}),
        json!({"view": 1e308}),
        json!({"view": -1}),
        json!({"view": [top]}),
        json!({"view": "zzzzzz"}),
        json!({"view": top, "sketch": ""}),
        json!({"view": top, "sketch": long}),
        json!({"view": top, "sketch": 5}),
        json!({"view": top, "sketch": null}),
        json!({"view": top, "feature": {"id": 1}}),
        json!({"view": top, "feature": "\u{0}"}),
        json!({"view": top, "body": long}),
        json!({"view": top, "body": []}),
        json!({"view": top, "layout": true}),
    ];
    for p in cases {
        refused(&mut s, p);
    }
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

/// A sketch or feature of a placed component is skipped, not supported yet: the component may be
/// placed more than once, and its sketch's plane is in the component's frame. A box and a sketch
/// made in the component Bolt each list their dimensions as in a component, and nothing comes in.
#[test]
fn a_components_sketches_and_features_are_skipped() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Bolt"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 40, "body_name": "Shank"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Mark"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["mark"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["mark"], "value": 10}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let top = view(&mut s, json!("top"), [150.0, 150.0]);
    let reply = brought(&mut s, &top, json!({}));
    assert_eq!(reply["added"], json!([]), "{reply}");
    let kinds: Vec<&str> = reply["skipped"].as_array().unwrap().iter().map(|k| k["from"]["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"sketch") && kinds.contains(&"feature"), "{reply}");
    assert!(skipped(&reply).iter().all(|(_, r)| r == "in_component"), "{reply}");
}

/// A view that doesn't draw the body a model dimension measures skips it as not drawn, with why
/// `drawing.dimension.add` would give. The front view draws only the component Bolt, not the plate.
#[test]
fn a_body_the_view_leaves_out_is_skipped_as_not_drawn() {
    let (mut s, _) = plate();
    run(&mut s, "component.create", json!({"name": "Bolt"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 40, "body_name": "Shank"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let bolt = s.doc.occurrences.first().unwrap().id;
    let front = view(&mut s, json!("front"), [150.0, 150.0]);
    run(&mut s, "drawing.view.edit", json!({"view": front, "set": {"source": {"occurrences": [[bolt]]}}}));
    let reply = brought(&mut s, &front, json!({}));
    assert_eq!(reply["added"], json!([]), "{reply}");
    assert_eq!(skipped_for(&reply, "not_drawn"), ["Distance"], "{reply}");
    let why = reply["skipped"].as_array().unwrap().iter().find(|k| k["reason"] == "not_drawn").unwrap()["why"].as_str().unwrap().to_string();
    assert!(why.contains("doesn't draw that part: its source draws only the occurrences it chose"), "{why}");
}

/// Whether `reply` brought in the plate's sketch's two dimensions, `width` reading 100 and `height` 60,
/// as a top view of the plate reads them.
fn brought_the_plates_two(s: &mut Session, reply: &Value, [width, height]: &[String; 2]) -> bool {
    let read = added(s, reply);
    read.len() == 2 && read[0].0 == *width && near(read[0].1, 100.0) && read[1].0 == *height && near(read[1].1, 60.0)
}

/// A view that draws an exploded view takes its model dimensions, as `drawing.dimension.add` takes
/// dimensions there (#13048): read where the view draws its parts. Apart moves only the bolt, so the
/// plate's sketch's 100 and 60 come in reading as on any top view of the plate.
#[test]
fn an_exploded_view_takes_its_model_dimensions() {
    let (mut s, _) = plate();
    let two: [String; 2] = dimensions_of(&s, "Plate").try_into().unwrap();
    run(&mut s, "component.create", json!({"name": "Bolt"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 40, "corner": [0, 0, 20], "body_name": "Shank"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    // An exploded view pulls components apart, so the design needs two.
    run(&mut s, "component.create", json!({"name": "Nut"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 20], "body_name": "Nut"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let bolt = s.doc.occurrences.first().unwrap().id;
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": bolt, "translate": [0, 0, 50]}]}));
    let top = view(&mut s, json!("top"), [150.0, 150.0]);
    run(&mut s, "drawing.view.edit", json!({"view": top, "set": {"exploded": "Apart"}}));
    let reply = brought(&mut s, &top, json!({}));
    assert!(brought_the_plates_two(&mut s, &reply, &two), "{reply}");
}

/// A view in a position of its own takes its model dimensions too, by the same rule. The plate, with
/// its sketch's two dimensions to bring in, and a bolt on revolute joint Turn turned 30°, which leaves
/// the plate where it is: the 100 and 60 come in as on any top view of the plate.
#[test]
fn a_posed_view_takes_its_model_dimensions() {
    let (mut s, _) = plate();
    let two: [String; 2] = dimensions_of(&s, "Plate").try_into().unwrap();
    run(&mut s, "component.create", json!({"name": "Bolt", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 40, "corner": [0, 0, 20], "body_name": "Shank"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let at = json!({"occurrence": 0, "point": [0, 0, 20], "z": [0, 0, 1]});
    run(&mut s, "joint.as_built", json!({"type": "revolute", "name": "Turn", "a": 0, "b": "Bolt", "at": at}));
    let view = view(&mut s, json!("top"), [150.0, 150.0]);
    run(&mut s, "drawing.view.edit", json!({"view": view, "set": {"pose": [{"joint": "Turn", "values": [30]}]}}));
    let reply = brought(&mut s, &view, json!({}));
    assert!(brought_the_plates_two(&mut s, &reply, &two), "{reply}");
}
