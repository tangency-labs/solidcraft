use serde_json::{Value, json};

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} should fail, gave {v}"),
        Err(e) => e.to_string(),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn row<'a>(list: &'a Value, name: &str) -> &'a Value {
    list["parameters"].as_array().and_then(|a| a.iter().find(|p| p["name"] == name)).unwrap_or(&Value::Null)
}

fn expr_of(s: &mut Session, name: &str) -> String {
    let l = run(s, "parameters.list", json!({}));
    row(&l, name)["expression"].as_str().unwrap_or_default().to_string()
}

/// A sketched block 40 x (len / 2) x 10 whose width is an expression of a user parameter.
fn block(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "len", "expression": "60 mm", "comment": "overall length", "favorite": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(s, "sketch.constraint.coincident", json!({"a": "p1", "b": "origin"}));
    run(s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(s, "sketch.dimension", json!({"entities": ["l2"], "value": "len/2"}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 10, "body_name": "Block"}));
}

#[test]
fn width_follows_an_expression() {
    let mut s = Session::default();
    block(&mut s);
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
    let r = run(&mut s, "parameters.change", json!({"name": "len", "expression": "100 mm"}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(rel(volume(&mut s), 40.0 * 50.0 * 10.0) < 1e-9);
    // Units convert: 4 in = 101.6 mm.
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "4 in"}));
    assert!(rel(volume(&mut s), 40.0 * 50.8 * 10.0) < 1e-9);
    // A parameter in inches reads bare numbers as inches.
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "5", "unit": "in"}));
    assert!(rel(volume(&mut s), 40.0 * 63.5 * 10.0) < 1e-9);
    let l = run(&mut s, "parameters.list", json!({}));
    assert_eq!(row(&l, "len")["value"], 5.0, "{l}");
    assert_eq!(row(&l, "len")["unit"], "in");
    // A failing parameter fails the features using it, with its error.
    let e = err(&mut s, "parameters.change", json!({"name": "len", "expression": "5 deg"}));
    assert!(e.contains("expected a length, got an angle"), "{e}");
}

/// Setting a value back reuses the earlier results and says so: the features it restored are
/// reported next to `recomputed`, so `recomputed: 0` is not mistaken for "nothing changed". A
/// sketch dimension set back solves the sketch again from its current state (value edits
/// continue from the last solved state); the features after it see the same geometry and are
/// restored.
#[test]
fn setting_a_parameter_back_reports_restored_features() {
    let mut s = Session::default();
    block(&mut s);
    let r = run(&mut s, "parameters.change", json!({"name": "len", "expression": "100 mm"}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(2), Some(0)), "{r}");
    let r = run(&mut s, "parameters.change", json!({"name": "len", "expression": "60 mm"}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(1), Some(1)), "{r}");
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
    let r = run(&mut s, "timeline.edit", json!({"feature": "Extrude1", "set": {"extent": {"distance": "20"}}}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(1), Some(0)), "{r}");
    let r = run(&mut s, "timeline.edit", json!({"feature": "Extrude1", "set": {"extent": {"distance": "10"}}}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(0), Some(1)), "{r}");
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
}

#[test]
fn feature_inputs_are_named_and_editable() {
    let mut s = Session::default();
    block(&mut s);
    let l = run(&mut s, "parameters.list", json!({"filter": "feature"}));
    let dist = l["parameters"].as_array().unwrap().iter().find(|r| r["input"] == "Distance").unwrap().clone();
    assert_eq!(dist["feature"], "Extrude1", "{l}");
    let d = dist["name"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": d, "expression": "len / 3"}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 20.0) < 1e-9);
    // Editing the feature keeps its parameter name.
    run(&mut s, "timeline.edit", json!({"feature": "Extrude1", "set": {"extent": {"distance": "12 mm"}}}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 12.0) < 1e-9);
    assert_eq!(expr_of(&mut s, &d), "12 mm");
    // Another feature can use it.
    run(&mut s, "solid.box", json!({"length": d, "width": 5, "height": 5, "corner": [100, 0, 0]}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 12.0 + 12.0 * 25.0) < 1e-9);
    let u = run(&mut s, "parameters.users", json!({"name": d}));
    assert_eq!(u["users"].as_array().map(Vec::len), Some(1), "{u}");
    assert_eq!(u["drives"].as_array().map(Vec::len), Some(2), "{u}");
    // Feature inputs can't be deleted on their own.
    assert!(err(&mut s, "parameters.delete", json!({"name": d})).contains("feature's input"));
}

#[test]
fn unit_errors_and_cycles_are_refused() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "a", "expression": "10 mm"}));
    let e = err(&mut s, "parameters.add", json!({"name": "bad", "expression": "a + 5 deg"}));
    assert!(e.contains("cannot add a length and an angle"), "{e}");
    let e = err(&mut s, "parameters.add", json!({"name": "ang", "expression": "a", "unit": "deg"}));
    assert!(e.contains("expected an angle, got a length"), "{e}");
    assert!(err(&mut s, "parameters.add", json!({"name": "x", "expression": "1", "unit": "furlong"})).contains("unknown unit"));
    run(&mut s, "parameters.add", json!({"name": "b", "expression": "a * 2"}));
    let e = err(&mut s, "parameters.change", json!({"name": "a", "expression": "b + 1"}));
    assert!(e.contains("circular reference: a → b → a"), "{e}");
    assert!(err(&mut s, "parameters.add", json!({"name": "selfref", "expression": "selfref"})).contains("circular"));
    assert!(err(&mut s, "parameters.add", json!({"name": "a", "expression": "1"})).contains("already"));
    assert!(err(&mut s, "parameters.add", json!({"name": "sin", "expression": "1"})).contains("function"));
    assert!(err(&mut s, "parameters.add", json!({"name": "2x", "expression": "1"})).contains("not a valid parameter name"));
    // Units are inferred for new parameters.
    run(&mut s, "parameters.add", json!({"name": "tilt", "expression": "30 deg"}));
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "b / a"}));
    assert_eq!(s.doc.param("tilt").map(|p| p.unit.as_str()), Some("deg"));
    assert_eq!(s.doc.param("n").map(|p| p.unit.as_str()), Some(""));
    let r = run(&mut s, "expr.evaluate", json!({"expression": "atan2(b; a) + tilt", "kind": "angle"}));
    assert!((r["display_value"].as_f64().unwrap_or(0.0) - (2.0f64.atan().to_degrees() + 30.0)).abs() < 1e-9, "{r}");
    let r = run(&mut s, "expr.evaluate", json!({"expression": "a + tilt"}));
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap_or("").contains("cannot add"), "{r}");
    let r = run(&mut s, "expr.evaluate", json!({"expression": "2", "unit": "in"}));
    assert!((r["value"].as_f64().unwrap_or(0.0) - 50.8).abs() < 1e-9, "{r}");
}

