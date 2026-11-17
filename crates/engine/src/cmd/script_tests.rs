//! `timeline.export_script` and `timeline.add` (#886): a design's script replays into the same
//! design, and a recorded feature that does not fit the design is refused, changing nothing.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn script(s: &mut Session) -> Value {
    let r = run(s, "timeline.export_script", json!({}));
    // Through text, as a script file carries it.
    serde_json::from_str(&r["script"].to_string()).unwrap()
}

fn replay(script: &Value) -> Session {
    let mut t = Session::default();
    if let Err(e) = t.run_script(script) {
        panic!("replay: {e}\n{script:#}");
    }
    t
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn params(s: &Session) -> Vec<solidcraft_doc::Param> {
    let mut v = s.doc.params.clone();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn errors(s: &Session) -> Vec<(u64, Option<String>)> {
    s.model.results.iter().map(|r| (r.id, r.error.clone())).collect()
}

fn same(a: &mut Session, b: &mut Session) {
    assert_eq!(a.doc.features, b.doc.features, "timeline");
    assert_eq!(params(a), params(b), "parameters");
    assert_eq!(a.doc.marker, b.doc.marker, "marker");
    assert_eq!(a.doc.favorites, b.doc.favorites, "favourites");
    assert_eq!(a.doc.param_comments, b.doc.param_comments, "comments");
    assert_eq!(a.doc.materials, b.doc.materials, "materials");
    assert_eq!(a.doc.user_materials, b.doc.user_materials, "user materials");
    assert_eq!(errors(a), errors(b), "feature results");
    assert_eq!(volume(a), volume(b), "volume");
    assert_eq!(a.doc.components, b.doc.components, "components");
    assert_eq!(a.doc.assembly.joints, b.doc.assembly.joints, "joints");
    assert_eq!(a.doc.occurrences, b.doc.occurrences, "occurrences");
    assert_eq!(a.doc.assembly.origins, b.doc.assembly.origins, "named joint origins");
    assert_eq!(a.doc.assembly.links, b.doc.assembly.links, "motion links");
    assert_eq!(a.doc.parts, b.doc.parts, "standard parts");
    assert_eq!(a.doc.links, b.doc.links, "linked designs");
}

fn commands(script: &Value) -> Vec<&str> {
    script["commands"].as_array().into_iter().flatten().filter_map(|c| c["command"].as_str()).collect()
}

/// A plate from a dimensioned sketch, with a user parameter driving a dimension.
fn plate(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "w", "expression": "40 mm", "comment": "plate width", "favorite": true}));
    run(s, "parameters.add", json!({"name": "count", "expression": "3", "unit": ""}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(s, "sketch.constraint.coincident", json!({"a": "p1", "b": "origin"}));
    run(s, "sketch.dimension", json!({"entities": ["l1"], "value": "w"}));
    run(s, "sketch.dimension", json!({"entities": ["l2"], "value": "w * 0.75"}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Base", "distance": "10 mm"}));
}

#[test]
fn a_dimensioned_part_replays_into_the_same_design() {
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "solid.fillet", json!({"edges": [[20, 0, 10]], "radius": "2 mm"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[20, 30, 10]], "distance": "1 mm"}));
    let sc = script(&mut s);
    let cmds = commands(&sc);
    assert_eq!(cmds.iter().filter(|c| **c == "timeline.add").count(), 4, "{cmds:?}");
    assert_eq!(cmds.first(), Some(&"parameters.add"), "user parameters come first: {cmds:?}");
    let mut t = replay(&sc);
    same(&mut s, &mut t);
    // Replayed parameters still drive the part.
    run(&mut t, "parameters.change", json!({"name": "w", "expression": "20 mm"}));
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "20 mm"}));
    same(&mut s, &mut t);
}

#[test]
fn ids_names_suppression_marker_and_failures_survive() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "name": "Gone"}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [40, 0, 0], "name": "Kept box"}));
    // A gap in the ids, a renamed feature, a suppressed one.
    run(&mut s, "timeline.delete", json!({"features": ["Gone"]}));
    plate(&mut s);
    run(&mut s, "timeline.suppress", json!({"feature": "Kept box", "suppressed": true}));
    // A feature input named by a user parameter that refers back to it.
    let extrude = s.doc.features.last().unwrap().clone();
    let depth = extrude.param_names.first().cloned().unwrap();
    run(&mut s, "parameters.add", json!({"name": "twice_depth", "expression": format!("{depth} * 2")}));
    run(&mut s, "parameters.comment", json!({"name": depth, "comment": "plate depth"}));
    run(&mut s, "parameters.favorite", json!({"name": depth}));
    // A fillet that a parameter change makes impossible (a radius below zero) fails, and the
    // design keeps it. (A convex round wider than its faces is built now, by subtraction.)
    run(&mut s, "parameters.add", json!({"name": "r", "expression": "1 mm"}));
    // Pick only the plate's edge, clear of the overlapping box's edge.
    run(&mut s, "solid.fillet", json!({"edges": [[20, 0, 10]], "radius": "r"}));
    let fillet = s.doc.features.last().unwrap().id;
    run(&mut s, "parameters.change", json!({"name": "r", "expression": "-1 mm"}));
    assert!(s.model.result(fillet).and_then(|r| r.error.clone()).is_some(), "the fillet fails");
    run(&mut s, "timeline.rollback", json!({"position": 3}));
    let mut t = replay(&script(&mut s));
    same(&mut s, &mut t);
    // The script up to a feature is the design rolled back to just after it.
    let full = script(&mut s);
    let cmds = full["commands"].as_array().unwrap();
    let cut = cmds.iter().position(|c| c["params"]["feature"]["type"] == "extrude").unwrap();
    let mut part = replay(&json!({"commands": cmds[..=cut].to_vec()}));
    let ids: Vec<u64> = part.doc.features.iter().map(|f| f.id).collect();
    assert_eq!(ids, s.doc.features.iter().map(|f| f.id).take(ids.len()).collect::<Vec<_>>());
    run(&mut s, "timeline.rollback", json!({"position": ids.len()}));
    assert_eq!(volume(&mut part), volume(&mut s));
}

