//! Document RGB and optional separate opacity editing. Interface paint uses tokens; gradients are colour data.
use super::{layout::label, metrics::dimension};
use crate::recent_colors::{MAX_RECENT_COLORS, recent_colors, remember_color};
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
    let id = ui.id().with("color_hsv");
    let color = crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0));
    // Retain hue for black/grey, and sub-byte precision while dragging.
    let (saved, hsv) = ui.ctx().data(|d| d.get_temp::<([u8; 3], HsvaGamma)>(id)).unwrap_or((*rgb, color.into()));
    let mut hsv = if saved == *rgb { hsv } else { color.into() };
    let before = hsv;
    let before_rgb = *rgb;
    let before_opacity = opacity.as_deref().copied();
    let hex_id = ui.id().with("color_hex");
    let mut hex = ui.ctx().data(|d| d.get_temp::<HexEdit>(hex_id)).unwrap_or_else(|| HexEdit::new(*rgb, before_opacity));
    hex.sync(*rgb, before_opacity);
    let format_id = egui::Id::new("color_hex_format");
    let percent_id = egui::Id::new("color_percent_format");
    let mut hexadecimal = ui.ctx().data(|d| d.get_temp::<bool>(format_id)).unwrap_or(false);
    let mut percent = ui.ctx().data(|d| d.get_temp::<bool>(percent_id)).unwrap_or(false);
    let mut linear = ui.global_style().visuals.numeric_color_space == egui::style::NumericColorSpace::Linear;
    let mut accepted_mixed = false;
    let mut picked_recent = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_10;
        ui.horizontal(|ui| {
            // U8 / F / % / Hex (#10201). Hex and % are mutually exclusive; F shares egui's global
            // numeric colour space with every other picker.
            let mut selected = if hexadecimal {
                3
            } else if percent {
                2
            } else {
                usize::from(linear)
            };
            ui.allocate_ui(vec2(t.control_height * 5.0 + t.space_12, t.control_height), |ui| {
                super::segmented(
                    ui,
                    &mut selected,
                    &["U8", "F", "%", i18n::tr("color.hex")],
                    super::Segments { compact: true, primary: true, ..Default::default() },
                );
                let rect = ui.min_rect();
                let tips = [("color.gamma", "U8"), ("color.linear", "F"), ("color.percent_help", "Percent"), ("color.hex_help", "Hex")];
                for (index, (key, name)) in tips.iter().enumerate() {
                    let rect =
                        egui::Rect::from_min_size(rect.min + vec2(index as f32 * rect.width() / 4.0, 0.0), vec2(rect.width() / 4.0, rect.height()));
                    ui.interact(rect, ui.id().with(("format_tip", index)), Sense::hover()).on_hover_text(i18n::tr(key));
                    scenario::publish_handle(&format!("color:format:{name}"), rect.center());
                }
            });
            hexadecimal = selected == 3;
            percent = selected == 2;
            if selected <= 1 && (selected == 1) != linear {
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
                    } else if percent {
                        let pct = |c: u8| (f64::from(c) / 255.0 * 100.0).round();
                        format!("{:.0}%, {:.0}%, {:.0}%", pct(r), pct(g), pct(b))
                    } else if linear {
                        format!("{lr:.03}, {lg:.03}, {lb:.03}")
                    } else {
                        format!("{r}, {g}, {b}")
                    });
                }
            });
        });
        let mut channels = if percent {
            rgb.map(|c| f64::from(c) / 255.0 * 100.0)
        } else if linear {
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
            let spin = if percent {
                Spin { max: 100.0, step: 1.0, speed: 0.5, decimals: 0 }
            } else if linear {
                Spin { max: 1.0, step: 0.01, speed: 0.003, decimals: 3 }
            } else {
                Spin { max: 255.0, step: 1.0, speed: 0.5, decimals: 0 }
            };
            ui.columns(3, |columns| {
                // Named like H, S and V: "R" alone doesn't say red in every language (#10201).
                for ((ui, channel), (name, tip)) in
                    columns.iter_mut().zip(channels.iter_mut()).zip([("R", "color.red"), ("G", "color.green"), ("B", "color.blue")])
                {
                    edited |= spin_channel(ui, name, Some(i18n::tr(tip)), channel, spin);
                }
            });
        }
        if edited {
            *rgb = if percent {
                channels.map(|v| ((v / 100.0) * 255.0).round().clamp(0.0, 255.0) as u8)
            } else if linear {
                let [r, g, b] = channels;
                let color = Color32::from(egui::Rgba::from_rgb(r as f32, g as f32, b as f32));
                [color.r(), color.g(), color.b()]
            } else {
                channels.map(|v| v.round() as u8)
            };
            hsv = crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0)).into();
        }
        // Only graphical edits convert HSV back to RGB. Typed byte values stay exact.
        let mut graphical_edit = false;
        // Hue stays in degrees. Saturation and value use the same number format as RGB in U8, F
        // and % (#10201); Hex keeps the familiar 0–100 percent scale for S/V.
        let sv = if hexadecimal || percent {
            Spin { max: 100.0, step: 1.0, speed: 0.5, decimals: 0 }
        } else if linear {
            Spin { max: 1.0, step: 0.01, speed: 0.003, decimals: 3 }
        } else {
            Spin { max: 255.0, step: 1.0, speed: 0.5, decimals: 0 }
        };
        let h_spin = Spin { max: 360.0, step: 1.0, speed: 0.5, decimals: 0 };
        let hsv_meta: [(&str, &str, Spin); 3] = [("H", "color.hue", h_spin), ("S", "color.saturation", sv), ("V", "color.value", sv)];
        let mut hsv_fields = [hsv.h, hsv.s, hsv.v].iter().zip(hsv_meta).map(|(v, (_, _, spin))| f64::from(*v) * spin.max).collect::<Vec<_>>();
        let mut hsv_edited = false;
        ui.columns(3, |columns| {
            for ((ui, value), (name, tip, spin)) in columns.iter_mut().zip(hsv_fields.iter_mut()).zip(hsv_meta) {
                hsv_edited |= spin_channel(ui, name, Some(i18n::tr(tip)), value, spin);
            }
        });
        if hsv_edited {
            let unit = |at: usize| {
                let (value, max) = (hsv_fields.get(at).copied().unwrap_or(0.0), hsv_meta.get(at).map_or(1.0, |h| h.2.max));
                if value.is_finite() { (value / max).clamp(0.0, 1.0) as f32 } else { 0.0 }
            };
            (hsv.h, hsv.s, hsv.v) = (unit(0), unit(1), unit(2));
            graphical_edit = true;
        }
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
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), i18n::tr("color.saturation_value")));
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
        if let Some(picked) = recent_row(ui, width) {
            *rgb = picked;
            picked_recent = true;
            accepted_mixed = mixed;
            hsv = crate::theme::srgb(rgb.map(|c| f32::from(c) / 255.0)).into();
            hex.set(*rgb, opacity.as_deref().copied());
        }
    });
    // A colour joins the recent list once an edit is finished: a drag or a held step button is
    // released, a typed value is entered. Every picker shares the list (`crate::recent_colors`).
    // While the popover stays open, fine-tuning replaces the colour it added rather than filling
    // the list with each step; a colour taken from the list starts a new entry.
    let session_id = id.with("recent_session");
    let pass = ui.ctx().cumulative_pass_nr();
    let mut session = ui.ctx().data(|d| d.get_temp::<RecentSession>(session_id)).unwrap_or_default();
    if session.pass.saturating_add(1) < pass {
        session = RecentSession::default();
    }
    session.pass = pass;
    session.pending |= *rgb != before_rgb;
    if session.pending && !ui.input(|i| i.pointer.any_down()) {
        let replacing = if picked_recent { None } else { session.entry };
        remember_color(ui.ctx(), *rgb, replacing);
        session.entry = if picked_recent { None } else { Some(*rgb) };
        session.pending = false;
    }
    ui.ctx().data_mut(|d| d.insert_temp(session_id, session));
    hex.sync(*rgb, opacity.as_deref().copied());
    ui.ctx().data_mut(|d| d.insert_temp(id, (*rgb, hsv)));
    ui.ctx().data_mut(|d| {
        d.insert_temp(hex_id, hex);
        d.insert_temp(format_id, hexadecimal);
        d.insert_temp(percent_id, percent);
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

/// How a channel number edits: its range `0..=max`, the step of its step buttons, the drag
/// speed and the decimals shown.
#[derive(Clone, Copy)]
struct Spin {
    max: f64,
    step: f64,
    speed: f64,
    decimals: usize,
}

/// One channel of the popover: its name above a drag field (drag, or click and type) and a pair
/// of step buttons for fine adjustment. Returns whether the value changed.
/// Tokens: space_2/3/14, control_height, font_small, form_dim.
fn spin_channel(ui: &mut Ui, name: &str, tip: Option<&str>, value: &mut f64, spin: Spin) -> bool {
    let t = Tokens::get();
    let Spin { max, step, speed, decimals } = spin;
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_3;
        let label = ui.add(egui::Label::new(egui::RichText::new(name).size(t.font_small).color(t.form_dim)));
        if let Some(tip) = tip {
            label.on_hover_text(tip);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_2;
            let width = dimension(ui.available_width() - t.space_14 - t.space_2);
            let response = ui.add_sized(
                vec2(width, t.control_height),
                egui::DragValue::new(value)
                    .range(0.0..=max)
                    .speed(speed)
                    .custom_parser(crate::prefs::parse_number)
                    .custom_formatter(move |v, _| format!("{v:.decimals$}")),
            );
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::drag_value(ui.is_enabled(), *value);
                info.label = Some(tip.unwrap_or(name).into());
                info
            });
            publish(&format!("color:channel:{name}"), &response);
            let response = match tip {
                Some(tip) => response.on_hover_text(tip),
                None => response,
            };
            changed |= response.changed();
            let steps = spin_buttons(ui, name, t.control_height);
            if steps != 0 {
                let current = if value.is_finite() { *value } else { 0.0 };
                *value = (current + f64::from(steps) * step).clamp(0.0, max);
                changed = true;
            }
        });
    });
    changed
}

