use serde_json::{Value, json};

use super::{
    ChainParams, ChainResult, ConstructionParams, ConstructionResult, CreateParams, CreateResult, DeleteParams, DeleteResult, DimensionTextParams,
    DimensionTextResult, DragEndParams, DragEndResult, DragParams, DragResult, MovePointParams, MovePointResult, MovePointsParams, MovePointsResult,
    OptionsParams, OptionsReply, OptionsResult, SketchOptions, SolveParams, SolveResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

/// A session with a sketch being edited that holds a square (10 mm either side of the origin) and
/// the ids of its four sides.
fn editing(level: ParamCheck) -> (Session, Vec<String>) {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap_or_else(|e| panic!("{e}"));
    let r = s.execute("sketch.rectangle.center", &json!({"center": [0, 0], "corner": [10, 10]})).unwrap_or_else(|e| panic!("{e}"));
    let sides = r["created"]["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    (s, sides)
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn error(level: ParamCheck, id: &str, p: &Value) -> Option<String> {
    let (mut s, _) = editing(level);
    s.execute(id, p).err().map(|e| e.to_string())
}

/// A call the handler already refuses gets the same message in strict, with a sketch being edited.
/// (`assert_strict_keeps_todays_errors` runs after the sketch is finished, so a refusal that comes
/// from the design's side would read "no sketch is being edited" there.)
fn assert_strict_keeps_todays_errors(cases: &[(&str, Value)]) {
    for (id, p) in cases {
        let off = error(ParamCheck::Off, id, p);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict, id, p), off, "{id} {p}");
    }
}

fn messages(id: &str, p: Value) -> Vec<String> {
    issues(id, &p).into_iter().map(|i| i.message).collect()
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<CreateParams>(),
        &["activate_part", "face", "flip", "frame", "name", "offset", "origin", "plane", "project_edges", "x_dir", "y_dir"],
    );
    assert_keys(input_schema_for::<ConstructionParams>(), &["curves", "sketch", "value"]);
    assert_keys(input_schema_for::<DeleteParams>(), &["entities", "sketch"]);
    assert_keys(input_schema_for::<MovePointParams>(), &["point", "sketch", "to"]);
    assert_keys(input_schema_for::<MovePointsParams>(), &["moves", "sketch", "tolerance"]);
    assert_keys(input_schema_for::<DragParams>(), &["entity", "from", "live", "sketch", "to", "with"]);
    assert_keys(input_schema_for::<DragEndParams>(), &["constraint", "sketch"]);
    assert_keys(input_schema_for::<DimensionTextParams>(), &["at", "dimension", "reset", "sketch"]);
    assert_keys(
        input_schema_for::<OptionsParams>(),
        &[
            "grid",
            "show_constraints",
            "show_construction",
            "show_dimensions",
            "show_points",
            "show_profile",
            "show_projected",
            "sketch",
            "sketch_3d",
            "slice",
            "snap",
        ],
    );
    assert_keys(input_schema_for::<ChainParams>(), &["curve", "sketch"]);
    assert_keys(input_schema_for::<SolveParams>(), &["sketch"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.create", json!({"face": [0, 0, 0]})),
        ("sketch.create", json!({"origin": null})),
        ("sketch.create", json!({"x_dir": [1, 0, 0]})),
        ("sketch.create", json!({"y_dir": 3})),
        ("sketch.create", json!({"activate_part": "yes"})),
        ("sketch.create", json!({"frame": "local", "plane": {"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("sketch.create", json!({"frame": 1})),
        ("sketch.create", json!({"frame": "world"})),
        ("sketch.create", json!({"frame": "component", "plane": "XY"})),
        ("sketch.create", json!({"plane": true})),
        ("sketch.create", json!({"plane": null})),
        ("sketch.create", json!({"plane": [1]})),
        ("sketch.create", json!({"plane": 1.5})),
        ("sketch.create", json!({"plane": -1})),
        ("sketch.create", json!({"plane": "Nope"})),
        ("sketch.create", json!({"plane": {"x_dir": [1, 0, 0]}})),
        ("sketch.create", json!({"plane": {"y_dir": [1, 0, 0]}})),
        ("sketch.create", json!({"plane": {"x_dir": [1, 0, 0], "y_dir": [2, 0, 0]}})),
        ("sketch.create", json!({"plane": {"origin": "x", "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("sketch.create", json!({"plane": {"face": 5}})),
        ("sketch.create", json!({"plane": {"face": [0, 0, 0], "fixed": "yes"}})),
        ("sketch.create", json!({"plane": {"face": [0, 0, 0], "body": ""}})),
        ("sketch.create", json!({"plane": {"face": [0, 0, 0], "body": "B"}})),
        ("sketch.create", json!({"plane": {"face": [0, 0, 0], "index": -1}})),
        ("sketch.create", json!({"plane": {"face": [0, 0, 0], "path": [0]}})),
        ("sketch.create", json!({"plane": {"face": [0, 0, 0], "path": "x"}})),
        ("sketch.create", json!({"flip": "z"})),
        ("sketch.create", json!({"flip": 1})),
        ("sketch.construction", json!({})),
        ("sketch.construction", json!({"curves": []})),
        ("sketch.construction", json!({"curves": null})),
        ("sketch.construction", json!({"curves": [1]})),
        ("sketch.construction", json!({"curves": ["l1", 2]})),
        ("sketch.construction", json!({"curves": 5})),
        ("sketch.construction", json!({"curves": ["nope"]})),
        ("sketch.construction", json!({"curves": ["l1"], "sketch": true})),
        ("sketch.construction", json!({"curves": ["l1"], "sketch": -1})),
        ("sketch.construction", json!({"curves": ["l1"], "sketch": 99})),
        ("sketch.delete", json!({})),
        ("sketch.delete", json!({"entities": []})),
        ("sketch.delete", json!({"entities": {"a": 1}})),
        ("sketch.delete", json!({"entities": ["nope"]})),
        ("sketch.delete", json!({"entities": ["origin"]})),
        ("sketch.move_point", json!({})),
        ("sketch.move_point", json!({"to": [1, 2]})),
        ("sketch.move_point", json!({"to": [1, 2], "point": 5})),
        ("sketch.move_point", json!({"to": [1, 2], "point": null})),
        ("sketch.move_point", json!({"to": "x", "point": "origin"})),
        ("sketch.move_point", json!({"to": null, "point": "origin"})),
        ("sketch.move_point", json!({"to": [1], "point": "origin"})),
        ("sketch.move_point", json!({"to": [1, 2], "point": "nope"})),
        ("sketch.move_points", json!({})),
        ("sketch.move_points", json!({"moves": []})),
        ("sketch.move_points", json!({"moves": null})),
        ("sketch.move_points", json!({"moves": "origin"})),
        ("sketch.move_points", json!({"moves": [{"point": "origin"}]})),
        ("sketch.move_points", json!({"moves": [{"to": "x", "point": "origin"}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1], "point": "origin"}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2]}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": 5}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": null}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "near": null}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "near": [1]}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "near": [1, 2], "point": 5}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "nope"}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "origin"}], "tolerance": 0})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "origin"}], "tolerance": -1})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "origin"}], "tolerance": "a bit"})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "origin"}], "sketch": 99})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "origin"}], "sketch": "nope"})),
        ("sketch.drag", json!({})),
        ("sketch.drag", json!({"to": [1, 2]})),
        ("sketch.drag", json!({"to": [1, 2], "entity": 3})),
        ("sketch.drag", json!({"to": [1, 2], "entity": "nope"})),
        ("sketch.drag", json!({"to": 3, "entity": "l1"})),
        ("sketch.drag", json!({"to": [1, 2], "from": "a", "entity": "l1"})),
        ("sketch.drag", json!({"to": [1, 2], "from": [1], "entity": "l1"})),
        ("sketch.drag", json!({"to": [1, 2], "entity": "l1", "with": "l2"})),
        ("sketch.drag", json!({"to": [1, 2], "entity": "l1", "with": [1]})),
        ("sketch.drag", json!({"to": [1, 2], "entity": "l1", "with": ["nope"]})),
        ("sketch.drag_end", json!({"constraint": {"command": "solid.box"}})),
        ("sketch.drag_end", json!({"constraint": {"command": 5}})),
        ("sketch.drag_end", json!({"constraint": {}})),
        ("sketch.drag_end", json!({"constraint": 5})),
        ("sketch.drag_end", json!({"constraint": "sketch.constraint.fix"})),
        ("sketch.drag_end", json!({"sketch": true})),
        ("sketch.dimension_text", json!({})),
        ("sketch.dimension_text", json!({"at": [1, 2]})),
        ("sketch.dimension_text", json!({"dimension": 5, "at": [1, 2]})),
        ("sketch.dimension_text", json!({"dimension": null, "reset": true})),
        ("sketch.dimension_text", json!({"dimension": "d1"})),
        ("sketch.dimension_text", json!({"dimension": "d1", "at": "x"})),
        ("sketch.dimension_text", json!({"dimension": "d1", "at": null})),
        ("sketch.dimension_text", json!({"dimension": "nope", "at": [1, 2]})),
        ("sketch.options", json!({"sketch": true})),
        ("sketch.options", json!({"sketch": 99})),
        ("sketch.options", json!({"show_points": false, "sketch": "nope"})),
        ("sketch.chain", json!({})),
        ("sketch.chain", json!({"curve": 5})),
        ("sketch.chain", json!({"curve": null})),
        ("sketch.chain", json!({"curve": "nope"})),
        ("sketch.chain", json!({"curve": "l1", "sketch": [1]})),
        ("sketch.solve", json!({"sketch": true})),
        ("sketch.solve", json!({"sketch": 99})),
        ("sketch.solve", json!({"sketch": "nope"})),
    ]);
    // What only the design can tell keeps its message too: two points as close to `near`, none
    // within `tolerance`, and more moves than the sketch has points. The last is not a length the
    // schema could state: it is the number of points of the sketch being edited.
    assert_strict_keeps_todays_errors(&[
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "near": [0, -10]}]})),
        ("sketch.move_points", json!({"moves": [{"to": [1, 2], "near": [0, -10]}], "tolerance": 10.5})),
        ("sketch.move_points", json!({"moves": (0..9).map(|_| json!({"to": [1, 2], "point": "origin"})).collect::<Vec<_>>()})),
    ]);
}

