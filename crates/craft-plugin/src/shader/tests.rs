use super::*;
use serde_json::json;

fn background(textures: &[&str]) -> Shader {
    Shader {
        id: "dusk".into(),
        name: "Dusk".into(),
        kind: ShaderKind::Background,
        source: "dusk.wgsl".into(),
        textures: textures.iter().map(|t| ShaderTexture { name: (*t).into(), file: format!("{t}.png") }).collect(),
        animated: false,
        state: None,
    }
}

/// An animated background with a `width` by `height` state grid.
fn stateful(width: u32, height: u32) -> Shader {
    Shader { animated: true, state: Some(ShaderState { width, height }), ..background(&[]) }
}

/// The state example in docs/plugin-examples/trails.
const TRAILS: &str = include_str!("../../../../docs/plugin-examples/trails/trails.wgsl");

/// A state's `fn state` must be there with the grid, and a grid needs no state of its own
/// otherwise; both sides of the manifest are checked.
#[test]
fn a_state_grid_needs_fn_state_and_fn_state_needs_a_grid() {
    let shader = stateful(64, 64);
    assert!(reason(compile(&shader, GRADIENT).unwrap_err()).contains("fn state"));
    compile(&shader, TRAILS).unwrap();
    let plain = reason(compile(&background(&[]), TRAILS).unwrap_err());
    assert!(plain.contains("no state grid"), "{plain}");
}

#[test]
fn a_state_grid_is_animated_and_sized_within_1024() {
    validate(&[stateful(64, 64)]).unwrap();
    validate(&[stateful(1024, 1)]).unwrap();
    for (width, height) in [(0, 64), (64, 0), (1025, 64), (64, 1025)] {
        assert!(validate(&[stateful(width, height)]).is_err(), "{width}x{height}");
    }
    let still = Shader { animated: false, ..stateful(64, 64) };
    assert!(validate(&[still]).is_err(), "a state needs animated");
}

#[test]
fn the_state_grid_wires_its_two_bindings_and_its_entry_point() {
    let compiled = compile(&stateful(64, 64), TRAILS).unwrap();
    assert!(compiled.wgsl.contains("fn sc_state_fragment"), "{}", compiled.wgsl);
    assert!(compiled.wgsl.contains("@binding(7)") && compiled.wgsl.contains("@binding(8)"), "{}", compiled.wgsl);
    naga::front::wgsl::parse_str(&compiled.wgsl).unwrap();
    // Without a grid the host declares none of it.
    let plain = compile(&background(&[]), GRADIENT).unwrap();
    assert!(!plain.wgsl.contains("sc_state"), "{}", plain.wgsl);
}

/// With a grid the `sc_state*` names are the host's, so a shader cannot declare them itself.
#[test]
fn sc_state_names_are_reserved() {
    assert!(compile(&stateful(64, 64), &format!("var<private> sc_state_prev: f32 = 1.0;\n{TRAILS}")).is_err());
    let function = format!("fn sc_state_help() -> f32 {{ return 1.0; }}\n{TRAILS}");
    assert!(compile(&stateful(64, 64), &function).is_err());
    let binding = format!("@group(0) @binding(9) var sc_state_other: texture_2d<f32>;\n{TRAILS}");
    assert!(compile(&stateful(64, 64), &binding).is_err());
}

/// The state step is budgeted like the background: its loops and calls spend the same budget.
#[test]
fn the_state_step_spends_the_loop_budget_too() {
    let plain = compile(&stateful(64, 64), TRAILS).unwrap();
    let looped = TRAILS.replace("    let fade", "    loop { }\n    let fade");
    let compiled = compile(&stateful(64, 64), &looped).unwrap();
    assert!(compiled.wgsl.matches("sc_loop_budget").count() > plain.wgsl.matches("sc_loop_budget").count(), "the loop in fn state spends the budget");
}

