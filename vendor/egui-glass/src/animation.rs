use egui::{Context, Id};

use crate::GlassStyle;

#[derive(Clone, Copy)]
enum Interaction {
    Idle,
    Hovered,
    Pressed,
}

pub(crate) fn interaction_style(ui: &egui::Ui, response: &egui::Response, style: GlassStyle, animate: bool) -> GlassStyle {
    let state = if !ui.is_enabled() {
        Interaction::Idle
    } else if response.is_pointer_button_down_on() || response.dragged() {
        Interaction::Pressed
    } else if response.hovered() {
        Interaction::Hovered
    } else {
        Interaction::Idle
    };
    let time = ui.style().animation_time;
    let duration = if animate && ui.is_enabled() && time.is_finite() { time.clamp(0.0, 0.12) } else { 0.0 };
    animate_style(ui.ctx(), response.id, style, state, duration)
}

fn animate_style(ctx: &Context, id: Id, base: GlassStyle, state: Interaction, duration: f32) -> GlassStyle {
    let hover = ctx.animate_bool_with_time(id.with("egui_glass::hover"), matches!(state, Interaction::Hovered), duration);
    let press = ctx.animate_bool_with_time(id.with("egui_glass::press"), matches!(state, Interaction::Pressed), duration);
    let hovered = base.hovered();
    let pressed = base.pressed();
    // Only interaction-dependent fields are blended; capsule radii may be infinite.
    GlassStyle {
        brightness: egui::lerp(egui::lerp(base.brightness..=hovered.brightness, hover)..=pressed.brightness, press),
        specular: egui::lerp(egui::lerp(base.specular..=hovered.specular, hover)..=pressed.specular, press),
        tint: base.tint.lerp_to_gamma(pressed.tint, press),
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &Context, time: f64, state: Interaction, duration: f32) -> GlassStyle {
        let mut style = GlassStyle::regular().capsule();
        let _ = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |_| {
            style = animate_style(ctx, Id::new("control"), GlassStyle::regular().capsule(), state, duration);
        });
        style
    }

    #[test]
    fn hover_and_press_transition_then_settle_without_changing_geometry() {
        let ctx = Context::default();
        let base = frame(&ctx, 0.0, Interaction::Idle, 0.12);
        let hover = frame(&ctx, 0.02, Interaction::Hovered, 0.12);
        assert!(hover.brightness > base.brightness && hover.brightness < base.hovered().brightness);
        assert_eq!(hover.corner_radius, f32::INFINITY);
        assert_eq!(hover.blur, base.blur);
        for n in 2..=12 { frame(&ctx, n as f64 * 0.02, Interaction::Hovered, 0.12); }
        assert_eq!(frame(&ctx, 0.26, Interaction::Hovered, 0.12), base.hovered());
        let press = frame(&ctx, 0.28, Interaction::Pressed, 0.12);
        assert!(press.brightness < base.hovered().brightness && press.brightness > base.pressed().brightness);
        for n in 15..=26 { frame(&ctx, n as f64 * 0.02, Interaction::Pressed, 0.12); }
        assert_eq!(frame(&ctx, 0.54, Interaction::Pressed, 0.12), base.pressed());
        for n in 28..=40 { frame(&ctx, n as f64 * 0.02, Interaction::Idle, 0.12); }
        assert_eq!(frame(&ctx, 0.82, Interaction::Idle, 0.12), base);
    }

    #[test]
    fn disabling_motion_snaps_an_in_progress_transition_and_resets_it() {
        let ctx = Context::default();
        let base = frame(&ctx, 0.0, Interaction::Idle, 0.12);
        frame(&ctx, 0.02, Interaction::Hovered, 0.12);
        assert_eq!(frame(&ctx, 0.04, Interaction::Pressed, 0.0), base.pressed());
        assert_eq!(frame(&ctx, 0.06, Interaction::Idle, 0.0), base);
        assert_eq!(frame(&ctx, 0.08, Interaction::Idle, 0.12), base);
    }

    #[test]
    fn widget_opt_out_and_zero_host_duration_switch_immediately() {
        for (animate, host_time) in [(false, 0.2), (true, 0.0)] {
            let ctx = Context::default();
            let base = GlassStyle::regular();
            let mut rect = egui::Rect::NOTHING;
            for hovered in [false, true] {
                let events = if hovered { vec![egui::Event::PointerMoved(rect.center())] } else { vec![] };
                let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                    ui.style_mut().animation_time = host_time;
                    let (area, response) = ui.allocate_exact_size(egui::vec2(100.0, 40.0), egui::Sense::click());
                    rect = area;
                    assert_eq!(response.hovered(), hovered);
                    let style = interaction_style(ui, &response, base, animate);
                    assert_eq!(style, if hovered { base.hovered() } else { base });
                });
            }
        }
    }
}
