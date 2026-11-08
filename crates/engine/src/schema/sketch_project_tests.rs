use serde_json::{Value, json};

use super::{
    AutoProjectParams, AutoProjectResult, FitCurvesParams, FitSectionResult, Include3dParams, IntersectParams, IntersectionCurveParams,
    IsoCurveParams, ProjectParams, ProjectRef, ProjectResult, ProjectToSurfaceParams, ProjectToSurfaceResult, SketchBreakLinkParams,
    SketchBreakLinkResult, SketchLinkResult, SpunProfileParams,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

/// A box and a tall cylinder through it (two bodies), a finished sketch `Src` with a line `source`,
/// and a sketch being edited, on the XY plane at height 5.
fn scene(level: ParamCheck) -> Session {
    scene_on(level, "XY")
}

/// [`scene`] with the sketch being edited on `plane`.
fn scene_on(level: ParamCheck, plane: &str) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("solid.box", json!({"corner": [0, 0, 0], "length": 20, "width": 20, "height": 10, "body_name": "Block"})),
        ("solid.cylinder", json!({"base": [10, 10, -5], "radius": 4, "height": 30, "operation": "new", "body_name": "Pin"})),
        ("sketch.create", json!({"plane": "XY", "offset": 20, "name": "Src"})),
        ("sketch.line", json!({"points": [[2, 2], [18, 2]], "ids": ["source"]})),
        ("sketch.line", json!({"points": [[2, 4], [18, 8]], "ids": ["other"]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": plane, "offset": 5})),
    ] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id} {p}: {e}");
        }
    }
    s
}

fn error(level: ParamCheck, id: &str, p: &Value) -> Option<String> {
    scene(level).execute(id, p).err().map(|e| e.to_string())
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ProjectParams>(), &["projection_link", "ref", "refs"]);
    assert_keys(input_schema_for::<IntersectParams>(), &["projection_link", "ref", "refs"]);
    assert_keys(input_schema_for::<Include3dParams>(), &["ref", "refs"]);
    assert_keys(input_schema_for::<ProjectToSurfaceParams>(), &["body", "curves", "face"]);
    assert_keys(input_schema_for::<IntersectionCurveParams>(), &["a", "b"]);
    assert_keys(input_schema_for::<SpunProfileParams>(), &["axis", "body"]);
    assert_keys(input_schema_for::<IsoCurveParams>(), &["body", "direction", "face"]);
    assert_keys(input_schema_for::<FitCurvesParams>(), &["body"]);
    assert_keys(input_schema_for::<SketchBreakLinkParams>(), &["entities", "link"]);
    assert_keys(input_schema_for::<AutoProjectParams>(), &["value"]);
}

/// Strict gives the handlers' messages for the calls they refuse for a key. These fail before the
/// handler looks at the sketch being edited, so they hold with none open.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.project", json!({})),
        ("sketch.project", json!({"refs": []})),
        ("sketch.project", json!({"ref": []})),
        ("sketch.project", json!({"refs": [], "ref": "origin"})),
        ("sketch.project", json!({"projection_link": "no", "refs": ["origin"]})),
        ("sketch.project", json!({"projection_link": null, "refs": ["origin"]})),
        ("sketch.project", json!({"projection_link": 1})),
        ("sketch.intersect", json!({})),
        ("sketch.intersect", json!({"refs": []})),
        ("sketch.intersect", json!({"projection_link": "no", "refs": [{"body": "Block"}]})),
        ("sketch.include_3d", json!({})),
        ("sketch.include_3d", json!({"refs": []})),
        ("sketch.project_to_surface", json!({})),
        ("sketch.project_to_surface", json!({"face": [1, 2]})),
        ("sketch.project_to_surface", json!({"face": null})),
        ("sketch.project_to_surface", json!({"face": "top"})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3]})),
        ("sketch.spun_profile", json!({})),
        ("sketch.spun_profile", json!({"body": 5, "axis": "Z"})),
        ("sketch.spun_profile", json!({"body": null})),
        ("sketch.spun_profile", json!({"body": "Block"})),
        ("sketch.spun_profile", json!({"body": "Block", "axis": 3})),
        ("sketch.spun_profile", json!({"body": "Block", "axis": null})),
        ("sketch.isoparametric_curve", json!({})),
        ("sketch.isoparametric_curve", json!({"face": [1, 2]})),
        ("sketch.isoparametric_curve", json!({"face": {"x": 1, "y": 2}})),
        ("sketch.fit_curves_to_section", json!({})),
        ("sketch.fit_curves_to_section", json!({"body": 7})),
        ("sketch.fit_curves_to_section", json!({"body": null})),
        ("sketch.fit_curves_to_section", json!({"body": ["Block"]})),
    ]);
}