/// A state entry point of the plugin's own is refused, and so is a second fn state.
#[test]
fn a_state_shader_brings_no_entry_points_of_its_own() {
    let entry = format!("{TRAILS}@fragment fn sc_state_mine() -> @location(0) vec4<f32> {{ return vec4<f32>(1.0); }}\n");
    assert!(reason(compile(&stateful(64, 64), &entry).unwrap_err()).contains("entry point"));
    assert!(compile(&stateful(64, 64), &format!("{TRAILS}fn state(px: ScStatePixel) -> vec4<f32> {{ return vec4<f32>(0.0); }}\n")).is_err());
}

const GRADIENT: &str = "fn background(px: ScPixel) -> vec4<f32> {\n    return mix(sc.bottom, sc.top, 0.5 + 0.5 * px.ray.z);\n}\n";

fn reason(error: HostError) -> String {
    match error {
        HostError::InvalidManifest { reason, .. } => reason,
        other => panic!("not a manifest refusal: {other:?}"),
    }
}

#[test]
fn a_plain_background_compiles_into_one_module_with_the_host_entry_points() {
    let compiled = compile(&background(&[]), GRADIENT).unwrap();
    assert!(compiled.wgsl.contains("fn sc_vertex") && compiled.wgsl.contains("fn sc_fragment"), "{}", compiled.wgsl);
    assert!(compiled.wgsl.contains("fn background"));
    // What the host hands the GPU parses again: the writer's output is valid WGSL.
    naga::front::wgsl::parse_str(&compiled.wgsl).unwrap();
}

#[test]
fn declared_textures_are_bound_after_the_samplers_and_sampled_by_name() {
    let source = "fn background(px: ScPixel) -> vec4<f32> {\n    let a = textureSample(sky, sc_repeat, px.uv);\n    let b = textureSample(stars, sc_clamp, px.uv);\n    return a + b;\n}\n";
    let compiled = compile(&background(&["sky", "stars"]), source).unwrap();
    assert!(compiled.wgsl.contains("@binding(3)") && compiled.wgsl.contains("@binding(4)"), "{}", compiled.wgsl);
    let undeclared = reason(compile(&background(&["sky"]), source).unwrap_err());
    assert!(undeclared.contains("line 3"), "{undeclared}");
}

#[test]
fn an_error_names_the_plugin_file_and_its_own_line() {
    let broken = "fn background(px: ScPixel) -> vec4<f32> {\n    let x = 1.0;\n    return vec4<f32>(x, x, x);\n}\n";
    let error = reason(compile(&background(&[]), broken).unwrap_err());
    assert!(error.starts_with("dusk.wgsl: line 3"), "{error}");
}

#[test]
fn a_shader_brings_no_entry_points_bindings_or_overrides_of_its_own() {
    let entry = format!("{GRADIENT}@fragment fn mine() -> @location(0) vec4<f32> {{ return vec4<f32>(1.0); }}\n");
    assert!(reason(compile(&background(&[]), &entry).unwrap_err()).contains("entry point"));
    let storage = format!("{GRADIENT}@group(1) @binding(0) var<storage, read_write> leak: array<u32>;\n");
    assert!(reason(compile(&background(&[]), &storage).unwrap_err()).contains("resource `leak`"));
    let uniform = format!("{GRADIENT}@group(0) @binding(9) var<uniform> other: vec4<f32>;\n");
    assert!(reason(compile(&background(&[]), &uniform).unwrap_err()).contains("resource `other`"));
    let over = format!("override strength: f32 = 1.0;\n{GRADIENT}");
    assert!(reason(compile(&background(&[]), &over).unwrap_err()).contains("override"));
    compile(&background(&[]), &format!("var<private> seed: u32 = 7u;\n{GRADIENT}")).unwrap();
}

#[test]
fn a_shader_without_background_or_over_the_size_limit_is_refused() {
    let none = "fn other(px: ScPixel) -> vec4<f32> { return sc.top; }\n";
    assert!(reason(compile(&background(&[]), none).unwrap_err()).contains("fn background"));
    let big = format!("{GRADIENT}//{}\n", "x".repeat(MAX_SOURCE_BYTES));
    assert!(reason(compile(&background(&[]), &big).unwrap_err()).contains("larger than 64 KiB"));
}

