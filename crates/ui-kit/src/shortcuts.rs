//! Shared chord keycaps and the search box of Preferences' title bar and the Browser's header.
use crate::icons;
use egui::{FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

fn chord_parts(chord: &str) -> Vec<&str> {
    // A literal plus key follows the separator, as in Ctrl++; keep its own full keycap.
    if let Some(prefix) = chord.strip_suffix('+') {
        let prefix = prefix.strip_suffix('+').unwrap_or(prefix);
        let mut parts: Vec<_> = prefix.split('+').filter(|part| !part.is_empty()).collect();
        parts.push("+");
        parts
    } else {
        chord.split('+').collect()
    }
}

/// Natural width of a complete chord in the same mono keycaps used by command search, or of
/// the host's `unassigned` text for an empty chord.
pub fn shortcut_binding_width(ui: &Ui, chord: &str, unassigned: &str) -> f32 {
    let t = Tokens::get();
    if chord.is_empty() {
        return super::ContentWidth::label(ui, unassigned) + t.space_8;
    }
    let parts = chord_parts(chord);
    parts
        .iter()
        .map(|part| ui.painter().layout_no_wrap((*part).to_owned(), FontId::monospace(t.font_small), t.form_dim).size().x + t.space_10)
        .sum::<f32>()
        + t.space_4 * parts.len().saturating_sub(1) as f32
}

/// One focusable binding, with a separate full keycap for every modifier and key; an empty
/// chord shows the host's `unassigned` text. Read-only callers keep exactly the same caps
/// without an interactive recorder target.
pub fn shortcut_binding(ui: &mut Ui, chord: &str, unassigned: &str, editable: bool) -> Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(
        vec2(shortcut_binding_width(ui, chord, unassigned), t.control_height),
        if editable { Sense::click() } else { Sense::hover() },
    );
    if editable && response.clicked() {
        response.request_focus();
    }
    let mut x = rect.left();
    if chord.is_empty() {
        let galley = ui.painter().layout_no_wrap(unassigned.to_owned(), FontId::proportional(t.font_label), t.form_dim);
        ui.painter().galley(pos2(x + t.space_4, rect.center().y - galley.size().y / 2.0), galley, t.form_dim);
    } else {
        for part in chord_parts(chord) {
            let width = ui.painter().layout_no_wrap(part.to_owned(), FontId::monospace(t.font_small), t.form_dim).size().x + t.space_10;
            super::CommandSearch::key(ui, pos2(x + width, rect.center().y), part);
            x += width + t.space_4;
        }
    }
    if response.has_focus() || (editable && response.hovered()) {
        ui.painter().rect_stroke(rect, t.radius_sm, Stroke::new(1.0, t.form_primary), egui::StrokeKind::Outside);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            if editable { egui::Role::Button } else { egui::Role::Label },
            ui.is_enabled(),
            if chord.is_empty() { unassigned } else { chord },
        )
    });
    response
}

/// The shortcut pill at the end of a search field names the key that focuses it, as this platform
/// names it: "Cmd F" on macOS, where the shortcut is Cmd+F, and "Ctrl F" elsewhere (#22641).
fn pill_chord(mac: bool) -> String {
    chord_label_for_platform("Ctrl+F", mac).replace('+', " ")
}

/// Native text editing with the same token frame and leading magnifier as command search. Its frame
/// and clear button are published as `{prefix}:search:frame` and `{prefix}:search:clear`.
pub fn preferences_search_field(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, prefix: &str) -> Response {
    let t = Tokens::get();
    let height = t.form_close_size;
    let (r, _) = ui.allocate_exact_size(vec2(super::metrics::dimension(ui.available_width()), height), Sense::hover());
    let search = SearchBox::frame(ui, r, id, t.control_radius, None, prefix);
    let show_clear = !value.is_empty();
    let clear_side = if show_clear {
        height
    } else {
        super::CommandSearch::key(ui, r.right_center() - vec2(t.space_6, 0.0), &pill_chord(cfg!(target_os = "macos"))) + t.space_12
    };
    let response = search.editor(ui, value, hint, usize::MAX, clear_side);
    if show_clear {
        let clear_rect = Rect::from_center_size(pos2(r.right() - height * 0.5, r.center().y), vec2(height, height));
        let clear = ui.push_id(id.with("clear"), |ui| super::quiet_icon_button_at(ui, clear_rect, "close", "Clear")).inner;
        solidcraft_ui_registry::publish_handle(&format!("{prefix}:search:clear"), clear_rect.center());
        if clear.clicked() {
            value.clear();
        }
    }
    response
}

/// A search box: one token frame, the magnifier at its leading end, then a frameless editor, with
/// room at its trailing end for the caller's buttons. Preferences' search and the Browser's share it.
pub struct SearchBox {
    rect: Rect,
    id: egui::Id,
    frame: Response,
    /// The magnifier, when it is a button ([`SearchBox::frame`]'s `end`).
    pub magnifier: Option<Response>,
}

