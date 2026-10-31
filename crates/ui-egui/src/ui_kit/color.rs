//! Document RGB and optional separate opacity editing. Interface paint uses tokens; gradients are colour data.
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
            let linear = egui::Rgba::from(crate::theme::srgb(bytes.map(|c| f32::from(c) / 255.0))).to_array();
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
    color_field_inner(ui, rgb, None, false)
}

/// The same picker, editing the caller's exact opacity without premultiplying RGB.
/// Call the opaque `color_field` instead when transparency is prohibited (including PBR).
pub(crate) fn color_field_with_opacity(ui: &mut Ui, rgb: &mut [u8; 3], opacity: &mut f64) -> Response {
    color_field_inner(ui, rgb, Some(opacity), false)
}

/// The same picker with a non-editable mixed-value label. Pass no opacity for opaque/PBR callers.
/// Explicitly accepting the first colour still reports an edit when targets have mixed colours.
pub(crate) fn mixed_color_field(ui: &mut Ui, rgb: &mut [u8; 3], opacity: Option<&mut f64>, mixed: Option<&str>) -> Response {
    let response = color_field_inner(ui, rgb, opacity, mixed.is_some());
    if let Some(mixed) = mixed {
        super::text(ui, mixed, super::Text::Note("color:mixed"));
    }
    response
}

fn color_field_inner(ui: &mut Ui, rgb: &mut [u8; 3], opacity: Option<&mut f64>, mixed: bool) -> Response {
    let t = Tokens::get();
    let (rect, mut response) = ui.allocate_exact_size(vec2(t.control_height + t.space_6, t.control_height), Sense::click());
    ui.painter().rect(
        rect,
        t.control_radius,
        crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0)),
        t.control_border_stroke(t.control_border),
        egui::StrokeKind::Inside,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ColorWell, ui.is_enabled(), i18n::tr("color.edit")));
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
                edited = if opacity.is_some() || mixed { color_popover_with_opacity(ui, rgb, opacity, mixed) } else { color_popover(ui, rgb) };
            });
        });
    if edited {
        response.mark_changed();
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.style().explanation_tooltips { response.on_hover_text(i18n::tr("color.edit")) } else { response }
}

/// Popover body, also rendered independently by the gallery. Format is global, alpha is opaque.
/// Tokens: form_card_width, control_*, form_*, on_accent, border, radius_xl, font_small, space_*.
pub(crate) fn color_popover(ui: &mut Ui, rgb: &mut [u8; 3]) -> bool {
    color_popover_with_opacity(ui, rgb, None, false)
}

