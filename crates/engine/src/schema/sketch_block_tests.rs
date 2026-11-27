use serde_json::{Value, json};

use super::{
    BlockCloseParams, BlockCloseResult, BlockEditParams, BlockOpenParams, BlockOpenResult, BlockParams, BlockResult, ExplodeBlockParams,
    ExplodeBlockResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A sketch being edited with a rectangle l1…l4 and a circle c1.
fn sketch_with_profile(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Profile"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(s, "sketch.circle.center", json!({"center": [20, 10], "radius": 5}));
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<BlockParams>(), &["curves", "name", "origin", "scale", "show_constraints", "sketch"]);
    assert_keys(input_schema_for::<ExplodeBlockParams>(), &["block", "sketch"]);
    assert_keys(input_schema_for::<BlockEditParams>(), &["add", "block", "name", "remove", "scale", "show_constraints", "sketch"]);
    assert_keys(input_schema_for::<BlockOpenParams>(), &["block", "sketch"]);
    assert_keys(input_schema_for::<BlockCloseParams>(), &["block", "sketch"]);
}

/// The curves of a new block, and the block every other command but Close Block acts on, are
/// required; Close Block finds the open block by itself.
#[test]
fn the_schemas_name_what_each_command_requires() {
    let required = |v: Value| v["required"].clone();
    assert_eq!(required(input_schema_for::<BlockParams>().as_value().clone()), json!(["curves"]));
    assert_eq!(required(input_schema_for::<ExplodeBlockParams>().as_value().clone()), json!(["block"]));
    assert_eq!(required(input_schema_for::<BlockEditParams>().as_value().clone()), json!(["block"]));
    assert_eq!(required(input_schema_for::<BlockOpenParams>().as_value().clone()), json!(["block"]));
    assert_eq!(required(input_schema_for::<BlockCloseParams>().as_value().clone()), Value::Null);
}

/// A call that fails today gets the same message in strict.
#[test]
fn strict_keeps_todays_errors() {
    for (id, p) in [
        ("sketch.block", json!({"curves": ["l1"], "scale": "huge"})),
        ("sketch.block", json!({"curves": ["l1"], "scale": 3})),
        ("sketch.block", json!({"curves": 5})),
        ("sketch.block", json!({"curves": [1]})),
        ("sketch.block", json!({"curves": ["nope"]})),
        ("sketch.block", json!({"curves": ["l1"], "origin": 4})),
        ("sketch.block", json!({"curves": ["l1"], "origin": "nope"})),
        ("sketch.block", json!({"curves": ["l1"], "name": []})),
        ("sketch.explode_block", json!({"block": "blk9"})),
        ("sketch.explode_block", json!({"block": 9})),
        ("sketch.block_edit", json!({"block": "blk9"})),
        ("sketch.block_edit", json!({"block": "blk9", "scale": "big"})),
        ("sketch.block_edit", json!({"block": "blk9", "add": 3})),
        ("sketch.block_edit", json!({"block": "blk9", "show_constraints": true})),
        ("sketch.block_open", json!({"block": "blk9"})),
        ("sketch.block_open", json!({"block": 9})),
        ("sketch.block_open", json!({})),
        ("sketch.block_close", json!({})),
        ("sketch.block_close", json!({"block": "blk9"})),
        ("sketch.block_close", json!({"block": 9})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            sketch_with_profile(&mut s);
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// The replies of Make Block, Edit Block and Explode Block match their schemas, in Warn without
/// warnings for good calls.
#[test]
fn block_commands_reply_with_their_schemas() {
    let matches = |schema: schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    sketch_with_profile(&mut s);
    let made = run(&mut s, "sketch.block", json!({"curves": ["l1", "l2", "l3", "l4", "c1"], "scale": "free", "name": "Plate"}));
    matches(output_schema_for::<BlockResult>(), &made, false);
    let edited = run(&mut s, "sketch.block_edit", json!({"block": made["block"], "scale": "fixed", "remove": ["c1"]}));
    matches(output_schema_for::<BlockResult>(), &edited, false);
    let shown = run(&mut s, "sketch.block_edit", json!({"block": made["block"], "show_constraints": true}));
    matches(output_schema_for::<BlockResult>(), &shown, false);
    let opened = run(&mut s, "sketch.block_open", json!({"block": made["block"]}));
    matches(output_schema_for::<BlockOpenResult>(), &opened, false);
    let closed = run(&mut s, "sketch.block_close", json!({}));
    matches(output_schema_for::<BlockCloseResult>(), &closed, false);
    let exploded = run(&mut s, "sketch.explode_block", json!({"block": made["block"]}));
    matches(output_schema_for::<ExplodeBlockResult>(), &exploded, false);
    let again = run(&mut s, "sketch.block", json!({"curves": "c1", "sketch": "Profile", "colour": "red"}));
    matches(output_schema_for::<BlockResult>(), &again, true);
}