#[test]
fn rename_updates_every_reference() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "twice", "expression": "len * 2"}));
    let r = run(&mut s, "parameters.rename", json!({"name": "len", "new_name": "length"}));
    assert_eq!(r["references_updated"], 2, "{r}");
    assert_eq!(expr_of(&mut s, "twice"), "length * 2");
    let l = run(&mut s, "parameters.list", json!({"favorites": true}));
    assert_eq!(l["count"], 1, "the favourite follows the rename");
    assert_eq!(row(&l, "length")["comment"], "overall length");
    run(&mut s, "parameters.change", json!({"name": "length", "expression": "80"}));
    assert!(rel(volume(&mut s), 40.0 * 40.0 * 10.0) < 1e-9);
    // Model and feature parameters can be renamed too: dimensions and features follow.
    let dim = s.doc.features.first().and_then(|f| match &f.kind {
        solidcraft_doc::FeatureKind::Sketch { sketch, .. } => sketch.constraints.iter().filter_map(|c| c.param.clone()).next(),
        _ => None,
    });
    let dim = dim.unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": dim, "new_name": "base_w", "expression": "50"}));
    assert!(rel(volume(&mut s), 50.0 * 40.0 * 10.0) < 1e-9);
    let ext = s.doc.features.get(1).and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
    run(&mut s, "parameters.rename", json!({"name": ext, "new_name": "thick"}));
    run(&mut s, "parameters.change", json!({"name": "thick", "expression": "base_w / 10"}));
    assert!(rel(volume(&mut s), 50.0 * 40.0 * 5.0) < 1e-9);
    assert!(err(&mut s, "parameters.rename", json!({"name": "twice", "new_name": "length"})).contains("already"));
    assert!(err(&mut s, "parameters.rename", json!({"name": "nope", "new_name": "x"})).contains("unknown"));
    // Undo puts the old name back.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.features.get(1).is_some_and(|f| f.param_names.first() == Some(&ext)));
}

#[test]
fn deleting_a_used_parameter_lists_its_users() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "spare", "expression": "len + 1"}));
    let e = err(&mut s, "parameters.delete", json!({"name": "len"}));
    assert!(e.contains("parameter spare") && e.contains("parameter d"), "{e}");
    let u = run(&mut s, "parameters.users", json!({"name": "len"}));
    assert_eq!(u["users"].as_array().map(Vec::len), Some(2), "{u}");
    assert_eq!(u["drives"][0]["name"], "Base", "{u}");
    let dim = u["users"].as_array().unwrap().iter().find(|x| x["name"] != "spare").unwrap()["name"].as_str().unwrap().to_string();
    let e = err(&mut s, "parameters.change", json!({"delete": dim}));
    assert!(e.contains("Base dimension"), "{e}");
    run(&mut s, "parameters.delete", json!({"name": "spare"}));
    assert!(s.doc.param("spare").is_none());
    let g = run(&mut s, "parameters.graph", json!({}));
    assert!(g["edges"].as_array().unwrap().iter().any(|e| e[0] == dim.as_str() && e[1] == "len"), "{g}");
    assert!(g["edges"].as_array().unwrap().iter().any(|e| e[0] == "Feature:Base" && e[1] == dim.as_str()), "{g}");
}

