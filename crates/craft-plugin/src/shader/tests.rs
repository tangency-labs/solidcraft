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
        settings: Vec::new(),
        blur: None,
    }
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
    let module = naga::front::wgsl::parse_str(&format!("{GRADIENT}\n{}", PRELUDE.replace("{textures}", ""))).unwrap();
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
    let newer = manifest(json!({"id": "sky", "name": "Sky", "version": "1.0.0", "capabilities": ["shaders@0.3"], "shaders": shaders})).unwrap_err();
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

fn tint_shader() -> Shader {
    Shader { kind: ShaderKind::Tint, id: "amber".into(), source: "amber.wgsl".into(), ..background(&[]) }
}

const AMBER: &str =
    "fn tint(r: ScRegion) -> vec4<f32> {\n    if r.kind == SC_ICON { return vec4<f32>(1.0, 0.5, 0.1, r.enabled); }\n    return vec4<f32>(0.0);\n}\n";

#[test]
fn a_tint_compiles_into_one_compute_module_and_needs_its_function() {
    let compiled = compile(&tint_shader(), AMBER).unwrap();
    assert!(compiled.wgsl.contains("fn sc_tint_main") && compiled.wgsl.contains("@compute"), "{}", compiled.wgsl);
    assert!(!compiled.wgsl.contains("fn sc_fragment"), "a tint has no fragment stage");
    naga::front::wgsl::parse_str(&compiled.wgsl).unwrap();
    let none = reason(compile(&tint_shader(), GRADIENT).unwrap_err());
    assert!(none.contains("fn tint(r: ScRegion)"), "{none}");
    assert!(compile(&background(&[]), AMBER).is_err(), "a background has no ScRegion and needs its own function");
}

#[test]
fn a_tint_spends_the_loop_budget_and_reads_only_what_the_host_binds() {
    let spin = "fn tint(r: ScRegion) -> vec4<f32> {\n    var x = 0.0;\n    loop { x += r.rect.x; }\n    return vec4<f32>(x);\n}\n";
    let compiled = compile(&tint_shader(), spin).unwrap();
    assert_eq!(compiled.wgsl.matches("sc_loop_budget = (").count(), 1 + 1, "{}", compiled.wgsl);
    let leak = format!("{AMBER}@group(2) @binding(0) var<storage, read_write> leak: array<u32>;\n");
    assert!(reason(compile(&tint_shader(), &leak).unwrap_err()).contains("resource `leak`"));
    let entry = format!("{AMBER}@compute @workgroup_size(1) fn mine() {{}}\n");
    assert!(reason(compile(&tint_shader(), &entry).unwrap_err()).contains("entry point"));
    // A background has no region list to read.
    let reads = "fn background(px: ScPixel) -> vec4<f32> { return sc_regions[0].base; }\n";
    assert!(compile(&background(&[]), reads).is_err());
}

#[test]
fn a_tint_cannot_touch_the_region_list_or_other_answers() {
    let write = "fn tint(r: ScRegion) -> vec4<f32> {
    sc_tints[0] = vec4<f32>(1.0);
    return vec4<f32>(0.0);
}
";
    let error = reason(compile(&tint_shader(), write).unwrap_err());
    assert!(error.contains("line 2") && error.contains("`sc_tints`"), "{error}");
    let read = "fn other() -> vec4<f32> { return sc_regions[3].base; }
fn tint(r: ScRegion) -> vec4<f32> { return other(); }
";
    let error = reason(compile(&tint_shader(), read).unwrap_err());
    assert!(error.contains("line 1") && error.contains("`sc_regions`"), "{error}");
}

#[test]
fn the_tint_region_is_the_size_the_host_writes() {
    let module = naga::front::wgsl::parse_str(&format!("{AMBER}\n{}", TINT_PRELUDE.replace("{textures}", ""))).unwrap();
    let (_, region) = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("ScRegion")).unwrap();
    let naga::TypeInner::Struct { span, .. } = region.inner else { panic!("ScRegion is a struct") };
    assert_eq!(span as usize, TINT_REGION_BYTES);
}

