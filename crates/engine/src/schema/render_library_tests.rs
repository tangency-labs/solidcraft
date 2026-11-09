use serde_json::{Value, json};

use super::{
    AppearanceApplied, AppearanceApplyParams, EnvironmentApplied, EnvironmentApplyParams, LibraryDescribeParams, LibraryDescription,
    LibraryListParams, LibraryPage, LibraryPreview, LibraryPreviewParams, LibraryTargets, LibraryTargetsParams, PhysicalPlan, PhysicalPrepareParams,
    RenderTarget, StableRenderTarget, StudioApplied, StudioApplyParams, StudioPrepareParams, StudioPrepared, SurfacePrepareParams, SurfacePrepared,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<LibraryListParams>(), &["availability", "kind", "limit", "offset", "query"]);
    assert_keys(input_schema_for::<LibraryDescribeParams>(), &["id", "kind"]);
    assert_keys(input_schema_for::<LibraryTargetsParams>(), &["limit", "offset", "query"]);
    assert_keys(input_schema_for::<LibraryPreviewParams>(), &["id"]);
    assert_keys(input_schema_for::<AppearanceApplyParams>(), &["bodies", "id", "variant"]);
    assert_keys(input_schema_for::<EnvironmentApplyParams>(), &["id", "intensity", "name", "replace_environment", "rotation_rad", "samples", "set"]);
    assert_keys(input_schema_for::<PhysicalPrepareParams>(), &["acknowledge_reference", "id", "name"]);
    assert_keys(input_schema_for::<SurfacePrepareParams>(), &["body_index", "body_name", "id", "origin", "plane", "rotation_rad", "scale"]);
    assert_keys(input_schema_for::<StudioPrepareParams>(), &["center_mm", "enable_indirect", "id", "name", "set", "subject_extent_mm", "variant"]);
    assert_keys(
        input_schema_for::<StudioApplyParams>(),
        &["enable_indirect", "expected_revision", "name", "replace_lighting_and_environment", "replace_stage", "rig", "set"],
    );
}

/// The list's schema takes an `availability` exactly when the reader for its `kind` does: the
/// physical profiles' takes `null` but not `specification` or `blocked`, the others' the reverse.
#[test]
fn the_list_schema_takes_each_readers_availability() {
    let schema = input_schema_for::<LibraryListParams>();
    let schema = schema.as_value();
    let calls = [
        json!({}),
        json!({"availability": "blocked"}),
        json!({"availability": null}),
        json!({"kind": "hdr", "availability": "specification"}),
        json!({"kind": "hdr", "availability": null}),
        json!({"kind": "physical_profiles"}),
        json!({"kind": "physical_profiles", "availability": "usable"}),
        json!({"kind": "physical_profiles", "availability": null}),
        json!({"kind": "physical_profiles", "availability": "blocked"}),
        json!({"kind": "physical_profiles", "availability": "specification"}),
    ];
    for p in calls {
        let handled = Session::default().execute("render.library.list", &p).is_ok();
        assert_eq!(conforms(schema, schema, &p), handled, "{p}");
    }
}

