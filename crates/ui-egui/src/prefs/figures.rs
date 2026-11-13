//! SolidCraft's own Preferences specimens: figures drawn from SolidCraft's content (a feature row,
//! its status colours, what the colour vision modes recolour, the autosave interval, the agent
//! pointer) with the kit's painters. Every colour and metric comes from tokens.
use crate::ui_kit::{self, BackingTooltipExt as _, dimension};
use egui::{FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_theme::{ColorVision, Tokens};

/// The interface scale's live preview: a field, OK and a feature row, at `scale`.
pub(crate) fn scale_specimen(ui: &mut Ui, scale: f32) {
    let t = Tokens::get();
    let scale = if scale.is_finite() { scale.clamp(0.5, 2.5) / ui.ctx().zoom_factor().max(0.5) } else { 1.0 };
    let (r, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_24 * 4.0 + t.space_16), Sense::hover());
    t.container_rect(ui.painter(), r, t.radius_lg, t.control_fill, t.dialog_rule);
    let p = ui.painter().with_clip_rect(r.shrink(t.space_8));
    let font = FontId::proportional(t.font_label * scale);
    p.text(
        r.right_top() + vec2(-t.space_8, t.space_8),
        egui::Align2::RIGHT_TOP,
        solidcraft_ui_text::tr("prefs.live_preview").to_uppercase(),
        FontId::proportional(t.font_micro),
        t.form_dim,
    );
    let origin = r.min + vec2(t.space_10, t.space_24);
    let line_h = t.control_height * scale;
    let distance = solidcraft_ui_text::tr_label("Distance");
    let label = p.layout_no_wrap(distance.to_owned(), font.clone(), t.form_dim);
    p.galley(origin + vec2(0.0, (line_h - label.size().y) / 2.0), label.clone(), t.form_dim);
    let field = Rect::from_min_size(origin + vec2(label.size().x + t.space_8 * scale, 0.0), vec2((t.form_label_width + t.space_6) * scale, line_h));
    p.rect(field, t.control_radius, t.input_fill(), t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    if let Some(bevel) = t.borders.bevel {
        bevel.paint(&p, field, t.control_radius, true, true);
    }
    ui_kit::centred_text(&p, field, "12.50", FontId::monospace(t.font_body * scale), t.form_ink);
    let button = Rect::from_min_size(field.right_top() + vec2(t.space_8 * scale, 0.0), vec2(t.space_24 * 1.5 * scale, line_h));
    p.rect_filled(button, t.control_radius, if t.borders.bevel.is_some() { t.control_fill } else { t.form_primary });
    if let Some(bevel) = t.borders.bevel {
        bevel.paint(&p, button, t.control_radius, false, true);
    }
    ui_kit::centred_text(
        &p,
        button,
        solidcraft_ui_text::tr_label("OK"),
        ui_kit::type_face::semibold(t.font_label * scale),
        if t.borders.bevel.is_some() { t.form_ink } else { t.on_accent },
    );
    let feature = Rect::from_min_size(origin + vec2(0.0, line_h + t.space_8 * scale), vec2(r.width() - t.space_10 * 2.0, line_h));
    p.rect_filled(feature, t.control_radius, t.selected_bg);
    let icon = Rect::from_center_size(pos2(feature.left() + t.space_16 * scale, feature.center().y), vec2(t.space_16, t.space_16) * scale);
    ui_kit::icons::paint_icon(&p, icon, "extrude", 1.0);
    p.text(
        pos2(icon.right() + t.space_6 * scale, feature.center().y),
        egui::Align2::LEFT_CENTER,
        "Extrude1",
        ui_kit::type_face::semibold(t.font_label * scale),
        t.selected_ink,
    );
    p.text(feature.right_center() - vec2(t.space_8, 0.0), egui::Align2::RIGHT_CENTER, "12.5 mm", font, t.selected_ink);
}

/// The status colours (selected, warning, error, done) as chips, in colour vision `mode`.
pub(crate) fn status_palette_sample(ui: &mut Ui, mode: ColorVision) {
    if ui_kit::settings_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let colours = Tokens::themed(solidcraft_theme::is_dark(), mode);
    ui.horizontal_wrapped(|ui| {
        for (label, icon, fill) in [
            ("prefs.status_selected", "finish", colours.form_primary),
            ("prefs.status_warning", "warning", colours.warning),
            ("prefs.status_error", "close", colours.error),
            ("prefs.status_done", "finish", colours.form_success),
        ] {
            let ink = if solidcraft_theme::contrast_ratio(t.on_accent, fill) >= 4.5 { t.on_accent } else { t.tab_on_fill_dark };
            let label = solidcraft_ui_text::tr(label);
            let galley = ui.painter().layout_no_wrap(label.to_owned(), ui_kit::type_face::semibold(t.font_small), ink);
            let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + t.space_24 + t.space_16, t.space_24), Sense::hover());
            solidcraft_ui_registry::publish_rect(&format!("prefs:status_sample:{label}"), r);
            ui.painter().rect_filled(r, t.radius_xl, fill);
            ui_kit::icons::paint_ink(
                ui.painter(),
                Rect::from_center_size(pos2(r.left() + t.space_12, r.center().y), vec2(t.space_12, t.space_12)),
                icon,
                ink,
            );
            ui.painter().galley(pos2(r.left() + t.space_24, r.center().y - galley.size().y / 2.0), galley, ink);
        }
    });
}