#[test]
fn units_materials_and_rules_come_back() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "material.create", json!({"name": "Test resin", "density_g_cm3": 1.3, "color": "#336699", "opacity": 0.5}));
    run(&mut s, "solid.box", json!({"length": "1 in", "width": "1 in", "height": "1 in", "body_name": "Cube"}));
    run(&mut s, "material.assign", json!({"bodies": ["Cube"], "material": "Test resin"}));
    run(&mut s, "sheet.manage_rules", json!({"name": "Thin", "thickness": "0.8 mm", "active": true}));
    let mut t = replay(&script(&mut s));
    same(&mut s, &mut t);
    assert_eq!(t.doc.units, "in");
    assert_eq!(t.doc.sheet, s.doc.sheet);
    let a = run(&mut s, "inspect.measure", json!({}))["total"].clone();
    let b = run(&mut t, "inspect.measure", json!({}))["total"].clone();
    assert_eq!(a, b, "mass comes from the replayed material");
}

#[test]
fn design_properties_come_back_and_odd_keys_are_listed() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "document.metadata.set", json!({"values": {"author": "Jane", "description": "two\nlines", "com.acme.tool": {"id": [1, 2]}}}));
    // Keys another tool wrote that the command would refuse.
    s.doc_mut().metadata.0.insert("revision".into(), json!(3));
    s.doc_mut().metadata.0.insert("org.other".into(), json!("text"));
    let r = run(&mut s, "timeline.export_script", json!({}));
    assert!(r["omitted"].to_string().contains("(org.other, revision)"), "{r}");
    let t = replay(&script(&mut s));
    assert_eq!(
        serde_json::to_value(&t.doc.metadata).unwrap(),
        json!({"author": "Jane", "description": "two\nlines", "com.acme.tool": {"id": [1, 2]}})
    );
}

#[test]
fn what_the_script_leaves_out_is_listed_and_components_are_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "B"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["B"], "color": "#ff0000"}));
    let r = run(&mut s, "timeline.export_script", json!({}));
    assert!(r["omitted"].to_string().contains("appearances"), "{r}");
    // A component is no longer refused: it exports and replays.
    run(&mut s, "component.create", json!({"name": "Sub"}));
    let script = script(&mut s);
    let steps = commands(&script);
    assert!(steps.contains(&"component.create"), "{steps:?}");
    assert!(steps.contains(&"occurrence.move"), "{steps:?}");
    assert!(steps.contains(&"occurrence.ground"), "{steps:?}");
    let mut t = replay(&script);
    same(&mut s, &mut t);
}

#[test]
fn nested_components_are_still_refused() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Outer", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Inner", "parent": "Outer"}));
    let e = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(e.contains("nested components"), "{e}");
}

#[test]
fn omitted_print_faces_are_reported_without_changing_the_design() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"}));
    assert_eq!(run(&mut s, "timeline.export_script", json!({}))["omitted"], json!([]));

    run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 0]}));
    let before = (*s.doc).clone();
    let r = run(&mut s, "timeline.export_script", json!({}));
    assert_eq!(r["omitted"], json!(["print-face assignments (body.print_face)"]));
    assert_eq!(*s.doc, before, "export must keep the saved print-face assignment");
    let mut t = replay(&r["script"]);
    assert!(t.doc.print_faces.is_empty(), "the reported omission matches the replay");
    same(&mut s, &mut t);

    run(&mut s, "body.print_face", json!({"body": "Block"}));
    assert_eq!(run(&mut s, "timeline.export_script", json!({}))["omitted"], json!([]));
}

/// A design with a configuration table: a parameter, a suppression and a material column,
/// two rows, a rule, and the second row active (#15441).
fn configured(s: &mut Session) {
    plate(s);
    run(s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [60, 0, 0], "body_name": "Tab"}));
    run(s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [80, 0, 0], "body_name": "Boss"}));
    let boss = s.doc.features.last().unwrap().name.clone();
    run(s, "material.create", json!({"name": "Test resin", "density_g_cm3": 1.3}));
    run(s, "config.configure", json!({"params": ["w"], "features": [boss.clone()], "materials": ["Tab"]}));
    let mut values = serde_json::Map::new();
    values.insert("param:w".into(), json!("60 mm"));
    values.insert(format!("suppress:{boss}"), json!("true"));
    values.insert("material:Tab".into(), json!("Test resin"));
    run(s, "config.row", json!({"name": "Wide", "values": values}));
    let rule = json!({"id": "span", "name": "Width range", "condition": {"type": "always"},
        "effect": {"type": "parameter_range", "parameter": "w", "minimum": "10 mm", "maximum": "100 mm"}});
    run(s, "config.rule", json!({"rules": [rule]}));
    run(s, "config.activate", json!({"row": "Wide"}));
}

/// A table by what a replay keeps: row ids are new, everything else is the same.
fn table(s: &Session) -> (Vec<solidcraft_doc::config::Column>, Vec<(String, Vec<String>)>, Vec<solidcraft_doc::config::ConfigRule>, Option<String>) {
    let t = &s.doc.configs;
    (t.columns.clone(), t.rows.iter().map(|r| (r.name.clone(), r.cells.clone())).collect(), t.rules.clone(), t.active.clone())
}

#[test]
fn a_configuration_table_comes_back() {
    let mut s = Session::default();
    configured(&mut s);
    let r = run(&mut s, "timeline.export_script", json!({}));
    assert_eq!(r["omitted"], json!([]), "{r}");
    let mut t = replay(&r["script"]);
    assert_eq!(table(&t), table(&s));
    assert!(t.doc.configuration_matches(), "the active row still describes the design");
    same(&mut s, &mut t);
    // The replayed rows switch the design as the originals do.
    run(&mut s, "config.activate", json!({"row": "Default"}));
    run(&mut t, "config.activate", json!({"row": "Default"}));
    same(&mut s, &mut t);
}

