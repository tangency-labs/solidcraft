use serde_json::{Value, json};

use super::{OpticsBindParams, OpticsBindingSaved, OpticsUnbindParams, OpticsUnbound, SurfaceBindParams, SurfaceBindingSaved, SurfaceUnbindParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A design with one box, `Lens`, and a saved render setup, `Studio`, that renders with the GPU
/// path tracer, at the parameter-check level `level`.
fn with_a_studio(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "Lens"})),
        ("render.settings.set", json!({"name": "Studio", "set": {"job": {"method": "gpu_path"}}})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

/// `render.optics.bind`'s call for the box, at the design's revision `revision`.
fn optics_on_the_lens(revision: u64) -> Value {
    json!({"name": "Studio", "body_index": 0, "body_name": "Lens", "expected_revision": revision,
        "optics": {"ior": 1.45, "subsurface": {"scattering_per_mm": 0.4, "anisotropy": 0.2}}})
}

/// The identity `Studio` keeps for the box once its optics are bound, as the unbind commands take it.
fn lens_target(s: &Session) -> Value {
    let target = s.doc.render.setups.get("Studio").and_then(|setup| setup.optics.iter().next()).map(|b| b.target.clone());
    serde_json::to_value(target.unwrap_or_else(|| panic!("no optics bound"))).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SurfaceBindParams>(), &["name", "replace", "set", "surface"]);
    assert_keys(input_schema_for::<SurfaceUnbindParams>(), &["name", "set", "target"]);
    assert_keys(input_schema_for::<OpticsBindParams>(), &["body_index", "body_name", "expected_revision", "name", "optics", "replace", "set"]);
    assert_keys(input_schema_for::<OpticsUnbindParams>(), &["expected_revision", "name", "target"]);
}

#[test]
fn advanced_glass_optics_matches_the_schema_and_the_saved_binding() {
    let input = input_schema_for::<OpticsBindParams>();
    let input = input.as_value();
    let optics = &input["properties"]["optics"];
    for (value, expected) in [
        (json!({}), true),
        (json!({"model": null}), true),
        (json!({"model": {}}), true),
        (json!({"model": {"dispersion": null, "thin_film": null}}), true),
        (json!({"model": {"roughness": 1.01}}), false),
        (json!({"model": {"dispersion": {}}}), false),
        (json!({"model": {"dispersion": {"abbe_number": 4.99}}}), false),
        (json!({"model": {"thin_film": {"ior": 1.37}}}), false),
        (json!({"model": {"thin_film": {"ior": 3.01, "thickness_nm": 263}}}), false),
        (json!({"model": {"thin_film": {"ior": 1.37, "thickness_nm": 100001}}}), false),
        (json!({"model": {"thin_film": {"ior": 1.37, "thickness_nm": 263, "extra": true}}}), false),
    ] {
        assert_eq!(conforms(input, optics, &value), expected, "{value}");
    }

    let mut session = with_a_studio(ParamCheck::Strict);
    let mut request = optics_on_the_lens(session.revision);
    let model = json!({"roughness": 0.43, "dispersion": {"abbe_number": 57.0}, "thin_film": {"ior": 1.37, "thickness_nm": 263.0}});
    request["optics"] = json!({"ior": 1.45, "model": &model});
    assert!(conforms(input, input, &request), "{request}");
    let reply = session.execute("render.optics.bind", &request).unwrap();
    assert_eq!(reply["setup"]["optics"][0]["optics"]["model"], model);
    assert_eq!(reply["geometry_and_engineering_unchanged"], true);
}

