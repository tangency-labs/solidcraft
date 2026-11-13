use super::*;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_doc::drawing::view::{View, ViewKind, ViewPlace};
use solidcraft_doc::drawing_view::{ViewDirection, ViewOverrides};
use solidcraft_geom::{drawing_view::NamedView, prelude::*};

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn fixture() -> (Session, u64, usize) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    run(
        &mut s,
        "solid.cylinder",
        json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"], "name": "Hole"}),
    );
    let state = s.model.state();
    let mesh = state.body("Box").unwrap().mesh();
    let centre = Vec3::new(40.0, 0.0, 15.0);
    let edge = mesh.edges.iter().position(|line| line.iter().all(|p| (p.dist(centre) - 6.0).abs() < 1e-6)).unwrap();
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut ids::IdSource::seeded(9419)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    d.add_views([Arc::new(View {
        id,
        sheet,
        kind: ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) },
        place: ViewPlace::At([200.0, 150.0]),
        settings: ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() },
        reference_shown_percent: 0.0,
    })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, id, edge)
}

#[test]
fn add_inspect_edit_remove_and_undo() {
    let (mut s, view, edge) = fixture();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let result = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"body": "Box", "index": edge}, "overshoot_mm": 2.0}));
    assert_eq!(result["status"], "ok");
    assert!((result["radius_mm"].as_f64().unwrap() - 6.0).abs() < 1e-9);
    let mark = result["mark"].clone();
    assert_eq!(run(&mut s, INSPECT, json!({"mark": mark}))["record"]["overshoot_mm"], 2.0);
    let unchanged = s.drawing("test").unwrap().to_json().unwrap();
    assert!(s.execute(EDIT, &json!({"mark": mark, "overshoot_mm": -1.0})).is_err());
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), unchanged);
    assert_eq!(run(&mut s, EDIT, json!({"mark": mark, "overshoot_mm": 3.0}))["changed"], json!(["overshoot_mm"]));
    run(&mut s, REMOVE, json!({"mark": mark}));
    assert!(s.execute(INSPECT, &json!({"mark": mark})).is_err());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, INSPECT, json!({"mark": mark}))["record"]["overshoot_mm"], 3.0);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

#[test]
fn selection_delete_always_reports_both_centre_annotation_lists() {
    let (mut s, view, edge) = fixture();
    let mark = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"body": "Box", "index": edge}, "overshoot_mm": 2.0}))["mark"].clone();
    let line =
        run(&mut s, "drawing.center_line.add", json!({"view": ids::encode(view), "a": [0, 0, 0], "b": [80, 0, 0], "overshoot_mm": 2.0}))["line"]
            .clone();
    let note = run(&mut s, "drawing.note.add", json!({"anchor": {"view": ids::encode(view)}, "at": [0, -20], "content": "note"}))["note"].clone();
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let selected = |object: Value| json!({"type": "drawing", "drawing": drawing, "object": object});
    let deleted = run(&mut s, "selection.delete", json!({"items": [selected(note)]}));
    assert_eq!(deleted["centre_marks"], json!([]));
    assert_eq!(deleted["centre_lines"], json!([]));
    let deleted = run(&mut s, "selection.delete", json!({"items": [selected(json!(ids::encode(view)))]}));
    assert_eq!(deleted["centre_marks"], json!([mark]));
    assert_eq!(deleted["centre_lines"], json!([line]));
    let schema = crate::schema::output_schema_for::<crate::schema::delete::SelectionDeleteResult>();
    assert!(crate::schema::tests::conforms(schema.as_value(), schema.as_value(), &deleted), "{deleted}");
}