/// #4420: sheet metal and plastic rules and the plane of a canvas hold expressions too. Renaming a
/// parameter they use renames it there, and they count as its users (so it can't be deleted).
#[test]
fn rules_and_canvases_follow_a_renamed_parameter() {
    let mut s = Session::default();
    for (name, expression) in [("gauge", "2 mm"), ("lift", "15 mm"), ("taper", "3 deg")] {
        run(&mut s, "parameters.add", json!({"name": name, "expression": expression}));
    }
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend([0, 0, 0, 4, 0, 0, 0, 2, 8, 6, 0, 0, 0, 0, 0, 0, 0]);
    run(&mut s, "canvas.insert", json!({"data": solidcraft_doc::canvas::base64_encode(&png), "plane": "XY", "offset": "lift"}));
    run(&mut s, "sheet.manage_rules", json!({"name": "Steel", "thickness": "gauge", "active": true}));
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 2, "body_name": "Tray"}));
    run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [30, 20, 2], "rule": "Steel"}));
    run(&mut s, "plastic.manage_rules", json!({"name": "Housing", "draft": "taper", "clearance": "gauge / 10"}));
    let users = |s: &mut Session, name: &str| -> Vec<String> {
        let u = run(s, "parameters.users", json!({"name": name}));
        u["users"].as_array().unwrap().iter().map(|x| x["type"].as_str().unwrap_or_default().to_string()).collect()
    };
    assert_eq!(users(&mut s, "gauge"), ["rule", "rule"], "the sheet metal thickness and the plastic clearance");
    assert_eq!(users(&mut s, "lift"), ["canvas"]);
    let e = err(&mut s, "parameters.delete", json!({"name": "taper"}));
    assert!(e.contains("plastic rule Housing (draft)"), "{e}");
    // The sheet metal features rebuild when a rule's parameter changes, so they are what it drives.
    let u = run(&mut s, "parameters.users", json!({"name": "gauge"}));
    assert_eq!(
        u["drives"].as_array().map(|d| d.iter().map(|x| x["name"].clone()).collect::<Vec<_>>()),
        Some(vec![json!("ConvertToSheetMetal1")]),
        "{u}"
    );
    let g = run(&mut s, "parameters.graph", json!({}));
    assert!(g["edges"].as_array().unwrap().iter().any(|e| e[0] == "Feature:ConvertToSheetMetal1" && e[1] == "gauge"), "{g}");
    assert!(!g["edges"].as_array().unwrap().iter().any(|e| e[0] == "Feature:Box1" && e[1] == "gauge"), "{g}");
    // Every field of the shared list follows: `gap` was set through `sheet.manage_rules` too.
    run(&mut s, "parameters.add", json!({"name": "seam", "expression": "0.5 mm"}));
    run(&mut s, "sheet.manage_rules", json!({"name": "Steel", "gap": "seam"}));
    run(&mut s, "parameters.rename", json!({"name": "seam", "new_name": "joint_gap"}));
    assert_eq!(s.doc.sheet.rules[0].gap, "joint_gap");
    for (old, new) in [("gauge", "sheet_t"), ("lift", "raise"), ("taper", "draft_angle")] {
        run(&mut s, "parameters.rename", json!({"name": old, "new_name": new}));
    }
    let failing: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(failing.is_empty(), "{failing:?}");
    assert_eq!(s.doc.sheet.rules[0].thickness, "sheet_t");
    let housing = s.doc.plastic.rules.iter().find(|r| r.name == "Housing").unwrap();
    assert_eq!((housing.draft.as_str(), housing.clearance.as_str()), ("draft_angle", "sheet_t / 10"));
    assert!(run(&mut s, "plastic.rules", json!({}))["rules"].as_array().unwrap().iter().any(|r| r["name"] == "Housing" && !r["values"].is_null()));
    match &s.doc.canvases[0].plane {
        solidcraft_doc::PlaneRef::Offset { distance, .. } => assert_eq!(distance.as_ref(), "raise"),
        other => panic!("{other:?}"),
    }
    assert_eq!(users(&mut s, "raise"), ["canvas"]);
    // Undo takes the old names back.
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(s.doc.sheet.rules[0].thickness, "gauge");
}

#[test]
fn favorites_comments_and_completion() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.favorite", json!({"name": "len", "favorite": false}));
    assert_eq!(run(&mut s, "parameters.list", json!({"favorites": true}))["count"], 0);
    let ext = s.doc.features.get(1).and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
    run(&mut s, "parameters.comment", json!({"name": ext, "comment": "plate thickness"}));
    let l = run(&mut s, "parameters.list", json!({}));
    assert_eq!(row(&l, &ext)["comment"], "plate thickness");
    let c = run(&mut s, "parameters.complete", json!({"prefix": "2 * le"}));
    assert_eq!(c["word"], "le");
    assert_eq!(c["suggestions"][0]["text"], "len", "{c}");
    let c = run(&mut s, "parameters.complete", json!({"prefix": "at"}));
    assert!(c["suggestions"].as_array().unwrap().iter().any(|x| x["text"] == "atan2("), "{c}");
}