/// The names under the Colour vision sample (message ids), one per figure of
/// [`ui_kit::paint_colour_vision`].
pub(crate) const COLOUR_VISION_SAMPLE: [&str; 6] =
    ["ui.colour_vision_fixed", "ui.colour_vision_projected", "ui.axes", "ui.colour_vision_bends", "ui.draft", "ui.colour_vision_failed"];

/// The Colour vision sample (#778): fixed and projected sketch lines, the X and Y axes, bends up
/// and down, Draft Analysis and a failed feature, each drawn as it is where it appears, in the
/// colours of `t` (the preference passes the mode being chosen, #7152). The six cells share one
/// responsive, clipped area.
pub(crate) fn colour_vision_sample(ui: &mut Ui, t: &Tokens) {
    if ui_kit::settings_search::suppress_chrome(ui) {
        return;
    }
    let count = COLOUR_VISION_SAMPLE.len() as f32;
    let gap = t.space_8.min(dimension(ui.available_width()) / (count - 1.0));
    let width = ((dimension(ui.available_width()) - gap * (count - 1.0)) / count).min(t.space_24 * 3.0 + t.space_12);
    let figure_height = t.space_24 + t.space_16 + t.space_4;
    let (area, _) = ui.allocate_exact_size(vec2(width * count + gap * (count - 1.0), figure_height + t.space_3 + t.space_16), Sense::hover());
    solidcraft_ui_registry::publish_rect("prefs:colour_vision_samples", area);
    let p = ui.painter_at(area);
    for (i, name) in COLOUR_VISION_SAMPLE.iter().enumerate() {
        let cell = Rect::from_min_size(area.min + vec2((width + gap) * i as f32, 0.0), vec2(width, figure_height));
        solidcraft_ui_registry::publish_rect(&format!("prefs:colour_vision_sample:{i}"), cell);
        ui_kit::paint_colour_vision(&p, cell, i, t);
        ui_kit::centred_text(
            &p,
            Rect::from_min_size(cell.left_bottom() + vec2(0.0, t.space_3), vec2(cell.width(), t.space_16)),
            solidcraft_ui_text::tr(name),
            FontId::proportional(t.font_small),
            t.text_dim,
        );
    }
}

/// An illustrative half-hour of autosave marks, using the actual configured interval.
pub(crate) fn autosave_timeline(ui: &mut Ui, minutes: f64) {
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_24 + t.space_12 + t.space_2), Sense::hover());
    let line = r;
    let page = ui_kit::settings_page_active(ui);
    if page {
        solidcraft_ui_registry::publish_rect("prefs:autosave_timeline", r);
    }
    ui.painter().hline(line.x_range(), r.top() + t.space_10, Stroke::new(2.0, t.dialog_rule));
    if minutes.is_finite() && minutes > 0.0 {
        let ticks = (30.0 / minutes).floor().clamp(0.0, 60.0) as u32;
        for i in 0..=ticks {
            let x = line.right() - line.width() * (f64::from(i) * minutes / 30.0) as f32;
            ui.painter().vline(x, r.top() + t.space_4..=r.top() + t.space_16, Stroke::new(2.0, t.form_primary));
        }
    }
    for (share, key) in [(0.0, "prefs.timeline_half_hour"), (0.5, "prefs.timeline_quarter_hour"), (1.0, "prefs.timeline_now")] {
        if page && share == 0.5 {
            continue;
        }
        ui.painter().text(
            pos2(line.left() + line.width() * share, r.top() + t.space_24),
            if share == 0.0 {
                egui::Align2::LEFT_TOP
            } else if share == 1.0 {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::CENTER_TOP
            },
            solidcraft_ui_text::tr(key),
            FontId::proportional(t.font_small),
            t.form_dim,
        );
    }
}

/// A static preview reuses the real agent cursor artwork; it never animates or requests frames.
pub(crate) fn pointer_preview(ui: &mut Ui, visible: bool, speed: &str, follows_camera: bool) {
    let t = Tokens::get();
    let width = dimension(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.space_24 * 3.0 + t.space_16), Sense::hover());
    ui.painter().rect(rect, t.radius_lg, t.viewport_top, Stroke::new(1.0, t.dialog_rule), egui::StrokeKind::Inside);
    let path = [pos2(rect.left() + t.space_24, rect.center().y), rect.center(), pos2(rect.right() - t.space_24 * 2.0, rect.center().y - t.space_12)];
    ui.painter().extend(egui::Shape::dashed_line(&path, Stroke::new(1.0, t.control_border), t.space_3, t.space_4));
    for point in path {
        ui.painter().circle_filled(point, t.space_3, t.control_border);
    }
    if visible {
        ui_kit::icons::glyphs::pointer(&ui.painter().with_clip_rect(rect.shrink(t.space_4)), path.get(1).copied().unwrap_or(rect.center()));
    }
    let caption = if visible {
        solidcraft_ui_text::format("ai.prefs.pointer_preview_speed", &[("speed", speed)])
    } else {
        solidcraft_ui_text::tr("ai.prefs.pointer_preview_hidden").to_owned()
    };
    let caption_rect = Rect::from_min_max(rect.min + vec2(t.space_8, t.space_4), pos2(rect.right() - t.space_8, rect.top() + t.space_24));
    ui_kit::paint_text(ui, caption_rect, &caption, FontId::proportional(t.font_small), t.form_dim, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), &caption));
    response.backing_on_hover_text(solidcraft_ui_text::tr(if follows_camera { "ai.prefs.follow_camera_on" } else { "ai.prefs.follow_camera_off" }));
}