#[test]
fn mark_follows_size_and_reports_a_lost_edge_until_undo() {
    let (mut s, view, edge) = fixture();
    let result = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"body": "Box", "index": edge}, "overshoot_mm": 2.0}));
    let mark = result["mark"].clone();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let list = run(&mut s, "parameters.list", json!({"filter": "feature"}));
    let size = list["parameters"].as_array().unwrap().iter().find(|p| p["value"].as_f64() == Some(6.0)).unwrap()["name"].clone();
    run(&mut s, "parameters.change", json!({"name": size, "expression": "8 mm"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let changed = run(&mut s, INSPECT, json!({"mark": mark}));
    assert!((changed["radius_mm"].as_f64().unwrap() - 8.0).abs() < 1e-9);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "timeline.delete", json!({"feature": "Hole"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let lost = run(&mut s, INSPECT, json!({"mark": mark}));
    assert_eq!(lost["status"], "lost");
    assert!(lost.get("centre").is_none());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert_eq!(run(&mut s, INSPECT, json!({"mark": mark}))["status"], "ok");
}

#[test]
fn centre_mark_follows_a_rim_split_into_pieces_on_the_same_circle() {
    let (mut s, view, edge) = fixture();
    let pick = json!({"body": "Box", "index": edge});
    let mark = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": pick, "overshoot_mm": 2.0}))["mark"].clone();
    let dimension =
        run(&mut s, "drawing.dimension.add", json!({"view": ids::encode(view), "kind": {"radial": {"edge": pick, "diameter": true}}}))["dimension"]
            .clone();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    for (x, name) in [(33.5, "Left notch"), (45.5, "Right notch")] {
        run(
            &mut s,
            "solid.box",
            json!({"length": 1, "width": 2, "height": 2, "corner": [x, -1, 14], "operation": "cut", "targets": ["Box"], "name": name}),
        );
    }
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let measured = run(&mut s, "drawing.dimension.inspect", json!({"dimension": dimension}));
    let resolved = run(&mut s, INSPECT, json!({"mark": mark}));
    assert_eq!(measured["status"], "ok", "{measured}");
    assert_eq!(resolved["status"], "ok", "{resolved}");
    assert!((resolved["radius_mm"].as_f64().unwrap() - 6.0).abs() < 1e-6);
}

#[test]
fn refuses_straight_edge_and_invalid_overshoot_without_mutation() {
    let (mut s, view, edge) = fixture();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    assert!(s.execute(ADD, &json!({"view": ids::encode(view), "edge": {"body": "Box", "index": edge}, "overshoot_mm": -1.0})).is_err());
    assert!(s.execute(ADD, &json!({"view": ids::encode(view), "edge": [40, 0, 0], "overshoot_mm": 2.0})).is_err());
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

#[test]
fn copying_a_view_copies_its_mark_and_deleting_the_sheet_removes_it() {
    let (mut s, view, edge) = fixture();
    let original = run(&mut s, ADD, json!({"view": ids::encode(view), "edge": {"body": "Box", "index": edge}, "overshoot_mm": 2.0}));
    let original_id = original["mark"].as_str().unwrap().to_owned();
    let note =
        run(&mut s, "drawing.note.add", json!({"anchor": {"view": ids::encode(view)}, "at": [0, -20], "content": "Keep this note"}))["note"].clone();
    let sheet = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].clone();
    let copy = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": ids::encode(view), "sheet": sheet}));
    let copy_view = copy["view"].as_str().unwrap().to_owned();
    let copied = run(&mut s, "drawing.view.inspect", json!({"view": copy_view}));
    let copy_id = copied["centre_marks"][0].as_str().unwrap().to_owned();
    let copied_view = ids::decode(&copy_view).unwrap();
    let copy_note = s.drawing("test").unwrap().notes.iter().find(|n| n.anchor.view() == Some(copied_view)).unwrap().id;
    assert_ne!(json!(ids::encode(copy_note)), note);
    assert_eq!(
        run(&mut s, "drawing.note.inspect", json!({"note": ids::encode(copy_note)}))["record"]["content"],
        json!([{"text": "Keep this note"}])
    );
    assert_ne!(copy_id, original_id);
    let copied_mark = run(&mut s, INSPECT, json!({"mark": copy_id}));
    assert_eq!(copied_mark["record"]["edge"], original["record"]["edge"]);
    assert!(s.execute("drawing.sheet.delete", &json!({"sheet": sheet})).is_err());
    run(&mut s, "drawing.sheet.delete", json!({"sheet": sheet, "contents": "delete"}));
    assert!(s.execute(INSPECT, &json!({"mark": copy_id})).is_err());
    assert!(s.execute("drawing.note.inspect", &json!({"note": ids::encode(copy_note)})).is_err());
    assert_eq!(run(&mut s, "drawing.note.inspect", json!({"note": note}))["status"], "ok");
    assert_eq!(run(&mut s, INSPECT, json!({"mark": original_id}))["status"], "ok");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, INSPECT, json!({"mark": copy_id}))["status"], "ok");
    assert_eq!(run(&mut s, "drawing.note.inspect", json!({"note": ids::encode(copy_note)}))["status"], "ok");
}

#[test]
fn centre_line_commands_reject_coincident_points_and_support_undo() {
    let (mut s, view, _) = fixture();
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let add = "drawing.center_line.add";
    assert!(s.execute(add, &json!({"view": ids::encode(view), "a": [0, 0, 0], "b": [0, 0, 0], "overshoot_mm": 2.0})).is_err());
    assert!(s.execute(add, &json!({"view": ids::encode(view), "a": [0, 0, 0], "b": [80, 0, 0], "overshoot_mm": -1.0})).is_err());
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
    let added = run(&mut s, add, json!({"view": ids::encode(view), "a": [0, 0, 0], "b": [80, 0, 0], "overshoot_mm": 2.0}));
    assert_eq!(added["status"], "ok");
    let line = added["line"].clone();
    let saved = s.drawing("test").unwrap().to_json().unwrap();
    assert_eq!(solidcraft_doc::drawing::Drawing::from_json(&saved).unwrap().centre_lines.len(), 1);
    assert_eq!(run(&mut s, "drawing.center_line.edit", json!({"line": line, "overshoot_mm": 3.0}))["changed"], json!(["overshoot_mm"]));
    run(&mut s, "drawing.center_line.remove", json!({"line": line}));
    assert!(s.execute("drawing.center_line.inspect", &json!({"line": line})).is_err());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.center_line.inspect", json!({"line": line}))["record"]["overshoot_mm"], 3.0);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.drawing("test").unwrap().to_json().unwrap(), before);
}

#[test]
fn copied_centre_lines_keep_references_and_are_removed_with_their_sheet() {
    let (mut s, view, _) = fixture();
    let original = run(
        &mut s,
        "drawing.center_line.add",
        json!({
            "view": ids::encode(view), "a": [0, 0, 0], "b": [80, 0, 0], "overshoot_mm": 2.0
        }),
    );
    let line = original["line"].clone();
    let sheet = run(&mut s, "drawing.sheet.add", json!({}))["sheet"].clone();
    let copy = run(&mut s, "drawing.view.copy_to_sheet", json!({"view": ids::encode(view), "sheet": sheet}));
    let copied_view = run(&mut s, "drawing.view.inspect", json!({"view": copy["view"]}));
    let copied_line = copied_view["centre_lines"][0].clone();
    assert_ne!(line, copied_line);
    let copied = run(&mut s, "drawing.center_line.inspect", json!({"line": copied_line}));
    assert_eq!(copied["record"]["a"], original["record"]["a"]);
    assert_eq!(copied["record"]["b"], original["record"]["b"]);
    assert!(s.execute("drawing.sheet.delete", &json!({"sheet": sheet})).is_err());
    run(&mut s, "drawing.sheet.delete", json!({"sheet": sheet, "contents": "delete"}));
    assert!(s.execute("drawing.center_line.inspect", &json!({"line": copied_line})).is_err());
    assert_eq!(run(&mut s, "drawing.center_line.inspect", json!({"line": line}))["status"], "ok");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "drawing.center_line.inspect", json!({"line": copied_line}))["status"], "ok");
}

