//! The window surface is clamped to the device's largest texture (#869): a window moved between
//! monitors of different scale asked for 11512 x 6808 against a maximum of 8192 and the
//! surface configure was a validation panic. The clamp lives in the vendored egui-wgpu, and so
//! does the present mode of a window other than the main one (the agent chat popped out).

use egui_wgpu::wgpu::PresentMode;
use egui_wgpu::{clamp_surface_extent, viewport_present_mode};

#[test]
fn a_window_that_fits_is_unchanged() {
    assert_eq!(clamp_surface_extent(3840, 2160, 8192), (3840, 2160, 1.0));
    assert_eq!(clamp_surface_extent(8192, 8192, 8192), (8192, 8192, 1.0));
}

#[test]
fn the_reported_window_is_clamped_to_the_device_limit() {
    // #869: 11512 x 6808 requested against a maximum of 8192.
    let (w, h, s) = clamp_surface_extent(11512, 6808, 8192);
    assert_eq!(w, 8192);
    assert!(h <= 8192 && h > 0);
    assert!((s - 8192.0 / 11512.0).abs() < 1e-6);
    // Aspect ratio kept within a pixel.
    let expected_h = 6808.0 * 8192.0 / 11512.0;
    assert!((f64::from(h) - expected_h).abs() <= 1.0, "{h} vs {expected_h}");
}

#[test]
fn a_tall_window_is_clamped_on_its_height() {
    let (w, h, s) = clamp_surface_extent(2000, 20000, 8192);
    assert_eq!(h, 8192);
    assert_eq!(w, 819);
    assert!(s < 1.0);
}

#[test]
fn extreme_sizes_never_give_zero_or_exceed_the_limit() {
    for (w, h, m) in [(u32::MAX, 1, 8192), (1, u32::MAX, 8192), (u32::MAX, u32::MAX, 16384), (0, 0, 8192), (5000, 3000, 0), (100_000, 3, 2048)] {
        let (cw, ch, s) = clamp_surface_extent(w, h, m);
        assert!(cw >= 1 && ch >= 1, "{w}x{h} max {m} gave {cw}x{ch}");
        assert!(cw <= m.max(1) && ch <= m.max(1), "{w}x{h} max {m} gave {cw}x{ch}");
        assert!(s > 0.0 && s <= 1.0);
    }
}

#[test]
fn a_popped_out_window_presents_without_waiting_for_the_display() {
    // Mesa's Vulkan on Wayland: the chat's window took the main window down to a crawl while
    // both waited for vsync in one frame.
    let wayland = [PresentMode::Fifo, PresentMode::Mailbox, PresentMode::Immediate];
    assert_eq!(viewport_present_mode(false, PresentMode::AutoVsync, &wayland), PresentMode::Mailbox);
    assert_eq!(viewport_present_mode(false, PresentMode::Fifo, &[PresentMode::Fifo, PresentMode::Immediate]), PresentMode::Immediate);
    // The main window keeps its vsync: it paces both.
    assert_eq!(viewport_present_mode(true, PresentMode::AutoVsync, &wayland), PresentMode::AutoVsync);
}

#[test]
fn a_surface_that_only_waits_and_a_chosen_mode_without_vsync_are_kept() {
    assert_eq!(viewport_present_mode(false, PresentMode::AutoVsync, &[PresentMode::Fifo]), PresentMode::AutoVsync);
    assert_eq!(viewport_present_mode(false, PresentMode::Fifo, &[]), PresentMode::Fifo);
    for configured in [PresentMode::AutoNoVsync, PresentMode::Immediate, PresentMode::Mailbox] {
        assert_eq!(viewport_present_mode(false, configured, &[PresentMode::Fifo, PresentMode::Mailbox]), configured);
        assert_eq!(viewport_present_mode(true, configured, &[PresentMode::Fifo]), configured);
    }
}
