use serde_json::{Value, json};

use super::{
    AppearanceAssignParams, AppearanceAssignResult, AppearanceCopyParams, AppearanceCopyResult, AppearanceLibraryParams, AppearanceLibraryResult,
    AppearanceListParams, AppearanceListResult, AppearancePaintParams, AppearancePaintResult, AppearancePickUpParams, AppearancePickUpResult,
    ChainTarget,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, issues, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<AppearanceAssignParams>(),
        &["appearance", "bodies", "body", "color", "components", "faces", "name", "opacity", "pbr"],
    );
    assert_keys(input_schema_for::<AppearanceLibraryParams>(), &[]);
    assert_keys(input_schema_for::<AppearanceListParams>(), &["body"]);
    assert_keys(input_schema_for::<AppearancePaintParams>(), &["appearance", "color", "done", "name", "opacity", "pbr"]);
    assert_keys(input_schema_for::<AppearancePickUpParams>(), &["target"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("appearance.assign", json!({"faces": [[1, 2]]})),
        ("appearance.assign", json!({"faces": [{"body": "Box1"}]})),
        ("appearance.assign", json!({"color": "red"})),
        ("appearance.assign", json!({"color": [0, 128, 300]})),
        ("appearance.assign", json!({"color": "#ff0000", "name": ""})),
        ("appearance.assign", json!({"color": "#ff0000", "pbr": {"metallic": 1}})),
        ("appearance.assign", json!({"color": "#ff0000", "pbr": {"metallic": "1", "roughness": 0.5, "dielectric_ior": 1.5}})),
        ("appearance.assign", json!({"color": "#ff0000", "pbr": {"metallic": 2, "roughness": 0.5, "dielectric_ior": 1.5}})),
        // The paint request reads its look through the same parser, so its refusals are the same.
        ("appearance.paint", json!({"color": "red"})),
        ("appearance.paint", json!({"color": "#ff0000", "name": ""})),
        ("appearance.paint", json!({"color": "#ff0000", "pbr": {"metallic": 1}})),
        // A name the library doesn't have is the handler's to refuse, in both modes.
        ("appearance.paint", json!({"appearance": "not in the library"})),
        // A target of the right shape that names no body is the handler's to refuse too.
        ("appearance.pick_up", json!({"target": {"body": "NoSuchBody"}})),
    ]);
}

/// A library look, a starter recipe, a colour with PBR on a face, a cleared body, the library
/// and the list with and without `body`, in Warn: each reply matches its output schema and
/// carries no warning; a misspelt key's warning still matches.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: schemars::Schema, id: &str, reply: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    };
    let assigned = |reply: &Value| {
        assert!(reply.get("param_warnings").is_none(), "{reply}");
        matches(output_schema_for::<AppearanceAssignResult>(), "appearance.assign", reply);
    };
    let made = run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
    let body = made["created_bodies"][0].as_str().unwrap_or_default().to_string();
    let library = run("appearance.library", json!({}));
    assert!(library.get("param_warnings").is_none(), "{library}");
    matches(output_schema_for::<AppearanceLibraryResult>(), "appearance.library", &library);
    let entries = library["appearances"].as_array().cloned().unwrap_or_default();
    let named = |pick: &dyn Fn(&Value) -> bool| entries.iter().find(|e| pick(e)).and_then(|e| e["name"].as_str()).unwrap_or_default().to_string();
    let plain = named(&|e| e.get("id").is_none() && e.get("pbr").is_none());
    let finish = named(&|e| e.get("id").is_none() && e.get("pbr").is_some());
    let starter = entries.iter().find(|e| e["available"] == true).and_then(|e| e["id"].as_str()).unwrap_or_default().to_string();
    assert!(!plain.is_empty() && !finish.is_empty() && !starter.is_empty(), "{library}");
    assigned(&run("appearance.assign", json!({"bodies": [body], "appearance": plain, "opacity": 0.5})));
    assigned(&run("appearance.assign", json!({"bodies": body, "appearance": finish})));
    assigned(&run("appearance.assign", json!({"bodies": [body], "appearance": starter})));
    let face = run(
        "appearance.assign",
        json!({"faces": [{"point": [5, 5, 10], "body": body}], "color": [200, 40, 40], "name": "Signal red",
               "pbr": {"metallic": 0, "roughness": 0.4, "dielectric_ior": 1.5}}),
    );
    assert_eq!(face["faces"][0]["point"], json!([5.0, 5.0, 10.0]), "{face}");
    assigned(&face);
    let all = run("appearance.list", json!({}));
    assert!(all.get("param_warnings").is_none() && all.get("body").is_none(), "{all}");
    matches(output_schema_for::<AppearanceListResult>(), "appearance.list", &all);
    let one = run("appearance.list", json!({"body": body}));
    assert_eq!(one["body"]["faces"].as_array().map(Vec::len), Some(1), "{one}");
    matches(output_schema_for::<AppearanceListResult>(), "appearance.list", &one);
    let cleared = run("appearance.assign", json!({"bodies": [body]}));
    assert!(cleared["appearance"].is_null(), "{cleared}");
    assigned(&cleared);
    let warned = run("appearance.list", json!({"bodyy": body}));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches(output_schema_for::<AppearanceListResult>(), "appearance.list", &warned);
}