/// A flip that is refused on a face gets the same message in strict.
#[test]
fn strict_keeps_the_flip_refusal_on_a_face() {
    let error = |level: ParamCheck| {
        let mut s = Session { param_check: level, ..Session::default() };
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10}));
        let at = json!({"face": [10, 10, 10]});
        for flip in [json!(true), json!("x"), json!("Y")] {
            let e = s.execute("sketch.create", &json!({"plane": at, "flip": flip}));
            assert!(e.is_err(), "{flip}");
        }
        s.execute("sketch.create", &json!({"plane": at, "flip": true})).err().map(|e| e.to_string())
    };
    let off = error(ParamCheck::Off);
    assert!(off.as_deref().is_some_and(|e| e.contains("a face already faces outward")), "{off:?}");
    assert_eq!(error(ParamCheck::Strict), off);
}

/// `conforms` doesn't read patterns, so the text of the two that say what a reader checks.
#[test]
fn the_patterns_are_pinned() {
    let schema = input_schema_for::<CreateParams>();
    assert_eq!(schema.as_value().pointer("/$defs/Flip/anyOf/1/pattern"), Some(&json!("^[xXyY]$")));
    let schema = input_schema_for::<DragEndParams>();
    assert_eq!(schema.as_value().pointer("/$defs/DragConstraint/properties/command/pattern"), Some(&json!("^sketch\\.constraint\\.")));
    for (flip, ok) in [("x", true), ("Y", true), ("z", false), ("xy", false), ("", false)] {
        assert_eq!(messages("sketch.create", json!({"flip": flip})).is_empty(), ok, "{flip}");
    }
}

