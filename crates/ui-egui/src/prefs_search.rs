//! Search across Preferences. The field is the Keyboard Shortcuts magnifier. A match is the
//! control's own label, its section, its page, or the value a dropdown is showing, scored the
//! same way as command search. English counts too, from the message catalog, so a new setting
//! is found by being drawn — nothing is registered for search.

use std::collections::{HashMap, HashSet};

use egui::{Context, Id, Pos2, Rect, Response, RichText, Sense, Ui, UiBuilder, Vec2, vec2};

use crate::theme::Tokens;

const STATE: &str = "prefs_search_state";
const SCROLL: &str = "prefs_search_force_scroll";
const NAV: &str = "prefs_search_nav";
const SINK_NOW: &str = "prefs_search_key_sink";
const SINK_PREV: &str = "prefs_search_key_sink_prev";
const REVEAL_ID: &str = "prefs_search_reveal";

/// How long an opened result stays outlined, in milliseconds.
const REVEAL_MS: f64 = 900.0;

/// One keyboard-reachable match from the last finished pass.
#[derive(Clone)]
pub(crate) struct NavHit {
    pub page: usize,
    pub label: String,
}

#[derive(Clone)]
struct PaintedHit {
    page: usize,
    label: String,
}

#[derive(Clone, Default)]
struct NavStore {
    query: String,
    cursor: usize,
    hits: Vec<NavHit>,
    /// The cursor moved this frame, so this frame's passes scroll the selected row into view.
    scroll: bool,
}

/// The row to scroll to and outline after a result is chosen.
#[derive(Clone)]
pub(crate) struct Reveal {
    pub page: usize,
    pub label: String,
    pub until: f64,
}

#[derive(Clone, Default)]
struct State {
    live: bool,
    query: String,
    page: String,
    page_shown: bool,
    section: String,
    section_matches: bool,
    section_shown: String,
    block_matches: bool,
    force: u32,
    /// Set while a matching row draws, so its own slider or buttons stay visible.
    passthrough: u32,
    /// A heading matched, so the controls under it stay visible until the next heading.
    heading_open: bool,
    hiding: u32,
    key: String,
    hits: u32,
    page_index: usize,
    /// Rows whose own text matched, in the order they were drawn this pass.
    nav: Vec<PaintedHit>,
    dirty: bool,
    child_matched: bool,
    memory_query: String,
    extras: HashMap<String, String>,
    open_blocks: HashSet<String>,
    seen_blocks: HashSet<String>,
}

/// Held until the preferences body finishes. A dropdown value learned while a row was hidden
/// asks egui to draw the body again in this frame, so the match appears at once.
pub(crate) struct SearchFrame {
    ctx: Context,
}

impl Drop for SearchFrame {
    fn drop(&mut self) {
        if finish(&self.ctx) {
            self.ctx.request_discard("preferences search");
        }
    }
}

/// Start a pass over the preferences body. An empty filter leaves every control drawn.
pub(crate) fn begin(ui: &Ui, filter: &str) -> SearchFrame {
    let query: String = filter.trim().chars().take(256).collect::<String>().to_lowercase();
    ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(Id::new(STATE), State::default);
        let changed = state.memory_query != query;
        if changed {
            state.memory_query = query.clone();
            state.open_blocks.clear();
            state.seen_blocks.clear();
        }
        state.live = true;
        state.query = query;
        state.page.clear();
        state.page_shown = false;
        state.section.clear();
        state.section_matches = false;
        state.section_shown.clear();
        state.block_matches = false;
        state.force = 0;
        state.passthrough = 0;
        state.heading_open = false;
        state.hiding = 0;
        state.key.clear();
        state.hits = 0;
        state.nav.clear();
        state.dirty = false;
        state.child_matched = false;
    });
    SearchFrame { ctx: ui.ctx().clone() }
}

fn finish(ctx: &Context) -> bool {
    let mut dirty = false;
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(Id::new(STATE));
        dirty = state.live && state.dirty;
        state.live = false;
    });
    dirty
}

