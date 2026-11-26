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
        &["activate_part", "face", "flip", "frame", "name", "offset", "origin", "plane", "project_edges", "view", "x_dir", "y_dir"],
    );
    assert_keys(input_schema_for::<ConstructionParams>(), &["curves", "points", "sketch", "value"]);
    assert_keys(input_schema_for::<DeleteParams>(), &["entities", "preserve_projection_link", "sketch"]);
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
        ("sketch.create", json!({"plane": {"origin": [0, true, "dy"], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("sketch.create", json!({"plane": {"origin": [0, "nope", 0], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("sketch.create", json!({"plane": {"origin": [0, "30 deg", 0], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
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
            (json!({"points": ["p1"]}), true),
            (json!({"curves": ["l1"], "points": "p2"}), true),
            (json!({}), false),
            (json!({"curves": []}), false),
            (json!({"points": []}), false),
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

fn model_view_context(focused: u64, recent: Vec<u64>) -> crate::ViewContext {
    use solidcraft_views::ViewKind;
    crate::ViewContext::new(
        vec![
            crate::ViewDescriptor { id: 1, kind: ViewKind::Model, window: 0, subject: None },
            crate::ViewDescriptor { id: 2, kind: ViewKind::Model, window: 1, subject: None },
            crate::ViewDescriptor { id: 3, kind: ViewKind::Render, window: 0, subject: None },
            crate::ViewDescriptor { id: 4, kind: ViewKind::Sheet, window: 0, subject: Some("sheet1".into()) },
        ],
        focused,
        recent,
    )
    .unwrap()
}

#[test]
fn model_sketch_targets_are_validated_and_returned_at_every_check_level() {
    use crate::schema::sketch::EditResult;
    for level in ParamCheck::ALL {
        let mut s = Session { param_check: level, ..Session::default() };
        s.set_view_context(model_view_context(3, vec![3, 2, 1])).unwrap();
        let created = run(&mut s, "sketch.create", json!({"plane":"XY"}));
        assert_eq!(created["sketching_view"], 2);
        assert_eq!(s.sketching_view, Some(2));
        assert!(conforms(output_schema_for::<CreateResult>().as_value(), output_schema_for::<CreateResult>().as_value(), &created));
        let sketch = created["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
        run(&mut s, "sketch.finish", json!({}));
        assert_eq!(s.sketching_view, None);
        let edited = run(&mut s, "sketch.edit", json!({"sketch":sketch,"view":1}));
        assert_eq!(edited["sketching_view"], 1);
        assert_eq!((s.active_sketch, s.sketching_view), (Some(sketch), Some(1)));
        assert!(conforms(output_schema_for::<EditResult>().as_value(), output_schema_for::<EditResult>().as_value(), &edited));
        let before = (s.doc.to_json(), s.active_sketch, s.sketching_view, s.undo.len());
        for target in [json!(0), json!(-1), json!(1.5), json!("2"), json!(true), json!([]), json!(3), json!(4), json!(99), json!(u64::MAX)] {
            for command in ["sketch.create", "sketch.edit"] {
                let params = if command == "sketch.create" { json!({"plane":"XY","view":target}) } else { json!({"sketch":sketch,"view":target}) };
                assert!(s.execute(command, &params).is_err(), "{level:?} {command} {target}");
                assert_eq!(s.doc.to_json(), before.0);
                assert_eq!((s.active_sketch, s.sketching_view), (None, None), "a refused new edit still finishes the previous sketch");
            }
        }
        assert_eq!((s.active_sketch, s.sketching_view), (None, None));
        run(&mut s, "sketch.create", json!({"plane":"XZ","view":2}));
        run(&mut s, "sketch.cancel", json!({}));
        assert_eq!((s.active_sketch, s.sketching_view), (None, None));
    }
}

#[test]
fn view_context_targets_only_real_models_and_caps_hostile_metadata() {
    use crate::{ViewContext, ViewDescriptor};
    use solidcraft_views::{MAX_VIEWS, ViewKind};
    let context = model_view_context(3, vec![3, 2, 1]);
    assert_eq!(context.resolve_sketch_target(None).unwrap(), 2);
    assert_eq!(context.resolve_sketch_target(Some(1)).unwrap(), 1);
    assert_eq!(model_view_context(1, vec![2, 1]).resolve_sketch_target(None).unwrap(), 1);
    assert_eq!(model_view_context(4, vec![4, 2, 1]).resolve_sketch_target(None).unwrap(), 2);
    assert_eq!(ViewContext::default().resolve_sketch_target(None).unwrap(), 1);
    for target in [0, 3, 4, 99, u64::MAX] {
        assert!(context.resolve_sketch_target(Some(target)).is_err());
    }
    let render = ViewDescriptor { id: 3, kind: ViewKind::Render, window: 0, subject: None };
    let context = ViewContext::new(vec![render.clone()], 3, vec![3]).unwrap();
    assert!(context.resolve_sketch_target(None).is_err(), "the engine must never invent a fallback Model ID");
    let mut s = Session::default();
    s.set_view_context(context).unwrap();
    let before = s.doc.to_json();
    assert!(s.execute("sketch.create", &json!({})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert!(ViewContext::new(vec![], 1, vec![]).is_err());
    assert!(ViewContext::new(vec![render.clone(); MAX_VIEWS + 1], 3, vec![]).is_err());
    assert!(ViewContext::new(vec![render.clone(), render.clone()], 3, vec![]).is_err());
    assert!(ViewContext::new(vec![render.clone()], 99, vec![]).is_err());
    assert!(ViewContext::new(vec![render.clone()], 3, vec![3, 3]).is_err());
    assert!(ViewContext::new(vec![render.clone()], 3, vec![99]).is_err());
    assert!(ViewContext::new(vec![render.clone()], 3, vec![3; MAX_VIEWS + 1]).is_err());
    for id in [0, (1 << 53) + 1, u64::MAX] {
        assert!(ViewContext::new(vec![ViewDescriptor { id, ..render.clone() }], id, vec![]).is_err());
    }
    assert!(ViewContext::new(vec![ViewDescriptor { window: u64::MAX, ..render.clone() }], 3, vec![]).is_err());
    assert!(ViewContext::new(vec![ViewDescriptor { subject: Some("x".repeat(257)), ..render.clone() }], 3, vec![]).is_err());
    let too_many_windows = (1..=10).map(|id| ViewDescriptor { id, window: id - 1, ..render.clone() }).collect();
    assert!(ViewContext::new(too_many_windows, 1, vec![1]).is_err());
    let popouts_without_root = (1..=9).map(|id| ViewDescriptor { id, window: id, ..render.clone() }).collect();
    assert!(ViewContext::new(popouts_without_root, 1, vec![1]).is_err(), "omitting root descriptors cannot grant an extra pop-out");
    let maximum_popouts = (1..=8).map(|id| ViewDescriptor { id, window: id, ..render.clone() }).collect();
    assert!(ViewContext::new(maximum_popouts, 1, vec![1]).is_ok(), "a root-less snapshot may still contain eight pop-outs");
    let maximum_open = (1..=MAX_VIEWS as u64).map(|id| ViewDescriptor { id, ..render.clone() }).collect();
    assert!(ViewContext::new(maximum_open, 1, vec![1]).is_ok());
    assert!(ViewContext::new(vec![ViewDescriptor { id: 1 << 53, ..render }], 1 << 53, vec![]).is_ok());
}

#[test]
fn model_sketch_owner_survives_history_and_normalizes_when_its_view_closes() {
    use crate::{ViewContext, ViewDescriptor};
    use solidcraft_views::ViewKind;
    let mut s = Session::default();
    s.set_view_context(model_view_context(2, vec![2, 1])).unwrap();
    let created = run(&mut s, "sketch.create", json!({"plane":"XY","view":2}));
    let sketch = created["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    let checkpoint = s.checkpoint();
    let root_only = ViewContext::new(vec![ViewDescriptor { id: 1, kind: ViewKind::Model, window: 0, subject: None }], 1, vec![1]).unwrap();
    assert!(s.set_view_context(root_only.clone()).is_err(), "a live owner cannot silently disappear");
    assert_eq!(s.view_context.focused(), 2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((s.active_sketch, s.sketching_view), (Some(sketch), Some(2)));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!((s.active_sketch, s.sketching_view), (Some(sketch), Some(2)));
    run(&mut s, "sketch.finish", json!({}));
    s.set_view_context(root_only.clone()).unwrap();
    let completed = s.doc.to_json();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((s.active_sketch, s.sketching_view), (None, None), "geometry undo never resurrects a closed owner");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), completed);
    assert_eq!((s.active_sketch, s.sketching_view), (None, None));
    s.restore_checkpoint(checkpoint).unwrap();
    assert_eq!(s.doc.to_json(), completed);
    assert_eq!((s.active_sketch, s.sketching_view), (None, None));
    assert_eq!(s.view_context, root_only, "history never restores stale host placement metadata");
}

#[test]
fn restoring_a_closed_owner_checkpoint_finalizes_its_temporary_marker() {
    let mut s = Session::default();
    s.set_view_context(model_view_context(2, vec![2, 1])).unwrap();
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","view":2}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.box", json!({"length":2,"width":3,"height":4}));
    run(&mut s, "sketch.edit", json!({"sketch":sketch,"view":2}));
    assert!(s.doc.marker.is_some());
    let checkpoint = s.checkpoint();
    run(&mut s, "sketch.finish", json!({}));
    s.set_view_context(crate::ViewContext::default()).unwrap();
    s.restore_checkpoint(checkpoint).unwrap();
    assert_eq!(s.doc.marker, None, "a vanished editor does not leave later features rolled away");
    assert_eq!((s.active_sketch, s.sketching_view), (None, None));
    assert_eq!(s.doc.features.len(), 2);
}

fn edit_start_line(s: &mut Session, name: &str) -> u64 {
    let id = run(s, "sketch.create", json!({"plane":"XY","name":name}))["sketch"].as_u64().unwrap();
    run(s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    id
}

#[test]
fn edit_start_metadata_is_semantic_and_mixed_setters_preserve_visibility() {
    for id in [
        "solid.extrude",
        "solid.fillet",
        "solid.hole",
        "construct.plane",
        "surface.patch",
        "surface.match",
        "body.repair",
        "timeline.paste",
        "sheet.flange",
        "plastic.boss",
        "frame.member",
        "frame.end_cap",
        "joint.create",
        "drawing.note.add",
        "drawing.dimension.add",
        "sketch.create",
        "sketch.edit",
    ] {
        let spec = crate::find_command(id).unwrap();
        assert!(spec.planned.is_none());
        assert!(spec.starts_edit(&json!({})), "{id}");
    }
    for id in [
        "parameters.change",
        "sketch.dimension",
        "sketch.line",
        "sketch.line3d",
        "frame.profile.insert",
        "timeline.edit",
        "timeline.redefine",
        "timeline.rename",
        "browser.visibility",
        "select.set",
        "drawing.open",
        "tab.view.layout",
        "tab.view.move",
        "tab.window.move_view",
    ] {
        if let Some(spec) = crate::find_command(id) {
            assert!(!spec.starts_edit(&json!({})), "{id}");
        }
    }
    let dimension = crate::find_command("drawing.dimension.edit").unwrap();
    assert!(dimension.starts_edit(&Value::Null), "opening its editor is a new edit, as a note's is");
    let note = crate::find_command("note.update").unwrap();
    assert!(!note.starts_edit(&json!({"note":1,"hidden":true})));
    assert!(note.starts_edit(&json!({"note":1,"text":"Changed"})));
    assert!(note.starts_edit(&Value::Null), "opening its editor is a new edit");
    let view = crate::find_command("drawing.view.edit").unwrap();
    assert!(!view.starts_edit(&json!({"view":1,"set":{"name":"Renamed","hidden":true}})));
    let sketching = Session { active_sketch: Some(1), ..Session::default() };
    assert!(
        crate::find_command("frame.end_cap").unwrap().allowed(&sketching).is_ok(),
        "a feature tool remains available to finish the current sketch"
    );
}

#[test]
fn edit_start_failed_live_commands_keep_the_exit_and_event_at_every_check_level() {
    for level in ParamCheck::ALL {
        for command in ["surface.match", "body.repair", "timeline.paste"] {
            let mut s = Session { param_check: level, ..Session::default() };
            let sketch = edit_start_line(&mut s, "Working");
            let document = s.doc.to_json();
            assert!(s.execute(command, &json!({})).is_err(), "{command}: invalid parameters or empty clipboard");
            assert_eq!(s.doc.to_json(), document, "{command}: the rejected feature adds nothing");
            assert_eq!((s.active_sketch, s.sketching_view), (None, None), "{command}");
            let event = s.take_sketch_finish_event().unwrap();
            assert_eq!(event.sketch, sketch, "{command}");
            assert_eq!(event.domain, crate::SketchDomain::Model);
            assert!(s.take_sketch_finish_event().is_none(), "{command}");
        }
        let mut s = Session { param_check: level, ..Session::default() };
        let sketch = edit_start_line(&mut s, "Working");
        let document = s.doc.to_json();
        assert!(s.execute("solid.fillet", &json!({"radius":-1})).is_err());
        assert_eq!(s.doc.to_json(), document, "the rejected feature adds nothing");
        assert_eq!((s.active_sketch, s.sketching_view), (None, None));
        let event = s.take_sketch_finish_event().unwrap();
        assert_eq!((event.sketch, event.name.as_str(), event.view), (sketch, "Working", Some(1)));
        assert_eq!(event.domain, crate::SketchDomain::Model);
        assert!(event.sequence > 0);
        assert!(s.take_sketch_finish_event().is_none());
        let next = edit_start_line(&mut s, "Authorized");
        s.set_command_check(Some(std::sync::Arc::new(|id, _, _| if id == "solid.box" { Err("denied".into()) } else { Ok(()) })));
        assert!(s.execute("solid.box", &json!({})).is_err());
        assert_eq!(s.active_sketch, Some(next), "host refusal precedes the edit boundary");
        assert!(s.take_sketch_finish_event().is_none());
    }
}

#[test]
fn edit_start_empty_origin_is_discarded_but_lines_points_and_unsolved_geometry_remain() {
    let mut s = Session::default();
    let empty = run(&mut s, "sketch.create", json!({}))["sketch"].as_u64().unwrap();
    s.mark_sketch_entered_by_person();
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.doc.feature(empty).is_none());
    let line = edit_start_line(&mut s, "Line");
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.doc.feature(line).is_some());
    let point = run(&mut s, "sketch.create", json!({}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.point", json!({"point":[7,9]}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.doc.feature(point).is_some());
    let unsolved = edit_start_line(&mut s, "Unsolved");
    let sketch = s.doc_mut().sketch_mut(unsolved).unwrap();
    for point in &mut sketch.points {
        point.fixed = true;
    }
    sketch.add_constraint(solidcraft_sketch::ConstraintKind::Distance { p: 0, q: 1, value: 999.0 }, None).unwrap();
    let evaluations = s.evaluations;
    let event = s.prepare_edit_start("solid.hole", &Value::Null).unwrap().unwrap();
    assert_eq!(event.sketch, unsolved);
    assert!(s.active_sketch.is_none());
    assert!(s.doc.feature(unsolved).is_some());
    assert_eq!(s.evaluations, evaluations, "leaving an unsolved sketch does not demand a solve");
}

fn edit_start_two_profiles() -> (Session, u64, u64) {
    let mut s = Session::default();
    let a = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Earlier"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]}));
    run(&mut s, "sketch.finish", json!({}));
    let b = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Later"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[5,5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.edit", json!({"sketch":a}));
    (s, a, b)
}
fn edit_start_extruded_source(s: &Session) -> u64 {
    match &s.doc.features.last().unwrap().kind {
        solidcraft_doc::FeatureKind::Extrude { sketch, .. } => sketch.0,
        _ => panic!("expected Extrude"),
    }
}

#[test]
fn edit_start_extrude_uses_the_earlier_finished_sketch_and_undo_stays_after_finish() {
    for explicit in [false, true] {
        let (mut s, a, b) = edit_start_two_profiles();
        let params = if explicit { json!({"sketch":b,"distance":2}) } else { json!({"distance":2}) };
        let reply = run(&mut s, "solid.extrude", params);
        assert_eq!(reply["finished_sketch"], "Earlier");
        assert_eq!(edit_start_extruded_source(&s), if explicit { b } else { a });
        let schema = output_schema_for::<crate::schema::features::FeatureResult>();
        assert!(schema.as_value()["properties"]["finished_sketch"].is_object());
        assert!(conforms(schema.as_value(), schema.as_value(), &reply));
        assert_eq!(s.doc.marker, None);
        let completed = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert!(s.active_sketch.is_none());
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), completed);
        assert!(s.active_sketch.is_none());
    }
}

#[test]
fn edit_start_prepared_source_survives_navigation_and_clears_after_success_or_failure() {
    for fail_first in [false, true] {
        let (mut s, a, b) = edit_start_two_profiles();
        let event = s.prepare_edit_start("solid.extrude", &Value::Null).unwrap().unwrap();
        assert_eq!(event.sketch, a);
        assert!(s.needs_sketch_finish_evaluation());
        assert_eq!(s.take_sketch_finish_event().unwrap().sequence, event.sequence);
        assert!(s.prepare_edit_start("solid.extrude", &Value::Null).unwrap().is_none());
        run(&mut s, "select.clear", json!({}));
        if fail_first {
            assert!(s.execute("solid.extrude", &json!({"distance":"invalid"})).is_err());
        } else {
            run(&mut s, "solid.extrude", json!({"distance":2}));
            assert_eq!(edit_start_extruded_source(&s), a);
        }
        assert!(s.take_sketch_finish_event().is_none(), "preparation does not publish its event twice");
        run(&mut s, "solid.extrude", json!({"distance":2}));
        assert_eq!(edit_start_extruded_source(&s), b, "the old source cannot leak into the next feature");
    }
}

#[test]
fn edit_start_nested_dispatch_and_dimension_continuations_follow_the_same_policy() {
    let mut s = Session::default();
    let id = edit_start_line(&mut s, "Nested");
    let fillet = crate::find_command("solid.fillet").unwrap();
    assert!(fillet.run_nested(&mut s, &json!({"radius":-1})).is_err());
    assert!(s.active_sketch.is_none());
    assert_eq!(s.take_sketch_finish_event().unwrap().sketch, id);
    let other = edit_start_line(&mut s, "Continuing");
    assert!(fillet.run_nested_continuing(&mut s, &json!({"radius":-1})).is_err());
    assert_eq!(s.active_sketch, Some(other));
    assert!(s.take_sketch_finish_event().is_none());
    run(&mut s, "parameters.add", json!({"name":"Width","expression":"10 mm"}));
    run(&mut s, "parameters.change", json!({"name":"Width","expression":"12 mm"}));
    run(&mut s, "timeline.rename", json!({"feature":other,"name":"Renamed"}));
    assert_eq!(s.active_sketch, Some(other));
    assert!(s.take_sketch_finish_event().is_none());
}

#[test]
fn edit_start_in_place_feature_updates_preserve_the_current_sketch() {
    let mut s = Session::default();
    let profile = run(&mut s, "sketch.create", json!({}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]}));
    run(&mut s, "sketch.finish", json!({}));
    let feature = run(&mut s, "solid.extrude", json!({"sketch":profile,"distance":2}))["feature"].as_u64().unwrap();
    let active = edit_start_line(&mut s, "Keep editing");
    run(&mut s, "timeline.edit", json!({"feature":feature,"set":{"extent.distance":"3"}}));
    run(&mut s, "timeline.redefine", json!({"feature":feature,"command":"solid.extrude","params":{"sketch":profile,"distance":4}}));
    assert_eq!(s.active_sketch, Some(active));
    assert!(s.take_sketch_finish_event().is_none());
}

#[test]
fn edit_start_model_and_sheet_receivers_are_explicit_even_after_drawing_focus_changes() {
    let mut s = Session::default();
    let model = edit_start_line(&mut s, "Model edit");
    let drawing = run(&mut s, "drawing.create", json!({}));
    let owner = s.open_drawing;
    assert_eq!(s.active_sketch, Some(model), "opening a drawing is focus, not an annotation edit");
    assert!(s.execute("drawing.note.add", &json!({})).is_err());
    let event = s.take_sketch_finish_event().unwrap();
    assert_eq!(event.domain, crate::SketchDomain::Model);
    assert_eq!(event.sketch, model);
    assert_eq!(s.open_drawing, owner);
    assert!(s.active_sheet_sketch.is_none());
    let made = run(&mut s, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
    run(&mut s, "sketch.line", json!({"points":[[10,10],[20,10]]}));
    assert!(s.execute("drawing.note.add", &json!({})).is_err());
    let event = s.take_sketch_finish_event().unwrap();
    assert!(matches!(event.domain, crate::SketchDomain::Sheet { .. }));
    assert_eq!(Some(event.sketch), solidcraft_doc::ids::decode(made["sketch"].as_str().unwrap()));
    assert!(s.active_sheet_sketch.is_none());
    assert_eq!(s.open_drawing, owner);
}
