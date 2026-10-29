//! Tests of the shared types and the checking machinery, and helpers for the per-module tests
//! (`features_tests.rs`, …).

use schemars::Schema;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::check::schema_keys;
use super::features::{BoxParams, ExtrudeParams};
use super::*;
use crate::Session;

fn parse<T: DeserializeOwned>(v: Value) -> Option<T> {
    serde_json::from_value(v).ok()
}

fn box_contract() -> TypedContract<BoxParams> {
    TypedContract::new()
}

fn messages(issues: &[Issue]) -> Vec<String> {
    issues.iter().map(|i| i.message.clone()).collect()
}

#[test]
fn expressions_accept_what_params_expr_accepts() {
    for v in [json!(5), json!(-2.5), json!("w / 2"), json!("  30 mm  "), json!(999_999_999.0)] {
        let want = crate::params::expr(&json!({ "v": v.clone() }), "v");
        assert_eq!(parse::<LengthExpr>(v.clone()).map(|e| e.0), want, "{v}");
        assert_eq!(parse::<NumberExpr>(v.clone()).map(|e| e.0), want, "{v}");
        assert!(want.is_some(), "{v}");
    }
    let long = "x".repeat(4097);
    // The cap is in bytes: 2049 two-byte characters are too long (the schema's maxLength counts
    // characters, so it can't say this).
    let wide = "é".repeat(2049);
    for v in [json!(true), json!(null), json!(""), json!("   "), json!(1e9), json!(-1e9), json!([1]), json!({}), json!(long), json!(wide)] {
        assert!(crate::params::expr(&json!({ "v": v.clone() }), "v").is_none(), "{v}");
        assert!(parse::<LengthExpr>(v.clone()).is_none(), "{v}");
        assert!(parse::<AngleExpr>(v.clone()).is_none(), "{v}");
        assert!(parse::<NumberExpr>(v.clone()).is_none(), "{v}");
    }
}

/// `AxisDir` accepts what `features::axis_dir_param` reads (X, Y or Z trimmed of what Rust's
/// `trim` removes, in any case, or a non-zero vector), and its schema agrees: its string pattern
/// allows exactly that whitespace (`conforms` doesn't read patterns), and a schema can't refuse
/// the zero vector. `ScaleFactors` takes exactly three numbers or expressions.
#[test]
fn axes_and_scale_factors_match_their_readers() {
    let root = schemars::schema_for!(AxisDir);
    let root = root.as_value();
    for v in [json!("X"), json!(" y "), json!("z"), json!("\u{85}Z\u{3000}"), json!([0, 0, 1]), json!([1, 2, 3]), json!({"x": 0, "y": 1, "z": 0})] {
        assert!(crate::cmd::axis_dir_form(&v).is_ok(), "{v}");
        assert!(parse::<AxisDir>(v.clone()).is_some(), "{v}");
        assert!(conforms(root, root, &v), "{v}");
    }
    for v in [json!("W"), json!("XY"), json!(""), json!("x\u{feff}"), json!([1, 2]), json!(null), json!(true), json!([0, 0, 0])] {
        assert!(crate::cmd::axis_dir_form(&v).is_err(), "{v}");
        assert!(parse::<AxisDir>(v.clone()).is_none(), "{v}");
        if !v.is_string() {
            assert_eq!(conforms(root, root, &v), v == json!([0, 0, 0]), "{v}");
        }
    }
    // The same class as the expression schema's, whose text `the_blank_pattern_matches_trim` pins.
    let ws = super::types::not_blank_pattern().trim_start_matches("[^").trim_end_matches(']').to_string();
    let pattern = root.pointer("/anyOf/0/pattern").and_then(Value::as_str);
    assert_eq!(pattern, Some(format!("^[{ws}]*[XYZxyz][{ws}]*$").as_str()));
    let root = schemars::schema_for!(ScaleFactors);
    let root = root.as_value();
    for v in [json!([1, 2, 3]), json!([1, "k / 2", 2.5])] {
        assert!(parse::<ScaleFactors>(v.clone()).is_some(), "{v}");
        assert!(conforms(root, root, &v), "{v}");
    }
    for v in [json!([1, 2]), json!([1, 2, 3, 4]), json!([1, true, 2]), json!([1, "", 2]), json!("2"), json!({})] {
        assert!(parse::<ScaleFactors>(v.clone()).is_none(), "{v}");
        assert!(!conforms(root, root, &v), "{v}");
    }
}

#[test]
fn points_accept_what_the_point_readers_accept() {
    assert!(parse::<Point3>(json!([1, 2, 3])).is_some());
    assert!(parse::<Point3>(json!({"x": 1, "y": 2, "z": 3, "label": "extra keys are fine"})).is_some());
    for v in [json!([1, 2]), json!([1, 2, 3, 4]), json!(["a", 2, 3]), json!(null), json!({"x": 1, "y": 2}), json!([1e9, 0, 0])] {
        assert!(parse::<Point3>(v.clone()).is_none(), "{v}");
    }
    assert!(parse::<Point2>(json!([1, 2])).is_some());
    assert!(parse::<Point2>(json!([1, 2, 3])).is_none());
    assert!(parse::<PointExpr3>(json!([1, "w / 2", 3])).is_some());
    assert!(parse::<PointExpr3>(json!({"x": "a", "y": 0, "z": "b + 1"})).is_some());
    for v in [json!([1, 2]), json!(null), json!([true, 1, 2]), json!({"x": 1, "y": 2}), json!("origin")] {
        assert!(parse::<PointExpr3>(v.clone()).is_none(), "{v}");
    }
}

