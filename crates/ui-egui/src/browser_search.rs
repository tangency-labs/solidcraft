//! The browser's search (#1413): a field above the tree, always there or opened from the header's
//! action (a preference, [`bar_shown`]), with Ctrl+F giving it the keyboard. It keeps only the rows whose names match, each with its parents (dimmed, opened for the search), a
//! list that narrows it to one kind, and Select all / Hide others for what it found. The matching is the engine's
//! ([`browser_find`]), the same as the `browser.find` command's.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use egui::{Pos2, Sense, vec2};
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::browser_find::{self, Kind, MAX_PATTERN_CHARS, Pattern, Search};
use solidcraft_engine::doc::{Document, FeatureKind, ModelState};

use crate::SolveApp;
use crate::theme::Tokens;

/// The search, kept with the browser's state.
#[derive(Default)]
pub struct BrowserSearch {
    /// What is typed.
    pub text: String,
    /// The kind picked (`None`: All).
    pub kind: Option<Kind>,
    /// Rows closed during this search; the tree's own folds are left as they were.
    closed: BTreeSet<String>,
    /// What the search found (`None`: no search).
    hits: Option<Hits>,
    /// The bar was asked for (the magnifier or Ctrl+F).
    shown: bool,
    /// The field takes the keyboard on the next frame.
    focus: bool,
}

