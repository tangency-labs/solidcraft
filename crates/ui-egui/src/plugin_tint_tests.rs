use super::*;
use crate::ui_kit::tint::Region;

fn region(region: Region, x: f32) -> Ask {
    let rect = egui::Rect::from_min_size(egui::pos2(x, 40.0), egui::vec2(32.0, 32.0));
    Ask { region, rect, at: rect.center(), base: egui::Color32::from_rgb(40, 90, 200), enabled: 1.0 }
}

fn example(name: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugin-examples").join(name).to_string_lossy().into_owned()
}

/// The tint plugins to run: every tint among the docs' examples, and any folders
/// `SOLIDCRAFT_TINT_PLUGINS` names (separated as `PATH` is), such as a plugin of one's own.
fn tint_plugins() -> Vec<(String, String)> {
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugin-examples");
    let mut folders: Vec<String> = std::fs::read_dir(&docs)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| crate::plugin_shaders::manifest(&p.to_string_lossy()).is_ok_and(|m| m.shaders.iter().any(|s| s.kind == ShaderKind::Tint)))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    folders.sort();
    if let Some(extra) = std::env::var_os("SOLIDCRAFT_TINT_PLUGINS") {
        folders.extend(std::env::split_paths(&extra).map(|p| p.to_string_lossy().into_owned()));
    }
    folders
        .into_iter()
        .flat_map(|folder| {
            let manifest = crate::plugin_shaders::manifest(&folder).unwrap_or_else(|e| panic!("{folder}: {e}"));
            manifest
                .shaders
                .iter()
                .filter(|s| s.kind == ShaderKind::Tint)
                .map(|s| (folder.clone(), format!("{}/{}", manifest.id, s.id)))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every tint plugin loads, compiles through the host's check and runs on the GPU: answers for
/// each kind of region, finite and in range; an animated one changes with time.
#[test]
fn tint_plugins_compile_and_run_on_the_gpu() {
    let plugins = tint_plugins();
    assert!(!plugins.is_empty());
    let asked: Vec<Ask> =
        [Region::Icon, Region::Button, Region::Panel].into_iter().flat_map(|r| [0.0, 400.0, 800.0, 1200.0].map(|x| region(r, x))).collect();
    let Some((adapter, device, queue)) = crate::gpu::offscreen_device() else {
        for (folder, key) in &plugins {
            crate::plugin_shaders::load(folder.clone(), key.clone(), Default::default(), 1024).unwrap_or_else(|e| panic!("{key}: {e}"));
            eprintln!("{key}: compiled; no GPU adapter, so not run");
        }
        return;
    };
    if !adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        eprintln!("{}: no compute shaders, tints not run", adapter.get_info().name);
        return;
    }
    for (folder, key) in &plugins {
        let run = |time: f32| {
            let loaded = crate::plugin_shaders::load(folder.clone(), key.clone(), Default::default(), 1024).unwrap_or_else(|e| panic!("{key}: {e}"));
            let animated = loaded.animated;
            (evaluate(&adapter, &device, &queue, loaded, &asked, time).unwrap_or_else(|e| panic!("{key}: {e}")), animated)
        };
        let (at_0, animated) = run(0.0);
        assert_eq!(at_0.len(), asked.len());
        assert!(at_0.iter().flatten().all(|v| v.is_finite() && (0.0..=1.0).contains(v)), "{key}: {at_0:?}");
        if animated {
            assert_ne!(run(1.0).0, at_0, "{key}: an animated tint moves with time");
        }
        eprintln!("{key}: ran {} regions on {} ({:?})", asked.len(), adapter.get_info().name, adapter.get_info().backend);
    }
}

#[test]
fn a_background_is_not_taken_for_a_tint() {
    let Some((adapter, device, queue)) = crate::gpu::offscreen_device() else { return };
    let loaded = crate::plugin_shaders::load(example("dusk"), "dusk/dusk".into(), Default::default(), 1024).unwrap();
    let error = build(&adapter, &device, &queue, loaded).err().unwrap();
    assert_eq!(error, "not a tint");
}

#[test]
fn nothing_is_tinted_by_default_or_while_the_plugin_is_off() {
    let mut h = crate::scenario::Harness::new();
    let ctx = egui::Context::default();
    assert_eq!(h.app.preferences.tint_shader, "", "no tint by default");
    begin(&mut h.app, &ctx);
    assert!(!tint::is_on(&ctx));
    h.app.plugin_tint.render = None;
    h.app.preferences.tint_shader = "accent-sweep/accent-sweep".into();
    h.app.preferences.disabled_plugins.insert("accent-sweep".into());
    begin(&mut h.app, &ctx);
    assert!(!tint::is_on(&ctx), "a turned-off plugin's tint does not run");
    assert!(h.app.plugin_tint.loading.is_none() && h.app.plugin_tint.active.is_none());
}

#[test]
fn the_host_uniforms_are_the_size_the_prelude_declares() {
    let ctx = egui::Context::default();
    let bytes = host_uniforms(&ctx, 2.5, &solidcraft_theme::Tokens::get(), [7.0, 0.0, 0.0, 0.0]);
    assert_eq!(bytes.len(), shader::HOST_UNIFORM_BYTES);
    assert_eq!(f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]), 2.5);
    assert_eq!(f32::from_le_bytes([bytes[144], bytes[145], bytes[146], bytes[147]]), 7.0, "sc.settings last");
}