/// The same for the calls that fail only once the handler has a sketch to work in: what it
/// refuses in a reference or a list, with the sketch being edited open.
#[test]
fn strict_keeps_todays_errors_in_a_sketch() {
    let cases = [
        ("sketch.project", json!({"refs": null})),
        ("sketch.project", json!({"refs": [true]})),
        ("sketch.project", json!({"refs": [5]})),
        ("sketch.project", json!({"refs": [null]})),
        ("sketch.project", json!({"refs": [[1, 2, 3]]})),
        ("sketch.project", json!({"refs": [{}]})),
        ("sketch.project", json!({"refs": [{"edge": [1, 2]}]})),
        ("sketch.project", json!({"refs": [{"edge": "x"}, {"plane": 3}]})),
        ("sketch.project", json!({"refs": ["origin", {"sketch": [1], "curve": "source"}]})),
        ("sketch.project", json!({"refs": [{"sketch": true}]})),
        ("sketch.project", json!({"refs": [{"sketch": "Src"}]})),
        ("sketch.project", json!({"refs": [{"sketch": "Src", "curve": 4}]})),
        ("sketch.project", json!({"ref": {"axis": 5}})),
        ("sketch.project", json!({"ref": 5})),
        ("sketch.intersect", json!({"refs": [{"vertex": [1, 2]}]})),
        ("sketch.intersect", json!({"ref": {"sketch": "Src", "point": 1}})),
        ("sketch.include_3d", json!({"refs": [1.5]})),
        ("sketch.include_3d", json!({"refs": {"colour": "red"}})),
        ("sketch.include_3d", json!({"refs": null})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": 5})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": null})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": {"sketch": "Src", "curve": "source"}})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": []})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": vec![json!({"sketch": "Src", "curve": "source"}); 501]})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": [{"edge": [1, 2, 3]}]})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": [{"sketch": "Src", "point": "p1"}]})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": [{"body": "Block"}]})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": ["origin"]})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": [{"origin": true}]})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": [{"nothing": 1}]})),
        ("sketch.project_to_surface", json!({"face": [1, 2, 3], "curves": [{"sketch": "Src"}]})),
        ("sketch.intersection_curve", json!({})),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}})),
        ("sketch.intersection_curve", json!({"b": {"body": "Block"}})),
        ("sketch.intersection_curve", json!({"a": null, "b": {"body": "Block"}})),
        ("sketch.intersection_curve", json!({"a": 5, "b": {"body": "Block"}})),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}, "b": {"nothing": 1}})),
        ("sketch.intersection_curve", json!({"a": "origin", "b": {"body": "Block"}})),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}, "b": "x"})),
        ("sketch.intersection_curve", json!({"a": {"edge": [1, 2, 3]}, "b": {"body": "Block"}})),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}, "b": {"vertex": [1, 2, 3]}})),
        ("sketch.intersection_curve", json!({"a": {"plane": "XY"}, "b": {"body": "Block"}})),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}, "b": {"sketch": "Src", "point": "p1"}})),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}, "b": {"sketch": "Src"}})),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "direction": null})),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "direction": "w"})),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "direction": [1, 2]})),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "direction": 3})),
        ("sketch.break_link", json!({})),
        ("sketch.break_link", json!({"entities": []})),
        ("sketch.break_link", json!({"entities": 5})),
        ("sketch.break_link", json!({"entities": [1]})),
        ("sketch.break_link", json!({"entities": ["a", true]})),
        ("sketch.break_link", json!({"entities": {"a": 1}})),
        ("sketch.break_link", json!({"link": "j9"})),
    ];
    for (id, p) in cases {
        let off = error(ParamCheck::Off, id, &p);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict, id, &p), off, "{id} {p}");
    }
}

