use serde_json::{Value, json};

use super::NoteParams;
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<NoteParams>(), &["count", "decimals", "edge", "feature", "language", "units"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("drawing.chamfer_note", json!({})),
        ("drawing.chamfer_note", json!({"feature": 0, "edge": "e"})),
        ("drawing.chamfer_note", json!({"feature": "Chamfer1", "edge": "e"})),
        ("drawing.chamfer_note", json!({"feature": 1})),
        ("drawing.chamfer_note", json!({"feature": 1, "edge": 5})),
        ("drawing.chamfer_note", json!({"feature": 1, "edge": "e", "decimals": 7})),
        ("drawing.chamfer_note", json!({"feature": 1, "edge": "e", "count": 1})),
    ]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<super::NoteResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10}));
    let mut replies = Vec::new();
    for chamfer in [json!({"distance": 2}), json!({"distance": 2, "angle": "30 deg"}), json!({"distance": 2, "distance2": 3})] {
        let mut p = json!({"edges": [[20, 0, 10]]});
        if let (Some(m), Some(e)) = (p.as_object_mut(), chamfer.as_object()) {
            m.extend(e.clone());
        }
        let feature = run(&mut s, "solid.chamfer", p)["feature"].clone();
        let id = feature.as_u64().unwrap_or_default();
        let edge = s.doc.feature(id).and_then(|f| f.edge_names.first().cloned()).unwrap_or_default();
        replies.push(run(&mut s, "drawing.chamfer_note", json!({"feature": feature, "edge": edge, "units": "in", "count": true})));
        replies.push(run(
            &mut s,
            "drawing.chamfer_note",
            json!({"feature": feature, "edge": edge, "units": null, "decimals": null, "language": null}),
        ));
        run(&mut s, "timeline.suppress", json!({"feature": id, "suppressed": true}));
        let suppressed = run(&mut s, "drawing.chamfer_note", json!({"feature": feature, "edge": edge}));
        assert_eq!(suppressed["status"], "suppressed", "{suppressed}");
        replies.push(suppressed);
        run(&mut s, "timeline.delete", json!({"feature": id}));
        let lost = run(&mut s, "drawing.chamfer_note", json!({"feature": feature, "edge": edge}));
        assert_eq!(lost["status"], "lost", "{lost}");
        replies.push(lost);
    }
    for r in &replies {
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    }
    assert_eq!(replies.iter().filter(|r| r["status"] == "resolved").count(), 6);
}