fn with<R>(ui: &Ui, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let mut out = None;
    ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(Id::new(STATE));
        if state.live {
            out = Some(f(state));
        }
    });
    out
}

pub(crate) fn querying(ui: &Ui) -> bool {
    with(ui, |state| !state.query.is_empty()).unwrap_or(false)
}

pub(crate) fn hits(ui: &Ui) -> u32 {
    with(ui, |state| state.hits).unwrap_or(0)
}

pub(crate) fn query(ui: &Ui) -> String {
    with(ui, |state| state.query.clone()).unwrap_or_default()
}

/// A child that does not move the parent until [`commit_search_child`] says this group matched.
/// An invisible child only counts hits, so a miss cannot paint over the setting that matched.
pub(crate) fn detached_child(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, invisible: bool) -> Ui {
    let mut builder = UiBuilder::new().id_salt(salt).max_rect(ui.available_rect_before_wrap()).layout(egui::Layout::top_down(egui::Align::Min));
    if invisible {
        builder = builder.invisible();
    }
    ui.new_child(builder)
}

/// Keep the child's layout only when it found something. An empty page or section leaves no gap.
pub(crate) fn commit_search_child(ui: &mut Ui, child: &Ui, before: u32) {
    if hits(ui) > before {
        place_search_child(ui, child);
    }
}

/// The group is already known to match. Keep whatever it drew.
pub(crate) fn place_search_child(ui: &mut Ui, child: &Ui) {
    let rect = child.min_rect();
    if rect.height().is_finite() && rect.height() > 0.0 && rect.width().is_finite() {
        ui.advance_cursor_after_rect(rect);
    }
}

pub(crate) struct SearchLayoutMark {
    page_shown: bool,
    section_shown: String,
    hits: u32,
    heading_open: bool,
    child_matched: bool,
}

pub(crate) fn search_layout_mark(ui: &Ui) -> SearchLayoutMark {
    with(ui, |state| SearchLayoutMark {
        page_shown: state.page_shown,
        section_shown: state.section_shown.clone(),
        hits: state.hits,
        heading_open: state.heading_open,
        child_matched: state.child_matched,
    })
    .unwrap_or(SearchLayoutMark { page_shown: false, section_shown: String::new(), hits: 0, heading_open: false, child_matched: false })
}

pub(crate) fn restore_search_layout(ui: &Ui, mark: SearchLayoutMark) {
    with(ui, |state| {
        state.page_shown = mark.page_shown;
        state.section_shown = mark.section_shown;
        state.hits = mark.hits;
        state.heading_open = mark.heading_open;
        state.child_matched = mark.child_matched;
    });
}

/// Forget the headings counted while every page was probed, and count the visible pass again.
/// Dropdown values learned on the probe stay, so the next pass can show them.
pub(crate) fn rewind_search_layout(ui: &Ui) {
    with(ui, |state| {
        state.page_shown = false;
        state.section_shown.clear();
        state.section_matches = false;
        state.block_matches = false;
        state.heading_open = false;
        state.hits = 0;
        state.child_matched = false;
    });
}

pub(crate) fn shows_all(ui: &Ui) -> bool {
    with(ui, |state| state.section_matches || state.block_matches || state.force > 0 || state.heading_open || state.passthrough > 0).unwrap_or(false)
}

/// Stray notes, figures and buttons. A matching row opts out with [`push_passthrough`].
pub(crate) fn suppress_chrome(ui: &Ui) -> bool {
    querying(ui) && !shows_all(ui)
}

pub(crate) fn set_heading_open(ui: &Ui, open: bool) {
    with(ui, |state| state.heading_open = open);
}

pub(crate) fn push_passthrough(ui: &Ui) {
    with(ui, |state| state.passthrough = state.passthrough.saturating_add(1));
}

pub(crate) fn pop_passthrough(ui: &Ui) {
    with(ui, |state| state.passthrough = state.passthrough.saturating_sub(1));
}

pub(crate) fn set_force_scroll(ctx: &Context, on: bool) {
    ctx.data_mut(|data| data.insert_temp(Id::new(SCROLL), on));
}