#[test]
fn a_design_that_moved_off_its_active_row_keeps_the_table_and_reports_the_row() {
    let mut s = Session::default();
    configured(&mut s);
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "45 mm"}));
    let r = run(&mut s, "timeline.export_script", json!({}));
    assert_eq!(r["omitted"], json!(["the active configuration (the design no longer matches its row)"]), "{r}");
    let mut t = replay(&r["script"]);
    let (theirs, mut ours) = (table(&t), table(&s));
    // The replay has no active row; everything else in the table is the same.
    assert_eq!(theirs.3, None);
    ours.3 = None;
    assert_eq!(theirs, ours);
    same(&mut s, &mut t);
}

/// The export leaves the whole table out and says why, and the rest of the script replays
/// without it (#15441).
fn left_out(s: &mut Session, reason: &str) {
    let r = run(s, "timeline.export_script", json!({}));
    let omitted = r["omitted"].to_string();
    assert!(omitted.contains("configurations (") && omitted.contains(reason), "{r}");
    let t = replay(&r["script"]);
    assert!(t.doc.configs.is_empty(), "the table is left out whole");
}

#[test]
fn a_column_for_a_text_parameter_leaves_the_table_out() {
    let mut s = Session::default();
    plate(&mut s);
    // The script does not create text parameters, so a replay would have no `Label` to configure.
    s.doc_mut().text_params.push(solidcraft_doc::text_params::TextParam { name: "Label".into(), text: "Plate".into(), comment: String::new() });
    run(&mut s, "config.configure", json!({"params": ["Label"]}));
    left_out(&mut s, "parameter `Label`");
}

#[test]
fn a_body_column_whose_body_is_not_there_leaves_the_table_out() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "Tab"}));
    let tab = s.doc.features.last().unwrap().name.clone();
    run(&mut s, "config.configure", json!({"materials": ["Tab"]}));
    // The feature that makes `Tab` is suppressed, so a replay builds no `Tab` to put a column on.
    run(&mut s, "timeline.suppress", json!({"feature": tab, "suppressed": true}));
    left_out(&mut s, "body `Tab`");
}

#[test]
fn a_row_the_rules_refuse_leaves_the_table_out() {
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "parameters.add", json!({"name": "cap", "expression": "100 mm"}));
    run(&mut s, "config.configure", json!({"params": ["w"]}));
    run(&mut s, "config.row", json!({"name": "Wide", "values": {"param:w": "60 mm"}}));
    let rule = json!({"id": "span", "name": "Width range", "condition": {"type": "always"},
        "effect": {"type": "parameter_range", "parameter": "w", "minimum": "10 mm", "maximum": "cap"}});
    run(&mut s, "config.rule", json!({"rules": [rule]}));
    // Lowering the cap leaves the stored row outside the range; `config.rule` would refuse it.
    run(&mut s, "parameters.change", json!({"name": "cap", "expression": "50 mm"}));
    left_out(&mut s, "row `Wide`");
}

#[test]
fn a_recorded_feature_that_does_not_fit_is_refused_and_changes_nothing() {
    let mut s = Session::default();
    plate(&mut s);
    let sc = script(&mut s);
    let sketch = sc["commands"].as_array().unwrap().iter().find(|c| c["params"]["feature"]["type"] == "sketch").unwrap()["params"].clone();
    let extrude = sc["commands"].as_array().unwrap().iter().find(|c| c["params"]["feature"]["type"] == "extrude").unwrap()["params"].clone();
    let before = (*s.doc).clone();
    let with = |base: &Value, path: &[&str], v: Value| {
        let mut p = base.clone();
        let mut at = &mut p;
        for k in &path[..path.len() - 1] {
            at = &mut at[*k];
        }
        at[path[path.len() - 1]] = v;
        p
    };
    let refused = [
        ("id taken", with(&extrude, &["feature", "name"], json!("Other"))),
        ("id out of range", with(&with(&extrude, &["feature", "id"], json!(1u64 << 60)), &["feature", "name"], json!("X"))),
        ("parameter taken", with(&with(&sketch, &["feature", "id"], json!(0)), &["feature", "name"], json!("S2"))),
        (
            "bad parameter name",
            with(
                &json!({"feature": {"type": "box", "length": "1", "width": "1", "height": "1"}}),
                &["params"],
                json!([{"name": "1x", "expression": "1"}]),
            ),
        ),
        (
            "unknown unit",
            with(
                &json!({"feature": {"type": "box", "length": "1", "width": "1", "height": "1"}}),
                &["params"],
                json!([{"name": "q", "expression": "1", "unit": "parsec"}]),
            ),
        ),
        ("bad input names", with(&extrude, &["feature", "param_names"], json!([1, 2]))),
        ("unknown type", json!({"feature": {"type": "warp_drive"}})),
        ("huge number", json!({"feature": {"type": "box", "length": "1", "width": "1", "height": "1", "corner": [1e300, 0, 0]}})),
        ("unknown component", json!({"feature": {"type": "box", "length": "1", "width": "1", "height": "1", "component": 9}})),
        ("not an object", json!({"feature": [1, 2, 3]})),
    ];
    for (what, p) in refused {
        assert!(s.execute("timeline.add", &p).is_err(), "{what}: {p}");
        assert_eq!(*s.doc, before, "{what}: nothing changed");
    }
    // A hand-written step needs only the feature's own fields.
    let r = run(&mut s, "timeline.add", json!({"feature": {"type": "box", "length": "5", "width": "5", "height": "5", "corner": [100, 0, 0]}}));
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(r["name"], "Box1");
    assert!(s.doc.feature(r["feature"].as_u64().unwrap()).is_some_and(|f| !f.param_names.is_empty()), "its inputs get parameter names");
}

