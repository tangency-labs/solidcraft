use std::sync::Arc;

use egui::{Color32, ColorImage, Context};
use egui_wgpu::RenderState;

use crate::{GlassContext, GlassError};

fn render_state() -> RenderState {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance.request_adapter(&Default::default()).await.expect("native GPU adapter required");
        let (device, queue) = adapter.request_device(&Default::default()).await.expect("GPU device required");
        let target_format = wgpu::TextureFormat::Rgba8Unorm;
        let renderer = egui_wgpu::Renderer::new(&device, target_format, Default::default());
        RenderState {
            adapter, available_adapters: vec![], device, queue, target_format,
            renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
            surface_config: egui_wgpu::SurfaceConfig::LOW_LATENCY,
        }
    })
}

#[test]
#[ignore = "requires a native wgpu adapter"]
fn managed_setup_rejects_duplicates_and_preserves_backdrop_on_error() {
    let rs = render_state();
    let ctx = Context::default();
    assert!(matches!(GlassContext::new(&ctx, &rs, 3), Err(GlassError::UnsupportedSampleCount(3))));
    let glass = GlassContext::new(&ctx, &rs, 0).unwrap();
    let id = glass.set_backdrop(&ColorImage::filled([4, 4], Color32::RED)).unwrap();
    assert!(matches!(GlassContext::new(&ctx, &rs, 1), Err(GlassError::AlreadyInitialized)));
    let mut invalid = ColorImage::filled([1, 1], Color32::WHITE);
    invalid.size = [2, 2];
    assert!(matches!(glass.set_backdrop(&invalid), Err(GlassError::InvalidImage { .. })));
    assert_eq!(crate::backdrop::backdrop_state(&ctx).unwrap().texture, id);
    assert!(rs.renderer.read().texture(&id).is_some());
    assert!(crate::live::test_support::contains_texture(&rs.renderer.read(), id));
    let replacement = glass.set_backdrop(&ColorImage::filled([4, 4], Color32::BLUE)).unwrap();
    assert_ne!(replacement, id);
    assert!(rs.renderer.read().texture(&id).is_none());
    assert!(!crate::live::test_support::contains_texture(&rs.renderer.read(), id));
    assert!(crate::live::test_support::contains_texture(&rs.renderer.read(), replacement));
    let legacy = render_state();
    crate::init(&legacy, 1);
    assert!(matches!(GlassContext::new(&ctx, &legacy, 1), Err(GlassError::AlreadyInitialized)));
}

#[test]
#[ignore = "requires a native wgpu adapter"]
fn native_texture_clones_keep_both_registrations_until_final_drop() {
    let rs = render_state();
    let glass = GlassContext::new(&Context::default(), &rs, 1).unwrap();
    let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("managed texture test"), size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
    });
    let handle = glass.register_native_texture(&texture.create_view(&Default::default()));
    let id = handle.id();
    let clone = handle.clone();
    drop(handle);
    drop(glass);
    assert!(rs.renderer.read().texture(&id).is_some());
    assert!(crate::live::test_support::contains_texture(&rs.renderer.read(), id));
    drop(clone);
    assert!(rs.renderer.read().texture(&id).is_none());
    assert!(!crate::live::test_support::contains_texture(&rs.renderer.read(), id));
}

#[test]
#[ignore = "requires a native wgpu adapter"]
fn managed_fonts_reach_visible_and_twin_contexts() {
    let rs = render_state();
    let ctx = Context::default();
    let glass = GlassContext::new(&ctx, &rs, 1).unwrap();
    let mut fonts = egui::FontDefinitions::default();
    fonts.families.insert(egui::FontFamily::Name("managed-test".into()), fonts.families[&egui::FontFamily::Proportional].clone());
    for family in ["initial", "updated"] {
        fonts.families.insert(egui::FontFamily::Name(family.into()), fonts.families[&egui::FontFamily::Proportional].clone());
        glass.set_fonts(fonts.clone());
        for _ in 0..2 {
            let mut calls = 0;
            let _ = ctx.run_ui(Default::default(), |ui| {
                glass.live_backdrop().clone().run(ui, Color32::WHITE, |ui| {
                    assert_eq!(ui.fonts(|view| view.definitions().families.clone()), fonts.families, "font families in layout {calls}");
                    calls += 1;
                });
            });
            assert!(calls >= 2, "both visible and offscreen layouts must be checked");
        }
    }
    let legacy_rs = render_state();
    crate::init(&legacy_rs, 1);
    let legacy = crate::LiveBackdrop::new(&legacy_rs, Some(fonts.clone()));
    let _ = ctx.run_ui(Default::default(), |ui| {
        legacy.run(ui, Color32::WHITE, |ui| {
            assert_eq!(ui.fonts(|view| view.definitions().families.clone()), fonts.families);
        });
    });
}
