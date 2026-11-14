//! Display-wake regression (#3043). Replay stale resize payloads and live window snapshots
//! without a monitor/GPU, as the surface retry tests do with a stand-in device. Native DPI is
//! supplied afresh each frame by egui-winit; exercise egui's real input/output and zoom here.

use egui_wgpu::{clamp_surface_extent, surface_resize_for_window};

#[test]
fn wake_without_a_final_resize_restores_the_surface_and_dpi() {
    for transient_scale in [0.75, 1.0] {
        for zoom in [1.0, 1.25] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            let final_size = (2400, 1500);
            let transient_size = ((1600.0 * transient_scale) as u32, (1000.0 * transient_scale) as u32);
            let mut requested = final_size;
            let mut configured = clamp_surface_extent(requested.0, requested.1, 8192);
            // Normal frame; transient scale+resize, then occlusion; wake with no resize;
            // another delayed stale resize payload; stable frame (no redundant configure).
            let frames = [
                (None, final_size, 1.5, false),
                (Some(transient_size), transient_size, transient_scale, false),
                (Some(transient_size), transient_size, transient_scale, true),
                (None, final_size, 1.5, false),
                (Some(transient_size), final_size, 1.5, false),
                (None, final_size, 1.5, false),
            ];
            let mut reconciles = 0;
            for (resize_event, live_size, native_scale, occluded) in frames {
                if let Some(size) = resize_event {
                    requested = size;
                    configured = clamp_surface_extent(size.0, size.1, 8192);
                }
                if occluded {
                    continue;
                }
                if let Some((w, h)) = surface_resize_for_window(requested, live_size) {
                    requested = (w.get(), h.get());
                    configured = clamp_surface_extent(requested.0, requested.1, 8192);
                    reconciles += 1;
                }
                let ppp = native_scale * zoom;
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(live_size.0 as f32, live_size.1 as f32) / ppp)),
                    ..Default::default()
                };
                input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(native_scale);
                let mut output = ctx.run_ui(input, |_| {});
                output.textures_delta.clear();
                assert_eq!(configured, (live_size.0, live_size.1, 1.0));
                assert_eq!(output.pixels_per_point, ppp, "native DPI and user zoom must both survive wake");
            }
            assert_eq!(reconciles, 2, "wake and delayed stale event are repaired, stable frames do not reconfigure");
        }
    }
}

#[test]
fn minimized_and_clamped_windows_do_not_reconfigure_each_frame() {
    for current in [(0, 0), (0, 1500), (2400, 0)] {
        assert_eq!(surface_resize_for_window((2400, 1500), current), None);
    }
    let requested = (16000, 10000);
    assert_eq!(clamp_surface_extent(requested.0, requested.1, 8192), (8192, 5120, 0.512));
    assert_eq!(surface_resize_for_window(requested, requested), None, "compare the live size with the request, not the clamped texture");
    let restored = surface_resize_for_window(requested, (2400, 1500)).map(|(w, h)| clamp_surface_extent(w.get(), h.get(), 8192));
    assert_eq!(restored, Some((2400, 1500, 1.0)), "wake must also clear a transient texture clamp");
}
