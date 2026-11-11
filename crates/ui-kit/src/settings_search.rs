//! Search across a settings page the kit draws; the Preferences window is SolidCraft's use of it.
//! The field is the Keyboard Shortcuts magnifier. A match is the control's own label, its
//! section, its page, or the value a dropdown is showing, scored the same way as command search.
//! English counts too, from the message catalog, so a new setting is found by being drawn —
//! nothing is registered for search.

use std::collections::{HashMap, HashSet};

use egui::{Context, Id, Pos2, Rect, Response, RichText, Sense, Ui, UiBuilder, Vec2, vec2};

use solidcraft_theme::Tokens;

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
pub struct NavHit {
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
pub struct Reveal {
    pub page: usize,
    pub label: String,
    pub until: f64,
    /// What the outlined row's name starts with: `{prefix}:revealed`.
    pub prefix: String,
}

#[derive(Clone, Default)]
struct State {
    live: bool,
    query: String,
    /// What the names this pass publishes start with: `{prefix}:search:current`, `{prefix}:revealed`.
    prefix: String,
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
pub struct SearchFrame {
    ctx: Context,
}

impl Drop for SearchFrame {
    fn drop(&mut self) {
        if finish(&self.ctx) {
            self.ctx.request_discard("preferences search");
        }
    }
}

/// Start a pass over the preferences body. An empty filter leaves every control drawn. The names
/// the pass publishes start with `prefix`.
pub fn begin(ui: &Ui, filter: &str, prefix: &str) -> SearchFrame {
    let query: String = filter.trim().chars().take(256).collect::<String>().to_lowercase();
    ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(Id::unique(STATE), State::default);
        let changed = state.memory_query != query;
        if changed {
            state.memory_query = query.clone();
            state.open_blocks.clear();
            state.seen_blocks.clear();
        }
        state.live = true;
        state.query = query;
        state.prefix.clear();
        state.prefix.push_str(prefix);
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
        let state = data.get_temp_mut_or_default::<State>(Id::unique(STATE));
        dirty = state.live && state.dirty;
        state.live = false;
    });
    dirty
}

fn with<R>(ui: &Ui, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let mut out = None;
    ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(Id::unique(STATE));
        if state.live {
            out = Some(f(state));
        }
    });
    out
}

pub fn querying(ui: &Ui) -> bool {
    with(ui, |state| !state.query.is_empty()).unwrap_or(false)
}

pub fn hits(ui: &Ui) -> u32 {
    with(ui, |state| state.hits).unwrap_or(0)
}

pub fn query(ui: &Ui) -> String {
    with(ui, |state| state.query.clone()).unwrap_or_default()
}

/// A child that does not move the parent until [`commit_search_child`] says this group matched.
/// An invisible child only counts hits, so a miss cannot paint over the setting that matched.
pub fn detached_child(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, invisible: bool) -> Ui {
    let mut builder = UiBuilder::new().id_salt(salt).max_rect(ui.available_rect_before_wrap()).layout(egui::Layout::top_down(egui::Align::Min));
    if invisible {
        builder = builder.invisible();
    }
    ui.new_child(builder)
}

/// Keep the child's layout only when it found something. An empty page or section leaves no gap.
pub fn commit_search_child(ui: &mut Ui, child: &Ui, before: u32) {
    if hits(ui) > before {
        place_search_child(ui, child);
    }
}

/// The group is already known to match. Keep whatever it drew.
pub fn place_search_child(ui: &mut Ui, child: &Ui) {
    let rect = child.min_rect();
    if rect.height().is_finite() && rect.height() > 0.0 && rect.width().is_finite() {
        ui.advance_cursor_after_rect(rect);
    }
}

pub struct SearchLayoutMark {
    page_shown: bool,
    section_shown: String,
    hits: u32,
    heading_open: bool,
    child_matched: bool,
}

pub fn search_layout_mark(ui: &Ui) -> SearchLayoutMark {
    with(ui, |state| SearchLayoutMark {
        page_shown: state.page_shown,
        section_shown: state.section_shown.clone(),
        hits: state.hits,
        heading_open: state.heading_open,
        child_matched: state.child_matched,
    })
    .unwrap_or(SearchLayoutMark { page_shown: false, section_shown: String::new(), hits: 0, heading_open: false, child_matched: false })
}

