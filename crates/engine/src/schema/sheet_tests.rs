use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::CornerJoinParams;
use super::{
    BendTableFormatParam, BendTableParams, BendTableResult, ConvertParams, DxfCurvesParam, DxfLayersParam, DxfNoteHeight, DxfTextParam,
    DxfUnitsParam, ExportDxfParams, ExportDxfResult, FixedPoint, FlangeEdges, FlangeParams, FlangePosition, FlangeTypeParam, FlatPatternParams,
    FlatPatternResult, FoldParams, FoldPoints, FoldPosition, FoldStays, HardwareParams, HardwareResult, HeightFromParam, HemParams, MaterialSide,
    MeasureBodyParams, PathFlangeParams, PerforateParams, PerforationPattern, RecognizeParams, ReliefShapeParam, SheetEdgePoint, SheetPoints,
    TabParams, ThenSteps, UnfoldParams,
};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// The built-in rule's name, which a design without rules uses.
const STEEL: &str = "Steel (mm)";

/// A command's own keys and `FeatureCommon`'s, sorted.
fn keys(own: &[&'static str]) -> Vec<&'static str> {
    let mut k = own.to_vec();
    k.extend(["body_name", "body_names", "name", "participants"]);
    k.sort_unstable();
    k
}

/// A 100 × 60 sheet of the default rule (2.5 thick) from sketch `Base`, and a 100 × 60 × 2.5
/// box `Plate` beside it.
fn sheet() -> Session {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})),
        ("sketch.finish", json!({})),
        ("sheet.flange", json!({"sketch": "Base"})),
        ("solid.box", json!({"length": 100, "width": 60, "height": 2.5, "corner": [0, 100, 0], "body_name": "Plate"})),
    ] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id}: {e}");
        }
    }
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CornerJoinParams>(), &keys(&["body", "covered", "covering", "gap", "kind", "overlap"]));
    assert_keys(
        input_schema_for::<FlangeParams>(),
        &keys(&[
            "angle",
            "bend_relief_shape",
            "body",
            "corner_relief",
            "corner_relief_shape",
            "corners",
            "curves",
            "distance",
            "edges",
            "flip",
            "height",
            "height_from",
            "offset",
            "orientation",
            "position",
            "profiles",
            "radius",
            "reverse",
            "rule",
            "segments",
            "sketch",
            "symmetric",
            "then",
            "type",
        ]),
    );
    assert_keys(input_schema_for::<PerforateParams>(), &keys(&["body", "diameter", "margin", "pattern", "pitch", "profiles", "sketch"]));
    assert_keys(
        input_schema_for::<HardwareParams>(),
        &keys(&["diameter", "direction", "flip", "kind", "length", "points", "position", "single", "sketch", "targets", "thread"]),
    );
    assert_keys(input_schema_for::<HemParams>(), &keys(&["body", "edges", "flip", "gap", "length"]));
    assert_keys(
        input_schema_for::<FoldParams>(),
        &keys(&["angle", "body", "curve", "fixed", "flip", "points", "position", "radius", "sketch", "stays"]),
    );
    assert_keys(input_schema_for::<UnfoldParams>(), &keys(&["body", "fixed"]));
    assert_keys(input_schema_for::<ConvertParams>(), &keys(&["bends", "body", "direction", "engrave", "face", "faces", "rule", "skip"]));
    assert_keys(input_schema_for::<RecognizeParams>(), &keys(&["bends", "body", "face", "new_rule", "rule", "skip"]));
    assert_keys(input_schema_for::<MeasureBodyParams>(), &["body", "face"]);
    assert_keys(input_schema_for::<PathFlangeParams>(), &keys(&["body", "curves", "edge", "radius", "sketch"]));
    assert_keys(input_schema_for::<FlatPatternParams>(), &["body"]);
    assert_keys(input_schema_for::<BendTableParams>(), &["body", "format", "path"]);
    assert_keys(
        input_schema_for::<ExportDxfParams>(),
        &[
            "bend_lines",
            "bend_notes",
            "body",
            "curves",
            "hardware_notes",
            "layers",
            "markings",
            "note_height",
            "path",
            "per_body",
            "profile",
            "text",
            "units",
        ],
    );
}

fn corner_join_params(s: &mut Session) -> Value {
    s.execute("sheet.flange", &json!({"edges": [[50, 0, 0], [100, 30, 0]], "height": 20})).unwrap();
    let state = s.model.state();
    let surface = &state.surface_sheets[0];
    let body = state.body(&surface.body).unwrap();
    let sources: Vec<_> = (0..surface.sheet.joints.len()).filter_map(|joint| surface.bend_source_edge(joint, body).ok()).collect();
    assert_eq!(sources.len(), 2);
    json!({"covering": sources[0], "covered": sources[1], "name": "Joined"})
}

#[test]
fn flange_corner_join_matches_a_separate_join_using_central_build_names() {
    for (kind, overlap) in [("butt", None), ("partial_overlap", Some(1.0)), ("full_overlap", None)] {
        for swap in [false, true] {
            let mut expected = sheet();
            let mut corner = corner_join_params(&mut expected);
            corner.as_object_mut().unwrap().remove("name");
            if swap {
                let covering = corner["covering"].take();
                corner["covering"] = corner["covered"].take();
                corner["covered"] = covering;
            }
            corner["kind"] = json!(kind);
            corner["overlap"] = json!(overlap);
            corner["gap"] = json!(0.6);
            let mut actual = sheet();
            for key in ["covering", "covered"] {
                assert!(corner[key].as_str().unwrap().contains('|'));
            }
            let params = json!({"edges": [[50, 0, 0], [100, 30, 0]], "height": 20, "corners": [corner], "name": "Joined"});
            assert!(takes::<FlangeParams>(&params));
            actual.execute("sheet.flange", &params).unwrap();
            expected.execute("sheet.corner_join", &corner).unwrap();
            assert_eq!(
                serde_json::to_value(&actual.model.state().surface_sheets[0]).unwrap(),
                serde_json::to_value(&expected.model.state().surface_sheets[0]).unwrap(),
                "{params}"
            );
            let saved = serde_json::to_value(&actual.doc.find_feature("Joined").unwrap().kind).unwrap();
            assert_eq!(saved["corners"][0]["covering"], corner["covering"]);
            assert_eq!(saved["corners"][0]["covered"], corner["covered"]);
            assert_eq!(saved["corners"][0]["kind"], corner["kind"]);
            let reopened = Session::new(serde_json::from_value(serde_json::to_value(&actual.doc).unwrap()).unwrap());
            assert_eq!(
                serde_json::to_value(&reopened.model.state().surface_sheets).unwrap(),
                serde_json::to_value(&actual.model.state().surface_sheets).unwrap(),
                "cold rebuild: {params}"
            );
            let mut edited = sheet();
            let feature =
                edited.execute("sheet.flange", &json!({"edges": [[50, 0, 0], [100, 30, 0]], "height": 20, "name": "Joined"})).unwrap()["feature"]
                    .as_u64()
                    .unwrap();
            let next_id = edited.doc.next_id;
            edited.execute("timeline.redefine", &json!({"feature": feature, "command": "sheet.flange", "params": params})).unwrap();
            assert_eq!(edited.doc.next_id, next_id);
            assert_eq!(serde_json::to_value(&edited.doc.find_feature("Joined").unwrap().kind).unwrap(), saved, "redefined choices: {params}");
            assert_eq!(
                serde_json::to_value(&edited.model.state().surface_sheets).unwrap(),
                serde_json::to_value(&actual.model.state().surface_sheets).unwrap(),
                "redefined geometry: {params}"
            );
            for invalid in [json!({"kind": "partial_overlap", "overlap": 3.0}), json!({"covering": "F999:cap.surface|F999:bend.inner"})] {
                let mut invalid_params = params.clone();
                for (key, value) in invalid.as_object().unwrap() {
                    invalid_params["corners"][0][key] = value.clone();
                }
                let before = edited.doc.clone();
                assert!(
                    edited.execute("timeline.redefine", &json!({"feature": feature, "command": "sheet.flange", "params": invalid_params})).is_err(),
                    "{invalid_params}"
                );
                assert_eq!(*edited.doc, *before, "{invalid_params}");
                assert_eq!(
                    serde_json::to_value(&edited.model.state().surface_sheets).unwrap(),
                    serde_json::to_value(&actual.model.state().surface_sheets).unwrap(),
                    "refused redefine: {invalid_params}"
                );
            }
            actual.execute("sheet.unfold", &json!({})).unwrap();
            actual.execute("sheet.refold", &json!({})).unwrap();
        }
    }
}