pub(crate) fn force_scroll(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp(Id::new(SCROLL))).unwrap_or(false)
}

pub(crate) fn begin_page(ui: &Ui, index: usize, label: &str) {
    with(ui, |state| {
        state.page_index = index;
        state.page = label.to_string();
        state.page_shown = false;
        state.section.clear();
        state.key.clear();
        state.section_matches = false;
        state.section_shown.clear();
        state.heading_open = false;
    });
}

pub(crate) fn begin_section(ui: &Ui, title: &str) {
    let matches = text_hit(ui, title);
    with(ui, |state| {
        state.section = title.to_string();
        state.section_matches = matches || state.force > 0;
        state.section_shown.clear();
        // A dropdown with no row of its own must not store its value under the last row's key.
        state.key.clear();
        state.heading_open = false;
    });
}

pub(crate) struct BlockGate {
    key: String,
    discover: bool,
    pub show: bool,
}

/// `Some` while a search is active. `show` draws the block; otherwise the caller runs it hidden
/// so a dropdown value inside can still be learned.
pub(crate) fn block_gate(ui: &Ui, label: &str, hint: Option<&str>) -> Option<BlockGate> {
    if !querying(ui) {
        return None;
    }
    let label_hit = text_hit(ui, label) || hint.is_some_and(|hint| text_hit(ui, hint));
    with(ui, |state| {
        let key = format!("{}\n{}\n{}", state.page, state.section, label);
        state.child_matched = false;
        state.key = key.clone();
        let discover = !state.seen_blocks.contains(&key);
        let show = state.section_matches || state.force > 0 || label_hit || state.open_blocks.contains(&key);
        state.block_matches = state.section_matches || state.force > 0 || label_hit;
        BlockGate { key, discover, show }
    })
}

pub(crate) fn block_finish(ui: &Ui, gate: &BlockGate, showed: bool, hits_before: u32) {
    with(ui, |state| {
        let gained = state.hits > hits_before || state.child_matched;
        state.seen_blocks.insert(gate.key.clone());
        if showed {
            if gained || state.block_matches {
                state.open_blocks.insert(gate.key.clone());
            } else if gate.discover {
                state.open_blocks.remove(&gate.key);
                state.dirty = true;
            }
        } else if gained {
            state.open_blocks.insert(gate.key.clone());
            state.dirty = true;
        }
        state.child_matched = false;
        state.block_matches = false;
    });
}

pub(crate) fn push_force(ui: &Ui) {
    with(ui, |state| state.force = state.force.saturating_add(1));
}

pub(crate) fn pop_force(ui: &Ui) {
    with(ui, |state| state.force = state.force.saturating_sub(1));
}

pub(crate) fn set_control(ui: &Ui, label: &str) {
    with(ui, |state| state.key = format!("{}\n{}\n{}", state.page, state.section, label));
}

/// Draw this control. A miss returns false and draws nothing; the caller still runs the control
/// off-screen when it needs the widget's current value.
pub(crate) fn visible(ui: &mut Ui, texts: &[&str]) -> bool {
    let show = wanted(ui, texts);
    if show && querying(ui) {
        if with(ui, |state| state.hiding > 0).unwrap_or(false) {
            with(ui, |state| state.child_matched = true);
        } else {
            reveal(ui);
        }
    }
    show
}

fn wanted(ui: &Ui, texts: &[&str]) -> bool {
    if !querying(ui) {
        return true;
    }
    if shows_all(ui) || texts.iter().any(|text| text_hit(ui, text)) {
        return true;
    }
    with(ui, |state| {
        let key = state.key.clone();
        extra_hit(state, &key)
    })
    .unwrap_or(false)
}

