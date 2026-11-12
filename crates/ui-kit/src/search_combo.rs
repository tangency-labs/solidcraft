//! `SearchCombo`: the picker for every list that can grow without limit (bodies, parameters,
//! materials, fonts, presets…). Its popup opens on a focused search box; typing filters the rows
//! (case-insensitive, anywhere in the label), Up/Down move the highlight, Enter picks it and Esc
//! closes. Rows come from a lazy [`ComboSource`] and only those in view are laid out, so a list of
//! ten thousand opens at once. See `docs/ui-kit.md`.
use super::fields::{compact, dropdown_arrow, search_row, text_input};
use super::layout::{label, publish};
use super::metrics::{dimension, row_height};
use crate::BackingTooltipExt as _;
use egui::{Ui, vec2};
use solidcraft_theme::Tokens;
use std::borrow::Cow;

/// Longest search text the box takes.
const TEXT_CAP: usize = 256;
/// Characters of a label shown on a row; a longer one shows whole on hover.
const LABEL_CAP: usize = 64;
/// Rows the popup makes room for at most before it scrolls.
const ROWS_SHOWN: usize = 10;

/// The options of a [`SearchCombo`], read when needed: the rows in view, and every label only
/// while something is typed.
pub trait ComboSource {
    /// How many options there are.
    fn count(&self) -> usize;
    /// The text of option `index`; it is shown and searched.
    fn label(&self, index: usize) -> Cow<'_, str>;
    /// The scenario name of option `index` under the picker's handles; its label by default.
    fn key(&self, index: usize) -> Cow<'_, str> {
        self.label(index)
    }
}

impl<S: AsRef<str>> ComboSource for [S] {
    fn count(&self) -> usize {
        self.len()
    }
    fn label(&self, index: usize) -> Cow<'_, str> {
        Cow::Borrowed(self.get(index).map_or("", AsRef::as_ref))
    }
}

impl<S: AsRef<str>> ComboSource for Vec<S> {
    fn count(&self) -> usize {
        self.len()
    }
    fn label(&self, index: usize) -> Cow<'_, str> {
        self.as_slice().label(index)
    }
}

/// A [`ComboSource`] from a count and a function giving each label: `ComboFn::new(n, |i| ...)`.
pub struct ComboFn<'s, F> {
    count: usize,
    label: F,
    borrows: std::marker::PhantomData<&'s ()>,
}

impl<'s, F: Fn(usize) -> Cow<'s, str>> ComboFn<'s, F> {
    pub fn new(count: usize, label: F) -> Self {
        Self { count, label, borrows: std::marker::PhantomData }
    }
}

impl<'s, F: Fn(usize) -> Cow<'s, str>> ComboSource for ComboFn<'s, F> {
    fn count(&self) -> usize {
        self.count
    }
    fn label(&self, index: usize) -> Cow<'_, str> {
        (self.label)(index)
    }
}

/// What a [`SearchCombo`] shows.
pub struct SearchCombo<'a, S: ComboSource + ?Sized> {
    /// ComboBox id salt; it also keys the popup's state, so one per picker in the window.
    pub id: &'a str,
    /// Scenario names: the control `{handles}`, its box `{handles}:search` and its rows
    /// `{handles}:{key}`. Empty: `dialog:{row label}`, as the plain dropdown names its rows.
    pub handles: &'a str,
    /// The text on the closed control.
    pub current: &'a str,
    /// The option to highlight as chosen.
    pub selected: Option<usize>,
    /// The search box's empty-text hint; empty for the catalog's "Search".
    pub hint: &'a str,
    pub source: &'a S,
    pub enabled: bool,
}