/// #10748: `timeline.inspect` reports a feature's design variant name in `type` and display name
/// in `type_name`; `timeline.add` accepts `timeline.inspect` results and inspect type names.
#[test]
fn timeline_add_accepts_inspect_result_and_type_names() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 5, "name": "Block"}));
    let mut inspected = run(&mut s, "timeline.inspect", json!({"feature": "Block"}));
    assert_eq!(inspected["type"], "box", "inspect type is the design file variant name");
    assert_eq!(inspected["type_name"], "BoxFeature", "inspect type_name is the display type");

    // 1. timeline.add accepts the full inspected object directly in a new session
    let mut s2 = Session::default();
    let added = run(&mut s2, "timeline.add", json!({"feature": inspected.clone()}));
    assert!(added["error"].is_null(), "{added}");
    assert_eq!(s2.doc.features.len(), 1);

    // 2. timeline.add with id 0 allocates a new id in the current session
    inspected["id"] = json!(0);
    let added2 = run(&mut s, "timeline.add", json!({"feature": inspected}));
    assert!(added2["error"].is_null(), "{added2}");
    assert_eq!(s.doc.features.len(), 2);

    // 3. timeline.add accepts the inspect type name (e.g. BoxFeature, HoleFeature)
    let added_by_type_name =
        run(&mut s, "timeline.add", json!({"feature": {"type": "BoxFeature", "length": "5", "width": "5", "height": "5", "corner": [50, 0, 0]}}));
    assert!(added_by_type_name["error"].is_null(), "{added_by_type_name}");
}

#[test]
fn the_script_is_written_to_a_file_that_replays() {
    let mut s = Session::default();
    plate(&mut s);
    let dir = std::env::temp_dir().join(format!("solidcraft-script-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("repro.json");
    let r = run(&mut s, "timeline.export_script", json!({"path": path.to_string_lossy()}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let written: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(written, r["script"]);
    let mut t = replay(&written);
    same(&mut s, &mut t);
    assert!(s.execute("timeline.export_script", &json!({"path": 7})).is_err(), "a path is text");
}

#[test]
fn replayed_features_claiming_the_same_body_name_do_not_crash() {
    let mut s = Session::default();
    let boxed = |name: &str, x: f64| json!({"feature": {"type": "box", "name": name, "length": "5", "width": "5", "height": "5", "corner": [x, 0, 0], "body_names": ["Twin"]}});
    run(&mut s, "timeline.add", boxed("A", 0.0));
    let r = s.execute("timeline.add", &boxed("B", 20.0));
    assert!(!matches!(r, Err(crate::EngineError::Internal(..))), "{r:?}");
    let m = run(&mut s, "inspect.measure", json!({}));
    assert!(m["body_count"].as_u64().is_some_and(|n| n >= 1), "{m}");
}

/// #1990: a sketch and the extrude built from it may share a name, as the sample and oracle
/// designs do; the design's script replays into the same design, names included.
#[test]
fn a_design_naming_a_sketch_and_its_extrude_alike_replays() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Block"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "5", "name": "Block"}));
    let names = |t: &Session| t.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s), ["Block", "Block"]);
    let written = script(&mut s);
    assert_eq!(commands(&written), ["timeline.add", "timeline.add"]);
    let mut t = replay(&written);
    same(&mut s, &mut t);
    assert_eq!(names(&t), ["Block", "Block"], "the replay keeps both names");
    assert_eq!(volume(&mut t), 1000.0);
    // The replay's own script is the same script.
    assert_eq!(script(&mut t), written);
}

/// #1990: `timeline.add` keeps a recorded name another feature has, as `solid.extrude` does.
#[test]
fn a_replayed_feature_keeps_a_name_another_feature_has() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": "5", "width": "5", "height": "5", "name": "Twin"}));
    let r = run(
        &mut s,
        "timeline.add",
        json!({"feature": {"type": "box", "name": "Twin", "length": "5", "width": "5", "height": "5", "corner": [20, 0, 0]}}),
    );
    assert_eq!(r["name"], "Twin");
    assert_eq!(s.doc.features.iter().filter(|f| f.name == "Twin").count(), 2);
    // A blank recorded name still gets a fresh one.
    let r = run(
        &mut s,
        "timeline.add",
        json!({"feature": {"type": "box", "name": " ", "length": "5", "width": "5", "height": "5", "corner": [40, 0, 0]}}),
    );
    assert_eq!(r["name"], "Box1");
}

/// #1134: a script recorded before pattern and mirror seeds and construction planes were
/// keyed by id names them by name; timeline.add resolves the names to ids exactly the way
/// opening a saved design migrates them, and a name that finds no feature keeps id 0, which
/// dangles at evaluation with the same error it always gave.
#[test]
fn add_resolves_legacy_seed_and_plane_names() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": "5", "width": "5", "height": "5", "name": "Base"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 10, "name": "Shelf"}));
    let base = s.doc.find_feature("Base").unwrap().id;
    let shelf = s.doc.find_feature("Shelf").unwrap().id;
    let r = run(
        &mut s,
        "timeline.add",
        json!({"feature": {"type": "mirror", "name": "Copied", "features": ["Base"], "plane": {"type": "construction", "name": "Shelf"}}}),
    );
    assert!(r["error"].is_null(), "{r}");
    let kind = serde_json::to_value(&s.doc.find_feature("Copied").unwrap().kind).unwrap();
    assert_eq!(kind["features"], json!([base]), "{kind}");
    assert_eq!(kind["plane"], json!({"type": "construction", "feature": shelf}), "{kind}");
    // A name that finds no feature keeps id 0: the mirror fails at evaluation, as a design
    // migrated with a dangling seed does.
    let r = run(
        &mut s,
        "timeline.add",
        json!({"feature": {"type": "mirror", "name": "Lost", "features": ["Nope"], "plane": {"type": "construction", "name": "Gone"}}}),
    );
    assert!(!r["error"].is_null(), "a dangling seed fails at evaluation: {r}");
    let kind = serde_json::to_value(&s.doc.find_feature("Lost").unwrap().kind).unwrap();
    assert_eq!(kind["features"], json!([0]), "{kind}");
    assert_eq!(kind["plane"], json!({"type": "construction", "feature": 0}), "{kind}");
}

