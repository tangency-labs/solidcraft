//! Opaque document RGB editing. Interface paint uses tokens; the gradients are colour data.
use super::{layout::label, metrics::dimension};
use crate::ui_kit::{Container, Surface};
use crate::{i18n, scenario, theme::Tokens};
use egui::{Color32, Response, Sense, Ui, ecolor::HsvaGamma, pos2, vec2};

fn publish(key: &str, response: &Response) {
    scenario::publish_rect(key, response.rect);
    scenario::publish_handle(key, response.rect.center());
}

/// Linear RGB swatch and precise channel fields. The preview is clipped to display gamut;
/// HDR values and sub-byte precision stay intact until a control is edited.
/// Tokens: control_height, space_4, control_fill/border and the shared colour popover.
pub(crate) fn linear_color_field(ui: &mut Ui, rgb: &mut [f64; 3], maximum: f64) -> Response {
    let maximum = if maximum.is_finite() { maximum.clamp(1.0, 1e6) } else { 1e6 };
    let mut changed = false;
    let output = ui.horizontal(|ui| {
        let preview = rgb.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) as f32 } else { 0.0 });
        let color = Color32::from(egui::Rgba::from_rgb(preview[0], preview[1], preview[2]));
        let mut bytes = [color.r(), color.g(), color.b()];
        if color_field(ui, &mut bytes).changed() {
            let linear = egui::Rgba::from(Color32::from_rgb(bytes[0], bytes[1], bytes[2])).to_array();
            for (channel, value) in rgb.iter_mut().zip(linear) {
                *channel = f64::from(value);
            }
            changed = true;
        }
        let width = ((ui.available_width() - Tokens::get().space_4 * 2.0) / 3.0).max(Tokens::get().space_16);
        for (index, value) in rgb.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.set_width(width);
                changed |= super::number_field(ui, value, 0.0..=maximum, 0.01, "").changed();
            });
        }
    });
    let mut response = output.response;
    if changed {
        response.mark_changed();
    }
    response
}

/// Shared colour button and outside-click-dismissed popover. Retains the enclosing row handle.
pub(crate) fn color_field(ui: &mut Ui, rgb: &mut [u8; 3]) -> Response {
    let t = Tokens::get();
    let (rect, mut response) = ui.allocate_exact_size(vec2(t.control_height + t.space_6, t.control_height), Sense::click());
    ui.painter().rect(
        rect,
        t.control_radius,
        crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0)),
        t.control_border_stroke(t.control_border),
        egui::StrokeKind::Inside,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, ui.is_enabled(), i18n::tr("color.edit")));
    publish(&format!("dialog:color:{}", label(ui)), &response);
    let mut edited = false;
    Container::new(ui.ctx(), Surface::Popup)
        .menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(t.form_card_width + t.space_24 + t.space_12)
        .frame(
            Container::popup_frame(ui.style())
                .fill(t.control_fill)
                .stroke(t.control_border_stroke(t.border))
                .corner_radius(t.radius_xl)
                .inner_margin(t.space_12),
        )
        .show(|ui| {
            super::scope(ui, |ui| {
                edited = color_popover(ui, rgb);
            });
        });
    if edited {
        response.mark_changed();
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.style().explanation_tooltips { response.on_hover_text(i18n::tr("color.edit")) } else { response }
}

/// A colour swatch and editable hexadecimal field; incomplete text remains local until valid.
pub(crate) fn color_hex_field(ui: &mut Ui, rgb: &mut [u8; 3], mixed: Option<&str>) -> Response {
    let id = ui.id().with(("color_hex", label(ui)));
    let current = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
    let is_mixed = mixed.is_some();
    let initial = || if is_mixed { String::new() } else { current.clone() };
    let (observed, was_mixed, mut hex) = ui.ctx().data(|d| d.get_temp::<([u8; 3], bool, String)>(id)).unwrap_or_else(|| (*rgb, is_mixed, initial()));
    if observed != *rgb || was_mixed != is_mixed {
        hex = initial();
    }
    let mut response = color_field(ui, rgb);
    if response.changed() {
        hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
    }
    let mut field = super::bounded_cell_editor(ui, id.with("field"), &mut hex, mixed.unwrap_or("#RRGGBB"), false, 32);
    scenario::publish_target(&format!("dialog:color_hex:{}", label(ui)), field.rect);
    let mut valid = false;
    if field.changed() {
        let text = hex.trim();
        let digits = text.strip_prefix('#').unwrap_or(text);
        if digits.len() == 6
            && let Ok(value) = u32::from_str_radix(digits, 16)
        {
            *rgb = [(value >> 16) as u8, (value >> 8) as u8, value as u8];
            valid = true;
            response.mark_changed();
        }
    }
    field.flags.set(egui::response::Flags::CHANGED, valid);
    ui.ctx().data_mut(|d| d.insert_temp(id, (*rgb, is_mixed, hex)));
    response.union(field)
}

