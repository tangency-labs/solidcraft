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
    /// Set while a row with no label of its own runs hidden: what its controls say stands in for it.
    unlabelled: u32,
    /// Rows drawn this page with no label, so each gets a key of its own.
    unlabelled_rows: u32,
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
        state.unlabelled_rows = 0;
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
        state.unlabelled_rows = 0;
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

/// Key a row that has no label of its own by its place in the section, so two of them do not share
/// one stored text. Returns the text to match the row on (still empty).
pub(crate) fn set_unlabelled_control(ui: &Ui) {
    with(ui, |state| {
        state.unlabelled_rows += 1;
        state.key = format!("{}\n{}\n#{}", state.page, state.section, state.unlabelled_rows);
    });
}

pub(crate) fn push_unlabelled(ui: &Ui) {
    with(ui, |state| state.unlabelled = state.unlabelled.saturating_add(1));
}

pub(crate) fn pop_unlabelled(ui: &Ui) {
    with(ui, |state| state.unlabelled = state.unlabelled.saturating_sub(1));
}

/// A control inside a row with no label says what it is. Stored for the row, so the next pass can
/// match the row on those words and draw it, as it does for a dropdown's current value.
pub(crate) fn note_unlabelled(ui: &Ui, texts: &[&str]) {
    if !querying(ui) {
        return;
    }
    with(ui, |state| {
        if state.hiding == 0 || state.unlabelled == 0 || state.key.is_empty() {
            return;
        }
        let mut stored = state.extras.get(&state.key).cloned().unwrap_or_default();
        let was = extra_hit_text(state, Some(stored.as_str()));
        for text in texts.iter().filter(|text| !text.is_empty()) {
            if !stored.contains(text) {
                if !stored.is_empty() {
                    stored.push(' ');
                }
                stored.push_str(text);
            }
        }
        let now = extra_hit_text(state, Some(stored.as_str()));
        let key = state.key.clone();
        state.extras.insert(key, stored);
        if was != now {
            state.child_matched = true;
            state.dirty = true;
        }
    });
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
    // A right-to-left label reaches here in display order (see `Catalog::shown`), while the query
    // is typed in reading order: compare the query in display order too, so a multi-word Hebrew
    // or Arabic label is found as it is typed (#10471; #7532 fixed only the test's half).
    setting_score(query, text) > 0
        || setting_score(&crate::bidi::visual(query), text) > 0
        || english_hit(query, text)
        || keywords(text).iter().any(|word| setting_score(query, word) > 0)
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
        "curved surfaces" => &["display quality"],
        "perspective" => &["perspective view"],
        "quick measurement" => &["quick measurement in the view's corner"],
        "analysis" => &["show viewport analysis status"],
        "section" => &["show viewport section status"],
        "live details" => &["live viewport details"],
        "fold items" => &["fold viewport status items"],
        "reference geometry through bodies" => &["show reference geometry through bodies"],
        "single surface analysis" => &["use a single surface analysis"],
        "only while the view is still" => &["only when the view is still"],
        "supersampling" => &["supersampling (ssaa)"],
        "design" => &["design defaults"],
        "step values with arrows and wheel" => &["step values with the arrow keys and the wheel"],
        "show values beside their handles" => &["show values beside their handles in the view"],
        "files from unknown programs" => &["for files from unknown programs"],
        "hide the origin folder in new designs" => &["hide origin by default"],
        "update changed links when a tab comes back" => &["update changed linked designs when a tab comes back"],
        "double-click a component to activate it" => &["double-click a component to activate it, or to edit a linked one in place"],
        "insert design lists open designs first" => &["insert design lists the designs open in other tabs first"],
        "snap to centers" | "snap to centres" => &["snap to face and edge centers", "snap to face and edge centres", "hole snapping"],
        "move by click or drag" => &["move holes by clicking or dragging"],
        "use joint motion handles for move" => &["use joint motion handles for assembly move"],
        "near-axis step" => &["near-axis rotation step (°)", "coarse rotation"],
        "far-axis step" => &["far-axis rotation step (°)", "fine rotation"],
        "axis distances in measure" => &["show x/y/z distances"],
        "tool at the cursor" => &["show tool at cursor"],
        "center and midpoint guides" | "centre and midpoint guides" => {
            &["center and midpoint alignment guides", "centre and midpoint alignment guides"]
        }
        "guides from sketch points too" => &["alignment guides from end points and corners while drawing"],
        "snap to where two guides cross" => &["auto snap to recent centers and midpoints", "auto snap to recent centres and midpoints"],
        "line up with other sketch points" => &["line up with other sketch points while drawing"],
        "center-point arc follows the cursor" | "centre-point arc follows the cursor" => {
            &["center point arc follows the cursor's direction", "centre point arc follows the cursor's direction"]
        }
        "dimension new shapes fully" => &["dimension new shapes"],
        "place from the sketch origin" => &["also place them from the origin (fully constrained)"],
        "shortcut again switches mode" => &["pressing a tool's shortcut again switches its mode"],
        "ask for project options first" => &["project options dialog"],
        "return to the view before" => &["return to the previous view"],
        "keep" => &["keep the current view"],
        "unless turned" => &["return unless rotated"],
        "state icons" => &["sketch icons in the color of their state", "sketch icons in the colour of their state"],
        "conflict color" | "conflict colour" => &["conflicting geometry in the conflict color", "conflicting geometry in the conflict colour"],
        "hide glyphs zoomed out" => &["hide constraint glyphs when zoomed out"],
        "prefer movable points where they overlap" => &["prefer movable sketch points when points overlap"],
        "select other reaches hidden geometry" => &["select hidden geometry with select other"],
        "whole bodies" => &["click selects whole bodies"],
        "free" => &["free orbit"],
        "turn about the world's up axis" => &["turn around the vertical axis"],
        "reverse horizontal pan" => &["reverse horizontal pan direction"],
        "reverse vertical pan" => &["reverse vertical pan direction"],
        "reverse trackpad orbit" => &["reverse trackpad orbit direction"],
        "two-finger zoom while holding" => &["zoom with two-finger scroll while holding"],
        "show it in the 3d view" => &["show the 3d mouse in the 3d view"],
        "picture position" => &["position"],
        "turn the view while a sketch is edited" => &["turn sketches with the 3d mouse"],
        "model" => &["traced"],
        "view center" | "view centre" => &["classic"],
        "axes (signed)" => &["axes", "signed axes"],
        "cube (compact)" => &["cube"],
        "availability" => &["assistant controls"],
        "let ai agents connect" => &["let ai agents connect (control channel / mcp)"],
        "add solidcraft to ai apps" => &["make solidcraft available to ai apps on this computer"],
        "run slow commands in the background" => &["run agent commands in the background"],
        "connect the last agent at start" => &["connect the last-used agent when solidcraft starts"],
        "open the chat at start" => &["open the assistant panel when solidcraft starts"],
        "ask first" => &["ask before changes", "ask every time"],
        "make edits" => &["allow model edits", "auto-allow model edits"],
        "do everything" => &["allow everything"],
        "dock the chat" => &["agent chat side"],
        "show the agent's pointer" => &["show agent pointer"],
        "follow the camera" => &["turn the camera to sketches"],
        "speed" => &["motion speed"],
        "agent pointer" => &["agent cursor"],
        "assistant button in the application bar" => &["show assistant button in application bar"],
        "same as above" => &["as set above"],
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
    fn collapsed_shortcut_results_enter_reveals_the_command_despite_an_old_page_filter() {
        use serde_json::json;
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.ai_features = false;
            h.call("ui.resize", json!({"width":1600,"height":1000}));
            h.call("ui.prefs", json!({"open":"Shortcuts","theme":theme}));
            h.step(&json!({"click":{"handle":"keymap:filter"}})).unwrap();
            h.step(&json!({"text":"solid.extrude"})).unwrap();
            let command = solidcraft_engine::command_specs().last().unwrap().id;
            let original = crate::toolbar_custom::export(&h.app);
            h.app.prefs_window.filter = command.into();
            h.frames(4);
            let (_, _, hits) = nav(&h.ctx);
            let matches: Vec<_> =
                hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_SHORTCUTS && hit.label.contains(command)).collect();
            assert_eq!(matches.len(), 1, "a collapsed result owns one real keyboard target");
            set_cursor(&h.ctx, matches[0].0);
            h.frames(2);
            h.step(&json!({"key":"Enter"})).unwrap();
            h.frames(4);
            assert!(h.app.prefs_window.filter.is_empty());
            assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_SHORTCUTS);
            let target = crate::scenario::rect_of(&format!("keymap:capture:{command}")).unwrap();
            let body = crate::scenario::rect_of("dialog:page_body").unwrap();
            assert!(body.contains_rect(target), "Enter reveals the actual off-screen command, clearing the old local filter");
            assert!(crate::scenario::handle_at("prefs:revealed").is_some());
            assert_eq!(crate::toolbar_custom::export(&h.app), original);
        }
    }

    #[test]
    fn assistant_permission_options_and_limited_value_have_real_enter_targets_while_ai_is_off() {
        use serde_json::json;
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.ai_features = false;
            h.call("ui.resize", json!({"width":1600,"height":1200}));
            h.call("ui.prefs", json!({"open":"General","theme":theme}));
            let original = (h.app.preferences.ai_permissions, h.app.preferences.agent_chat_turn_limit);
            for (query, target) in [
                ("Ask first", "prefs:ai_mode:ask"),
                ("Make edits", "prefs:ai_mode:edits"),
                ("Do everything", "prefs:ai_mode:all"),
                ("Limited", "prefs:agent_chat_turn_limit:mode"),
            ] {
                h.app.prefs_window.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matches: Vec<_> =
                    hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_ASSISTANT && hit.label == query).collect();
                assert_eq!(matches.len(), 1, "{query} owns a single real setting target");
                set_cursor(&h.ctx, matches[0].0);
                h.frames(2);
                h.step(&json!({"key":"Enter"})).unwrap();
                h.frames(3);
                assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_ASSISTANT);
                let rect = crate::scenario::rect_of(target).unwrap();
                let revealed = crate::scenario::handle_at("prefs:revealed").unwrap();
                assert!(revealed.distance(rect.center()) <= 1.0, "{query} reveals its real control");
                assert_eq!(
                    (h.app.preferences.ai_permissions, h.app.preferences.agent_chat_turn_limit),
                    original,
                    "navigation never selects an option"
                );
            }
        }
    }

    #[test]
    fn layout_controls_keep_one_real_search_target_and_native_names() {
        use egui::accesskit::Role;
        for width in [600, 1600] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":width,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","open":"Layout"}));
            for (query, native) in [
                ("History", None),
                ("Browser", None),
                ("Menus", None),
                ("Highlight active commands", Some((Role::CheckBox, "prefs:persistent_ribbon_pressed"))),
                ("Show navigation bar", Some((Role::CheckBox, "prefs:navbar:show"))),
                ("Position", None),
            ] {
                h.app.prefs_window.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> = hits.iter().filter(|hit| hit.page == crate::prefs::PAGE_LAYOUT && hit.label == query).collect();
                assert_eq!(matching.len(), 1, "{width}: {query} has one real Layout target");
                if let Some((role, target)) = native {
                    let tree = h.output.accesskit_update.as_ref().unwrap();
                    let nodes: Vec<_> =
                        tree.nodes.iter().filter(|(_, node)| node.role() == role && node.label() == Some(query) && !node.is_disabled()).collect();
                    assert_eq!(nodes.len(), 1, "{query} retains its native accessible name");
                    let bounds = nodes[0].1.bounds().unwrap();
                    let centre = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
                    assert!(crate::scenario::rect_of(target).unwrap().contains(centre));
                }
            }
        }
    }

    #[test]
    fn navigation_and_mouse3d_search_hits_reveal_the_actual_cards_and_controls() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":1600,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","theme":theme}));
            for (query, page, target) in [
                ("Orbit around", crate::prefs::PAGE_NAVIGATION, "prefs:orbit_around:View center"),
                ("Under the cursor", crate::prefs::PAGE_NAVIGATION, "prefs:orbit_around:Under the cursor"),
                ("Origin", crate::prefs::PAGE_NAVIGATION, "prefs:orbit_around:Origin"),
                ("View navigator", crate::prefs::PAGE_NAVIGATION, "prefs:navigator:Signed axes"),
                ("Axes (signed)", crate::prefs::PAGE_NAVIGATION, "prefs:navigator:Signed axes"),
                ("Cube (compact)", crate::prefs::PAGE_NAVIGATION, "prefs:navigator:Cube"),
                ("Turntable", crate::prefs::PAGE_NAVIGATION, "prefs:turntable"),
                ("Wheel zoom speed", crate::prefs::PAGE_NAVIGATION, "prefs:wheel_zoom_speed"),
                ("Two-finger zoom while holding", crate::prefs::PAGE_NAVIGATION, "prefs:trackpad_zoom_modifier"),
                ("Overall speed", crate::prefs::PAGE_MOUSE3D, "prefs:space_mouse_speed"),
                ("Traced", crate::prefs::PAGE_MOUSE3D, "prefs:3d_mouse_orbit_traced"),
                ("Picture position", crate::prefs::PAGE_MOUSE3D, "prefs:space_mouse_ghost_position"),
            ] {
                h.call("ui.prefs", serde_json::json!({"open":"General"}));
                h.app.prefs_window.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> = hits.iter().enumerate().filter(|(_, hit)| hit.page == page).collect();
                assert_eq!(matching.len(), 1, "{theme}: {query} has one indexed setting");
                set_cursor(&h.ctx, matching[0].0);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                h.frames(3);
                assert_eq!(h.app.prefs_window.section, page);
                assert!(h.app.prefs_window.filter.is_empty());
                let body = crate::scenario::rect_of("dialog:page_body").unwrap();
                assert!(body.contains_rect(crate::scenario::rect_of(target).unwrap()), "{query} reveals its live control");
            }
        }
    }

    #[test]
    fn sketch_picture_groups_and_options_have_one_real_search_hit_and_enter_reveals_the_cards() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":1600,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","theme":theme}));
            let original = (h.app.preferences.sketch_exit_view, h.app.ui.pick_bodies);
            for (query, target, option) in [
                ("When a sketch ends", "prefs:sketch_exit_view:choices", "Keep"),
                ("Return to the view before", "prefs:sketch_exit_view:previous", "Return to the view before"),
                ("Keep", "prefs:sketch_exit_view:keep", "Keep"),
                ("Unless turned", "prefs:sketch_exit_view:unless_rotated", "Unless turned"),
                ("Click selects", "prefs:pick_bodies:choices", "Whole bodies"),
                ("Faces and edges", "prefs:pick_bodies:faces_edges", "Faces and edges"),
                ("Whole bodies", "prefs:pick_bodies:bodies", "Whole bodies"),
            ] {
                h.call("ui.prefs", serde_json::json!({"open":"General"}));
                h.app.prefs_window.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> = hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_SKETCH).collect();
                assert_eq!(matching.len(), 1, "{theme}: {query} has exactly one real Sketch setting hit");
                assert_eq!(matching[0].1.label, query);
                let tree = h.output.accesskit_update.as_ref().unwrap();
                assert!(
                    tree.nodes.iter().any(|(_, node)| {
                        node.role() == egui::accesskit::Role::RadioButton && node.label() == Some(option) && !node.is_disabled()
                    }),
                    "{query} exposes the enabled native card"
                );
                set_cursor(&h.ctx, matching[0].0);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                h.frames(3);
                assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_SKETCH);
                assert!(h.app.prefs_window.filter.is_empty());
                let body = crate::scenario::rect_of("dialog:page_body").unwrap();
                let actual = crate::scenario::rect_of(target).unwrap();
                let revealed = crate::scenario::handle_at("prefs:revealed").expect("Enter publishes an actual reveal target");
                assert!(body.contains_rect(actual), "{theme}: {query} reveals the actual cards inside the body");
                assert!(revealed.distance(actual.center()) <= 1.0, "{theme}: {query} outlines its real group/card");
                if target.ends_with(":choices") {
                    let group = crate::scenario::rect_of(&format!("dialog:row:{query}")).unwrap();
                    assert!((actual.left() - group.left()).abs() <= 1.0, "{theme}: {query} has no phantom label column beside its cards");
                }
                assert_eq!((h.app.preferences.sketch_exit_view, h.app.ui.pick_bodies), original, "navigation does not change a choice");
            }
        }
    }

    #[test]
    fn graphics_cards_tiles_and_short_status_labels_keep_search_paths_and_reveal() {
        for theme in ["light", "dark"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", serde_json::json!({"theme":theme,"language":"en"}));
            h.call("ui.prefs", serde_json::json!({"open":"Graphics"}));
            for (query, label, handle) in [
                ("Display quality", "Curved surfaces", "prefs:display_quality:normal"),
                ("MSAA", "Anti-aliasing", "prefs:msaa:4"),
                ("Ambient occlusion", "Ambient occlusion", "prefs:ambient_occlusion"),
                ("Quick measurement in the view's corner", "Quick measurement", "prefs:quick_measure"),
                ("Show viewport analysis status", "Analysis", "prefs:viewport_analysis_status"),
                ("Show viewport section status", "Section", "prefs:viewport_section_status"),
                ("Live viewport details", "Live details", "prefs:viewport_status_details"),
                ("Fold viewport status items", "Fold items", "prefs:viewport_status_summary"),
            ] {
                h.app.prefs_window.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> =
                    hits.iter().enumerate().filter(|(_, hit)| hit.page == crate::prefs::PAGE_GRAPHICS && hit.label == label).collect();
                assert_eq!(matching.len(), 1, "{theme}: {query} keeps one real Graphics search target");
                let index = matching[0].0;
                set_cursor(&h.ctx, index);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                h.frames(3);
                assert_eq!(h.app.prefs_window.section, crate::prefs::PAGE_GRAPHICS);
                assert!(h.app.prefs_window.filter.is_empty());
                let page = crate::scenario::rect_of("dialog:page_body").unwrap();
                assert!(page.contains_rect(crate::scenario::rect_of(handle).unwrap()), "{query} reveals the real control");
            }
        }
    }

    #[test]
    fn appearance_controls_keep_one_search_result_and_their_page() {
        use egui::accesskit::Role;
        for width in [520, 1600] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":width,"height":1100}));
            h.call("ui.prefs", serde_json::json!({"language":"en","open":"Appearance"}));
            for (query, label, native) in [
                ("3D view colors", "3D view colors", None),
                ("Interface scale", "Interface scale", Some((Role::Slider, "Interface scale", "prefs:scale"))),
                ("Follow a theme file", "Follow a theme file", None),
                ("Background", "Background", Some((Role::Button, "Color", "dialog:Background:Color"))),
                ("Font", "Font", Some((Role::ComboBox, "Interface font", "prefs:ui_font"))),
            ] {
                h.app.prefs_window.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let matching: Vec<_> = hits.iter().filter(|hit| hit.label == label).collect();
                assert_eq!(matching.len(), 1, "width={width}: {query} needs exactly one real navigation hit");
                assert_eq!(matching[0].page, crate::prefs::PAGE_APPEARANCE);
                let at = h.widget(label).unwrap();
                let body = crate::scenario::rect_of("prefs:page_body").unwrap();
                assert!(body.contains(egui::pos2(at[0] as f32, at[1] as f32)));
                if let Some((role, name, target)) = native {
                    let tree = h.output.accesskit_update.as_ref().unwrap();
                    let controls: Vec<_> =
                        tree.nodes.iter().filter(|(_, node)| !node.is_disabled() && node.role() == role && node.label() == Some(name)).collect();
                    assert_eq!(controls.len(), 1, "width={width}: {query} must expose one enabled native {name} control");
                    let bounds = controls[0].1.bounds().unwrap();
                    let centre = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
                    let rect = crate::scenario::rect_of(target).unwrap();
                    assert!(body.contains_rect(rect) && rect.contains(centre), "width={width}: {name} is visible at its real target");
                    if name == "Interface scale" {
                        let row = crate::scenario::rect_of("dialog:row:Interface scale").unwrap();
                        assert!((rect.left() - row.left()).abs() <= 1.0, "accessible-only name reserves no label column");
                    }
                    if name == "Color" {
                        h.step(&serde_json::json!({"click":{"handle":target}})).unwrap();
                        h.frames(2);
                        assert_eq!(h.app.preferences.viewport_background, Some([255, 255, 255]), "the search hit edits the real setting");
                    }
                    if name == "Interface font" {
                        h.step(&serde_json::json!({"click":{"handle":target}})).unwrap();
                        h.frames(2);
                        assert!(crate::scenario::handle_at("prefs:font_builtin").is_some(), "real searchable font picker opens at either width");
                    }
                }
            }
        }
    }

    #[test]
    fn sketch_short_labels_keep_the_legacy_search_terms_in_every_complete_locale() {
        let cases = [
            ("prefs.sketch_tracking_guides", "Center and midpoint alignment guides"),
            ("prefs.sketch_tracking_points", "Alignment guides from end points and corners while drawing"),
            ("prefs.sketch_recent_snap", "Auto snap to recent centers and midpoints"),
            ("prefs.sketch_align_points", "Line up with other sketch points while drawing"),
            ("ui.center_arc_follows_cursor", "Center point arc follows the cursor's direction"),
            ("Also place them from the origin (fully constrained)", "Also place them from the origin (fully constrained)"),
            ("ui.shortcut_cycles_modes", "Pressing a tool's shortcut again switches its mode"),
            ("prefs.project_options_dialog", "Project options dialog"),
            ("prefs.sketch_exit_view.previous", "Return to the previous view"),
            ("prefs.sketch_exit_view.keep", "Keep the current view"),
            ("prefs.sketch_exit_view.unless_rotated", "Return unless rotated"),
            ("prefs.sketch_state_icons", "Sketch icons in the color of their state"),
            ("prefs.sketch_conflict_colour", "Conflicting geometry in the conflict color"),
            ("prefs.hide_constraint_glyphs_zoomed_out", "Hide constraint glyphs when zoomed out"),
            ("prefs.prefer_free_sketch_points", "Prefer movable sketch points when points overlap"),
            ("prefs.select_other_hidden", "Select hidden geometry with Select Other"),
            ("prefs.sketch.whole_bodies", "Click selects whole bodies"),
        ];
        for lang in ["en", "en-GB", "ar", "de", "he", "ja", "ko", "vi", "zh"] {
            let _language = crate::i18n::scope(lang);
            for (id, legacy) in cases {
                assert!(matches_text(legacy, &[crate::i18n::tr(id)]), "{lang}: {legacy} still finds {id}");
            }
        }
    }

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

    /// A multi-word right-to-left label reaches the search in display order while the query is
    /// typed in reading order: typing the label's words as written must find the row (#10471).
    #[test]
    fn search_finds_right_to_left_labels_typed_in_reading_order() {
        let _language = crate::i18n::scope("he");
        let label = crate::i18n::tr("prefs.bodies_folders");
        assert!(matches_text("תיקיות גופים", &[label]), "the label as it is typed");
        assert!(matches_text("גופים תיקיות", &[label]), "the words swapped still match");
        assert!(matches_text("תיקיות", &[label]), "one word still matches");
        let _language = crate::i18n::scope("ar");
        let label = crate::i18n::tr("prefs.bodies_folders");
        assert!(matches_text("مجلدات الأجسام", &[label]), "the label as it is typed");
        assert!(matches_text("الأجسام مجلدات", &[label]), "the words swapped still match");
        assert!(matches_text("مجلدات", &[label]), "one word still matches");
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

    #[test]
    fn general_redesigned_controls_remain_searchable_without_duplicate_settings() {
        use egui::accesskit::Role;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
        for (query, label) in [
            ("reopen the designs", "Reopen the designs that were open last time"),
            ("autosave every", "Autosave every"),
            ("largest file to open: step", "Largest file to open: STEP and import"),
            ("largest file to open: design", "Largest file to open: Design"),
            ("allow network locations", "Allow network locations in designs"),
            ("slicer that is already running", "Open files in a slicer that is already running"),
            ("print face highlight", "Print face highlight"),
            ("interface language", "Interface language"),
        ] {
            h.app.prefs_window.filter = query.into();
            h.frames(4);
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            let at = h.widget(label).unwrap_or_else(|| panic!("{label} must match {query}"));
            assert!(body.contains(egui::pos2(at[0] as f32, at[1] as f32)), "{label} is drawn in the results");
            assert!(h.widget(h.app.tr("prefs.search_empty")).is_none(), "{query} has a real result instead of the empty-results message");
            let (_, _, hits) = nav(&h.ctx);
            let matching: Vec<_> = hits.iter().filter(|hit| hit.label == label).collect();
            assert_eq!(matching.len(), 1, "{label} must have one real search result");
            assert_eq!(matching[0].page, crate::prefs::PAGE_GENERAL);
            let control = match label {
                "Autosave every" => Some(("prefs:autosave_minutes", Role::SpinButton, "Minutes", h.app.autosave_minutes)),
                "Largest file to open: STEP and import" => {
                    Some(("prefs:max_step_file_mb", Role::SpinButton, label, h.app.preferences.max_step_file_mb as f64))
                }
                "Largest file to open: Design" => {
                    Some(("prefs:max_design_file_mb", Role::SpinButton, label, h.app.preferences.max_design_file_mb as f64))
                }
                "Print face highlight" => {
                    Some(("prefs:print_face_highlight", Role::Slider, label, f64::from(h.app.preferences.print_face_highlight)))
                }
                _ => None,
            };
            if let Some((identity, role, accessible_label, value)) = control {
                let field = crate::scenario::rect_of(identity).expect("the matching field retains its stable identity");
                assert!(body.contains_rect(field), "{identity} is drawn in the results");
                let tree = h.output.accesskit_update.as_ref().unwrap();
                // Search also measures invisible, disabled probe UIs. Count the live control.
                let controls: Vec<_> = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| !node.is_disabled() && node.role() == role && node.label() == Some(accessible_label))
                    .collect();
                assert_eq!(controls.len(), 1, "{identity} must have one real searchable control");
                let node = &controls[0].1;
                assert_eq!(node.numeric_value(), Some(value), "{identity} exposes its actual setting value");
                let bounds = node.bounds().unwrap();
                assert!(field.contains(egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32)));
            }
        }
        // An unavailable control is still a real result. Invisible search probes are also
        // disabled, so require the live navigation hit and painted label as well as metadata.
        h.app.updater.kind = Some(crate::update::InstallKind::Dev);
        assert!(!crate::update::available(&mut h.app));
        let label = "Check at start and once a day";
        h.app.prefs_window.filter = label.into();
        for checked in [false, true] {
            h.app.preferences.check_updates = checked;
            h.frames(4);
            let (_, _, hits) = nav(&h.ctx);
            let matching: Vec<_> = hits.iter().filter(|hit| hit.label == label).collect();
            assert_eq!(matching.len(), 1, "the disabled setting has one live result");
            assert_eq!(matching[0].page, crate::prefs::PAGE_GENERAL);
            assert!(h.widget(h.app.tr("prefs.search_empty")).is_none());
            let body = crate::scenario::rect_of("prefs:page_body").unwrap();
            let field = crate::scenario::rect_of("prefs:check_updates").expect("the real scheduling control");
            assert!(body.contains_rect(field));
            let tree = h.output.accesskit_update.as_ref().unwrap();
            let (_, node) = tree
                .nodes
                .iter()
                .rev()
                .find(|(_, node)| {
                    node.role() == Role::CheckBox
                        && node.label() == Some(label)
                        && node.bounds().is_some_and(|bounds| {
                            field.contains(egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32))
                        })
                })
                .expect("the disabled checkbox exposes its real state at the rendered control");
            assert!(node.is_disabled());
            assert_eq!(node.toggled(), Some(if checked { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False }));
            let painted = h
                .shapes
                .iter()
                .filter(|shape| {
                    matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == label && body.contains(text.pos) && shape.clip_rect.contains(text.pos))
                })
                .count();
            assert_eq!(painted, 1, "one visible disabled control is painted, beyond probe metadata");
            h.step(&serde_json::json!({"click":{"handle":"prefs:check_updates"}})).unwrap();
            assert_eq!(h.app.preferences.check_updates, checked, "the search result cannot edit an unavailable setting");
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
        let (_, _, hits) = nav(&h.ctx);
        let cursor = hits
            .iter()
            .position(|hit| hit.page == crate::prefs::PAGE_NAVIGATION && hit.label == "Reverse zoom direction")
            .expect("the Navigation setting remains keyboard reachable");
        set_cursor(&h.ctx, cursor);
        h.frames(2);
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
        crate::prefs::reveal_preference(&mut h, "prefs:max_step_file_mb");
        let field = crate::scenario::rect_of("prefs:max_step_file_mb").expect("STEP limit field");
        assert!(crate::scenario::rect_of("prefs:page_body").expect("Preferences body").contains_rect(field));
        h.step(&serde_json::json!({"click": [field.center().x, field.center().y]})).expect("click the field");
        h.frames(2);
        let focused = h.ctx.memory(|memory| memory.focused()).expect("the number editor has focus");
        assert!(field.contains_rect(h.ctx.read_response(focused).expect("focused editor response").rect));
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
    /// Settings drawn in a row with no label of their own are found by the words they show (#9265).
    #[test]
    fn preferences_search_finds_settings_in_rows_without_a_label() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_LAYOUT);
        h.frames(2);
        // Drawn, not merely present: a hidden row's controls still exist, far off screen.
        let drawn = |h: &crate::scenario::Harness, label: &str| {
            let body = crate::scenario::rect_of("dialog:page_body");
            h.widget(label).is_some_and(|[x, y]| body.is_none_or(|body| body.contains(egui::pos2(x as f32, y as f32))))
        };
        let rows = [
            ("status bar", "Status bar"),
            ("sketch tab shown", "Keep the Sketch tab shown"),
            ("search results", "Show search results in the ribbon"),
            ("play history", "Play History button"),
            ("view's corner", "Also in the view's corner"),
            ("highlight active", "Highlight active commands"),
            ("On hover", "On hover"),
        ];
        for (query, label) in rows {
            h.app.prefs_window.filter = query.into();
            h.frames(4);
            assert!(drawn(&h, label), "searching '{query}' should show '{label}'");
        }
        h.app.prefs_window.filter = "status bar".into();
        h.frames(4);
        assert!(!drawn(&h, "Keep the Sketch tab shown"), "a row that does not match stays out of the result");
        // Three unlabelled rows sit under one labelled row: each is found on its own words only.
        h.app.prefs_window.filter = "sketch tab shown".into();
        h.frames(4);
        assert!(drawn(&h, "Keep the Sketch tab shown"));
        assert!(!drawn(&h, "Highlight active commands"), "its neighbour is not a match");
        assert!(!drawn(&h, "Show search results in the ribbon"), "its neighbour is not a match");
        h.app.prefs_window.filter = "zzzz-no-setting".into();
        h.frames(4);
        assert!(!drawn(&h, "Status bar"), "a miss shows nothing");
    }
    #[test]
    fn design_and_tools_short_labels_keep_legacy_search_and_reveal_the_real_control() {
        for language in ["en", "ja"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.prefs", serde_json::json!({"language":language,"open":"General"}));
            for (query, id, page, handle) in [
                ("Step values with the arrow keys and the wheel", "ui.step_values", crate::prefs::PAGE_DESIGN, "prefs:step_values"),
                ("For files from unknown programs", "prefs.stand_up_unknown", crate::prefs::PAGE_DESIGN, "prefs:stand_up_unknown"),
                ("Hide origin by default", "ui.hide_origin_by_default", crate::prefs::PAGE_DESIGN, "prefs:hide_origin"),
                ("Snap to face and edge centres", "Snap to face and edge centres", crate::prefs::PAGE_TOOLS, "prefs:hole_snaps"),
                ("Move holes by clicking or dragging", "Move holes by clicking or dragging", crate::prefs::PAGE_TOOLS, "prefs:hole_moves"),
                ("Show moved parts' old place as a ghost", "prefs.design_tools.ghost", crate::prefs::PAGE_TOOLS, "prefs:drive_joints_ghost"),
                ("Near-axis rotation step (°)", "joint_move.coarse", crate::prefs::PAGE_TOOLS, "prefs:joint_rotation_coarse"),
                ("Far-axis rotation step (°)", "joint_move.fine", crate::prefs::PAGE_TOOLS, "prefs:joint_rotation_fine"),
                ("Show X/Y/Z distances", "settings.measure_axis_distances", crate::prefs::PAGE_TOOLS, "prefs:measure_axis_distances"),
                ("Show tool at cursor", "Show tool at cursor", crate::prefs::PAGE_TOOLS, "view:tool_cursor"),
            ] {
                h.app.prefs_window.filter = query.into();
                h.frames(5);
                let (_, _, hits) = nav(&h.ctx);
                let label = h.app.tr(id);
                let matches: Vec<_> = hits.iter().enumerate().filter(|(_, hit)| hit.page == page && hit.label == label).collect();
                assert_eq!(matches.len(), 1, "{language}: {query} has one real setting");
                set_cursor(&h.ctx, matches[0].0);
                h.frames(2);
                h.step(&serde_json::json!({"key":"Enter"})).unwrap();
                h.frames(3);
                assert_eq!(h.app.prefs_window.section, page);
                assert!(h.app.prefs_window.filter.is_empty());
                let body = crate::scenario::rect_of("dialog:page_body").unwrap();
                assert!(body.contains_rect(crate::scenario::rect_of(handle).unwrap()), "{language}: {query} reveals {handle}");
            }
        }
    }

    #[test]
    fn moved_preferences_search_paths_and_enter_follow_window_groups() {
        for (query, path, page) in [
            ("Direct interaction", "Work > Tools", crate::prefs::PAGE_TOOLS),
            ("Navigate with the 3D mouse", "Input > 3D mouse", crate::prefs::PAGE_MOUSE3D),
            ("Origin plane names", "Work > Design", crate::prefs::PAGE_DESIGN),
            ("Status bar", "Look > Layout", crate::prefs::PAGE_LAYOUT),
        ] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_GENERAL);
            h.app.prefs_window.filter = query.into();
            h.frames(4);
            assert!(h.widget(path).is_some(), "{query} shows {path}");
            let (_, _, hits) = nav(&h.ctx);
            let index = hits.iter().position(|hit| hit.page == page && hit.label == query).expect("moved control is keyboard reachable");
            set_cursor(&h.ctx, index);
            h.frames(2);
            h.step(&serde_json::json!({"key":"Enter"})).unwrap();
            h.frames(2);
            assert_eq!(h.app.prefs_window.section, page);
            assert!(h.app.prefs_window.filter.is_empty());
            assert!(h.widget(query).is_some());
        }
    }
}
