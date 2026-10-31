//! Metrics elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use super::layout::FORM;
use crate::theme::Tokens;
use egui::Ui;

/// Row height for controls. Tokens: `control_height`.
pub(crate) fn row_height() -> f32 {
    Tokens::get().control_height
}

/// Feature heading height. Tokens: `form_header_height`.
pub(crate) fn header_height() -> f32 {
    Tokens::get().form_header_height
}

/// Sanitize an input dimension. Tokens: `none`.
pub(super) fn dimension(value: f32) -> f32 {
    if value.is_finite() { value.max(0.0) } else { 0.0 }
}

/// A table column's header-width basis, as a multiple of `form_label_width`.
pub(super) const TABLE_COLUMN_BASIS: f32 = 3.2;

/// Share of a row given to the name in `label_actions`.
pub(super) const LABEL_ACTIONS_SHARE: f32 = 0.45;

/// Widest a label column may grow, as a multiple of `form_label_width`.
const LABEL_COLUMN_MAX: f32 = 1.6;

/// A label as drawn: the leading indent kept, the text itself translated (a nested row such as
/// `"  Of"` lines up under the one above and still translates). Measuring (`label_width`) and the
/// row label (`sized_row`) both prepare their text with this, so a label is translated exactly
/// once and the label column and the captions line up on the same text.
pub(super) fn indented_label(text: &str) -> String {
    let indent = text.len() - text.trim_start().len();
    format!("{}{}", &text[..indent], crate::i18n::tr_label(text.trim_start()))
}

/// Measured width of a label in the form label font. Tokens: `font_label, dialog_label`.
pub(super) fn label_width(ui: &Ui, text: &str) -> f32 {
    let t = Tokens::get();
    ui.painter().layout_no_wrap(indented_label(text), egui::FontId::proportional(t.font_label), t.dialog_label).size().x.ceil()
}

/// Minimum content widths for table controls, including their token/native control metrics.
pub(crate) struct ContentWidth;

impl ContentWidth {
    pub(crate) fn label(ui: &Ui, text: &str) -> f32 {
        label_width(ui, text)
    }

    /// Width of a strong configuration name, using the same font as its table cell.
    pub(crate) fn name(ui: &Ui, text: &str) -> f32 {
        let t = Tokens::get();
        ui.painter().layout_no_wrap(text.to_owned(), crate::fonts::semibold(t.font_label), t.form_ink).size().x.ceil()
    }

    pub(crate) fn button(ui: &Ui, label: &str) -> f32 {
        label_width(ui, label) + 2.0 * ui.spacing().button_padding.x
    }

    pub(crate) fn dropdown(ui: &Ui, options: &[&str]) -> f32 {
        let text = options.iter().map(|text| label_width(ui, text)).fold(0.0, f32::max);
        text + 2.0 * ui.spacing().button_padding.x + ui.spacing().icon_width + ui.spacing().icon_spacing
    }
}

/// Width of a table header column, measured directly: it never touches the form's shared label-column state.
/// Tokens: `form_label_width, font_label, dialog_label, space_4`.
pub(super) fn header_column(ui: &Ui, text: &str) -> f32 {
    let t = Tokens::get();
    let base = t.form_label_width;
    (label_width(ui, text) + t.space_4).clamp(base, base * LABEL_COLUMN_MAX).min(base * TABLE_COLUMN_BASIS * 0.5)
}

/// Allocate exactly one control-height row. Long labels elide; controls cannot widen the card.
/// The design's label column is `form_label_width` wide; a dialog with longer labels widens the
/// column for all of its rows (up to 1.6 times that), so the controls still line up and labels
/// are not elided. The widest label of the last pass sizes the column, so it settles within
/// two frames and shrinks again when the long rows go away.
/// Shared label width, capped at 1.6x. Tokens: `form_label_width, font_label, dialog_label, space_4`.
pub(super) fn label_column(ui: &Ui, text: &str, row_width: f32) -> f32 {
    if let Some(width) = ui.stack().iter().find_map(|node| node.tags().get_downcast::<f32>(super::layout::FIXED_LABEL)) {
        return width.min(row_width * 0.6);
    }
    let t = Tokens::get();
    // Wide manager cards use the approved 104 pt label column; compact cards retain room
    // for their existing controls. Both sizes derive from the same form/spacing tokens.
    let base = if row_width >= t.form_card_width + t.space_24 * 2.0 { t.form_label_width + t.space_24 + t.space_10 } else { t.form_label_width };
    let natural = label_width(ui, text);
    let widest = match ui.stack().iter().find(|node| node.tags().contains(FORM)).map(|node| node.unique_id) {
        Some(form) => {
            let key = form.with("label_column");
            let pass = ui.ctx().cumulative_pass_nr();
            let (seen, current, previous) = ui.ctx().data(|d| d.get_temp::<(u64, f32, f32)>(key)).unwrap_or((pass, 0.0, 0.0));
            let (current, previous) = if seen == pass {
                (current, previous)
            } else if seen + 1 == pass {
                (0.0, current)
            } else {
                (0.0, 0.0)
            };
            let current = current.max(natural);
            ui.ctx().data_mut(|d| d.insert_temp(key, (pass, current, previous)));
            current.max(previous)
        }
        None => natural,
    };
    (widest + t.space_4).clamp(base, base * LABEL_COLUMN_MAX).min(row_width * 0.5)
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;

/// Table rows leave four points above/below the shared control. Tokens: control_height, space_8.
pub(crate) fn table_row_height() -> f32 {
    Tokens::get().control_height + Tokens::get().space_8
}

/// Label/control gutter for the same responsive form grid as label_column.
pub(super) fn label_gap(ui: &Ui, width: f32) -> f32 {
    let t = Tokens::get();
    if ui.stack().iter().any(|node| node.tags().contains(super::layout::FIXED_LABEL)) {
        t.space_14
    } else if width >= t.form_card_width + t.space_24 * 2.0 {
        t.space_10
    } else {
        t.space_8
    }
}

#[cfg(test)]
mod label_tests {
    use super::indented_label;

    /// `label_width` and `sized_row` share `indented_label`, so a label is translated once. German
    /// `Profiles` is `Profile`, which is itself an English key that would translate again to
    /// `Profil`; the row must show `Profile`, not the second translation.
    #[test]
    fn a_label_is_translated_once() {
        let _de = crate::i18n::scope("de");
        assert_eq!(indented_label("Profiles"), "Profile");
        assert_eq!(indented_label("  Profiles"), "  Profile");
        assert_eq!(indented_label("Profile"), "Profil");
    }
}