#[test]
fn a_3d_sketch_with_dimensions_and_constraints_replays() {
    // #10608: a 3D path whose run is dimensioned by parameter `run`, the rise held along Z.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let r = run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [100, 0, 0], [100, 0, 60]]}));
    let (a, b) = (r["curves"][0].as_str().unwrap().to_string(), r["curves"][1].as_str().unwrap().to_string());
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{a}.start")}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": a, "axis": "x"}));
    run(&mut s, "sketch.constraint.along_axis", json!({"line": b, "axis": "z"}));
    run(&mut s, "sketch.dimension", json!({"entities": [a], "value": "run = 120"}));
    run(&mut s, "sketch.finish", json!({}));
    let mut t = replay(&script(&mut s));
    let sketch = |s: &Session| {
        s.doc.features.iter().find_map(|f| match &f.kind {
            solidcraft_doc::FeatureKind::Sketch { sketch, .. } => Some(sketch.clone()),
            _ => None,
        })
    };
    let (was, now) = (sketch(&s).unwrap(), sketch(&t).unwrap());
    assert_eq!(now.wire_constraints, was.wire_constraints);
    assert_eq!(now.wires, was.wires);
    // The parameter still drives the replayed path.
    run(&mut t, "parameters.change", json!({"name": "run", "expression": "150 mm"}));
    let end = sketch(&t).unwrap().wires.iter().find(|w| w.id == a).unwrap().fit[1];
    assert!((end.x - 150.0).abs() < 1e-6, "{end:?}");
}

/// Two components with placements, grounding, a revolute joint with values and
/// limits, and a motion link: the script replays into the same design, ids
/// included (both sessions allocate in the same order).
#[test]
fn an_assembly_with_a_joint_and_a_motion_link_replays() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base", "grounded": true, "activate": false}));
    run(&mut s, "component.create", json!({"name": "Arm", "activate": false}));
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence": "Arm:1", "space": "local",
        "to_matrix": [1, 0, 0, 10, 0, 1, 0, 0, 0, 0, 1, 0]}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "Pivot",
        "a": {"occurrence": "Base:1", "frame": {"origin": [10, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}},
        "b": {"occurrence": "Arm:1", "frame": {"origin": [10, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}},
        "values": [30]}),
    );
    run(&mut s, "joint.limits", json!({"joint": "Pivot", "min": -10, "max": 45}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "name": "Slide",
        "a": {"occurrence": "Base:1", "frame": {"origin": [0, 0, 0], "z": [1, 0, 0]}},
        "b": {"occurrence": "Arm:1", "frame": {"origin": [0, 0, 0], "z": [1, 0, 0]}}}),
    );
    run(&mut s, "joint.motion_link", json!({"a": "Pivot", "b": "Slide", "ratio": 2.0}));

    let script = script(&mut s);
    let steps = commands(&script);
    assert_eq!(steps.iter().filter(|c| **c == "component.create").count(), 2);
    assert!(steps.contains(&"occurrence.move"));
    assert!(steps.contains(&"occurrence.ground"));
    assert!(steps.contains(&"joint.create"));
    assert!(steps.contains(&"joint.motion_link"));
    // Values travel explicitly in degrees (a joint's values are anchored at
    // its snap mate, so re-creating it where the parts are keeps them);
    // limits travel as their own steps in degrees.
    let pivot = script["commands"].as_array().unwrap().iter().find(|c| c["command"] == "joint.create" && c["params"]["name"] == "Pivot").unwrap();
    let values = pivot["params"]["values"].as_array().unwrap();
    assert_eq!(values.len(), 1, "{pivot}");
    assert!((values[0].as_f64().unwrap() - 30.0).abs() < 1e-9, "values travel explicitly in degrees: {pivot}");
    // The rest angle and the offset travel too, zero included: a value left
    // out would be re-inferred at creation from wherever the parts sit.
    assert_eq!(pivot["params"]["angle"], json!(0.0), "{pivot}");
    assert_eq!(pivot["params"]["offset"], json!(0.0), "{pivot}");
    let limits = script["commands"].as_array().unwrap().iter().find(|c| c["command"] == "joint.limits" && c["params"]["joint"] == "Pivot").unwrap();
    assert!((limits["params"]["min"].as_f64().unwrap() + 10.0).abs() < 1e-9);
    assert!((limits["params"]["max"].as_f64().unwrap() - 45.0).abs() < 1e-9);

    let mut t = replay(&script);
    same(&mut s, &mut t);
}

/// Feature ownership follows the replayed components even when creation
/// interleaves and ids leave gaps.
#[test]
fn features_keep_their_component_through_the_replay_ids() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Chassis", "activate": true}));
    plate(&mut s);
    run(&mut s, "component.create", json!({"name": "Lid", "parent": "root", "activate": true}));
    let script = script(&mut s);
    let mut t = replay(&script);
    same(&mut s, &mut t);
    for (a, b) in s.doc.features.iter().zip(t.doc.features.iter()) {
        assert_eq!(a.component, b.component, "ownership of {}", a.name);
    }
}

/// Occurrence copies replay with their placements and names.
#[test]
fn occurrence_copies_replay() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Cap", "activate": false}));
    run(&mut s, "occurrence.copy", json!({"occurrence": "Cap:1", "translate": [5, 0, 0]}));
    let script = script(&mut s);
    let mut t = replay(&script);
    same(&mut s, &mut t);
    assert_eq!(s.doc.occurrences.len(), 2);
}

