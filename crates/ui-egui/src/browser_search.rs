//! The browser's search (#1413, in @Nathan-Dane's compact design #1634): a magnifier in the Browser
//! panel's header (a header action of the shared panel host) puts a field in the header in the
//! title's place, with Ctrl+F giving it the keyboard and Esc closing it. It keeps only the rows whose
//! names match, each with its parents (dimmed, opened for the search); the field's filter ticks the
//! kinds to list, each with how many match, and a line under the header (only once there is a
//! search) counts the matches and offers Select all / Hide others, says how many Hide others hid
//! with an Undo, or says that nothing matched. The matching is the engine's ([`browser_find`]), the
//! same as the `browser.find` command's.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use egui::{Pos2, vec2};
use serde_json::json;
use solidcraft_engine::browser_find::{self, Kind, MAX_PATTERN_CHARS, Pattern, Search};
use solidcraft_engine::doc::{Document, FeatureKind, ModelState};
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::ui_kit::{BackingTooltipExt as _, Item};
use solidcraft_theme::Tokens;

/// The search, kept with the browser's state.
#[derive(Default)]
pub struct BrowserSearch {
    /// What is typed.
    pub text: String,
    /// The kinds ticked (none: every kind).
    pub kinds: BTreeSet<Kind>,
    /// Rows closed during this search; the tree's own folds are left as they were.
    closed: BTreeSet<String>,
    /// What the search found (`None`: no search).
    hits: Option<Hits>,
    /// The field was asked for (the magnifier or Ctrl+F).
    shown: bool,
    /// The field takes the keyboard on the next frame.
    focus: bool,
    /// How many bodies the last Hide others hid, and the design's revision right after it.
    hid: Option<(usize, u64)>,
}

impl BrowserSearch {
    /// A search is on: there is text, or a kind is ticked.
    pub fn on(&self) -> bool {
        self.hits.is_some()
    }

    /// How many bodies Hide others hid, while it is still the last change to the design: an
    /// undo or any other step ends it.
    pub fn hid(&self, session: &Session) -> Option<usize> {
        self.hid.filter(|(_, revision)| *revision == session.revision).map(|(count, _)| count)
    }

    pub(crate) fn hits(&self) -> Option<&Hits> {
        self.hits.as_ref()
    }

    /// Ends the search: the tree is shown as it was, and a bar that was asked for goes.
    pub fn clear(&mut self) {
        *self = BrowserSearch::default();
    }

    /// Asks for the bar, with the keyboard in its field.
    pub fn show(&mut self) {
        self.shown = true;
        self.focus = true;
    }

    pub(crate) fn closed(&self, key: &str) -> bool {
        self.closed.contains(key)
    }

    /// Lifts a row's mark from being closed in this search, so a reveal can open it.
    pub(crate) fn open(&mut self, key: &str) {
        self.closed.remove(key);
    }

    pub(crate) fn toggle_closed(&mut self, key: &str) {
        if !self.closed.remove(key) {
            self.closed.insert(key.to_string());
        }
    }
}

/// What a search found, arranged for drawing the tree.
pub(crate) struct Hits {
    /// The text and kinds it was found for, and the text as a pattern (`None`: kinds only).
    text: String,
    kinds: BTreeSet<Kind>,
    pattern: Option<Pattern>,
    /// What it looked through, as the browser's model keeps them (`browser_model`): the document,
    /// the evaluated bodies, and the sketch being edited (that hides bodies removed later). Held,
    /// so a change makes new ones; showing or hiding something changes none of them.
    doc: Arc<Document>,
    state: Arc<ModelState>,
    sketch: Option<u64>,
    pub(crate) search: Search,
    /// The matching rows of each placement (the design itself at the empty path): their keys
    /// in Bodies, Sketches and Construction.
    rows: HashMap<Vec<u64>, [HashSet<String>; 3]>,
    /// The placements listed: the matching ones and every one that holds a match.
    placements: HashSet<Vec<u64>>,
    /// The placements that match themselves.
    matched: HashSet<Vec<u64>>,
    /// The placements with something listed beneath them (a match inside, or a placement that
    /// holds one): only those fold during a search.
    holds: HashSet<Vec<u64>>,
    joints: HashSet<u64>,
}

fn folder_index(folder: &str) -> usize {
    match folder {
        "bodies" => 0,
        "sketches" => 1,
        _ => 2,
    }
}