/// #2757: a parameter is suggested for a word anywhere in its name, not only at its start;
/// names that start with the word come first. Functions still match from the start.
#[test]
fn completion_matches_parameters_anywhere_in_their_name() {
    let mut s = Session::default();
    for (name, e) in [("cab_side_thickness", "18 mm"), ("side", "40 mm"), ("door_width", "300 mm")] {
        run(&mut s, "parameters.add", json!({"name": name, "expression": e}));
    }
    let texts = |c: &Value| -> Vec<String> {
        c["suggestions"].as_array().unwrap().iter().filter(|x| x["kind"] == "parameter").map(|x| x["text"].as_str().unwrap().to_string()).collect()
    };
    let c = run(&mut s, "parameters.complete", json!({"prefix": "2 * side"}));
    assert_eq!(texts(&c), ["side", "cab_side_thickness"], "the name starting with it first: {c}");
    let c = run(&mut s, "parameters.complete", json!({"prefix": "THICK"}));
    assert_eq!(texts(&c), ["cab_side_thickness"], "any case: {c}");
    assert!(texts(&run(&mut s, "parameters.complete", json!({"prefix": "xyz"}))).is_empty());
    // Functions keep matching from the start: `in` is not `sin(`.
    let c = run(&mut s, "parameters.complete", json!({"prefix": "in"}));
    assert!(!c["suggestions"].as_array().unwrap().iter().any(|x| x["text"] == "sin("), "{c}");
}

