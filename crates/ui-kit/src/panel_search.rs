//! A panel's search box (#19042): the magnifier, the field, an optional filter and the ×, in one
//! token frame inset from the panel's edge. The panel host decides where it goes: in the header, in
//! the title's place, or as the body's first row when the header holds tabs. The panel keeps the
//! query and what it narrows; the kit draws the box and names no host text.
use egui::{Rect, Response, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

use crate::{BackingTooltipExt as _, SearchBox};

/// The host's words for the box, already translated.
#[derive(Clone, Copy)]
pub struct PanelSearchText<'a> {
    /// In the empty field.
    pub hint: &'a str,
    /// The field's tooltip.
    pub tip: &'a str,
    /// The magnifier's label: it ends the search.
    pub end: &'a str,
    /// The ×'s label: it ends the search too.
    pub clear: &'a str,
}

/// The box's filter: an icon button whose list the panel fills, with a badge counting the `picked`
/// choices. `handle` names it under the box: `{prefix}:search:{handle}`.
#[derive(Clone, Copy)]
pub struct PanelSearchFilter<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    pub handle: &'a str,
    pub picked: usize,
}

/// The box, built for one frame.
pub struct PanelSearch<'a> {
    prefix: &'a str,
    text: PanelSearchText<'a>,
    limit: usize,
    focus: bool,
    filter: Option<PanelSearchFilter<'a>>,
}

/// The box with its field drawn; [`PanelSearchField::finish`] draws the buttons after the field.
#[must_use = "finish draws the filter and the ×, and says whether the search ended"]
pub struct PanelSearchField<'a> {
    pub field: Response,
    prefix: &'a str,
    frame: Rect,
    clear: &'a str,
    filter: Option<PanelSearchFilter<'a>>,
    ended: bool,
}

impl<'a> PanelSearch<'a> {
    /// The box of the panel `prefix` (its instance id, which also names its handles), with the
    /// field holding at most `limit` characters.
    pub fn new(prefix: &'a str, text: PanelSearchText<'a>, limit: usize) -> Self {
        Self { prefix, text, limit, focus: false, filter: None }
    }

    /// Gives the field the keyboard this frame.
    pub fn focus(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    /// A filter between the field and the ×.
    pub fn filter(mut self, filter: PanelSearchFilter<'a>) -> Self {
        self.filter = Some(filter);
        self
    }

    /// The field's id. It comes from the panel, not from the parent `Ui`, so the field keeps the
    /// keyboard and its caret when the box moves between the header and the body.
    pub fn id(prefix: &str) -> egui::Id {
        egui::Id::unique(("panel_search", prefix))
    }

    /// The narrowest the box gets: room for five buttons and four gaps with a filter (the
    /// magnifier, two for the field, the filter and the ×), one of each fewer without; 136 and 108
    /// at the default tokens. A header narrower than this moves its actions to a row below.
    pub fn min_width(filter: bool) -> f32 {
        let t = Tokens::get();
        let buttons = if filter { 5.0 } else { 4.0 };
        t.space_24 * buttons + t.space_4 * (buttons - 1.0)
    }

    /// The height of the box's row when it is the body's first row (the header holds tabs).
    pub fn row_height() -> f32 {
        crate::row_height() + Tokens::get().space_8
    }

    /// Draws the frame across `rect`, centred in its height, with the magnifier and the field.
    /// Publishes `{prefix}:search:bar` (`rect`), `{prefix}:search:frame`, `{prefix}:search` (the
    /// field) and `{prefix}:magnifier`. Tokens: `space_2/6/24, form_close_size, radius_sm`;
    /// [`SearchBox`]'s.
    pub fn show(self, ui: &mut Ui, rect: Rect, query: &mut String) -> PanelSearchField<'a> {
        let t = Tokens::get();
        let prefix = self.prefix;
        solidcraft_ui_registry::publish_rect(&format!("{prefix}:search:bar"), rect);
        let frame = Rect::from_min_max(
            pos2(rect.left() + t.space_6, rect.center().y - t.form_close_size / 2.0),
            pos2(rect.right().max(rect.left() + t.space_6), rect.center().y + t.form_close_size / 2.0),
        );
        let search = SearchBox::frame(ui, frame, Self::id(prefix), t.radius_sm, Some(self.text.end), prefix);
        let buttons = if self.filter.is_some() { 2.0 } else { 1.0 };
        let field = search.editor(ui, query, self.text.hint, self.limit, buttons * t.space_24 + t.space_2);
        solidcraft_ui_registry::publish_handle(&format!("{prefix}:search"), field.rect.center());
        solidcraft_ui_registry::publish_rect(&format!("{prefix}:search"), field.rect);
        if self.focus {
            field.request_focus();
        }
        // Esc in the field ends the search (egui gives up the focus on Esc).
        let escaped = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
        let field = field.backing_on_hover_text(self.text.tip);
        if let Some(m) = &search.magnifier {
            solidcraft_ui_registry::publish_handle(&format!("{prefix}:magnifier"), m.rect.center());
        }
        let magnifier = search.magnifier.as_ref().is_some_and(Response::clicked);
        PanelSearchField { field, prefix, frame, clear: self.text.clear, filter: self.filter, ended: escaped || magnifier }
    }
}

impl PanelSearchField<'_> {
    /// Draws the filter, its list from `add` while open, then the ×, in the order the keyboard
    /// visits them, and says whether the search ended this frame: the magnifier, the × or Esc in the
    /// field. Publishes `{prefix}:search:clear`, `{prefix}:search:{handle}` and, when something is
    /// picked, `{prefix}:search:{handle}:badge`. Tokens: `space_2/24`; icon button tokens.
    pub fn finish(self, ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> bool {
        let t = Tokens::get();
        let (prefix, frame, button) = (self.prefix, self.frame, t.space_24);
        // The buttons at the trailing end, counted from it: the × first, then the filter.
        let at = |n: f32| Rect::from_center_size(pos2(frame.right() - t.space_2 - button * (n - 0.5), frame.center().y), vec2(button, button));
        if let Some(PanelSearchFilter { icon, label, handle, picked }) = self.filter {
            let filter = crate::filter_button_at(ui, at(2.0), icon, label, picked, add);
            let name = format!("{prefix}:search:{handle}");
            solidcraft_ui_registry::publish_handle(&name, filter.response.rect.center());
            solidcraft_ui_registry::publish_rect(&name, filter.response.rect);
            if let Some(badge) = filter.badge {
                solidcraft_ui_registry::publish_handle(&format!("{name}:badge"), badge.center());
            }
        }
        let clear = crate::icon_button_at(ui, at(1.0), "close", self.clear);
        solidcraft_ui_registry::publish_handle(&format!("{prefix}:search:clear"), clear.rect.center());
        self.ended || clear.clicked()
    }
}

#[cfg(test)]
#[path = "panel_search_tests.rs"]
mod tests;