#[test]
fn names_and_lists() {
    assert_eq!(parse::<StringList>(json!("A")).map(|l| l.0), Some(vec!["A".to_string()]));
    assert_eq!(parse::<StringList>(json!(["A", "B"])).map(|l| l.0), Some(vec!["A".to_string(), "B".to_string()]));
    let too_many: Vec<&str> = vec!["x"; 10_001];
    for v in [json!([1]), json!(5), json!(null), json!(too_many.clone())] {
        assert!(parse::<StringList>(v.clone()).is_none(), "{v}");
    }
    let c = TypedContract::<ExtrudeParams>::new();
    assert_eq!(messages(&c.check(&json!({"distance": 5, "targets": too_many}))), vec!["`targets` must have at most 10000 items"]);
    assert!(parse::<IdOrName>(json!(3)).is_some());
    assert!(parse::<IdOrName>(json!("Base")).is_some());
    assert!(parse::<IdOrName>(json!(null)).is_none());
}

#[test]
fn operations_match_operation_parse() {
    for s in [
        "new",
        "join",
        "cut",
        "intersect",
        "newbody",
        "newcomponent",
        "union",
        "add",
        "subtract",
        "difference",
        "intersection",
        "New Body",
        "NEW-body",
        "Cut",
    ] {
        assert!(parse::<OperationParam>(json!(s)).is_some(), "{s}");
        assert!(parse::<ExtrudeOperation>(json!(s)).is_some(), "{s}");
    }
    assert!(parse::<OperationParam>(json!("auto")).is_none(), "box takes no auto");
    assert!(parse::<ExtrudeOperation>(json!("AUTO")).is_some());
    assert!(parse::<ExtrudeOperation>(json!(" auto")).is_none(), "auto is matched without trimming");
    for v in [json!("bogus"), json!(5), json!(null)] {
        assert!(parse::<OperationParam>(v.clone()).is_none(), "{v}");
    }
}

#[test]
fn extrude_keywords() {
    assert!(parse::<DirectionParam>(json!("SYMMETRIC")).is_some());
    assert!(parse::<DirectionParam>(json!("flip")).is_some());
    assert!(parse::<DirectionParam>(json!(" symmetric")).is_none(), "not trimmed");
    assert!(parse::<ExtentKeyword>(json!("Through_All")).is_some());
    assert!(parse::<ExtentKeyword>(json!("to_next")).is_none());
    assert!(parse::<ConventionParam>(json!("total_distance")).is_some());
    assert!(parse::<ConventionParam>(json!("Total_Distance")).is_none(), "serde, case-sensitive");
    assert!(parse::<ThinSideParam>(json!(" Two-Sides ")).is_some(), "ThinSide::parse trims and maps '-'");
    assert!(parse::<ThinSideParam>(json!("middle")).is_none());
}

#[test]
fn profiles_match_features_profiles() {
    for v in [json!("all"), json!("ALL"), json!(0), json!([0, 1]), json!([{"point": [1, 2]}]), json!([["l1", "l2"]]), json!([{"curves": ["l1"]}])] {
        assert!(parse::<ProfilesParam>(v.clone()).is_some(), "{v}");
    }
    for v in [json!("none"), json!([]), json!([true]), json!(null), json!(-1), json!([0, {"point": [1, 2]}])] {
        assert!(parse::<ProfilesParam>(v.clone()).is_none(), "{v}");
    }
    let c = TypedContract::<ExtrudeParams>::new();
    let issue = |profiles: Value| messages(&c.check(&json!({"distance": 5, "profiles": profiles})));
    assert_eq!(issue(json!([])), vec!["`profiles` is empty"]);
    assert_eq!(issue(json!(1.5)), vec!["`profiles` must be \"all\", an index or a list"]);
    assert_eq!(issue(json!(vec![0; 10_001])), vec!["`profiles` must have at most 10000 items"]);
}

#[test]
fn unsupported_profile_objects_offer_valid_public_examples() {
    let contract = TypedContract::<ExtrudeParams>::new();
    for profiles in [
        json!({"type": "curves", "loops": [["c1"], ["c2"]]}),
        json!({"type": "curves", "loops": [["c1"]]}),
        json!({"type": "index", "index": 0}),
        json!({"type": "indices", "indices": [0]}),
        json!({"curves": ["c1"]}),
        json!({}),
    ] {
        let issues = contract.check(&json!({"distance": 5, "profiles": profiles}));
        assert_eq!(issues.len(), 1, "{profiles}: {issues:?}");
        let issue = issues.first().unwrap();
        assert_eq!(issue.path, "profiles");
        assert!(!issue.message.contains("offset_boundaries"), "{profiles}: {}", issue.message);
        // The diagnostic is useful to scripts only if its public examples actually parse.
        let examples: Vec<Value> = issue.message.split('`').skip(1).step_by(2).filter_map(|text| serde_json::from_str(text).ok()).collect();
        assert_eq!(examples.len(), 5, "{}", issue.message);
        for example in examples {
            let issues = contract.check(&json!({"distance": 5, "profiles": example}));
            assert!(issues.is_empty(), "example {example}: {issues:?}");
        }
    }
    for profiles in [
        json!({"offset_boundaries": ["a"]}),
        json!({"type": "offset_boundaries", "associations": ["a"]}),
        json!({"type": "curves", "offset_boundaries": ["a"]}),
    ] {
        assert!(contract.check(&json!({"distance": 5, "profiles": profiles})).is_empty());
    }
}