/// Seconds a step button is held before it repeats, then between repeats.
const REPEAT_DELAY: f64 = 0.4;
const REPEAT_EVERY: f64 = 0.05;

/// Step buttons stacked beside a number: a press steps once, holding repeats. Returns this
/// frame's steps (+ up, − down). Tokens: space_14, control_radius, hover, form_ink, form_dim.
fn spin_buttons(ui: &mut Ui, key: &str, height: f32) -> i32 {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(t.space_14, height), Sense::hover());
    let (top, bottom) = rect.split_top_bottom_at_fraction(0.5);
    let mut steps = 0;
    for (direction, half, label, end) in [(1, top, "color.step_up", "up"), (-1, bottom, "color.step_down", "down")] {
        let id = ui.id().with(("color_spin", key, end));
        // Sensing drags too keeps a long press down on the button: a press that senses clicks
        // only stops counting once it outlasts egui's longest click (0.8 s), and the repeat with
        // it, after 8 steps (#10201).
        let response = ui.interact(half, id, Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), i18n::tr(label)));
        publish(&format!("color:spin:{key}:{end}"), &response);
        let held = response.is_pointer_button_down_on();
        if response.hovered() || held {
            ui.painter().rect_filled(half, t.control_radius, t.hover);
        }
        let ink = if response.hovered() || held { t.form_ink } else { t.form_dim };
        let side = half.height().min(half.width());
        let icon = egui::Rect::from_center_size(half.center(), vec2(side, side));
        if direction > 0 {
            crate::icons::paint_ink_flipped(ui.painter(), icon, "chevron", ink);
        } else {
            crate::icons::paint_ink(ui.painter(), icon, "chevron", ink);
        }
        let repeat = id.with("repeat");
        if held {
            let now = ui.input(|i| i.time);
            let next = ui.ctx().data(|d| d.get_temp::<f64>(repeat));
            let due = match next {
                None => Some(now + REPEAT_DELAY),
                Some(next) if now >= next => Some(now + REPEAT_EVERY),
                Some(_) => None,
            };
            if let Some(due) = due {
                steps += direction;
                ui.ctx().data_mut(|d| d.insert_temp(repeat, due));
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(REPEAT_EVERY));
        } else {
            ui.ctx().data_mut(|d| d.remove::<f64>(repeat));
        }
        response.on_hover_text(i18n::tr(label));
    }
    steps
}