#[test]
fn strict_keeps_todays_errors() {
    let long = "x".repeat(257);
    assert_strict_keeps_todays_errors(&[
        ("render.library.list", json!({"kind": "not-a-kind"})),
        ("render.library.list", json!({"query": long})),
        ("render.library.list", json!({"query": "a\u{7}b"})),
        ("render.library.list", json!({"limit": 0})),
        ("render.library.list", json!({"limit": 129})),
        ("render.library.list", json!({"offset": 4097})),
        ("render.library.list", json!({"availability": "sometimes"})),
        ("render.library.list", json!({"availability": null})),
        ("render.library.list", json!({"kind": "physical_profiles", "availability": "blocked"})),
        ("render.library.list", json!({"kind": "physical_profiles", "limit": 0})),
        ("render.library.describe", json!({"kind": "not-a-kind", "id": "MAT-001"})),
        ("render.library.describe", json!({"kind": "materials", "id": "MAT 001"})),
        ("render.library.describe", json!({"kind": "materials", "id": "MAT-999"})),
        ("render.library.describe", json!({"kind": "physical_profiles", "id": "no-such-profile"})),
        ("render.library.targets", json!({"limit": 0})),
        ("render.library.targets", json!({"limit": 129})),
        ("render.library.targets", json!({"query": long})),
        ("render.library.targets", json!({"offset": 262_145})),
        ("render.library.preview", json!({"id": "MAT-001"})),
        ("render.library.preview", json!({"id": ""})),
    ]);
    // The six apply and prepare commands: the handlers' own messages, for a key left out and for
    // a value of the wrong type, which the types read as the handlers' structs do.
    let (rig, set) = (json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1}), json!({"width": 4}));
    assert_strict_keeps_todays_errors(&[
        ("render.library.appearance.apply", json!({})),
        ("render.library.appearance.apply", json!({"id": "SURF-001"})),
        ("render.library.appearance.apply", json!({"id": 5, "variant": "uniform_opaque", "bodies": ["Target"]})),
        ("render.library.appearance.apply", json!({"id": null, "variant": "uniform_opaque", "bodies": ["Target"]})),
        ("render.library.appearance.apply", json!({"id": "SURF-001", "variant": "uniform_opaque", "bodies": "Target"})),
        ("render.library.appearance.apply", json!({"id": "SURF-001", "variant": "uniform_opaque", "bodies": [7]})),
        ("render.library.appearance.apply", json!({"id": "SURF-001", "variant": "other", "bodies": ["Target"]})),
        ("render.library.appearance.apply", json!({"id": "SURF-001", "variant": "uniform_opaque", "bodies": []})),
        ("render.library.appearance.apply", json!({"id": "SURF-001", "variant": "uniform_opaque", "bodies": ["Nope"]})),
        ("render.library.environment.apply", json!({})),
        ("render.library.environment.apply", json!({"id": "HDR-001"})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": 3})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "set": 5})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "set": null})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "rotation_rad": "x"})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "rotation_rad": null})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "intensity": "x"})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "samples": -1})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "samples": 1.5})),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "replace_environment": "yes"})),
        (
            "render.library.environment.apply",
            json!({"id": "HDR-001", "name": "E", "set": {"job": {"method": "gpu_path", "options": {}}}, "rotation_rad": 1e9}),
        ),
        ("render.library.environment.apply", json!({"id": "HDR-001", "name": "E", "set": set})),
        ("render.library.physical.prepare", json!({})),
        ("render.library.physical.prepare", json!({"id": "steel-304-plate", "name": "R"})),
        ("render.library.physical.prepare", json!({"id": "steel-304-plate", "name": "R", "acknowledge_reference": false})),
        ("render.library.physical.prepare", json!({"id": "steel-304-plate", "name": "R", "acknowledge_reference": "true"})),
        ("render.library.physical.prepare", json!({"id": "steel-304-plate", "name": "R", "acknowledge_reference": null})),
        ("render.library.physical.prepare", json!({"id": "steel-304-plate", "name": " R", "acknowledge_reference": true})),
        ("render.library.physical.prepare", json!({"id": 1, "name": "R", "acknowledge_reference": true})),
        ("render.library.physical.prepare", json!({"id": "no-such-profile", "name": "R", "acknowledge_reference": true})),
        ("render.library.surface.prepare", json!({})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": "0", "body_name": "Target"})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": -1, "body_name": "Target"})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0.0, "body_name": "Target"})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target", "origin": [1, 2]})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target", "origin": {"x": 1, "y": 2, "z": 3}})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target", "plane": "XY"})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target", "plane": null})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target", "scale": null})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target", "scale": 0})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target", "rotation_rad": null})),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target"})),
        ("render.library.studio.prepare", json!({})),
        ("render.library.studio.prepare", json!({"id": "RIG-002", "variant": "complete"})),
        ("render.library.studio.prepare", json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0], "subject_extent_mm": 1})),
        ("render.library.studio.prepare", json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": "1"})),
        ("render.library.studio.prepare", json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 0})),
        ("render.library.studio.prepare", json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "name": 5})),
        (
            "render.library.studio.prepare",
            json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "set": null}),
        ),
        (
            "render.library.studio.prepare",
            json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "set": set}),
        ),
        (
            "render.library.studio.prepare",
            json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "enable_indirect": true}),
        ),
        (
            "render.library.studio.prepare",
            json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "enable_indirect": null}),
        ),
        (
            "render.library.studio.prepare",
            json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "name": "R"}),
        ),
        ("render.library.studio.apply", json!({})),
        ("render.library.studio.apply", json!({"rig": rig})),
        ("render.library.studio.apply", json!({"rig": rig, "name": "R"})),
        ("render.library.studio.apply", json!({"rig": rig, "name": "R", "replace_lighting_and_environment": false})),
        ("render.library.studio.apply", json!({"rig": 5, "name": "R", "replace_lighting_and_environment": true})),
        ("render.library.studio.apply", json!({"rig": null, "name": "R", "replace_lighting_and_environment": true})),
        ("render.library.studio.apply", json!({"rig": {"id": "RIG-002"}, "name": "R", "replace_lighting_and_environment": true})),
        (
            "render.library.studio.apply",
            json!({"rig": {"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "x": 1}, "name": "R", "replace_lighting_and_environment": true}),
        ),
        (
            "render.library.studio.apply",
            json!({"rig": {"id": "RIG-002", "variant": "complete", "center_mm": "a", "subject_extent_mm": 1}, "name": "R", "replace_lighting_and_environment": true}),
        ),
        ("render.library.studio.apply", json!({"rig": rig, "name": 3, "replace_lighting_and_environment": true})),
        ("render.library.studio.apply", json!({"rig": rig, "name": "R", "replace_lighting_and_environment": true, "expected_revision": -1})),
        ("render.library.studio.apply", json!({"rig": rig, "name": "R", "replace_lighting_and_environment": true, "expected_revision": 99999})),
        ("render.library.studio.apply", json!({"rig": rig, "name": "R", "replace_lighting_and_environment": true, "replace_stage": "x"})),
        ("render.library.studio.apply", json!({"rig": rig, "name": "R", "replace_lighting_and_environment": true, "set": 5})),
    ]);
}

