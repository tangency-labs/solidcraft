use serde_json::{Value, json};

use super::{
    LibraryDescribeParams, LibraryDescription, LibraryListParams, LibraryPage, LibraryPreview, LibraryPreviewParams, LibraryTargets,
    LibraryTargetsParams, RenderTarget, StableRenderTarget,
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