#[test]
fn flange_corner_redefine_normalizes_bare_lengths_in_design_units() {
    let mut source = sheet();
    let mut corner = corner_join_params(&mut source);
    corner.as_object_mut().unwrap().remove("name");
    corner["kind"] = json!("partial_overlap");
    corner["overlap"] = json!(0.04);
    corner["gap"] = json!("0.5 mm");
    let base = json!({"edges": [[50, 0, 0], [100, 30, 0]], "height": "20 mm", "name": "Flanges"});
    let mut params = base.clone();
    params["corners"] = json!([corner]);
    let mut direct = sheet();
    direct.execute("document.units", &json!({"units": "in"})).unwrap();
    direct.execute("sheet.flange", &params).unwrap();
    let expected = serde_json::to_value(&direct.doc.find_feature("Flanges").unwrap().kind).unwrap();
    assert_eq!(expected["corners"][0]["overlap"], "0.04 in");
    assert_eq!(expected["corners"][0]["gap"], "0.5 mm");

    let mut edited = sheet();
    let feature = edited.execute("sheet.flange", &base).unwrap()["feature"].clone();
    edited.execute("document.units", &json!({"units": "in"})).unwrap();
    edited.execute("timeline.redefine", &json!({"feature": feature, "command": "sheet.flange", "params": params})).unwrap();
    let actual = serde_json::to_value(&edited.doc.find_feature("Flanges").unwrap().kind).unwrap();
    assert_eq!(actual["corners"], expected["corners"]);
}

#[test]
fn flange_corner_join_absent_null_and_empty_choices_keep_the_rule_corners() {
    let base = json!({"edges": [[50, 0, 0], [100, 30, 0]], "height": 20});
    let mut expected = sheet();
    expected.execute("sheet.flange", &base).unwrap();
    let surface = serde_json::to_value(&expected.model.state().surface_sheets[0]).unwrap();
    for corners in [None, Some(serde_json::Value::Null), Some(json!([]))] {
        let mut params = base.clone();
        if let Some(corners) = corners {
            params["corners"] = corners;
        }
        assert!(takes::<FlangeParams>(&params));
        let mut actual = sheet();
        actual.execute("sheet.flange", &params).unwrap();
        assert_eq!(serde_json::to_value(&actual.model.state().surface_sheets[0]).unwrap(), surface, "{params}");
        let saved = serde_json::to_value(&actual.doc.features.last().unwrap().kind).unwrap();
        assert!(saved.get("corners").is_none(), "{params}");
    }
}

#[test]
fn flange_corner_join_refuses_invalid_choices_without_adding_a_feature() {
    let mut reference = sheet();
    let mut corner = corner_join_params(&mut reference);
    corner.as_object_mut().unwrap().remove("name");
    let mut actual = sheet();
    let doc = actual.doc.clone();
    let surface = serde_json::to_value(&actual.model.state().surface_sheets[0]).unwrap();
    for patch in [
        json!({"kind": "partial"}),
        json!({"kind": true}),
        json!({"kind": "partial_overlap"}),
        json!({"kind": "partial_overlap", "overlap": "not_a_parameter"}),
        json!({"kind": "partial_overlap", "overlap": "90 deg"}),
        json!({"gap": -1}),
        json!({"gap": "90 deg"}),
        json!({"covering": "base:rim.missing:top"}),
        json!({"covered": corner["covering"]}),
        json!({"body": "Plate"}),
        json!({"unknown": true}),
    ] {
        let mut invalid = corner.clone();
        for (key, value) in patch.as_object().unwrap() {
            invalid[key] = value.clone();
        }
        let params = json!({"edges": [[50, 0, 0], [100, 30, 0]], "height": 20, "corners": [invalid]});
        assert!(actual.execute("sheet.flange", &params).is_err(), "{params}");
        assert_eq!(*actual.doc, *doc, "{params}");
        assert_eq!(serde_json::to_value(&actual.model.state().surface_sheets[0]).unwrap(), surface, "{params}");
    }
    for (case, corners) in [json!({}), json!(true), json!([null]), json!(vec![corner.clone(); 2001])].into_iter().enumerate() {
        let params = json!({"edges": [[50, 0, 0], [100, 30, 0]], "height": 20, "corners": corners});
        assert!(actual.execute("sheet.flange", &params).is_err(), "corner list case {case}");
        assert_eq!(*actual.doc, *doc, "corner list case {case}");
        assert_eq!(serde_json::to_value(&actual.model.state().surface_sheets[0]).unwrap(), surface, "corner list case {case}");
    }
}

#[test]
fn corner_join_command_uses_real_flange_source_names_and_rolls_back_missing_sources() {
    let mut s = sheet();
    let mut params = corner_join_params(&mut s);
    params["kind"] = json!("full_overlap");
    let reply = s.execute("sheet.corner_join", &params).unwrap();
    let schema = output_schema_for::<FeatureResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply));
    let saved = serde_json::to_value(&s.doc).unwrap();
    assert!(saved.to_string().contains("full_overlap"));
    assert!(saved.to_string().contains(params["covering"].as_str().unwrap()));
    let before = serde_json::to_value(&s.model.state().surface_sheets[0]).unwrap();
    assert!(s.execute("sheet.corner_join", &json!({"covering": "base:rim.missing:top", "covered": params["covered"]})).is_err());
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), saved);
    assert_eq!(serde_json::to_value(&s.model.state().surface_sheets[0]).unwrap(), before);
    s.execute("sheet.unfold", &json!({})).unwrap();
    s.execute("sheet.refold", &json!({})).unwrap();
}

#[test]
fn corner_join_command_defaults_and_inactive_overlap_match_saved_parameters() {
    for (kind, expected) in
        [(None, "butt"), (Some(Value::Null), "butt"), (Some(json!("butt")), "butt"), (Some(json!("full_overlap")), "full_overlap")]
    {
        let mut s = sheet();
        let mut params = corner_join_params(&mut s);
        if let Some(kind) = kind {
            params["kind"] = kind;
        }
        params["overlap"] = json!("not_a_parameter");
        params["gap"] = Value::Null;
        s.execute("sheet.corner_join", &params).unwrap();
        let saved = serde_json::to_value(&s.doc.find_feature("Joined").unwrap().kind).unwrap();
        assert_eq!(saved["kind"], expected);
        assert!(saved["overlap"].is_null(), "{saved}");
        assert!(saved["gap"].is_null(), "{saved}");
    }
}

#[test]
fn corner_join_command_refuses_invalid_active_parameters_atomically() {
    let mut s = sheet();
    let params = corner_join_params(&mut s);
    let doc = s.doc.clone();
    let surface = serde_json::to_value(&s.model.state().surface_sheets[0]).unwrap();
    for patch in [
        json!({"kind": "not_a_kind"}),
        json!({"kind": "partial_overlap"}),
        json!({"kind": "partial_overlap", "overlap": null}),
        json!({"kind": "partial_overlap", "overlap": "not_a_parameter"}),
        json!({"kind": "partial_overlap", "overlap": -1}),
        json!({"kind": "full_overlap", "gap": -1}),
        json!({"kind": "full_overlap", "gap": 0}),
        json!({"kind": "full_overlap", "gap": "not_a_parameter"}),
        json!({"covering": "base:rim.missing:top"}),
        json!({"covered": params["covering"]}),
    ] {
        let mut invalid = params.clone();
        for (key, value) in patch.as_object().unwrap() {
            invalid[key] = value.clone();
        }
        assert!(s.execute("sheet.corner_join", &invalid).is_err(), "{invalid}");
        assert_eq!(*s.doc, *doc, "{invalid}");
        assert_eq!(serde_json::to_value(&s.model.state().surface_sheets[0]).unwrap(), surface, "{invalid}");
    }
}