#[test]
fn unsupported_profile_objects_keep_the_design_in_every_check_mode() {
    let mut session = Session::default();
    session.execute("sketch.create", &json!({"plane": "xy", "name": "Ring"})).unwrap();
    session.execute("sketch.circle.center", &json!({"center": [0, 0], "radius": 10})).unwrap();
    session.execute("sketch.circle.center", &json!({"center": [0, 0], "radius": 6})).unwrap();
    session.execute("sketch.finish", &json!({})).unwrap();
    let design = session.doc.to_json();
    let undo = session.undo.len();
    for mode in [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict] {
        session.param_check = mode;
        for profiles in [json!({"type": "curves", "loops": [["c1"], ["c2"]]}), json!({"type": "index", "index": 0})] {
            let error = session
                .execute("solid.extrude", &json!({"sketch": "Ring", "distance": 3, "operation": "new", "profiles": profiles}))
                .unwrap_err()
                .to_string();
            assert!(error.contains("\"all\"") && error.contains("curves"), "{mode:?}: {error}");
            assert!(!error.contains("offset_boundaries"), "{mode:?}: {error}");
            assert_eq!(session.doc.to_json(), design);
            assert_eq!(session.undo.len(), undo);
        }
    }
}

#[test]
fn unknown_keys_get_suggestions() {
    let c = box_contract();
    let issues = c.check(&json!({"length": 5, "width": 5, "height": 5, "operaton": "cut", "colour": "red"}));
    assert_eq!(messages(&issues), vec!["unknown key `colour`", "unknown key `operaton`; did you mean `operation`?"]);
    assert_eq!(issues.iter().find(|i| i.path == "operaton").and_then(|i| i.suggestion.clone()), Some("operation".into()));
    // A hostile key is cut in the message.
    let issues = c.check(&json!({"length": 5, "width": 5, "height": 5, "k".repeat(100_000): 1}));
    assert_eq!(messages(&issues), vec![format!("unknown key `{}…`", "k".repeat(64))]);
    // Short keys get a suggestion only one edit away.
    let issues = TypedContract::<ExtrudeParams>::new().check(&json!({"distance": 5, "x": 1, "p0": 1, "ids": 1, "sid": "inside"}));
    assert_eq!(messages(&issues), vec!["unknown key `ids`", "unknown key `p0`", "unknown key `sid`; did you mean `side`?", "unknown key `x`"]);
}

#[test]
fn shape_problems_are_issues() {
    let c = box_contract();
    assert_eq!(messages(&c.check(&json!({"width": 5, "height": 5}))), vec!["`length` is required"]);
    assert_eq!(messages(&c.check(&json!({"length": true, "width": 5, "height": 5}))), vec!["`length` must be a number or an expression"]);
    // A number out of range or an over-long expression says which limit it broke (#4712).
    assert_eq!(messages(&c.check(&json!({"length": 1e9, "width": 5, "height": 5}))), vec!["`length` must be a number between -1e9 and 1e9"]);
    assert_eq!(messages(&c.check(&json!({"length": -2e12, "width": 5, "height": 5}))), vec!["`length` must be a number between -1e9 and 1e9"]);
    assert_eq!(
        messages(&c.check(&json!({"length": "1+".repeat(2049) + "1", "width": 5, "height": 5}))),
        vec!["`length` must be an expression of at most 4096 characters"]
    );
    assert_eq!(
        messages(&c.check(&json!({"length": 5, "width": 5, "height": 5, "corner": null}))),
        vec!["`corner` must be a point [x, y, z] of numbers or length expressions"]
    );
    assert_eq!(messages(&c.check(&json!({"length": 5, "width": 5, "height": 5, "name": 7}))), vec!["`name` must be text"]);
    let e = TypedContract::<ExtrudeParams>::new();
    assert_eq!(messages(&e.check(&json!({"distance": 5, "thin": "true"}))), vec!["`thin` must be true or false"]);
    // serde quotes the caller's value: split at the last ", expected ", and cap the message.
    assert_eq!(messages(&e.check(&json!({"distance": 5, "through_all": "x, expected y"}))), vec!["`through_all` must be true or false"]);
    let long = messages(&e.check(&json!({"distance": 5, "through_all": format!("{}, expected y", "a".repeat(100_000))})));
    assert_eq!(long, vec!["`through_all` must be true or false"]);
    assert_eq!(messages(&c.check(&json!([1, 2, 3]))), vec!["parameters must be an object"]);
    assert!(c.check(&json!(null)).is_empty(), "null is read as {{}}, which the handler then judges");
    let many: serde_json::Map<String, Value> = (0..70).map(|i| (format!("k{i}"), json!(1))).collect();
    assert!(messages(&c.check(&Value::Object(many))).iter().any(|m| m.starts_with("too many parameters")));
}

#[test]
fn present_null_is_refused_but_absent_is_fine() {
    let base = json!({"length": 5, "width": 5, "height": 5});
    assert!(box_contract().check(&base).is_empty());
    let c: TypedContract<ExtrudeParams> = TypedContract::new();
    assert!(c.check(&json!({"distance": 5})).is_empty());
    // `body` picks the placed instance `face` is read in (frames::to_local), so it is a key.
    assert!(c.check(&json!({"distance": 5, "face": [0, 0, 0], "body": "Part"})).is_empty());
    for key in ["sketch", "profiles", "faces", "convention", "to", "from", "thin"] {
        let mut p = json!({"distance": 5});
        p[key] = Value::Null;
        assert_eq!(c.check(&p).len(), 1, "{key}: null");
    }
    // Keys today's reader treats `null` like absence stay optional.
    for key in ["taper", "direction", "face", "operation", "name"] {
        let mut p = json!({"distance": 5});
        p[key] = Value::Null;
        assert!(c.check(&p).is_empty(), "{key}: null");
    }
}

