use serde_json::{Value, json};
use solidcraft_io::MAX_DRAWING_BYTES;

use super::{DrawingInserted, ExportDxfParams, ExportDxfResult, InsertDxfParams, InsertSvgParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A 30 x 20 rectangle as DXF lines.
const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n30\n21\n0\n0\nLINE\n10\n30\n20\n0\n11\n30\n21\n20\n0\nLINE\n10\n30\n20\n20\n11\n0\n21\n20\n0\nLINE\n10\n0\n20\n20\n11\n0\n21\n0\n0\nENDSEC\n0\nEOF\n";

const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><rect x="0" y="0" width="20" height="10"/><text x="2" y="5" font-size="3">Hi</text></svg>"#;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-12508-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn schemas_and_the_key_check_agree() {
    let insert = ["align", "angle", "at", "height", "name", "path", "plane", "scale", "sketch", "text", "tolerance", "width"];
    assert_keys(input_schema_for::<InsertDxfParams>(), &insert);
    assert_keys(input_schema_for::<InsertSvgParams>(), &insert);
    assert_keys(input_schema_for::<ExportDxfParams>(), &["join", "layer", "layers", "path", "sketch", "units"]);
}

/// Strict gives the handlers' messages for the calls they refuse for a key (a missing `path` and
/// `text` included: the handler's words, not "is required"). The session has a sketch being edited.
#[test]
fn strict_keeps_todays_errors() {
    let too_big = "x".repeat(MAX_DRAWING_BYTES + 1);
    let mut cases: Vec<(&str, Value)> = vec![
        ("sketch.insert_dxf", json!({})),
        ("sketch.insert_dxf", json!({"path": ""})),
        ("sketch.insert_dxf", json!({"path": "  "})),
        ("sketch.insert_dxf", json!({"path": 5})),
        ("sketch.insert_dxf", json!({"path": null})),
        ("sketch.insert_dxf", json!({"path": "x".repeat(4096)})),
        ("sketch.insert_dxf", json!({"text": too_big})),
        ("sketch.insert_svg", json!({})),
        ("sketch.insert_svg", json!({"path": " "})),
        ("sketch.insert_svg", json!({"text": null})),
        ("sketch.export_dxf", json!({"sketch": true})),
        ("sketch.export_dxf", json!({"sketch": [1]})),
        ("sketch.export_dxf", json!({"sketch": -1})),
        ("sketch.export_dxf", json!({"sketch": 1.5})),
    ];
    for (id, text) in [("sketch.insert_dxf", DXF), ("sketch.insert_svg", SVG)] {
        for extra in [
            json!({"width": 0}),
            json!({"width": -2}),
            json!({"width": null}),
            json!({"width": "wide"}),
            json!({"width": [1]}),
            json!({"height": 0}),
            json!({"height": null}),
            json!({"height": true}),
            json!({"angle": "x"}),
            json!({"angle": null}),
            json!({"angle": {}}),
            json!({"align": "left"}),
            json!({"align": "Center"}),
            json!({"align": ""}),
            json!({"sketch": true}),
            json!({"sketch": null, "width": 0}),
            json!({"sketch": [1]}),
            json!({"sketch": 1.5}),
        ] {
            let mut p = json!({"text": text});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            cases.push((id, p));
        }
    }
    assert_strict_keeps_todays_errors(&cases);
}