#[test]
fn corner_join_schema_matches_defaults_and_refuses_hostile_source_names() {
    let params = json!({"covering": "F7:top(F1:side:l2)|F7:bend.inner(F1:side:l2)", "covered": "F8:bottom(F1:side:l1)|F8:bend.inner(F1:side:l1)"});
    for (patch, accepted) in [
        (json!({}), true),
        (json!({"kind": null, "overlap": null, "gap": null}), true),
        (json!({"kind": "butt"}), true),
        (json!({"kind": "partial_overlap", "overlap": "corner_overlap", "gap": 0.5}), true),
        (json!({"kind": "full_overlap", "overlap": "unused"}), true),
        (json!({"kind": "partial"}), false),
        (json!({"kind": 1}), false),
        (json!({"covering": null}), false),
        (json!({"covered": 3}), false),
        (json!({"overlap": true}), false),
        (json!({"gap": []}), false),
    ] {
        let mut value = params.clone();
        for (key, field) in patch.as_object().unwrap() {
            value[key] = field.clone();
        }
        assert_eq!(takes::<CornerJoinParams>(&value), accepted, "{value}");
    }
    for name in [
        "base:rim.far:side",
        "base:far:top",
        "base:rim.:top",
        "F7..base:rim.far:top",
        "|F7:top",
        "F7:top|",
        "F18446744073709551616:top|F7:bend.inner",
    ] {
        for key in ["covering", "covered"] {
            let mut invalid = params.clone();
            invalid[key] = json!(name);
            assert!(parse::<CornerJoinParams>(invalid.clone()).is_none(), "{invalid}");
        }
    }
}

fn parse<T: DeserializeOwned>(v: Value) -> Option<T> {
    serde_json::from_value(v).ok()
}

/// Whether `T` accepts `v` and its schema agrees.
fn takes<T: DeserializeOwned + schemars::JsonSchema>(v: &Value) -> bool {
    let schema = input_schema_for::<T>();
    let schema = schema.as_value();
    let accepted = parse::<T>(v.clone()).is_some();
    assert_eq!(conforms(schema, schema, v), accepted, "{v}: the schema and the reader disagree");
    accepted
}

