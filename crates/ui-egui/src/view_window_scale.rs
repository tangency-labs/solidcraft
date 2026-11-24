//! Translate one native child viewport into interface points without changing shared egui zoom.

use egui::{Context, Event, FullOutput, MouseWheelUnit, Pos2, RawInput, Rect, ViewportId};

#[derive(Default)]
pub(crate) struct WindowScale {
    pending: Option<(ViewportId, f32)>,
    painting: Option<f32>,
}

pub(crate) fn in_window(ctx: &Context, id: ViewportId, scale: f32, show: impl FnOnce()) {
    if scale == 1.0 {
        show();
        return;
    }
    let plugin = ctx.plugin_or_default::<WindowScale>();
    plugin.lock().pending = Some((id, scale));
    show();
    let mut state = plugin.lock();
    state.pending = None;
    state.painting = None;
}

impl egui::Plugin for WindowScale {
    fn debug_name(&self) -> &'static str {
        "SolidCraft pop-out interface scale"
    }

    fn input_hook(&mut self, _: &Context, input: &mut RawInput) {
        let Some((id, scale)) = self.pending.filter(|(id, _)| *id == input.viewport_id) else { return };
        let Some(viewport) = input.viewports.get_mut(&id) else { return };
        let Some(native_scale) = viewport.native_pixels_per_point else { return };
        viewport.native_pixels_per_point = Some(native_scale * scale);
        let inverse = scale.recip();
        input.screen_rect = input.screen_rect.map(|rect| resize(rect, inverse));
        input.safe_area_insets = input.safe_area_insets.map(|mut insets| {
            insets.0.left *= inverse;
            insets.0.right *= inverse;
            insets.0.top *= inverse;
            insets.0.bottom *= inverse;
            insets
        });
        viewport.inner_rect = viewport.inner_rect.map(|rect| resize(rect, inverse));
        viewport.outer_rect = viewport.outer_rect.map(|rect| resize(rect, inverse));
        viewport.monitor_size = viewport.monitor_size.map(|size| size * inverse);

        for event in &mut input.events {
            match event {
                Event::PointerMoved(pos) | Event::PointerButton { pos, .. } | Event::MiddleClickPaste { pos, .. } | Event::Touch { pos, .. } => {
                    *pos = point(*pos, inverse)
                }
                Event::MouseWheel { unit: MouseWheelUnit::Point, delta, .. } => *delta *= inverse,
                _ => {}
            }
        }
        self.painting = Some(scale);
    }

    fn output_hook(&mut self, _: &Context, output: &mut FullOutput) {
        let Some(scale) = self.painting.take() else { return };
        // The native input/output bridge still uses monitor DPI and the shared context zoom.
        if let Some(ime) = &mut output.platform_output.ime {
            ime.rect = resize(ime.rect, scale);
            ime.cursor_rect = resize(ime.cursor_rect, scale);
        }
    }
}

fn point(pos: Pos2, scale: f32) -> Pos2 {
    (pos.to_vec2() * scale).to_pos2()
}

fn resize(rect: Rect, scale: f32) -> Rect {
    Rect::from_min_max(point(rect.min, scale), point(rect.max, scale))
}