/// A revolute joint whose value and offset follow parameters, a rigid joint
/// and a suppressed joint: the script replays into the same design,
/// expressions verbatim, and the replayed expressions still drive the
/// assembly as the parameters change.
#[test]
fn expressions_a_rigid_joint_and_a_suppressed_joint_replay() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "turn", "expression": "30 deg"}));
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "2 mm"}));
    run(&mut s, "component.create", json!({"name": "Base", "grounded": true, "activate": false}));
    run(&mut s, "component.create", json!({"name": "Arm", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Block", "activate": false}));
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence": "Arm:1", "space": "local",
        "to_matrix": [1, 0, 0, 10, 0, 1, 0, 0, 0, 0, 1, 0]}),
    );
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence": "Block:1", "space": "local",
        "to_matrix": [1, 0, 0, 0, 0, 1, 0, 20, 0, 0, 1, 0]}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "Pivot",
        "a": {"occurrence": "Base:1", "frame": {"origin": [10, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}},
        "b": {"occurrence": "Arm:1", "frame": {"origin": [10, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}},
        "values": ["turn"], "offset": "gap", "limits": [[-10, 45]]}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Weld",
        "a": {"occurrence": "Base:1", "frame": {"origin": [0, 20, 0], "z": [0, 0, 1]}},
        "b": {"occurrence": "Block:1", "frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}}}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "name": "Slide",
        "a": {"occurrence": "Base:1", "frame": {"origin": [0, 0, 0], "z": [1, 0, 0]}},
        "b": {"occurrence": "Arm:1", "frame": {"origin": [0, 0, 0], "z": [1, 0, 0]}}}),
    );
    run(&mut s, "joint.edit", json!({"joint": "Slide", "suppressed": true}));

    let script = script(&mut s);
    let pivot = script["commands"].as_array().unwrap().iter().find(|c| c["command"] == "joint.create" && c["params"]["name"] == "Pivot").unwrap();
    assert_eq!(pivot["params"]["values"], json!(["turn"]), "{pivot}");
    assert_eq!(pivot["params"]["offset"], json!("gap"), "{pivot}");
    let mut t = replay(&script);
    same(&mut s, &mut t);
    // The replayed expressions still drive the assembly.
    for (name, value) in [("turn", "45 deg"), ("gap", "3 mm")] {
        run(&mut s, "parameters.change", json!({"name": name, "expression": value}));
        run(&mut t, "parameters.change", json!({"name": name, "expression": value}));
    }
    same(&mut s, &mut t);
}

/// A joint made as an assembly constraint carries the rule it was made as,
/// and no script step restores that tag: the export refuses it with its own
/// message rather than replaying it as a plain joint.
#[test]
fn a_joint_made_as_an_assembly_constraint_is_refused() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Stand"}));
    run(&mut s, "component.create", json!({"name": "Lid", "parent": "root", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Top"}));
    run(
        &mut s,
        "assembly.constraint.create",
        json!({"type": "mate", "name": "Seat",
        "a": {"occurrence": "Base:1", "face": [5, 5, 10]},
        "b": {"occurrence": "Lid:1", "face": [25, 5, 0]}}),
    );
    let e = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(e.contains("assembly constraint"), "{e}");
}

/// A standard part replays through parts.insert at its own ids: the library
/// rebuilds its features, its seat joint travels through the joint pass (not
/// as picks), and the part properties the design edited come back.
#[test]
fn a_standard_part_replays_with_its_seat_joint() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base", "grounded": true, "activate": true}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "parts.insert", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20, "at": [10, 10, 5]}));
    run(&mut s, "component.metadata.set", json!({"component": "SHCS M6x20", "description": "cover screw"}));
    // A user parameter may refer to the names the part's builder gives its
    // inputs; they come back with the part.
    let comp = s.doc.components.iter().find(|c| c.name.starts_with("SHCS")).unwrap().id;
    let input = s.doc.features.iter().find(|f| f.component == comp && !f.param_names.is_empty()).unwrap().param_names[0].clone();
    run(&mut s, "parameters.add", json!({"name": "grip", "expression": format!("{input} * 2")}));

    let script = script(&mut s);
    let steps = commands(&script);
    assert!(steps.contains(&"parts.insert"), "{steps:?}");
    assert!(steps.contains(&"joint.create"), "{steps:?}");
    let insert = script["commands"].as_array().unwrap().iter().find(|c| c["command"] == "parts.insert").unwrap();
    assert_eq!(insert["params"]["family"], "socket_head_cap_screw", "{insert}");
    assert_eq!(insert["params"]["size"], "M6", "{insert}");
    assert_eq!(insert["params"]["name"], "SHCS M6x20", "{insert}");
    assert!(insert["params"].get("at").is_none(), "the seat joint travels through the joint pass, not as picks: {insert}");
    let mut t = replay(&script);
    same(&mut s, &mut t);
}

/// A standard part whose builder output someone changed is refused with its
/// own message per kind of change: the replay would silently replace the
/// changes with the library's.
#[test]
fn a_changed_standard_part_is_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut s, "parts.insert", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20, "at": [10, 10, 5]}));
    let comp = s.doc.components.iter().find(|c| c.name.starts_with("SHCS")).unwrap().id;
    let built: Vec<u64> = s.doc.features.iter().filter(|f| f.component == comp).map(|f| f.id).collect();
    assert!(built.len() >= 2, "{built:?}");
    let target = *built.last().unwrap();
    let name = s.doc.feature(target).unwrap().name.clone();

    // Hand-edited: a renamed builder feature comes back under the library's name.
    s.doc_mut().feature_mut(target).unwrap().name = "Renamed".to_string();
    let e = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(e.contains("hand-edited"), "{e}");
    s.doc_mut().feature_mut(target).unwrap().name = name.clone();

    // Suppressed.
    run(&mut s, "timeline.suppress", json!({"feature": name, "suppressed": true}));
    let e = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(e.contains("suppressed"), "{e}");
    run(&mut s, "timeline.suppress", json!({"feature": name, "suppressed": false}));

    // Deleted.
    run(&mut s, "timeline.delete", json!({"features": [name]}));
    let e = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(e.contains("deleted"), "{e}");
}