/// The value types accept what the handlers' readers accept, and their schemas agree (apart
/// from the word types' patterns, which the test validator doesn't read: those are checked on
/// the readers alone).
#[test]
fn the_value_types_match_the_readers() {
    let many = |n: usize| Value::Array(vec![json!([1, 2, 3]); n]);
    for (v, ok) in [
        (json!([[1, 2, 3]]), true),
        (json!([{"x": 1, "y": 2, "z": 3}, {"point": [1, 2, 3]}, {"point": {"x": 1, "y": 2, "z": 3}, "body": "B"}]), true),
        (many(1000), true),
        (many(1001), false),
        (json!([]), false),
        (json!([[1, 2]]), false),
        (json!([{"point": [1, 2]}]), false),
        (json!([[1e10, 0, 0]]), false),
        (json!(5), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<SheetPoints>(&v), ok, "{v}");
        assert_eq!(parse::<SheetPoints>(v.clone()).map(|p| p.0), crate::cmd::sheet_points(Some(&v)).ok(), "{v}");
    }
    for (v, ok) in [
        (json!([[1, 2, 3], {"x": 1, "y": 2, "z": 3}]), true),
        (json!([{"point": [1, 2, 3]}, {"point": [1, 2, 3], "angle": "30 deg", "height": 5}, {"point": [1, 2, 3], "angle": null}]), true),
        (many(1000), true),
        (many(1001), false),
        (json!([{"x": 1, "y": 2, "z": 3, "point": [1, 2, 3]}]), false),
        (json!([{"x": 1, "y": 2, "z": 3, "angle": 30}]), false),
        (json!([{"angle": 30}]), false),
        (json!([{"point": [1, 2, 3], "depth": 1}]), false),
        (json!([{"point": [1, 2, 3], "angle": true}]), false),
        (json!([{"point": [1, 2]}]), false),
        (json!([[1, 2]]), false),
        (json!([]), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<FlangeEdges>(&v), ok, "{v}");
    }
    let bends = |n: usize| Value::Array(vec![json!({"height": 10}); n]);
    for (v, ok) in [
        (json!([{"height": 10}]), true),
        (json!([{"height": 10, "angle": "45 deg", "flip": true}, {"height": "5 mm", "angle": null, "flip": null}]), true),
        (json!([]), true),
        (bends(16), true),
        (bends(17), false),
        (json!([{"angle": 30}]), false),
        (json!([{"height": 10, "depth": 1}]), false),
        (json!([{"height": 10, "flip": "yes"}]), false),
        (json!([5]), false),
        (json!(5), false),
    ] {
        assert_eq!(takes::<ThenSteps>(&v), ok, "{v}");
        assert_eq!(parse::<ThenSteps>(v.clone()).map(|t| t.0), crate::cmd::then_list(Some(&v)).ok(), "{v}");
    }
    // The handler reads a bend's `angle` that is no expression as left out (90°); the type
    // refuses it, which is what warn and strict report.
    for angle in [json!(true), json!([45])] {
        let v = json!([{"height": 10, "angle": angle}]);
        assert!(!takes::<ThenSteps>(&v), "{v}");
        assert_eq!(crate::cmd::then_list(Some(&v)), crate::cmd::then_list(Some(&json!([{"height": 10}]))), "{v}");
    }
    for (v, ok) in [
        (json!([[0, 0, 0], [1, 1, 1]]), true),
        (json!([{"x": 0, "y": 0, "z": 0}, [1, 1, 1]]), true),
        (json!([[0, 0, 0]]), false),
        (json!([[0, 0, 0], [1, 1]]), false),
        (json!([[0, 0, 0], [1, 1, 1], [2, 2, 2]]), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<FoldPoints>(&v), ok, "{v}");
    }
    for (v, ok) in [(json!([1, 2, 3]), true), (json!({"x": 1, "y": 2, "z": 3}), true), (json!([1, 2]), false), (json!(null), false)] {
        assert_eq!(takes::<FixedPoint>(&v), ok, "{v}");
        assert_eq!(takes::<SheetEdgePoint>(&v), ok, "{v}");
    }
    for (v, ok) in [
        (json!("staggered"), true),
        (json!("square"), true),
        (json!("Square"), false),
        (json!("hex"), false),
        (json!(5), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<PerforationPattern>(&v), ok, "{v}");
    }
    let words = |accepted: &[&str], refused: &[Value], read: &dyn Fn(Value) -> bool| {
        for w in accepted {
            assert!(read(json!(w)), "{w}");
        }
        for v in refused {
            assert!(!read(v.clone()), "{v}");
        }
    };
    words(&["base", "Edge", "CONTOUR"], &[json!("sideways"), json!(""), json!(5)], &|v| parse::<FlangeTypeParam>(v).is_some());
    words(
        &["inside", "Outside", "MIDDLE", "bend_outside", "Material_Outside", "TANGENT"],
        &[json!("mid"), json!(" inside"), json!("material outside"), json!(""), json!(3)],
        &|v| parse::<FlangePosition>(v).is_some(),
    );
    words(&["outer", " Inner ", "TANGENT"], &[json!("sharp"), json!(""), json!(5)], &|v| parse::<HeightFromParam>(v).is_some());
    words(&["centerline", "Start", "END", "mould", "Mold"], &[json!("center"), json!("side"), json!(null)], &|v| parse::<FoldPosition>(v).is_some());
    words(&["left", "Right", "LEFT"], &[json!("up"), json!(" left"), json!(""), json!(5), json!(null)], &|v| parse::<FoldStays>(v).is_some());
    words(&["notch", " Round ", "rectangular", "TEAR"], &[json!("oval"), json!(""), json!(5)], &|v| parse::<ReliefShapeParam>(v).is_some());
    words(&["inside", "Outside", "INSIDE"], &[json!("both"), json!(""), json!(5)], &|v| parse::<MaterialSide>(v).is_some());
    // A bend table's format: what the handler reads, in any case but without blanks.
    let formats = ["csv", "CSV", "Json", "jSoN"];
    words(&formats, &[json!(" csv"), json!("xml"), json!(""), json!(5), json!(null)], &|v| parse::<BendTableFormatParam>(v).is_some());
    for f in formats {
        assert!(crate::cmd::bend_table_format(f).is_some(), "{f}");
    }
    for f in [" csv", "csv ", "xml", "", "ｃｓｖ"] {
        assert!(crate::cmd::bend_table_format(f).is_none(), "{f}");
    }
    let format = input_schema_for::<BendTableFormatParam>();
    assert_eq!(format.as_value()["pattern"], "^([Cc][Ss][Vv]|[Jj][Ss][Oo][Nn])$");
    // A DXF export's words and its note height: what the handler's own readers take.
    words(&["exact", "EXACT", "arcs", "Arcs"], &[json!("arc"), json!(" exact"), json!("exact "), json!(""), json!(5), json!(null)], &|v| {
        parse::<DxfCurvesParam>(v).is_some()
    });
    words(&["cut", "CUT", "etch", "Etch"], &[json!("engrave"), json!("cut "), json!(""), json!(true), json!(null)], &|v| {
        parse::<DxfTextParam>(v).is_some()
    });
    words(&["roles", "ROLES", "single", "Single"], &[json!("one"), json!("0"), json!(""), json!(5), json!(null)], &|v| {
        parse::<DxfLayersParam>(v).is_some()
    });
    words(&["mm", "MM", "inches", "INCHES"], &[json!("feet"), json!("cm"), json!("mm "), json!(""), json!(4), json!(null)], &|v| {
        parse::<DxfUnitsParam>(v).is_some()
    });
    for w in ["exact", "EXACT", "arcs", "Arcs"] {
        assert_eq!(parse::<DxfCurvesParam>(json!(w)).map(|c| c.0), crate::cmd::dxf_curves(w), "{w}");
    }
    for w in ["cut", "CUT", "etch", "Etch"] {
        assert_eq!(parse::<DxfTextParam>(json!(w)).map(|t| t.0), crate::cmd::dxf_text(w), "{w}");
    }
    for w in ["roles", "ROLES", "single", "Single"] {
        assert_eq!(parse::<DxfLayersParam>(json!(w)).map(|l| l.0), crate::cmd::dxf_layers(w), "{w}");
    }
    for w in ["mm", "MM", "inches", "INCHES"] {
        assert_eq!(parse::<DxfUnitsParam>(json!(w)).map(|u| u.0), crate::cmd::dxf_units(w), "{w}");
    }
    for (v, ok) in [
        (json!(3), true),
        (json!(0.1), true),
        (json!(100), true),
        (json!(0.09), false),
        (json!(100.5), false),
        (json!(-1), false),
        (json!("3"), false),
        (json!(true), false),
        (json!(null), false),
    ] {
        assert_eq!(takes::<DxfNoteHeight>(&v), ok, "{v}");
    }
    for mm in [0.1, 3.0, 100.0] {
        assert_eq!(parse::<DxfNoteHeight>(json!(mm)).map(|h| h.0), crate::cmd::dxf_note_height(mm), "{mm}");
    }
    for (ty, want) in [
        (input_schema_for::<DxfUnitsParam>(), "^([Mm][Mm]|[Ii][Nn][Cc][Hh][Ee][Ss])$"),
        (input_schema_for::<DxfCurvesParam>(), "^([Ee][Xx][Aa][Cc][Tt]|[Aa][Rr][Cc][Ss])$"),
        (input_schema_for::<DxfTextParam>(), "^([Cc][Uu][Tt]|[Ee][Tt][Cc][Hh])$"),
        (input_schema_for::<DxfLayersParam>(), "^([Rr][Oo][Ll][Ee][Ss]|[Ss][Ii][Nn][Gg][Ll][Ee])$"),
    ] {
        assert_eq!(ty.as_value()["pattern"], want);
    }
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sheet.flange", json!({"type": "sideways"})),
        ("sheet.flange", json!({"type": "EDGEWAYS", "edges": [[0, 0, 0]], "height": 10})),
        ("sheet.flange", json!({"edges": [], "height": 10})),
        ("sheet.flange", json!({"edges": 5, "height": 10})),
        ("sheet.flange", json!({"edges": [[0, 0]], "height": 10})),
        ("sheet.flange", json!({"edges": [[0, 0, 0], {"point": [0, 0, 0], "depth": 3}], "height": 10})),
        ("sheet.flange", json!({"edges": [{"point": [0, 0]}], "height": 10})),
        ("sheet.flange", json!({"edges": [{"point": [0, 0, 0], "angle": true}], "height": 10})),
        ("sheet.flange", json!({"edges": [{"point": [0, 0, 0], "height": "  "}], "height": 10})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "position": "nowhere"})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "height_from": "sharp"})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "then": 5})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "then": [{"angle": 30}]})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "then": [{"height": 5, "turn": 1}]})),
        ("sheet.flange", json!({"edges": [[0, 0, 0]], "height": 10, "segments": [{"from": 0, "to": 1}]})),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 5, "pattern": "hex"})),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 5, "pattern": "Square"})),
        ("sheet.hem", json!({"edges": [], "length": 5})),
        ("sheet.hem", json!({"edges": 5, "length": 5})),
        ("sheet.hem", json!({"edges": null, "length": 5})),
        ("sheet.hem", json!({"edges": [[1, 2]], "length": 5})),
        ("sheet.fold", json!({"points": [[0, 0, 0]]})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [1, 2]]})),
        ("sheet.fold", json!({"points": null})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [10, 0, 0]], "position": "side"})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [10, 0, 0]], "fixed": [1, 2]})),
        ("sheet.fold", json!({"points": [[0, 0, 0], [10, 0, 0]], "fixed": null})),
        ("sheet.fold", json!({"sketch": true, "curve": "l1"})),
        ("sheet.convert", json!({"body": "Nothing", "face": [0, 0, 0]})),
        ("sheet.path_flange", json!({"edge": [1, 2], "sketch": "Path", "curves": ["p1"]})),
        ("sheet.path_flange", json!({"edge": null, "sketch": "Path", "curves": ["p1"]})),
        ("sheet.path_flange", json!({"edge": [1, 0, 20], "sketch": true, "curves": ["p1"]})),
        ("sheet.flat_pattern", json!({})),
        ("sheet.flat_pattern", json!({"body": "Nothing"})),
        ("sheet.export_bend_table", json!({"format": "xml"})),
        ("sheet.export_bend_table", json!({"format": " csv", "body": "Nothing"})),
        ("sheet.export_bend_table", json!({"format": "JSON", "body": "Nothing"})),
        // A DXF export reads its words and its note height before it looks for a sheet.
        ("sheet.export_dxf", json!({"units": "feet"})),
        ("sheet.export_dxf", json!({"curves": "arc"})),
        ("sheet.export_dxf", json!({"curves": " exact"})),
        ("sheet.export_dxf", json!({"text": "engrave"})),
        ("sheet.export_dxf", json!({"text": true})),
        ("sheet.export_dxf", json!({"layers": "one"})),
        ("sheet.export_dxf", json!({"layers": 0})),
        ("sheet.export_dxf", json!({"note_height": 0.09})),
        ("sheet.export_dxf", json!({"note_height": 101})),
        ("sheet.export_dxf", json!({"note_height": "3"})),
        ("sheet.export_dxf", json!({"bend_notes": 5})),
        ("sheet.export_dxf", json!({"bend_lines": "no"})),
        ("sheet.export_dxf", json!({"per_body": 5})),
        ("sheet.export_dxf", json!({"per_body": true, "body": "Base"})),
        ("sheet.export_dxf", json!({"body": "Nothing"})),
    ]);
    // Calls that get past the handler's first checks only on a sheet.
    for (id, p) in [
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20, "corner_relief_shape": "oval"})),
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20, "corner_relief_shape": 5})),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": []})),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": [[1, 2]]})),
        ("sheet.convert", json!({"body": "Plate", "face": ""})),
        ("sheet.convert", json!({"body": "Plate", "face": true})),
        ("sheet.convert", json!({"body": "Plate", "direction": "both"})),
        ("sheet.convert", json!({"body": "Plate", "skip": [[50, 130, 2.5], ""]})),
        ("sheet.convert", json!({"body": "Plate", "engrave": [[50, 130, 2.5], ""]})),
        ("sheet.path_flange", json!({"edge": [50, 0, 2.5], "sketch": "Base", "curves": []})),
        ("sheet.path_flange", json!({"edge": [50, 0, 2.5], "sketch": "Base", "curves": vec!["l1"; 65]})),
        ("sheet.path_flange", json!({"edge": [50, 0, 2.5], "sketch": "Base", "curves": ["l1"], "radius": "2 deg"})),
        ("sheet.export_bend_table", json!({"format": "yaml"})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = sheet();
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// A rule's value given to a flange or a bent conversion is refused either way: by the handler,
/// which points to `sheet.manage_rules`, and in strict as a key the command doesn't take.
#[test]
fn rule_values_are_refused_with_the_check_off_and_in_strict() {
    for (id, p, key) in [
        ("sheet.flange", json!({"sketch": "Base", "thickness": 2}), "thickness"),
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20, "k_factor": 0.4}), "k_factor"),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": [[50, 100, 1.25]], "bend_radius": 2}), "bend_radius"),
    ] {
        let error = |level: ParamCheck| {
            let mut s = sheet();
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default()
        };
        let off = error(ParamCheck::Off);
        assert!(off.contains(&format!("`{key}` is not a {id} parameter: the sheet metal rule sets it")), "{p}: {off}");
        assert!(error(ParamCheck::Strict).contains(&format!("unknown key `{key}`")), "{p}");
    }
}