pub fn restore_search_layout(ui: &Ui, mark: SearchLayoutMark) {
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
pub fn rewind_search_layout(ui: &Ui) {
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

pub fn shows_all(ui: &Ui) -> bool {
    with(ui, |state| state.section_matches || state.block_matches || state.force > 0 || state.heading_open || state.passthrough > 0).unwrap_or(false)
}

/// Stray notes, figures and buttons. A matching row opts out with [`push_passthrough`].
pub fn suppress_chrome(ui: &Ui) -> bool {
    querying(ui) && !shows_all(ui)
}

pub fn set_heading_open(ui: &Ui, open: bool) {
    with(ui, |state| state.heading_open = open);
}

pub fn push_passthrough(ui: &Ui) {
    with(ui, |state| state.passthrough = state.passthrough.saturating_add(1));
}

pub fn pop_passthrough(ui: &Ui) {
    with(ui, |state| state.passthrough = state.passthrough.saturating_sub(1));
}

pub fn set_force_scroll(ctx: &Context, on: bool) {
    ctx.data_mut(|data| data.insert_temp(Id::unique(SCROLL), on));
}

pub fn force_scroll(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp(Id::unique(SCROLL))).unwrap_or(false)
}

pub fn begin_page(ui: &Ui, index: usize, label: &str) {
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

pub fn begin_section(ui: &Ui, title: &str) {
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

pub struct BlockGate {
    key: String,
    discover: bool,
    pub show: bool,
    /// The block is a result in its own right: its label or hint, its section or a forced pass
    /// matches. Such a block counts as a hit even when the control inside it (a plain number
    /// field) does not report one.
    pub matched: bool,
}

/// `Some` while a search is active. `show` draws the block; otherwise the caller runs it hidden
/// so a dropdown value inside can still be learned.
pub fn block_gate(ui: &Ui, label: &str, hint: Option<&str>) -> Option<BlockGate> {
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
        BlockGate { key, discover, show, matched: state.block_matches }
    })
}

/// How many keyboard stops this pass has painted so far, to tell whether a block's controls
/// registered one of their own.
pub fn nav_len(ui: &Ui) -> usize {
    with(ui, |state| state.nav.len()).unwrap_or(0)
}

pub fn block_finish(ui: &Ui, gate: &BlockGate, showed: bool, hits_before: u32) {
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

pub fn push_force(ui: &Ui) {
    with(ui, |state| state.force = state.force.saturating_add(1));
}

pub fn pop_force(ui: &Ui) {
    with(ui, |state| state.force = state.force.saturating_sub(1));
}

pub fn set_control(ui: &Ui, label: &str) {
    with(ui, |state| state.key = format!("{}\n{}\n{}", state.page, state.section, label));
}

/// Key a row that has no label of its own by its place in the section, so two of them do not share
/// one stored text. Returns the text to match the row on (still empty).
pub fn set_unlabelled_control(ui: &Ui) {
    with(ui, |state| {
        state.unlabelled_rows += 1;
        state.key = format!("{}\n{}\n#{}", state.page, state.section, state.unlabelled_rows);
    });
}

pub fn push_unlabelled(ui: &Ui) {
    with(ui, |state| state.unlabelled = state.unlabelled.saturating_add(1));
}

pub fn pop_unlabelled(ui: &Ui) {
    with(ui, |state| state.unlabelled = state.unlabelled.saturating_sub(1));
}

/// A control inside a row with no label says what it is. Stored for the row, so the next pass can
/// match the row on those words and draw it, as it does for a dropdown's current value.
pub fn note_unlabelled(ui: &Ui, texts: &[&str]) {
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
pub fn visible(ui: &mut Ui, texts: &[&str]) -> bool {
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
pub fn note_current(ui: &Ui, text: &str) {
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

/// Keep per-option aliases searchable when a compound row learns its choices.
pub fn note_choices(ui: &Ui, choices: &[&str]) {
    let mut text = choices.join(" ");
    if texts_hit(ui, choices) {
        text.push(' ');
        text.push_str(&query(ui));
    }
    note_current(ui, &text);
}

/// After a hidden run, show the control on the next pass if its value now matches.
pub fn recheck(ui: &Ui, texts: &[&str]) {
    if wanted(ui, texts) {
        with(ui, |state| {
            state.child_matched = true;
            state.dirty = true;
        });
    }
}

pub fn reveal(ui: &mut Ui) {
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
        // A setting with the section's name owns its search stop and reveal target.
        let setting_owns_label = with(ui, |state| state.key.rsplit('\n').next() == Some(section.as_str())).unwrap_or(false);
        if !setting_owns_label {
            remember(ui, &section, response.rect);
        }
    }
}

pub fn hidden_response(ui: &mut Ui) -> Response {
    let mut child = ui.new_child(UiBuilder::new().invisible().max_rect(Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0))));
    child.allocate_response(Vec2::ZERO, Sense::hover())
}

pub fn run_hidden<R>(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
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
pub fn texts_hit(ui: &Ui, texts: &[&str]) -> bool {
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
        || setting_score(&solidcraft_ui_text::bidi::visual(query), text) > 0
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
pub fn matches_text(query: &str, texts: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    texts.iter().any(|text| text_matches(&query, text))
}

/// Words that find a setting even though they are not written on it.
fn keywords(text: &str) -> &'static [&'static str] {
    let sketch: &'static [&'static str] = match fold(text).as_str() {
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
        _ => &[],
    };
    if !sketch.is_empty() {
        return sketch;
    }
    match fold(text).as_str() {
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
        "theme" => &["dark", "light", "appearance"],
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
        "axes" | "axes (signed)" => &["axes", "signed axes", "axes (signed)"],
        "cube" | "cube (compact)" => &["cube", "cube (compact)"],
        "under cursor" => &["under the cursor"],
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
        let lang = solidcraft_ui_text::language().to_string();
        let mut slot = slot.borrow_mut();
        if slot.0 != lang {
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            let locale = solidcraft_ui_text::Locale::for_language(&lang);
            solidcraft_ui_text::each_english_message(|id, english| {
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
pub fn shift_key_sink(ctx: &Context) -> bool {
    let held = ctx.data(|data| data.get_temp::<bool>(Id::unique(SINK_NOW))).unwrap_or(false);
    ctx.data_mut(|data| data.insert_temp(Id::unique(SINK_NOW), false));
    held
}

pub fn key_sink(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(Id::unique(SINK_PREV))).unwrap_or(false)
}

pub fn note_key_sink(ctx: &Context) {
    ctx.data_mut(|data| data.insert_temp(Id::unique(SINK_NOW), true));
}

pub fn set_key_sink_prev(ctx: &Context, held: bool) {
    ctx.data_mut(|data| data.insert_temp(Id::unique(SINK_PREV), held));
}

/// The matches drawn last frame, for Up/Down and Enter before this pass rebuilds them.
pub fn nav(ctx: &Context) -> (String, usize, Vec<NavHit>) {
    ctx.data(|data| {
        let store = data.get_temp::<NavStore>(Id::unique(NAV)).unwrap_or_default();
        (store.query, store.cursor, store.hits)
    })
}

pub fn set_cursor(ctx: &Context, cursor: usize) {
    ctx.data_mut(|data| {
        let store = data.get_temp_mut_or_default::<NavStore>(Id::unique(NAV));
        store.scroll = store.cursor != cursor;
        store.cursor = cursor;
    });
}

/// Keep this pass's matches for the next frame's keys.
pub fn store_nav(ui: &Ui) {
    let (query, hits) =
        with(ui, |state| (state.query.clone(), state.nav.iter().map(|hit| NavHit { page: hit.page, label: hit.label.clone() }).collect::<Vec<_>>()))
            .unwrap_or_default();
    ui.ctx().data_mut(|data| {
        let store = data.get_temp_mut_or_default::<NavStore>(Id::unique(NAV));
        store.query = query;
        store.hits = hits;
        if store.cursor >= store.hits.len() {
            store.cursor = 0;
        }
    });
}

/// A row whose own words matched. Section titles and controls both call this once they have a rect.
pub fn remember(ui: &mut Ui, label: &str, rect: Rect) {
    remember_matched(ui, label, None, rect);
}

/// [`remember`] for a row whose hint can match too: a row found only by its hint is still a
/// stop for the arrow keys, and its label stays the target Enter opens.
pub fn remember_matched(ui: &mut Ui, label: &str, hint: Option<&str>, rect: Rect) {
    if !ui.is_visible() || !querying(ui) || label.is_empty() || !(text_hit(ui, label) || hint.is_some_and(|hint| text_hit(ui, hint))) {
        return;
    }
    if with(ui, |state| state.hiding > 0).unwrap_or(true) {
        return;
    }
    let (cursor, scroll) = ui.ctx().data(|data| {
        let store = data.get_temp::<NavStore>(Id::unique(NAV)).unwrap_or_default();
        (store.cursor, store.scroll)
    });
    let selected = with(ui, |state| {
        let selected = state.nav.len() == cursor;
        state.nav.push(PaintedHit { page: state.page_index, label: label.to_string() });
        selected.then(|| state.prefix.clone())
    })
    .flatten();
    let Some(prefix) = selected else { return };
    let t = Tokens::get();
    ui.painter().rect_stroke(rect, t.radius_sm, egui::Stroke::new(t.space_2, t.form_primary), egui::StrokeKind::Outside);
    // Only when Up/Down moved the cursor: following it every frame pins the list to the match.
    if scroll {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
    solidcraft_ui_registry::publish_handle(&format!("{prefix}:search:current"), rect.center());
}

pub fn set_reveal(ctx: &Context, reveal: Option<Reveal>) {
    ctx.data_mut(|data| {
        if let Some(reveal) = reveal {
            data.insert_temp(Id::unique(REVEAL_ID), reveal);
        } else {
            data.remove::<Reveal>(Id::unique(REVEAL_ID));
        }
    });
}

pub fn reveal_for(page: usize, label: &str, prefix: &str) -> Reveal {
    Reveal { page, label: label.to_string(), until: now_ms() + REVEAL_MS, prefix: prefix.to_string() }
}

impl Reveal {
    /// Whether the outline's time is up, on the same clock that set [`Reveal::until`].
    pub fn expired(&self) -> bool {
        now_ms() > self.until
    }
}

/// Scroll the opened setting into view and outline it until [`Reveal::until`].
pub fn follow(ui: &Ui, label: &str, rect: Rect) {
    let Some(reveal) = ui.data(|data| data.get_temp::<Reveal>(Id::unique(REVEAL_ID))) else { return };
    if reveal.expired() || fold(label) != fold(&reveal.label) {
        return;
    }
    let page = with(ui, |state| state.page_index);
    if page.is_some_and(|page| page != reveal.page) {
        return;
    }
    ui.scroll_to_rect(rect, Some(egui::Align::Center));
    let t = Tokens::get();
    ui.painter().rect_stroke(rect, t.radius_sm, egui::Stroke::new(t.space_2, t.form_primary), egui::StrokeKind::Outside);
    solidcraft_ui_registry::publish_handle(&format!("{}:revealed", reveal.prefix), rect.center());
}

/// Wall clock in milliseconds, for how long a reveal stays outlined.
fn now_ms() -> f64 {
    use web_time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two pages of a host's settings window.
    const PAGE_LAYOUT: usize = 2;
    const PAGE_GRAPHICS: usize = 3;

    /// The search field and its empty result name the window they search (#8318): "Search
    /// settings" under a "Preferences" title read as two names for one window.
    #[test]
    fn shared_standalone_cards_and_chips_filter_index_and_reveal_their_bounds() {
        use crate::Card;
        for kind in 0..4 {
            let ctx = crate::test_support::context();
            let mut target = Rect::NOTHING;
            let mut miss = Rect::NOTHING;
            let mut draw = |filter: &str| {
                crate::test_support::raw_frame(&ctx, Default::default(), |ui| {
                    let _search = begin(ui, filter, "prefs");
                    begin_page(ui, PAGE_LAYOUT, "Layout");
                    for label in ["Performance", "Quality"] {
                        let card =
                            Card { label, sub: None, selected: false, enabled: false, options: None, check: false, aspect: 1.0, figure: &|_, _| {} };
                        let response = match kind {
                            0 => crate::picture_card(ui, &card, 100.0),
                            1 => crate::toggle_tile(ui, &card, 100.0),
                            2 => crate::preset_card(ui, label, "", false, false, &|_, _, _| {}, 100.0),
                            _ => crate::settings_chip(ui, label, Default::default()),
                        };
                        assert!(!response.clicked());
                        if label == "Quality" {
                            target = response.rect;
                        } else {
                            miss = response.rect;
                        }
                    }
                    store_nav(ui);
                });
            };
            draw("Quality");
            assert!(!miss.is_positive(), "non-matching control {kind} has no layout");
            assert!(target.is_positive());
            let (_, _, hits) = nav(&ctx);
            assert_eq!(hits.len(), 1, "control {kind} owns one real keyboard target");
            assert_eq!((hits[0].page, hits[0].label.as_str()), (PAGE_LAYOUT, "Quality"));
            set_reveal(&ctx, Some(Reveal { page: PAGE_LAYOUT, label: "Quality".into(), until: f64::MAX, prefix: "prefs".into() }));
            solidcraft_ui_registry::clear_handles();
            // The search is cleared before the selected setting is revealed.
            crate::test_support::raw_frame(&ctx, Default::default(), |ui| {
                let _search = begin(ui, "", "prefs");
                begin_page(ui, PAGE_LAYOUT, "Layout");
                let card = Card {
                    label: "Quality",
                    sub: None,
                    selected: false,
                    enabled: false,
                    options: None,
                    check: false,
                    aspect: 1.0,
                    figure: &|_, _| {},
                };
                let response = match kind {
                    0 => crate::picture_card(ui, &card, 100.0),
                    1 => crate::toggle_tile(ui, &card, 100.0),
                    2 => crate::preset_card(ui, "Quality", "", false, false, &|_, _, _| {}, 100.0),
                    _ => crate::settings_chip(ui, "Quality", Default::default()),
                };
                assert_eq!(solidcraft_ui_registry::handle_at("prefs:revealed"), Some(response.rect.center()));
            });
        }
    }

    #[test]
    fn shared_picture_choices_index_the_row_or_option_once_and_learn_aliases() {
        use crate::PictureChoice;
        let choices =
            [PictureChoice { key: "on", label: "Anti-aliasing", figure: |_, _| {} }, PictureChoice { key: "off", label: "Off", figure: |_, _| {} }];
        for (query, expected) in [("Sampling", "Sampling"), ("Anti-aliasing", "Anti-aliasing"), ("msaa", "Anti-aliasing")] {
            let ctx = crate::test_support::context();
            for _ in 0..4 {
                crate::test_support::raw_frame(&ctx, Default::default(), |ui| {
                    let _search = begin(ui, query, "prefs");
                    begin_page(ui, PAGE_GRAPHICS, "Graphics");
                    assert_eq!(crate::settings_picture_choices(ui, "shared", "Sampling", &choices, 0, true), None);
                    store_nav(ui);
                });
            }
            let (_, _, hits) = nav(&ctx);
            assert_eq!(hits.len(), 1, "{query} has one target");
            assert_eq!(hits[0].label, expected);
            assert_eq!(hits[0].page, PAGE_GRAPHICS);
            set_reveal(&ctx, Some(Reveal { page: PAGE_GRAPHICS, label: expected.into(), until: f64::MAX, prefix: "prefs".into() }));
            crate::test_support::raw_frame(&ctx, Default::default(), |ui| {
                let _search = begin(ui, "", "prefs");
                begin_page(ui, PAGE_GRAPHICS, "Graphics");
                crate::settings_picture_choices(ui, "shared", "Sampling", &choices, 0, true);
                let rect = solidcraft_ui_registry::rect_of(if expected == "Sampling" { "dialog:row:Sampling" } else { "prefs:shared:on" }).unwrap();
                assert_eq!(solidcraft_ui_registry::handle_at("prefs:revealed"), Some(rect.center()));
            });
        }
    }
}