#[test]
fn centre_line_follows_model_edits_and_restores_on_undo() {
    let (mut s, view, _) = fixture();
    let added = run(
        &mut s,
        "drawing.center_line.add",
        json!({
            "view": ids::encode(view), "a": [0, 0, 0], "b": [80, 0, 0], "overshoot_mm": 2.0
        }),
    );
    let line = added["line"].clone();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    let params = run(&mut s, "parameters.list", json!({"filter":"feature"}));
    let length = params["parameters"].as_array().unwrap().iter().find(|p| p["value"].as_f64() == Some(80.0)).unwrap()["name"].clone();
    run(&mut s, "parameters.change", json!({"name":length,"expression":"100 mm"}));
    run(&mut s, "drawing.open", json!({"drawing":drawing}));
    let changed = run(&mut s, "drawing.center_line.inspect", json!({"line":line}));
    assert_eq!(changed["status"], "ok");
    assert!((changed["points"][1][0].as_f64().unwrap() - 100.0).abs() < 1e-6);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "drawing.open", json!({"drawing":drawing}));
    let restored = run(&mut s, "drawing.center_line.inspect", json!({"line":line}));
    assert_eq!(restored["status"], "ok");
    assert!((restored["points"][1][0].as_f64().unwrap() - 80.0).abs() < 1e-6);
}