/// A colour's text pattern allows what `inspect::color_param` reads: spaces at either end, as
/// `trim` drops them, any number of `#` before the three bytes, and a byte written as `+` and
/// one digit, as `from_str_radix` reads it. `conforms` doesn't read patterns, so the pattern's
/// text is pinned here, next to the reader's answers.
#[test]
fn the_colour_pattern_allows_what_the_reader_trims() {
    for text in ["#ff00ff", "ff00ff", "  #FF00ff\t", "##ff00ff"] {
        assert_eq!(crate::cmd::inspect::color_param(&json!(text)), Some([255, 0, 255]), "{text}");
    }
    assert_eq!(crate::cmd::inspect::color_param(&json!("# ff00ff")), None, "a space after the `#` isn't trimmed");
    assert_eq!(crate::cmd::inspect::color_param(&json!("+fff00")), Some([15, 255, 0]), "`from_str_radix` takes a leading `+`");
    let schema = schemars::schema_for!(super::ColorParam);
    let ws = crate::schema::types::whitespace_class();
    let pattern = format!("^[{ws}]*#*(?:[0-9A-Fa-f]{{2}}|\\+[0-9A-Fa-f]){{3}}[{ws}]*$");
    assert_eq!(schema.as_value()["anyOf"][0]["pattern"], json!(pattern), "{}", schema.as_value());
}

/// The chain's three target forms, as `cmd::appearance_chain::chain` reads them: the same objects
/// mean the same target, and an object it refuses comes back with the chain's own message.
#[test]
fn the_target_is_read_as_the_chain_reads_it() {
    let reads = |p: Value| serde_json::from_value::<ChainTarget>(p.clone()).ok();
    for (p, want) in [
        (json!({"body": "Box1"}), ChainTarget::Body("Box1".into())),
        (json!({"body": "Box1", "face": 2}), ChainTarget::Face { body: "Box1".into(), face: 2 }),
        (json!({"component": "K:2"}), ChainTarget::Component("K:2".into())),
        (json!({"component": 7}), ChainTarget::Component("7".into())),
    ] {
        assert_eq!(reads(p.clone()), Some(want), "{p}");
    }
    let refused = |p: Value| issues("appearance.pick_up", &json!({"target": p})).into_iter().map(|i| i.message).collect::<Vec<_>>();
    for (p, message) in [
        (json!("Box1"), "give one target object"),
        (json!([]), "give one target object"),
        (json!({}), "target must be {body}, {body, face}, or {component}"),
        (json!({"face": 1}), "target must be {body}, {body, face}, or {component}"),
        (json!({"body": "Box1", "extra": 1}), "target must be {body}, {body, face}, or {component}"),
        (json!({"body": "Box1", "component": "C"}), "target must be {body}, {body, face}, or {component}"),
        (json!({"body": ""}), "target name must have 1..4096 bytes"),
        (json!({"body": 5}), "target name must have 1..4096 bytes"),
        (json!({"body": "Box1", "face": -1}), "face must be a zero-based unsigned index"),
        (json!({"body": "Box1", "face": 1.5}), "face must be a zero-based unsigned index"),
        (json!({"component": -1}), "component must be a name or unsigned id"),
        (json!({"component": null}), "component must be a name or unsigned id"),
    ] {
        assert_eq!(refused(p.clone()), [message], "{p}");
        assert_eq!(reads(p.clone()), None, "{p}");
    }
}