impl<'a, S: ComboSource + ?Sized> SearchCombo<'a, S> {
    /// A picker over `source` with `current` on the closed control and the rest defaulted.
    pub fn new(id: &'a str, current: &'a str, source: &'a S) -> Self {
        Self { id, handles: "", current, selected: None, hint: "", source, enabled: true }
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub fn handles(mut self, handles: &'a str) -> Self {
        self.handles = handles;
        self
    }

    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draws the control; the index picked this frame, if any, and the control's response.
    pub fn show(self, ui: &mut Ui) -> egui::InnerResponse<Option<usize>> {
        search_combo(ui, &self)
    }
}

/// The open popup's state, kept between frames and dropped when it closes.
#[derive(Clone, Default)]
struct Open {
    filter: String,
    /// The options that match `filter`, for the source length they were found in; `None` while
    /// nothing is typed, when every option is listed.
    matches: Option<(usize, Vec<usize>)>,
    /// The highlighted row, a position in the listed rows.
    highlight: Option<usize>,
    /// Set by a key or by the opening: scroll the highlight into view this frame.
    reveal: bool,
    focused: bool,
    height: Option<f32>,
}

/// Whether `label` contains `query`, already lowercase, in any case.
pub fn combo_matches(label: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let lower: String = label.chars().take(TEXT_CAP).flat_map(char::to_lowercase).collect();
    lower.contains(query)
}

/// The indices of the options in `source` whose label contains `filter`, in any case, in order.
pub fn combo_filter<S: ComboSource + ?Sized>(source: &S, filter: &str) -> Vec<usize> {
    let query: String = filter.trim().chars().flat_map(char::to_lowercase).collect();
    (0..source.count()).filter(|&index| combo_matches(&source.label(index), &query)).collect()
}

/// The highlight after `down` presses of Down less `up` of Up over `rows` rows: clamped to the
/// list, starting from the first row (Down) or the last (Up) when there is none.
pub fn combo_step(highlight: Option<usize>, rows: usize, down: usize, up: usize) -> Option<usize> {
    if rows == 0 {
        return None;
    }
    let last = rows - 1;
    match highlight {
        None if down == 0 && up == 0 => None,
        None if down >= up => Some((down - up).saturating_sub(1).min(last)),
        None => Some(last.saturating_sub(up - down - 1)),
        Some(at) => Some((at.min(last) + down).saturating_sub(up).min(last)),
    }
}

fn search_combo<S: ComboSource + ?Sized>(ui: &mut Ui, p: &SearchCombo<'_, S>) -> egui::InnerResponse<Option<usize>> {
    super::settings_search::note_current(ui, p.current);
    if super::settings_search::querying(ui)
        && (!ui.is_visible() || (super::settings_search::suppress_chrome(ui) && !super::settings_search::texts_hit(ui, &[p.current])))
    {
        return egui::InnerResponse::new(None, super::settings_search::hidden_response(ui));
    }
    let handles = if p.handles.is_empty() { format!("dialog:{}", label(ui)) } else { p.handles.to_owned() };
    let state = egui::Id::unique(p.id).with("search_combo");
    let mut picked = None;
    let response = ui
        .add_enabled_ui(p.enabled, |ui| {
            let combo = egui::ComboBox::from_id_salt(p.id)
                .width(dimension(ui.available_width()))
                .height(Tokens::get().space_24 * 20.0)
                .selected_text(p.current)
                .truncate()
                .icon(dropdown_arrow)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
            let width = ui.available_width();
            let response = super::container::show_combo(ui, ui.make_persistent_id(egui::IdSalt::new(p.id)), combo, |ui| {
                picked = popup(ui, p, state, &handles, width);
            })
            .response;
            if !egui::ComboBox::is_open(ui.ctx(), response.id) {
                ui.data_mut(|data| data.remove::<Open>(state));
            }
            response
        })
        .inner;
    publish(ui, &response, egui::Role::ComboBox);
    solidcraft_ui_registry::publish_target(&handles, response.rect);
    egui::InnerResponse::new(picked, response)
}

/// What the popup shows while it is open; the pick, if one was made this frame.
fn popup<S: ComboSource + ?Sized>(ui: &mut Ui, p: &SearchCombo<'_, S>, state: egui::Id, handles: &str, control_width: f32) -> Option<usize> {
    let t = Tokens::get();
    let mut open: Open = ui.data(|data| data.get_temp(state)).unwrap_or_else(|| Open { highlight: p.selected, reveal: true, ..Open::default() });
    // Keys the box would otherwise take: Esc would also cancel a dialog around the popup.
    let (escape, enter, down, up) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
        )
    });
    if escape {
        ui.close();
    }
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    ui.spacing_mut().button_padding.x = t.form_inset;
    let most = (ui.ctx().content_rect().width() - t.space_24 * 2.0).max(0.0);
    ui.set_min_width(dimension(control_width.max(t.form_card_width).min(most)));
    let hint = if p.hint.is_empty() { solidcraft_ui_text::tr_label("Search") } else { p.hint };
    let search = ui.add(
        text_input(egui::TextEdit::singleline(&mut open.filter))
            .id(state.with("search"))
            .char_limit(TEXT_CAP)
            .hint_text(hint)
            .background_color(t.input_fill())
            .desired_width(f32::INFINITY),
    );
    t.input_bevel(ui, &search);
    solidcraft_ui_registry::publish_target(&format!("{handles}:search"), search.rect);
    // Asked until it has it: over a modal window egui drops the request of the opening's first frame.
    if !open.focused {
        if search.has_focus() {
            open.focused = true;
        } else {
            search.request_focus();
            ui.ctx().request_repaint();
        }
    }
    if search.has_focus() {
        super::settings_search::note_key_sink(ui.ctx());
    }
    let count = p.source.count();
    if search.changed() || open.matches.as_ref().is_some_and(|(n, _)| *n != count) {
        open.matches = (!open.filter.trim().is_empty()).then(|| (count, combo_filter(p.source, &open.filter)));
        open.highlight = open.matches.as_ref().map_or(p.selected, |_| Some(0));
        open.reveal = true;
    }
    let listed = open.matches.as_ref().map_or(count, |(_, m)| m.len());
    let option = |row: usize| open.matches.as_ref().map_or(Some(row), |(_, m)| m.get(row).copied());
    if down + up > 0 {
        open.highlight = combo_step(open.highlight, listed, down, up);
        open.reveal = true;
    }
    let mut picked = None;
    if enter {
        // Enter picks the highlight, else the first row; with nothing listed it keeps the popup.
        let row = open.highlight.filter(|&h| h < listed).or((listed > 0).then_some(0));
        match row.and_then(option) {
            Some(index) => picked = Some(index),
            None if open.filter.trim().is_empty() => ui.close(),
            None => {}
        }
    }
    let gap = ui.spacing().item_spacing.y;
    let row_h = row_height();
    let step = row_h + gap;
    // A popup can't grow once open: its list keeps the height it opened with.
    let height = *open.height.get_or_insert_with(|| {
        let rows = count.clamp(1, ROWS_SHOWN) as f32;
        (rows * step).min(ui.ctx().content_rect().height() - t.space_24 * 4.0).max(step)
    });
    if listed == 0 {
        ui.allocate_ui(vec2(dimension(ui.available_width()), height), |ui| {
            ui.set_min_height(height);
            ui.label(egui::RichText::new(solidcraft_ui_text::tr("ui.search_combo_empty")).size(t.font_small).color(t.text_dim));
        });
    } else {
        let mut scroll =
            egui::ScrollArea::vertical().id_salt(state.with("list")).max_height(height).min_scrolled_height(height).auto_shrink([false, false]);
        if open.reveal
            && let Some(h) = open.highlight
        {
            let offset: f32 = ui.data(|data| data.get_temp(state.with("offset"))).unwrap_or(0.0);
            let top = h as f32 * step;
            let wanted = if top < offset {
                Some(top)
            } else if top + row_h > offset + height {
                Some(top + row_h - height)
            } else {
                None
            };
            if let Some(wanted) = wanted {
                scroll = scroll.vertical_scroll_offset(wanted.max(0.0));
            }
        }
        open.reveal = false;
        let shown = scroll.show_rows(ui, row_h, listed, |ui, range| {
            for row in range {
                let Some(index) = option(row) else { continue };
                let text = p.source.label(index);
                let chosen = p.selected == Some(index);
                // The keyboard's row reads as hovered, under its text; the chosen one as selected.
                let backing = ui.painter().add(egui::Shape::Noop);
                let response = search_row(ui, &compact(&text, LABEL_CAP), chosen);
                if open.highlight == Some(row) && !chosen {
                    ui.painter().set(backing, egui::Shape::rect_filled(response.rect, t.radius_sm, t.hover));
                }
                solidcraft_ui_registry::publish_target(&format!("{handles}:{}", p.source.key(index)), response.rect);
                let response = if text.chars().count() > LABEL_CAP { response.backing_on_hover_text(text.into_owned()) } else { response };
                if response.clicked() {
                    picked = Some(index);
                }
            }
        });
        ui.data_mut(|data| data.insert_temp(state.with("offset"), shown.state.clamped_offset().y));
    }
    if picked.is_some() {
        ui.close();
    }
    ui.data_mut(|data| data.insert_temp(state, open));
    picked
}

#[cfg(test)]
#[path = "search_combo_tests.rs"]
mod tests;