/// The value a dropdown is showing. Stored for this control so the next pass can match it.
pub(crate) fn note_current(ui: &Ui, text: &str) {
    if !querying(ui) || text.is_empty() {
        return;
    }
    let clipped: String = text.chars().take(256).collect();
    with(ui, |state| {
        if state.key.is_empty() {
            return;
        }
        let key = state.key.clone();
        let old = state.extras.get(&key).cloned();
        if old.as_deref() == Some(clipped.as_str()) {
            return;
        }
        let was = extra_hit_text(state, old.as_deref());
        state.extras.insert(key, clipped.clone());
        let now = extra_hit_text(state, Some(clipped.as_str()));
        if was != now {
            state.child_matched = true;
            state.dirty = true;
        }
    });
}

/// After a hidden run, show the control on the next pass if its value now matches.
pub(crate) fn recheck(ui: &Ui, texts: &[&str]) {
    if wanted(ui, texts) {
        with(ui, |state| {
            state.child_matched = true;
            state.dirty = true;
        });
    }
}

pub(crate) fn reveal(ui: &mut Ui) {
    let Some((page, section, draw_page, draw_section)) = with(ui, |state| {
        if state.hiding > 0 {
            return None;
        }
        let draw_page = !state.page_shown && !state.page.is_empty();
        let draw_section = !state.section.is_empty() && state.section_shown != state.section;
        if draw_page {
            state.page_shown = true;
            state.section_shown.clear();
        }
        if draw_section {
            state.section_shown = state.section.clone();
        }
        state.hits = state.hits.saturating_add(1);
        Some((state.page.clone(), state.section.clone(), draw_page, draw_section))
    })
    .flatten() else {
        return;
    };
    let t = Tokens::get();
    if draw_page {
        ui.add_space(t.space_8);
        ui.label(RichText::new(page).strong().size(t.font_label).color(t.form_ink));
    }
    if draw_section {
        let response = ui.label(RichText::new(section.to_uppercase()).size(t.font_small).color(t.text_dim));
        remember(ui, &section, response.rect);
    }
}

pub(crate) fn hidden_response(ui: &mut Ui) -> Response {
    let mut child = ui.new_child(UiBuilder::new().invisible().max_rect(Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0))));
    child.allocate_response(Vec2::ZERO, Sense::hover())
}

pub(crate) fn run_hidden<R>(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    with(ui, |state| state.hiding = state.hiding.saturating_add(1));
    let mut child = ui.new_child(
        UiBuilder::new()
            .invisible()
            .id_salt(("prefs-search-hidden", salt))
            .max_rect(Rect::from_min_size(egui::pos2(0.0, -10_000.0), vec2(480.0, 800.0))),
    );
    let result = add(&mut child);
    with(ui, |state| state.hiding = state.hiding.saturating_sub(1));
    result
}

fn extra_hit(state: &State, key: &str) -> bool {
    extra_hit_text(state, state.extras.get(key).map(String::as_str))
}

fn extra_hit_text(state: &State, text: Option<&str>) -> bool {
    text.is_some_and(|text| setting_score(&state.query, text) > 0 || english_hit(&state.query, text))
}

/// Whether any of `texts` matches the current query. Does not draw a heading.
pub(crate) fn texts_hit(ui: &Ui, texts: &[&str]) -> bool {
    texts.iter().any(|text| text_hit(ui, text))
}

fn text_hit(ui: &Ui, text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    with(ui, |state| text_matches(&state.query, text)).unwrap_or(false)
}

fn text_matches(query: &str, text: &str) -> bool {
    setting_score(query, text) > 0 || english_hit(query, text) || keywords(text).iter().any(|word| setting_score(query, word) > 0)
}

/// Prefix, the start of a word, or a run of at least three characters. Scattered letters are not
/// a match: "zoom" finds "Reverse zoom direction", and "rzd" does not. Marks are ignored, so
/// "preferences" finds "préférences".
fn setting_score(query: &str, text: &str) -> u32 {
    if query.chars().count() < 2 || text.is_empty() {
        return 0;
    }
    let query = fold(query);
    let text = fold(text);
    if text.starts_with(&query) {
        return 1000;
    }
    if text.split(|c: char| !c.is_alphanumeric()).any(|word| !word.is_empty() && word.starts_with(&query)) {
        return 800;
    }
    if query.chars().count() >= 3 && text.contains(&query) {
        return 600;
    }
    0
}