/// A further bend's `angle` that is no expression: with the check off the flange bends it 90°,
/// as before; warn reports it and strict refuses it.
#[test]
fn a_bend_angle_that_is_no_expression_is_reported() {
    let p = json!({"edges": [[0, 30, 2.5]], "height": 12, "then": [{"height": 8, "angle": true}]});
    let want = "then[0]: `angle` must be a number or an expression";
    let run = |level: ParamCheck| {
        let mut s = sheet();
        s.param_check = level;
        s.execute("sheet.flange", &p)
    };
    let off = run(ParamCheck::Off).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(off.get("param_warnings"), None, "{off}");
    let warn = run(ParamCheck::Warn).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(warn["param_warnings"], json!([want]), "{warn}");
    let strict = run(ParamCheck::Strict).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(strict.ends_with(want), "{strict}");
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let check = |r: Value| assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    let warn = || {
        let mut s = sheet();
        s.param_check = ParamCheck::Warn;
        s
    };
    let ok = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert_eq!(r.get("param_warnings"), None, "{id} {p}: {r}");
        r
    };
    // Each form each command takes, in Warn, without warnings.
    let mut s = warn();
    for (id, p) in [
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20})),
        ("sheet.hem", json!({"edges": [[50, 0, 20]], "length": 8, "gap": 1, "flip": false})),
        (
            "sheet.flange",
            json!({"type": "Edge", "edges": [{"point": [100, 30, 2.5], "angle": "60 deg", "height": null}], "height": 15, "angle": 90,
                   "radius": 2, "position": "OUTSIDE", "flip": true, "corner_relief_shape": " round ", "corner_relief": 3, "name": "Side"}),
        ),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 5, "margin": 3, "pattern": "square"})),
        (
            "sheet.flange",
            json!({"edges": [[0, 30, 2.5]], "height": 12, "position": "material_outside", "height_from": "inner",
                   "then": [{"height": 8}, {"height": 6, "angle": "45 deg", "flip": true}], "name": "Chain"}),
        ),
        ("sheet.unfold", json!({})),
        ("sheet.refold", json!({"name": "Back"})),
        ("sheet.flange", json!({"type": "BASE", "sketch": "Base", "profiles": "all", "rule": STEEL, "flip": true, "body_names": ["Second"]})),
        ("sheet.unfold", json!({"body": "Second"})),
        ("sheet.refold", json!({"body": "Second"})),
        ("sheet.convert", json!({"body": "Plate", "face": [50, 130, 2.5], "bends": null, "rule": STEEL, "body_name": "Plated"})),
    ] {
        check(ok(&mut s, id, p));
    }
    let mut s = warn();
    for (id, p) in [
        ("sheet.fold", json!({"points": [[60, 0, 2.5], [60, 60, 2.5]], "angle": "45 deg", "radius": 1, "position": "Mould", "fixed": [10, 30, 2.5]})),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 6})),
    ] {
        check(ok(&mut s, id, p));
    }
    let mut s = warn();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "offset": 2.5, "name": "FoldLine"})),
        ("sketch.line", json!({"points": [[40, 0], [40, 60]], "ids": ["fl"]})),
        ("sketch.finish", json!({})),
    ] {
        ok(&mut s, id, p);
    }
    let body = s.model.state().surface_sheets.first().map(|sh| sh.body.clone()).unwrap_or_default();
    check(ok(&mut s, "sheet.fold", json!({"sketch": "FoldLine", "curve": "fl", "position": "start", "flip": true, "body": body})));
    let mut s = warn();
    for (id, p) in [
        ("sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, -1]}, "name": "Contour"})),
        ("sketch.line", json!({"points": [[0, 0], [40, 0], [40, -30], [70, -30]], "ids": ["l1", "l2", "l3"]})),
        ("sketch.finish", json!({})),
    ] {
        ok(&mut s, id, p);
    }
    check(ok(
        &mut s,
        "sheet.flange",
        json!({"sketch": "Contour", "curves": ["l1", "l2", "l3"], "distance": 50, "reverse": true, "flip": true, "rule": STEEL}),
    ));
    check(ok(&mut s, "sheet.flange", json!({"type": "contour", "sketch": "Contour", "curves": "l1", "distance": "20 mm", "body_name": "Strip"})));
    // A misspelt key: the call runs, and the reply carrying its warning still matches.
    let mut s = warn();
    let r = s.execute("sheet.unfold", &json!({"bodies": "x"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    check(r);
}

#[test]
fn rule_commands_publish_contracts_and_refuse_unknown_keys() {
    for id in ["sheet.rules", "sheet.manage_rules"] {
        let spec = crate::command_specs().into_iter().find(|c| c.id == id).unwrap();
        assert!(spec.contract.is_some() && spec.output.is_some(), "{id}: input schema missing");
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        let p = if id == "sheet.rules" { json!({"unknown_key": true}) } else { json!({"name": "Test", "thikness": 1}) };
        assert!(s.execute(id, &p).is_err(), "{id}: unknown key accepted");
        assert!(s.doc.sheet.rules.is_empty());
    }
}

#[test]
fn rule_contract_keys_and_readers_match() {
    use super::{AngleDisplayParam, ManageRulesParams, RuleLibraryParams, RuleName, RulesParams, UnfoldMethodParam};
    assert_keys(input_schema_for::<RulesParams>(), &[]);
    assert_keys(input_schema_for::<RuleLibraryParams>(), &["material"]);
    assert_keys(
        input_schema_for::<ManageRulesParams>(),
        &[
            "active",
            "angle_display",
            "bend_method",
            "bend_radius",
            "bend_relief_shape",
            "corner_relief",
            "corner_relief_shape",
            "delete",
            "gap",
            "gauge",
            "hem_gap",
            "k_factor",
            "material",
            "name",
            "relief_depth",
            "relief_width",
            "thickness",
            "unfold_method",
            "unfold_value",
        ],
    );
    for (text, valid) in [("deduction", true), (" Bend Allowance ", true), ("K-Factor", true), ("neutral", false), ("", false)] {
        assert_eq!(parse::<UnfoldMethodParam>(json!(text)).is_some(), valid, "{text}");
    }
    assert!(parse::<UnfoldMethodParam>(json!(1)).is_none());
    for (text, valid) in [("bend", true), (" INCLUDED ", true), ("\u{2003}BeNd\u{2003}", true), ("outside", false)] {
        assert_eq!(parse::<AngleDisplayParam>(json!(text)).is_some(), valid);
    }
    assert!(parse::<AngleDisplayParam>(json!(false)).is_none());
    for v in [json!(null), json!(false), json!(1), json!(""), json!(" \n "), json!("a".repeat(129)), json!("é".repeat(65))] {
        assert!(parse::<RuleName>(v).is_none());
    }
    assert_eq!(parse::<RuleName>(json!("  My rule  ")).unwrap().0, "My rule");
    assert!(parse::<RuleName>(json!("é".repeat(64))).is_some());
    let pattern = input_schema_for::<AngleDisplayParam>();
    assert!(pattern.as_value()["pattern"].as_str().unwrap().contains(&super::blanks()));
    assert_strict_keeps_todays_errors(&[
        ("sheet.manage_rules", json!({"name": null})),
        ("sheet.manage_rules", json!({"name": true})),
        ("sheet.manage_rules", json!({"name": "  "})),
        ("sheet.manage_rules", json!({"name": "a".repeat(129)})),
        ("sheet.manage_rules", json!({"name": "R", "angle_display": "outer"})),
        ("sheet.manage_rules", json!({"name": "R", "angle_display": 5})),
        ("sheet.manage_rules", json!({"name": "R", "corner_relief_shape": "oval"})),
        ("sheet.manage_rules", json!({"name": "R", "bend_relief_shape": "round"})),
        ("sheet.manage_rules", json!({"name": "R", "thickness": -1})),
        ("sheet.manage_rules", json!({"name": "R", "k_factor": 2})),
        ("sheet.manage_rules", json!({"name": "Missing", "delete": true})),
    ]);
}

/// Run all rule forms in Warn, compare with Off, and pin the successful reply shapes.
#[test]
fn rule_replies_match_the_output_schemas_without_warnings() {
    use super::{ManageRulesResult, RulesResult};
    let mut off = Session::default();
    let mut warn = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let calls = [
        ("sheet.rules", json!({})),
        (
            "sheet.manage_rules",
            json!({"name": "  Metric  ", "thickness": 2, "k_factor": "0.4", "bend_radius": "Thickness / 2",
            "relief_width": 1, "relief_depth": "Thickness", "corner_relief": 3, "corner_relief_shape": " Round ",
            "bend_relief_shape": " Obround ", "hem_gap": 0.1, "gap": "Thickness / 2", "angle_display": " INCLUDED ", "active": true}),
        ),
        (
            "sheet.manage_rules",
            json!({"name": "Metric", "thickness": "3 mm", "k_factor": 0.5, "bend_radius": 1,
            "relief_width": "1 mm", "relief_depth": 2, "corner_relief": "3 mm", "corner_relief_shape": "tear",
            "bend_relief_shape": "TEAR", "hem_gap": "0.2 mm", "gap": 2, "angle_display": "BeNd", "active": false, "delete": false}),
        ),
        ("sheet.manage_rules", json!({"name": "Other"})),
        (
            "sheet.manage_rules",
            json!({"name": "Other", "thickness": null, "k_factor": null, "bend_radius": null,
            "relief_width": null, "relief_depth": null, "corner_relief": null, "corner_relief_shape": null,
            "bend_relief_shape": null, "hem_gap": null, "gap": null, "angle_display": null, "unfold_method": null, "unfold_value": null,
            "active": null, "delete": null}),
        ),
        ("sheet.rules", json!({})),
        ("sheet.manage_rules", json!({"name": "Other", "active": true})),
        ("sheet.manage_rules", json!({"name": "Metric", "delete": true})),
        ("sheet.manage_rules", json!({"name": "Other", "delete": true})),
        ("sheet.rules", json!({})),
    ];
    for (id, p) in calls {
        let a = off.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let b = warn.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert_eq!(a, b, "{id} {p}");
        assert!(b.get("param_warnings").is_none(), "{id} {p}: {b}");
        let schema = if id == "sheet.rules" { output_schema_for::<RulesResult>() } else { output_schema_for::<ManageRulesResult>() };
        assert!(conforms(schema.as_value(), schema.as_value(), &b), "{id}: {b}");
        if let Some(rule) = b.get("rule") {
            let saved = off.doc.sheet.rules.iter().find(|r| json!(r.name) == rule["name"]).unwrap();
            let mut old = json!(saved);
            old["angle_display"] = json!(saved.angle_display);
            old["unfold_method"] = json!(saved.unfold_method);
            old["unfold_value"] = json!(saved.unfold_value);
            assert_eq!(*rule, old);
            let (vals, _) = off.doc.param_values();
            assert_eq!(b["values"], json!(off.doc.rule_values(&vals, saved).unwrap()));
            assert_eq!(b["errors"], json!([]));
        }
    }
    // Saved rules whose expressions no longer evaluate are still listed, with null values.
    warn.doc_mut().sheet.rules.push(solidcraft_doc::sheet::SheetRule {
        name: "Invalid".into(),
        thickness: "Missing".into(),
        ..solidcraft_doc::sheet::SheetRule::steel()
    });
    let reply = warn.execute("sheet.rules", &json!({})).unwrap();
    assert_eq!(reply["rules"][0]["values"], Value::Null);
    let schema = output_schema_for::<RulesResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply));
}

#[test]
fn rule_warnings_match_the_output_schema_and_strict_does_not_mutate() {
    use super::{ManageRulesResult, RulesResult};
    for (id, p) in [("sheet.rules", json!({"naem": "R"})), ("sheet.manage_rules", json!({"name": "R", "thikness": 1}))] {
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        assert!(s.execute(id, &p).is_err());
        assert!(s.doc.sheet.rules.is_empty());
        s.param_check = ParamCheck::Warn;
        let reply = s.execute(id, &p).unwrap();
        assert!(!reply["param_warnings"].as_array().unwrap().is_empty());
        let schema = if id == "sheet.rules" { output_schema_for::<RulesResult>() } else { output_schema_for::<ManageRulesResult>() };
        assert!(conforms(schema.as_value(), schema.as_value(), &reply));
    }
}

/// Runs `id` in `s` and returns its reply, which carries no warnings.
fn quiet(s: &mut Session, id: &str, p: Value) -> Value {
    let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    assert_eq!(r.get("param_warnings"), None, "{id} {p}: {r}");
    r
}

/// The wall bracket of the path flange tests in `cmd/sheet_tests.rs`, in Warn: a 150 × 100 base
/// of a 2 mm rule with a 40 mm wall up its edge at x = 0, and the lines `p1` and `p2` of sketch
/// `Path` drawn across the wall's end edge (y = 0) at half its height.
fn path_bracket() -> Session {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for (id, p) in [
        (
            "sheet.manage_rules",
            json!({"name": "Demo", "thickness": "2 mm", "bend_radius": "2 mm", "gap": "0.5 mm", "k_factor": "0.4", "active": true}),
        ),
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [150, 100]})),
        ("sketch.finish", json!({})),
        ("sheet.flange", json!({"sketch": "Base"})),
        ("sheet.flange", json!({"edges": [[0, 50, 2]], "height": 40})),
        ("sketch.create", json!({"plane": {"origin": [0, 0, 20], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Path"})),
        ("sketch.line", json!({"points": [[0, 0], [20, -20], [100, -20]], "ids": ["p1", "p2"]})),
        ("sketch.finish", json!({})),
    ] {
        quiet(&mut s, id, p);
    }
    s
}

#[test]
fn path_flange_replies_match_the_output_schema() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    // Each form, in Warn, without warnings. The path's two lines make two bends besides the
    // wall's; one line makes one.
    for (p, bends) in [
        (json!({"edge": [1, 0, 20], "sketch": "Path", "curves": ["p1", "p2"]}), 3),
        (json!({"edge": {"x": 1, "y": 0, "z": 20}, "sketch": "Path", "curves": "p1", "radius": "1 mm", "body": "?", "name": "Flap"}), 2),
        (json!({"edge": [1, 0, 20], "sketch": "Path", "curves": ["p2", "p1"], "radius": null, "body": null}), 3),
    ] {
        let mut s = path_bracket();
        let body = s.model.state().surface_sheets.first().map(|sk| sk.body.clone()).unwrap_or_default();
        let mut p = p;
        if p["body"] == "?" {
            p["body"] = json!(body);
        }
        let r = quiet(&mut s, "sheet.path_flange", p.clone());
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        let flat = quiet(&mut s, "sheet.flat_pattern", json!({}));
        assert_eq!(flat["bends"].as_array().map(Vec::len), Some(bends), "{p}: {flat}");
    }
    // A misspelt key: the call runs, and the reply carrying its warning still matches.
    let mut s = path_bracket();
    let r = s
        .execute("sheet.path_flange", &json!({"edge": [1, 0, 20], "sketch": "Path", "curves": ["p1"], "radiuss": 1}))
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r}");
}