#[test]
fn serde_error_conversion_keeps_the_original_message() {
    let error = serde_json::from_str::<Value>("{").unwrap_err();
    let message = error.to_string();
    assert!(matches!(EngineError::from(error), EngineError::Other(text) if text == message));
}

/// A holed box placed through an occurrence and a separate root body, with a front view.
fn placed_hole() -> (Session, u64, u64, usize) {
    let mut s = Session::default();
    let occurrence = run(&mut s, "component.create", json!({"name": "Bracket"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30, "body_name": "Box"}));
    run(
        &mut s,
        "solid.cylinder",
        json!({"radius": 6, "height": 42, "base": [40, -1, 15], "axis": "Y", "operation": "cut", "targets": ["Box"], "name": "Hole"}),
    );
    let state = s.model.state();
    let centre = Vec3::new(40.0, 0.0, 15.0);
    let edge = state.body("Box").unwrap().mesh().edges.iter().position(|line| line.iter().all(|p| (p.dist(centre) - 6.0).abs() < 1e-6)).unwrap();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [200, 0, 0], "operation": "new", "body_name": "Base"}));
    run(&mut s, "drawing.create", json!({}));
    let mut d = (**s.drawing("test").unwrap()).clone();
    let id = d.allocate_id(&mut ids::IdSource::seeded(9421)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([200.0, 150.0]), settings, reference_shown_percent: 0.0 })]);
    d.validate().unwrap();
    s.doc_mut().put_drawing(Arc::new(d));
    (s, id, occurrence, edge)
}

fn refused_without_change(s: &mut Session, cmd: &str, params: Value, why: &str) {
    let before = s.drawing("test").unwrap().to_json().unwrap();
    let steps = s.undo.len();
    let error = s.execute(cmd, &params).err().unwrap_or_else(|| panic!("accepted {cmd}: {params}"));
    assert!(error.to_string().contains(why), "{cmd} {params}: {error}");
    assert_eq!((s.drawing("test").unwrap().to_json().unwrap(), s.undo.len()), (before, steps), "{cmd} {params}");
}

/// Each picked path must be drawn, including either endpoint of a line and a projected view's
/// inherited settings. A refusal changes neither the drawing nor its undo history.
#[test]
fn centre_marks_and_both_line_endpoints_require_drawn_occurrences() {
    let (mut s, view, occurrence, edge) = placed_hole();
    let v = ids::encode(view);
    let projected = run(&mut s, "drawing.view.projected", json!({"parent": v, "side": "right"}))["view"].as_str().unwrap().to_owned();
    let mark = |view: &str| json!({"view": view, "edge": {"body": "Box", "index": edge, "path": [occurrence]}, "overshoot_mm": 2.0});
    let on_part = json!({"point": [0, 0, 0], "path": [occurrence]});
    let on_root = json!([200, 0, 0]);
    let line = |view: &str, a: Value, b: Value| json!({"view": view, "a": a, "b": b, "overshoot_mm": 2.0});

    let added_mark = run(&mut s, ADD, mark(&v));
    assert_eq!(added_mark["status"], "ok");
    assert_eq!(added_mark["record"]["edge"]["path"], json!([occurrence]));
    for (a, b, slot) in [(on_part.clone(), on_root.clone(), "a"), (on_root.clone(), on_part.clone(), "b")] {
        let added = run(&mut s, "drawing.center_line.add", line(&v, a, b));
        assert_eq!(added["status"], "ok");
        assert_eq!(added["record"][slot]["path"], json!([occurrence]));
    }

    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": [[occurrence]]}}));
    for (target, inherited) in [(&v, false), (&projected, true)] {
        let mut why = format!("view {target} doesn't draw that part: its hide leaves out occurrence path [{occurrence}]");
        if inherited {
            why.push_str(&format!(" (set on view {v})"));
        }
        refused_without_change(&mut s, ADD, mark(target), &why);
        refused_without_change(&mut s, "drawing.center_line.add", line(target, on_part.clone(), on_root.clone()), &why);
        refused_without_change(&mut s, "drawing.center_line.add", line(target, on_root.clone(), on_part.clone()), &why);
    }

    run(&mut s, "drawing.view.edit", json!({"view": v, "set": {"hide": null, "source": {"occurrences": [[]]}}}));
    let why = format!("view {v} doesn't draw that part: its source draws only the occurrences it chose");
    refused_without_change(&mut s, ADD, mark(&v), &why);
    refused_without_change(&mut s, "drawing.center_line.add", line(&v, on_part.clone(), on_root.clone()), &why);
    refused_without_change(&mut s, "drawing.center_line.add", line(&v, on_root, on_part), &why);
}

