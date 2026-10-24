//! Search across Preferences. The field is the Keyboard Shortcuts magnifier. A match is the
//! control's own label, its section, its page, or the value a dropdown is showing, scored the
//! same way as command search. English counts too, from the message catalog, so a new setting
//! is found by being drawn — nothing is registered for search.

use std::collections::{HashMap, HashSet};

use egui::{Context, Id, Pos2, Rect, Response, RichText, Sense, Ui, UiBuilder, Vec2, vec2};

use crate::shortcut_box::score;
use crate::theme::Tokens;

const STATE: &str = "prefs_search_state";
const SCROLL: &str = "prefs_search_force_scroll";

#[derive(Clone, Default)]
struct State {
    live: bool,
    query: String,
    page: String,
    page_matches: bool,
    page_shown: bool,
    section: String,
    section_matches: bool,
    section_shown: String,
    block_matches: bool,
    force: u32,
    hiding: u32,
    key: String,
    hits: u32,
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
        state.page_matches = false;
        state.page_shown = false;
        state.section.clear();
        state.section_matches = false;
        state.section_shown.clear();
        state.block_matches = false;
        state.force = 0;
        state.hiding = 0;
        state.key.clear();
        state.hits = 0;
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

pub(crate) fn shows_all(ui: &Ui) -> bool {
    with(ui, |state| state.page_matches || state.section_matches || state.block_matches || state.force > 0).unwrap_or(false)
}

pub(crate) fn set_force_scroll(ctx: &Context, on: bool) {
    ctx.data_mut(|data| data.insert_temp(Id::new(SCROLL), on));
}

pub(crate) fn force_scroll(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp(Id::new(SCROLL))).unwrap_or(false)
}

pub(crate) fn begin_page(ui: &mut Ui, label: &str) {
    let matches = text_hit(ui, label);
    with(ui, |state| {
        state.page = label.to_string();
        state.page_matches = matches;
        state.page_shown = false;
        state.section.clear();
        state.section_matches = false;
        state.section_shown.clear();
    });
    if matches {
        reveal(ui);
    }
}

pub(crate) fn begin_section(ui: &Ui, title: &str) {
    let matches = text_hit(ui, title);
    with(ui, |state| {
        state.section = title.to_string();
        state.section_matches = state.page_matches || matches || state.force > 0;
        state.section_shown.clear();
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
        let show = state.page_matches || state.section_matches || state.force > 0 || label_hit || state.open_blocks.contains(&key) || discover;
        state.block_matches = state.page_matches || state.section_matches || state.force > 0 || label_hit;
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
        ui.label(RichText::new(section.to_uppercase()).size(t.font_small).color(t.text_dim));
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
    text.is_some_and(|text| !state.query.is_empty() && (score(&state.query, text) > 0 || english_hit(&state.query, text)))
}

/// Whether any of `texts` matches the current query. Does not draw a heading.
pub(crate) fn texts_hit(ui: &Ui, texts: &[&str]) -> bool {
    texts.iter().any(|text| text_hit(ui, text))
}

fn text_hit(ui: &Ui, text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    with(ui, |state| !state.query.is_empty() && (score(&state.query, text) > 0 || english_hit(&state.query, text))).unwrap_or(false)
}

/// Whether `query` matches any of `texts`, including the English source of a translation.
pub(crate) fn matches_text(query: &str, texts: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    texts.iter().any(|text| !text.is_empty() && (score(&query, text) > 0 || english_hit(&query, text)))
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
        slot.1.get(shown).is_some_and(|lines| lines.iter().any(|english| score(query, english) > 0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_a_label_a_subsequence_and_the_english_source() {
        assert!(matches_text("zoom", &["Reverse zoom direction"]));
        assert!(matches_text("rzd", &["Reverse zoom direction"]));
        assert!(!matches_text("qqqq", &["Units of new designs"]));
        let _language = crate::i18n::scope("fr");
        assert!(matches_text("units", &["Unités des nouvelles conceptions"]));
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
        h.app.prefs_window.filter = "fusion".into();
        h.frames(3);
        assert!(h.widget("Mouse mapping").is_some(), "the dropdown's current value counts");
        h.app.prefs_window.filter = "zzzz-no-setting".into();
        h.frames(2);
        assert!(h.widget("No matching settings").is_some());
        h.app.prefs_window.filter.clear();
        h.frames(2);
        assert!(h.widget("Units of new designs").is_none());
        assert!(h.widget("No matching settings").is_none());
    }
}