/// Whether `query` matches any of `texts`, including the English source of a translation.
pub(crate) fn matches_text(query: &str, texts: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    texts.iter().any(|text| text_matches(&query, text))
}

/// Words that find a setting even though they are not written on it.
fn keywords(text: &str) -> &'static [&'static str] {
    match fold(text).as_str() {
        "theme" => &["dark", "light", "appearance"],
        "anti-aliasing" | "antialiasing" => &["antialiasing", "msaa"],
        _ => &[],
    }
}

/// Lower case, without the marks that distinguish "é" from "e".
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'œ' | 'Œ' => out.push_str("oe"),
            'æ' | 'Æ' => out.push_str("ae"),
            'ß' => out.push_str("ss"),
            c => out.extend(strip_mark(c).to_lowercase()),
        }
    }
    out
}

fn strip_mark(c: char) -> char {
    match c {
        'À'..='Å' | 'à'..='å' | 'Ā' | 'ā' | 'Ă' | 'ă' | 'Ą' | 'ą' => 'a',
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Č' | 'č' => 'c',
        'È'..='Ë' | 'è'..='ë' | 'Ē' | 'ē' | 'Ĕ' | 'ĕ' | 'Ė' | 'ė' | 'Ę' | 'ę' | 'Ě' | 'ě' => 'e',
        'Ì'..='Ï' | 'ì'..='ï' | 'Ī' | 'ī' | 'Į' | 'į' => 'i',
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ň' | 'ň' => 'n',
        'Ò'..='Ö' | 'ò'..='ö' | 'Ō' | 'ō' | 'Ő' | 'ő' => 'o',
        'Ù'..='Ü' | 'ù'..='ü' | 'Ū' | 'ū' | 'Ů' | 'ů' | 'Ű' | 'ű' | 'Ų' | 'ų' => 'u',
        'Ý' | 'ý' | 'Ÿ' | 'ÿ' => 'y',
        'Ž' | 'ž' => 'z',
        c => c,
    }
}

fn english_hit(query: &str, shown: &str) -> bool {
    use std::cell::RefCell;
    thread_local! {
        static MAP: RefCell<(String, HashMap<String, Vec<String>>)> = RefCell::new((String::new(), HashMap::new()));
    }
    MAP.with(|slot| {
        let lang = crate::i18n::language().to_string();
        let mut slot = slot.borrow_mut();
        if slot.0 != lang {
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            let locale = crate::i18n::Locale::for_language(&lang);
            crate::i18n::each_english_message(|id, english| {
                if english.contains('{') {
                    return;
                }
                let shown = locale.text(id);
                if shown != english {
                    map.entry(shown.to_string()).or_default().push(english.to_string());
                }
            });
            *slot = (lang, map);
        }
        slot.1.get(shown).is_some_and(|lines| {
            lines.iter().any(|english| setting_score(query, english) > 0 || keywords(english).iter().any(|word| setting_score(query, word) > 0))
        })
    })
}

/// A text field or drag value had the keyboard on the previous frame.
pub(crate) fn shift_key_sink(ctx: &Context) -> bool {
    let held = ctx.data(|data| data.get_temp::<bool>(Id::new(SINK_NOW))).unwrap_or(false);
    ctx.data_mut(|data| data.insert_temp(Id::new(SINK_NOW), false));
    held
}

pub(crate) fn key_sink(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(Id::new(SINK_PREV))).unwrap_or(false)
}

pub(crate) fn note_key_sink(ctx: &Context) {
    ctx.data_mut(|data| data.insert_temp(Id::new(SINK_NOW), true));
}

pub(crate) fn set_key_sink_prev(ctx: &Context, held: bool) {
    ctx.data_mut(|data| data.insert_temp(Id::new(SINK_PREV), held));
}

/// The matches drawn last frame, for Up/Down and Enter before this pass rebuilds them.
pub(crate) fn nav(ctx: &Context) -> (String, usize, Vec<NavHit>) {
    ctx.data(|data| {
        let store = data.get_temp::<NavStore>(Id::new(NAV)).unwrap_or_default();
        (store.query, store.cursor, store.hits)
    })
}