/// What the two handlers read as nothing at all, the contract names: `done` as a boolean, and a
/// target that isn't an object. The pick-up's message is the one difference on purpose: today it
/// comes from the chain the request runs, strict names the command that was called.
#[test]
fn strict_names_the_command_that_was_called() {
    let error = |id: &str, p: Value, level: ParamCheck| {
        let mut s = Session { param_check: level, ..Session::default() };
        s.execute(id, &p).err().map(|e| e.to_string())
    };
    assert_eq!(error("appearance.paint", json!({"done": "yes"}), ParamCheck::Off), bad_params("appearance.paint", "choose a look first"));
    assert_eq!(error("appearance.paint", json!({"done": "yes"}), ParamCheck::Strict), bad_params("appearance.paint", "`done` must be true or false"));
    assert_eq!(error("appearance.pick_up", json!({"target": "Box1"}), ParamCheck::Off), bad_params("appearance.chain", "give one target object"));
    assert_eq!(
        error("appearance.pick_up", json!({"target": "Box1"}), ParamCheck::Strict),
        bad_params("appearance.pick_up", "give one target object")
    );
}

/// The engine's refusal for a call with bad parameters.
fn bad_params(cmd: &str, message: &str) -> Option<String> {
    Some(format!("invalid parameters for `{cmd}`: {message}"))
}

/// Picking a look up from a body, painting with what came back, ending the tool and asking the UI
/// to sample, in Warn: each reply matches its output schema and carries no warning, and a
/// misspelt key's warning still matches.
#[test]
fn the_gestures_answer_with_their_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: schemars::Schema, id: &str, reply: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    };
    let made = run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
    let body = made["created_bodies"][0].as_str().unwrap_or_default().to_string();
    run(
        "appearance.assign",
        json!({"bodies": [body], "color": "#345678", "name": "Sampled", "pbr": {"metallic": 0.2, "roughness": 0.3, "dielectric_ior": 1.6}}),
    );
    let picked = run("appearance.pick_up", json!({"target": {"body": body}}));
    assert_eq!(picked["look"]["name"], "Sampled", "{picked}");
    assert!(picked.get("param_warnings").is_none(), "{picked}");
    matches(output_schema_for::<AppearancePickUpResult>(), "appearance.pick_up", &picked);
    let painted = run("appearance.paint", picked["look"].clone());
    assert_eq!(painted["look"], picked["look"], "painting with a picked look keeps it");
    matches(output_schema_for::<AppearancePaintResult>(), "appearance.paint", &painted);
    let ended = run("appearance.paint", json!({"done": true}));
    assert_eq!(ended, json!({"done": true}), "ending the tool answers with nothing else");
    matches(output_schema_for::<AppearancePaintResult>(), "appearance.paint", &ended);
    let asked = run("appearance.pick_up", json!({}));
    assert_eq!(asked, json!({"tool": "pick_up"}), "no target asks the UI to sample one");
    matches(output_schema_for::<AppearancePickUpResult>(), "appearance.pick_up", &asked);
    let warned = run("appearance.pick_up", json!({"target": {"body": body}, "targt": 1}));
    assert_eq!(warned["look"]["name"], "Sampled", "{warned}");
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches(output_schema_for::<AppearancePickUpResult>(), "appearance.pick_up", &warned);
    let warned = run("appearance.paint", json!({"color": "#ff0000", "colour": "#00ff00"}));
    assert_eq!(warned["look"]["color"], "#ff0000", "{warned}");
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches(output_schema_for::<AppearancePaintResult>(), "appearance.paint", &warned);
}