impl Hits {
    fn new(text: String, kinds: BTreeSet<Kind>, pattern: Option<Pattern>, app: &SolveApp, search: Search) -> Hits {
        let mut hits = Hits {
            text,
            kinds,
            pattern,
            doc: app.session.doc.clone(),
            state: app.session.model.state(),
            sketch: app.session.active_sketch,
            search: Search::default(),
            rows: HashMap::new(),
            placements: HashSet::new(),
            matched: HashSet::new(),
            holds: HashSet::new(),
            joints: HashSet::new(),
        };
        for f in &search.found {
            for depth in 1..=f.path.len() {
                if let Some(at) = f.path.get(..depth) {
                    hits.placements.insert(at.to_vec());
                    if depth < f.path.len() {
                        hits.holds.insert(at.to_vec());
                    }
                }
            }
            let folder = match f.kind {
                Kind::Component => {
                    hits.matched.insert(f.path.clone());
                    continue;
                }
                Kind::Joint => {
                    hits.joints.extend(f.key.parse::<u64>().ok());
                    continue;
                }
                Kind::Body => 0,
                Kind::Sketch => 1,
                Kind::Construction => 2,
            };
            hits.holds.insert(f.path.clone());
            if let Some(keys) = hits.rows.entry(f.path.clone()).or_default().get_mut(folder) {
                keys.insert(f.key.clone());
            }
        }
        hits.search = search;
        hits
    }

    fn is_for(&self, text: &str, kinds: &BTreeSet<Kind>, app: &SolveApp) -> bool {
        self.text == text
            && self.kinds == *kinds
            && self.sketch == app.session.active_sketch
            && Arc::ptr_eq(&self.doc, &app.session.doc)
            && Arc::ptr_eq(&self.state, &app.session.model.state())
    }

    pub(crate) fn total(&self) -> usize {
        self.search.total
    }

    /// Where the typed text matches `name`, to draw that part strong (none for a wildcard search,
    /// which matches the whole name).
    pub(crate) fn span(&self, name: &str) -> Option<std::ops::Range<usize>> {
        self.pattern.as_ref()?.span(name)
    }

    /// Is the row with `key` in a folder (bodies, sketches or construction) of the placement at
    /// `path` (empty: the design itself) a match?
    pub(crate) fn row(&self, path: &[u64], folder: &str, key: &str) -> bool {
        self.rows.get(path).and_then(|f| f.get(folder_index(folder))).is_some_and(|keys| keys.contains(key))
    }

    /// Does the search list anything in this folder of the placement at `path`?
    pub(crate) fn folder(&self, path: &[u64], folder: &str) -> bool {
        self.rows.get(path).and_then(|f| f.get(folder_index(folder))).is_some_and(|keys| !keys.is_empty())
    }

    /// Is the placement at `path` listed: it matches, or holds a match?
    pub(crate) fn lists(&self, path: &[u64]) -> bool {
        self.placements.contains(path)
    }

    pub(crate) fn matches(&self, path: &[u64]) -> bool {
        self.matched.contains(path)
    }

    /// Does the placement at `path` list anything beneath it?
    pub(crate) fn holds(&self, path: &[u64]) -> bool {
        self.holds.contains(path)
    }

    /// Did the search find this body? By its name in the world, as a pick in the view names it.
    pub(crate) fn found_body(&self, world: &str) -> bool {
        self.search.found.iter().any(|f| matches!(&f.item, Some(Sel::Body { name }) if name == world))
    }

    pub(crate) fn joint(&self, id: u64) -> bool {
        self.joints.contains(&id)
    }

    /// Something Hide others can keep shown: a body, or a placement (with its bodies).
    fn shows_bodies(&self) -> bool {
        self.search.found.iter().any(|f| matches!(f.kind, Kind::Body | Kind::Component))
    }
}

/// Brings what the search found up to date with the text, the kinds and the design: a new
/// search only when one of them changed.
pub(crate) fn refresh(app: &mut SolveApp) {
    let search = &app.tree.search;
    if search.hits.as_ref().is_some_and(|h| h.is_for(&search.text, &search.kinds, app)) {
        return;
    }
    let pattern = Pattern::new(&search.text);
    if pattern.is_none() && search.kinds.is_empty() {
        app.tree.search.hits = None;
        app.tree.search.closed.clear();
        return;
    }
    // Another text or other kinds is another search: the rows it closed open again, and what Hide
    // others did belongs to the search before.
    if search.hits.as_ref().is_none_or(|h| h.text != search.text || h.kinds != search.kinds) {
        app.tree.search.closed.clear();
        app.tree.search.hid = None;
    }
    let (text, kinds) = (app.tree.search.text.clone(), app.tree.search.kinds.clone());
    let found = browser_find::find(&app.session, pattern.as_ref(), &kinds.iter().copied().collect::<Vec<_>>());
    app.tree.search.hits = Some(Hits::new(text, kinds, pattern, app, found));
}

