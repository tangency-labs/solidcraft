use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{AnimationExported, ExportAnimationParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

const CMD: &str = "motion.export_animation";

/// Cube B (x 20…30) slides on joint `Rail` toward cube A (x 0…10), as in
/// `cmd/motion_tests.rs`: study `Push` takes it from 10 mm away until it touches A, study
/// `Through` 5 mm into A, and the exploded view `Apart` pulls the two apart.
fn rail() -> Session {
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: Value| {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    };
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = s.doc.find_component("A").and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, 0]]}]}));
    run(&mut s, "motion.study", json!({"name": "Through", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, -5]]}]}));
    run(&mut s, "explode.create", json!({"name": "Apart", "scale": 1}));
    s
}

/// A folder of its own for one test's files.
fn out_dir(test: &str) -> std::path::PathBuf {
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-anim-schema-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

#[test]
fn schemas_and_the_key_check_agree() {
    let keys = ["axes", "back", "camera", "fps", "from", "grid", "height", "index", "joint", "path", "samples", "study", "to", "view", "width"];
    assert_keys(input_schema_for::<ExportAnimationParams>(), &keys);
    let schema = input_schema_for::<ExportAnimationParams>();
    let schema = schema.as_value();
    assert_eq!(schema["required"], json!(["path"]));
    assert_eq!(schema["dependentRequired"], json!({"back": ["view"], "joint": ["from", "to"]}));
    assert_eq!(schema["oneOf"], json!([{"required": ["view"]}, {"required": ["study"]}, {"required": ["joint"]}]));
}

/// #13310: calls that fail today get the same message in strict: the handler's for `path`, the
/// frame size, `fps`, `grid` and `axes`, what moves (none, two of `view`, `study` and `joint`,
/// `back` without a view, a view, study or joint of the wrong shape, `from` or `to` left out or
/// of the wrong shape), `camera` (not an object, an unknown field, a field of the wrong shape or
/// out of range) and `samples` (also past what the frame size allows), and the handler's own for
/// what only the design can tell (a view, study or joint it lacks, `index` past the joint's
/// values, `to` that is no expression).
#[test]
fn strict_keeps_todays_errors() {
    let dir = out_dir("strict");
    let gif = dir.join("x.gif").to_string_lossy().to_string();
    let frames = dir.join("frames").to_string_lossy().to_string();
    let push = |extra: Value| {
        let mut p = json!({"study": "Push", "path": gif});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        p
    };
    let camera = |c: Value| push(json!({"camera": c}));
    for p in [
        json!({}),
        json!({"study": "Push", "path": null}),
        json!({"study": "Push", "path": ""}),
        json!({"study": "Push", "path": "  "}),
        json!({"study": "Push", "path": 5}),
        json!({"study": "Push", "path": "x".repeat(4097)}),
        json!({"study": "Push", "path": "a\u{1}b.gif"}),
        json!({"study": "Push", "path": "one.png"}),
        json!({"study": "Push", "path": "One.PNG"}),
        push(json!({"width": 15})),
        push(json!({"width": "800"})),
        push(json!({"width": null})),
        push(json!({"height": 1e9})),
        push(json!({"fps": 0})),
        push(json!({"fps": "20"})),
        push(json!({"grid": "no"})),
        push(json!({"axes": null})),
        json!({"path": gif}),
        json!({"path": gif, "view": "Apart", "study": "Push"}),
        json!({"path": gif, "view": "Apart", "joint": "Rail", "from": 0, "to": 1}),
        json!({"path": gif, "view": "Apart", "study": null}),
        json!({"path": gif, "view": "Apart", "back": "yes"}),
        json!({"path": gif, "view": [1]}),
        json!({"path": gif, "view": null}),
        json!({"path": gif, "view": "Nope"}),
        push(json!({"back": true})),
        push(json!({"back": false})),
        json!({"path": gif, "joint": "Rail", "from": 0, "to": 1, "back": null}),
        push(json!({"joint": "Rail"})),
        push(json!({"study": null})),
        push(json!({"study": [1]})),
        push(json!({"study": "Nope"})),
        json!({"path": gif, "joint": null}),
        json!({"path": gif, "joint": true, "from": 0, "to": 1}),
        json!({"path": gif, "joint": "Nope", "from": 0, "to": 1}),
        json!({"path": gif, "joint": "Rail"}),
        json!({"path": gif, "joint": "Rail", "from": 0}),
        json!({"path": gif, "joint": "Rail", "from": true, "to": 1}),
        json!({"path": gif, "joint": "Rail", "from": 0, "to": 2e9}),
        json!({"path": gif, "joint": "Rail", "from": 0, "to": null}),
        json!({"path": gif, "joint": "Rail", "from": 0, "to": "1 +"}),
        json!({"path": gif, "joint": "Rail", "index": 1, "from": 0, "to": 1}),
        json!({"path": gif, "joint": "Rail", "index": -1, "from": 0, "to": 1}),
        json!({"path": gif, "joint": "Rail", "index": "0", "from": 0, "to": 1}),
        camera(json!("iso")),
        camera(json!(null)),
        camera(json!({"fit": true})),
        camera(json!({"yaw": 1, "zoom": 2})),
        camera(json!({"target": [1, 2]})),
        camera(json!({"yaw": "a"})),
        camera(json!({"up": "x"})),
        camera(json!({"distance": -1})),
        camera(json!({"distance": 1e300})),
        camera(json!({"pitch": 4})),
        camera(json!({"fov": 3})),
        camera(json!({"fov": -0.1})),
        push(json!({"samples": 1})),
        push(json!({"samples": 1001})),
        push(json!({"samples": "10"})),
        push(json!({"samples": null})),
        // 800 × 500 frames: a GIF holds 167 of them; 4096 × 4096: 4, and a folder of PNG 64.
        push(json!({"samples": 168})),
        push(json!({"width": 4096, "height": 4096, "samples": 5})),
        json!({"study": "Push", "path": frames, "width": 4095.6, "height": 4096, "samples": 65}),
    ] {
        let error = |level: ParamCheck| {
            let mut s = rail();
            s.param_check = level;
            s.execute(CMD, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// #13310: the contract takes what the input schema allows: a call's check finds no issue
/// exactly when it matches the schema. Left out of this on purpose, as JSON Schema (or the
/// schema check here) can't tell them: what moves (one of `view`, `study` and `joint`, `back`
/// only with a view and `from` and `to` with a joint: `oneOf` and `dependentRequired`), a blank
/// or `.png` path, `samples` past what the frame size allows, and `index`, which the handler
/// checks against the joint's value count.
#[test]
fn the_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<ExportAnimationParams>();
    let schema = schema.as_value();
    let full_camera = json!({"target": [0, 0, 0], "yaw": 1, "pitch": -1.5, "roll": 0.2, "distance": 100, "fov": 0.5, "up": "y"});
    for (p, takes) in [
        (json!({"path": "a.gif", "study": "Push"}), true),
        (json!({"path": "frames", "study": 1, "width": 16, "height": 4096, "fps": 50, "grid": false, "axes": true, "samples": 2}), true),
        (json!({"path": "a.gif", "joint": "Rail", "from": 0, "to": "10 mm", "index": 0}), true),
        (json!({"path": "a.gif", "view": "Apart", "back": true, "samples": 81}), true),
        (json!({"path": "a.gif", "study": "Push", "camera": full_camera}), true),
        (json!({"path": "a.gif", "study": "Push", "camera": {}}), true),
        // Read only with a joint, as the handler reads them.
        (json!({"path": "a.gif", "study": "Push", "index": null, "from": [], "to": false}), true),
        (json!({"path": "", "study": "Push"}), false),
        (json!({"path": 5, "study": "Push"}), false),
        (json!({"path": "a.gif", "study": "Push", "width": 15}), false),
        (json!({"path": "a.gif", "study": "Push", "height": 4097}), false),
        (json!({"path": "a.gif", "study": "Push", "width": null}), false),
        (json!({"path": "a.gif", "study": "Push", "fps": 0.5}), false),
        (json!({"path": "a.gif", "study": "Push", "grid": 1}), false),
        (json!({"path": "a.gif", "study": [1]}), false),
        (json!({"path": "a.gif", "view": null}), false),
        (json!({"path": "a.gif", "view": "Apart", "back": "yes"}), false),
        (json!({"path": "a.gif", "joint": "Rail", "from": true, "to": 1}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": "iso"}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"fit": true}}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"target": [1, 2]}}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"yaw": "a"}}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"distance": 0}}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"distance": 2e15}}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"pitch": 2}}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"fov": 3}}), false),
        (json!({"path": "a.gif", "study": "Push", "camera": {"up": "x"}}), false),
        (json!({"path": "a.gif", "study": "Push", "samples": 1}), false),
        (json!({"path": "a.gif", "study": "Push", "samples": 1001}), false),
        (json!({"path": "a.gif", "study": "Push", "samples": null}), false),
        (json!({"path": "a.gif", "study": "Push", "colour": "red"}), false),
    ] {
        let issues = crate::schema::issues(CMD, &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// The keys of a reply.
fn keys(r: &Value) -> BTreeSet<&str> {
    r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect()
}

/// An export through the registry, in Warn: it replies as its output schema says, without
/// warnings.
fn exported(s: &mut Session, p: Value) -> Value {
    let schema = output_schema_for::<AnimationExported>();
    let schema = schema.as_value();
    let r = s.execute(CMD, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    r
}

/// #13310: the reply keeps the keys it had: a GIF of a study played (with its `fps`), a folder of
/// PNG frames of a joint driven (without), an exploded view coming apart and back, and a study
/// that a contact set stops short. The design is not changed.
#[test]
fn an_animation_replies_with_its_schema() {
    crate::capture::install_test_renderer();
    let mut s = rail();
    s.param_check = ParamCheck::Warn;
    let dir = out_dir("replies");
    let gif = dir.join("push.gif").to_string_lossy().to_string();
    let frames = dir.join("frames").to_string_lossy().to_string();
    let before = s.doc.clone();
    let plain: BTreeSet<&str> =
        ["path", "format", "frames", "width", "height", "bytes", "renderer", "stopped_at", "stopped_by_loop", "stopped_by_contact"].into();

    let played = exported(&mut s, json!({"study": "Push", "path": gif, "samples": 3, "width": 16, "height": 16, "fps": 25}));
    assert_eq!(keys(&played), plain.iter().copied().chain(["fps"]).collect::<BTreeSet<_>>(), "{played}");
    assert_eq!((&played["format"], &played["frames"], &played["fps"]), (&json!("gif"), &json!(3), &json!(25.0)), "{played}");
    assert_eq!((&played["stopped_at"], &played["stopped_by_contact"]), (&Value::Null, &Value::Null), "{played}");

    let driven = exported(&mut s, json!({"joint": "Rail", "from": 10, "to": 0, "path": frames, "samples": 2, "width": 16, "height": 16}));
    assert_eq!(keys(&driven), plain, "{driven}");
    assert_eq!((&driven["format"], &driven["frames"]), (&json!("png"), &json!(2)), "{driven}");

    let exploded = exported(&mut s, json!({"view": "Apart", "back": true, "path": gif, "samples": 3, "width": 16, "height": 16}));
    assert_eq!(keys(&exploded), plain.iter().copied().chain(["fps"]).collect::<BTreeSet<_>>(), "{exploded}");
    assert_eq!(*s.doc, *before, "the design is not changed");

    // B stops where it touches A, once the two are a contact set.
    s.execute("contact.create", &json!({"occurrences": ["A", "B"]})).unwrap_or_else(|e| panic!("{e}"));
    let stopped = exported(&mut s, json!({"study": "Through", "path": gif, "width": 16, "height": 16}));
    assert!(stopped["stopped_at"].is_u64() && stopped["stopped_by_contact"].is_object(), "{stopped}");
    let _ = std::fs::remove_dir_all(&dir);
}