#[test]
fn the_host_names_cannot_be_redefined() {
    let clash = format!("struct ScHost {{ x: f32 }}\n{GRADIENT}");
    assert!(compile(&background(&[]), &clash).is_err());
    let sampler = format!("var<private> sc_clamp: f32 = 1.0;\n{GRADIENT}");
    assert!(compile(&background(&[]), &sampler).is_err());
}

/// Every loop, `for`, `while` and `loop`, nested or in a called function, starts by checking and
/// spending the one per-pixel budget, so an endless loop ends.
#[test]
fn every_loop_spends_one_shared_budget() {
    let source = "fn spin(n: f32) -> f32 {\n    var t = n;\n    loop { t = t + 1.0; }\n    return t;\n}\nfn background(px: ScPixel) -> vec4<f32> {\n    var s = 0.0;\n    for (var i = 0; i < 100000; i++) {\n        var j = 0;\n        while (j >= 0) { s += 0.001; j++; }\n    }\n    return vec4<f32>(spin(s));\n}\n";
    let compiled = compile(&background(&[]), source).unwrap();
    // Three loops, and the plugin's two functions (spin and background).
    assert_eq!(compiled.wgsl.matches("sc_loop_budget = (").count(), 3 + 2, "{}", compiled.wgsl);
    assert!(compiled.wgsl.contains(&format!("sc_loop_budget: u32 = {LOOP_BUDGET}u")), "{}", compiled.wgsl);
}

/// Sampling with derivatives (textureSample) in a loop is refused by uniformity analysis once the
/// loop can end early; textureSampleLevel works there.
#[test]
fn a_texture_sampled_in_a_loop_uses_an_explicit_level() {
    let source = "fn background(px: ScPixel) -> vec4<f32> {\n    var c = vec4<f32>(0.0);\n    for (var i = 0; i < 4; i++) {\n        c += textureSampleLevel(sky, sc_clamp, px.uv + vec2<f32>(f32(i) * 0.01, 0.0), 0.0);\n    }\n    return c * 0.25;\n}\n";
    compile(&background(&["sky"]), source).unwrap();
}

#[test]
fn the_manifest_list_checks_ids_files_and_texture_names() {
    validate(&[background(&["sky"])]).unwrap();
    let mut twice = vec![background(&[]), background(&[])];
    assert!(validate(&twice).is_err());
    twice[1].id = "night".into();
    validate(&twice).unwrap();
    for (source, ok) in [("dusk.wgsl", true), ("../dusk.wgsl", false), ("dir/dusk.wgsl", false), ("Dusk.wgsl", false), ("dusk.glsl", false)] {
        let mut shader = background(&[]);
        shader.source = source.into();
        assert_eq!(validate(&[shader]).is_ok(), ok, "{source}");
    }
    for (name, ok) in [("sky", true), ("city_2", true), ("sc_sky", false), ("sc", false), ("2sky", false), ("Sky", false), ("background", false)] {
        assert_eq!(validate(&[background(&[name])]).is_ok(), ok, "{name}");
    }
    let mut picture = background(&["sky"]);
    picture.textures[0].file = "sky.exe".into();
    assert!(validate(&[picture]).is_err());
    assert!(validate(&[background(&["a", "b", "c", "d", "e"])]).is_err());
    let many: Vec<Shader> = (0..=MAX_SHADERS)
        .map(|i| {
            let mut s = background(&[]);
            s.id = format!("s{i}");
            s
        })
        .collect();
    assert!(validate(&many).is_err());
}

#[test]
fn the_host_uniform_size_matches_the_prelude() {
    let module = naga::front::wgsl::parse_str(&format!("{GRADIENT}\n{}", PRELUDE.replace("{textures}", "").replace("{state}", ""))).unwrap();
    let (_, host) = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("ScHost")).unwrap();
    let naga::TypeInner::Struct { span, .. } = host.inner else { panic!("ScHost is a struct") };
    assert_eq!(span as usize, HOST_UNIFORM_BYTES);
}