/// A linked design replays through component.insert_design: the constructor
/// reads the file again and rebuilds the Import feature, the report says the
/// file must be present at replay, and the design copies the link again like
/// any component.
#[test]
fn a_linked_design_replays_and_copies() {
    let dir = std::env::temp_dir().join(format!("solidcraft-link-script-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("part.solidcraft");
    let mut u = Session::default();
    run(&mut u, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut u, "file.save", json!({"path": source.to_string_lossy()}));

    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": source.to_string_lossy(), "name": "Cube"}));
    run(&mut s, "occurrence.copy", json!({"occurrence": "Cube:1", "translate": [30, 0, 0]}));

    let r = run(&mut s, "timeline.export_script", json!({}));
    assert!(r["omitted"].to_string().contains("must be present at replay"), "{r}");
    let steps = commands(&r["script"]);
    assert!(steps.contains(&"component.insert_design"), "{steps:?}");
    assert!(steps.contains(&"occurrence.copy"), "{steps:?}");
    let mut t = replay(&r["script"]);
    same(&mut s, &mut t);

    // A suppressed Import feature is refused: the constructor would bring back
    // the linked geometry the design keeps out.
    run(&mut s, "timeline.suppress", json!({"feature": "Cube", "suppressed": true}));
    let e = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(e.contains("suppressed"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A features-only design with a reordered timeline keeps exporting: the
/// script replays the features in timeline order with their stored ids.
#[test]
fn a_reordered_features_only_timeline_still_exports() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "name": "First", "body_name": "Box"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "name": "Second", "body_name": "Cube"}));
    run(&mut s, "timeline.reorder", json!({"feature": "Second", "position": 0}));
    let script = script(&mut s);
    let mut t = replay(&script);
    same(&mut s, &mut t);
}

/// A linked file is named as the replay resolves it: a design saved beside its
/// part keeps the path relative to its own folder, and the script writes the
/// file's place so a replay from another folder reads the same file.
#[test]
fn a_linked_design_names_its_file_by_its_place() {
    let dir = std::env::temp_dir().join(format!("solidcraft-link-place-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("part.solidcraft");
    let mut u = Session::default();
    run(&mut u, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut u, "file.save", json!({"path": source.to_string_lossy()}));

    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": source.to_string_lossy(), "name": "Cube"}));
    run(&mut s, "file.save", json!({"path": dir.join("assembly.solidcraft").to_string_lossy()}));
    // The saved file keeps the link relative to its own folder; the design in
    // memory keeps the file's place. A link read as the file keeps it would
    // resolve against wherever the replay runs.
    s.doc_mut().links.values_mut().next().unwrap().path = "part.solidcraft".to_string();

    let r = run(&mut s, "timeline.export_script", json!({}));
    let insert = r["script"]["commands"].as_array().unwrap().iter().find(|c| c["command"] == "component.insert_design").unwrap();
    let named = insert["params"]["path"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&named).is_absolute(), "the script names the file by its place: {named}");

    let t = replay(&r["script"]);
    assert_eq!(t.doc.links.values().next().unwrap().path, named);
    assert_eq!(std::fs::read(&named).unwrap(), std::fs::read(&source).unwrap(), "the replay reads the file the design named");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same design read through the places its items sit in: a replay gives
/// components, occurrences and joints fresh ids wherever deleted steps left
/// gaps, so identity compares by place and every reference by the place it
/// names. Features keep their ids and their contents.
fn same_rebuilt(a: &mut Session, b: &mut Session) {
    fn places(s: &Session) -> (BTreeMap<u64, u64>, BTreeMap<u64, u64>, BTreeMap<u64, u64>) {
        let by = |ids: Vec<u64>| ids.into_iter().enumerate().map(|(i, id)| (id, i as u64)).collect();
        (
            by(s.doc.components.iter().map(|c| c.id).collect()),
            by(s.doc.occurrences.iter().map(|o| o.id).collect()),
            by(s.doc.assembly.joints.iter().map(|j| j.id).collect()),
        )
    }
    let (ca, oa, ja) = places(a);
    let (cb, ob, jb) = places(b);
    let features = |s: &Session, c: &BTreeMap<u64, u64>, o: &BTreeMap<u64, u64>| -> Vec<Value> {
        s.doc
            .features
            .iter()
            .map(|f| {
                let mut v = serde_json::to_value(f).unwrap();
                super::script::remap_replay_refs(&mut v, c, o).unwrap();
                v
            })
            .collect()
    };
    assert_eq!(features(a, &ca, &oa), features(b, &cb, &ob), "timeline");
    assert_eq!(a.doc.components.len(), b.doc.components.len(), "components");
    for (x, y) in a.doc.components.iter().zip(&b.doc.components) {
        assert_eq!(x.name, y.name);
        assert_eq!(x.metadata, y.metadata);
        assert_eq!(x.properties, y.properties);
    }
    assert_eq!(a.doc.occurrences.len(), b.doc.occurrences.len(), "occurrences");
    for (x, y) in a.doc.occurrences.iter().zip(&b.doc.occurrences) {
        assert_eq!(x.name, y.name);
        assert_eq!(x.transform, y.transform);
        assert_eq!(x.grounded, y.grounded);
        assert_eq!(ca[&x.component], cb[&y.component], "placement of {}", x.name);
    }
    assert_eq!(a.doc.assembly.joints.len(), b.doc.assembly.joints.len(), "joints");
    for (x, y) in a.doc.assembly.joints.iter().zip(&b.doc.assembly.joints) {
        assert_eq!(x.name, y.name);
        assert_eq!(x.kind, y.kind);
        assert_eq!(x.values, y.values);
        assert_eq!(x.limits, y.limits);
        assert_eq!(oa[&x.a.occurrence], ob[&y.a.occurrence], "a side of {}", x.name);
        assert_eq!(oa[&x.b.occurrence], ob[&y.b.occurrence], "b side of {}", x.name);
    }
    assert_eq!(a.doc.assembly.links.len(), b.doc.assembly.links.len(), "motion links");
    for (x, y) in a.doc.assembly.links.iter().zip(&b.doc.assembly.links) {
        assert_eq!(ja[&x.a], jb[&y.a]);
        assert_eq!(ja[&x.b], jb[&y.b]);
        assert_eq!(x.ratio, y.ratio);
    }
    assert_eq!(volume(a), volume(b), "volume");
}

/// Deleted steps leave gaps the replay's allocator does not cross: items
/// created after one come back under fresh ids, and ownership, placements,
/// grounding and assembly behaviour survive the renumbering. A joint created
/// before a later feature has an id of its own to spare.
#[test]
fn items_renumbered_past_a_gap_keep_ownership_and_behaviour() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "First"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "name": "Gone"}));
    run(&mut s, "timeline.delete", json!({"features": ["Gone"]}));
    run(&mut s, "component.create", json!({"name": "Holder", "activate": true}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, 20, 0], "body_name": "Held"}));
    run(&mut s, "component.create", json!({"name": "Tail", "parent": "root", "activate": false}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Weld",
        "a": {"occurrence": "Holder:1", "frame": {"origin": [0, 20, 0], "z": [0, 0, 1]}},
        "b": {"occurrence": "Tail:1", "frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}}}),
    );
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, 0, 20], "body_name": "Late"}));

    let mut t = replay(&script(&mut s));
    // The replay numbers past the gap differently; everything else holds.
    assert_ne!(s.doc.components.first().unwrap().id, t.doc.components.first().unwrap().id);
    same_rebuilt(&mut s, &mut t);
    for (x, y) in s.doc.features.iter().zip(t.doc.features.iter()) {
        let owners = |f: &solidcraft_doc::Feature, s: &Session| s.doc.components.iter().position(|c| c.id == f.component);
        assert_eq!(owners(x, &s), owners(y, &t), "ownership of {}", x.name);
    }
}