/// #11443: the four commands reply as their output schemas say, in Warn without warnings. Every
/// resource of every catalog, physical profiles included, is listed through its pages, and the
/// first of each is described. Like the handlers, warn and strict refuse a misspelt key.
#[test]
fn render_library_commands_reply_with_their_schemas() {
    let (page, described, targets, preview) = (
        output_schema_for::<LibraryPage>(),
        output_schema_for::<LibraryDescription>(),
        output_schema_for::<LibraryTargets>(),
        output_schema_for::<LibraryPreview>(),
    );
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: &schemars::Schema, r: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
    };
    let kinds = [
        "capabilities",
        "environments",
        "hdr",
        "lighting",
        "materials",
        "native_scenes",
        "physical_profiles",
        "quality",
        "rigs",
        "scenes",
        "selene_appearances",
        "selene_shots",
        "surface_manifest",
        "surfaces",
    ];
    for kind in kinds {
        let mut offset = Some(0);
        let mut first = None;
        let mut listed = 0;
        while let Some(at) = offset {
            let r = run(&mut s, "render.library.list", json!({"kind": kind, "offset": at, "limit": 128}));
            matches(&page, &r);
            let items = r["items"].as_array().cloned().unwrap_or_default();
            first = first.or_else(|| items.first().map(|i| i["id"].clone()));
            listed += items.len();
            offset = r["next_offset"].as_u64();
        }
        assert!(listed > 0, "{kind} lists nothing");
        let id = first.unwrap_or_else(|| panic!("no first {kind}"));
        let r = run(&mut s, "render.library.describe", json!({"kind": kind, "id": id}));
        matches(&described, &r);
        assert_eq!(r["physical_references"].is_object(), matches!(kind, "materials" | "surfaces"), "{kind}: {r}");
    }
    matches(&page, &run(&mut s, "render.library.list", json!({"query": "brushed", "availability": "usable", "offset": 0, "limit": 5})));
    matches(&page, &run(&mut s, "render.library.list", json!({"kind": "physical_profiles", "availability": null})));
    let empty = run(&mut s, "render.library.targets", json!({}));
    matches(&targets, &empty);
    assert!(empty["bounds"].is_null(), "{empty}");
    let created = run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 5, "body_name": "Target"}));
    run(
        &mut s,
        "appearance.assign",
        json!({"bodies": ["Target"], "color": [180, 120, 70], "pbr": {"metallic": 1.0, "roughness": 0.3, "dielectric_ior": 1.5}}),
    );
    let r = run(&mut s, "render.library.targets", json!({"query": "targ", "offset": 0, "limit": 1}));
    matches(&targets, &r);
    assert_eq!((r["total"].as_u64(), r["items"][0]["name"].as_str()), (Some(1), Some("Target")), "{r}");
    assert!(r["items"][0]["pbr"].is_object() && r["bounds"].is_object(), "{r}");
    assert_eq!(
        r["items"][0]["stable_target"],
        json!({"definition":"Target","feature":created["feature"],"component":0,"occurrence_path":[]}),
        "the typed reply must preserve the saved root-body identity"
    );
    let mut without_stable = r.clone();
    without_stable["items"][0] = serde_json::to_value(RenderTarget {
        ordinal: 0,
        name: "Unbound".into(),
        stable_target: None,
        color: [180, 120, 70],
        opacity: 1.0,
        pbr: None,
    })
    .unwrap();
    assert_eq!(without_stable["items"][0].get("stable_target"), Some(&Value::Null), "an unavailable identity stays an explicit null");
    matches(&targets, &without_stable);
    let authority =
        solidcraft_doc::render_surface::RenderBodyTarget { definition: "Part".into(), feature: 7, component: 3, occurrence_path: vec![11, 3, 6] };
    authority.validate().unwrap();
    let mut nested = r.clone();
    nested["items"][0]["stable_target"] = serde_json::to_value(StableRenderTarget::from(&authority)).unwrap();
    matches(&targets, &nested);
    assert_eq!(
        serde_json::from_value::<solidcraft_doc::render_surface::RenderBodyTarget>(nested["items"][0]["stable_target"].clone()).unwrap(),
        authority
    );
    nested["items"][0]["stable_target"]["occurrence_path"] = json!([11, "not an occurrence id", 6]);
    assert!(!conforms(targets.as_value(), targets.as_value(), &nested), "the reply schema must describe the stable identity's actual fields");
    if cfg!(feature = "bundled-render-library") {
        matches(&preview, &run(&mut s, "render.library.preview", json!({"id": "SURF-001"})));
    }
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = Session { param_check: level, ..Session::default() };
        let error = s.execute("render.library.list", &json!({"knd": "hdr"})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("knd"), "{level:?}: {error}");
    }
}

