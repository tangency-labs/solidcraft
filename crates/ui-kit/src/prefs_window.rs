//! The Preferences window: the page list (icons, groups), a search over every page that keeps and
//! outlines the settings that match, Restore Defaults for the page shown, and OK / Cancel / Apply
//! over a snapshot of the host's settings. The pages and the settings are the host's: the window
//! draws a page through its [`HostPage::draw`], resets it through [`HostPage::defaults`], and takes
//! and puts back the snapshot through [`Host`].
use crate::{BackingTooltipExt as _, Icon, settings_search};
use egui::Ui;
use solidcraft_theme::Tokens;

/// One page of the window.
pub enum Page<S> {
    /// A page the host draws with the kit's `settings_*` helpers, so it searches, outlines and
    /// resets like every other page.
    Host(HostPage<S>),
}

impl<S> Page<S> {
    fn host(&self) -> &HostPage<S> {
        match self {
            Page::Host(page) => page,
        }
    }
}

/// A host page: its icon, label and group (text ids the window translates), the handle its tab
/// is published under, whether its body scrolls, how it draws, and what Restore Defaults does.
pub struct HostPage<S> {
    pub icon: Icon<'static>,
    pub label: &'static str,
    /// The heading of the group this page starts in the list.
    pub group: Option<&'static str>,
    pub handle: &'static str,
    pub scroll: bool,
    pub draw: fn(&mut Ui, &mut S),
    pub defaults: fn(&mut S),
}

/// The window's words, already in the user's language.
pub struct Labels<'a> {
    pub title: &'a str,
    /// The kit icon in the title bar.
    pub icon: &'a str,
    pub ok: &'a str,
    pub cancel: &'a str,
    pub apply: &'a str,
    pub search: &'a str,
    pub no_results: &'a str,
    pub restore_defaults: &'a str,
    pub restore_defaults_hint: &'a str,
}

/// How the host frames the window: its id, its words, the prefix of the names it publishes, a
/// block pinned under the page list, and a strip drawn above every page body.
pub struct Options<'a, S> {
    pub id: egui::Id,
    /// Registry names are this followed by `:` and the part: `defaults`, `search`, `search:frame`,
    /// `search:clear`, `search:current`, `revealed`, `page_body`, `window`, `footer`, `ok`, `apply`,
    /// `cancel`, `actions`.
    pub prefix: &'a str,
    pub labels: Labels<'a>,
    pub sidebar_footer: Option<(f32, fn(&mut Ui, &mut S))>,
    pub above_page: Option<fn(&mut Ui, &mut S)>,
}

/// A footer button, or the same press from a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Ok,
    Cancel,
    Apply,
    /// Restore Defaults for the page shown.
    Defaults,
}