/// Each key is read in the cases its handler reads it, so a call that works today gets no warning:
/// `project_edges` is read only for a picked face, `frame` and `reset` as the handlers do, `at`
/// only without `reset: true`, a constraint only when it is not `null`.
#[test]
fn the_types_read_each_key_as_the_handler_does() {
    // sketch.create: `project_edges` is read only on a face; the handler ignores it elsewhere.
    assert!(messages("sketch.create", json!({"plane": "XY", "project_edges": "yes"})).is_empty());
    assert!(messages("sketch.create", json!({"project_edges": [1]})).is_empty());
    assert_eq!(messages("sketch.create", json!({"plane": {"face": [0, 0, 1]}, "project_edges": "yes"})), ["`project_edges` must be true or false"]);
    assert!(messages("sketch.create", json!({"plane": {"face": [0, 0, 1]}, "project_edges": false, "activate_part": true})).is_empty());
    // `frame` is for an explicit plane only, and `null` reads as left out.
    let explicit = json!({"origin": [0, 0, 1], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]});
    assert!(messages("sketch.create", json!({"plane": explicit, "frame": "component"})).is_empty());
    assert!(messages("sketch.create", json!({"plane": explicit, "frame": null, "flip": "X"})).is_empty());
    assert!(messages("sketch.create", json!({"plane": "XZ", "frame": null, "flip": null, "offset": "w / 2", "name": "S"})).is_empty());
    assert_eq!(messages("sketch.create", json!({"plane": explicit, "frame": "local"})), ["`frame` must be \"world\" or \"component\""]);
    assert!(messages("sketch.create", json!({"plane": "XY", "frame": "world"}))[0].starts_with("`frame` is for an explicit plane"));
    // The keys that belong inside `plane` are refused with their own message, not as unknown keys.
    assert_eq!(messages("sketch.create", json!({"face": [0, 0, 1]})), [r#"`face` belongs inside `plane`: {"plane": {"face": [x, y, z]}}"#]);
    assert_eq!(messages("sketch.create", json!({"plane": "XY", "x_dir": null})).len(), 1);
    // A face is not flipped, but `flip: false` is fine.
    assert!(messages("sketch.create", json!({"plane": {"face": [0, 0, 1]}, "flip": false})).is_empty());
    assert_eq!(
        messages("sketch.create", json!({"plane": {"face": [0, 0, 1]}, "flip": true})),
        ["`flip` is for origin, construction and explicit planes; a face already faces outward"]
    );

    // sketch.dimension_text: `at` is read only without `reset: true`.
    assert!(messages("sketch.dimension_text", json!({"dimension": "d1", "reset": true, "at": "anywhere"})).is_empty());
    assert!(messages("sketch.dimension_text", json!({"dimension": "d1", "reset": true})).is_empty());
    assert_eq!(messages("sketch.dimension_text", json!({"dimension": "d1", "reset": false, "at": 1})), ["`at` must be a point [x, y]"]);
    assert_eq!(messages("sketch.dimension_text", json!({"dimension": "d1", "reset": "yes"})), ["`reset` must be true or false"]);
    assert!(messages("sketch.dimension_text", json!({"dimension": "d1", "at": {"x": 1, "y": 2}})).is_empty());

    // sketch.drag: `from` may be `null`; `with` too.
    assert!(messages("sketch.drag", json!({"entity": "l1", "to": [1, 2], "from": null, "with": null, "live": null})).is_empty());
    assert!(messages("sketch.drag", json!({"entity": "l1", "to": {"x": 1, "y": 2}, "from": [0, 0], "with": ["l2", "p3"], "live": false})).is_empty());
    assert_eq!(messages("sketch.drag", json!({"entity": "l1", "to": [1, 2], "live": 1})), ["`live` must be true or false"]);

    // sketch.move_points: every entry needs `to`; the point is `near` when that key is there, and
    // the handler never looks at `point` then.
    assert!(messages("sketch.move_points", json!({"moves": [{"point": "origin", "to": [0, 0]}]})).is_empty());
    assert!(
        messages("sketch.move_points", json!({"moves": [{"near": [1, 2], "to": {"x": 3, "y": 4}}], "tolerance": null, "sketch": null})).is_empty()
    );
    assert!(messages("sketch.move_points", json!({"moves": [{"near": [1, 2], "to": [3, 4], "point": 5}], "tolerance": 0.2})).is_empty());
    assert!(messages("sketch.move_points", json!({"moves": [{"point": "origin", "to": [0, 0], "bogus": 1}]})).is_empty());
    assert_eq!(messages("sketch.move_points", json!({"moves": []})), ["`moves` must be a list of {point | near, to}"]);
    assert_eq!(messages("sketch.move_points", json!({"moves": "origin"})), ["`moves` must be a list of {point | near, to}"]);
    assert_eq!(messages("sketch.move_points", json!({"moves": [{"point": "origin"}]})), ["moves[0]: `to` must be a point [x, y]"]);
    assert_eq!(messages("sketch.move_points", json!({"moves": [{"to": [1], "point": "origin"}]})), ["moves[0]: `to` must be a point [x, y]"]);
    assert_eq!(messages("sketch.move_points", json!({"moves": [{"to": [1, 2], "near": null}]})), ["moves[0]: `near` must be a point [x, y]"]);
    assert_eq!(messages("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": 5}]})), ["`point` must be a point reference"]);
    assert_eq!(
        messages("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "origin"}, {"to": [1, 2], "point": 5}]})),
        ["`point` must be a point reference"],
    );
    assert_eq!(messages("sketch.move_points", json!({"moves": [{"to": [1, 2]}], "tolerance": 0})), ["`tolerance` must be a positive length (mm)"]);
    assert_eq!(messages("sketch.move_points", json!({"moves": [{"to": [1, 2]}], "tolerance": 1e9})), ["`tolerance` must be a positive length (mm)"]);
    assert!(messages("sketch.move_points", json!({"moves": [{"to": [1, 2], "point": "origin"}], "tolerance": 999_999_999.0})).is_empty());

    // sketch.drag_end: a constraint only when it is not `null`; its params are the command's own.
    assert!(messages("sketch.drag_end", json!({"constraint": null, "sketch": null})).is_empty());
    assert!(messages("sketch.drag_end", json!({"constraint": {"command": "sketch.constraint.fix", "params": {"point": "origin"}}})).is_empty());
    assert!(messages("sketch.drag_end", json!({"constraint": {"command": "sketch.constraint.fix"}})).is_empty());
    assert_eq!(
        messages("sketch.drag_end", json!({"constraint": {"command": "sketch.line"}})),
        ["`constraint.command` must be a sketch.constraint.* command"]
    );

    // Lists of ids: one text or a list of text, as `string_list` reads them.
    assert!(messages("sketch.construction", json!({"curves": "l1", "value": null})).is_empty());
    assert!(messages("sketch.delete", json!({"entities": ["l1", "p2", "d1"]})).is_empty());
    assert_eq!(messages("sketch.construction", json!({"curves": ["l1", 2]})), ["`curves` must list names; the entry 2 is 2"]);
    assert_eq!(messages("sketch.construction", json!({"curves": ["l1"], "value": "yes"})), ["`value` must be true or false"]);
    assert_eq!(messages("sketch.delete", json!({"entities": true})), ["`entities` must be a list of names; it is true"]);

    // sketch.options: the options are flags; anything else is named.
    assert!(messages("sketch.options", json!({"show_points": false, "grid": true, "slice": null, "sketch": 3})).is_empty());
    assert_eq!(messages("sketch.options", json!({"snap": "on"})), ["`snap` must be true or false"]);
    assert_eq!(messages("sketch.options", json!({"show_profil": true})), ["unknown key `show_profil`; did you mean `show_profile`?"]);
}