fn color_popover_with_opacity(ui: &mut Ui, rgb: &mut [u8; 3], mut opacity: Option<&mut f64>, mixed: bool) -> bool {
    let t = Tokens::get();
    ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
    let id = ui.scope_id().with("color_hsv");
    let color = crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0));
    // Retain hue for black/grey, and sub-byte precision while dragging.
    let (saved, hsv) = ui.ctx().data(|d| d.get_temp::<([u8; 3], HsvaGamma)>(id)).unwrap_or((*rgb, color.into()));
    let mut hsv = if saved == *rgb { hsv } else { color.into() };
    let before = hsv;
    let before_rgb = *rgb;
    let before_opacity = opacity.as_deref().copied();
    let hex_id = ui.scope_id().with("color_hex");
    let mut hex = ui.ctx().data(|d| d.get_temp::<HexEdit>(hex_id)).unwrap_or_else(|| HexEdit::new(*rgb, before_opacity));
    hex.sync(*rgb, before_opacity);
    let format_id = egui::Id::unique("color_hex_format");
    let mut hexadecimal = ui.ctx().data(|d| d.get_temp::<bool>(format_id)).unwrap_or(false);
    let mut linear = ui.global_style().visuals.numeric_color_space == egui::style::NumericColorSpace::Linear;
    let mut accepted_mixed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_10;
        ui.horizontal(|ui| {
            let mut selected = if hexadecimal { 2 } else { usize::from(linear) };
            ui.allocate_ui(vec2(t.control_height * 4.0 + t.space_12, t.control_height), |ui| {
                super::segmented(
                    ui,
                    &mut selected,
                    &["U8", "F", i18n::tr("color.hex")],
                    super::Segments { compact: true, primary: true, ..Default::default() },
                );
                let rect = ui.min_rect();
                for (index, (key, name)) in [("color.gamma", "U8"), ("color.linear", "F"), ("color.hex_help", "Hex")].iter().enumerate() {
                    let rect =
                        egui::Rect::from_min_size(rect.min + vec2(index as f32 * rect.width() / 3.0, 0.0), vec2(rect.width() / 3.0, rect.height()));
                    ui.interact(rect, ui.scope_id().with(("format_tip", index)), Sense::hover()).on_hover_text(i18n::tr(key));
                    scenario::publish_handle(&format!("color:format:{name}"), rect.center());
                }
            });
            hexadecimal = selected == 2;
            if !hexadecimal && (selected == 1) != linear {
                linear = selected == 1;
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
                    ui.ctx().copy_text(if hexadecimal {
                        hex.accepted.clone()
                    } else if linear {
                        format!("{lr:.03}, {lg:.03}, {lb:.03}")
                    } else {
                        format!("{r}, {g}, {b}")
                    });
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
        if hexadecimal {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = t.space_3;
                ui.add(egui::Label::new(egui::RichText::new(i18n::tr("color.hex")).size(t.font_small).color(t.form_dim)));
                let response = super::cell_editor(ui, hex_id.with("input"), &mut hex.draft, i18n::tr("color.hex_hint"), false)
                    .on_hover_text(i18n::tr("color.hex_help"));
                publish("color:hex", &response);
                if response.changed()
                    && let Some((value, alpha)) = parse_hex(&hex.draft)
                    && (opacity.is_some() || alpha.is_none_or(|a| a == 255))
                {
                    accepted_mixed = mixed;
                    *rgb = value;
                    if let (Some(opacity), Some(alpha)) = (opacity.as_deref_mut(), alpha) {
                        *opacity = f64::from(alpha) / 255.0;
                    }
                    hex.alpha = alpha.is_some();
                    hex.set(*rgb, opacity.as_deref().copied());
                    if *rgb != before_rgb {
                        hsv = crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0)).into();
                    }
                }
            });
        } else {
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
        }
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
        // Only graphical edits convert HSV back to RGB. Typed byte values stay exact.
        let mut graphical_edit = false;
        let width = dimension(ui.available_width());
        let (square, response) = ui.allocate_exact_size(vec2(width, t.control_height * 7.0 + t.space_4), Sense::click_and_drag());
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            hsv.s = ((pos.x - square.left()) / square.width()).clamp(0.0, 1.0);
            hsv.v = (1.0 - (pos.y - square.top()) / square.height()).clamp(0.0, 1.0);
            graphical_edit = true;
        }
        gradient(ui, square, |x, y| Color32::from(HsvaGamma { h: hsv.h, s: x, v: 1.0 - y, a: 1.0 }));
        marker(ui, pos2(square.left() + hsv.s * square.width(), square.bottom() - hsv.v * square.height()), t.space_6);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, ui.is_enabled(), i18n::tr("color.saturation_value")));
        publish("color:sv", &response);
        response.on_hover_text(i18n::tr("color.saturation_value"));
        let (hue, response) = ui.allocate_exact_size(vec2(width, t.space_14), Sense::click_and_drag());
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            hsv.h = ((pos.x - hue.left()) / hue.width()).clamp(0.0, 1.0);
            graphical_edit = true;
        }
        gradient(ui, hue, |x, _| Color32::from(HsvaGamma { h: x, s: 1.0, v: 1.0, a: 1.0 }));
        marker(ui, pos2(hue.left() + hsv.h * hue.width(), hue.center().y), t.space_8);
        response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(hsv.h), i18n::tr("color.hue")));
        publish("color:hue", &response);
        response.on_hover_text(i18n::tr("color.hue"));
        if graphical_edit {
            let c = Color32::from(hsv);
            *rgb = [c.r(), c.g(), c.b()];
            hex.set(*rgb, opacity.as_deref().copied());
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
    hex.sync(*rgb, opacity.as_deref().copied());
    ui.ctx().data_mut(|d| d.insert_temp(id, (*rgb, hsv)));
    ui.ctx().data_mut(|d| {
        d.insert_temp(hex_id, hex);
        d.insert_temp(format_id, hexadecimal);
    });
    scenario::publish_rect("color:popover", ui.min_rect());
    *rgb != before_rgb || opacity.as_deref().copied() != before_opacity || hsv != before || accepted_mixed
}

/// Text drafts are independent of model values. Copy retains the last accepted representation,
/// even while an incomplete/invalid draft is visible. Never round-trip opacity through its byte.
#[derive(Clone)]
struct HexEdit {
    draft: String,
    accepted: String,
    rgb: [u8; 3],
    opacity: Option<f64>,
    alpha: bool,
}

impl HexEdit {
    fn new(rgb: [u8; 3], opacity: Option<f64>) -> Self {
        let mut edit = Self { draft: String::new(), accepted: String::new(), rgb, opacity, alpha: opacity.is_some_and(|a| a != 1.0) };
        edit.set(rgb, opacity);
        edit
    }

    fn set(&mut self, rgb: [u8; 3], opacity: Option<f64>) {
        let [r, g, b] = rgb;
        self.accepted = format!("#{r:02x}{g:02x}{b:02x}");
        if self.alpha {
            let a = (opacity.unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            self.accepted.push_str(&format!("{a:02x}"));
        }
        self.draft.clone_from(&self.accepted);
        self.rgb = rgb;
        self.opacity = opacity;
    }

    fn sync(&mut self, rgb: [u8; 3], opacity: Option<f64>) {
        if self.rgb != rgb || self.opacity != opacity {
            // An external opacity edit must be visible, including after six-digit input.
            if opacity.is_some() && self.opacity != opacity {
                self.alpha = true;
            }
            self.set(rgb, opacity);
        }
    }
}

fn parse_hex(text: &str) -> Option<([u8; 3], Option<u8>)> {
    let digits = text.strip_prefix('#')?;
    if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| digits.get(at..at + 2).and_then(|value| u8::from_str_radix(value, 16).ok());
    Some(([byte(0)?, byte(2)?, byte(4)?], if digits.len() == 8 { Some(byte(6)?) } else { None }))
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