impl SearchBox {
    /// Paints the frame in `rect` with `radius` corners, its border `form_primary` while the field
    /// `id` has the keyboard, and the magnifier: an icon, or a button labelled `end` (it ends the
    /// search; the caller reads [`SearchBox::magnifier`]). The frame is published as `{prefix}:search:frame`.
    /// Tokens: `control_fill, control_border, form_primary, form_dim, space_4/8/14`; quiet icon button tokens.
    pub fn frame(ui: &mut Ui, rect: Rect, id: egui::Id, radius: impl Into<egui::CornerRadius>, end: Option<&str>, prefix: &str) -> Self {
        let t = Tokens::get();
        let frame = ui.interact(rect, id.with("frame"), Sense::click());
        let focused = ui.memory(|m| m.has_focus(id));
        let border = if focused { Stroke::new(1.0, t.form_primary) } else { t.control_border_stroke(t.control_border) };
        ui.painter().rect(rect, radius, t.control_fill, border, egui::StrokeKind::Inside);
        let side = t.space_14;
        let icon = Rect::from_center_size(pos2(rect.left() + t.space_8 + side * 0.5, rect.center().y), vec2(side, side));
        let magnifier = match end {
            Some(label) => {
                let button = Rect::from_center_size(icon.center(), egui::Vec2::splat(rect.height() - 2.0 * t.space_4));
                Some(ui.push_id(id.with("magnifier"), |ui| super::quiet_icon_button_at(ui, button, "zoom", label)).inner)
            }
            None => {
                icons::paint_ink(ui.painter(), icon, "zoom", t.form_dim);
                None
            }
        };
        solidcraft_ui_registry::publish_rect(&format!("{prefix}:search:frame"), rect);
        Self { rect, id, frame, magnifier }
    }

    /// The frameless editor, from the magnifier to `trailing` short of the frame's end, holding at
    /// most `limit` characters; a click on the frame's empty part gives it the keyboard.
    /// Tokens: `font_label, form_ink, space_4/6/8/14`.
    pub fn editor(&self, ui: &mut Ui, value: &mut String, hint: &str, limit: usize, trailing: f32) -> Response {
        let t = Tokens::get();
        let r = self.rect;
        let start = r.left() + t.space_8 + t.space_14 + t.space_6;
        let editor = Rect::from_min_max(pos2(start.min(r.right()), r.top()), pos2((r.right() - trailing).max(start - t.space_6), r.bottom()));
        let response = ui
            .scope_builder(egui::UiBuilder::new().max_rect(editor), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(editor));
                let mut response = ui.add_sized(
                    editor.size(),
                    // As tall as the frame, not a field's height: the editor stays inside its box.
                    super::fields::text_input(egui::TextEdit::singleline(value))
                        .min_size(vec2(0.0, editor.height()))
                        .id(self.id)
                        .char_limit(limit)
                        .hint_text(solidcraft_ui_text::tr_label(hint))
                        .font(FontId::proportional(t.font_label))
                        .text_color(t.form_ink)
                        .margin(vec2(t.space_4, 0.0))
                        .clip_text(true)
                        .frame(egui::Frame::NONE),
                );
                super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE.limited(limit));
                response
            })
            .inner;
        if self.frame.clicked() {
            response.request_focus();
        }
        super::layout::publish(ui, &response, egui::Role::TextInput);
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::TextInput, ui.is_enabled(), solidcraft_ui_text::tr_label(hint));
            info.current_text_value = Some(value.clone());
            info
        });
        response
    }
}

/// A stored chord as this platform names it: on macOS Ctrl is the primary modifier, shown as
/// Cmd, and Alt is Option; elsewhere the chord is unchanged. A recorded macOS Control+Command
/// chord (`Ctrl+Cmd+…`) names both keys explicitly, so its Ctrl stays.
pub fn chord_label(key: &str) -> String {
    chord_label_for_platform(key, cfg!(target_os = "macos"))
}

pub fn chord_label_for_platform(key: &str, mac: bool) -> String {
    let both = key.starts_with("Ctrl+Cmd+");
    key.split('+')
        .map(|part| match (mac, part) {
            (true, "Ctrl") if !both => "Cmd",
            (true, "Alt") => "Option",
            _ => part,
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_field_pill_names_the_shortcut_as_the_platform_names_it() {
        // #22641: the pill was a literal "Ctrl F" even on macOS, where the shortcut is Cmd+F.
        assert_eq!(pill_chord(true), "Cmd F");
        assert_eq!(pill_chord(false), "Ctrl F");
    }

    #[test]
    fn an_empty_search_field_draws_the_pill_with_that_label() {
        let ctx = crate::test_support::context();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 100.0))), ..Default::default() };
        let mut value = String::new();
        let output = crate::test_support::raw_frame(&ctx, input, |ui| {
            preferences_search_field(ui, egui::Id::unique("prefs_search"), &mut value, "Search settings", "prefs");
        });
        let expected = pill_chord(cfg!(target_os = "macos"));
        assert!(
            output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == expected)),
            "the field ends with a pill naming the shortcut: {expected}"
        );
    }
}