/// A later hide or source exclusion keeps the annotations and warns until the edit is undone.
#[test]
fn centre_marks_and_lines_warn_when_their_view_leaves_the_part_out_later() {
    let (mut s, view, occurrence, edge) = placed_hole();
    let v = ids::encode(view);
    let mark = run(&mut s, ADD, json!({"view": v, "edge": {"body": "Box", "index": edge, "path": [occurrence]}, "overshoot_mm": 2.0}));
    let line = run(
        &mut s,
        "drawing.center_line.add",
        json!({"view": v, "a": {"point": [0, 0, 0], "path": [occurrence]}, "b": [200, 0, 0], "overshoot_mm": 2.0}),
    );
    assert_eq!(mark["status"], "ok");
    assert_eq!(line["status"], "ok");
    assert!(mark["warnings"].is_null() && line["warnings"].is_null());
    let cases = [
        (json!({"hide": [[occurrence]]}), format!("its hide leaves out occurrence path [{occurrence}]")),
        (json!({"source": {"occurrences": [[]]}}), "its source draws only the occurrences it chose".to_owned()),
    ];
    for (set, why) in cases {
        run(&mut s, "drawing.view.edit", json!({"view": v, "set": set}));
        let hidden_mark = run(&mut s, INSPECT, json!({"mark": mark["mark"]}));
        let hidden_line = run(&mut s, "drawing.center_line.inspect", json!({"line": line["line"]}));
        let warnings = json!([format!("view {v} doesn't draw this part now: {why}")]);
        assert_eq!((&hidden_mark["status"], &hidden_mark["record"], &hidden_mark["centre"]), (&json!("ok"), &mark["record"], &mark["centre"]));
        assert_eq!((&hidden_line["status"], &hidden_line["record"], &hidden_line["points"]), (&json!("ok"), &line["record"], &line["points"]));
        assert_eq!(hidden_mark["warnings"], warnings, "{hidden_mark}");
        assert_eq!(hidden_line["warnings"], warnings, "{hidden_line}");
        let mark_schema = crate::schema::output_schema_for::<crate::schema::drawing_centre_mark::MarkResult>();
        let line_schema = crate::schema::output_schema_for::<crate::schema::drawing_centre_line::LineResult>();
        assert!(crate::schema::tests::conforms(mark_schema.as_value(), mark_schema.as_value(), &hidden_mark), "{hidden_mark}");
        assert!(crate::schema::tests::conforms(line_schema.as_value(), line_schema.as_value(), &hidden_line), "{hidden_line}");
        run(&mut s, "edit.undo", json!({}));
        let restored_mark = run(&mut s, INSPECT, json!({"mark": mark["mark"]}));
        let restored_line = run(&mut s, "drawing.center_line.inspect", json!({"line": line["line"]}));
        assert_eq!(restored_mark["record"], mark["record"]);
        assert_eq!(restored_line["record"], line["record"]);
        assert!(restored_mark["warnings"].is_null() && restored_line["warnings"].is_null());
    }
}