/// Strict says the same wherever the handler's message comes from the contract's reading: every
/// shape of reference is read by the type as the handler reads it, so the contract refuses it
/// exactly when the handler does for its form (a sketch that exists and is another sketch's).
#[test]
fn the_reference_type_reads_like_the_handler() {
    let forms = [
        json!("origin"),
        json!("ORIGIN"),
        json!("x"),
        json!("Z"),
        json!("Block"),
        json!("nothing"),
        json!({"edge": [10, 0, 5]}),
        json!({"edge": {"x": 10, "y": 0, "z": 5}, "body": "Block"}),
        json!({"edge": "bad", "body": "Block"}),
        json!({"edge": [1, 2], "face": [10, 10, 10]}),
        json!({"edge": [1, 2, 3, 4]}),
        json!({"face": [10, 10, 10], "body": 4}),
        json!({"vertex": [0, 0, 10]}),
        json!({"vertex": [1e9, 0, 0]}),
        json!({"sketch": "Src", "curve": "source"}),
        json!({"sketch": "Src", "point": "p1"}),
        json!({"sketch": "Src", "curve": "source", "point": "p1"}),
        json!({"sketch": "Src", "curve": 7, "point": "p1"}),
        json!({"sketch": "Src"}),
        json!({"sketch": "Src", "curve": null}),
        json!({"sketch": 1.5}),
        json!({"sketch": true}),
        json!({"sketch": null}),
        json!({"sketch": ["Src"]}),
        json!({"sketch": {"name": "Src"}, "body": "Block"}),
        json!({"body": "Block"}),
        json!({"body": 5}),
        json!({"body": "Block", "axis": "Z"}),
        json!({"axis": "X"}),
        json!({"axis": "x"}),
        json!({"axis": 5}),
        json!({"plane": "XY"}),
        json!({"plane": null}),
        json!({"origin": null}),
        json!({"origin": [0, 0, 0]}),
        json!({}),
        json!([]),
        json!(null),
        json!(true),
        json!(5),
        json!({"nothing": 1}),
    ];
    for form in forms {
        let issues = issues("sketch.include_3d", &json!({"refs": [form]}));
        let off = error(ParamCheck::Off, "sketch.include_3d", &json!({"refs": [form]}));
        let strict = error(ParamCheck::Strict, "sketch.include_3d", &json!({"refs": [form]}));
        let typed = serde_json::from_value::<ProjectRef>(form.clone());
        // The type, the check and the handler agree on what is refused for its form.
        assert_eq!(typed.is_err(), !issues.is_empty(), "{form}");
        if typed.is_err() {
            assert_eq!(strict, off, "{form}");
        } else {
            let message = off.unwrap_or_default();
            // What the handler refuses of a form the type accepts needs the design.
            for shape in ["cannot project", "`sketch` must be", "`refs` is empty"] {
                assert!(!message.contains(shape), "{form}: {message}");
            }
        }
    }
    // A single reference is read as a list of one, and `ref` is read only without `refs`.
    let count = |p: Value| issues("sketch.project", &p).len();
    assert_eq!(count(json!({"refs": "origin"})), 0);
    assert_eq!(count(json!({"refs": {"origin": 1}})), 0);
    assert_eq!(count(json!({"ref": "origin"})), 0);
    assert_eq!(count(json!({"refs": ["origin"], "ref": 5})), 0, "`ref` is not read when `refs` is given");
    assert_eq!(count(json!({"refs": null, "ref": "origin"})), 1, "`refs` is read even when null");
    assert_eq!(count(json!({"ref": 5})), 1);
    assert_eq!(count(json!({"refs": []})), 1);
    assert_eq!(count(json!({"refs": [], "ref": "origin"})), 1);
    // Only the first 1000 references are read.
    let mut many = vec![json!("origin"); 1000];
    many.push(json!(5));
    assert_eq!(count(json!({"refs": many})), 0);
    many.insert(0, json!(5));
    assert_eq!(count(json!({"refs": many})), 1);
    // `include_3d` takes no `projection_link`: the handler never reads it, so strict names it.
    assert_eq!(issues("sketch.include_3d", &json!({"refs": ["origin"], "projection_link": false})).len(), 1);
    assert_eq!(issues("sketch.project", &json!({"refs": ["origin"], "projection_link": false})).len(), 0);
}