pub(super) fn sketch_rectangle(s: &mut Session) {
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.rectangle.center", json!({"center": [0, 0], "corner": [10, 10]})),
        ("sketch.finish", json!({})),
    ] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id}: {e}");
        }
    }
}

fn features(s: &Session) -> usize {
    s.doc.features.len()
}

#[test]
fn off_runs_the_handler_untouched() {
    let mut s = Session::default();
    assert_eq!(s.param_check, ParamCheck::Off);
    let r = s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "operaton": "cut"}));
    let r = r.unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_none());
    assert_eq!(r["created_bodies"], json!(["Body1"]), "the typo is ignored, as before: a new body");
}

#[test]
fn warn_runs_and_reports() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let r = s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "operaton": "cut"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r["param_warnings"], json!(["unknown key `operaton`; did you mean `operation`?"]));
    assert_eq!(features(&s), 1);
    let r = s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_none(), "a clean call carries no warnings");
}

#[test]
fn strict_refuses_and_leaves_the_design_alone() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let e = s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "operaton": "cut"})).err().map(|e| e.to_string());
    assert_eq!(e.as_deref(), Some("invalid parameters for `solid.box`: unknown key `operaton`; did you mean `operation`?"));
    assert_eq!(features(&s), 0);
    sketch_rectangle(&mut s);
    let before = features(&s);
    let e = s.execute("solid.extrude", &json!({"distance": 5, "tapper": "10 deg"})).err().map(|e| e.to_string());
    assert_eq!(e.as_deref(), Some("invalid parameters for `solid.extrude`: unknown key `tapper`; did you mean `taper`?"));
    let e = s.execute("solid.extrude", &json!({"distance": 5, "operation": 5})).err().map(|e| e.to_string());
    assert!(e.as_deref().is_some_and(|e| e.contains("`operation` must be one of")), "{e:?}");
    assert_eq!(features(&s), before);
    s.execute("solid.extrude", &json!({"distance": 5, "direction": "symmetric"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(features(&s), before + 1);
}

#[test]
fn nested_runs_are_checked_too() {
    let mut s = Session::default();
    sketch_rectangle(&mut s);
    s.execute("solid.extrude", &json!({"distance": 5, "name": "Slab"})).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = ParamCheck::Strict;
    let e = s
        .execute("timeline.redefine", &json!({"feature": "Slab", "command": "solid.extrude", "params": {"distance": 8, "directon": "symmetric"}}))
        .err()
        .map(|e| e.to_string());
    assert!(e.as_deref().is_some_and(|e| e.contains("unknown key `directon`; did you mean `direction`?")), "{e:?}");
}

#[test]
fn the_level_survives_a_new_design() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("file.new", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.param_check, ParamCheck::Strict);
    assert_eq!(s.scratch().param_check, ParamCheck::Strict);
}

/// Whether `v` conforms to `node` of the schema `root`, for the keywords these schemas use
/// apart from `pattern`: `type`, `properties`, `required`, `additionalProperties: false`,
/// `items`, `minItems`, `maxItems`, `uniqueItems`, `enum`, `minLength`, `maxLength`, `minimum`,
/// `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `anyOf`, `not` and `$ref`. (`integer`
/// here is what serde reads as one, so `1.0` is not.)
pub(super) fn conforms(root: &Value, node: &Value, v: &Value) -> bool {
    if let Some(name) = node.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/")) {
        return root.get("$defs").and_then(|d| d.get(name)).is_some_and(|t| conforms(root, t, v));
    }
    if let Some(branches) = node.get("anyOf").and_then(Value::as_array)
        && !branches.iter().any(|b| conforms(root, b, v))
    {
        return false;
    }
    if node.get("not").is_some_and(|n| conforms(root, n, v)) {
        return false;
    }
    let is = |t: &str| match t {
        "null" => v.is_null(),
        "boolean" => v.is_boolean(),
        "integer" => v.is_u64() || v.is_i64(),
        "number" => v.is_number(),
        "string" => v.is_string(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        _ => false,
    };
    let typed = match node.get("type") {
        Some(Value::String(t)) => is(t),
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).any(is),
        _ => true,
    };
    if !typed {
        return false;
    }
    if let (Some(items), Some(a)) = (node.get("items"), v.as_array())
        && !a.iter().all(|x| conforms(root, items, x))
    {
        return false;
    }
    let bound = |k: &str| node.get(k).and_then(Value::as_u64).map(|n| n as usize);
    if let Some(a) = v.as_array() {
        let unique = a.iter().enumerate().all(|(i, x)| !a.iter().skip(i + 1).any(|y| y == x));
        if bound("minItems").is_some_and(|n| a.len() < n)
            || bound("maxItems").is_some_and(|n| a.len() > n)
            || (node.get("uniqueItems") == Some(&json!(true)) && !unique)
        {
            return false;
        }
    }
    if let Some(s) = v.as_str() {
        let n = s.chars().count();
        if bound("minLength").is_some_and(|m| n < m) || bound("maxLength").is_some_and(|m| n > m) {
            return false;
        }
    }
    if let Some(x) = v.as_f64() {
        let limit = |k: &str| node.get(k).and_then(Value::as_f64);
        if limit("minimum").is_some_and(|m| x < m)
            || limit("maximum").is_some_and(|m| x > m)
            || limit("exclusiveMinimum").is_some_and(|m| x <= m)
            || limit("exclusiveMaximum").is_some_and(|m| x >= m)
        {
            return false;
        }
    }
    if let Some(values) = node.get("enum").and_then(Value::as_array)
        && !values.contains(v)
    {
        return false;
    }
    if let Some(o) = v.as_object() {
        let props = node.get("properties").and_then(Value::as_object);
        let mut required = node.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        if !required.all(|k| o.contains_key(k)) {
            return false;
        }
        for (k, x) in o {
            match props.and_then(|p| p.get(k)) {
                Some(p) if !conforms(root, p, x) => return false,
                None if node.get("additionalProperties") == Some(&json!(false)) => return false,
                _ => {}
            }
        }
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_recorder_keeps_a_bounded_number_of_signatures() {
    use super::record::tally;
    let mut seen = std::collections::HashMap::new();
    // Lines are due at counts 1, 2, 4, …
    assert_eq!(tally(&mut seen, "a".into(), 2), Some((false, 1)));
    assert_eq!(tally(&mut seen, "a".into(), 2), Some((false, 2)));
    assert_eq!(tally(&mut seen, "a".into(), 2), None);
    assert_eq!(tally(&mut seen, "b".into(), 2), Some((false, 1)));
    // Full: new signatures are counted under one entry; known ones still count.
    assert_eq!(tally(&mut seen, "c".into(), 2), Some((true, 1)));
    assert_eq!(tally(&mut seen, "d".into(), 2), Some((true, 2)));
    assert_eq!(tally(&mut seen, "a".into(), 2), Some((false, 4)));
    assert_eq!(seen.len(), 3);
}

/// The profiles schema and the contract agree on every listed case; the schema is deliberately
/// narrower on non-text curve ids and loops past the outer one (and describes the app's
/// `whole_boundaries` selection only by its `type`).
#[test]
fn the_profiles_schema_matches_the_contract() {
    let root = input_schema_for::<ExtrudeParams>();
    let root = root.as_value();
    let node = &root["properties"]["profiles"];
    let accepted = |v: &Value| parse::<ProfilesParam>(v.clone()).is_some();
    for v in [
        json!(3),
        json!(-1),
        json!([]),
        json!([0, 1]),
        json!([0, {"point": [1, 2]}]),
        json!([{"point": [1, 2]}, {"point": [3, 4]}]),
        json!([{"point": [1e9, 2]}]),
        json!([["l1"]]),
        json!([[]]),
        json!([{"curves": "l1"}, ["l2"]]),
        json!([{"curves": []}]),
        json!([{"loops": []}]),
        json!([{"loops": [5]}]),
        json!([{"loops": [{"curves": ["l1"]}]}]),
        json!({}),
        json!({"offset_boundaries": ["a"]}),
        json!({"offset_boundaries": []}),
        json!({"offset_boundaries": ["a", "a"]}),
        json!({"type": "offset_boundaries", "associations": ["a", "b"]}),
        json!({"type": "something"}),
        serde_json::from_str("18446744073709551616").unwrap_or_default(),
        // The helper's precedence: all items with `point` are points; `curves` before `loops`;
        // `offset_boundaries` before `type`.
        json!([{"point": null, "curves": ["l1"]}]),
        json!([{"point": [1, 2], "curves": ["l1"]}]),
        json!([{"curves": [], "loops": [{"curves": ["l1"]}]}]),
        json!([{"point": [0, 0], "curves": ["l1"]}, ["l2"]]),
        json!([{"point": [1, 2], "curves": ["a"]}, {"curves": ["b"]}]),
        json!([{"point": [0, 0], "loops": [{"curves": ["l1"]}]}, {"curves": "l2"}]),
        json!({"type": "offset_boundaries", "associations": ["a"], "offset_boundaries": []}),
    ] {
        assert_eq!(conforms(root, node, &v), accepted(&v), "{v}");
    }
    // An index is bounded to what the helper reads as a u64. (`conforms` refuses 2^64 above on
    // type alone, so the bound itself is checked here.)
    let index = |b: &Value| b.get("type") == Some(&json!("integer"));
    let bounds: Vec<&Value> = root["$defs"]["Profiles"]["anyOf"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|b| [b, &b["items"]])
        .filter(|b| index(b))
        .map(|b| &b["maximum"])
        .collect();
    assert_eq!(bounds, [&json!(u64::MAX), &json!(u64::MAX)]);
    // Where the schema is deliberately narrower: non-text ids and loops past the outer one.
    for v in [json!([["l1", 5]]), json!([{"loops": [{"curves": ["l1"]}, 5]}])] {
        assert!(accepted(&v) && !conforms(root, node, &v), "{v}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn recorder_lines_have_the_documented_shape() {
    let l = super::record::line("solid.box", &["unknown key `x`"], "ok", "a_test", 4, &json!({"x": "y".repeat(5000)}));
    let keys: Vec<&str> = l.as_object().into_iter().flat_map(|o| o.keys().map(String::as_str)).collect();
    assert_eq!(keys, ["command", "count", "issues", "outcome", "sample_params", "source"]);
    assert_eq!((l["command"].clone(), l["count"].clone(), l["outcome"].clone()), (json!("solid.box"), json!(4), json!("ok")));
    assert!(l["sample_params"].as_str().is_some_and(|s| s.len() <= 1024 + "…".len() && s.ends_with('…')), "{l}");
    // A strict refusal is told apart from a handler's error.
    use super::record::outcome_text;
    assert_eq!(outcome_text(None), "refused");
    assert_eq!(outcome_text(Some(&Ok(json!({})))), "ok");
    assert_eq!(outcome_text(Some(&Err(crate::EngineError::Other("boom".into())))), "error: boom");
    // The recorder is on exactly when built with `param-audit` and the variable names a directory.
    let set = std::env::var_os("SOLIDCRAFT_PARAM_AUDIT").is_some_and(|v| !v.is_empty());
    assert_eq!(super::record::enabled(), cfg!(feature = "param-audit") && set);
}

/// A contract's input schema lists exactly `fields` (sorted) as its keys, refuses other keys
/// and declares draft 2020-12. Each module's `schemas_and_the_key_check_agree` calls this for
/// its contracts, so their keys change only on purpose. (Whether the keys are the ones the
/// handler reads is what the audit run checks.)
pub(super) fn assert_keys(schema: Schema, fields: &[&str]) {
    let v = schema.as_value();
    assert_eq!(schema_keys(v).into_iter().collect::<Vec<_>>(), fields);
    assert_eq!(v.get("additionalProperties"), Some(&json!(false)), "the schema refuses unknown keys too");
    assert_eq!(v.get("$schema").and_then(Value::as_str), Some("https://json-schema.org/draft/2020-12/schema"));
}

/// A call with one problem that already fails gets the same error in strict as with the check
/// off, apart from messages that differ on purpose. Chiefly: a missing key; a value the handler
/// reads as missing where its message for a missing key says something else (a wrong type, a
/// list none of whose items is text, blank text, a number 1e9 or more in size or text over 4 KB
/// for an optional expression, an `extent` other than through_all); a non-object, a profile
/// index, a list over 10,000 items, a message quoting nearly 4 KB of the caller's text or more
/// (strict caps it); and a scale `factors` item that is blank, 1e9 or more in size, over 4 KB,
/// or not a number or an expression (see `ScaleFactors`). A call with more than one problem can
/// get another one named: strict and the handler look at the keys in different orders
/// (strict's comes from `TypedContract::check` and serde), and the handler may stop first at
/// one the contract can't see (against the design: a pick that finds nothing, a missing sketch,
/// an expression that doesn't evaluate; one of two keys, such as `radius` or `diameter`; an
/// empty list).
/// Each module's `strict_keeps_todays_errors` calls this with its commands' cases; each runs
/// after `sketch_rectangle`.
pub(super) fn assert_strict_keeps_todays_errors(cases: &[(&str, Value)]) {
    for (id, p) in cases {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            sketch_rectangle(&mut s);
            s.execute(id, p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// Every `default` in the input schemas is a value its own property schema allows.
#[test]
fn schema_defaults_are_valid() {
    let contracts: Vec<_> = crate::cmd::command_specs().into_iter().filter_map(|c| c.contract).collect();
    assert!(contracts.len() >= 3);
    for c in contracts {
        let root = c.input_schema();
        let root = root.as_value();
        for (key, property) in root["properties"].as_object().into_iter().flatten() {
            if let Some(d) = property.get("default") {
                assert!(conforms(root, property, d), "`{key}`: default {d} doesn't match {property}");
            }
        }
    }
}

/// The expression schema's "not blank" pattern uses exactly the whitespace Rust's `trim` does.
#[test]
fn the_blank_pattern_matches_trim() {
    let listed = |c: char| super::types::WHITESPACE.iter().any(|&(a, b)| (a..=b).contains(&c));
    for c in (0..=0x10_ffff).filter_map(char::from_u32) {
        assert_eq!(listed(c), c.is_whitespace(), "U+{:04X}", c as u32);
    }
    assert_eq!(super::types::not_blank_pattern(), r"[^\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028-\u2029\u202f\u205f\u3000]");
    assert!(parse::<LengthExpr>(json!("\u{85}")).is_none() && parse::<LengthExpr>(json!("\u{feff}")).is_some());
}

/// A handler-worded message for a key the handler ignores stays bounded in a warning.
#[test]
fn long_values_in_handler_messages_are_capped() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    sketch_rectangle(&mut s);
    let r = s.execute("solid.extrude", &json!({"distance": 5, "side": "x".repeat(1_000_000)})).unwrap_or_else(|e| panic!("{e}"));
    let w = r["param_warnings"][0].as_str().unwrap_or_default();
    assert!(w.starts_with("unknown side `xxx") && w.len() <= 4096 + "…".len(), "{} bytes", w.len());
}

/// The pick types accept what the pick readers accept. (Comparing a type with the helper it
/// calls only guards against the two drifting apart; the schema comparisons and the handler
/// tests below are the real checks.)
#[test]
fn face_and_edge_refs_match_the_pick_readers() {
    let long = "f".repeat(513);
    // 171 three-byte characters are 513 bytes, too long for the reader; the schema's maxLength
    // counts characters, so it can't say this.
    let wide = "面".repeat(171);
    let face_schema = input_schema_for::<FaceRef>();
    let face_schema = face_schema.as_value();
    let edge_schema = input_schema_for::<EdgeRef>();
    let edge_schema = edge_schema.as_value();
    for v in [
        json!([1, 2, 3]),
        json!({"x": 1, "y": 2, "z": 3}),
        json!({"point": [1, 2, 3]}),
        json!({"point": {"x": 1, "y": 2, "z": 3}}),
        json!({"x": "a", "point": [1, 2, 3]}),
        json!("F3:1"),
        json!(""),
        json!(long),
        json!([1, 2]),
        json!([1e9, 0, 0]),
        json!({"point": [1, 2]}),
        json!({"body": "A", "index": 0}),
        json!({"body": "A", "index": u64::MAX}),
        json!({"body": "A", "index": -1}),
        json!({"body": 5, "index": 0}),
        json!({"body": "A"}),
        json!({"point": "x", "body": "A", "index": 2}),
        json!(null),
        json!(5),
        json!(true),
    ] {
        let face = crate::cmd::face_ref_form(&v).is_ok();
        assert_eq!(parse::<FaceRef>(v.clone()).is_some(), face, "{v}");
        assert_eq!(conforms(face_schema, face_schema, &v), face, "face {v}");
        let edge = crate::cmd::edge_ref_form(&v).is_ok();
        assert_eq!(parse::<EdgeRef>(v.clone()).is_some(), edge, "{v}");
        assert_eq!(conforms(edge_schema, edge_schema, &v), edge, "edge {v}");
    }
    // Where the schemas can't follow the readers: a name's cap is in bytes, and an index of
    // `1.0`, which the reader refuses, is an integer to JSON Schema (`conforms` reads integers
    // as serde does, so it agrees with the reader here; a standard validator would not).
    assert!(parse::<FaceRef>(json!(wide)).is_none() && conforms(face_schema, face_schema, &json!(wide)));
    assert!(parse::<EdgeRef>(json!({"body": "A", "index": 1.0})).is_none());
    assert_eq!(edge_schema["anyOf"][2]["properties"]["index"]["type"], "integer");
    assert_eq!(parse::<EdgeRef>(json!({"body": "A", "index": 4})), Some(EdgeRef::Index { body: "A".into(), index: 4 }));
    assert_eq!(parse::<FaceRef>(json!({"point": [1, 2, 3]})), Some(FaceRef::Point(solidcraft_geom::Vec3::new(1.0, 2.0, 3.0))));
}

/// The pick lists take 1 to 1000 picks, as `face_points` and `edge_points` do, and so do their
/// schemas.
#[test]
fn pick_lists_match_the_list_readers() {
    let faces = input_schema_for::<FaceRefs>();
    let faces = faces.as_value();
    let edges = input_schema_for::<EdgeRefs>();
    let edges = edges.as_value();
    let point = json!([1, 2, 3]);
    for (v, accepted) in [
        (json!([point]), true),
        (json!(vec![point.clone(); 1000]), true),
        (json!([point, {"point": [4, 5, 6]}]), true),
        (json!([]), false),
        (json!(vec![point.clone(); 1001]), false),
        (json!([point, [1, 2]]), false),
        (json!(point), false),
        (json!(null), false),
        (json!("F1"), false),
    ] {
        assert_eq!(parse::<FaceRefs>(v.clone()).is_some(), accepted, "{v}");
        assert_eq!(conforms(faces, faces, &v), accepted, "faces {v}");
        assert_eq!(parse::<EdgeRefs>(v.clone()).is_some(), accepted, "{v}");
        assert_eq!(conforms(edges, edges, &v), accepted, "edges {v}");
    }
    // Names are faces, not edges; `{body, index}` is an edge, not a face.
    let mixed = json!(["F1", {"body": "A", "index": 0}]);
    assert!(parse::<FaceRefs>(json!(["F1"])).is_some() && parse::<EdgeRefs>(json!(["F1"])).is_none());
    assert!(parse::<FaceRefs>(mixed.clone()).is_none() && !conforms(faces, faces, &mixed));
    assert_eq!(parse::<EdgeRefs>(json!([{"body": "A", "index": 0}])).map(|e| e.0.len()), Some(1));
}

#[test]
fn sketch_refs_match_sketch_id() {
    let schema = input_schema_for::<SketchRef>();
    let schema = schema.as_value();
    for v in [json!(3), json!(2.5), json!(-1), json!("Profile"), json!(""), json!(null), json!(true), json!([1]), json!({"id": 1})] {
        let sketch = crate::cmd::sketch_key(Some(&v));
        assert_eq!(parse::<SketchRef>(v.clone()).map(|s| s.0), sketch, "{v}");
        assert_eq!(conforms(schema, schema, &v), sketch.is_some(), "{v}");
    }
}

/// A contract built from the shared types, as a converted feature command's would be: the pick
/// keys refuse `null`, as their readers do.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code)]
struct Probe {
    radius: LengthExpr,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "FaceRefs")]
    faces: Option<FaceRefs>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "EdgeRefs")]
    edges: Option<EdgeRefs>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "SketchRef")]
    path_sketch: Option<SketchRef>,
    #[serde(flatten, deserialize_with = "feature_common")]
    common: FeatureCommon,
}