/// One popover's part in the recent list while it stays open (drawn on consecutive passes).
#[derive(Clone, Copy, Default)]
struct RecentSession {
    /// The pass the popover was last drawn on; a gap means it was closed and opened again.
    pass: u64,
    /// The colour changed and the edit is not finished yet (a button is still down).
    pending: bool,
    /// The colour this opening added, replaced by the next finished edit.
    entry: Option<[u8; 3]>,
}

/// The recent colours as a row of samples (empty slots outlined); returns the one clicked.
/// Tokens: space_3/4/8, font_small, form_dim, control_radius/border, border, form_ink.
fn recent_row(ui: &mut Ui, width: f32) -> Option<[u8; 3]> {
    let t = Tokens::get();
    let colors = recent_colors(ui.ctx());
    let mut picked = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_3;
        ui.add(egui::Label::new(egui::RichText::new(i18n::tr("color.recent")).size(t.font_small).color(t.form_dim)));
        let gap = t.space_4;
        let slots = MAX_RECENT_COLORS as f32;
        let side = ((width - gap * (slots - 1.0)) / slots).max(t.space_8);
        let (row, response) = ui.allocate_exact_size(vec2(width, side), Sense::hover());
        publish("color:recent", &response);
        if colors.is_empty() {
            response.on_hover_text(i18n::tr("color.recent_help"));
        }
        for slot in 0..MAX_RECENT_COLORS {
            let rect = egui::Rect::from_min_size(row.min + vec2(slot as f32 * (side + gap), 0.0), vec2(side, side));
            let Some(&[r, g, b]) = colors.get(slot) else {
                ui.painter().rect_stroke(rect, t.control_radius, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
                continue;
            };
            let response = ui.interact(rect, ui.id().with(("color_recent", slot)), Sense::click());
            let stroke = if response.hovered() { t.control_border_stroke(t.form_ink) } else { t.control_border_stroke(t.control_border) };
            ui.painter().rect(rect, t.control_radius, crate::theme::srgb([r, g, b].map(|c| f32::from(c) / 255.0)), stroke, egui::StrokeKind::Inside);
            let hex = format!("#{r:02x}{g:02x}{b:02x}");
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &hex));
            publish(&format!("color:recent:{slot}"), &response);
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&hex);
            if response.clicked() {
                picked = Some([r, g, b]);
            }
        }
    });
    picked
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