/// What the contracts take and refuse in the schemas as well as the types.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let check = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value().clone();
        for (p, takes) in cases {
            assert_eq!(conforms(&schema, &schema, p), *takes, "{p}");
        }
    };
    for schema in [input_schema_for::<ProjectParams>(), input_schema_for::<IntersectParams>()] {
        check(
            schema,
            &[
                (json!({"refs": [{"edge": [1, 2, 3], "body": "Block"}, {"face": [1, 2, 3]}, "origin", {"axis": "X"}]}), true),
                (json!({"refs": {"sketch": 2, "curve": "c1"}, "projection_link": false}), true),
                (json!({"ref": {"body": "Block"}}), true),
                (json!({"refs": [{"sketch": "Src", "point": "p1"}, {"plane": "XY"}, {"vertex": {"x": 1, "y": 2, "z": 3}}]}), true),
                (json!({}), false),
                (json!({"projection_link": true}), false),
                (json!({"refs": []}), false),
                (json!({"refs": [{"edge": [1, 2]}]}), false),
                (json!({"refs": ["origin"], "projection_link": "yes"}), false),
                (json!({"refs": ["origin"], "projection_lnk": true}), false),
            ],
        );
    }
    check(
        input_schema_for::<Include3dParams>(),
        &[(json!({"refs": ["origin"]}), true), (json!({"refs": ["origin"], "projection_link": true}), false)],
    );
    check(
        input_schema_for::<ProjectToSurfaceParams>(),
        &[
            (json!({"face": [0, 0, 20], "curves": [{"sketch": 4, "curve": "source"}]}), true),
            (json!({"face": [0, 0, 20], "curves": [{"sketch": "Src", "curve": "source"}], "body": "Block"}), true),
            (json!({"face": [0, 0, 20]}), false),
            (json!({"curves": [{"sketch": 4, "curve": "source"}]}), false),
            (json!({"face": [0, 0, 20], "curves": []}), false),
            (json!({"face": [0, 0, 20], "curves": [{"sketch": 4}]}), false),
        ],
    );
    check(
        input_schema_for::<IntersectionCurveParams>(),
        &[
            (json!({"a": {"body": "Block"}, "b": {"face": [1, 2, 3], "body": "Pin"}}), true),
            (json!({"a": {"sketch": "Src", "curve": "source"}, "b": "Pin"}), true),
            (json!({"a": {"body": "Block"}}), false),
            (json!({"a": {"body": "Block"}, "b": {"edge": [1, 2, 3]}}), false),
        ],
    );
    check(
        input_schema_for::<SpunProfileParams>(),
        &[
            (json!({"body": "Pin", "axis": "Z"}), true),
            (json!({"body": "Pin"}), false),
            (json!({"axis": "Z"}), false),
            (json!({"body": 1, "axis": "Z"}), false),
        ],
    );
    check(
        input_schema_for::<IsoCurveParams>(),
        &[
            (json!({"face": [1, 2, 3]}), true),
            (json!({"face": [1, 2, 3], "body": "Block", "direction": "v"}), true),
            (json!({"face": [1, 2, 3], "direction": [0, 1, 0]}), true),
            (json!({"direction": "u"}), false),
            (json!({"face": [1, 2, 3], "direction": "w"}), false),
        ],
    );
    check(input_schema_for::<FitCurvesParams>(), &[(json!({"body": "Pin"}), true), (json!({}), false)]);
    check(
        input_schema_for::<SketchBreakLinkParams>(),
        &[
            (json!({"link": "j1"}), true),
            (json!({"entities": "c1"}), true),
            (json!({"entities": ["c1", "p2"], "link": "j1"}), true),
            (json!({}), false),
            (json!({"entities": [1]}), false),
        ],
    );
    check(input_schema_for::<AutoProjectParams>(), &[(json!({}), true), (json!({"value": false}), true), (json!({"value": "no"}), false)]);
}