/// The kinds the design has (an entry for each), and the ticked ones even when they have none left.
fn kinds_present(app: &SolveApp) -> Vec<Kind> {
    let doc = &app.session.doc;
    let has = |k: Kind| match k {
        Kind::Component => !doc.occurrences.is_empty(),
        Kind::Body => !app.session.model.state().bodies.is_empty(),
        Kind::Sketch => doc.features.iter().any(|f| matches!(f.kind, FeatureKind::Sketch { .. })),
        Kind::Construction => doc.features.iter().any(|f| f.kind.is_construction()),
        Kind::Joint => !doc.assembly.joints.is_empty(),
    };
    Kind::ALL.into_iter().filter(|k| has(*k) || app.tree.search.kinds.contains(k)).collect()
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Component => solidcraft_ui_text::tr("ui.browser_search_components"),
        Kind::Body => solidcraft_ui_text::tr("ui.bodies"),
        Kind::Sketch => solidcraft_ui_text::tr("ui.sketches"),
        Kind::Construction => solidcraft_ui_text::tr("ui.browser_search_construction"),
        Kind::Joint => solidcraft_ui_text::tr("ui.joints"),
    }
}

/// "1 body", "12 bodies": the Browser's header status (#1634), the bodies the design shows in the world.
pub(crate) fn body_count(session: &Session) -> String {
    let count = session.world_state().map_or(0, |world| world.bodies.len());
    solidcraft_ui_text::format(if count == 1 { "ui.browser_bodies_one" } else { "ui.browser_bodies_other" }, &[("count", &count.to_string())])
}

/// The narrowest the header's box gets (`field_row`): the magnifier, a field, the filter and the ×,
/// with the inset and spacing; 136 at the default tokens, so it fits the
/// default 250-wide Browser beside its actions. Narrower headers move their actions to a row below
/// (the panel host's rule for a long title).
pub(crate) fn header_min_width() -> f32 {
    let t = Tokens::get();
    t.space_24 * 5.0 + t.space_4 * 4.0
}

/// The field is in the panel's header this frame (`ContentSlot::Title`): the body has no field row.
pub(crate) fn in_header(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(header_pass_id())) == Some(pass)
}

fn header_pass_id() -> egui::Id {
    egui::Id::unique("browser_search_in_header")
}

/// The search field in the Browser panel's header, in the title's place (`ContentSlot::Title`).
pub(crate) fn header_field(app: &mut SolveApp, ui: &mut egui::Ui) {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| d.insert_temp(header_pass_id(), pass));
    let rect = ui.max_rect();
    field_row(app, ui, rect);
}

/// The field's row is there once asked for, and while a search is on, which only the field can
/// end. Never in Render, which has its own search.
pub(crate) fn bar_shown(app: &SolveApp) -> bool {
    let search = &app.tree.search;
    !crate::render_workspace::active(app) && (search.shown || search.on())
}

/// The search box in `rect`, in @Nathan-Dane's design: the magnifier, the field, the filter of kinds
/// and the ×, all inside one frame inset from the panel's edge. `rect` is the Browser panel's header,
/// or its first row when the host can't give the header (a Browser grouped with other panels). The
/// magnifier, the × or Esc ends the search and takes the box away.
pub(crate) fn field_row(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect) {
    let t = Tokens::get();
    refresh(app);
    crate::scenario::publish_rect("browser:search:bar", rect);
    let frame = egui::Rect::from_min_max(
        egui::pos2(rect.left() + t.space_6, rect.center().y - t.form_close_size / 2.0),
        egui::pos2(rect.right().max(rect.left() + t.space_6), rect.center().y + t.form_close_size / 2.0),
    );
    // One id wherever it is drawn: a field that moves into the header keeps the keyboard.
    let id = egui::Id::unique("browser_search_field");
    let end = solidcraft_ui_text::tr("ui.browser_search_end");
    let search = crate::ui_kit::SearchBox::frame(ui, frame, id, t.radius_sm, Some(end), "browser");
    let button = t.space_24;
    let trailing = 2.0 * button + t.space_2;
    let hint = solidcraft_ui_text::tr("ui.browser_search_hint");
    let field = search.editor(ui, &mut app.tree.search.text, hint, MAX_PATTERN_CHARS, trailing);
    crate::scenario::publish_handle("browser:search", field.rect.center());
    crate::scenario::publish_rect("browser:search", field.rect);
    if std::mem::take(&mut app.tree.search.focus) {
        field.request_focus();
    }
    // Esc in the field ends the search and closes it (egui gives up the focus on Esc).
    let escaped = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
    field.backing_on_hover_text(solidcraft_ui_text::tr("ui.browser_search_tip"));
    let magnifier = search.magnifier.as_ref().is_some_and(egui::Response::clicked);
    if let Some(m) = &search.magnifier {
        crate::scenario::publish_handle("browser:magnifier", m.rect.center());
    }
    let at =
        |n: f32| egui::Rect::from_center_size(egui::pos2(frame.right() - t.space_2 - button * (n - 0.5), frame.center().y), vec2(button, button));
    kind_filter(app, ui, at(2.0));
    let clear = crate::ui_kit::icon_button_at(ui, at(1.0), "close", solidcraft_ui_text::tr("ui.browser_search_clear"));
    crate::scenario::publish_handle("browser:search:clear", clear.rect.center());
    if clear.clicked() || escaped || magnifier {
        app.tree.search.clear();
    }
}

