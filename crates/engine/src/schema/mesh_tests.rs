use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::MeshConvertParams;
use crate::Sel;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};
use crate::tests::mesh_convert::imported_box;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<MeshConvertParams>(), &["body", "name", "participants", "tolerance"]);
}

/// The handler finds the body before it reads the tolerance, so on a design with no mesh body
/// each of these fails on the body, the same way in strict. A blank or `null` body means the
/// selected one, and nothing is selected.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("mesh.convert", json!({})),
        ("mesh.convert", json!({"body": null})),
        ("mesh.convert", json!({"body": " "})),
        ("mesh.convert", json!({"body": "Nope"})),
        ("mesh.convert", json!({"body": "Nope", "tolerance": "nonsense"})),
        ("mesh.convert", json!({"body": "Nope", "tolerance": 0.01, "name": "Rebuilt", "participants": ["*"]})),
    ]);
}

/// The error a call gets on the imported box with a solid box `Solid` beside it, at `level`.
fn conversion_error(level: ParamCheck, p: &Value) -> Option<String> {
    let mut s = imported_box();
    s.param_check = level;
    let solid = json!({"length": 5, "width": 5, "height": 5, "corner": [50, 0, 0], "body_name": "Solid"});
    s.execute("solid.box", &solid).unwrap_or_else(|e| panic!("{e}"));
    s.execute("mesh.convert", p).err().map(|e| e.to_string())
}

/// On a design with a mesh body, what the contract leaves to the handler and the kernel fails the
/// same way in strict: a body that isn't a mesh, a tolerance that doesn't evaluate or isn't a
/// length, and one the kernel refuses (outside 0.000001 mm to 1 mm).
#[test]
fn strict_keeps_the_conversions_own_errors() {
    for p in [
        json!({"body": "Solid"}),
        json!({"body": "Mesh", "tolerance": "nonsense"}),
        json!({"body": "Mesh", "tolerance": "30 deg"}),
        json!({"body": "Mesh", "tolerance": 5}),
        json!({"body": "Mesh", "tolerance": "0 mm"}),
        json!({"body": "Mesh", "tolerance": "-1"}),
    ] {
        let off = conversion_error(ParamCheck::Off, &p);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(conversion_error(ParamCheck::Strict, &p), off, "{p}");
    }
}

/// #13006: Convert Mesh's contract takes what its input schema allows: each call's check finds no
/// issue exactly when the call matches the schema.
#[test]
fn the_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<MeshConvertParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({}), true),
        (json!({"body": "Mesh"}), true),
        (json!({"body": "Mesh", "tolerance": "0.01 mm", "name": "Rebuilt", "participants": ["Mesh"]}), true),
        (json!({"body": "", "tolerance": 0.02, "participants": "*"}), true),
        (json!({"body": null, "tolerance": null, "name": null, "participants": null}), true),
        (json!({"body": 5}), false),
        (json!({"body": ["Mesh"]}), false),
        (json!({"tolerance": ""}), false),
        (json!({"tolerance": true}), false),
        (json!({"tolerance": 2e9}), false),
        (json!({"name": 5}), false),
        (json!({"participants": [1]}), false),
        (json!({"body_name": "Solid"}), false),
        (json!({"bodies": ["Mesh"]}), false),
    ] {
        let issues = crate::schema::issues("mesh.convert", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// #13006: Convert Mesh replies with `FeatureResult`, with the keys it gave before, when it names
/// the body and when it converts the selected one (no `body`, or a blank one). The solid takes the
/// mesh body's place under its name, as a body the new feature makes, so the reply lists it as
/// created (and not as changed or removed). In Warn, without warnings, so its contract takes each
/// call.
#[test]
fn convert_mesh_replies_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let before = BTreeSet::from(["feature", "name", "bodies", "created_bodies", "changed_bodies", "removed_bodies", "warning", "ms"]);
    for p in [
        json!({"body": "Mesh"}),
        json!({}),
        json!({"body": "", "tolerance": "0.01 mm", "name": "Rebuilt"}),
        json!({"body": "Mesh", "tolerance": 0.02, "participants": ["Mesh"]}),
    ] {
        let mut s = imported_box();
        s.param_check = ParamCheck::Warn;
        s.selection = vec![Sel::Body { name: "Mesh".into() }];
        let r = s.execute("mesh.convert", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        let keys: BTreeSet<&str> = r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, before, "{p}: {r}");
        let lists = (&r["created_bodies"], &r["changed_bodies"], &r["removed_bodies"]);
        assert_eq!(lists, (&json!(["Mesh"]), &json!([]), &json!([])), "{p}: {r}");
        assert!(s.model.state().body("Mesh").is_some_and(|b| !b.body.is_mesh()), "{p}: still a mesh");
    }
}