#[test]
fn feature_common_flattens_into_a_contract() {
    let c = TypedContract::<Probe>::new();
    assert_keys(input_schema_for::<Probe>(), &["body_name", "body_names", "edges", "faces", "name", "participants", "path_sketch", "radius"]);
    let ok = c.check(&json!({"radius": 1, "name": "Round", "body_name": "B", "body_names": ["B", "C"], "participants": "A"}));
    assert!(ok.is_empty(), "{ok:?}");
    assert!(c.check(&json!({"radius": 1, "name": null, "participants": null})).is_empty(), "null is absent, as for `str_`");
    let parsed: Option<Probe> = parse(json!({"radius": 1, "name": "Round", "participants": ["A", "B"]}));
    assert_eq!(parsed.map(|p| p.common.participants), Some(Some(StringList(vec!["A".into(), "B".into()]))));
    // Messages and paths name the key, as for the parent's own fields.
    for (p, path, message) in [
        (json!({"radius": 1, "name": 5}), "name", "`name` must be text"),
        (json!({"radius": 1, "body_name": [1]}), "body_name", "`body_name` must be text"),
        (json!({"radius": 1, "participants": 5}), "participants", "`participants` must be a name or a list of names"),
        (json!({"radius": 1, "body_names": vec!["x"; 10_001]}), "body_names", "`body_names` must have at most 10000 items"),
        (json!({"radius": 1, "bodyname": "x"}), "bodyname", "unknown key `bodyname`; did you mean `body_name`?"),
        (json!({"radius": 1, "faces": null}), "faces", "`faces` must be a list of points or face names"),
        (json!({"radius": 1, "edges": null}), "edges", "`edges` must be a list"),
        (json!({"radius": 1, "path_sketch": null}), "path_sketch", "`path_sketch` must be a sketch id or name"),
        (json!({"name": "x"}), "radius", "`radius` is required"),
        // The flattened keys are read after the parent's own, whatever their order.
        (json!({"name": 5}), "radius", "`radius` is required"),
        (json!({"body_name": 5, "radius": true}), "radius", "`radius` must be a number or an expression"),
    ] {
        let issues = c.check(&p);
        assert_eq!(messages(&issues), vec![message], "{p}");
        assert_eq!(issues.first().map(|i| i.path.as_str()), Some(path), "{p}");
    }
}