impl BrowserSearch {
    /// A search is on: there is text, or a kind is picked.
    pub fn on(&self) -> bool {
        self.hits.is_some()
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
    /// The text and kind it was found for.
    text: String,
    kind: Option<Kind>,
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
    fn new(text: String, kind: Option<Kind>, app: &SolveApp, search: Search) -> Hits {
        let mut hits = Hits {
            text,
            kind,
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

    fn is_for(&self, text: &str, kind: Option<Kind>, app: &SolveApp) -> bool {
        self.text == text
            && self.kind == kind
            && self.sketch == app.session.active_sketch
            && Arc::ptr_eq(&self.doc, &app.session.doc)
            && Arc::ptr_eq(&self.state, &app.session.model.state())
    }

    pub(crate) fn total(&self) -> usize {
        self.search.total
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

/// Brings what the search found up to date with the text, the kind and the design: a new
/// search only when one of them changed.
pub(crate) fn refresh(app: &mut SolveApp) {
    let kind = app.tree.search.kind;
    if app.tree.search.hits.as_ref().is_some_and(|h| h.is_for(&app.tree.search.text, kind, app)) {
        return;
    }
    let pattern = Pattern::new(&app.tree.search.text);
    if pattern.is_none() && kind.is_none() {
        app.tree.search.hits = None;
        app.tree.search.closed.clear();
        return;
    }
    // Another text or kind is another search: the rows it closed open again.
    if app.tree.search.hits.as_ref().is_none_or(|h| h.text != app.tree.search.text || h.kind != kind) {
        app.tree.search.closed.clear();
    }
    let kinds: Vec<Kind> = kind.into_iter().collect();
    let search = browser_find::find(&app.session, pattern.as_ref(), &kinds);
    app.tree.search.hits = Some(Hits::new(app.tree.search.text.clone(), kind, app, search));
}

/// The kinds the design has (an entry for each), and the picked one even when it has none left.
fn kinds_present(app: &SolveApp) -> Vec<Kind> {
    let doc = &app.session.doc;
    let has = |k: Kind| match k {
        Kind::Component => !doc.occurrences.is_empty(),
        Kind::Body => !app.session.model.state().bodies.is_empty(),
        Kind::Sketch => doc.features.iter().any(|f| matches!(f.kind, FeatureKind::Sketch { .. })),
        Kind::Construction => doc.features.iter().any(|f| matches!(f.kind, FeatureKind::ConstructionPlane { .. })),
        Kind::Joint => !doc.assembly.joints.is_empty(),
    };
    Kind::ALL.into_iter().filter(|k| has(*k) || app.tree.search.kind == Some(*k)).collect()
}

fn kind_label(kind: Option<Kind>) -> &'static str {
    match kind {
        None => crate::i18n::tr("ui.all"),
        Some(Kind::Component) => crate::i18n::tr("ui.browser_search_components"),
        Some(Kind::Body) => crate::i18n::tr("ui.bodies"),
        Some(Kind::Sketch) => crate::i18n::tr("ui.sketches"),
        Some(Kind::Construction) => crate::i18n::tr("ui.browser_search_construction"),
        Some(Kind::Joint) => crate::i18n::tr("ui.joints"),
    }
}

/// The bar is above the tree: always, or once asked for, and with a search on, which only the bar
/// can end. Never in Render, which has its own search.
pub(crate) fn bar_shown(app: &SolveApp) -> bool {
    let search = &app.tree.search;
    !crate::render_workspace::active(app)
        && (app.preferences.browser_search_bar == crate::prefs::BrowserSearchBar::Always || search.shown || search.on())
}

/// The search field, the kind it is narrowed to with its count, and the actions on what it
/// found: above the browser's rows, in both browser layouts. Made of the UI kit's field,
/// dropdown, text and buttons, like the Appearance library's search.
pub(crate) fn bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    refresh(app);
    crate::scenario::publish_rect("browser:search:bar", ui.max_rect());
    ui.spacing_mut().item_spacing = vec2(t.space_4, t.space_4);
    ui.horizontal(|ui| {
        // The × ends a search or closes a bar that was asked for; a bar always there with nothing to
        // clear has no use for it, and its field takes the room.
        let search = &app.tree.search;
        let clears = !search.text.is_empty() || search.kind.is_some() || app.preferences.browser_search_bar != crate::prefs::BrowserSearchBar::Always;
        let clear_width = if clears { t.space_24 + ui.spacing().item_spacing.x } else { 0.0 };
        let size = vec2((ui.available_width() - clear_width).max(t.space_24), crate::ui_kit::row_height());
        let hint = crate::i18n::tr("ui.browser_search_hint");
        let field = ui
            .allocate_ui(size, |ui| {
                crate::ui_kit::bounded_cell_editor(ui, egui::Id::new("browser_search"), &mut app.tree.search.text, hint, false, MAX_PATTERN_CHARS)
            })
            .inner;
        crate::scenario::publish_handle("browser:search", field.rect.center());
        if std::mem::take(&mut app.tree.search.focus) {
            field.request_focus();
        }
        // Esc in the field ends the search (egui gives up the focus on Esc).
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.tree.search.clear();
        }
        field.on_hover_text(crate::i18n::tr("ui.browser_search_tip"));
        if clears {
            let (rect, _) = ui.allocate_exact_size(vec2(t.space_24, t.space_24), Sense::hover());
            let clear = crate::ui_kit::icon_button_at(ui, rect, "close", crate::i18n::tr("ui.browser_search_clear"));
            crate::scenario::publish_handle("browser:search:clear", clear.rect.center());
            if clear.clicked() {
                app.tree.search.clear();
            }
        }
    });
    let count = app.tree.search.hits().map(|h| {
        let id = if h.search.incomplete { "ui.browser_search_found_more" } else { "ui.browser_search_found" };
        crate::i18n::format(id, &[("count", &h.total().to_string())])
    });
    // One row high, with the count centred on the list.
    let row = vec2(ui.available_width(), crate::ui_kit::row_height());
    ui.allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        let font = egui::FontId::proportional(t.font_small);
        let count_width =
            count.as_ref().map_or(0.0, |c| ui.painter().layout_no_wrap(c.clone(), font, t.form_dim).size().x + ui.spacing().item_spacing.x);
        let size = vec2((ui.available_width() - count_width).max(t.space_24), crate::ui_kit::row_height());
        let current = kind_label(app.tree.search.kind);
        let picker = ui
            .allocate_ui(size, |ui| {
                crate::ui_kit::dropdown_menu(ui, "browser_search_kind", current, |ui| {
                    // Only while the list is open: the bar is there on every frame.
                    let kinds: Vec<Option<Kind>> = std::iter::once(None).chain(kinds_present(app).into_iter().map(Some)).collect();
                    for kind in &kinds {
                        let name = format!("browser:search:kind:{}", kind.map_or("all", Kind::id));
                        let r = crate::ui_kit::list_picker(ui, &name, kind_label(*kind), app.tree.search.kind == *kind);
                        crate::scenario::publish_handle(&name, r.rect.center());
                        if r.clicked() {
                            app.tree.search.kind = *kind;
                        }
                    }
                })
            })
            .inner;
        crate::scenario::publish_handle("browser:search:kinds", picker.rect.center());
        crate::scenario::publish_rect("browser:search:kinds", picker.rect);
        if let Some(count) = &count {
            crate::ui_kit::text(ui, count, crate::ui_kit::Text::Note("browser_search_count"));
        }
    });
    // This frame's typing and kind.
    refresh(app);
    let Some(hits) = app.tree.search.hits() else {
        ui.add_space(t.space_4);
        return;
    };
    let (total, hide) = (hits.total(), hits.shows_bodies());
    if total == 0 {
        crate::ui_kit::text(ui, crate::i18n::tr("ui.browser_search_none"), crate::ui_kit::Text::Note("browser_search_none"));
        return;
    }
    // Wrapped, so a long translation moves Hide others to the next line rather than cutting it off;
    // a label stays on one line, so the whole button moves, never half its text.
    ui.horizontal_wrapped(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        let label = crate::i18n::format("ui.browser_select_all", &[("count", &total.to_string())]);
        let r = crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small);
        crate::scenario::publish_handle("browser:search:select-all", r.rect.center());
        crate::scenario::publish_rect("browser:search:select-all", r.rect);
        if r.on_hover_text(crate::i18n::tr("ui.browser_select_all_tip")).clicked() {
            select_found(app);
        }
        // Disabled in place: a child scope (`add_enabled_ui`) would be placed without the row's wrap.
        // It is the row's last button, so nothing after it is disabled too.
        if !hide {
            ui.disable();
        }
        let r = crate::ui_kit::button(ui, crate::i18n::tr("ui.browser_hide_others"), crate::ui_kit::Button::Small);
        crate::scenario::publish_handle("browser:search:hide-others", r.rect.center());
        crate::scenario::publish_rect("browser:search:hide-others", r.rect);
        if r.on_hover_text(crate::i18n::tr("ui.browser_hide_others_tip")).clicked() {
            hide_others(app);
        }
    });
    ui.add_space(t.space_4);
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
    let Some(hits) = app.tree.search.hits() else { return };
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
    let isolate = crate::context_menu::Item::action("ui.isolate", "Isolate", "").with(json!({ "bodies": keep }));
    crate::context_menu::run_item(app, &isolate, Pos2::ZERO);
}