/// Popover body, also rendered independently by the gallery. Format is global, alpha is opaque.
/// Tokens: form_card_width, control_*, form_*, on_accent, border, radius_xl, font_small, space_*.
pub(crate) fn color_popover(ui: &mut Ui, rgb: &mut [u8; 3]) -> bool {
    let t = Tokens::get();
    ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
    let id = ui.id().with("color_hsv");
    let color = crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0));
    // Retain hue for black/grey, and sub-byte precision while dragging.
    let (saved, hsv) = ui.ctx().data(|d| d.get_temp::<([u8; 3], HsvaGamma)>(id)).unwrap_or((*rgb, color.into()));
    let mut hsv = if saved == *rgb { hsv } else { color.into() };
    let before = hsv;
    let mut linear = ui.global_style().visuals.numeric_color_space == egui::style::NumericColorSpace::Linear;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_10;
        ui.horizontal(|ui| {
            let mut selected = usize::from(linear);
            ui.allocate_ui(vec2(t.control_height * 3.0, t.control_height), |ui| {
                super::segmented(ui, &mut selected, &["U8", "F"], super::Segments { compact: true, primary: true, ..Default::default() });
                let rect = ui.min_rect();
                for (index, key) in ["color.gamma", "color.linear"].iter().enumerate() {
                    let rect =
                        egui::Rect::from_min_size(rect.min + vec2(index as f32 * rect.width() / 2.0, 0.0), vec2(rect.width() / 2.0, rect.height()));
                    ui.interact(rect, ui.id().with(("format_tip", index)), Sense::hover()).on_hover_text(i18n::tr(key));
                    scenario::publish_handle(&format!("color:format:{}", if index == 0 { "U8" } else { "F" }), rect.center());
                }
            });
            linear = selected == 1;
            if selected != usize::from(ui.global_style().visuals.numeric_color_space == egui::style::NumericColorSpace::Linear) {
                ui.ctx().all_styles_mut(|s| {
                    s.visuals.numeric_color_space =
                        if linear { egui::style::NumericColorSpace::Linear } else { egui::style::NumericColorSpace::GammaByte }
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), Sense::hover());
                let copy = super::icon_button_at(ui, rect, "copy", i18n::tr("color.copy"));
                publish("color:copy", &copy);
                if copy.clicked() {
                    let [r, g, b] = *rgb;
                    let [lr, lg, lb, _] = egui::Rgba::from(color).to_array();
                    ui.ctx().copy_text(if linear { format!("{lr:.03}, {lg:.03}, {lb:.03}") } else { format!("{r}, {g}, {b}") });
                }
            });
        });
        let mut channels = if linear {
            let [r, g, b, _] = egui::Rgba::from(color).to_array();
            [r as f64, g as f64, b as f64]
        } else {
            rgb.map(f64::from)
        };
        let mut edited = false;
        ui.columns(3, |columns| {
            for ((ui, channel), name) in columns.iter_mut().zip(channels.iter_mut()).zip(["R", "G", "B"]) {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = t.space_3;
                    ui.add(egui::Label::new(egui::RichText::new(name).size(t.font_small).color(t.form_dim)));
                    let response = ui.add_sized(
                        vec2(dimension(ui.available_width()), t.control_height),
                        egui::DragValue::new(channel)
                            .range(0.0..=if linear { 1.0 } else { 255.0 })
                            .speed(if linear { 0.003 } else { 0.5 })
                            .custom_parser(crate::prefs::parse_number)
                            .custom_formatter(move |v, _| if linear { format!("{v:.03}") } else { format!("{v:.0}") }),
                    );
                    response.widget_info(|| {
                        let mut info = egui::WidgetInfo::drag_value(ui.is_enabled(), *channel);
                        info.label = Some(name.into());
                        info
                    });
                    publish(&format!("color:channel:{name}"), &response);
                    edited |= response.changed();
                });
            }
        });
        if edited {
            let [r, g, b] = channels;
            *rgb = if linear {
                let color = Color32::from(egui::Rgba::from_rgb(r as f32, g as f32, b as f32));
                [color.r(), color.g(), color.b()]
            } else {
                channels.map(|v| v.round() as u8)
            };
            hsv = crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0)).into();
        }
        let width = dimension(ui.available_width());
        let (square, response) = ui.allocate_exact_size(vec2(width, t.control_height * 7.0 + t.space_4), Sense::click_and_drag());
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            hsv.s = ((pos.x - square.left()) / square.width()).clamp(0.0, 1.0);
            hsv.v = (1.0 - (pos.y - square.top()) / square.height()).clamp(0.0, 1.0);
            edited = true;
        }
        gradient(ui, square, |x, y| Color32::from(HsvaGamma { h: hsv.h, s: x, v: 1.0 - y, a: 1.0 }));
        marker(ui, pos2(square.left() + hsv.s * square.width(), square.bottom() - hsv.v * square.height()), t.space_6);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), i18n::tr("color.saturation_value")));
        publish("color:sv", &response);
        response.on_hover_text(i18n::tr("color.saturation_value"));
        let (hue, response) = ui.allocate_exact_size(vec2(width, t.space_14), Sense::click_and_drag());
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            hsv.h = ((pos.x - hue.left()) / hue.width()).clamp(0.0, 1.0);
            edited = true;
        }
        gradient(ui, hue, |x, _| Color32::from(HsvaGamma { h: x, s: 1.0, v: 1.0, a: 1.0 }));
        marker(ui, pos2(hue.left() + hsv.h * hue.width(), hue.center().y), t.space_8);
        response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(hsv.h), i18n::tr("color.hue")));
        publish("color:hue", &response);
        response.on_hover_text(i18n::tr("color.hue"));
        if edited {
            let c = Color32::from(hsv);
            *rgb = [c.r(), c.g(), c.b()];
        }
        let (strip, response) = ui.allocate_exact_size(vec2(width, t.form_close_size), Sense::hover());
        ui.painter().rect(
            strip,
            t.control_radius,
            crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0)),
            t.control_border_stroke(t.control_border),
            egui::StrokeKind::Inside,
        );
        publish("color:current", &response);
        response.on_hover_text(i18n::tr("color.selected"));
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, (*rgb, hsv)));
    scenario::publish_rect("color:popover", ui.min_rect());
    hsv != before
}