/// A sketch placed on an occurrence's datum follows the occurrence to the id
/// the replay gives it; a path through more than one occurrence cannot be
/// predicted and is refused with its own message, apart from the one for
/// bodies moved between components.
#[test]
fn datum_paths_follow_their_place_and_nested_ones_refuse() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "First"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "name": "Gone"}));
    run(&mut s, "timeline.delete", json!({"features": ["Gone"]}));
    run(&mut s, "component.create", json!({"name": "Holder", "activate": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Placed"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    let holder = s.doc.occurrences.iter().find(|o| o.name == "Holder:1").unwrap().id;
    let sketch = s.doc.features.iter().find(|f| f.name == "Placed").unwrap().id;
    let datum = |path: Vec<u64>| solidcraft_doc::PlaneRef::Datum {
        datum: solidcraft_doc::construct::DatumRef { path, item: solidcraft_doc::construct::DatumItem::XY },
    };
    let place_on = |s: &mut Session, path: Vec<u64>| {
        if let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &mut s.doc_mut().feature_mut(sketch).unwrap().kind {
            *plane = datum(path);
        }
    };
    place_on(&mut s, vec![holder]);

    // The exported step names the datum by the replay's place for it, and the
    // rebuilt design comes back the same read through the places.
    let r = run(&mut s, "timeline.export_script", json!({}));
    let text = r["script"].to_string();
    assert!(!text.contains(&format!("o:{holder}/origin/XY")), "the path is rewritten: {text}");
    let mut t = replay(&r["script"]);
    same_rebuilt(&mut s, &mut t);

    // A nested path cannot be predicted.
    place_on(&mut s, vec![holder, 99]);
    let nested = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(nested.contains("nested occurrence path"), "{nested}");

    // Its own message, apart from the one for moved bodies.
    let mut m = Session::default();
    run(&mut m, "component.create", json!({"name": "From", "activate": true}));
    run(&mut m, "solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "Mover"}));
    run(&mut m, "component.create", json!({"name": "To", "parent": "root", "activate": true}));
    run(&mut m, "component.move_bodies", json!({"bodies": ["Mover"], "component": "To"}));
    let moved = m.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(moved.contains("bodies moved between components"), "{moved}");
    assert!(!moved.contains("nested occurrence path"), "{moved}");
    assert!(!nested.contains("bodies moved between components"), "{nested}");
}

/// A renamed occurrence and an identification colour the palette cannot reach
/// are said out loud: the first refuses (the script names occurrences by the
/// naming rule), the second is listed as left out.
#[test]
fn renamed_occurrences_refuse_and_odd_identification_colours_are_listed() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Cap", "activate": false}));
    let first = s.doc.occurrences[0].id;
    s.doc_mut().occurrences.iter_mut().find(|o| o.id == first).unwrap().name = "Cap:7".to_string();
    let e = s.execute("timeline.export_script", &json!({})).unwrap_err().to_string();
    assert!(e.contains("name differently"), "{e}");
    s.doc_mut().occurrences.iter_mut().find(|o| o.id == first).unwrap().name = "Cap:1".to_string();

    run(&mut s, "component.cycle_color", json!({"component": "Cap"}));
    run(&mut s, "component.cycle_color", json!({"component": "Cap"}));
    let r = run(&mut s, "timeline.export_script", json!({}));
    assert!(r["omitted"].to_string().contains("identification colour"), "{r}");
}