/// A design with one box, `Target`, and a saved CPU setup, `Cpu`, at the check level `level`.
fn with_a_target(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("solid.box", json!({"length": 10, "width": 20, "height": 5, "body_name": "Target"})),
        ("render.settings.set", json!({"name": "Cpu", "set": {"width": 8, "height": 8, "job": {"method": "cpu_raytrace", "options": {}}}})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

/// Every call, of the six commands, that reads its keys: working calls in every form, and calls
/// the handlers refuse for a key left out, a value of the wrong type or a value out of range.
fn apply_and_prepare_calls() -> Vec<(&'static str, Value)> {
    let gpu = json!({"job": {"method": "gpu_path", "options": {}}});
    let cpu = json!({"width": 8, "height": 8, "job": {"method": "cpu_raytrace", "options": {}}});
    let rig = json!({"id": "RIG-002", "variant": "complete", "center_mm": [10, 20, 30], "subject_extent_mm": 100});
    let appearance = |extra: Value| {
        let mut p = json!({"id": "SURF-001", "variant": "uniform_opaque", "bodies": ["Target"]});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let environment = |extra: Value| {
        let mut p = json!({"id": "HDR-001", "name": "E", "set": cpu});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let physical = |extra: Value| {
        let mut p = json!({"id": "steel-304-plate", "name": "Reference", "acknowledge_reference": true});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let surface = |extra: Value| {
        let mut p = json!({"id": "SURF-001", "body_index": 0, "body_name": "Target"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let prepare = |extra: Value| {
        let mut p = rig.clone();
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let apply = |extra: Value| {
        let mut p = json!({"rig": rig, "name": "Rig", "set": gpu, "replace_lighting_and_environment": true});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    let mut calls = vec![];
    let mut add = |id: &'static str, p: Value| calls.push((id, p));
    let (a, e, ph, su, pr, ap) = (
        "render.library.appearance.apply",
        "render.library.environment.apply",
        "render.library.physical.prepare",
        "render.library.surface.prepare",
        "render.library.studio.prepare",
        "render.library.studio.apply",
    );
    for p in [
        json!({}),
        json!({"id": "SELENE-AP-001"}),
        json!({"id": "MAT-001"}),
        json!({"id": 5}),
        json!({"id": null}),
        json!({"variant": "x"}),
        json!({"variant": null}),
        json!({"bodies": []}),
        json!({"bodies": "Target"}),
        json!({"bodies": [1]}),
        json!({"bodies": null}),
        json!({"bodies": ["Nope"]}),
        json!({"bodies": ["Target", "Target"]}),
    ] {
        add(a, appearance(p));
    }
    add(a, json!({}));
    add(a, json!({"id": "SURF-001"}));
    add(a, json!({"id": "SURF-001", "variant": "uniform_opaque"}));
    for id in ["SELENE-AP-001", "SELENE-AP-002", "SURF-002"] {
        add(a, json!({"id": id, "variant": "uniform_opaque", "bodies": ["Target"]}));
    }
    for p in [
        json!({}),
        json!({"rotation_rad": 0.5, "intensity": 2, "samples": 4, "replace_environment": true}),
        json!({"rotation_rad": 1, "intensity": 0, "samples": 1}),
        json!({"set": gpu, "intensity": 3, "samples": 5}),
        json!({"set": gpu, "intensity": 300}),
        json!({"set": {}}),
        json!({"set": null}),
        json!({"set": 5}),
        json!({"set": [1]}),
        json!({"rotation_rad": null}),
        json!({"rotation_rad": "x"}),
        json!({"rotation_rad": 1e9}),
        json!({"intensity": null}),
        json!({"intensity": "x"}),
        json!({"samples": null}),
        json!({"samples": -1}),
        json!({"samples": 1.5}),
        json!({"samples": 3}),
        json!({"samples": 4294967296u64}),
        json!({"replace_environment": null}),
        json!({"replace_environment": "yes"}),
        json!({"id": 5}),
        json!({"name": 3}),
        json!({"name": null}),
        json!({"name": " E"}),
    ] {
        add(e, environment(p));
    }
    add(e, json!({}));
    add(e, json!({"id": "HDR-001"}));
    add(e, json!({"name": "E"}));
    for p in [
        json!({}),
        json!({"name": "R"}),
        json!({"acknowledge_reference": false}),
        json!({"acknowledge_reference": "true"}),
        json!({"acknowledge_reference": null}),
        json!({"name": " R"}),
        json!({"name": ""}),
        json!({"name": 4}),
        json!({"id": 1}),
        json!({"id": "no-such-profile"}),
        json!({"id": "../profiles"}),
        json!({"id": "aluminum-6061-t6"}),
    ] {
        add(ph, physical(p));
    }
    add(ph, json!({}));
    add(ph, json!({"id": "steel-304-plate", "name": "R"}));
    add(ph, json!({"name": "R", "acknowledge_reference": true}));
    for p in [
        json!({}),
        json!({"origin": [1, 2, 3], "scale": 2.5, "plane": "yz", "rotation_rad": 0.4}),
        json!({"plane": "zx"}),
        json!({"plane": "xy"}),
        json!({"origin": null}),
        json!({"origin": [1, 2]}),
        json!({"origin": [1, 2, 3, 4]}),
        json!({"origin": ["a", 2, 3]}),
        json!({"origin": {"x": 1, "y": 2, "z": 3}}),
        json!({"plane": "XY"}),
        json!({"plane": null}),
        json!({"plane": 1}),
        json!({"scale": null}),
        json!({"scale": 0}),
        json!({"scale": "2"}),
        json!({"rotation_rad": null}),
        json!({"rotation_rad": 1e7}),
        json!({"body_index": "0"}),
        json!({"body_index": -1}),
        json!({"body_index": 1.5}),
        json!({"body_index": 1}),
        json!({"body_name": 1}),
        json!({"body_name": "Other"}),
        json!({"id": "MAT-001"}),
    ] {
        add(su, surface(p));
    }
    add(su, json!({}));
    add(su, json!({"id": "SURF-001", "body_index": 0}));
    add(su, json!({"id": "SURF-001", "body_name": "Target"}));
    add(su, json!({"body_index": 0, "body_name": "Target"}));
    for p in [
        json!({}),
        json!({"variant": "emitters_only"}),
        json!({"id": "RIG-001", "variant": "emitters_only"}),
        json!({"id": "RIG-001", "variant": "complete"}),
        json!({"id": "RIG-020", "name": "Cpu", "set": cpu, "enable_indirect": true}),
        json!({"id": "RIG-020", "name": "Cpu", "set": cpu}),
        json!({"name": "Cpu"}),
        json!({"name": "Cpu", "set": {"width": 16}}),
        json!({"name": null}),
        json!({"name": 5}),
        json!({"name": "Cpu", "set": null}),
        json!({"name": "Cpu", "set": 5}),
        json!({"set": {"width": 4}}),
        json!({"enable_indirect": true}),
        json!({"enable_indirect": false}),
        json!({"enable_indirect": null}),
        json!({"enable_indirect": "yes"}),
        json!({"name": "Rig", "set": gpu}),
        json!({"center_mm": [0, 0]}),
        json!({"center_mm": [0, 0, 0, 0]}),
        json!({"center_mm": [0, "a", 0]}),
        json!({"center_mm": [1e13, 0, 0]}),
        json!({"center_mm": null}),
        json!({"subject_extent_mm": 0}),
        json!({"subject_extent_mm": "100"}),
        json!({"subject_extent_mm": null}),
        json!({"variant": 1}),
        json!({"id": 1}),
    ] {
        add(pr, prepare(p));
    }
    for key in ["id", "variant", "center_mm", "subject_extent_mm"] {
        let mut p = rig.clone();
        p.as_object_mut().unwrap().remove(key);
        add(pr, p);
    }
    add(pr, json!({}));
    for p in [
        json!({}),
        json!({"set": cpu}),
        json!({"set": cpu, "enable_indirect": true}),
        json!({"set": cpu, "enable_indirect": null}),
        json!({"set": cpu, "enable_indirect": "x"}),
        json!({"rig": {"id": "RIG-020", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 100}, "set": cpu}),
        json!({"rig": {"id": "RIG-020", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 100}, "set": cpu, "enable_indirect": true}),
        json!({"rig": {"id": "RIG-001", "variant": "emitters_only", "center_mm": [0, 0, 0], "subject_extent_mm": 100}}),
        json!({"set": null}),
        json!({"set": 5}),
        json!({"set": {"width": 0}}),
        json!({"replace_stage": true}),
        json!({"replace_stage": null}),
        json!({"replace_stage": "x"}),
        json!({"expected_revision": 3}),
        json!({"expected_revision": null}),
        json!({"expected_revision": 99999}),
        json!({"expected_revision": -1}),
        json!({"expected_revision": "3"}),
        json!({"replace_lighting_and_environment": false}),
        json!({"replace_lighting_and_environment": null}),
        json!({"replace_lighting_and_environment": 1}),
        json!({"name": 3}),
        json!({"name": "Rig", "set": {}}),
        json!({"rig": 5}),
        json!({"rig": null}),
        json!({"rig": {}}),
        json!({"rig": {"id": "RIG-002"}}),
        json!({"rig": {"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0]}}),
        json!({"rig": {"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1, "extra": 1}}),
        json!({"rig": {"id": 2, "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 1}}),
        json!({"rig": {"id": "RIG-002", "variant": "complete", "center_mm": "a", "subject_extent_mm": 1}}),
        json!({"rig": {"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": null}}),
        json!({"rig": {"id": "RIG-002", "variant": "other", "center_mm": [0, 0, 0], "subject_extent_mm": 1}}),
    ] {
        add(ap, apply(p));
    }
    for key in ["rig", "name", "replace_lighting_and_environment"] {
        let mut p = apply(json!({}));
        p.as_object_mut().unwrap().remove(key);
        add(ap, p);
    }
    calls
}

/// The types read each key as the handlers do: a call the handler accepts is accepted at every
/// check level, with no `param_warnings`, and a call it refuses gets its own message in warn and
/// in strict. That includes calls with a key left out or `null`, which the handler's struct
/// refuses or reads as absent, depending on the key.
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    let calls = apply_and_prepare_calls();
    assert!(calls.len() > 150, "{}", calls.len());
    let mut tally = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    for (id, p) in &calls {
        let run = |level: ParamCheck| {
            let mut s = with_a_target(level);
            s.execute(id, p).map_err(|e| e.to_string())
        };
        let off = run(ParamCheck::Off);
        assert_eq!(run(ParamCheck::Warn), off, "warn: {id} {p}");
        assert_eq!(run(ParamCheck::Strict), off, "strict: {id} {p}");
        let count = tally.entry(id).or_default();
        match &off {
            Ok(reply) => {
                count.0 += 1;
                assert!(reply.get("param_warnings").is_none(), "{id} {p}: {reply}");
                assert!(crate::schema::issues(id, p).is_empty(), "{id} {p}");
            }
            Err(_) => count.1 += 1,
        }
    }
    // Every command ran calls of both kinds (the HDR ones accept only in a build with the bundled
    // library).
    assert_eq!(tally.len(), 6, "{tally:?}");
    for (id, (accepted, refused)) in &tally {
        let bundled = cfg!(feature = "bundled-render-library") || !id.ends_with("environment.apply");
        assert!((*accepted >= 2 || !bundled) && *refused >= 3, "{id}: {accepted} accepted, {refused} refused");
    }
}

/// The schemas take what the handlers read: the keys a handler refuses without are required, a
/// default is the one the handler fills in, and a value of the wrong type is refused.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let requires = |schema: schemars::Schema, keys: &[&str]| {
        let mut required: Vec<_> = schema.as_value()["required"].as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_string()).collect();
        required.sort();
        assert_eq!(required, keys);
    };
    requires(input_schema_for::<AppearanceApplyParams>(), &["bodies", "id", "variant"]);
    requires(input_schema_for::<EnvironmentApplyParams>(), &["id", "name"]);
    requires(input_schema_for::<PhysicalPrepareParams>(), &["acknowledge_reference", "id", "name"]);
    requires(input_schema_for::<SurfacePrepareParams>(), &["body_index", "body_name", "id"]);
    requires(input_schema_for::<StudioPrepareParams>(), &["center_mm", "id", "subject_extent_mm", "variant"]);
    requires(input_schema_for::<StudioApplyParams>(), &["name", "replace_lighting_and_environment", "rig"]);
    let defaults = |schema: schemars::Schema, key: &str| schema.as_value()["properties"][key]["default"].clone();
    assert_eq!(defaults(input_schema_for::<EnvironmentApplyParams>(), "rotation_rad"), json!(0.0));
    assert_eq!(defaults(input_schema_for::<EnvironmentApplyParams>(), "intensity"), json!(1.0));
    assert_eq!(defaults(input_schema_for::<EnvironmentApplyParams>(), "samples"), json!(16));
    assert_eq!(defaults(input_schema_for::<EnvironmentApplyParams>(), "replace_environment"), json!(false));
    assert_eq!(defaults(input_schema_for::<EnvironmentApplyParams>(), "set"), json!({}));
    assert_eq!(defaults(input_schema_for::<SurfacePrepareParams>(), "scale"), json!(1.0));
    assert_eq!(defaults(input_schema_for::<SurfacePrepareParams>(), "plane"), json!("xy"));
    assert_eq!(defaults(input_schema_for::<SurfacePrepareParams>(), "rotation_rad"), json!(0.0));
    assert_eq!(defaults(input_schema_for::<StudioPrepareParams>(), "enable_indirect"), json!(false));
    assert_eq!(defaults(input_schema_for::<StudioApplyParams>(), "replace_stage"), json!(false));
    // Every call that works conforms to its schema; so does no call that is missing a required
    // key or has a value of the wrong type.
    let mut conforming = 0;
    for (id, p) in apply_and_prepare_calls() {
        let schema = crate::cmd::find_command(id).and_then(|c| c.contract).map(|c| c.input_schema()).unwrap();
        let schema = schema.as_value();
        let works = with_a_target(ParamCheck::Off).execute(id, &p).is_ok();
        let fits = conforms(schema, schema, &p);
        assert!(!works || fits, "{id} {p} works but doesn't match its schema");
        conforming += usize::from(fits);
        let object = p.as_object().unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().all(|k| object.contains_key(k.as_str().unwrap())) || !fits, "{id} {p}");
    }
    assert!(conforming > 40, "{conforming}");
}

/// The id pattern, the `const: true` confirmations and the cap on `set` are in the schemas, so
/// that a client sees them without calling.
#[test]
fn the_id_pattern_the_confirmations_and_the_set_cap_are_pinned() {
    let id = json!({"type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$"});
    let property = |schema: schemars::Schema, key: &str| schema.as_value()["properties"][key].clone();
    for (name, schema) in [
        ("appearance", input_schema_for::<AppearanceApplyParams>()),
        ("environment", input_schema_for::<EnvironmentApplyParams>()),
        ("surface", input_schema_for::<SurfacePrepareParams>()),
        ("studio prepare", input_schema_for::<StudioPrepareParams>()),
    ] {
        let found = property(schema, "id");
        assert_eq!((&found["type"], &found["pattern"]), (&id["type"], &id["pattern"]), "{name}");
    }
    let confirmed = json!({"type": "boolean", "const": true});
    for (schema, key) in [
        (input_schema_for::<PhysicalPrepareParams>(), "acknowledge_reference"),
        (input_schema_for::<StudioApplyParams>(), "replace_lighting_and_environment"),
    ] {
        let found = property(schema, key);
        assert_eq!((&found["type"], &found["const"]), (&confirmed["type"], &confirmed["const"]), "{key}");
    }
    for (name, schema) in [
        ("environment", input_schema_for::<EnvironmentApplyParams>()),
        ("studio prepare", input_schema_for::<StudioPrepareParams>()),
        ("studio apply", input_schema_for::<StudioApplyParams>()),
    ] {
        let set = property(schema, "set");
        assert_eq!(set["maxProperties"], json!(15), "{name}");
        assert_eq!(set["type"], json!("object"), "{name}");
    }
    // The schema says what the handlers do about them.
    let apply = |p: Value| with_a_target(ParamCheck::Off).execute("render.library.studio.apply", &p).is_err();
    assert!(apply(json!({"name": "Rig", "rig": {"id": "RIG-001", "variant": "emitters_only", "center_mm": [0, 0, 0], "subject_extent_mm": 1}})));
    let physical = with_a_target(ParamCheck::Off)
        .execute("render.library.physical.prepare", &json!({"id": "PHYS-001", "name": "x", "acknowledge_reference": false}));
    assert!(physical.is_err());
    // Without `name`, enable_indirect true is refused; with it, it makes no difference to a prepare.
    let prepare = |p: Value| with_a_target(ParamCheck::Off).execute("render.library.studio.prepare", &p);
    let rig = json!({"id": "RIG-001", "variant": "emitters_only", "center_mm": [0, 0, 0], "subject_extent_mm": 100});
    let mut with = rig.clone();
    with["enable_indirect"] = json!(true);
    assert!(prepare(with).is_err());
    let description = property(input_schema_for::<StudioPrepareParams>(), "enable_indirect")["description"].as_str().unwrap().to_string();
    assert!(description.contains("no effect when `name` is given") && description.contains("refused"), "{description}");
}

/// The six commands reply as their output schemas say, in Warn without warnings: every surface,
/// Selene appearance, physical profile and rig, in each form the commands take.
#[test]
fn apply_and_prepare_commands_reply_with_their_schemas() {
    let (appearance, environment, physical, surface, prepared, applied) = (
        output_schema_for::<AppearanceApplied>(),
        output_schema_for::<EnvironmentApplied>(),
        output_schema_for::<PhysicalPlan>(),
        output_schema_for::<SurfacePrepared>(),
        output_schema_for::<StudioPrepared>(),
        output_schema_for::<StudioApplied>(),
    );
    let ids = |kind: &str| -> Vec<String> {
        let mut s = Session::default();
        let r = s.execute("render.library.list", &json!({"kind": kind, "limit": 128})).unwrap();
        r["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
    };
    let run = |schema: &schemars::Schema, id: &str, p: Value| -> Result<Value, String> {
        let mut s = with_a_target(ParamCheck::Warn);
        let r = s.execute(id, &p).map_err(|e| e.to_string())?;
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{id} {p}: {r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        Ok(r)
    };
    let mut made = [0usize; 6];
    for id in ids("surfaces").into_iter().chain(ids("selene_appearances")) {
        let r = run(&appearance, "render.library.appearance.apply", json!({"id": id, "variant": "uniform_opaque", "bodies": ["Target"]}));
        made[0] += usize::from(r.is_ok());
    }
    for id in ids("surfaces") {
        for plane in ["xy", "yz", "zx"] {
            let p = json!({"id": id, "body_index": 0, "body_name": "Target", "plane": plane, "scale": 2, "rotation_rad": 0.3, "origin": [1, 2, 3]});
            made[1] += usize::from(run(&surface, "render.library.surface.prepare", p).is_ok());
        }
        let p = json!({"id": id, "body_index": 0, "body_name": "Target", "origin": null});
        made[1] += usize::from(run(&surface, "render.library.surface.prepare", p).is_ok());
    }
    for id in ids("physical_profiles") {
        let r = run(&physical, "render.library.physical.prepare", json!({"id": id, "name": "Reference", "acknowledge_reference": true}));
        made[2] += usize::from(r.is_ok());
    }
    let cpu = json!({"width": 8, "height": 8, "job": {"method": "cpu_raytrace", "options": {}}});
    let gpu = json!({"width": 8, "height": 8, "job": {"method": "gpu_path", "options": {}}});
    for id in ids("rigs") {
        for variant in ["complete", "emitters_only"] {
            let rig = json!({"id": id, "variant": variant, "center_mm": [10, 20, 30], "subject_extent_mm": 100});
            made[3] += usize::from(run(&prepared, "render.library.studio.prepare", rig.clone()).is_ok());
            for extra in [json!({"name": "Cpu"}), json!({"name": "Rig", "set": cpu, "enable_indirect": true}), json!({"name": null, "set": {}})] {
                let mut p = rig.clone();
                p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
                made[3] += usize::from(run(&prepared, "render.library.studio.prepare", p).is_ok());
            }
            for (set, indirect) in [(&gpu, false), (&cpu, false), (&cpu, true)] {
                let p = json!({"rig": rig, "name": "Rig", "set": set, "replace_lighting_and_environment": true, "enable_indirect": indirect,
                    "replace_stage": false, "expected_revision": null});
                made[4] += usize::from(run(&applied, "render.library.studio.apply", p).is_ok());
            }
        }
    }
    if cfg!(feature = "bundled-render-library") {
        for id in ids("hdr") {
            for set in [&cpu, &gpu] {
                let p = json!({"id": id, "name": "E", "set": set, "rotation_rad": 0.5, "intensity": 2, "samples": 4, "replace_environment": false});
                made[5] += usize::from(run(&environment, "render.library.environment.apply", p).is_ok());
            }
        }
        let p = json!({"id": "HDR-001", "name": "Cpu", "replace_environment": true});
        made[5] += usize::from(run(&environment, "render.library.environment.apply", p).is_ok());
    }
    // Surfaces and appearances, profiles with one density, rigs with a CPU or GPU projection:
    // each command made some replies, and `physical.prepare` refuses the profiles without one.
    assert!(made[0] >= 90 && made[1] >= 190 && (40..=60).contains(&made[2]) && made[3] >= 50 && made[4] >= 30, "{made:?}");
    assert!(!cfg!(feature = "bundled-render-library") || made[5] >= 45, "{made:?}");
    // Exact fields, not only the shape.
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let r = s
        .execute(
            "render.library.studio.prepare",
            &json!({"id": "RIG-001", "variant": "complete", "center_mm": [10, 20, 30], "subject_extent_mm": 100}),
        )
        .unwrap();
    assert_eq!((r["projection"]["passive_cards_included"].as_u64(), r["stage"][0]["id"].as_str()), (Some(1), Some("RIG-001_fill_card")), "{r}");
    assert!(r.get("setup").is_none() && r.get("indirect_adoption").is_none(), "without `name`, no CPU projection: {r}");
}

/// Like the handlers' structs, which refuse a key they don't know, warn and strict refuse a
/// misspelt key. What warn reports for a call the handler accepted (`issues`) is the same issue
/// strict refuses with: the key, and the one it was probably meant to be.
#[test]
fn a_misspelt_key_is_reported_in_warn_and_refused_in_strict() {
    let cases = [
        ("render.library.appearance.apply", json!({"id": "SURF-001", "variant": "uniform_opaque", "bodies": ["Target"]}), "bodie", "bodies"),
        (
            "render.library.physical.prepare",
            json!({"id": "steel-304-plate", "name": "R", "acknowledge_reference": true}),
            "acknowledge_referense",
            "acknowledge_reference",
        ),
        ("render.library.surface.prepare", json!({"id": "SURF-001", "body_index": 0, "body_name": "Target"}), "scal", "scale"),
        (
            "render.library.studio.prepare",
            json!({"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 100}),
            "subject_extent",
            "subject_extent_mm",
        ),
        (
            "render.library.studio.apply",
            json!({"rig": {"id": "RIG-002", "variant": "complete", "center_mm": [0, 0, 0], "subject_extent_mm": 100}, "name": "Rig",
                "set": {"job": {"method": "gpu_path", "options": {}}}, "replace_lighting_and_environment": true}),
            "replace_stag",
            "replace_stage",
        ),
        (
            "render.library.environment.apply",
            json!({"id": "HDR-001", "name": "E", "set": {"width": 64, "height": 32, "job": {"method": "cpu_raytrace", "options": {}}}}),
            "intensty",
            "intensity",
        ),
    ];
    for (id, mut p, typo, key) in cases {
        let ok = p.clone();
        p[typo] = json!(1);
        assert!(crate::schema::issues(id, &ok).is_empty(), "{id}: the call without the typo is fine");
        let issues = crate::schema::issues(id, &p);
        let want = format!("unknown key `{typo}`; did you mean `{key}`?");
        assert_eq!(issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>(), [want.as_str()], "{id}");
        for level in [ParamCheck::Warn, ParamCheck::Strict] {
            let mut s = with_a_target(level);
            let error = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(error.contains(typo), "{id} {level:?}: {error}");
            assert_eq!(error.contains(&want), level == ParamCheck::Strict, "{id} {level:?}: {error}");
        }
    }
}