/// The box's filter in `rect`: a list of the design's kinds to tick, each with how many match whether
/// ticked or not, and All kinds to clear them; a badge on the button counts the kinds ticked.
fn kind_filter(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect) {
    let ticked = app.tree.search.kinds.len();
    let filter = crate::ui_kit::filter_button_at(ui, rect, "find_kinds", solidcraft_ui_text::tr("ui.browser_search_kinds"), ticked, |ui| {
        // Only while the list is open: the field is there on every frame.
        let all = crate::ui_kit::text_button(ui, solidcraft_ui_text::tr("ui.browser_search_all_kinds"), ticked > 0);
        crate::scenario::publish_handle("browser:search:kind:all", all.rect.center());
        if all.clicked() {
            app.tree.search.kinds.clear();
        }
        for kind in kinds_present(app) {
            // How many match, once there is a search to count.
            let label = match app.tree.search.hits() {
                Some(hits) => format!("{} ({})", kind_label(kind), hits.search.count(kind)),
                None => kind_label(kind).to_string(),
            };
            let mut on = app.tree.search.kinds.contains(&kind);
            let r = crate::ui_kit::checkbox(ui, &label, &mut on);
            crate::scenario::publish_handle(&format!("browser:search:kind:{}", kind.id()), r.rect.center());
            if r.changed() {
                if on {
                    app.tree.search.kinds.insert(kind);
                } else {
                    app.tree.search.kinds.remove(&kind);
                }
            }
        }
    });
    crate::scenario::publish_handle("browser:search:kinds", filter.response.rect.center());
    crate::scenario::publish_rect("browser:search:kinds", filter.response.rect);
    if let Some(badge) = filter.badge {
        crate::scenario::publish_handle("browser:search:kinds:badge", badge.center());
    }
}

/// Under the header, only once there is a search: how many it found, with Select all and Hide
/// others as links; after Hide others, how many it hid, with Undo; or, when nothing matches, what
/// was looked for, a hint on `*` and `?`, and End search. The matches themselves are the tree below.
pub(crate) fn results(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    // This frame's typing and kinds.
    refresh(app);
    let Some(hits) = app.tree.search.hits() else { return };
    let (total, listed, more) = (hits.total(), hits.search.found.len(), hits.search.incomplete);
    // Hide others would hide matches the list left out.
    let hide = hits.shows_bodies() && hits.search.complete();
    crate::scenario::publish_rect("browser:search:results", ui.max_rect());
    ui.spacing_mut().item_spacing = vec2(t.space_4, t.space_4);
    if total == 0 {
        nothing_found(app, ui);
        return;
    }
    let count =
        solidcraft_ui_text::format(if more { "ui.browser_search_found_more" } else { "ui.browser_search_found" }, &[("count", &total.to_string())]);
    let hid = app.tree.search.hid(&app.session);
    // Wrapped, so a long translation moves a link to the next line rather than cutting it off; a
    // label stays on one line, so the whole link moves, never half its text.
    ui.horizontal_wrapped(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        // The links are plain text: a wider gap keeps "Select all" and "Hide others" two actions.
        ui.spacing_mut().item_spacing.x = t.space_12;
        if let Some(hid) = hid {
            let key = if hid == 1 { "ui.browser_search_hid_one" } else { "ui.browser_search_hid_other" };
            crate::ui_kit::text(
                ui,
                &solidcraft_ui_text::format(key, &[("count", &hid.to_string())]),
                crate::ui_kit::Text::Note("browser_search_hid"),
            );
            let r = crate::ui_kit::text_button(ui, solidcraft_ui_text::tr("ui.undo"), true);
            crate::scenario::publish_handle("browser:search:undo", r.rect.center());
            if r.clicked() {
                let _ = app.run("edit.undo", json!({}));
                app.tree.search.hid = None;
            }
            return;
        }
        crate::ui_kit::text(ui, &count, crate::ui_kit::Text::Note("browser_search_count"));
        // What it selects: the listed matches.
        let label = solidcraft_ui_text::format("ui.browser_select_all", &[("count", &listed.to_string())]);
        let r = crate::ui_kit::text_button(ui, &label, true);
        crate::scenario::publish_handle("browser:search:select-all", r.rect.center());
        crate::scenario::publish_rect("browser:search:select-all", r.rect);
        if r.backing_on_hover_text(solidcraft_ui_text::tr("ui.browser_select_all_tip")).clicked() {
            select_found(app);
        }
        let r = crate::ui_kit::text_button(ui, solidcraft_ui_text::tr("ui.browser_hide_others"), hide);
        crate::scenario::publish_handle("browser:search:hide-others", r.rect.center());
        crate::scenario::publish_rect("browser:search:hide-others", r.rect);
        if r.backing_on_hover_text(solidcraft_ui_text::tr("ui.browser_hide_others_tip")).clicked() {
            hide_others(app);
        }
    });
    ui.add_space(t.space_4);
}