#[test]
fn a_tint_or_settings_need_shaders_0_2_and_0_1_plugins_still_install() {
    let shaders = json!([{"id": "amber", "name": "Amber", "kind": "tint", "source": "amber.wgsl"}]);
    let plugin = manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.2"], "shaders": shaders})).unwrap();
    assert_eq!(plugin.shaders[0].kind, ShaderKind::Tint);
    let old = manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.1"], "shaders": shaders})).unwrap_err();
    assert!(old.to_string().contains("shaders@0.2"), "{old}");
    let dusk = json!([{"id": "dusk", "name": "Dusk", "kind": "background", "source": "dusk.wgsl"}]);
    manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.1"], "shaders": dusk})).unwrap();
    manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.2"], "shaders": dusk})).unwrap();
    let slider = json!([{"id": "dusk", "name": "Dusk", "kind": "background", "source": "dusk.wgsl",
        "settings": [{"id": "glow", "name": "Glow", "min": 0, "max": 1, "default": 0.5}]}]);
    assert!(manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.1"], "shaders": slider})).is_err());
    manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.2"], "shaders": slider})).unwrap();
}

#[test]
fn settings_are_checked_and_read_within_their_range() {
    let setting = |id: &str, min: f32, max: f32, default: f32| ShaderSetting { id: id.into(), name: id.into(), min, max, default };
    let mut shader = background(&[]);
    shader.settings = vec![setting("length", 300.0, 8000.0, 2400.0), setting("speed", 10.0, 1500.0, 150.0)];
    validate(std::slice::from_ref(&shader)).unwrap();
    let values = shader.setting_values(|id| match id {
        "length" => Some(99999.0),
        _ => None,
    });
    assert_eq!(values, [8000.0, 150.0, 0.0, 0.0], "clamped, defaulted, zero past the last");
    assert_eq!(shader.setting_values(|_| Some(f32::NAN))[0], 2400.0);
    for bad in [setting("a", 1.0, 1.0, 1.0), setting("a", 0.0, 1.0, 2.0), setting("A", 0.0, 1.0, 0.5), setting("a", f32::NAN, 1.0, 0.5)] {
        let mut s = background(&[]);
        s.settings = vec![bad.clone()];
        assert!(validate(&[s]).is_err(), "{bad:?}");
    }
    let mut twice = background(&[]);
    twice.settings = vec![setting("a", 0.0, 1.0, 0.5), setting("a", 0.0, 1.0, 0.5)];
    assert!(validate(&[twice]).is_err());
    let mut many = background(&[]);
    many.settings = (0..5).map(|i| setting(&format!("s{i}"), 0.0, 1.0, 0.5)).collect();
    assert!(validate(&[many]).is_err());
}

fn panel(textures: &[&str]) -> Shader {
    Shader { id: "frost".into(), name: "Frost".into(), kind: ShaderKind::Panel, source: "frost.wgsl".into(), ..background(textures) }
}

const FROST: &str = "fn panel(px: ScPanel) -> vec4<f32> {\n    let rim = smoothstep(-1.5, 0.0, px.edge);\n    let c = mix(sc_blurred(px.position).rgb, vec3<f32>(1.0), 0.2 * sc.settings.x + 0.3 * rim);\n    return vec4<f32>(mix(c, sc_backdrop(px.position).rgb, px.hovered * 0.1), 1.0);\n}\n";

#[test]
fn a_panel_shader_compiles_with_the_panel_entry_points_and_binds_what_the_host_lays_out() {
    let source = FROST.replace("1.0);\n}", "1.0) + 0.0 * textureSampleLevel(grain, sc_repeat, px.uv, 0.0);\n}");
    let compiled = compile(&panel(&["grain"]), &source).unwrap();
    assert!(compiled.wgsl.contains("fn sc_fragment") && compiled.wgsl.contains("fn panel"), "{}", compiled.wgsl);
    naga::front::wgsl::parse_str(&compiled.wgsl).unwrap();
    assert_eq!(
        compiled.bindings,
        vec![
            (0, Resource::Uniform(HOST_UNIFORM_BYTES as u64)),
            (1, Resource::Sampler),
            (2, Resource::Sampler),
            (FIRST_TEXTURE_BINDING, Resource::Texture),
            (PANEL_RECT_BINDING, Resource::Uniform(PANEL_RECT_UNIFORM_BYTES as u64)),
            (PANEL_RECT_BINDING + 1, Resource::Texture),
            (PANEL_RECT_BINDING + 2, Resource::Texture),
        ]
    );
}

#[test]
fn a_panel_shader_needs_panel_and_cannot_take_the_hosts_names() {
    let none = "fn background(px: ScPixel) -> vec4<f32> { return vec4<f32>(1.0); }\n";
    assert!(reason(compile(&panel(&[]), none).unwrap_err()).contains("fn panel(px: ScPanel)"));
    let own = format!("{FROST}fn sc_backdrop(p: vec2<f32>) -> vec4<f32> {{ return vec4<f32>(0.0); }}\n");
    assert!(compile(&panel(&[]), &own).is_err(), "redefining a host helper is refused");
    let encode = format!("{FROST}fn sc_encode(c: vec3<f32>) -> vec3<f32> {{ return c; }}\n");
    assert!(reason(compile(&panel(&[]), &encode).unwrap_err()).contains("sc_encode"), "a background's helper is not a panel's");
    let bound = format!("@group(0) @binding(12) var<uniform> mine: vec4<f32>;\n{FROST}");
    assert!(reason(compile(&panel(&[]), &bound).unwrap_err()).contains("resource `mine`"));
    let reserved = ShaderTexture { name: "panel".into(), file: "p.png".into() };
    assert!(validate(&[Shader { textures: vec![reserved], ..panel(&[]) }]).is_err(), "panel is a reserved texture name");
}

#[test]
fn the_panel_uniform_sizes_match_the_prelude() {
    let module = naga::front::wgsl::parse_str(&format!("{FROST}\n{}", PANEL_PRELUDE.replace("{textures}", ""))).unwrap();
    let size = |name: &str| {
        let (_, ty) = module.types.iter().find(|(_, t)| t.name.as_deref() == Some(name)).unwrap();
        let naga::TypeInner::Struct { span, .. } = ty.inner else { panic!("{name} is a struct") };
        span as usize
    };
    assert_eq!((size("ScHost"), size("ScPanelRect")), (HOST_UNIFORM_BYTES, PANEL_RECT_UNIFORM_BYTES));
}

#[test]
fn blur_belongs_to_panel_shaders_within_range() {
    validate(&[Shader { blur: Some(14.0), ..panel(&[]) }]).unwrap();
    assert_eq!((Shader { blur: Some(14.0), ..panel(&[]) }.blur_radius(), panel(&[]).blur_radius()), (14.0, DEFAULT_BLUR));
    for bad in [Shader { blur: Some(0.0), ..panel(&[]) }, Shader { blur: Some(61.0), ..panel(&[]) }, Shader { blur: Some(10.0), ..background(&[]) }] {
        assert!(validate(std::slice::from_ref(&bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn a_panel_shader_needs_shaders_0_2() {
    let shaders = json!([{"id": "frost", "name": "Frost", "kind": "panel", "source": "frost.wgsl", "blur": 14,
        "settings": [{"id": "tint", "name": "Tint", "min": 0, "max": 1, "default": 0.3}]}]);
    let plugin = manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.2"], "shaders": shaders})).unwrap();
    assert_eq!((plugin.shaders[0].kind, plugin.shaders[0].blur), (ShaderKind::Panel, Some(14.0)));
    let old = manifest(json!({"id": "sky", "name": "Sky", "version": "1", "capabilities": ["shaders@0.1"], "shaders": shaders})).unwrap_err();
    assert!(old.to_string().contains("shaders@0.2"), "{old}");
}