/// What the host gives the window: where its [`State`] lives, and the snapshot Cancel puts back.
pub trait Host: Sized {
    /// Everything the window's pages change, as it was when the window opened or at the last Apply.
    type Snapshot;
    fn window(&mut self) -> &mut State<Self::Snapshot>;
    fn window_ref(&self) -> &State<Self::Snapshot>;
    fn take_snapshot(&self) -> Self::Snapshot;
    /// Whether the settings are as in `snapshot`: Apply has nothing to do.
    fn matches(&self, snapshot: &Self::Snapshot) -> bool;
    /// Put the settings back (Cancel). An error keeps the window open with the snapshot, and the
    /// next close tries again.
    fn restore(&mut self, snapshot: &Self::Snapshot) -> Result<(), String>;
    /// Validate and commit draft fields for OK or Apply. Failure keeps the window editable.
    fn confirm(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// What Apply does before the settings become the new snapshot, for a setting a running part
    /// of the host takes up only on Apply. An error keeps the old snapshot.
    fn apply(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// A control is taking the keys (a shortcut being recorded): search leaves them and Esc does
    /// not close the window.
    fn keys_captured(&self) -> bool {
        false
    }
    /// The user left the page shown for another tab.
    fn page_left(&mut self) {}
    /// A search result on `page` was opened, just before the search clears.
    fn result_opened(&mut self, _page: usize) {}
    /// The window closed (after any restore).
    fn closed(&mut self) {}
}

/// The window's own state, kept by the host.
pub struct State<H> {
    pub open: bool,
    /// The page shown, an index into the host's pages.
    pub section: usize,
    /// The search text. Empty shows the selected page as it is.
    pub filter: String,
    /// The page on screen when the current search started. Clearing the field returns there.
    search_from: Option<usize>,
    /// The row to scroll to and outline after a result is chosen.
    reveal: Option<settings_search::Reveal>,
    snapshot: Option<H>,
    /// Why the last Cancel could not put the settings back; Apply waits until one succeeds.
    restore_error: Option<String>,
    confirmation_error: Option<String>,
}

impl<H> State<H> {
    /// Keeps the outline of the row a search result revealed on screen until the test is done.
    #[cfg(any(test, feature = "test-support"))]
    pub fn keep_reveal_visible(&mut self) -> bool {
        let Some(reveal) = self.reveal.as_mut() else { return false };
        reveal.until = f64::INFINITY;
        true
    }
}

impl<H> Default for State<H> {
    fn default() -> Self {
        State {
            open: false,
            section: 0,
            filter: String::new(),
            search_from: None,
            reveal: None,
            snapshot: None,
            restore_error: None,
            confirmation_error: None,
        }
    }
}

impl<H> State<H> {
    /// Open the window at `page`, bounded by the `pages` there are.
    pub fn open_at(&mut self, page: usize, pages: usize) {
        self.open = true;
        self.section = page.min(pages.saturating_sub(1));
    }

    /// What Cancel puts back, while the window holds it.
    pub fn snapshot(&self) -> Option<&H> {
        self.snapshot.as_ref()
    }

    pub fn restore_error(&self) -> Option<&str> {
        self.restore_error.as_deref()
    }

    /// Leave search: a page tab shows that page on its own.
    pub fn clear_search(&mut self) {
        self.filter.clear();
        self.search_from = None;
        self.reveal = None;
    }
}

/// The search field's id in the window with id `window`.
pub fn search_id(window: egui::Id) -> egui::Id {
    window.with("search")
}

/// Take the snapshot if the window holds none yet.
pub fn hold<S: Host>(host: &mut S) {
    if host.window_ref().snapshot.is_none() {
        let snapshot = host.take_snapshot();
        host.window().snapshot = Some(snapshot);
    }
}

/// Whether Apply has anything to do: something changed since the window opened or since the last
/// Apply. False while the window holds no snapshot.
pub fn apply_pending<S: Host>(host: &S) -> bool {
    host.window_ref().snapshot.as_ref().is_some_and(|snapshot| !host.matches(snapshot))
}

/// Do what `button` does, from the footer or from a command. Returns a draft or Apply error.
pub fn press<S: Host>(host: &mut S, pages: &[Page<S>], button: Button) -> Result<(), String> {
    host.window().confirmation_error = None;
    if matches!(button, Button::Ok | Button::Apply)
        && host.window_ref().restore_error.is_none()
        && let Err(reason) = host.confirm()
    {
        host.window().confirmation_error = Some(reason.clone());
        return Err(reason);
    }
    match button {
        Button::Ok => close(host, false),
        Button::Cancel => close(host, true),
        Button::Apply => {
            if host.window_ref().restore_error.is_none() {
                host.apply()?;
                let snapshot = host.take_snapshot();
                host.window().snapshot = Some(snapshot);
            }
        }
        Button::Defaults => {
            if let Some(page) = pages.get(host.window_ref().section) {
                (page.host().defaults)(host);
            }
        }
    }
    Ok(())
}

/// Close the window; `revert` puts back the snapshot (Cancel, Esc, ×). While a restore has failed,
/// every close reverts.
pub fn close<S: Host>(host: &mut S, revert: bool) {
    let window = host.window();
    let revert = revert || window.restore_error.is_some();
    if let Some(snapshot) = window.snapshot.take()
        && revert
        && let Err(reason) = host.restore(&snapshot)
    {
        let window = host.window();
        window.snapshot = Some(snapshot);
        window.open = true;
        window.restore_error = Some(reason);
        return;
    }
    let window = host.window();
    window.restore_error = None;
    window.confirmation_error = None;
    window.open = false;
    window.clear_search();
    host.closed();
}

/// Draw the window if it is open. `edited` runs after the pages drew and before the pressed
/// button acts, for what the host does when a setting changed. Returns the button pressed.
pub fn show<S: Host>(ctx: &egui::Context, host: &mut S, pages: &[Page<S>], options: &Options<'_, S>, edited: impl FnOnce(&mut S)) -> Option<Button> {
    if !host.window_ref().open {
        let window = host.window();
        window.clear_search();
        window.snapshot = None;
        settings_search::set_reveal(ctx, None);
        return None;
    }
    hold(host);
    if host.window_ref().reveal.as_ref().is_some_and(settings_search::Reveal::expired) {
        host.window().reveal = None;
    }
    let sink = settings_search::shift_key_sink(ctx);
    settings_search::set_key_sink_prev(ctx, sink);
    let pending = apply_pending(host);
    let labels = &options.labels;
    let t = Tokens::get();
    let window = crate::Window::new(options.id, labels.icon, labels.title)
        .cards(10.0 / 3.0)
        .page_rows(24.0)
        .page_header_in_titlebar()
        .page_padding(egui::Margin {
            left: (t.space_24 - t.space_2) as i8,
            right: (t.space_24 - t.space_2) as i8,
            top: (t.space_16 + t.space_2) as i8,
            bottom: t.space_10 as i8,
        })
        .fields()
        .actions(labels.ok, labels.cancel, Some(labels.apply))
        .apply_enabled(pending && host.window_ref().restore_error.is_none());
    let window = if host.keys_captured() { window.keep_open_on_escape() } else { window };
    let body = |ui: &mut Ui, host: &mut S| page_body(ui, host, pages, options);
    let list: Vec<crate::WindowPage<'_, S>> = pages
        .iter()
        .map(|page| {
            let page = page.host();
            crate::WindowPage { icon: page.icon, label: page.label, group: page.group, handle: page.handle, scroll: page.scroll, draw: &body }
        })
        .collect();
    let header = |ui: &mut Ui, host: &mut S| search_header(ui, host, options);
    let on_select = |host: &mut S, index: usize| {
        host.window().clear_search();
        if host.window_ref().section != index {
            host.page_left();
        }
        host.window().section = index;
    };
    let hooks = crate::panel::PageListHooks {
        header: &header,
        on_select: &on_select,
        sidebar_footer: options.sidebar_footer.as_ref().map(|(height, footer)| (*height, footer as &dyn Fn(&mut Ui, &mut S))),
    };
    let mut selected = host.window_ref().section;
    let mut defaults = false;
    let (closed, action) = window.show_pages_with_header(ctx, &mut selected, &list, host, &hooks, |ui| {
        let button = crate::button(ui, labels.restore_defaults, crate::Button::Normal).backing_on_hover_text(labels.restore_defaults_hint);
        solidcraft_ui_registry::publish_target(&format!("{}:defaults", options.prefix), button.rect);
        defaults = button.clicked();
    });
    if let Some(rect) = solidcraft_ui_registry::rect_of("dialog:page_body") {
        solidcraft_ui_registry::publish_rect(&format!("{}:page_body", options.prefix), rect);
    }
    for name in ["window", "footer", "ok", "apply", "cancel"] {
        if let Some(rect) = solidcraft_ui_registry::rect_of(&format!("dialog:{name}")) {
            solidcraft_ui_registry::publish_target(&format!("{}:{name}", options.prefix), rect);
        }
    }
    if let Some(rect) = solidcraft_ui_registry::rect_of("dialog:footer:actions") {
        solidcraft_ui_registry::publish_rect(&format!("{}:actions", options.prefix), rect);
    }
    edited(host);
    let button = match action {
        crate::Action::Ok => Some(Button::Ok),
        crate::Action::Apply => Some(Button::Apply),
        crate::Action::Cancel => Some(Button::Cancel),
        crate::Action::None if defaults => Some(Button::Defaults),
        crate::Action::None if closed => Some(Button::Cancel),
        crate::Action::None => None,
    };
    // A failed Apply shows on the window like a failed restore: Apply waits and a close reverts.
    if let Some(button) = button
        && let Err(reason) = press(host, pages, button)
        && host.window_ref().confirmation_error.is_none()
    {
        host.window().restore_error = Some(reason);
    }
    // The footer was drawn before this frame's edits (and before an Apply): show Apply's new
    // state on the next frame rather than at the next input.
    if host.window_ref().open && apply_pending(host) != pending {
        ctx.request_repaint();
    }
    button
}

/// A page's place in the list, as search results name it: "Group > Page".
fn page_path<S>(pages: &[Page<S>], index: usize) -> String {
    let Some(page) = pages.get(index) else { return String::new() };
    let group = pages.get(..=index).and_then(|pages| pages.iter().rev().find_map(|page| page.host().group));
    match group {
        Some(group) => format!("{} > {}", solidcraft_ui_text::tr(group), solidcraft_ui_text::tr(page.host().label)),
        None => solidcraft_ui_text::tr(page.host().label).to_owned(),
    }
}

fn search_header<S: Host>(ui: &mut Ui, host: &mut S, options: &Options<'_, S>) {
    let id = search_id(options.id);
    let typed = drive_search(ui, host, id, options.prefix);
    let field = crate::preferences_search_field(ui, id, &mut host.window().filter, options.labels.search, options.prefix);
    if typed {
        place_search_cursor(ui.ctx(), id, host.window_ref().filter.chars().count());
    }
    solidcraft_ui_registry::publish_handle(&format!("{}:search", options.prefix), field.rect.center());
    if field.has_focus() {
        settings_search::note_key_sink(ui.ctx());
    }
    settle_search(host.window());
    settings_search::set_reveal(ui.ctx(), host.window_ref().reveal.clone());
    settings_search::set_force_scroll(ui.ctx(), !host.window_ref().filter.trim().is_empty());
}

/// Keys for the search field: Cmd+F focuses it, typing anywhere goes to it, Up/Down move through
/// the results, Enter opens one, Esc clears it. A control taking the keys keeps them.
fn drive_search<S: Host>(ui: &mut Ui, host: &mut S, id: egui::Id, prefix: &str) -> bool {
    let ctx = ui.ctx().clone();
    if host.keys_captured() {
        return false;
    }
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
        ctx.memory_mut(|memory| memory.request_focus(id));
    }
    let sink = settings_search::key_sink(&ctx);
    let mut typed = false;
    if !sink {
        let modifiers = ctx.input(|input| input.modifiers);
        if !modifiers.command && !modifiers.ctrl && !modifiers.alt {
            let mut chars = String::new();
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    let egui::Event::Text(text) = event else { return true };
                    if text.chars().any(char::is_control) {
                        return true;
                    }
                    chars.push_str(text);
                    false
                });
            });
            if !chars.is_empty() {
                host.window().filter.push_str(&chars);
                ctx.memory_mut(|memory| memory.request_focus(id));
                typed = true;
            }
        }
    }
    let query = host.window_ref().filter.trim().to_lowercase();
    if query.is_empty() {
        return typed;
    }
    let (nav_query, cursor, hits) = settings_search::nav(&ctx);
    let fresh = nav_query == query;
    let cursor = if fresh { cursor.min(hits.len().saturating_sub(1)) } else { 0 };
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) && fresh && !hits.is_empty() {
        settings_search::set_cursor(&ctx, (cursor + 1).min(hits.len() - 1));
    } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) && fresh && !hits.is_empty() {
        settings_search::set_cursor(&ctx, cursor.saturating_sub(1));
    } else if fresh && !hits.is_empty() && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        if let Some(hit) = hits.get(cursor) {
            open_result(host, hit.page, &hit.label, prefix);
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
    } else {
        settings_search::set_cursor(&ctx, cursor);
    }
    if !host.window_ref().filter.trim().is_empty() && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        host.window().filter.clear();
        ctx.memory_mut(|memory| memory.surrender_focus(id));
    }
    typed
}