/// Copying a look from a body and from one of its faces, in Warn: the look is the object the
/// pick-up answers with, the reply matches the published output schema, and what was copied is what
/// `appearance.paste` applies. The published input schema says the target is required, because a
/// copy has no ask-the-UI form.
#[test]
fn a_copied_look_answers_with_its_schema() {
    assert_keys(input_schema_for::<AppearanceCopyParams>(), &["target"]);
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: schemars::Schema, id: &str, reply: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    };
    let described = run("engine.describe", json!({"command": "appearance.copy"}));
    assert_eq!(described["input"]["required"], json!(["target"]), "a copy always names a target");
    assert_eq!(described["input"]["additionalProperties"], false);
    let made = run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
    let body = made["created_bodies"][0].as_str().unwrap_or_default().to_string();
    let pbr = json!({"metallic": 0.2, "roughness": 0.3, "dielectric_ior": 1.6});
    run("appearance.assign", json!({"bodies": [body], "color": "#345678", "name": "Copied", "pbr": pbr.clone()}));
    let copied = run("appearance.copy", json!({"target": {"body": body}}));
    assert_eq!(copied["look"], json!({"name": "Copied", "color": "#345678", "opacity": 1.0, "pbr": pbr}), "{copied}");
    assert!(copied.get("param_warnings").is_none(), "{copied}");
    matches(output_schema_for::<AppearanceCopyResult>(), "appearance.copy", &copied);
    // A face of that body reads the same look, and the clipboard is what paste applies.
    let face = run("appearance.copy", json!({"target": {"body": body, "face": 0}}));
    assert_eq!(face, copied, "a face with no look of its own follows its body");
    let other = run("solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [20, 0, 0]}));
    let other = other["created_bodies"][0].as_str().unwrap_or_default().to_string();
    run("appearance.paste", json!({"bodies": [other]}));
    let pasted = run("appearance.pick_up", json!({"target": {"body": other}}));
    assert_eq!(pasted["look"], copied["look"], "pasting applies the copied look");
    // A key the command doesn't take is a warning, not a refusal: the chain ignores it, as it does
    // with the pick-up.
    let warned = run("appearance.copy", json!({"target": {"body": body}, "targt": 1}));
    assert_eq!(warned["look"], copied["look"], "{warned}");
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches(output_schema_for::<AppearanceCopyResult>(), "appearance.copy", &warned);
}

/// A target the chain can't read keeps the chain's own message with the check off and in warn, which
/// is what it has always been; in strict the contract answers first and names the command that was
/// called. That is the one difference, and it is the same one #16481 documents for
/// `appearance.pick_up`.
#[test]
fn copy_refusals_stay_the_chains_below_strict() {
    let error = |p: Value, level: ParamCheck| {
        let mut s = Session { param_check: level, ..Session::default() };
        s.execute("appearance.copy", &p).err().map(|e| e.to_string())
    };
    for (target, message) in [
        (json!([]), "give one target object"),
        (json!({"body": "Box1", "face": "front"}), "face must be a zero-based unsigned index"),
        (json!({"body": "Box1", "other": 1}), "target must be {body}, {body, face}, or {component}"),
    ] {
        let p = json!({"target": target});
        for level in [ParamCheck::Off, ParamCheck::Warn] {
            assert_eq!(error(p.clone(), level), bad_params("appearance.chain", message), "{p}");
        }
        // The schema reads the target the same way, so only the command it names changes.
        assert_eq!(error(p.clone(), ParamCheck::Strict), bad_params("appearance.copy", message), "{p}");
    }
    // No target at all: refused at every level, by the chain below strict and by the schema in it.
    for level in [ParamCheck::Off, ParamCheck::Warn] {
        assert_eq!(error(json!({}), level), bad_params("appearance.chain", "give one target object"));
    }
    assert_eq!(error(json!({}), ParamCheck::Strict), bad_params("appearance.copy", "`target` is required"));
}