pub(crate) fn set_cursor(ctx: &Context, cursor: usize) {
    ctx.data_mut(|data| {
        let store = data.get_temp_mut_or_default::<NavStore>(Id::new(NAV));
        store.scroll = store.cursor != cursor;
        store.cursor = cursor;
    });
}

/// Keep this pass's matches for the next frame's keys.
pub(crate) fn store_nav(ui: &Ui) {
    let (query, hits) =
        with(ui, |state| (state.query.clone(), state.nav.iter().map(|hit| NavHit { page: hit.page, label: hit.label.clone() }).collect::<Vec<_>>()))
            .unwrap_or_default();
    ui.ctx().data_mut(|data| {
        let store = data.get_temp_mut_or_default::<NavStore>(Id::new(NAV));
        store.query = query;
        store.hits = hits;
        if store.cursor >= store.hits.len() {
            store.cursor = 0;
        }
    });
}

/// A row whose own words matched. Section titles and controls both call this once they have a rect.
pub(crate) fn remember(ui: &mut Ui, label: &str, rect: Rect) {
    if !ui.is_visible() || !querying(ui) || label.is_empty() || !text_hit(ui, label) {
        return;
    }
    if with(ui, |state| state.hiding > 0).unwrap_or(true) {
        return;
    }
    let (cursor, scroll) = ui.ctx().data(|data| {
        let store = data.get_temp::<NavStore>(Id::new(NAV)).unwrap_or_default();
        (store.cursor, store.scroll)
    });
    let selected = with(ui, |state| {
        let selected = state.nav.len() == cursor;
        state.nav.push(PaintedHit { page: state.page_index, label: label.to_string() });
        selected
    })
    .unwrap_or(false);
    if !selected {
        return;
    }
    let t = Tokens::get();
    ui.painter().rect_stroke(rect, t.radius_sm, egui::Stroke::new(t.space_2, t.form_primary), egui::StrokeKind::Outside);
    // Only when Up/Down moved the cursor: following it every frame pins the list to the match.
    if scroll {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
    crate::scenario::publish_handle("prefs:search:current", rect.center());
}

pub(crate) fn set_reveal(ctx: &Context, reveal: Option<Reveal>) {
    ctx.data_mut(|data| {
        if let Some(reveal) = reveal {
            data.insert_temp(Id::new(REVEAL_ID), reveal);
        } else {
            data.remove::<Reveal>(Id::new(REVEAL_ID));
        }
    });
}

pub(crate) fn reveal_for(page: usize, label: &str) -> Reveal {
    Reveal { page, label: label.to_string(), until: crate::now_ms() + REVEAL_MS }
}

/// Scroll the opened setting into view and outline it until [`Reveal::until`].
pub(crate) fn follow(ui: &Ui, label: &str, rect: Rect) {
    let Some(reveal) = ui.data(|data| data.get_temp::<Reveal>(Id::new(REVEAL_ID))) else { return };
    if crate::now_ms() > reveal.until || fold(label) != fold(&reveal.label) {
        return;
    }
    let page = with(ui, |state| state.page_index);
    if page.is_some_and(|page| page != reveal.page) {
        return;
    }
    ui.scroll_to_rect(rect, Some(egui::Align::Center));
    let t = Tokens::get();
    ui.painter().rect_stroke(rect, t.radius_sm, egui::Stroke::new(t.space_2, t.form_primary), egui::StrokeKind::Outside);
    crate::scenario::publish_handle("prefs:revealed", rect.center());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_a_label_a_subsequence_and_the_english_source() {
        assert!(matches_text("zoom", &["Reverse zoom direction"]));
        assert!(matches_text("zo", &["Reverse zoom direction"]));
        assert!(!matches_text("rzd", &["Reverse zoom direction"]));
        assert!(!matches_text("q", &["Units of new designs"]));
        assert!(!matches_text("qqqq", &["Units of new designs"]));
        let _language = crate::i18n::scope("fr");
        assert!(matches_text("units", &["Unités des nouvelles conceptions"]));
        assert!(matches_text("preferences", &["préférences"]));
        assert!(matches_text("dark", &["Theme"]));
        assert!(matches_text("antialiasing", &["Anti-aliasing"]));
        assert!(!matches_text("dark", &["Units of new designs"]));
    }

    /// The search field and its empty result name the window they search (#8318): "Search
    /// settings" under a "Preferences" title read as two names for one window.
    #[test]
    fn the_search_field_names_the_window_it_searches() {
        let name = crate::i18n::tr("ui.preferences").to_lowercase();
        for id in ["prefs.search", "prefs.search_empty"] {
            let text = crate::i18n::tr(id).to_lowercase();
            assert!(text.contains(&name), "`{text}` should name the window `{name}`");
        }
    }

    /// Typing focuses the field, Enter opens that setting, and Escape returns to the page
    /// that was up when the search started. Recording a shortcut keeps the key.
    #[test]
    fn preferences_search_types_opens_and_returns() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GRAPHICS);
        h.frames(2);
        h.step(&serde_json::json!({"key": "F", "cmd": true})).expect("find");
        h.frames(1);
        assert!(h.ctx.memory(|memory| memory.has_focus(egui::Id::new("prefs_search"))), "Cmd+F focuses search");
        h.step(&serde_json::json!({"text": "zo"})).expect("type");
        h.frames(3);
        assert!(h.widget("Reverse zoom direction").is_some(), "typed letters search from the open page");
        assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_GRAPHICS);
        let first = crate::scenario::handle_at("prefs:search:current");
        h.step(&serde_json::json!({"key": "ArrowDown"})).expect("next match");
        h.frames(1);
        let second = crate::scenario::handle_at("prefs:search:current");
        assert!(first.is_some() && second.is_some() && first != second, "Down moves to the next match");
        h.step(&serde_json::json!({"key": "ArrowUp"})).expect("previous match");
        h.frames(1);
        assert_eq!(crate::scenario::handle_at("prefs:search:current"), first, "Up moves back");
        h.step(&serde_json::json!({"key": "Enter"})).expect("open");
        h.frames(2);
        assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_NAVIGATION, "Enter opens the match's page");
        assert!(h.app.prefs_window.filter.is_empty());
        assert!(h.widget("Reverse zoom direction").is_some());
        assert!(crate::scenario::handle_at("prefs:revealed").is_some(), "the opened row is highlighted");
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GRAPHICS);
        h.frames(2);
        h.step(&serde_json::json!({"text": "zo"})).expect("search again");
        h.frames(2);
        h.step(&serde_json::json!({"key": "Escape"})).expect("clear");
        h.frames(1);
        assert!(h.app.prefs_window.filter.is_empty());
        assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_GRAPHICS, "Escape returns to the page from before the search");
    }

    /// Typing in a number field (the largest STEP file) edits that field instead of starting
    /// a search.
    #[test]
    fn preferences_number_fields_keep_typed_text() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        h.frames(2);
        let field = crate::scenario::rect_of("prefs:max_step_file_mb").expect("STEP limit field");
        h.step(&serde_json::json!({"click": [field.center().x, field.center().y]})).expect("click the field");
        h.frames(2);
        h.step(&serde_json::json!({"text": "7"})).expect("type");
        h.frames(2);
        assert!(h.app.prefs_window.filter.is_empty(), "typed `{}` into the search", h.app.prefs_window.filter);
    }

    /// The selected match is scrolled to when the arrows move it, not on every frame, so the
    /// wheel can scroll the results away from it.
    #[test]
    fn preferences_search_results_scroll_freely() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        h.frames(2);
        h.app.prefs_window.filter = "view".into();
        h.frames(3);
        let first = crate::scenario::handle_at("prefs:search:current").expect("first match");
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        };
        // The wheel scrolls smoothly: give it time to come to rest before each check.
        h.frame_with(vec![egui::Event::PointerMoved(first), wheel.clone()]);
        h.frames(60);
        let scrolled = crate::scenario::handle_at("prefs:search:current").expect("match still drawn");
        assert!(scrolled.y < first.y - 100.0, "the wheel scrolls past the first match ({} -> {})", first.y, scrolled.y);
        h.step(&serde_json::json!({"key": "ArrowDown"})).expect("next match");
        h.frames(60);
        let next = crate::scenario::handle_at("prefs:search:current").expect("next match");
        let body = crate::scenario::rect_of("dialog:page_body").expect("page body");
        assert!(body.contains(next), "Down brings the next match into view ({next:?} in {body:?})");
        h.frame_with(vec![egui::Event::PointerMoved(next), wheel]);
        h.frames(60);
        let after = crate::scenario::handle_at("prefs:search:current").expect("match still drawn");
        assert!(after.y < next.y - 100.0, "the wheel still scrolls after Down ({} -> {})", next.y, after.y);
    }

    #[test]
    fn preferences_search_filters_live_controls_across_pages() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        h.frames(2);
        assert!(h.widget("Units of new designs").is_none(), "another page stays put until a search");
        h.app.prefs_window.filter = "units".into();
        h.frames(3);
        assert!(h.widget("Units of new designs").is_some(), "a row is found from its label");
        assert!(h.widget("Reverse zoom direction").is_none());
        h.app.prefs_window.filter = "zoom".into();
        h.frames(3);
        assert!(h.widget("Reverse zoom direction").is_some());
        assert!(h.widget("Units of new designs").is_none());
        assert!(h.widget("Grid").is_none(), "a checkbox on another page stays hidden");
        assert!(h.widget("Interface language").is_none(), "the page around a match stays hidden");
        assert!(h.widget("Advanced").is_none(), "a folded heading is not a match");
        let search = crate::scenario::handle_at("prefs:search").expect("search field");
        let hit = h.widget("Reverse zoom direction").expect("zoom row");
        let gap = hit[1] - f64::from(search.y);
        assert!(gap < 520.0, "search results leave a {gap:.0}px gap under the field");
        assert!(h.widget("Translation X").is_none(), "unmatched pages do not stack on the result");
        h.app.prefs_window.filter = "extrude".into();
        h.frames(3);
        assert!(crate::scenario::rect_of("prefs:search:shortcuts").is_some(), "shortcut matches collapse to one row");
        assert!(crate::scenario::rect_of("dialog:cell:Extrude").is_none(), "the shortcut list stays closed");
        h.step(&serde_json::json!({"click": {"handle": "prefs:search:clear"}})).expect("clear");
        h.frames(2);
        assert!(h.app.prefs_window.filter.is_empty(), "the search X clears the field");
        h.app.prefs_window.filter = "fusion".into();
        h.frames(3);
        assert!(h.widget("Mouse mapping").is_some(), "the dropdown's current value counts");
        h.app.prefs_window.filter = "zzzz-no-setting".into();
        h.frames(2);
        assert!(h.widget("No matching preferences").is_some());
        h.app.prefs_window.filter = "units".into();
        h.frames(2);
        h.step(&serde_json::json!({"click": {"handle": "prefs:page:Design"}})).expect("design page");
        h.frames(2);
        assert!(h.app.prefs_window.filter.is_empty(), "a page tab clears the search");
        assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_DESIGN);
        assert!(h.widget("Units of new designs").is_some(), "the tab shows its own page");
        assert!(crate::scenario::rect_of("prefs:search:shortcuts").is_none(), "shortcut matches leave with the search");
        h.app.prefs_window.filter = "zoom".into();
        h.frames(2);
        h.step(&serde_json::json!({"click": {"handle": "prefs:page:Design"}})).expect("same page");
        h.frames(2);
        assert!(h.app.prefs_window.filter.is_empty(), "the current tab also clears the search");
        assert!(h.widget("Reverse zoom direction").is_none());
        assert!(h.widget("Units of new designs").is_some());
    }
}