fn place_search_cursor(ctx: &egui::Context, id: egui::Id, chars: usize) {
    let Some(mut state) = egui::TextEdit::load_state(ctx, id) else { return };
    let cursor = egui::text::CCursor::new(chars);
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(cursor)));
    state.store(ctx, id);
}

/// Show the result's page with the matching row outlined, and leave search.
fn open_result<S: Host>(host: &mut S, page: usize, label: &str, prefix: &str) {
    host.result_opened(page);
    let window = host.window();
    window.filter.clear();
    window.search_from = None;
    window.section = page;
    window.reveal = Some(settings_search::reveal_for(page, label, prefix));
}

/// An empty field returns to the page that was up when the search started.
fn settle_search<H>(window: &mut State<H>) {
    if window.filter.trim().is_empty() {
        if window.reveal.is_none()
            && let Some(page) = window.search_from.take()
        {
            window.section = page;
        }
    } else if window.search_from.is_none() {
        window.search_from = Some(window.section);
    }
}

/// The page body: the selected page, or while searching every page that has a match, each under
/// its path, with the matches kept and outlined.
fn page_body<S: Host>(ui: &mut Ui, host: &mut S, pages: &[Page<S>], options: &Options<'_, S>) {
    if let Some(above) = options.above_page {
        above(ui, host);
    }
    let filter = host.window_ref().filter.clone();
    let _search = settings_search::begin(ui, &filter, options.prefix);
    if settings_search::querying(ui) {
        // Paint only the pages that match. A first pass is invisible, so rows drawn later cannot
        // land on top of the setting that did match.
        let mut keep = vec![false; pages.len()];
        for (index, page) in pages.iter().enumerate() {
            settings_search::begin_page(ui, index, &page_path(pages, index));
            let before = settings_search::hits(ui);
            let mut probe = settings_search::detached_child(ui, ("prefs-search-probe", index), true);
            (page.host().draw)(&mut probe, host);
            if let Some(kept) = keep.get_mut(index) {
                *kept = settings_search::hits(ui) > before;
            }
        }
        settings_search::rewind_search_layout(ui);
        for (index, page) in pages.iter().enumerate() {
            if !keep.get(index).copied().unwrap_or(false) {
                continue;
            }
            settings_search::begin_page(ui, index, &page_path(pages, index));
            let before = settings_search::hits(ui);
            let mut child = settings_search::detached_child(ui, ("prefs-search-page", index), false);
            (page.host().draw)(&mut child, host);
            settings_search::commit_search_child(ui, &child, before);
        }
        if settings_search::hits(ui) == 0 {
            let t = Tokens::get();
            ui.label(egui::RichText::new(options.labels.no_results).size(t.font_body).color(t.form_dim));
        }
    } else {
        let section = host.window_ref().section;
        settings_search::begin_page(ui, section, &page_path(pages, section));
        if let Some(page) = pages.get(section) {
            (page.host().draw)(ui, host);
        }
    }
    settings_search::store_nav(ui);
}

#[cfg(test)]
#[path = "prefs_window_tests.rs"]
mod tests;