/// The handlers refuse these calls after reading them; strict refuses them with their message.
/// They need a body, a saved setup and the design's revision, so the shared helper, which has
/// a sketch only, doesn't fit.
#[test]
fn strict_keeps_todays_errors() {
    let stranger = json!({"definition": "", "feature": 1, "component": 0, "occurrence_path": []});
    let cases: Vec<(&str, Box<dyn Fn(u64) -> Value>)> = vec![
        (
            "render.optics.bind",
            Box::new(|_| json!({"name": "Studio", "body_index": 0, "body_name": "Lens", "expected_revision": 999_999, "optics": {}})),
        ),
        (
            "render.optics.bind",
            Box::new(|r| json!({"name": "Studio", "body_index": 0, "body_name": "Lens", "expected_revision": r, "optics": {"ior": 0.5}})),
        ),
        (
            "render.optics.bind",
            Box::new(|r| json!({"name": "Studio", "body_index": 0, "body_name": "Lens", "expected_revision": r, "optics": {"thickness_mm": 1}})),
        ),
        ("render.optics.bind", Box::new(|r| json!({"name": "Studio", "body_index": 3, "body_name": "Lens", "expected_revision": r, "optics": {}}))),
        ("render.optics.bind", Box::new(|r| json!({"name": "Studio", "body_index": 0, "body_name": "Nope", "expected_revision": r, "optics": {}}))),
        ("render.optics.bind", Box::new(|r| json!({"name": "Raster", "body_index": 0, "body_name": "Lens", "expected_revision": r, "optics": {}}))),
        (
            "render.optics.bind",
            Box::new(|r| json!({"name": "Studio", "set": 5, "body_index": 0, "body_name": "Lens", "expected_revision": r, "optics": {}})),
        ),
        (
            "render.optics.unbind",
            Box::new(
                |r| json!({"name": "Nope", "target": {"definition": "d", "feature": 1, "component": 0, "occurrence_path": []}, "expected_revision": r}),
            ),
        ),
        (
            "render.optics.unbind",
            Box::new(
                |_| json!({"name": "Studio", "target": {"definition": "d", "feature": 1, "component": 0, "occurrence_path": []}, "expected_revision": 999_999}),
            ),
        ),
        ("render.optics.unbind", Box::new(move |r| json!({"name": "Studio", "target": stranger, "expected_revision": r}))),
        (
            "render.library.surface.unbind",
            Box::new(|_| json!({"name": "Studio", "target": {"definition": "d", "feature": 0, "component": 0, "occurrence_path": []}})),
        ),
        (
            "render.library.surface.unbind",
            Box::new(|_| json!({"name": " Studio", "target": {"definition": "d", "feature": 1, "component": 0, "occurrence_path": []}})),
        ),
        (
            "render.library.surface.unbind",
            Box::new(|_| json!({"name": "Studio", "set": 5, "target": {"definition": "d", "feature": 1, "component": 0, "occurrence_path": []}})),
        ),
        // Without the bundled render library, both refuse before reading anything else.
        ("render.library.surface.bind", Box::new(|_| json!({"name": "Studio", "surface": {"id": "MAT-001", "body_index": 0, "body_name": "Lens"}}))),
        ("render.library.surface.bind", Box::new(|_| json!({"name": "Raster", "surface": {"id": "SURF-001", "body_index": 0, "body_name": "Lens"}}))),
    ];
    for (id, p) in &cases {
        let error = |level: ParamCheck| {
            let mut s = with_a_studio(level);
            let p = p(s.revision);
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {} should fail today", p(0));
        assert_eq!(error(ParamCheck::Strict), off, "{id} {}", p(0));
    }
}

/// #11741: binding and unbinding reply as their output schemas say, in Warn without warnings.
/// With the bundled render library, a surface binding is saved and removed as well. Like the
/// handlers, warn and strict refuse a misspelt key.
#[test]
fn binding_commands_reply_with_their_schemas() {
    let (optics_saved, unbound, surface_saved) =
        (output_schema_for::<OpticsBindingSaved>(), output_schema_for::<OpticsUnbound>(), output_schema_for::<SurfaceBindingSaved>());
    let mut s = with_a_studio(ParamCheck::Warn);
    let run = |s: &mut Session, schema: &schemars::Schema, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let revision = s.revision;
    let r = run(&mut s, &optics_saved, "render.optics.bind", optics_on_the_lens(revision));
    assert!(r["setup"]["optics"].is_array(), "{r}");
    let target = lens_target(&s);
    let p = json!({"name": "Studio", "target": target, "expected_revision": s.revision});
    assert_eq!(run(&mut s, &unbound, "render.optics.unbind", p)["removed"], true);
    let p = json!({"name": "Studio", "target": target, "expected_revision": s.revision});
    assert_eq!(run(&mut s, &unbound, "render.optics.unbind", p)["removed"], false, "nothing left to remove");
    // Removing a surface binding the body doesn't have saves the setup as it is.
    run(&mut s, &surface_saved, "render.library.surface.unbind", json!({"name": "Studio", "target": target, "set": {}}));
    if cfg!(feature = "bundled-render-library") {
        let surface = json!({"id": "SURF-001", "body_index": 0, "body_name": "Lens", "plane": "xy", "scale": 2, "origin": [0, 0, 0]});
        let r = run(&mut s, &surface_saved, "render.library.surface.bind", json!({"name": "Studio", "surface": surface, "replace": false}));
        assert_eq!(r["mapping_space"], "definition_local_mm", "{r}");
        run(&mut s, &surface_saved, "render.library.surface.unbind", json!({"name": "Studio", "target": target}));
    }
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = with_a_studio(level);
        let mut p = optics_on_the_lens(s.revision);
        // Rename the key, so the call has a misspelt `optic` and no `optics` at all.
        if let Some(keys) = p.as_object_mut()
            && let Some(optics) = keys.remove("optics")
        {
            keys.insert("optic".into(), optics);
        }
        let error = s.execute("render.optics.bind", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("`optic`"), "{level:?}: {error}");
    }
}

/// `set` names the render setup's fields: `patched_settings` reads the patched setup as a
/// `RenderSetup`, which refuses any other key, so the schema refuses a misspelt one too (and
/// more than 13 at once).
#[test]
fn the_set_schema_names_the_setup_fields() {
    let root = input_schema_for::<SurfaceBindParams>();
    let root = root.as_value();
    let set = &root["properties"]["set"];
    for (patch, takes) in
        [(json!({}), true), (json!({"width": 640, "height": 480, "job": {"method": "gpu_path"}}), true), (json!({"widht": 640}), false)]
    {
        assert_eq!(conforms(root, set, &patch), takes, "{patch}");
    }
    // `conforms` doesn't read `maxProperties`, so the limit is pinned here.
    assert_eq!((set["maxProperties"].as_u64(), set["properties"].as_object().map(|p| p.len())), (Some(13), Some(super::SETUP_FIELDS.len())), "{set}");
}