/// What the types accept, the handlers accept in Off: each call below runs there and has no issue.
#[test]
fn what_the_types_accept_the_handlers_run() {
    let (mut s, sides) = editing(ParamCheck::Off);
    let bottom = sides[0].clone();
    for (id, p) in [
        ("sketch.solve", json!({"sketch": null})),
        ("sketch.chain", json!({"curve": bottom, "sketch": null})),
        ("sketch.options", json!({"show_points": null, "grid": null})),
        ("sketch.construction", json!({"curves": bottom, "value": null})),
        ("sketch.move_point", json!({"point": "origin", "to": {"x": 0, "y": 0}})),
        ("sketch.move_points", json!({"moves": [{"point": "origin", "to": [0, 0]}, {"near": [10, -10], "to": [10, -10]}], "tolerance": null})),
        ("sketch.drag", json!({"entity": bottom, "to": [0, -10], "from": null, "with": null, "live": null})),
        ("sketch.drag_end", json!({"constraint": null})),
        ("sketch.create", json!({"plane": "XY", "frame": null, "flip": false, "project_edges": "ignored"})),
    ] {
        assert!(messages(id, p.clone()).is_empty(), "{id} {p}");
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
}

/// The contracts say what the handlers need: the keys a handler refuses without are required in
/// the schemas, though the types leave them to the handlers' messages.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<CreateParams>(),
        &[
            (json!({}), true),
            (json!({"plane": "XZ", "offset": 5, "flip": "x", "name": "S"}), true),
            (json!({"plane": 3, "flip": true}), true),
            (
                json!({"plane": {"face": [1, 2, 3], "body": "B", "index": 2, "path": [1], "fixed": true}, "project_edges": false, "activate_part": true}),
                true,
            ),
            (json!({"plane": {"origin": [0, 0, 1], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "frame": "component"}), true),
            (json!({"plane": "XY", "frame": "nowhere"}), false),
            (json!({"plane": 1.5}), false),
            (json!({"plane": null}), false),
            (json!({"face": [1, 2, 3]}), false),
            (json!({"x_dir": null}), false),
        ],
    );
    takes(
        input_schema_for::<ConstructionParams>(),
        &[
            (json!({"curves": ["l1", "l2"]}), true),
            (json!({"curves": "l1", "value": false, "sketch": "S"}), true),
            (json!({}), false),
            (json!({"curves": []}), false),
            (json!({"curves": [1]}), false),
        ],
    );
    takes(
        input_schema_for::<DeleteParams>(),
        &[
            (json!({"entities": ["l1", "d1"]}), true),
            (json!({"entities": "p3", "sketch": 4}), true),
            (json!({}), false),
            (json!({"entities": []}), false),
        ],
    );
    takes(
        input_schema_for::<MovePointParams>(),
        &[
            (json!({"point": "l1.end", "to": [1, 2]}), true),
            (json!({"point": "p1", "to": {"x": 1, "y": 2}}), true),
            (json!({"to": [1, 2]}), false),
            (json!({"point": "p1"}), false),
            (json!({"point": 3, "to": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<MovePointsParams>(),
        &[
            (json!({"moves": [{"point": "p1", "to": [1, 2]}]}), true),
            (json!({"moves": [{"near": [1, 2], "to": {"x": 3, "y": 4}}], "tolerance": 0.2, "sketch": "S"}), true),
            (json!({"moves": [{"point": "p1", "to": [1, 2]}], "tolerance": null}), true),
            (json!({"moves": [{"point": "p1", "to": [1, 2], "near": [1, 2]}]}), true),
            (json!({}), false),
            (json!({"moves": []}), false),
            (json!({"moves": [{"point": "p1"}]}), false),
            (json!({"moves": [{"to": [1, 2]}]}), false),
            (json!({"moves": [{"point": "p1", "to": [1, 2]}], "tolerance": 0}), false),
            (json!({"moves": [{"point": "p1", "to": [1, 2]}], "tolerance": -0.5}), false),
        ],
    );
    takes(
        input_schema_for::<DragParams>(),
        &[
            (json!({"entity": "l1", "to": [1, 2]}), true),
            (json!({"entity": "c1", "from": [0, 0], "to": [1, 2], "with": ["p1"], "live": true, "sketch": "S"}), true),
            (json!({"entity": "c1", "from": null, "to": [1, 2], "with": null}), true),
            (json!({"entity": "l1"}), false),
            (json!({"to": [1, 2]}), false),
            (json!({"entity": "l1", "to": [1, 2], "with": "p1"}), false),
        ],
    );
    takes(
        input_schema_for::<DragEndParams>(),
        &[
            (json!({}), true),
            (json!({"constraint": {"command": "sketch.constraint.fix", "params": {"point": "p1"}}}), true),
            (json!({"constraint": {"params": {}}}), false),
            (json!({"constraint": 5}), false),
        ],
    );
    takes(
        input_schema_for::<DimensionTextParams>(),
        &[
            (json!({"dimension": "d1", "at": [1, 2]}), true),
            (json!({"dimension": "d1", "reset": true}), true),
            (json!({"dimension": "d1", "reset": true, "at": [1, 2]}), true),
            (json!({"dimension": "d1", "reset": false}), false),
            (json!({"dimension": "d1"}), false),
            (json!({"at": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<OptionsParams>(),
        &[(json!({}), true), (json!({"show_points": false, "sketch_3d": true, "sketch": "S"}), true), (json!({"grid": "yes"}), false)],
    );
    takes(
        input_schema_for::<ChainParams>(),
        &[(json!({"curve": "l1"}), true), (json!({"curve": "l1", "sketch": 2}), true), (json!({}), false), (json!({"curve": 1}), false)],
    );
    takes(input_schema_for::<SolveParams>(), &[(json!({}), true), (json!({"sketch": "S"}), true), (json!({"sketch": true}), false)]);
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// Every form of Create Sketch, in Warn: a reply that matches the output schema, no warning.
#[test]
fn create_replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let made = |s: &mut Session, p: Value| {
        let r = run(s, "sketch.create", p);
        matches(output_schema_for::<CreateResult>(), "sketch.create", &r);
        run(s, "sketch.finish", json!({}));
        r
    };
    let first = made(&mut s, json!({"plane": "XY"}));
    assert_eq!(first["attachment"], "fixed", "{first}");
    assert!(first["projected"].is_null() && first.get("frame").is_none(), "{first}");
    made(&mut s, json!({}));
    made(&mut s, json!({"plane": "xz", "offset": 5, "name": "Wall", "flip": "x"}));
    made(&mut s, json!({"plane": "YZ", "offset": "10 / 2", "flip": true, "activate_part": false}));
    made(&mut s, json!({"plane": "XY", "flip": false, "frame": null, "project_edges": 7}));
    let explicit = json!({"origin": [0, 0, 5], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]});
    let world = made(&mut s, json!({"plane": explicit}));
    assert_eq!(world["frame"], "world", "{world}");
    let own = made(&mut s, json!({"plane": explicit, "frame": "component", "flip": "Y"}));
    assert_eq!(own["frame"], "component", "{own}");
    // A construction plane by name and by id.
    let plane = run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 12, "name": "Mid"}));
    made(&mut s, json!({"plane": "Mid"}));
    made(&mut s, json!({"plane": plane["feature"]}));
    // A picked face, with its edges projected or not, and in another component.
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
    let top = made(&mut s, json!({"plane": {"face": [10, 10, 10]}}));
    assert_eq!(top["attachment"], "exact_face", "{top}");
    assert!(top["projected"].is_string(), "{top}");
    let bare = made(&mut s, json!({"plane": {"face": [10, 10, 10], "fixed": true}, "project_edges": false, "flip": false}));
    assert!(bare["projected"].is_null(), "{bare}");
    // The exact pick: the face's index is the one of the six that holds the point.
    let exact = (0..6).find_map(|index| {
        let pick = json!({"plane": {"face": [10, 10, 10], "body": "Block", "index": index}, "activate_part": true, "name": "On face"});
        s.execute("sketch.create", &pick).ok().map(|r| (pick, r))
    });
    let (_, exact) = exact.unwrap_or_else(|| panic!("no face of the box holds the point"));
    matches(output_schema_for::<CreateResult>(), "sketch.create", &exact);
    assert_eq!(exact["attachment"], "exact_face", "{exact}");
}

/// The reply's `note` (a world plane into a moved component) and `activated` (a face of another
/// component's body), in Warn: both match the output schema and carry no warning.
#[test]
fn create_note_and_activated_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let part = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": part["occurrence"], "translate": [10, 0, 0]}));
    let r = run(&mut s, "sketch.create", json!({"plane": {"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    assert!(r["note"].as_str().is_some_and(|n| n.contains("world coordinates")) && r["frame"] == "world", "{r}");
    matches(output_schema_for::<CreateResult>(), "sketch.create", &r);
    run(&mut s, "sketch.finish", json!({}));
    // Another component with a body, then the root active: a face pick with `activate_part`.
    run(&mut s, "component.create", json!({"name": "Other", "grounded": true}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let r = (0..6)
        .find_map(|index| {
            let pick = json!({"plane": {"face": [10, 10, 10], "body": "Block", "index": index}, "activate_part": true});
            s.execute("sketch.create", &pick).ok()
        })
        .unwrap_or_else(|| panic!("no face of the box holds the point"));
    assert!(r["activated"]["component"].is_u64() && r["activated"]["path"].is_array(), "{r}");
    matches(output_schema_for::<CreateResult>(), "sketch.create", &r);
}

/// The editing commands, in Warn, on a sketch being edited: each reply matches its output schema
/// and carries no warning.
#[test]
fn editing_replies_match_the_output_schema() {
    let (mut s, sides) = editing(ParamCheck::Warn);
    let (bottom, right) = (sides[0].clone(), sides[1].clone());
    let sketch = s.active_sketch.unwrap_or_default();

    let r = run(&mut s, "sketch.solve", json!({}));
    assert!(r["status"] == "solved" && r["failing"] == json!([]), "{r}");
    matches(output_schema_for::<SolveResult>(), "sketch.solve", &r);
    matches(output_schema_for::<SolveResult>(), "sketch.solve", &run(&mut s, "sketch.solve", json!({"sketch": sketch})));

    let r = run(&mut s, "sketch.chain", json!({"curve": bottom}));
    assert!(r["curves"].as_array().is_some_and(|c| c.iter().any(|id| id == bottom.as_str())), "{r}");
    matches(output_schema_for::<ChainResult>(), "sketch.chain", &r);
    matches(output_schema_for::<ChainResult>(), "sketch.chain", &run(&mut s, "sketch.chain", json!({"curve": right, "sketch": sketch})));

    let r = run(&mut s, "sketch.options", json!({}));
    assert!(r["options"]["show_points"] == true && r.get("sketch").is_none(), "{r}");
    matches(output_schema_for::<OptionsResult>(), "sketch.options", &r);
    let r = run(&mut s, "sketch.options", json!({"show_points": false, "grid": false, "slice": true, "sketch": sketch}));
    assert!(r["options"]["show_points"] == false && r["options"]["slice"] == true && r["sketch"].is_object(), "{r}");
    matches(output_schema_for::<OptionsResult>(), "sketch.options", &r);
    let r = run(&mut s, "sketch.options", json!({"show_points": null, "snap": true}));
    matches(output_schema_for::<OptionsResult>(), "sketch.options", &r);

    let r = run(&mut s, "sketch.construction", json!({"curves": [bottom]}));
    assert!(r["construction"] == true && r["changed"] == 1, "{r}");
    matches(output_schema_for::<ConstructionResult>(), "sketch.construction", &r);
    let r = run(&mut s, "sketch.construction", json!({"curves": bottom, "value": false, "sketch": sketch}));
    assert!(r["construction"] == false && r["changed"] == 1, "{r}");
    matches(output_schema_for::<ConstructionResult>(), "sketch.construction", &r);

    let r = run(
        &mut s,
        "sketch.move_points",
        json!({"moves": [{"point": "origin", "to": [0, 0]}, {"near": [10, -10], "to": [10, -10]}], "tolerance": 0.5}),
    );
    let moved = r["moved"].as_array().cloned().unwrap_or_default();
    assert_eq!(moved.len(), 2, "{r}");
    assert_eq!(moved[0]["point"], json!("origin"), "{r}");
    assert!(moved[1]["point"].is_string(), "{r}");
    assert!(moved.iter().all(|m| m["to"].is_array() && m["at"].is_array() && m["reached"].is_boolean()), "{r}");
    assert!(r["sketch"].is_object(), "{r}");
    matches(output_schema_for::<MovePointsResult>(), "sketch.move_points", &r);
    let r = run(&mut s, "sketch.move_points", json!({"moves": [{"point": "origin", "to": [0, 0]}], "sketch": sketch}));
    matches(output_schema_for::<MovePointsResult>(), "sketch.move_points", &r);

    let r = run(&mut s, "sketch.move_point", json!({"point": format!("{bottom}.start"), "to": [-12, -10]}));
    assert!(r["sketch"].is_object(), "{r}");
    matches(output_schema_for::<MovePointResult>(), "sketch.move_point", &r);
    matches(
        output_schema_for::<MovePointResult>(),
        "sketch.move_point",
        &run(&mut s, "sketch.move_point", json!({"point": "origin", "to": {"x": 1, "y": 1}})),
    );

    let d = run(&mut s, "sketch.dimension", json!({"entities": [right]}));
    let param = d["param"].as_str().unwrap_or_default().to_string();
    let r = run(&mut s, "sketch.dimension_text", json!({"dimension": param, "at": [30, 12]}));
    assert!(r["dimension"].is_string(), "{r}");
    let dimension = r["dimension"].clone();
    matches(output_schema_for::<DimensionTextResult>(), "sketch.dimension_text", &r);
    let r = run(&mut s, "sketch.dimension_text", json!({"dimension": dimension, "reset": true, "at": "ignored"}));
    matches(output_schema_for::<DimensionTextResult>(), "sketch.dimension_text", &r);
    let r = run(&mut s, "sketch.dimension_text", json!({"dimension": param, "reset": false, "at": [1, 2], "sketch": sketch}));
    matches(output_schema_for::<DimensionTextResult>(), "sketch.dimension_text", &r);

    let r = run(&mut s, "sketch.drag", json!({"entity": right, "from": [10, 0], "to": [14, 0]}));
    assert!(r["reached"].is_number() && r["at"].is_array() && r["retry"] == false, "{r}");
    matches(output_schema_for::<DragResult>(), "sketch.drag", &r);
    let r = run(&mut s, "sketch.drag", json!({"entity": bottom, "to": [0, -10], "with": [right], "live": false, "sketch": sketch}));
    matches(output_schema_for::<DragResult>(), "sketch.drag", &r);

    let r = run(&mut s, "sketch.drag_end", json!({}));
    assert!(r["constraint"].is_null(), "{r}");
    matches(output_schema_for::<DragEndResult>(), "sketch.drag_end", &r);
    let fix = json!({"command": "sketch.constraint.fix", "params": {"entity": format!("{bottom}.start")}});
    let r = run(&mut s, "sketch.drag_end", json!({"constraint": fix, "sketch": sketch}));
    assert!(r["constraint"]["result"].is_object() && r["sketch"] == r["constraint"]["sketch"], "{r}");
    // The nested command is not warned about its own call by the outer one: its reply carries what
    // its own contract found, here nothing.
    matches(output_schema_for::<DragEndResult>(), "sketch.drag_end", &r);
    let r = run(&mut s, "sketch.drag_end", json!({"constraint": null}));
    matches(output_schema_for::<DragEndResult>(), "sketch.drag_end", &r);
    // A constraint that cannot be added is not kept; the drag still ends.
    let bad = json!({"command": "sketch.constraint.coincident", "params": {"a": "origin", "b": "origin"}});
    let r = run(&mut s, "sketch.drag_end", json!({"constraint": bad}));
    assert!(r["constraint"]["error"].is_string(), "{r}");
    matches(output_schema_for::<DragEndResult>(), "sketch.drag_end", &r);

    let r = run(&mut s, "sketch.delete", json!({"entities": [dimension, bottom]}));
    assert_eq!(r["deleted"], 2, "{r}");
    matches(output_schema_for::<DeleteResult>(), "sketch.delete", &r);
    let r = run(&mut s, "sketch.delete", json!({"entities": right, "sketch": sketch}));
    matches(output_schema_for::<DeleteResult>(), "sketch.delete", &r);
}

/// A misspelt key still runs in Warn and says so, with a reply that matches the output schema; in
/// Strict the call is refused with the suggestion.
#[test]
fn a_misspelt_key_is_reported() {
    let (mut s, sides) = editing(ParamCheck::Warn);
    let bottom = sides[0].clone();
    let cases = [
        ("sketch.solve", json!({"skech": 1}), "skech", "sketch"),
        ("sketch.chain", json!({"curve": bottom, "sketh": 1}), "sketh", "sketch"),
        ("sketch.construction", json!({"curves": [bottom], "valu": true}), "valu", "value"),
        ("sketch.move_point", json!({"point": "origin", "to": [1, 1], "too": [2, 2]}), "too", "to"),
        ("sketch.drag", json!({"entity": bottom, "to": [0, -9], "frm": [0, -10]}), "frm", "from"),
        ("sketch.drag_end", json!({"constrant": null}), "constrant", "constraint"),
        ("sketch.delete", json!({"entities": [bottom], "sketch ": 1}), "sketch ", "sketch"),
        ("sketch.create", json!({"plane": "XY", "ofset": 5}), "ofset", "offset"),
    ];
    for (id, p, wrong, right) in cases {
        let issues = messages(id, p.clone());
        assert_eq!(issues, [format!("unknown key `{wrong}`; did you mean `{right}`?")], "{id}");
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert_eq!(r["param_warnings"], json!(issues), "{id}: {r}");
    }
    // sketch.options refuses any key that is not an option itself, so it never replies with
    // warnings; the shape it would have (flattened untagged enum plus `param_warnings`) still
    // matches its schema.
    let opts = serde_json::to_value(OptionsResult {
        options: OptionsReply::Changed {
            options: SketchOptions {
                show_profile: true,
                show_points: true,
                show_dimensions: true,
                show_constraints: true,
                show_projected: true,
                show_construction: true,
                grid: true,
                snap: true,
                slice: false,
                sketch_3d: false,
            },
            sketch: None,
        },
        param_warnings: vec!["w".into()],
    })
    .unwrap_or_default();
    let schema = output_schema_for::<OptionsResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &opts) && opts["sketch"].is_null() && opts["param_warnings"][0] == "w", "{opts}");
    // The warned reply still matches its schema.
    let r = run(&mut s, "sketch.solve", json!({"skech": 1}));
    let schema = output_schema_for::<SolveResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
    let (mut strict, _) = editing(ParamCheck::Strict);
    let e = strict.execute("sketch.solve", &json!({"skech": 1})).err().map(|e| e.to_string());
    assert_eq!(e.as_deref(), Some("invalid parameters for `sketch.solve`: unknown key `skech`; did you mean `sketch`?"));
    let e = strict.execute("sketch.options", &json!({"show_profil": true})).err().map(|e| e.to_string());
    assert!(e.is_some_and(|e| e.contains("unknown key `show_profil`")));
}