#[test]
fn csv_and_json_round_trip() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "note", "expression": "len / 4", "comment": "has, a \"comma\""}));
    let csv = run(&mut s, "parameters.export", json!({"format": "csv"}))["text"].as_str().unwrap_or_default().to_string();
    assert!(csv.contains("note,mm,len / 4,15,\"has, a \"\"comma\"\"\",false"), "{csv}");
    let json_text = run(&mut s, "parameters.export", json!({"format": "json"}))["text"].as_str().unwrap_or_default().to_string();

    let mut t = Session::default();
    run(&mut t, "parameters.import", json!({"text": csv}));
    assert_eq!(t.doc.param("note").map(|p| p.comment.as_str()), Some("has, a \"comma\""));
    let mut u = Session::default();
    run(&mut u, "parameters.import", json!({"text": json_text}));
    assert_eq!(u.doc.param("note").map(|p| p.expr.as_str()), Some("len / 4"));

    // Importing into the design updates values and rebuilds; all or nothing.
    run(&mut s, "parameters.import", json!({"text": "name,expression\nlen,80 mm\nextra,3\n"}));
    assert!(rel(volume(&mut s), 40.0 * 40.0 * 10.0) < 1e-9);
    assert!(s.execute("parameters.import", &json!({"text": "name,expression\nlen,90 mm\nbroken,len + 1 deg\n"})).is_err());
    assert_eq!(s.doc.param("len").map(|p| p.expr.as_str()), Some("80 mm"));
    assert!(matches!(s.execute("parameters.import", &json!({})), Err(EngineError::BadParams { .. })));

    let dir = std::env::temp_dir().join(format!("solidcraft-params-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("p.json");
    let r = run(&mut s, "parameters.export", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["format"], "json");
    let mut v = Session::default();
    run(&mut v, "parameters.import", json!({"path": path.to_string_lossy()}));
    assert_eq!(v.doc.param("len").map(|p| p.expr.as_str()), Some("80 mm"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two triangles joined by three links (a b c, d e f; links a–d, b–e, c–f), a on the origin,
/// a–b horizontal, every distance a dimension: the construction leaves it to the iteration.
/// Returns the session, the sketch feature and the nine dimensions' parameters.
fn linkage_session() -> (Session, u64, Vec<String>) {
    let layout = [[0.0, 0.0], [40.0, 0.0], [18.0, 30.0], [8.0, 55.0], [52.0, 48.0], [30.0, 80.0]];
    let pairs = [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (0, 3), (1, 4), (2, 5)];
    let ids = ["a", "b", "c", "d", "e", "f"];
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Linkage"}));
    for (id, at) in ids.iter().zip(layout) {
        run(&mut s, "sketch.point", json!({"point": at, "id": id}));
    }
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["a", "b"], "mode": "horizontal"}));
    let params = pairs
        .iter()
        .map(|&(i, j)| run(&mut s, "sketch.dimension", json!({"entities": [ids[i], ids[j]]}))["param"].as_str().unwrap_or_default().to_string())
        .collect();
    let feature = s.doc.find_feature("Linkage").map(|f| f.id).unwrap_or_default();
    (s, feature, params)
}

/// Edited values the iteration cannot reach in one jump from the layout.
const REACHED_BY_CONTINUATION: [f64; 9] = [
    49.75798473660107,
    44.249179252138724,
    28.66332553713371,
    49.725624597008775,
    45.81086166277159,
    35.887660702567544,
    45.64700450204954,
    50.756441510188495,
    63.72063313963373,
];

/// Turn of both triangles in the evaluated sketch.
fn linkage_turns(s: &Session, feature: u64) -> [bool; 2] {
    let state = s.model.state();
    let sk = &state.sketch(feature).unwrap().sketch;
    let p = |id: &str| sk.point(sk.resolve_point(id).unwrap()).unwrap();
    [(p("b") - p("a")).cross(p("c") - p("a")) > 0.0, (p("e") - p("d")).cross(p("f") - p("d")) > 0.0]
}

/// Set the parameters in the document only (as an edit that does not store the sketch).
fn set_params(s: &mut Session, params: &[String], values: &[f64]) {
    let changes: Vec<(String, String)> = params.iter().zip(values).map(|(name, value)| (name.clone(), format!("{value} mm"))).collect();
    s.doc_mut().change_params_atomic(&changes).unwrap();
}

#[test]
fn dimension_edits_of_a_triconnected_linkage_continue_and_store_the_solved_sketch() {
    // The edit steps to the new values from the stored sketch, keeping both triangles' turn,
    // and stores what it reached: evaluating the document afresh needs no continuation.
    let (mut s, feature, params) = linkage_session();
    run(&mut s, "sketch.finish", json!({}));
    let before = linkage_turns(&s, feature);
    // All nine at once: one at a time, some intermediate values have no solution.
    let batch: Vec<_> = params.iter().zip(REACHED_BY_CONTINUATION).map(|(n, v)| json!({"name": n, "expression": format!("{v} mm")})).collect();
    run(&mut s, "parameters.import", json!({"text": serde_json::to_string(&batch).unwrap()}));
    let solved = s.model.state().sketch(feature).unwrap().report.clone();
    assert!(solved.ok(), "{solved:?}");
    assert_eq!(linkage_turns(&s, feature), before);
    let mut fresh = solidcraft_doc::Model::default();
    fresh.evaluate(&s.doc);
    let again = fresh.state().sketch(feature).unwrap().report.clone();
    assert!(again.ok() && again.continuations == 0, "{again:?}");
    // The kept results are what evaluating the stored document gives.
    let (kept, afresh) = (s.model.state(), fresh.state());
    let pairs = kept.sketch(feature).unwrap().sketch.points.iter().zip(&afresh.sketch(feature).unwrap().sketch.points);
    assert!(pairs.into_iter().all(|(a, b)| a.pos.dist(b.pos) < 1e-9));
    // In one go (one evaluation from the layout) it needs continuation.
    let (mut t, feature, params) = linkage_session();
    run(&mut t, "sketch.finish", json!({}));
    set_params(&mut t, &params, &REACHED_BY_CONTINUATION);
    t.refresh();
    let once = t.model.state().sketch(feature).unwrap().report.clone();
    assert!(once.ok() && once.continuations > 0, "{once:?}");
}

#[test]
fn extending_after_a_parameter_edit_continues_from_the_stored_sketch() {
    let (mut s, feature, params) = linkage_session();
    run(&mut s, "sketch.line", json!({"points": [[100, 0], [110, 0]], "ids": ["ext"]}));
    run(&mut s, "sketch.line", json!({"points": [[130, -20], [130, 20]], "ids": ["wall"]}));
    let before = linkage_turns(&s, feature);
    set_params(&mut s, &params, &REACHED_BY_CONTINUATION);
    run(&mut s, "sketch.extend", json!({"curve": "ext", "at": [110, 0]}));
    let solved = s.model.state().sketch(feature).unwrap().report.clone();
    assert!(solved.ok(), "{solved:?}");
    assert_eq!(linkage_turns(&s, feature), before);
}

#[test]
fn a_failed_edit_keeps_the_stored_sketch_solved_for_its_values() {
    // Parameters no geometry satisfies, then an edit that does not insist on a solution: the
    // stored dimensions keep the values its geometry is solved for.
    let (mut s, feature, params) = linkage_session();
    let mut values = REACHED_BY_CONTINUATION;
    values[7] = 500.0;
    set_params(&mut s, &params, &values);
    run(&mut s, "sketch.point", json!({"point": [200, 200]}));
    let stored = s.doc.sketch(feature).unwrap();
    for c in stored.constraints.iter().filter(|c| c.param.is_some()) {
        let measured = solidcraft_sketch::measure_dimension(stored, &c.kind).unwrap();
        assert!((measured - c.kind.value().unwrap()).abs() < 1e-6, "{}: {measured} vs {:?}", c.id, c.kind.value());
    }
}

/// Values the linkage reaches only with its top triangle mirrored.
const FLIPPING: [f64; 9] = [
    26.483003374917082,
    90.0253808040292,
    84.67215428223084,
    54.580784880976275,
    68.12871394439186,
    30.98508537669166,
    116.94895853350752,
    103.75535113133466,
    32.83103173640168,
];

/// All nine parameters set by one command.
fn import_values(params: &[String], values: &[f64]) -> Value {
    let batch: Vec<_> = params.iter().zip(values).map(|(n, v)| json!({"name": n, "expression": format!("{v} mm")})).collect();
    json!({"text": serde_json::to_string(&batch).unwrap()})
}

#[test]
fn an_edit_that_would_flip_the_sketch_is_refused_and_named() {
    let (mut s, feature, params) = linkage_session();
    run(&mut s, "sketch.finish", json!({}));
    let (points, before) = (stored_points(&s, feature), params.iter().map(|n| s.doc.param(n).map(|p| p.expr.clone())).collect::<Vec<_>>());
    let why = err(&mut s, "parameters.import", import_values(&params, &FLIPPING));
    assert!(why.contains("Linkage") && why.contains("flip the sketch over"), "{why}");
    assert_eq!(params.iter().map(|n| s.doc.param(n).map(|p| p.expr.clone())).collect::<Vec<_>>(), before);
    assert_eq!(stored_points(&s, feature), points);
    assert!(s.model.state().sketch(feature).unwrap().report.keeps_orientation());
}

#[test]
fn a_design_stored_with_other_values_still_rebuilds() {
    // A design whose stored sketch is not solved for its parameters (saved before value edits
    // stored their solve): evaluating it never refuses. The flipped solve is kept, with a
    // warning, the flipped constraints listed as conflicts, and the solid built from it.
    let (mut s, feature, params) = linkage_session();
    for line in [[[8.0, 55.0], [52.0, 48.0]], [[52.0, 48.0], [30.0, 80.0]], [[30.0, 80.0], [8.0, 55.0]]] {
        run(&mut s, "sketch.line", json!({"points": line}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Linkage", "distance": 5}));
    set_params(&mut s, &params, &FLIPPING);
    let s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let state = s.model.state();
    let solved = state.sketch(feature).unwrap();
    assert!(solved.report.ok() && !solved.report.folded.is_empty(), "{:?}", solved.report);
    let warning = s.model.result(feature).and_then(|r| r.warning.clone()).unwrap_or_default();
    assert!(warning.contains("flip the sketch over") && solved.report.folded.iter().all(|id| warning.contains(id.as_str())), "{warning}");
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    assert_eq!(state.bodies.len(), 1);
    // Listed as conflicts wherever conflicts are shown.
    let mut s = s;
    s.active_sketch = Some(feature);
    let si = run(&mut s, "sketch.inspect", json!({}));
    let glyphs = run(&mut s, "sketch.glyphs", json!({}));
    for id in &solved.report.folded {
        assert!(si["failing"].as_array().unwrap().contains(&json!(id)), "{si}");
        assert!(glyphs["conflicts"].as_array().unwrap().contains(&json!(id)), "{glyphs}");
    }
}

#[test]
fn an_edit_that_breaks_a_projecting_sketch_downstream_applies_with_a_warning() {
    // `w` places a guide line; a second sketch projects it and ends a 12 mm line on it. Moving
    // the guide out of reach is the guide sketch's edit, not a value edit of the projecting
    // sketch: it applies, and the projecting sketch carries a warning.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Guide"}));
    run(&mut s, "sketch.line", json!({"points": [[10, -30], [10, 30]], "ids": ["guide"], "connect": false}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "guide", "mode": "vertical"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "guide.start"], "type": "horizontal", "value": "w"}));
    run(&mut s, "sketch.finish", json!({}));
    let id = run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Reach"}))["sketch"].as_u64().unwrap();
    let projected = run(&mut s, "sketch.project", json!({"refs": [{"sketch": "Guide", "curve": "guide"}]}))["links"][0]["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    let line = run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, -(44.0_f64).sqrt()]]}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.dimension", json!({"entities": [line], "value": 12}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{line}.end"), "b": projected}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.model.state().sketch(id).unwrap().report.ok());
    let r = run(&mut s, "parameters.change", json!({"name": "w", "expression": "20 mm"}));
    assert_eq!(r["value"], json!(20.0), "the edit applies: {r}");
    assert!(!s.model.state().sketch(id).unwrap().report.ok());
    assert!(s.model.result(id).and_then(|r| r.warning.clone()).is_some_and(|w| w.contains("conflict")));
}

#[test]
fn a_failed_auto_constrain_keeps_the_stored_sketch_consistent() {
    // Auto-constrain works on the current values: reached by continuation, its result is
    // stored solved for them; values no geometry satisfies refuse it and change nothing.
    let consistent = |s: &Session, feature: u64| {
        let stored = s.doc.sketch(feature).unwrap();
        stored.constraints.iter().filter(|c| c.param.is_some()).all(|c| {
            let measured = solidcraft_sketch::measure_dimension(stored, &c.kind).unwrap();
            (measured - c.kind.value().unwrap()).abs() < 1e-6
        })
    };
    let (mut s, feature, params) = linkage_session();
    let mut values = REACHED_BY_CONTINUATION;
    values[7] = 500.0;
    set_params(&mut s, &params, &values);
    let before = s.doc.sketch(feature).unwrap().clone();
    let why = err(&mut s, "sketch.auto_constrain", json!({}));
    assert!(why.contains("conflict"), "{why}");
    assert_eq!(*s.doc.sketch(feature).unwrap(), before);
    assert!(consistent(&s, feature));
    set_params(&mut s, &params, &REACHED_BY_CONTINUATION);
    run(&mut s, "sketch.auto_constrain", json!({}));
    assert!(consistent(&s, feature));
    let stored = s.doc.sketch(feature).unwrap();
    for (c, v) in stored.constraints.iter().filter(|c| c.param.is_some()).zip(REACHED_BY_CONTINUATION) {
        assert!((c.kind.value().unwrap() - v).abs() < 1e-9, "{}", c.id);
    }
}

/// The stored sketch's points, by id.
fn stored_points(s: &Session, feature: u64) -> Vec<solidcraft_geom::Vec2> {
    s.doc.sketch(feature).unwrap().points.iter().map(|p| p.pos).collect()
}

#[test]
fn an_unsolvable_value_edit_is_refused_and_changes_nothing() {
    let (mut s, feature, params) = linkage_session();
    run(&mut s, "sketch.finish", json!({}));
    let (expr, points) = (s.doc.param(&params[7]).map(|p| p.expr.clone()), stored_points(&s, feature));
    let why = err(&mut s, "parameters.change", json!({"name": params[7], "expression": "500 mm"}));
    assert!(why.contains("Linkage") && why.contains("conflict"), "{why}");
    assert_eq!(s.doc.param(&params[7]).map(|p| p.expr.clone()), expr);
    assert_eq!(stored_points(&s, feature), points);
    assert!(s.model.state().sketch(feature).unwrap().report.ok());
    // A batch with one such value: none of it.
    let before: Vec<_> = params.iter().map(|n| s.doc.param(n).map(|p| p.expr.clone())).collect();
    let batch: Vec<_> = params
        .iter()
        .enumerate()
        .map(|(i, n)| json!({"name": n, "expression": if i == 7 { "500 mm".to_string() } else { format!("{} mm", REACHED_BY_CONTINUATION[i]) }}))
        .collect();
    let why = err(&mut s, "parameters.import", json!({"text": serde_json::to_string(&batch).unwrap()}));
    assert!(why.contains("Linkage"), "{why}");
    assert_eq!(params.iter().map(|n| s.doc.param(n).map(|p| p.expr.clone())).collect::<Vec<_>>(), before);
    assert_eq!(stored_points(&s, feature), points);
}

#[test]
fn value_edits_continue_from_the_last_solved_state() {
    // A → B → C, each a small edit solved in one jump; C is solved from B (the stored state),
    // and setting both values back reproduces A (the linkage is fully constrained).
    let close = |a: &[solidcraft_geom::Vec2], b: &[solidcraft_geom::Vec2], tol: f64| a.iter().zip(b).all(|(p, q)| p.dist(*q) < tol);
    let (mut s, feature, params) = linkage_session();
    run(&mut s, "sketch.finish", json!({}));
    let a = stored_points(&s, feature);
    let (p7, p8) = (s.doc.param(&params[7]).unwrap().expr.clone(), s.doc.param(&params[8]).unwrap().expr.clone());
    run(&mut s, "parameters.change", json!({"name": params[8], "expression": "53 mm"}));
    let b_sketch = s.doc.sketch(feature).unwrap().clone();
    let b = stored_points(&s, feature);
    assert!(!close(&a, &b, 1e-3) && s.model.state().sketch(feature).unwrap().report.continuations == 0);
    run(&mut s, "parameters.change", json!({"name": params[7], "expression": "50 mm"}));
    let c = stored_points(&s, feature);
    // The same edit solved from B directly.
    let mut from_b = b_sketch.clone();
    let stored = from_b.driving_values();
    let (vals, _) = s.doc.param_values();
    s.doc.apply_dimension_values(&vals, &mut from_b).unwrap();
    assert!(solidcraft_sketch::solve_after_value_edit(&mut from_b, &stored).ok());
    assert!(close(&c, &from_b.points.iter().map(|p| p.pos).collect::<Vec<_>>(), 1e-6) && !close(&c, &b, 1e-3));
    run(&mut s, "parameters.change", json!({"name": params[7], "expression": p7}));
    run(&mut s, "parameters.change", json!({"name": params[8], "expression": p8}));
    assert!(close(&stored_points(&s, feature), &a, 1e-6));
}

#[test]
fn switching_configurations_back_reuses_the_later_features() {
    // Rows A and B of `len`: back on A, the sketch solves again from B's state to A's
    // geometry, and the extrude after it is restored from the cache.
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "config.configure", json!({"params": ["len"]}));
    run(&mut s, "config.row", json!({"name": "A", "values": {"len": "60 mm"}}));
    run(&mut s, "config.row", json!({"name": "B", "values": {"len": "100 mm"}}));
    run(&mut s, "config.activate", json!({"row": "B"}));
    assert!(rel(volume(&mut s), 40.0 * 50.0 * 10.0) < 1e-9);
    let hits = |s: &Session| s.model.cache.lock().map(|c| c.hits).unwrap_or(0);
    let before = hits(&s);
    let r = run(&mut s, "config.activate", json!({"row": "A"}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
    assert!(hits(&s) > before, "{r}");
}

/// The user parameters' names in the order the Parameters dialog lists them.
fn user_order(s: &mut Session) -> Vec<String> {
    let v = run(s, "parameters.list", json!({"filter": "user"}));
    v["parameters"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn user_parameters_reorder_with_undo() {
    // #1187: move a user parameter before or after another; nothing else changes.
    let mut s = Session::default();
    for (n, e) in [("a", "1 mm"), ("b", "2 mm"), ("c", "a + b")] {
        run(&mut s, "parameters.add", json!({"name": n, "expression": e}));
    }
    assert_eq!(user_order(&mut s), ["a", "b", "c"]);
    let r = run(&mut s, "parameters.move", json!({"name": "c", "before": "a"}));
    assert_eq!(r["order"], json!(["c", "a", "b"]));
    assert_eq!(user_order(&mut s), ["c", "a", "b"]);
    run(&mut s, "parameters.move", json!({"name": "c", "after": "a"}));
    assert_eq!(user_order(&mut s), ["a", "c", "b"]);
    // Values and references are untouched; the order is kept in the design file.
    let list = run(&mut s, "parameters.list", json!({"filter": "user"}));
    let c = list["parameters"].as_array().unwrap().iter().find(|r| r["name"] == "c").unwrap().clone();
    assert_eq!(c["expression"], "a + b");
    assert!((c["value"].as_f64().unwrap() - 3.0).abs() < 1e-9, "{c}");
    let back = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let names: Vec<&str> = back.doc.params.iter().filter(|p| !p.model).map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["a", "c", "b"]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(user_order(&mut s), ["c", "a", "b"]);
}

#[test]
fn moving_a_parameter_refuses_bad_targets() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "a", "expression": "1 mm"}));
    run(&mut s, "parameters.add", json!({"name": "b", "expression": "2 mm"}));
    let before = s.doc.to_json();
    for p in [
        json!({"name": "a", "before": "a"}),
        json!({"name": "a", "before": "nope"}),
        json!({"name": "nope", "before": "a"}),
        json!({"name": "a"}),
        json!({"name": "a", "before": "b", "after": "b"}),
    ] {
        err(&mut s, "parameters.move", p);
    }
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn a_large_import_is_checked_once_and_keeps_each_rows_unit() {
    // 3000 parameters, each the one before plus a millimetre, then an angle, a unit-less
    // ratio and a row that names a parameter defined further down: all new, in one import.
    let mut csv = String::from("name,expression\n");
    csv.push_str("a,later * 2\np0,1 mm\n");
    for i in 1..3000 {
        csv.push_str(&format!("p{i},p{} + 1 mm\n", i - 1));
    }
    csv.push_str("turn,30 deg\nratio,p10 / p4\nlater,5\n");
    let mut s = Session::default();
    let t = std::time::Instant::now();
    run(&mut s, "parameters.import", json!({"text": csv}));
    println!("import of {} rows: {:.2} s", 3005, t.elapsed().as_secs_f64());
    let (vals, errors) = s.doc.param_values();
    assert!(errors.is_empty(), "{errors:?}");
    let unit = |n: &str| s.doc.param(n).map(|p| p.unit.clone()).unwrap_or_default();
    assert_eq!((unit("p2999"), unit("turn"), unit("ratio"), unit("later")), ("mm".into(), "deg".into(), String::new(), "mm".into()));
    assert!(
        vals.get("p2999").is_some_and(|v| (v.to_kind(solidcraft_doc::expr::Kind::Length).unwrap_or(f64::NAN) - 3000.0).abs() < 1e-9),
        "{:?}",
        vals.get("p2999")
    );
    assert!(
        vals.get("a").is_some_and(|v| (v.to_kind(solidcraft_doc::expr::Kind::Length).unwrap_or(f64::NAN) - 10.0).abs() < 1e-9),
        "{:?}",
        vals.get("a")
    );
    // Changing the first moves the whole chain; a row that breaks changes nothing.
    run(&mut s, "parameters.import", json!({"text": "name,expression\np0,101 mm\n"}));
    assert!(
        s.doc
            .param_values()
            .0
            .get("p2999")
            .is_some_and(|v| (v.to_kind(solidcraft_doc::expr::Kind::Length).unwrap_or(f64::NAN) - 3100.0).abs() < 1e-9)
    );
    assert!(s.execute("parameters.import", &json!({"text": "name,expression\np0,2 mm\np5,p0 + 1 deg\n"})).is_err());
    assert_eq!(s.doc.param("p0").map(|p| p.expr.as_str()), Some("101 mm"));
}

#[test]
fn several_parameters_change_in_one_rebuild_and_one_undo() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "10 mm"}));
    block(&mut s);
    let d = run(&mut s, "parameters.list", json!({"filter": "feature"}))["parameters"][0]["name"].as_str().unwrap_or_default().to_string();
    run(&mut s, "parameters.change", json!({"name": d, "expression": "h"}));
    let r = run(&mut s, "parameters.change", json!({"changes": {"len": "100 mm", "h": 20}}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    assert_eq!(r["changed"]["len"], 100.0, "{r}");
    assert!(rel(volume(&mut s), 40.0 * 50.0 * 20.0) < 1e-9);
    // All or nothing: an unknown name or a broken expression changes nothing.
    assert!(err(&mut s, "parameters.change", json!({"changes": {"len": "60 mm", "nope": "1 mm"}})).contains("nope"));
    assert!(s.execute("parameters.change", &json!({"changes": {"len": "60 mm", "h": "1 / 0"}})).is_err());
    assert!(s.execute("parameters.change", &json!({"changes": {}})).is_err());
    assert_eq!(expr_of(&mut s, "len"), "100 mm");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((expr_of(&mut s, "len"), expr_of(&mut s, "h")), ("60 mm".to_string(), "10 mm".to_string()));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
}

/// A sketch made with `flip` on an offset plane follows the offset's parameter (the parameter
/// walk stopped at the flipped plane, so a change rebuilt nothing).
#[test]
fn flipped_offset_sketch_plane_follows_its_parameter() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "d", "expression": "30 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Near"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "offset": "d", "flip": "x", "name": "Far"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.loft", json!({"sections": [{"sketch": "Near"}, {"sketch": "Far"}]}));
    assert!((volume(&mut s) - 400.0 * 30.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "d", "expression": "50 mm"}));
    assert!((volume(&mut s) - 400.0 * 50.0).abs() < 1e-6, "{}", volume(&mut s));
}
