use serde_json::{Value, json};

use super::{SelectionGetParams, SelectionGetResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box with its body selected, and its first feature selected too.
fn box_selected(s: &mut Session) {
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    let name = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    run(s, "select.set", json!({"items": [{"type": "body", "name": name}, {"type": "feature", "id": 1}]}));
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SelectionGetParams>(), &["detail"]);
}

/// A call that fails today gets the same message in strict.
#[test]
fn strict_keeps_todays_errors() {
    for p in [json!({"detail": "yes"}), json!({"detail": 1}), json!({"detail": []}), json!({"detail": {}})] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            s.execute("selection.get", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
    // `null` is the default, in every mode.
    for level in ParamCheck::ALL {
        let mut s = Session { param_check: level, ..Session::default() };
        assert!(s.execute("selection.get", &json!({"detail": null})).is_ok());
    }
}

/// #15900: the reply as the output schema says, in Warn without warnings for good calls and with
/// them for a misspelt key; with nothing selected, with items resolved, and without detail.
#[test]
fn selection_get_replies_with_its_schema() {
    let schema = output_schema_for::<SelectionGetResult>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    matches(&run(&mut s, "selection.get", json!({})), false);
    box_selected(&mut s);
    let r = run(&mut s, "selection.get", json!({}));
    matches(&r, false);
    assert_eq!(r["count"], 2, "{r}");
    assert_eq!(r["items"][0]["kind"], "body", "{r}");
    assert!(r["items"][0]["volume_mm3"].is_number() || r["items"][0]["feature"].is_object(), "detail resolved the body: {r}");
    let r = run(&mut s, "selection.get", json!({"detail": false}));
    matches(&r, false);
    assert!(r["items"][0].get("feature").is_none(), "no detail, no resolved fields: {r}");
    matches(&run(&mut s, "selection.get", json!({"detial": false})), true);
}