fn marker(ui: &Ui, at: egui::Pos2, radius: f32) {
    let t = Tokens::get();
    ui.painter().circle_stroke(at, radius, egui::Stroke::new(t.space_3, t.border));
    ui.painter().circle_stroke(at, radius, egui::Stroke::new(1.0, t.on_accent));
}

fn gradient(ui: &Ui, rect: egui::Rect, color: impl Fn(f32, f32) -> Color32) {
    let mut mesh = egui::Mesh::default();
    // Bounded tessellation gives perceptually even HSV gamma gradients.
    const STEPS: u32 = 32;
    let radius = f32::from(Tokens::get().control_radius).min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut rows: Vec<f32> = (0..=STEPS).map(|y| rect.height() * y as f32 / STEPS as f32).collect();
    // Extra samples follow each corner arc even on the tall SV square. Each row spans only
    // the rounded silhouette; colours use actual coordinates, preserving the HSV mapping.
    for step in 0..=8 {
        let y = radius * (1.0 - (step as f32 * std::f32::consts::FRAC_PI_2 / 8.0).cos());
        rows.extend([y, rect.height() - y]);
    }
    rows.sort_by(f32::total_cmp);
    rows.dedup();
    for (row, y) in rows.iter().copied().enumerate() {
        let dy = (radius - y.min(rect.height() - y)).max(0.0);
        let inset = radius - (radius * radius - dy * dy).max(0.0).sqrt();
        for x in 0..=STEPS {
            let px = inset + (rect.width() - 2.0 * inset) * x as f32 / STEPS as f32;
            mesh.colored_vertex(rect.min + vec2(px, y), color(px / rect.width(), y / rect.height()));
            if row > 0 && x < STEPS {
                let i = (row as u32 - 1) * (STEPS + 1) + x;
                mesh.add_triangle(i, i + 1, i + STEPS + 1);
                mesh.add_triangle(i + 1, i + STEPS + 2, i + STEPS + 1);
            }
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    ui.painter().rect_stroke(
        rect,
        Tokens::get().control_radius,
        Tokens::get().control_border_stroke(Tokens::get().control_border),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
#[path = "color_tests.rs"]
mod tests;
