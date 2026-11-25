use egui_wgpu::wgpu;
use solidcraft_engine::plugin::shader::{self, Resource};

/// Every resource the compiled shader binds is in the hook's layout at the same binding, of the
/// same kind and size, and the layout asks for nothing the shader lacks.
pub(super) fn binds_to_the_hook(compiled: &shader::Compiled, textures: usize) {
    let layout = crate::blur::panel_hook_layout(textures);
    assert_eq!(compiled.bindings.len(), layout.len(), "{:?}", compiled.bindings);
    for (binding, resource) in &compiled.bindings {
        let entry = layout.iter().find(|e| e.binding == *binding).unwrap_or_else(|| panic!("binding {binding} is not in the hook's layout"));
        let matches = match (resource, entry.ty) {
            (Resource::Uniform(size), wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, min_binding_size, .. }) => {
                min_binding_size.map(u64::from) == Some(*size)
            }
            (Resource::Sampler, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)) => true,
            (
                Resource::Texture,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ) => true,
            _ => false,
        };
        assert!(matches, "binding {binding}: the shader has {resource:?}, the hook lays out {:?}", entry.ty);
    }
}

/// Loads a plugin folder's panel shader as the app does and checks it against the hook.
pub(super) fn loads_and_binds(folder: &std::path::Path, key: &str) -> crate::plugin_shaders::Loaded {
    let loaded = crate::plugin_shaders::load(folder.to_string_lossy().into_owned(), key.into(), 4096).unwrap_or_else(|e| panic!("{key}: {e}"));
    assert_eq!(loaded.kind, shader::ShaderKind::Panel);
    let source = std::fs::read_to_string(folder.join(&loaded.shader.source)).unwrap();
    binds_to_the_hook(&shader::compile(&loaded.shader, &source).unwrap(), loaded.shader.textures.len());
    loaded
}

#[test]
fn the_hooks_layout_is_the_one_the_prelude_declares() {
    assert_eq!(crate::blur::PANEL_HOOK_GLOBAL_BYTES, shader::HOST_UNIFORM_BYTES);
    assert_eq!(crate::blur::PANEL_HOOK_PANEL_BYTES, shader::PANEL_RECT_UNIFORM_BYTES);
    assert_eq!(crate::blur::PANEL_HOOK_FIRST_TEXTURE, shader::FIRST_TEXTURE_BINDING);
    assert_eq!(crate::blur::PANEL_HOOK_PANEL_BINDING, shader::PANEL_RECT_BINDING);
}

#[test]
fn the_frost_example_loads_and_binds_to_the_hook() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugin-examples/frost");
    let loaded = loads_and_binds(&folder, "frost/frost");
    assert!(loaded.wgsl.contains("fn panel") && !loaded.animated);
    assert_eq!((loaded.shader.blur_radius(), loaded.shader.settings.len()), (16.0, 1));
}

#[test]
fn nothing_is_on_by_default() {
    assert_eq!(crate::prefs::Prefs::default().panel_shader, "", "no panel shader unless a plugin's switch is on");
    let mut h = crate::scenario::Harness::new();
    super::prepare(&mut h.app, &egui::Context::default());
    assert!(!h.app.blur.panel_hook_active(), "panels draw as without plugins");
}

#[test]
fn a_panel_shader_stops_when_its_plugin_is_turned_off_and_comes_back_when_on() {
    let mut h = crate::scenario::Harness::new();
    h.app.preferences.panel_shader = "frost/frost".into();
    h.app.plugin_panels.loading = Some(("frost/frost".into(), crate::plugin_shaders::Pending::default()));
    assert!(super::allowed(&mut h.app, "frost/frost"));
    h.app.preferences.disabled_plugins.insert("frost".into());
    assert!(!super::allowed(&mut h.app, "frost/frost"), "a turned-off plugin's shader must not draw");
    assert!(h.app.plugin_panels.loading.is_none(), "its pending load is dropped");
    h.app.preferences.disabled_plugins.remove("frost");
    assert!(super::allowed(&mut h.app, "frost/frost"));
    assert_eq!(h.app.plugin_panels.failed, None, "tried again");
    super::forget(&mut h.app, "frost");
    assert!(h.app.plugin_panels.loading.is_none());
}