/// A flattened `FeatureCommon`'s errors are located below wherever its parent is.
#[test]
fn feature_common_errors_keep_their_place() {
    #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Outer {
        options: Probe,
        #[serde(default)]
        list: Vec<Probe>,
    }
    let c = TypedContract::<Outer>::new();
    let probe = json!({"radius": 1});
    for (p, path, message) in [
        (json!({"options": {"radius": 1, "name": 5}}), "options.name", "`options.name` must be text"),
        (
            json!({"options": probe, "list": [probe, {"radius": 1, "participants": 5}]}),
            "list[1].participants",
            "`list[1].participants` must be a name or a list of names",
        ),
        (json!({"options": probe, "list": [{"radius": 1, "faces": []}]}), "list[0].faces", "select 1…1000 faces"),
    ] {
        let issues = c.check(&p);
        assert_eq!(messages(&issues), vec![message], "{p}");
        assert_eq!(issues.first().map(|i| i.path.as_str()), Some(path), "{p}");
    }
}

/// The pick types refuse what the handlers refuse, with the handlers' messages.
#[test]
fn shared_types_keep_the_handlers_messages() {
    let c = TypedContract::<Probe>::new();
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    sketch_rectangle(&mut s);
    for (id, p, probe) in [
        ("solid.shell", json!({"thickness": 1, "faces": 5}), json!({"faces": 5})),
        ("solid.shell", json!({"thickness": 1, "faces": null}), json!({"faces": null})),
        ("solid.shell", json!({"thickness": 1, "faces": []}), json!({"faces": []})),
        ("solid.shell", json!({"thickness": 1, "faces": [true]}), json!({"faces": [true]})),
        ("solid.shell", json!({"thickness": 1, "faces": [""]}), json!({"faces": [""]})),
        ("solid.fillet", json!({"radius": 1, "edges": {}}), json!({"edges": {}})),
        ("solid.fillet", json!({"radius": 1, "edges": vec![[0, 0, 0]; 1001]}), json!({"edges": vec![[0, 0, 0]; 1001]})),
        ("solid.fillet", json!({"radius": 1, "edges": [[1, 2]]}), json!({"edges": [[1, 2]]})),
        ("solid.fillet", json!({"radius": 1, "edges": [{"body": "Body1", "index": "0"}]}), json!({"edges": [{"body": "Body1", "index": "0"}]})),
        ("solid.sweep", json!({"path_sketch": true, "path": ["l1"]}), json!({"path_sketch": true})),
        ("solid.sweep", json!({"path_sketch": null, "path": ["l1"]}), json!({"path_sketch": null})),
    ] {
        let mut probe = probe;
        probe["radius"] = json!(1);
        let issue = messages(&c.check(&probe)).into_iter().next().unwrap_or_default();
        let today = s.execute(id, &p).err().map(|e| e.to_string());
        assert_eq!(today, Some(crate::params::bad(id, issue).to_string()), "{id} {p}");
    }
}