/// The 100 × 60 sheet with a flange up its edge at y = 0, a 6 mm hole at (30, 30), a 1 mm deep
/// pocket from below, a perforation field and a 1.5 mm bore into its rim at x = 100; in Warn.
fn worked_sheet() -> Session {
    let mut s = sheet();
    for (id, p) in [
        ("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20})),
        ("solid.hole", json!({"position": [30, 30, 2.5], "direction": [0, 0, -1], "diameter": 6})),
        ("sketch.create", json!({"plane": "XY", "name": "Pocket"})),
        ("sketch.rectangle.two_point", json!({"p0": [60, 20], "p1": [70, 30]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"sketch": "Pocket", "distance": 1, "operation": "cut"})),
        ("sheet.perforate", json!({"diameter": 2, "pitch": 6})),
        ("solid.hole", json!({"position": [100, 45, 1.25], "direction": [-1, 0, 0], "diameter": 1.5})),
    ] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id}: {e}");
        }
    }
    s.param_check = ParamCheck::Warn;
    s
}

#[test]
fn flat_pattern_replies_match_the_output_schema() {
    let schema = output_schema_for::<FlatPatternResult>();
    let schema = schema.as_value();
    let mut s = worked_sheet();
    let body = s.model.state().surface_sheets.first().map(|sk| sk.body.clone()).unwrap_or_default();
    let f = quiet(&mut s, "sheet.flat_pattern", json!({}));
    assert!(conforms(schema, schema, &f), "{f} doesn't match {schema}");
    // Every part of the reply is there, from the features above.
    assert_eq!(f["body"], json!(body));
    assert_eq!(f["rule"], STEEL);
    assert_eq!(f["unfolded"], false);
    assert_eq!(f["bends"].as_array().map(Vec::len), Some(1), "{f}");
    assert_eq!((&f["bends"][0]["angle_deg"], &f["bends"][0]["included_angle_deg"]), (&json!(90.0), &json!(90.0)), "{f}");
    let circles: Vec<&Value> = f["cutout_shapes"].as_array().into_iter().flatten().filter_map(|c| c.get("circle")).collect();
    assert_eq!(circles.len(), 1, "{f}");
    assert!((circles[0]["radius"].as_f64().unwrap_or_default() - 3.0).abs() < 1e-9, "{f}");
    assert_eq!(f["pockets"].as_array().map(Vec::len), Some(1), "{f}");
    assert_eq!((&f["pockets"][0]["side"], &f["pockets"][0]["depth_mm"]), (&json!("bottom"), &json!(1.0)), "{f}");
    assert!(f["pockets"][0]["cutout_shapes"].is_array(), "{f}");
    let perforation = &f["perforations"][0];
    let count = perforation["count"].as_f64().unwrap_or_default();
    assert!(count > 0.0, "{f}");
    assert_eq!(perforation["pattern"], "staggered");
    assert!((perforation["open_area_mm2"].as_f64().unwrap_or_default() - count * std::f64::consts::PI).abs() < 1e-6, "{f}");
    assert_eq!(f["bores"].as_array().map(Vec::len), Some(1), "{f}");
    assert!((f["bores"][0]["diameter"].as_f64().unwrap_or_default() - 1.5).abs() < 1e-9, "{f}");
    assert_eq!(f["bend_zones"].as_array().map(Vec::len), Some(1), "{f}");
    // The same sheet by name, and with a null body.
    assert_eq!(quiet(&mut s, "sheet.flat_pattern", json!({"body": body})), f);
    assert_eq!(quiet(&mut s, "sheet.flat_pattern", json!({"body": null})), f);
    // Unfolded, and a plain sheet without bores, which the reply leaves out.
    quiet(&mut s, "sheet.unfold", json!({}));
    let unfolded = quiet(&mut s, "sheet.flat_pattern", json!({}));
    assert_eq!(unfolded["unfolded"], true);
    assert!(conforms(schema, schema, &unfolded), "{unfolded}");
    let mut plain = sheet();
    plain.param_check = ParamCheck::Warn;
    let r = quiet(&mut plain, "sheet.flat_pattern", json!({}));
    assert!(r.get("bores").is_none(), "{r}");
    assert!(conforms(schema, schema, &r), "{r}");
    // A misspelt key: the call runs, and the reply carrying its warning still matches.
    let r = plain.execute("sheet.flat_pattern", &json!({"bodyy": body})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r}");
}

