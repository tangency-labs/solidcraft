use serde_json::{Value, json};

use super::{Bom, BomParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// As in the command's own tests: a plate placed three times, and a hinge subassembly placed
/// twice that holds one pin. Here the hinge has a part number too, so it can keep its pin.
fn assembly(s: &mut Session) {
    run(s, "component.create", json!({"name": "Plate"}));
    run(s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(s, "component.activate", json!({"component": 0}));
    run(s, "component.create", json!({"name": "Hinge", "activate": false}));
    run(s, "component.create", json!({"name": "Pin", "parent": "Hinge"}));
    run(s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(s, "component.activate", json!({"component": 0}));
    for x in [50, 100] {
        run(s, "occurrence.copy", json!({"component": "Plate", "translate": [x, 0, 0]}));
    }
    run(s, "occurrence.copy", json!({"component": "Hinge", "translate": [0, 50, 0]}));
    run(s, "component.metadata.set", json!({"component": "Plate", "part_number": "PL-100", "description": "Plate, 3 mm", "revision": "B"}));
    run(s, "component.metadata.set", json!({"component": "Pin", "part_number": "PN-040", "properties": {"finish": "zinc"}}));
    run(s, "component.metadata.set", json!({"component": "Hinge", "part_number": "HG-1"}));
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<BomParams>(), &["component", "format", "group_by", "group_children", "levels", "path"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let file = std::env::temp_dir().join(format!("solidcraft-bom-schema-{}.csv", std::process::id()));
    let file = file.to_string_lossy();
    assert_strict_keeps_todays_errors(&[
        ("assembly.bom", json!({"component": true})),
        ("assembly.bom", json!({"component": "Nope"})),
        ("assembly.bom", json!({"levels": "deep"})),
        ("assembly.bom", json!({"levels": 1})),
        ("assembly.bom", json!({"group_by": "size"})),
        ("assembly.bom", json!({"group_by": ["size"]})),
        ("assembly.bom", json!({"group_by": ["part_number", "definition"]})),
        ("assembly.bom", json!({"group_by": [5]})),
        ("assembly.bom", json!({"group_by": 5})),
        ("assembly.bom", json!({"group_children": "yes"})),
        ("assembly.bom", json!({"path": file.replace(".csv", ".txt")})),
        ("assembly.bom", json!({"path": file, "format": "xml"})),
    ]);
}

/// #11075: a bill of materials replies as its output schema says, in Warn without warnings for
/// good calls and with them for a misspelt key: flat and top, grouped by definition, with the
/// hinge keeping its pin as children, and written to a JSON and a CSV file.
#[test]
fn a_bill_of_materials_replies_with_its_schema() {
    let schema = output_schema_for::<Bom>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    assembly(&mut s);
    let r = run(&mut s, "assembly.bom", json!({}));
    matches(&r, false);
    assert!(r.get("path").is_none() && r["rows"].as_array().is_some_and(|rows| rows.iter().all(|row| row.get("children").is_none())), "{r}");
    matches(&run(&mut s, "assembly.bom", json!({"component": 0, "levels": "top", "group_by": "definition"})), false);
    let r = run(&mut s, "assembly.bom", json!({"levels": "flat", "group_by": ["part_number"], "group_children": true}));
    matches(&r, false);
    let hinge = r["rows"].as_array().and_then(|rows| rows.iter().find(|row| row["part_number"] == "HG-1")).cloned().unwrap_or_default();
    assert_eq!((hinge["children"][0]["part_number"].as_str(), hinge["children"][0]["item"].is_string()), (Some("PN-040"), true), "{r}");
    let dir = std::env::temp_dir();
    for (name, format) in [("json", Value::Null), ("csv", json!("CSV"))] {
        let path = dir.join(format!("solidcraft-bom-schema-{}.{name}", std::process::id())).to_string_lossy().into_owned();
        let r = run(&mut s, "assembly.bom", json!({"path": path, "format": format}));
        matches(&r, false);
        assert_eq!(r["format"], name, "{r}");
        let _ = std::fs::remove_file(&path);
    }
    matches(&run(&mut s, "assembly.bom", json!({"level": "top"})), true);
}