/// The lookups that follow the shape checks the pick types share with the handlers: picks
/// by `{body, index}` and sketches by name still find the design, and still fail in order, one
/// pick at a time.
#[test]
fn picks_are_still_looked_up_one_at_a_time() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let error = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
    let fillet = |edges: Value| json!({"radius": 1, "edges": edges});
    assert_eq!(
        error(&mut s, "solid.fillet", fillet(json!([{"body": "Nope", "index": 0}, [1, 2]]))),
        "invalid parameters for `solid.fillet`: no body `Nope`"
    );
    assert_eq!(
        error(&mut s, "solid.fillet", fillet(json!([{"body": "Body1", "index": 999}]))),
        "invalid parameters for `solid.fillet`: Body1 has no edge 999"
    );
    let far = error(&mut s, "solid.fillet", fillet(json!([[100, 100, 100], [1, 2]])));
    assert!(far.starts_with("invalid parameters for `solid.fillet`: no edge at [100, 100, 100]"), "{far}");
    let unknown = error(&mut s, "solid.shell", json!({"thickness": 1, "faces": ["NoSuchFace"]}));
    assert!(!unknown.is_empty());
    assert_eq!(error(&mut s, "solid.shell", json!({"thickness": 1, "faces": ["NoSuchFace", true]})), unknown, "the name is looked up first");
    sketch_rectangle(&mut s);
    assert_eq!(
        error(&mut s, "solid.sweep", json!({"path_sketch": "Nope", "path": ["l1"]})),
        "invalid parameters for `solid.sweep`: no sketch `Nope`"
    );
    assert_eq!(error(&mut s, "solid.extrude", json!({"distance": 5, "sketch": "Nope"})), "invalid parameters for `solid.extrude`: no sketch `Nope`");
    let r = s.execute("solid.fillet", &fillet(json!([{"body": "Body1", "index": 0}]))).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r["changed_bodies"], json!(["Body1"]), "{r}");
}