#[test]
fn bend_table_replies_match_the_output_schema() {
    let schema = output_schema_for::<BendTableResult>();
    let schema = schema.as_value();
    let mut s = worked_sheet();
    let body = s.model.state().surface_sheets.first().map(|sk| sk.body.clone()).unwrap_or_default();
    let flat = quiet(&mut s, "sheet.flat_pattern", json!({}));
    let rows = flat["bends"].as_array().map_or(0, Vec::len);
    let table = |s: &mut Session, p: Value| {
        let r = quiet(s, "sheet.export_bend_table", p);
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r["bends"], json!(rows), "{r}");
        r
    };
    // csv: a header and a row per bend.
    let csv = table(&mut s, json!({}));
    assert_eq!(csv["format"], "csv");
    let text = csv["text"].as_str().unwrap_or_default().to_string();
    assert_eq!(text.matches("\r\n").count(), rows + 1, "{text}");
    assert_eq!(table(&mut s, json!({"format": "CSV", "path": "  ", "body": null})), csv);
    // json: the flat pattern as text.
    let json = table(&mut s, json!({"format": "Json", "body": body}));
    assert_eq!(json["format"], "json");
    assert_eq!(serde_json::from_str::<Value>(json["text"].as_str().unwrap_or_default()).ok(), Some(flat));
    // To a file: the reply names it, and the file holds the text.
    let path = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-bend-table-{}.csv", std::process::id()));
    let path = path.to_string_lossy().to_string();
    let written = table(&mut s, json!({"format": null, "path": path}));
    assert_eq!((written.get("text"), &written["path"], &written["format"]), (None, &json!(path), &json!("csv")), "{written}");
    assert_eq!(std::fs::read_to_string(&path).ok(), Some(text));
    let _ = std::fs::remove_file(&path);
    // A misspelt key: the call runs, and the reply carrying its warning still matches.
    let r = s.execute("sheet.export_bend_table", &json!({"formatt": "json"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r}");
}

#[test]
fn export_dxf_replies_match_the_output_schema() {
    let schema = output_schema_for::<ExportDxfResult>();
    let schema = schema.as_value();
    let mut s = worked_sheet();
    let body = s.model.state().surface_sheets.first().map(|sh| sh.body.clone()).unwrap_or_default();
    let rows = quiet(&mut s, "sheet.flat_pattern", json!({}))["bends"].as_array().map_or(0, Vec::len);
    let dxf = |s: &mut Session, p: Value| {
        let r = quiet(s, "sheet.export_dxf", p);
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r["bends"], json!(rows), "{r}");
        r["dxf"].as_str().unwrap_or_default().to_string()
    };
    // No path: the DXF text, with a layer for each role and the bend lines.
    let text = dxf(&mut s, json!({}));
    assert!(text.contains("SECTION") && text.contains("ENDSEC"), "{text}");
    assert_eq!(dxf(&mut s, json!({"body": body, "path": "  ", "curves": null, "text": null})), text);
    assert_eq!(dxf(&mut s, json!({"curves": "EXACT", "text": "Cut", "layers": "Roles", "note_height": 3})), text);
    // Bend notes add entities, leaving the bend lines out takes away the ones they draw.
    let notes = dxf(&mut s, json!({"bend_notes": true, "note_height": 5}));
    assert!(notes.len() > text.len(), "{notes}");
    let bare = dxf(&mut s, json!({"bend_lines": false}));
    assert!(bare.len() < text.len(), "{bare}");
    // Every curve and text form, and one layer for the whole drawing: still a drawing.
    for p in [json!({"curves": "arcs", "text": "etch"}), json!({"curves": "arcs", "bend_notes": true, "bend_lines": false, "layers": "single"})] {
        let drawing = dxf(&mut s, p.clone());
        assert!(drawing.contains("SECTION") && drawing.contains("ENDSEC"), "{p}: {drawing}");
    }
    // To a file: the reply names it, and the file holds the drawing.
    let path = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-export-dxf-{}.dxf", std::process::id()));
    let path = path.to_string_lossy().to_string();
    let r = quiet(&mut s, "sheet.export_dxf", json!({"path": path}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!((r.get("dxf"), &r["path"], &r["bends"]), (None, &json!(path), &json!(rows)), "{r}");
    assert_eq!(std::fs::read_to_string(&path).ok(), Some(text.clone()));
    let _ = std::fs::remove_file(&path);
    // `per_body`: one entry per sheet body, its text without a path and its own file with one.
    let r = quiet(&mut s, "sheet.export_dxf", json!({"per_body": true}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    let files = r["files"].as_array().cloned().unwrap_or_default();
    assert_eq!(files.as_slice(), &[json!({"body": body, "dxf": text.clone(), "bends": rows})], "{r}");
    let stem = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-export-dxf-{}-all", std::process::id()));
    let stem = stem.to_string_lossy().to_string();
    let r = quiet(&mut s, "sheet.export_dxf", json!({"per_body": true, "path": stem}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    let files = r["files"].as_array().cloned().unwrap_or_default();
    let file = files.first().and_then(|f| f["path"].as_str()).unwrap_or_default().to_string();
    assert_eq!((files.len(), files.first().and_then(|f| f.get("dxf"))), (1, None), "{r}");
    assert!(file.ends_with(&format!("-{body}.dxf")), "{file}");
    assert_eq!(std::fs::read_to_string(&file).ok(), Some(text));
    let _ = std::fs::remove_file(&file);
    // A misspelt key: the call runs, and the reply carrying its warning still matches.
    let r = s.execute("sheet.export_dxf", &json!({"cures": "arcs"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r}");
}

#[test]
fn hardware_replies_match_the_output_schema() {
    let schema = output_schema_for::<HardwareResult>();
    let schema = schema.as_value();
    let mut s = sheet();
    let r = quiet(&mut s, "sheet.hardware", json!({"kind": "stud", "thread": "M4", "length": "10 mm", "position": [50, 30, 2.5]}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!((&r["hardware"]["kind"], &r["hardware"]["part"], &r["hardware"]["length"]), (&json!("stud"), &json!("stud.m4"), &json!(10.0)), "{r}");
    // A nut has no length: the key is null, and the feature's own keys are there too.
    let r = quiet(&mut s, "sheet.hardware", json!({"kind": "nut", "thread": "M3", "position": [20, 30, 2.5]}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!(r["hardware"]["length"], Value::Null);
    assert!(r["feature"].is_u64() && r["bodies"].is_array(), "{r}");
    // The flat pattern lists both, and its reply still matches its own schema.
    let schema = output_schema_for::<FlatPatternResult>();
    let schema = schema.as_value();
    let r = quiet(&mut s, "sheet.flat_pattern", json!({}));
    assert_eq!(r["hardware"].as_array().map(Vec::len), Some(2), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
}

// ---- Tab (#8325) ----

#[test]
fn tab_schema_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TabParams>(), &keys(&["body", "exact_regions", "profiles", "sketch"]));
}

#[test]
fn tab_strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sheet.tab", json!({"sketch": true})),
        ("sheet.tab", json!({"sketch": "Nowhere"})),
        ("sheet.tab", json!({"profiles": [{"point": [1]}]})),
        ("sheet.tab", json!({"profiles": [["l1"], 7]})),
    ]);
}

/// [`sheet`] and a 40 x 40 profile `Ear` on its top face, from (30, 50) to (70, 90): over its
/// long edge by 30 mm.
fn sheet_with_tab_sketch(param_check: ParamCheck) -> Session {
    use solidcraft_geom::prelude::*;
    let mut s = sheet();
    s.param_check = param_check;
    if let Err(e) = s.execute("sketch.create", &json!({"plane": {"face": [50, 30, 2.5]}, "name": "Ear"})) {
        panic!("{e}");
    }
    let plane = s.model.state().sketches.last().map(|ss| ss.plane).unwrap_or_else(|| panic!("no sketch"));
    let (p0, p1) = (plane.to_local(solidcore_record::Vec3::new(30.0, 50.0, 2.5)), plane.to_local(solidcore_record::Vec3::new(70.0, 90.0, 2.5)));
    for (id, p) in [("sketch.rectangle.two_point", json!({"p0": [p0.x, p0.y], "p1": [p1.x, p1.y]})), ("sketch.finish", json!({}))] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id}: {e}");
        }
    }
    s
}

/// Every form the command takes answers with a `FeatureResult` and, in Warn, no warnings.
#[test]
fn tab_replies_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    // The face's projected outline parts the drawn profile into regions: the exact region past the
    // face's y = 60 edge, picked by a point in it (an index would be the regions' order).
    let past = {
        use solidcraft_geom::prelude::*;
        let s = sheet_with_tab_sketch(ParamCheck::Warn);
        let plane = s.model.state().sketches.last().map(|ss| ss.plane).unwrap_or_else(|| panic!("no sketch"));
        let q = plane.to_local(solidcore_record::Vec3::new(50.0, 80.0, 2.5));
        [q.x, q.y]
    };
    for p in [
        json!({}),
        json!({"sketch": "Ear"}),
        json!({"sketch": "Ear", "profiles": "all", "body": "Body1", "name": "Ear tab"}),
        json!({"sketch": "Ear", "profiles": [{"point": past}], "exact_regions": true}),
    ] {
        let mut s = sheet_with_tab_sketch(ParamCheck::Warn);
        let r = s.execute("sheet.tab", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        assert_eq!(r["created_bodies"], json!([]), "{p}: no new body");
        assert_eq!(r["changed_bodies"], json!(["Body1"]), "{p}");
    }
    let mut s = sheet_with_tab_sketch(ParamCheck::Strict);
    let e = s.execute("sheet.tab", &json!({"sketch": "Ear", "height": 5})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("height"), "{e}");
    assert!(matches!(s.execute("sheet.tab", &json!({"sketch": "Ear"})), Ok(Value::Object(_))));
}

// ---- Bead (#11054) ----

#[test]
fn bead_schema_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<super::BeadParams>(),
        &keys(&[
            "angle",
            "base_radius",
            "body",
            "crown_radius",
            "crown_width",
            "curves",
            "depth",
            "end_length",
            "flip",
            "margin",
            "shape",
            "sketch",
            "width",
        ]),
    );
}

#[test]
fn bead_strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sheet.bead", json!({"sketch": true, "curves": ["c1"], "depth": 2, "width": 10})),
        ("sheet.bead", json!({"sketch": "Nowhere", "curves": ["c1"], "depth": 2, "width": 10})),
    ]);
}