/// Every command replies as its output schema says, in Warn without warnings, and in the same JSON
/// as with the check off.
#[test]
fn replies_match_the_output_schema() {
    fn matches(schema: schemars::Schema, r: &Value) {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    }
    let calls: Vec<(&str, Value, fn() -> schemars::Schema)> = vec![
        ("sketch.project", json!({"refs": [{"edge": [10, 0, 10]}]}), output_schema_for::<ProjectResult>),
        ("sketch.project", json!({"refs": [{"vertex": [0, 0, 10]}, {"face": [10, 10, 10], "body": "Block"}]}), output_schema_for::<ProjectResult>),
        ("sketch.project", json!({"refs": {"edge": [20, 10, 10], "body": "Block"}, "projection_link": true}), output_schema_for::<ProjectResult>),
        ("sketch.project", json!({"ref": {"edge": [10, 20, 10]}, "projection_link": false}), output_schema_for::<ProjectResult>),
        ("sketch.project", json!({"refs": ["origin"]}), output_schema_for::<ProjectResult>),
        (
            "sketch.project",
            json!({"refs": [{"axis": "X"}, {"plane": "XZ"}, {"sketch": "Src", "curve": "source"}]}),
            output_schema_for::<ProjectResult>,
        ),
        ("sketch.project", json!({"refs": [{"sketch": "Src", "point": "source.start"}]}), output_schema_for::<ProjectResult>),
        ("sketch.project", json!({"refs": [{"edge": "bad", "body": "Pin"}]}), output_schema_for::<ProjectResult>),
        ("sketch.project", json!({"refs": ["Pin"], "ref": 5}), output_schema_for::<ProjectResult>),
        ("sketch.intersect", json!({"refs": [{"body": "Block"}]}), output_schema_for::<ProjectResult>),
        ("sketch.intersect", json!({"refs": {"face": [0, 10, 5], "body": "Block"}, "projection_link": false}), output_schema_for::<ProjectResult>),
        ("sketch.intersect", json!({"ref": {"edge": [0, 0, 5]}}), output_schema_for::<ProjectResult>),
        ("sketch.include_3d", json!({"refs": [{"edge": [20, 10, 10]}]}), output_schema_for::<ProjectResult>),
        ("sketch.include_3d", json!({"ref": {"vertex": [0, 0, 10]}}), output_schema_for::<ProjectResult>),
        (
            "sketch.project_to_surface",
            json!({"face": [10, 10, 10], "curves": [{"sketch": "Src", "curve": "source"}, {"sketch": "Src", "curve": "other"}]}),
            output_schema_for::<ProjectToSurfaceResult>,
        ),
        (
            "sketch.project_to_surface",
            json!({"face": {"x": 10, "y": 10, "z": 10}, "body": "Block", "curves": [{"sketch": 3, "curve": "source"}]}),
            output_schema_for::<ProjectToSurfaceResult>,
        ),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}, "b": {"body": "Pin"}}), output_schema_for::<SketchLinkResult>),
        ("sketch.intersection_curve", json!({"a": {"face": [10, 10, 10], "body": "Block"}, "b": "Pin"}), output_schema_for::<SketchLinkResult>),
        (
            "sketch.intersection_curve",
            json!({"a": {"sketch": "Src", "curve": "source"}, "b": {"body": "Block"}}),
            output_schema_for::<SketchLinkResult>,
        ),
        ("sketch.spun_profile", json!({"body": "Pin", "axis": "Z"}), output_schema_for::<SketchLinkResult>),
        ("sketch.spun_profile", json!({"body": "Pin", "axis": "x"}), output_schema_for::<SketchLinkResult>),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10]}), output_schema_for::<SketchLinkResult>),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "body": "Block", "direction": "V"}), output_schema_for::<SketchLinkResult>),
        ("sketch.isoparametric_curve", json!({"face": {"x": 10, "y": 10, "z": 10}, "direction": [1, 0, 0]}), output_schema_for::<SketchLinkResult>),
        ("sketch.fit_curves_to_section", json!({"body": "Block"}), output_schema_for::<FitSectionResult>),
        ("sketch.auto_project", json!({}), output_schema_for::<AutoProjectResult>),
        ("sketch.auto_project", json!({"value": true}), output_schema_for::<AutoProjectResult>),
    ];
    for (id, p, output) in calls {
        // The spun profile needs a plane the cylinder's axis lies in.
        let plane = if id == "sketch.spun_profile" { "XZ" } else { "XY" };
        let a = scene_on(ParamCheck::Off, plane).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let b = scene_on(ParamCheck::Warn, plane).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(b.get("param_warnings").is_none(), "{id} {p}: {b}");
        assert_eq!(a, b, "{id} {p}");
        matches(output(), &b);
    }
    let run = |p: Value| scene(ParamCheck::Warn).execute("sketch.project", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    // The shapes a caller sees: the link, its curves and points; null where the link is not kept.
    let r = run(json!({"refs": [{"edge": [10, 0, 10]}]}));
    assert_eq!(r["links"].as_array().map(Vec::len), Some(1), "{r}");
    let l = &r["links"][0];
    assert!(l["link"].is_string() && l["curves"].is_array() && l["points"].is_array(), "{r}");
    assert_eq!(l.as_object().map(|o| o.len()), Some(3), "{r}");
    assert!(r.get("already_projected").is_none(), "no `already_projected` when all was projected: {r}");
    assert!(r["created"]["curves"].is_array(), "the engine reports what a sketch command created: {r}");
    let r = run(json!({"refs": [{"edge": [10, 0, 10]}], "projection_link": false}));
    assert!(r["links"][0]["link"].is_null() && r["links"][0]["curves"][0].is_string(), "{r}");
    matches(output_schema_for::<ProjectResult>(), &r);
    // Projecting what the sketch shows already says so, and the reply still matches.
    let mut s = scene(ParamCheck::Warn);
    s.execute("sketch.project", &json!({"refs": [{"edge": [10, 0, 10]}]})).unwrap_or_default();
    let r = s.execute("sketch.project", &json!({"refs": [{"edge": [10, 0, 10]}, {"edge": [10, 20, 10]}]})).unwrap_or_default();
    assert_eq!(r["already_projected"].as_array().map(Vec::len), Some(1), "{r}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    matches(output_schema_for::<ProjectResult>(), &r);
    // A `value` that is not true or false is read as left out, and the check says so.
    let r = scene(ParamCheck::Warn).execute("sketch.auto_project", &json!({"value": "maybe"})).unwrap_or_default();
    assert_eq!(r["param_warnings"], json!(["`value` must be true or false"]), "{r}");
    matches(output_schema_for::<AutoProjectResult>(), &r);
    // The shape of the others.
    let mut s = scene(ParamCheck::Warn);
    let r = s.execute("sketch.isoparametric_curve", &json!({"face": [10, 10, 10]})).unwrap_or_default();
    let mut keys: Vec<_> = r.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    keys.sort();
    assert_eq!(keys, ["created", "curves", "link", "points", "wires"], "{r}");
    let r = s.execute("sketch.auto_project", &json!({"value": true})).unwrap_or_default();
    assert_eq!(r["auto_project"], json!(true));
    let r = s.execute("sketch.auto_project", &json!({})).unwrap_or_default();
    assert_eq!(r["auto_project"], json!(false), "no value toggles");
    let r = s.execute("sketch.fit_curves_to_section", &json!({"body": "Block"})).unwrap_or_default();
    assert!(r["curves"].is_array(), "{r}");
}

/// Breaking links: by `link`, by `entities` of both kinds (a curve, a point), and with both.
#[test]
fn break_link_replies_without_warnings() {
    let mut s = scene(ParamCheck::Warn);
    let linked = |s: &mut Session, p: Value| {
        let r = s.execute("sketch.project", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let l = &r["links"][0];
        let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
        (text(&l["link"]), text(&l["curves"][0]), text(&l["points"][0]))
    };
    let (l1, _, _) = linked(&mut s, json!({"refs": [{"edge": [10, 0, 10]}]}));
    let (_, c2, _) = linked(&mut s, json!({"refs": [{"edge": [10, 20, 10]}]}));
    let (l3, _, _) = linked(&mut s, json!({"refs": [{"edge": [20, 10, 10]}]}));
    let (_, _, p4) = linked(&mut s, json!({"refs": [{"vertex": [0, 0, 10]}]}));
    assert!(!c2.is_empty() && !p4.is_empty());
    let schema = output_schema_for::<SketchBreakLinkResult>();
    for p in [json!({"link": l1}), json!({"entities": c2}), json!({"entities": [p4], "link": l3})] {
        let r = s.execute("sketch.break_link", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
        assert_eq!(r["broken"].as_array().map(Vec::len), Some(p.as_object().map_or(0, |o| o.len())), "{p}: {r}");
    }
}

/// A key spelt wrong runs in warn and says so (the reply still matches), and strict refuses it.
#[test]
fn a_misspelt_key_is_reported_in_warn_and_refused_in_strict() {
    let cases = [
        ("sketch.project", json!({"refs": ["origin"], "projection_lnk": false})),
        ("sketch.intersect", json!({"refs": [{"body": "Block"}], "projectionlink": false})),
        ("sketch.include_3d", json!({"refs": [{"edge": [20, 10, 10]}], "projection_link": false})),
        ("sketch.project_to_surface", json!({"face": [10, 10, 10], "curves": [{"sketch": "Src", "curve": "source"}], "bdy": "Block"})),
        ("sketch.intersection_curve", json!({"a": {"body": "Block"}, "b": {"body": "Pin"}, "c": 1})),
        ("sketch.spun_profile", json!({"body": "Pin", "axis": "Z", "axes": "Y"})),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "directon": "v"})),
        ("sketch.fit_curves_to_section", json!({"body": "Block", "bodies": []})),
        ("sketch.auto_project", json!({"vlaue": false})),
    ];
    for (id, p) in cases {
        let plane = if id == "sketch.spun_profile" { "XZ" } else { "XY" };
        let r = scene_on(ParamCheck::Warn, plane).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let warnings = r["param_warnings"].as_array().unwrap_or_else(|| panic!("{id} {p}: {r}"));
        assert_eq!(warnings.len(), 1, "{id} {p}: {r}");
        assert!(warnings[0].as_str().is_some_and(|w| w.contains("unknown key")), "{id}: {r}");
        let error = error(ParamCheck::Strict, id, &p).unwrap_or_default();
        assert!(error.contains("unknown key"), "{id}: {error}");
    }
    let r = scene(ParamCheck::Warn).execute("sketch.auto_project", &json!({"vlaue": false})).unwrap_or_default();
    assert_eq!(r["param_warnings"], json!(["unknown key `vlaue`"]), "{r}");
    // The reply with its warnings still matches its schema.
    let schema = output_schema_for::<AutoProjectResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
}