/// Nothing matched: what was looked for, how to match part of a name, and End search.
fn nothing_found(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.vertical_centered(|ui| {
        ui.add_space(t.space_8);
        let text = app.tree.search.text.trim();
        // With kinds ticked, another kind may still have the name: say only that none matched.
        let said = if text.is_empty() || !app.tree.search.kinds.is_empty() {
            solidcraft_ui_text::tr("ui.browser_search_none").to_string()
        } else {
            solidcraft_ui_text::format("ui.browser_search_nothing_named", &[("text", text)])
        };
        crate::ui_kit::text(ui, &said, crate::ui_kit::Text::Note("browser_search_none"));
        crate::ui_kit::text(ui, solidcraft_ui_text::tr("ui.browser_search_wildcards"), crate::ui_kit::Text::Note("browser_search_wildcards"));
        let r = crate::ui_kit::button(ui, solidcraft_ui_text::tr("ui.browser_search_end"), crate::ui_kit::Button::Small);
        crate::scenario::publish_handle("browser:search:end", r.rect.center());
        if r.clicked() {
            app.tree.search.clear();
        }
        ui.add_space(t.space_8);
    });
}

/// Select all: everything the search found (joints are picked in the tree).
pub(crate) fn select_found(app: &mut SolveApp) {
    let Some(hits) = app.tree.search.hits() else { return };
    let mut items: Vec<Sel> = Vec::new();
    // A sketch or plane of a component is listed in every placement of it, as one item.
    let (mut features, mut planes) = (HashSet::new(), HashSet::new());
    let mut joints = Vec::new();
    for f in &hits.search.found {
        match &f.item {
            Some(Sel::Feature { id }) if !features.insert(*id) => {}
            Some(Sel::Plane { name }) if !planes.insert(name.clone()) => {}
            Some(item) => items.push(item.clone()),
            None => joints.extend(f.key.parse::<u64>().ok()),
        }
    }
    app.tree.picked_components.clear();
    app.tree.picked_canvases.clear();
    let _ = if items.is_empty() { app.run("select.clear", json!({})) } else { app.run("select.set", json!({ "items": items })) };
    app.tree.picked_joints = joints;
}

/// Hide others: only the bodies the search found, and the bodies of the placements it found,
/// stay shown (the browser's Isolate, on what was found).
pub(crate) fn hide_others(app: &mut SolveApp) {
    let Some(hits) = app.tree.search.hits().filter(|h| h.search.complete()) else { return };
    let mut keep: Vec<String> = Vec::new();
    for f in &hits.search.found {
        match (&f.kind, &f.item) {
            (Kind::Body, Some(Sel::Body { name })) => keep.push(name.clone()),
            (Kind::Component, _) => keep.extend(app.session.occurrence_bodies(&f.path, true).unwrap_or_default()),
            _ => {}
        }
    }
    if keep.is_empty() {
        return;
    }
    keep.sort_unstable();
    keep.dedup();
    let shown = |s: &Session| s.world_state().map_or(0, |world| world.bodies.iter().filter(|b| s.body_visible(&b.name)).count());
    let before = shown(&app.session);
    let isolate = Item::action("ui.isolate", "Isolate", "").with(json!({ "bodies": keep }));
    crate::context_menu::run_item(app, &isolate, Pos2::ZERO);
    let hid = before.saturating_sub(shown(&app.session));
    app.tree.search.hid = (hid > 0).then_some((hid, app.session.revision));
}