#[test]
fn a_bead_reply_matches_the_output_schema() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let mut s = Session::default();
    // A plate 80 x 50 x 1 thickened from its top face (z = 1), and a line on that face.
    for (id, p) in [
        ("solid.box", json!({"length": 80, "width": 50, "height": 1, "body_name": "Plate"})),
        ("sheet.convert", json!({"mode": "thicken", "body": "Plate", "face": [40, 25, 1]})),
        ("sketch.create", json!({"plane": "XY", "offset": 1, "name": "Beads"})),
        ("sketch.line", json!({"points": [[20, 25], [60, 25]], "ids": ["c1"]})),
        ("sketch.finish", json!({})),
    ] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id}: {e}");
        }
    }
    let r = s
        .execute(
            "sheet.bead",
            &json!({"sketch": "Beads", "curves": ["c1"], "depth": 2, "width": 10, "base_radius": 0.5, "crown_radius": 0.5, "name": "Rib"}),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    // A misspelt key, in the warn mode, on a second line clear of the first bead: the call runs,
    // and the reply carrying its warning still matches.
    s.param_check = ParamCheck::Warn;
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "offset": 1, "name": "More"})),
        ("sketch.line", json!({"points": [[20, 40], [60, 40]], "ids": ["c1"]})),
        ("sketch.finish", json!({})),
    ] {
        if let Err(e) = s.execute(id, &p) {
            panic!("{id}: {e}");
        }
    }
    let r = s
        .execute(
            "sheet.bead",
            &json!({"sketch": "More", "curves": ["c1"], "depth": 2, "width": 10, "base_radius": 0.5, "crown_radius": 0.5, "colour": "red"}),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let warned = r["param_warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().is_some_and(|w| w.contains("colour"))));
    assert!(warned, "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
}
