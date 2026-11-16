use serde_json::{Value, json};

use super::{HdrCaptureParams, HdrCaptured, HdrInspectParams, HdrInspected};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A small HDR file in the temporary folder, removed when dropped: 4 × 2 pixels with declared
/// sRGB/D65 primaries, which both renderers accept. `bytes` replaces its content.
struct HdrFile(String);

impl HdrFile {
    fn new(name: &str, bytes: Option<&[u8]>) -> Self {
        let path = std::env::temp_dir().join(format!("solidcraft-hdr-schema-{}-{name}.hdr", std::process::id()));
        let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        hdr.extend([128, 64, 32, 129].repeat(8));
        std::fs::write(&path, bytes.unwrap_or(&hdr)).unwrap_or_else(|e| panic!("{e}"));
        HdrFile(path.to_string_lossy().into_owned())
    }
}

impl Drop for HdrFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A design with two saved render setups, `Gpu` (the GPU path tracer) and `Cpu` (the CPU
/// raytracer), at the parameter-check level `level`.
fn with_setups(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (name, job) in [("Gpu", json!({"method": "gpu_path"})), ("Cpu", json!({"method": "cpu_raytrace", "options": {"budget": "studio"}}))] {
        let p = json!({"name": name, "set": {"job": job}});
        s.execute("render.settings.set", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    }
    s
}

/// The `set` of a capture: the saved setup `name` as `render.settings.set` gives its job.
fn job_of(name: &str) -> Value {
    if name == "Gpu" { json!({"job": {"method": "gpu_path"}}) } else { json!({"job": {"method": "cpu_raytrace", "options": {"budget": "studio"}}}) }
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<HdrInspectParams>(), &["path"]);
    assert_keys(
        input_schema_for::<HdrCaptureParams>(),
        &["assume_linear_srgb", "expected_sha256", "intensity", "name", "path", "rotation_rad", "samples", "set"],
    );
}

/// The handlers refuse these calls after reading them; strict refuses them with their message.
/// They need an HDR file and saved setups, so the shared helper doesn't fit.
#[test]
fn strict_keeps_todays_errors() {
    let hdr = HdrFile::new("strict", None);
    let not_hdr = HdrFile::new("strict-not-hdr", Some(b"not an HDR file"));
    let missing = hdr.0.replace("strict", "strict-missing");
    let capture = |name: &str, extra: Value| {
        let mut p = json!({"name": name, "set": job_of(name), "path": hdr.0});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        p
    };
    let cases = [
        ("render.hdr.inspect", json!({"path": ""})),
        ("render.hdr.inspect", json!({"path": missing})),
        ("render.hdr.inspect", json!({"path": not_hdr.0})),
        ("render.settings.capture_hdr", json!({"name": "Raster", "set": {}, "path": hdr.0})),
        ("render.settings.capture_hdr", json!({"name": "Gpu", "set": 5, "path": hdr.0})),
        ("render.settings.capture_hdr", json!({"name": " Gpu", "set": {}, "path": hdr.0})),
        ("render.settings.capture_hdr", capture("Gpu", json!({"samples": 16}))),
        ("render.settings.capture_hdr", capture("Gpu", json!({"intensity": 200}))),
        ("render.settings.capture_hdr", capture("Gpu", json!({"rotation_rad": 2e6}))),
        ("render.settings.capture_hdr", capture("Gpu", json!({"expected_sha256": "0".repeat(64)}))),
        // Only the key's presence matters to the handler: `null` is refused like `false`.
        ("render.settings.capture_hdr", capture("Cpu", json!({"assume_linear_srgb": null}))),
        ("render.settings.capture_hdr", capture("Cpu", json!({"samples": 5}))),
        ("render.settings.capture_hdr", capture("Cpu", json!({"intensity": 65}))),
    ];
    for (id, p) in &cases {
        let error = |level: ParamCheck| with_setups(level).execute(id, p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// #11761: inspecting an HDR file and capturing it into a GPU and a CPU setup reply as the
/// output schemas say, in Warn without warnings. Like the handlers, warn and strict refuse a
/// misspelt key.
#[test]
fn hdr_commands_reply_with_their_schemas() {
    let (inspected, captured) = (output_schema_for::<HdrInspected>(), output_schema_for::<HdrCaptured>());
    let hdr = HdrFile::new("replies", None);
    let mut s = with_setups(ParamCheck::Warn);
    let run = |s: &mut Session, schema: &schemars::Schema, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let info = run(&mut s, &inspected, "render.hdr.inspect", json!({"path": hdr.0}));
    assert_eq!((info["width"].as_u64(), info["height"].as_u64()), (Some(4), Some(2)), "{info}");
    let gpu = json!({"name": "Gpu", "set": job_of("Gpu"), "path": hdr.0, "expected_sha256": info["sha256"],
        "rotation_rad": -0.25, "intensity": 2.5, "assume_linear_srgb": false});
    let r = run(&mut s, &captured, "render.settings.capture_hdr", gpu);
    assert_eq!(r["resource"]["sha256"], info["sha256"], "{r}");
    let cpu = json!({"name": "Cpu", "set": job_of("Cpu"), "path": hdr.0, "samples": 4, "expected_sha256": null});
    run(&mut s, &captured, "render.settings.capture_hdr", cpu);
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let p = json!({"path": hdr.0, "pth": hdr.0});
        let error = with_setups(level).execute("render.hdr.inspect", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("`pth`"), "{level:?}: {error}");
    }
}

/// `set` names the render setup's fields: `patched_settings` reads the patched setup as a
/// `RenderSetup`, which refuses any other key, so the schema refuses a misspelt one too (and
/// more than `SETUP_FIELDS` at once). `coats` is one of them (#12783).
#[test]
fn the_set_schema_names_the_setup_fields() {
    let root = input_schema_for::<HdrCaptureParams>();
    let root = root.as_value();
    let set = &root["properties"]["set"];
    for (patch, takes) in [
        (json!({}), true),
        (json!({"width": 640, "height": 480, "job": {"method": "gpu_path"}}), true),
        (json!({"coats": {}}), true),
        (json!({"widht": 640}), false),
    ] {
        assert_eq!(conforms(root, set, &patch), takes, "{patch}");
    }
    // `conforms` doesn't read `maxProperties`, so the limit is pinned here.
    assert_eq!(
        (set["maxProperties"].as_u64(), set["properties"].as_object().map(|p| p.len())),
        (Some(crate::schema::render_settings::SETUP_FIELDS.len() as u64), Some(crate::schema::render_settings::SETUP_FIELDS.len())),
        "{set}"
    );
}