fn manifest(value: serde_json::Value) -> crate::Result<crate::Manifest> {
    crate::parse_manifest(value.to_string().as_bytes(), "sky")
}

#[test]
fn a_manifest_with_shaders_needs_the_capability_and_may_have_no_runtime() {
    let shaders = json!([{"id": "dusk", "name": "Dusk", "kind": "background", "source": "dusk.wgsl", "animated": true}]);
    let plugin = manifest(json!({"id": "sky", "name": "Sky", "version": "1.0.0", "capabilities": ["shaders@0.1"], "shaders": shaders})).unwrap();
    assert!(plugin.runtime.is_none());
    assert!(plugin.shaders[0].animated);
    let missing = manifest(json!({"id": "sky", "name": "Sky", "version": "1.0.0", "shaders": shaders})).unwrap_err();
    assert!(missing.to_string().contains("shaders@0.1"), "{missing}");
    let newer = manifest(json!({"id": "sky", "name": "Sky", "version": "1.0.0", "capabilities": ["shaders@0.2"], "shaders": shaders})).unwrap_err();
    assert!(matches!(newer, HostError::UnsupportedCapability { .. }), "{newer:?}");
    let unknown_kind = json!([{"id": "dusk", "name": "Dusk", "kind": "panel", "source": "dusk.wgsl"}]);
    assert!(manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.1"], "shaders": unknown_kind})).is_err());
}

/// A 0.41 manifest (commands@0.1 alone) reads and serializes as before.
#[test]
fn a_commands_only_manifest_is_unchanged() {
    let text = json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["commands@0.1"], "runtime": {
        "module": "sky.wasm", "entries": ["make"], "commands": ["solid.box"]
    }});
    let plugin = manifest(text).unwrap();
    assert!(plugin.shaders.is_empty());
    assert!(serde_json::to_value(&plugin).unwrap().get("shaders").is_none());
}

#[test]
fn the_documented_examples_compile() {
    for (folder, manifest_text, source) in [
        ("dusk", include_str!("../../../../docs/plugin-examples/dusk/plugin.json"), include_str!("../../../../docs/plugin-examples/dusk/dusk.wgsl")),
        (
            "panorama",
            include_str!("../../../../docs/plugin-examples/panorama/plugin.json"),
            include_str!("../../../../docs/plugin-examples/panorama/panorama.wgsl"),
        ),
        (
            "trails",
            include_str!("../../../../docs/plugin-examples/trails/plugin.json"),
            include_str!("../../../../docs/plugin-examples/trails/trails.wgsl"),
        ),
        (
            "galaxy",
            include_str!("../../../../docs/plugin-examples/galaxy/plugin.json"),
            include_str!("../../../../docs/plugin-examples/galaxy/galaxy.wgsl"),
        ),
        ("rain", include_str!("../../../../docs/plugin-examples/rain/plugin.json"), include_str!("../../../../docs/plugin-examples/rain/rain.wgsl")),
    ] {
        let manifest = crate::parse_manifest(manifest_text.as_bytes(), folder).unwrap();
        compile(&manifest.shaders[0], source).unwrap_or_else(|e| panic!("{folder}: {e}"));
    }
}

/// 2^20 calls and no loop: every plugin function spends one, and returns zero once it is gone.
#[test]
fn calls_that_fan_out_spend_the_budget_too() {
    let mut source = String::from("fn f0(x: f32) -> f32 { return x + 1.0; }\n");
    for i in 1..20 {
        source.push_str(&format!("fn f{i}(x: f32) -> f32 {{ return f{}(x) + f{}(x); }}\n", i - 1, i - 1));
    }
    source.push_str("fn background(px: ScPixel) -> vec4<f32> { return vec4<f32>(f19(px.uv.x)); }\n");
    let compiled = compile(&background(&[]), &source).unwrap();
    // 20 chain functions and background.
    assert_eq!(compiled.wgsl.matches("sc_loop_budget = (").count(), 21, "{}", compiled.wgsl);
    assert!(compiled.wgsl.contains("return f32();"), "a spent call returns zero: {}", compiled.wgsl);
}

