#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};

use super::{
    LibraryAddParams, LibraryEditParams, LibraryEntryResult, LibraryListParams, LibraryListResult, LibraryRemoveParams, LibraryRemoveResult,
    LibraryReplaceParams, LibraryReplaceResult, ProfilePointsParams, ProfilePointsResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{input_schema_for, output_schema_for};

const ENTRY_KEYS: &[&str] =
    &["category", "default_point", "description", "material", "points", "profile", "profile_sketch", "profiles", "stock_length"];

fn keys(own: &[&'static str]) -> Vec<&'static str> {
    let mut all: Vec<&str> = ENTRY_KEYS.iter().copied().chain(own.iter().copied()).collect();
    all.sort_unstable();
    all
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<LibraryAddParams>(), &keys(&["name"]));
    assert_keys(input_schema_for::<LibraryEditParams>(), &keys(&["id", "name"]));
    assert_keys(input_schema_for::<LibraryListParams>(), &[]);
    assert_keys(input_schema_for::<LibraryRemoveParams>(), &["id"]);
    assert_keys(input_schema_for::<LibraryReplaceParams>(), &["from", "to"]);
    assert_keys(input_schema_for::<ProfilePointsParams>(), &["library", "profile"]);
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// Every reply conforms to the output schema the command publishes.
#[test]
fn replies_conform_to_their_output_schemas() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [500, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3});
    let check = |schema: schemars::Schema, v: &Value| {
        let root = schema.as_value();
        assert!(conforms(root, root, v), "{v} doesn't match {root}");
    };
    let added = run(
        &mut s,
        "frame.library.add",
        json!({"name": "SHS 40", "profile": tube, "points": [{"name": "bolt", "at": [0, 10]}], "stock_length": "6 m"}),
    );
    check(output_schema_for::<LibraryEntryResult>(), &added);
    run(&mut s, "frame.library.add", json!({"name": "SHS 50", "profile": {"kind": "rect_tube", "width": 50, "height": 50, "wall": 3}}));
    run(&mut s, "frame.member", json!({"sketch": "Frame", "library": "SHS 40"}));
    check(output_schema_for::<LibraryEntryResult>(), &run(&mut s, "frame.library.edit", json!({"id": "SHS 40", "material": "Steel"})));
    check(output_schema_for::<LibraryListResult>(), &run(&mut s, "frame.library.list", json!({})));
    check(output_schema_for::<ProfilePointsResult>(), &run(&mut s, "frame.profile.points", json!({"library": "SHS 40"})));
    check(output_schema_for::<LibraryReplaceResult>(), &run(&mut s, "frame.library.replace", json!({"from": "SHS 40", "to": "SHS 50"})));
    check(output_schema_for::<LibraryRemoveResult>(), &run(&mut s, "frame.library.remove", json!({"id": "SHS 40"})));
}