/// Wrong-typed values the handlers read as left out: the typed field names them in warn (and
/// strict refuses), `null` is left out and gets no warning, and the reply is the same as with the
/// check off.
#[test]
fn values_read_as_left_out_are_reported_but_change_nothing() {
    let cases = [
        ("sketch.project_to_surface", json!({"face": [10, 10, 10], "curves": [{"sketch": "Src", "curve": "source"}], "body": 5}), true),
        ("sketch.project_to_surface", json!({"face": [10, 10, 10], "curves": [{"sketch": "Src", "curve": "source"}], "body": null}), false),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "body": 5}), true),
        ("sketch.isoparametric_curve", json!({"face": [10, 10, 10], "body": null}), false),
        ("sketch.auto_project", json!({"value": null}), false),
        ("sketch.auto_project", json!({"value": 1}), true),
    ];
    for (id, p, warns) in cases {
        let off = scene(ParamCheck::Off).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let mut warn = scene(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert_eq!(warn.get("param_warnings").is_some(), warns, "{id} {p}: {warn}");
        if let Some(o) = warn.as_object_mut() {
            o.remove("param_warnings");
        }
        assert_eq!(off, warn, "{id} {p}");
    }
    // break_link: `link` of the wrong type is left out, so `entities` alone does the work.
    let mut s = scene(ParamCheck::Warn);
    let r = s.execute("sketch.project", &json!({"refs": [{"edge": [10, 0, 10]}]})).unwrap_or_default();
    let curve = r["links"][0]["curves"][0].clone();
    let r = s.execute("sketch.break_link", &json!({"link": 5, "entities": [curve]})).unwrap_or_default();
    assert_eq!(r["broken"].as_array().map(Vec::len), Some(1), "{r}");
    assert_eq!(r["param_warnings"].as_array().map(Vec::len), Some(1), "{r}");
    let r = scene(ParamCheck::Warn).execute("sketch.break_link", &json!({"link": null}));
    assert!(r.is_err(), "nothing to break");
}