/// The same refusals for a sketch that is named but doesn't exist, a width the drawing can't
/// give (a drawing with no width to size is refused by the handler alone), and the export's
/// options, which the handler reads only once a sketch is being edited.
#[test]
fn strict_keeps_the_handlers_own_refusals() {
    // A vertical line has no width to size.
    let vertical = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n0\n21\n9\n0\nENDSEC\n0\nEOF\n";
    for (id, p) in [
        ("sketch.insert_dxf", json!({"text": DXF, "sketch": "Nope"})),
        ("sketch.insert_dxf", json!({"text": DXF, "sketch": 9999})),
        ("sketch.insert_dxf", json!({"text": vertical, "width": 10})),
        ("sketch.insert_dxf", json!({"text": "0\nEOF\n"})),
        ("sketch.export_dxf", json!({"sketch": "Nope"})),
        ("sketch.export_dxf", json!({"sketch": 9999})),
        ("sketch.export_dxf", json!({"units": 5})),
        ("sketch.export_dxf", json!({"units": "Yard"})),
        ("sketch.export_dxf", json!({"layer": 1})),
        ("sketch.export_dxf", json!({"layers": "all"})),
        ("sketch.export_dxf", json!({"layers": true})),
        ("sketch.export_dxf", json!({"join": "yes"})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// The types read each key as the handler does: keys it never reads in a call are not read by the
/// contract either, and the ones it reads fail with its words.
#[test]
fn the_types_read_like_the_handler() {
    let reads = |p: Value| serde_json::from_value::<InsertDxfParams>(p).is_ok();
    let issues = |p: Value| crate::schema::issues("sketch.insert_svg", &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    // `path` is not read when `text` is given; `height` not when `width` is; `scale` not when
    // either is.
    assert!(reads(json!({"text": "x", "path": 5})) && reads(json!({"text": "x", "path": ""})));
    assert!(reads(json!({"width": 5, "height": "x", "scale": "y"})) && reads(json!({"width": 5, "height": null})));
    assert!(reads(json!({"height": 5, "scale": "y"})) && reads(json!({"height": 5, "scale": null})));
    assert!(reads(json!({"path": "a.dxf", "scale": 2, "tolerance": 0.01, "at": [1, 2], "angle": 90, "align": "center"})));
    assert!(reads(json!({"text": "x", "at": {"x": 1, "y": 2}, "angle": 720.5, "width": 1e12, "scale": 0, "tolerance": -1})));
    // They are read when the handler reads them.
    // Stricter than the handler, which reads a `text` that isn't text as left out.
    assert!(!reads(json!({"text": 5, "path": "a.dxf"})) && !reads(json!({"path": "a.dxf", "scale": "y"})));
    assert!(!reads(json!({"path": ""})) && !reads(json!({"path": 5, "scale": 1})));
    assert!(!reads(json!({"text": "x", "height": 0})) && !reads(json!({"text": "x", "height": "x", "scale": 2})));
    assert!(!reads(json!({"text": "x", "width": null, "height": 5})) && !reads(json!({"text": "x", "scale": "y", "width": 0})));
    // Only while no sketch is being edited does `plane` or `name` matter, which the check can't
    // see: they take anything.
    assert!(reads(json!({"text": "x", "plane": 5, "name": [1]})) && reads(json!({"text": "x", "plane": null, "name": null})));
    assert_eq!(issues(json!({"text": "x", "width": 0})), ["`width` must be a positive number (mm)"]);
    assert_eq!(issues(json!({"text": "x", "height": -1})), ["`height` must be a positive number (mm)"]);
    assert_eq!(issues(json!({"text": "x", "angle": "1"})), ["`angle` must be a number (degrees)"]);
    assert_eq!(issues(json!({"text": "x", "align": "left"})), ["`align` must be \"origin\" or \"center\""]);
    assert_eq!(issues(json!({"path": " "})), ["give `path` or `text`"]);
    assert_eq!(issues(json!({"text": "x", "sketch": true})), ["`sketch` must be an id or a name"]);
    assert_eq!(issues(json!({"text": "x", "wdth": 5})), ["unknown key `wdth`; did you mean `width`?"]);
    assert!(issues(json!({"text": "x", "width": 5, "height": "x", "path": 5})).is_empty());
    let export = |p: Value| serde_json::from_value::<ExportDxfParams>(p).is_ok();
    assert!(export(json!({})) && export(json!({"sketch": null, "path": null})) && export(json!({"sketch": "S", "path": ""})));
    assert!(export(json!({"sketch": 3, "path": "a.dxf"})) && !export(json!({"sketch": true})) && !export(json!({"path": 5})));
}

/// What the schemas take: a drawing from `path` or `text`, and the keys' own shapes.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    for schema in [input_schema_for::<InsertDxfParams>(), input_schema_for::<InsertSvgParams>()] {
        takes(
            schema,
            &[
                (json!({"text": "x"}), true),
                (json!({"path": "a.svg", "at": [1, 2], "scale": 2, "angle": 45, "align": "center", "tolerance": 0.01}), true),
                (json!({"text": "x", "width": 40, "plane": "XZ", "name": "Logo", "sketch": "Sketch1"}), true),
                (json!({"text": "x", "plane": {"origin": [0, 0, 1], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "sketch": 4}), true),
                (json!({}), false),
                (json!({"width": 40}), false),
                (json!({"text": "x", "width": 0}), false),
                (json!({"text": "x", "height": -1}), false),
                (json!({"text": "x", "angle": "x"}), false),
                (json!({"text": "x", "align": "left"}), false),
                (json!({"text": "x", "at": [1]}), false),
                (json!({"text": "x", "wdth": 5}), false),
                (json!({"path": "x".repeat(4096)}), false),
                (json!({"text": "x", "path": " "}), true),
            ],
        );
    }
    // A `path` read without `text` is not blank (`conforms` doesn't check patterns).
    for schema in [input_schema_for::<InsertDxfParams>(), input_schema_for::<InsertSvgParams>()] {
        let read_path = &schema.as_value()["anyOf"][1]["properties"]["path"];
        assert_eq!(read_path["pattern"], json!(crate::schema::types::not_blank_pattern()));
    }
    takes(
        input_schema_for::<ExportDxfParams>(),
        &[
            (json!({}), true),
            (json!({"sketch": "S", "path": "a.dxf"}), true),
            (json!({"sketch": 2}), true),
            (json!({"sketch": true}), false),
            (json!({"file": "a.dxf"}), false),
        ],
    );
}

fn matches(schema: schemars::Schema, r: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
}

/// Every form of the three commands replies as its output schema says, in Warn without warnings;
/// the replies carry the keys they always had; a misspelt key gets its warning and the reply still
/// matches, and strict refuses it.
#[test]
fn replies_match_the_output_schema() {
    let dir = temp_dir("replies");
    let svg_file = dir.join("plate.svg");
    let dxf_file = dir.join("plate.dxf");
    std::fs::write(&svg_file, SVG).unwrap();
    std::fs::write(&dxf_file, DXF).unwrap();
    let (svg_path, dxf_path) = (svg_file.to_string_lossy().into_owned(), dxf_file.to_string_lossy().into_owned());
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        r
    };
    // The session adds `created` (the ids a sketch command made) to every `sketch.*` reply, outside
    // the handlers; it is not part of what they build.
    let keys = |r: &Value| {
        r.as_object().map(|o| o.keys().filter(|k| *k != "created").cloned().collect::<std::collections::BTreeSet<_>>()).unwrap_or_default()
    };
    let insert_keys: std::collections::BTreeSet<String> = ["created_sketch", "curves", "info", "sketch", "texts"].map(String::from).into();

    // With no sketch being edited: a new one is made, on the plane and with the name given.
    let r = run(&mut s, "sketch.insert_dxf", json!({"text": DXF, "plane": "XZ", "name": "Plate", "at": [5, 5]}));
    matches(output_schema_for::<DrawingInserted>(), &r);
    assert_eq!(keys(&r), insert_keys, "{r}");
    assert_eq!((r["curves"].as_u64(), r["texts"].as_u64()), (Some(4), Some(0)), "{r}");
    assert_eq!(r["created_sketch"], r["sketch"], "{r}");
    // Exactly what the handler returned before it built its reply from the type.
    let mut handler_reply = r.clone();
    handler_reply.as_object_mut().unwrap().remove("created");
    assert_eq!(
        handler_reply,
        json!({"sketch": r["sketch"], "created_sketch": r["sketch"], "curves": 4, "texts": 0, "info": {"solved": true, "dof": 8}})
    );
    assert!(r["info"]["solved"].is_boolean() && r["info"]["dof"].is_u64(), "{r}");
    // With one: into it, and `plane` and `name` (of any kind) are not read.
    let id = s.active_sketch.unwrap();
    for p in [
        json!({"text": DXF, "path": 5, "plane": 7, "name": [1]}),
        json!({"text": DXF, "width": 60, "height": "x", "scale": "y", "angle": 90, "align": "center", "at": {"x": 1, "y": 2}}),
        json!({"text": DXF, "height": 10, "scale": "y", "tolerance": 0.01}),
        json!({"text": DXF, "scale": 2, "align": "origin", "sketch": id}),
        json!({"text": DXF, "sketch": s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default()}),
        json!({"path": dxf_path, "text": null, "sketch": null}),
    ] {
        let r = run(&mut s, "sketch.insert_dxf", p);
        matches(output_schema_for::<DrawingInserted>(), &r);
        assert_eq!(keys(&r), insert_keys);
        assert_eq!((r["sketch"].as_u64(), r["created_sketch"].clone(), r["curves"].as_u64()), (Some(id), Value::Null, Some(4)), "{r}");
    }
    let r = run(&mut s, "sketch.insert_svg", json!({"text": SVG, "angle": 15}));
    matches(output_schema_for::<DrawingInserted>(), &r);
    assert_eq!((r["curves"].as_u64(), r["texts"].as_u64()), (Some(4), Some(1)), "{r}");
    let r = run(&mut s, "sketch.insert_svg", json!({"path": svg_path, "width": 40, "align": "center", "at": [10, 10]}));
    assert_eq!(r["texts"], 1, "{r}");
    s.execute("sketch.finish", &json!({})).unwrap();
    let r = run(&mut s, "sketch.insert_svg", json!({"path": svg_path, "plane": "YZ"}));
    matches(output_schema_for::<DrawingInserted>(), &r);
    assert_ne!(r["created_sketch"], Value::Null, "{r}");
    s.execute("sketch.finish", &json!({})).unwrap();

    // Export: the text, or a file.
    let r = run(&mut s, "sketch.export_dxf", json!({"sketch": id}));
    matches(output_schema_for::<ExportDxfResult>(), &r);
    assert_eq!(keys(&r), ["dxf", "entities"].map(String::from).into(), "{r}");
    assert!(r["dxf"].as_str().is_some_and(|t| t.contains("ENTITIES")), "{r}");
    assert!(r["entities"].as_u64().is_some_and(|n| n >= 4), "{r}");
    let out = dir.join("out.dxf");
    let out_path = out.to_string_lossy().into_owned();
    let r = run(&mut s, "sketch.export_dxf", json!({"sketch": id, "path": out_path}));
    matches(output_schema_for::<ExportDxfResult>(), &r);
    assert_eq!(r["path"], json!(out_path), "{r}");
    assert_eq!(keys(&r), ["entities", "path"].map(String::from).into(), "{r}");
    assert!(std::fs::read_to_string(&out).unwrap().contains("ENTITIES"));
    // A blank `path` and a null `sketch` read as left out.
    s.execute("sketch.edit", &json!({"sketch": id})).unwrap();
    let r = run(&mut s, "sketch.export_dxf", json!({"sketch": null, "path": "  "}));
    assert!(r.get("dxf").is_some(), "{r}");
    matches(output_schema_for::<ExportDxfResult>(), &r);
    // The units and layer options, in any case; null and a blank layer read as left out.
    for p in [
        json!({"units": "IN", "layer": "PART"}),
        json!({"units": "design", "layers": "Roles"}),
        json!({"join": true, "layer": "  "}),
        json!({"units": null, "layer": null, "layers": null, "join": null}),
    ] {
        let r = run(&mut s, "sketch.export_dxf", p);
        matches(output_schema_for::<ExportDxfResult>(), &r);
        assert_eq!(keys(&r), ["dxf", "entities"].map(String::from).into(), "{r}");
    }
    let _ = std::fs::remove_dir_all(&dir);

    // A misspelt key runs in warn and says so; the reply still matches. Strict refuses it.
    let p = json!({"text": DXF, "wdth": 5});
    let r = s.execute("sketch.insert_dxf", &p).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| w.iter().any(|m| m.as_str().is_some_and(|m| m.contains("`wdth`")))), "{r}");
    matches(output_schema_for::<DrawingInserted>(), &r);
    let r = s.execute("sketch.export_dxf", &json!({"pth": "a.dxf"})).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    matches(output_schema_for::<ExportDxfResult>(), &r);
    let r = s.execute("sketch.insert_svg", &json!({"text": SVG, "angel": 5})).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("sketch.insert_dxf", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`wdth`") && error.contains("did you mean `width`"), "{error}");
}