/// Alpha below 1 is premultiplied on the screen's (sRGB) values, as egui blends.
#[test]
fn alpha_is_premultiplied_in_gamma_space() {
    let compiled = compile(&background(&[]), GRADIENT).unwrap();
    assert!(compiled.wgsl.contains("= sc_encode(colour.xyz);") && !compiled.wgsl.contains("sc_decode"), "{}", compiled.wgsl);
}

/// A pixel that used up the budget still gets encoded, so a capped loop ends instead of drawing black.
#[test]
fn host_functions_do_not_spend_the_budget() {
    let source = "fn background(px: ScPixel) -> vec4<f32> {\n    loop { }\n    return vec4<f32>(1.0);\n}\n";
    let compiled = compile(&background(&[]), source).unwrap();
    let start = compiled.wgsl.find("fn sc_encode(").expect("sc_encode in the output");
    let body = &compiled.wgsl[start..];
    let body = &body[..body.find("\n}\n").expect("end of sc_encode")];
    assert!(!body.contains("sc_loop_budget"), "sc_encode spends the budget: {body}");
}

/// So the fan-out above cannot dodge the call budget by taking a host-looking name.
#[test]
fn plugin_functions_named_like_the_host_are_refused() {
    let mut source = String::from("fn sc_f0(x: f32) -> f32 { return x + 1.0; }\n");
    for i in 1..20 {
        source.push_str(&format!("fn sc_f{i}(x: f32) -> f32 {{ return sc_f{}(x) + sc_f{}(x); }}\n", i - 1, i - 1));
    }
    source.push_str("fn background(px: ScPixel) -> vec4<f32> { return vec4<f32>(sc_f19(px.uv.x)); }\n");
    let error = reason(compile(&background(&[]), &source).unwrap_err());
    assert!(error.contains("`sc_f0`") && error.contains("sc_"), "{error}");
}

/// WGSL allows comments between tokens: `background` is found by the parser, not by the text.
#[test]
fn background_is_found_with_comments_between_its_tokens() {
    let source = "fn /* sky */ background(px: ScPixel) -> vec4<f32> { return sc.top; }
";
    assert!(compile(&background(&[]), source).is_ok());
    let commented_out = "// fn background(px: ScPixel) -> vec4<f32> { return sc.top; }
fn sky() {}
";
    assert!(reason(compile(&background(&[]), commented_out).unwrap_err()).contains("fn background"));
}

/// A plugin written before `state` existed: no `state` key, so it parses, validates, serialises
/// without one, and compiles to a module that declares none of the state bindings.
#[test]
fn a_plugin_without_a_state_grid_is_unchanged() {
    let manifest_text = include_str!("../../../../docs/plugin-examples/dusk/plugin.json");
    let manifest = crate::parse_manifest(manifest_text.as_bytes(), "dusk").unwrap();
    let shader = &manifest.shaders[0];
    assert_eq!(shader.state, None);
    assert!(shader.animated);
    assert!(!serde_json::to_string(shader).unwrap().contains("state"));
    validate(&manifest.shaders).unwrap();
    let compiled = compile(shader, include_str!("../../../../docs/plugin-examples/dusk/dusk.wgsl")).unwrap();
    assert!(!compiled.wgsl.contains("sc_state"), "{}", compiled.wgsl);
    assert!(!compiled.wgsl.contains("@binding(7)") && !compiled.wgsl.contains("@binding(8)"), "{}", compiled.wgsl);
    naga::front::wgsl::parse_str(&compiled.wgsl).unwrap();
}

/// Without a grid the state names are the plugin's own, as before `state` existed: a `fn state` helper
/// and private `sc_` globals still compile.
#[test]
fn a_shader_without_a_grid_may_use_state_and_sc_names_of_its_own() {
    let source = format!(
        "var<private> sc_x: f32 = 1.0;\nvar<private> sc_state: f32 = 2.0;\nfn state(v: f32) -> f32 {{ return v * sc_x * sc_state; }}\n{GRADIENT}"
    );
    compile(&background(&[]), &source).unwrap();
}
