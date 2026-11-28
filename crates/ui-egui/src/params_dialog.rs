//! The Change Parameters dialog and parameter-name autocomplete for expression boxes.
//!
//! A window over the engine's `parameters.*` commands: Named contains key parameters once,
//! followed by other named parameters. Dimensions indexes automatic labels by their owner.
//! The Name heading sorts each band by name
//! in natural order (#8563). Expressions validate as you type
//! (`expr.evaluate`) with a live model preview; Enter or leaving the field applies them.
//! With "Apply each change as you leave its field" off (a remembered preference, #9279), they
//! wait instead, without a preview, until Apply or OK sets them in one `parameters.change`.
//! Escape closes the window and discards pending edits and their preview; Cancel and Escape also
//! put back the expressions applied live since the dialog opened, in one undo step (#15377).
//! Import and export go through `parameters.import` / `parameters.export`.
//!
//! [`complete`] adds suggestions (parameters, functions, units) under any expression box: the
//! names come from `parameters.complete`, cached per document revision by [`publish`].

use crate::ui_kit::BackingTooltipExt as _;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::doc::expr;

use crate::SolveApp;
use crate::ui_kit;

#[path = "params_named.rs"]
mod redesign;

/// What Import reads. The picker must list them: on macOS, files of a type no filter names
/// can't be picked (#3216).
pub(crate) const IMPORT_TYPES: &[&str] = &["csv", "json"];

/// The units a user parameter's Unit cell offers and reads back: every unit expressions know, in
/// their table's order (lengths, angles, masses, volumes, areas: `expr::UNITS`, so the first is
/// mm), then none and text. The cell still shows a row's own unit when it is a spelling the table
/// doesn't have (`degrees`, set by a command).
const UNITS: [&str; expr::UNITS.len() + 2] = {
    let mut names = [""; expr::UNITS.len() + 2];
    let mut i = 0;
    while i < expr::UNITS.len() {
        names[i] = expr::UNITS[i].0;
        i += 1;
    }
    names[i + 1] = "text";
    names
};

/// Dialog state (what is being typed; the parameters themselves live in the document).
#[derive(Default)]
pub struct ParamsDialog {
    redesign: redesign::State,
    pub filter: String,
    /// The order by name inside each band (#8563), remembered across openings.
    pub(crate) sort: ui_kit::Sort,
    reset_scroll: bool,
    /// Expressions being edited, by parameter name.
    pub editing: std::collections::BTreeMap<String, String>,
    /// Expressions applied live since the dialog opened (#15377), by parameter name: the one the
    /// parameter had before the first, and the one the design listed right after the last, read
    /// before anything else can change it. Cancel and Escape put the first back.
    applied: std::collections::BTreeMap<String, (String, String)>,
    /// Expressions applied and still computing off the window thread, by parameter name: their
    /// rows show them until the change is in.
    pub(crate) applying: std::collections::BTreeMap<String, String>,
    /// Comments being edited, by parameter name.
    pub comments: std::collections::BTreeMap<String, String>,
    /// A parameter being renamed: (old name, new name).
    pub renaming: Option<(String, String)>,
    /// The band the rename was started in; each editor retains its own grid focus.
    pub rename_table: String,
    pub new_name: String,
    pub adding: bool,
    pub focus_new: bool,
    pub new_unit: usize,
    pub new_expr: String,
    pub new_comment: String,
    /// The last error from an action in the dialog.
    pub error: Option<String>,
    /// Why adding the add row's draft was refused, kept while the draft and the design document
    /// stay.
    new_refused: Option<NewRefused>,
    /// The expression edit to preview: (name, expression).
    pub preview: Option<(String, String)>,
    /// Whether an expression box set the preview this frame (else it ends with the edit).
    previewing: bool,
    /// Which opening of the window this is: each section's grid keeps its cursor and edit under
    /// it, so a window opened again starts clean.
    opening: u64,
    /// The sections whose grids were drawn, so closing the window forgets their state.
    grids: Vec<String>,
    /// The cell being typed in (section, parameter, column) and the draft it held before, which
    /// is what the cell holds until the edit commits: `editing` and `comments` show the typing.
    typing: Option<(String, String, usize, Option<String>)>,
    /// The rows as the engine listed them at one revision of the design (#2723): listing
    /// evaluates every parameter, so it happens when the design changes, not every frame.
    listed: Option<(u64, Arc<Vec<Row>>)>,
    /// The add row's live check, by (revision, unit, expression).
    new_check: Option<((u64, String, String), Result<String, String>)>,
    /// How many times the rows were listed and the add row checked (for the tests).
    pub lists: u32,
    pub checks: u32,
    /// Each section band starts open and remembers its state while the dialog is open.
    bands: std::collections::HashMap<String, bool>,
}

impl ParamsDialog {
    /// The add row starts blank, its expression too (#2919): a value put there for the user was
    /// almost never the one wanted, and had to be erased first.
    pub fn new() -> Self {
        static OPENINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let opening = OPENINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ParamsDialog { reset_scroll: true, opening, ..Default::default() }
    }

    /// Forget the grids' cursors and open edits, as the window closes.
    fn forget_grids(&self, ctx: &egui::Context) {
        for salt in &self.grids {
            ui_kit::Grid::forget(ctx, grid_state(self, salt));
        }
    }

    /// What the window's banner says: the last action's message, else why the add row's draft
    /// was refused.
    pub fn banner(&self) -> Option<&str> {
        self.error.as_deref().or(self.new_refused.as_ref().map(|r| r.error.as_str()))
    }
    /// The command the live preview should run (a valid expression being edited).
    pub fn preview_commands(&self) -> Option<Vec<(String, Value)>> {
        let (n, e) = self.preview.as_ref()?;
        Some(vec![("parameters.change".to_string(), json!({"name": n, "expression": e}))])
    }
}

/// One row as the engine lists it.
#[derive(Clone, Debug, Default)]
struct Row {
    name: String,
    expression: String,
    unit: String,
    value: Option<f64>,
    error: Option<String>,
    comment: String,
    source: String,
    group: String,
    /// The component the row's sketch or feature belongs to; 0 = the top level.
    component: u64,
    favorite: bool,
    measures: String,
    readers: Vec<String>,
    reader_features: usize,
    /// A text parameter's text.
    text: Option<String>,
    /// Read-only evaluated parameter (e.g. driven sketch dimension, #14426).
    read_only: bool,
}

impl Row {
    /// Whether the Filter box's text (lower case) is in the row's name, expression, text,
    /// comment or group. The expression lets a parameter's name find every one that uses it
    /// (#9706). The name also matches by the starts of its words, in order: `toodia` finds
    /// `tooth_outward_flange_diameter` (#11978).
    fn matches(&self, filter: &str) -> bool {
        filter.is_empty()
            || [&self.name, &self.expression, &self.comment, &self.group, &self.measures]
                .into_iter()
                .chain(self.text.as_ref())
                .any(|field| field.to_ascii_lowercase().contains(filter))
            || solidcraft_engine::doc::name_words_match(&self.name, filter)
    }
}

fn rows(app: &mut SolveApp) -> Vec<Row> {
    let list = app.session.execute("parameters.list", &json!({})).unwrap_or_default();
    // Sketch dimension parameters: the sketch that owns each.
    let mut owner: std::collections::BTreeMap<String, String> = Default::default();
    let mut measures = std::collections::BTreeMap::new();
    for f in &app.session.doc.features {
        if let solidcraft_engine::doc::FeatureKind::Sketch { sketch, .. } = &f.kind {
            for c in &sketch.constraints {
                if let Some(p) = &c.param {
                    owner.insert(p.clone(), f.name.clone());
                    measures.insert(p.clone(), redesign::measure(sketch, &c.kind));
                }
                if c.driven
                    && let Some(p) = &c.driven_param
                {
                    measures.insert(p.clone(), redesign::measure(sketch, &c.kind));
                }
            }
        }
    }
    // The component each sketch or feature belongs to, as the design records it.
    let doc = &app.session.doc;
    let mut component_of: std::collections::HashMap<&str, u64> = Default::default();
    for f in &doc.features {
        component_of.insert(f.name.as_str(), shown_component(&doc.components, f.component));
    }
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    list["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let name = s(r, "name");
            let source = s(r, "source");
            let group = match source.as_str() {
                "feature" | "driven" => s(r, "feature"),
                "sketch" => owner.get(&name).cloned().unwrap_or_else(|| "Sketch".into()),
                _ => String::new(),
            };
            let component = component_of.get(group.as_str()).copied().unwrap_or(0);
            Row {
                component,
                measures: measures.remove(&name).unwrap_or_else(|| solidcraft_ui_text::tr_label(&s(r, "input")).to_owned()),
                readers: r.get("used_by").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
                reader_features: 0,
                expression: s(r, "expression"),
                unit: s(r, "unit"),
                // The dialog shows the parameter's own unit, so it reads `display_value`
                // (`value` is mm / degrees in every reply, #10747).
                value: r.get("display_value").and_then(Value::as_f64).or_else(|| r.get("value").and_then(Value::as_f64)),
                error: r.get("error").and_then(Value::as_str).map(str::to_string),
                comment: s(r, "comment"),
                favorite: r.get("favorite").and_then(Value::as_bool).unwrap_or(false),
                text: r.get("text").and_then(Value::as_str).map(str::to_string),
                read_only: r.get("read_only").and_then(Value::as_bool).unwrap_or(false),
                source,
                group,
                name,
            }
        })
        .collect()
}

/// The component to list a feature under: its own when it is in the design tree, else the top
/// level (a component that is missing, or whose parents never reach the top, cannot be shown
/// as a folder).
fn shown_component(components: &[solidcraft_engine::doc::Component], id: u64) -> u64 {
    let mut at = id;
    for _ in 0..=components.len() {
        if at == 0 {
            return id;
        }
        match components.iter().find(|c| c.id == at) {
            Some(c) => at = c.parent,
            None => return 0,
        }
    }
    0
}

/// One entry of the Model Parameters list, in the order it is drawn.
#[derive(Debug, PartialEq)]
enum Entry {
    /// The model group at this index of the sections' groups.
    Group(usize),
    /// A component's folder, holding its own groups and then its child components' folders.
    Folder { id: u64, name: String, entries: Vec<Entry> },
}

/// The order the Model Parameters list is drawn in (#20648). `group_components` is each group's
/// component (0 = the top level) in listing order; `components` is every component as
/// (id, parent, name) in the design's order. With `grouped` off the list is flat. Otherwise the
/// top-level groups come first (so do groups whose component is not in the tree, so a row is
/// never lost), then a folder per component with groups in it or below it, nested like the
/// components, a component's own groups before its child folders.
fn listing_order(group_components: &[u64], components: &[(u64, u64, &str)], grouped: bool) -> Vec<Entry> {
    if !grouped {
        return (0..group_components.len()).map(Entry::Group).collect();
    }
    let mut children: std::collections::HashMap<u64, Vec<(u64, &str)>> = Default::default();
    for (id, parent, name) in components.iter().filter(|(id, parent, _)| *id != 0 && id != parent) {
        children.entry(*parent).or_default().push((*id, *name));
    }
    let mut by_component: std::collections::HashMap<u64, Vec<usize>> = Default::default();
    for (index, component) in group_components.iter().enumerate().filter(|(_, c)| **c != 0) {
        by_component.entry(*component).or_default().push(index);
    }
    fn build(
        parent: u64,
        children: &std::collections::HashMap<u64, Vec<(u64, &str)>>,
        by_component: &std::collections::HashMap<u64, Vec<usize>>,
        seen: &mut std::collections::HashSet<u64>,
    ) -> Vec<Entry> {
        let mut folders = Vec::new();
        for (id, name) in children.get(&parent).into_iter().flatten() {
            // A component is visited once, so a damaged parent loop ends.
            if !seen.insert(*id) {
                continue;
            }
            let mut entries: Vec<Entry> = by_component.get(id).into_iter().flatten().map(|i| Entry::Group(*i)).collect();
            entries.extend(build(*id, children, by_component, seen));
            if !entries.is_empty() {
                folders.push(Entry::Folder { id: *id, name: (*name).to_owned(), entries });
            }
        }
        folders
    }
    let folders = build(0, &children, &by_component, &mut Default::default());
    fn placed(entries: &[Entry], into: &mut std::collections::HashSet<usize>) {
        for entry in entries {
            match entry {
                Entry::Group(i) => {
                    into.insert(*i);
                }
                Entry::Folder { entries, .. } => placed(entries, into),
            }
        }
    }
    let mut inside: std::collections::HashSet<usize> = Default::default();
    placed(&folders, &mut inside);
    let mut order: Vec<Entry> = (0..group_components.len()).filter(|i| !inside.contains(i)).map(Entry::Group).collect();
    order.extend(folders);
    order
}

/// Rows in `entries`, counting every group below a folder.
fn entry_rows(entries: &[Entry], groups: &[(&str, Vec<&Row>)]) -> usize {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Group(i) => groups.get(*i).map_or(0, |(_, members)| members.len()),
            Entry::Folder { entries, .. } => entry_rows(entries, groups),
        })
        .sum()
}

/// Draw `entries` into the shared table: each group as its band, each component folder as a
/// header band holding its own entries, indented one step per level. A closed folder hides all
/// that is in it. The table has no nesting of its own, so depth is shown by the label's indent.
fn draw_entries(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    table: &mut ui_kit::Table<'_>,
    d: &mut ParamsDialog,
    (entries, groups): (&[Entry], &[(&str, Vec<&Row>)]),
    columns: &[ui_kit::GridColumn],
    depth: usize,
) {
    for entry in entries {
        match entry {
            Entry::Group(i) => {
                if let Some((group, members)) = groups.get(*i) {
                    let label = if d.redesign.selected_group.as_deref() == Some(group) {
                        solidcraft_ui_text::format("parameters.redesign.selected_group", &[("group", group)])
                    } else {
                        group.to_string()
                    };
                    band_at(app, ui, table, d, (&format!("model:{group}"), &label, members), columns, depth);
                }
            }
            Entry::Folder { id, name, entries } => {
                // A component's name is the user's own text, so the header shows it as written
                // and never as a catalog word (a component named "Frame" is not the word "Frame").
                let salt = format!("component:{id}");
                let open = *d.bands.entry(salt.clone()).or_insert(true);
                let mut now = open;
                let response = table.group(ui, &salt, &format!("{}{name}", INDENT.repeat(depth)), entry_rows(entries, groups), &mut now);
                d.bands.insert(salt, now);
                crate::scenario::publish_target(&format!("dialog:section:{name}"), response.rect);
                if now {
                    draw_entries(app, ui, table, d, (entries, groups), columns, depth + 1);
                }
            }
        }
    }
}

/// One level of a component folder's indent in a band label.
const INDENT: &str = "\u{2003}\u{2003}";

fn show_value(r: &Row) -> String {
    match r.value {
        Some(v) if r.unit.is_empty() => format!("{v:.4}"),
        Some(v) => format!("{v:.4} {}", r.unit),
        None => r.text.clone().unwrap_or_else(|| "—".into()),
    }
}

/// Open the dialog (Change Parameters on the toolbar).
pub fn open(app: &mut SolveApp) {
    let mut dialog = ParamsDialog::new();
    let unit = crate::length_input::unit(&app.session.doc);
    dialog.new_unit = UNITS.iter().position(|u| *u == unit).unwrap_or(0);
    app.params = Some(dialog);
}

/// Cancel before drawing the editors: egui has already blurred the focused TextEdit on Esc,
/// and drawing it would apply its pending value through `lost_focus`.
pub(crate) fn cancel_on_escape(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    if !app.home.open
        && let Some(d) = app.params.as_mut()
        && (d.redesign.naming.is_some() || d.redesign.demote.is_some())
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        d.redesign.naming = None;
        d.redesign.demote = None;
        d.renaming = None;
        d.error = None;
        for salt in &d.grids {
            ui_kit::Grid::forget(ctx, grid_state(d, salt));
        }
        ctx.input_mut(|i| i.raw.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Escape, .. })));
        app.esc_handled = true;
        return true;
    }
    if app.home.open || app.params.is_none() || !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        return false;
    }
    ctx.input_mut(|i| {
        i.raw.events.retain(|event| !matches!(event, egui::Event::Key { key: egui::Key::Escape, modifiers, .. } if modifiers.matches_logically(egui::Modifiers::NONE)));
    });
    // Dropping the dialog discards every draft. The regular preview update clears its scene.
    // What was applied live goes back first; a refused put-back keeps the dialog open to say so.
    if let Some(mut d) = app.params.take() {
        if revert_applied(app, &mut d) {
            d.forget_grids(ctx);
        } else {
            app.params = Some(d);
        }
    }
    app.esc_handled = true;
    true
}

/// Draw the dialog when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    // Completions are offered only under a focused text box; gathering them evaluates every
    // parameter, so a design changing command after command (an agent building over MCP)
    // does not pay for it on every change while nobody is typing.
    if app.params.is_some() || ctx.memory(|m| m.focused().is_some()) {
        publish(app, ctx);
    }
    let Some(mut d) = app.params.take() else { return };
    forget_refused_add(app, &mut d);
    d.sort = ctx.data_mut(|m| m.get_persisted(sort_id())).unwrap_or_default();
    let sort = d.sort;
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let viewport = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    if !viewport.is_finite() || !viewport.is_positive() {
        app.params = Some(d);
        return;
    }
    let side = crate::frame::Side::Right;
    let (_, docked) = crate::shell::floating_editor_anchor(app, ctx);
    let moved = crate::frame::dock_offset(ctx, "PARAMETERS");
    let drag_bounds = crate::shell::tool_drag_rect(app, viewport);
    let anchor = crate::frame::offset_anchor(docked, moved, drag_bounds, side);
    let title = solidcraft_ui_text::tr_label("Parameters");
    let panel = ui_kit::Panel::new(
        title,
        egui::Id::unique("sc_params_dialog"),
        anchor,
        drag_bounds,
        side,
        ui_kit::Panel::feature_frame(ctx, app.ui.layout, side),
    )
    .wide()
    // A reset asked for while drawing (a view switch, a parameter chosen in a Used by list) is
    // kept for the next frame's panel; this frame's is used up here.
    .reset_scroll(std::mem::take(&mut d.reset_scroll));
    // Expanded Key controls use the available body height; the shared body scrolls the rest.
    // A manually resized height still takes precedence below.
    let panel = if d.redesign.compact.is_some() { panel } else { panel.body_rows(16.0) };
    // A width or body height the user dragged the card to replaces that default; each axis stays
    // automatic until the user drags it (review of #13422).
    let (chosen_width, chosen_height) = (SIZE.width(ctx), SIZE.height(ctx));
    let t = solidcraft_theme::Tokens::get();
    let default_width = (t.form_card_width * 3.0).max(d.redesign.header_width + t.space_12 * 2.0 + t.space_16);
    let width = chosen_width.map_or(default_width, |w| w.max(crate::frame::DIALOG_MIN_W));
    let panel = panel.body_height(chosen_height);
    let panel = panel.width(width.min((docked.x - drag_bounds.left()).max(crate::frame::DIALOG_MIN_W)));
    let all = redesign::listed(app, &mut d);
    let mut outcome = (false, ui_kit::Action::None);
    let live = app.preferences.parameters_apply_live;
    let mut live_choice = live;
    let held_status = if live { None } else { held_status(held(&d, &all).len()) };
    let mut resize = crate::card_resize::Resize::None;
    d.previewing = false;
    let shown = panel.window(ctx).show(ctx, |ui| {
        outcome = panel.feature_with_left(
            ui,
            ui_kit::FeatureOptions {
                icon: solidcraft_engine::find_command("parameters.change").map_or("", |c| c.icon),
                title,
                drag_key: "PARAMETERS",
                moved,
                docked,
                apply_label: (!live).then_some("Apply"),
            },
            false,
            !all.iter().any(|r| r.error.is_some()),
            |ui| {
                redesign::toolbar(app, ui, &mut d, &all);
                if let Some(error) = d.banner() {
                    ui_kit::text(ui, error, ui_kit::Text::Error("parameters_error"));
                }
                let filter = d.filter.to_ascii_lowercase();
                let visible: Vec<_> = all.iter().filter(|r| r.matches(&filter)).collect();
                // One Name-column floor for every table in this dialog so Favorites, User and
                // Model sections stay lined up when only some lists hold a long name (#8551 review).
                let name_floor = visible
                    .iter()
                    .map(|r| ui_kit::ContentWidth::name(ui, &r.name))
                    .fold(solidcraft_theme::Tokens::get().form_label_width, f32::max)
                    .min(solidcraft_theme::Tokens::get().form_card_width);
                let mut sections = sections(&visible, d.sort);
                redesign::follow_selection(app, &mut d, &mut sections);
                table(app, ui, &mut d, &sections, name_floor, &all);
                redesign::states(app, ui, &mut d, &all, &visible);
                ui_kit::ComputeStatus { text: None, busy: false, applying: false }
            },
            |ui| apply_live_checkbox(ui, &mut live_choice, held_status.as_deref()),
        );
        resize = SIZE.edges(ui, side, panel.last_body_scrollbar(ui.ctx()));
    });
    if live_choice != live {
        app.preferences.parameters_apply_live = live_choice;
        if live_choice {
            apply_held(app, &mut d, &all);
        }
    }
    SIZE.apply(ctx, resize, shown.as_ref().map(|r| r.response.rect), panel.last_body_height(ctx), viewport);
    redesign::confirm_demote(app, ctx, &mut d, &all);
    // Held edits (#9279) are set by Apply and OK; Cancel, the close button and Escape drop them.
    // Cancel and Escape also put back what was applied live (#15377); the close button keeps it,
    // as closing the window the usual way never loses an edit.
    let (closed, action) = outcome;
    let applied = live || !matches!(action, ui_kit::Action::Ok | ui_kit::Action::Apply) || apply_held(app, &mut d, &all);
    let cancelled = action == ui_kit::Action::Cancel && revert_applied(app, &mut d);
    let close = closed || cancelled || (action == ui_kit::Action::Ok && applied);
    // Each cell owns fresh Enter from raw events. The panel itself must not submit on
    // Enter in the filter or with unrelated focus (the legacy editor left those alone), but
    // the filter, rename and comment fields still see Enter first to give up focus.
    if let Some(at) = crate::scenario::handle_at("dialog:close") {
        crate::scenario::publish_handle("params:close", at);
    }
    if let Some(rect) = crate::scenario::rect_of("dialog:header") {
        crate::scenario::publish_handle("params:title", rect.center());
    }
    if d.sort != sort {
        let chosen = d.sort;
        ctx.data_mut(|m| m.insert_persisted(sort_id(), chosen));
    }
    // The preview lasts while its expression is being edited.
    if !d.previewing {
        d.preview = None;
    }
    if close {
        d.forget_grids(ctx);
    } else {
        app.params = Some(d);
    }
}

/// Where the chosen order is kept across dialog openings.
fn sort_id() -> egui::Id {
    egui::Id::unique("sc_params_sort")
}

/// Where the card's dragged width and body height are kept, and the names its edges publish.
const SIZE: crate::card_resize::CardSize = crate::card_resize::CardSize { width: "sc_params_width", height: "sc_params_height", handle: "params" };

/// Where the column widths the user dragged are kept, shared by every section's table.
fn columns_id() -> egui::Id {
    egui::Id::unique("sc_params_columns")
}

/// The dialog's sections, each in the chosen order.
struct Sections<'a> {
    favorites: Vec<&'a Row>,
    users: Vec<&'a Row>,
    /// Model parameters by the sketch or feature that drives them, groups in listing order.
    groups: Vec<(&'a str, Vec<&'a Row>)>,
}

/// Split the listed rows into sections and sort each one by name on its own (#8563): a sort
/// orders the rows inside Favorites, User Parameters and each model group, never moves a row
/// to another section, and keeps the groups themselves in the design's order.
fn sections<'a>(visible: &[&'a Row], sort: ui_kit::Sort) -> Sections<'a> {
    fn by_name<'o>(r: &'o &Row) -> &'o str {
        &r.name
    }
    let mut favorites: Vec<_> = visible.iter().copied().filter(|r| r.favorite && redesign::named(r)).collect();
    sort.apply(&mut favorites, by_name);
    let mut users: Vec<_> = visible.iter().copied().filter(|r| redesign::named(r) && !r.favorite).collect();
    sort.apply(&mut users, by_name);
    // Group once, in listing order, rather than scanning every parameter for each feature on
    // every frame (#2733).
    let mut groups: Vec<(&str, Vec<&Row>)> = Vec::new();
    let mut indices = std::collections::HashMap::new();
    for r in visible.iter().copied().filter(|r| !redesign::named(r)) {
        let index = *indices.entry(r.group.as_str()).or_insert_with(|| {
            groups.push((&r.group, Vec::new()));
            groups.len() - 1
        });
        if let Some((_, members)) = groups.get_mut(index) {
            members.push(r);
        }
    }
    for (_, members) in &mut groups {
        sort.apply(members, by_name);
    }
    Sections { favorites, users, groups }
}

/// Live check of an expression typed for a parameter of this unit: Ok(shown value) or the message.
fn check(app: &mut SolveApp, e: &str, unit: &str) -> Result<String, String> {
    let r = app.session.execute("expr.evaluate", &json!({"expression": e, "unit": unit})).map_err(|e| e.to_string())?;
    if let Some(text) = r["text"].as_str() {
        return Ok(text.to_string());
    }
    if r["ok"].as_bool().unwrap_or(false) {
        let v = r["display_value"].as_f64().unwrap_or(0.0);
        Ok(if unit.is_empty() { format!("{v:.4}") } else { format!("{v:.4} {unit}") })
    } else {
        Err(r["error"].as_str().unwrap_or("invalid").to_string())
    }
}

/// The add row's check of its expression, kept until the design, unit or text changes.
fn new_check(app: &mut SolveApp, d: &mut ParamsDialog, unit: &str) -> Result<String, String> {
    let key = (app.session.revision, unit.to_string(), d.new_expr.clone());
    if let Some((k, r)) = &d.new_check
        && *k == key
    {
        return r.clone();
    }
    let r = check(app, &d.new_expr, unit);
    d.checks += 1;
    d.new_check = Some((key, r.clone()));
    r
}

/// The columns of every section's grid, in order.
const STAR: usize = 0;
const NAME: usize = 1;
const UNIT: usize = 2;
const EXPRESSION: usize = 3;
const VALUE: usize = 4;
const COMMENT: usize = 5;

/// One section's parameters as a [`ui_kit::Grid`]: the cells show the design (or an edit held for
/// Apply), and each gesture on them comes back as one edit list, applied as one undo step.
struct ParamGrid<'a> {
    app: &'a mut SolveApp,
    d: &'a mut ParamsDialog,
    rows: &'a [&'a Row],
    salt: &'a str,
    columns: &'a [ui_kit::GridColumn],
    /// How far right an expression being edited may open (#9358).
    right: f32,
    /// The live check of each row's held or typed expression that differs from the design's.
    live: std::collections::HashMap<String, Result<String, String>>,
    cols: &'static [usize],
}

impl ParamGrid<'_> {
    fn column(&self, col: usize) -> usize {
        self.cols.get(col).copied().unwrap_or(usize::MAX)
    }
    fn row(&self, row: usize) -> Option<&Row> {
        self.rows.get(row).copied()
    }

    /// The expression a row shows: an edit held for Apply or being typed, else one still being
    /// applied, else the design's.
    fn expression<'r>(&'r self, r: &'r Row) -> &'r str {
        self.d.editing.get(&r.name).or_else(|| self.d.applying.get(&r.name)).map_or(r.expression.as_str(), String::as_str)
    }

    /// What the cell being typed in holds until its edit commits: the draft it had before (an edit
    /// held for Apply), else the design's.
    fn before_typing<'r>(&'r self, r: &'r Row, col: usize) -> Option<&'r str> {
        let (salt, name, typed, before) = self.d.typing.as_ref()?;
        if salt != self.salt || *name != r.name || *typed != col {
            return None;
        }
        Some(before.as_deref().unwrap_or(if col == COMMENT { r.comment.as_str() } else { r.expression.as_str() }))
    }

    /// The Value column: the live check of an edited expression, the row's error, or its value.
    fn shown(&self, r: &Row) -> (String, bool) {
        match (self.live.get(&r.name), &r.error) {
            (Some(Err(message)), _) | (None, Some(message)) => (message.clone(), true),
            (Some(Ok(value)), _) => (value.clone(), false),
            (None, None) => (show_value(r), false),
        }
    }
}

fn parameter_columns(name_floor: f32, unit_width: f32) -> Vec<ui_kit::GridColumn> {
    use ui_kit::{CellKind, GridColumn};
    let t = solidcraft_theme::Tokens::get();
    let units = UNITS.iter().map(|u| unit_label(u).to_owned()).collect();
    // Name keeps a larger share and a shared content floor so long identifiers stay readable
    // and every section's columns line up (#8533, #8551 review).
    vec![
        GridColumn::new("favorite", "", 0.0, CellKind::Custom).min_content_width(t.control_height),
        GridColumn::new("name", "Name", 2.0, CellKind::Text).flex().min_content_width(name_floor),
        GridColumn::new("unit", "Unit", 0.65, CellKind::Choice(units)).min_content_width(unit_width),
        GridColumn::new("expression", "Expression", 1.5, CellKind::Expression).flex().edit_on_click(),
        GridColumn::new("value", "Value", 1.1, CellKind::ReadOnly).flex(),
        GridColumn::new("comment", "Comment", 1.0, CellKind::Text).flex().edit_on_click(),
        GridColumn::new("actions", "Actions", 0.0, CellKind::Custom).icon_only().min_content_width(ui_kit::table_row_height()),
    ]
}

impl ui_kit::GridSource for ParamGrid<'_> {
    fn cell_ranges(&self) -> bool {
        self.d.redesign.ranges
    }
    fn columns(&self) -> Vec<ui_kit::GridColumn> {
        self.columns.to_vec()
    }

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn row_key(&self, row: usize) -> std::borrow::Cow<'_, str> {
        self.row(row).map_or(std::borrow::Cow::Borrowed(""), |r| std::borrow::Cow::Borrowed(r.name.as_str()))
    }

    fn text(&self, row: usize, col: usize) -> std::borrow::Cow<'_, str> {
        let col = self.column(col);
        use std::borrow::Cow;
        let Some(r) = self.row(row) else { return Cow::Borrowed("") };
        if let Some(before) = self.before_typing(r, col) {
            return Cow::Borrowed(before);
        }
        match col {
            STAR => Cow::Borrowed(if r.favorite { "★" } else { "" }),
            NAME => Cow::Borrowed(&r.name),
            UNIT => Cow::Borrowed(unit_label(&r.unit)),
            EXPRESSION => Cow::Borrowed(self.expression(r)),
            VALUE => Cow::Owned(self.shown(r).0),
            COMMENT => Cow::Borrowed(self.d.comments.get(&r.name).unwrap_or(&r.comment)),
            redesign::MEASURES => Cow::Borrowed(&r.measures),
            redesign::USED => Cow::Owned(r.readers.len().to_string()),
            _ => Cow::Borrowed(""),
        }
    }

    fn kind(&self, row: usize, col: usize) -> Option<ui_kit::CellKind> {
        let col = self.column(col);
        let r = self.row(row)?;
        // A model parameter's unit is the dimension's: shown, not chosen.
        // A driven dimension's expression is measured, not edited (#14426).
        ((col == UNIT && r.source != "user") || (col == EXPRESSION && r.read_only)).then_some(ui_kit::CellKind::ReadOnly)
    }

    fn message(&self, row: usize, col: usize) -> Option<(std::borrow::Cow<'_, str>, bool)> {
        let col = self.column(col);
        let r = self.row(row)?;
        let (shown, error) = self.shown(r);
        (col == EXPRESSION && error).then_some((std::borrow::Cow::Owned(shown), true))
    }

    fn set_cell(&mut self, row: usize, col: usize, text: &str) -> Result<(), String> {
        let col = self.column(col);
        let Some(r) = self.row(row) else { return Err(String::new()) };
        match col {
            NAME if !redesign::named(r) => redesign::validate_name(self.app, &r.name, text, true),
            NAME if text.trim().is_empty() => Err(solidcraft_ui_text::tr("parameters.grid_name_required").to_owned()),
            UNIT => unit_from(text).map(|_| ()),
            EXPRESSION if r.read_only => Err(String::new()),
            // Applied as you go, an expression is checked before it is handed back, so a wrong one
            // keeps its box open; held for Apply, it is checked when it is held (#9279).
            EXPRESSION if self.app.preferences.parameters_apply_live => {
                let unit = r.unit.clone();
                check(self.app, text, &unit).map(|_| ())
            }
            _ => Ok(()),
        }
    }

    fn add_row(&self) -> Option<std::borrow::Cow<'_, str>> {
        (self.salt == "user").then_some(std::borrow::Cow::Borrowed("Add parameter"))
    }

    fn draw(&mut self, ui: &mut egui::Ui, row: usize, col: usize) -> Option<egui::Response> {
        let col = self.column(col);
        let rows = self.rows;
        let r = *rows.get(row)?;
        match col {
            STAR => Some(
                ui.horizontal(|ui| {
                    let star = if r.favorite { "★" } else { "☆" };
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let favorite = ui_kit::compact_toggle(ui, star, "Favourite", true, r.favorite);
                    crate::scenario::publish_handle(&format!("param:star:{}", r.name), favorite.rect.center());
                    if favorite.clicked() {
                        if !redesign::named(r) && !r.favorite {
                            self.d.redesign.start_name(r, true);
                        } else {
                            self.d.error = self.app.run("parameters.favorite", json!({"name": r.name, "favorite": !r.favorite})).err();
                        }
                    }
                    favorite
                })
                .inner,
            ),
            NAME => {
                // Keep long identifiers readable (#8533) without stacking the cell's own tooltip.
                let response = ui_kit::name_cell_with_hint(ui, &r.name, false, solidcraft_ui_text::tr_label("Double-click to rename"));
                crate::scenario::publish_handle(&format!("param:name:{}", r.name), response.rect.center());
                Some(response)
            }
            UNIT if r.source != "user" => Some(ui_kit::text(ui, unit_label(&r.unit), ui_kit::Text::MonoCell)),
            EXPRESSION if r.read_only => Some(ui_kit::text(ui, &r.expression, ui_kit::Text::MonoCell)),
            VALUE => {
                let (shown, error) = self.shown(r);
                let number = if self.d.redesign.view == redesign::View::Named && !self.d.redesign.narrow && !error && !r.unit.is_empty() {
                    shown.strip_suffix(&format!(" {}", r.unit)).unwrap_or(&shown)
                } else {
                    &shown
                };
                Some(ui_kit::text(ui, number, ui_kit::Text::Value(&format!("value:{}", r.name), error)).backing_on_hover_text(&shown))
            }
            COMMENT => {
                let request = egui::Id::unique(("sc_parameter_comment_expand", self.d.opening, self.salt, &r.name));
                let text = self.d.comments.get(&r.name).unwrap_or(&r.comment);
                let response = ui_kit::expandable_cell_preview(ui, request, text, &format!("param:comment:{}", r.name));
                if response.clicked() && !ui.input(|input| input.modifiers.shift) {
                    ui.data_mut(|data| data.insert_temp(request, true));
                }
                Some(response)
            }
            redesign::MEASURES => Some(ui_kit::text(ui, &r.measures, ui_kit::Text::MonoCell)),
            redesign::USED => Some(redesign::used_by(self.app, ui, self.d, r)),
            ACTIONS => Some(redesign::row_menu(self.app, ui, self.d, r, self.rows)),
            _ => None,
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui, row: usize, col: usize, id: egui::Id, text: &mut String) -> ui_kit::CellEditor {
        let col = self.column(col);
        let Some(r) = self.row(row).cloned() else {
            return ui_kit::CellEditor { response: ui_kit::cell_editor(ui, id, text, "", false), taken: false };
        };
        match col {
            EXPRESSION => {
                let live = (*text != r.expression).then(|| check(self.app, text, &r.unit));
                let (shown, error) = match (&live, &r.error) {
                    (Some(Err(message)), _) | (None, Some(message)) => (message.clone(), true),
                    (Some(Ok(value)), _) => (value.clone(), false),
                    (None, None) => (show_value(&r), false),
                };
                let response = ui_kit::wide_cell_editor(ui, id, text, "Expression", error, self.right, (&shown, error));
                let taken = complete(ui, &response, text);
                // Held edits (#9279) preview nothing: the preview is a rebuild too.
                if self.app.preferences.parameters_apply_live && response.has_focus() && matches!(live, Some(Ok(_))) {
                    self.d.preview = Some((r.name.clone(), text.clone()));
                }
                self.d.previewing |= self.d.preview.as_ref().is_some_and(|(name, _)| *name == r.name);
                ui_kit::CellEditor { response, taken }
            }
            NAME => redesign::name_editor(self.app, ui, &r, id, text),
            _ => {
                let request = egui::Id::unique(("sc_parameter_comment_expand", self.d.opening, self.salt, &r.name));
                if ui.data_mut(|data| data.remove_temp::<bool>(request)).unwrap_or(false) {
                    ui.memory_mut(|memory| memory.request_focus(id));
                    ui.data_mut(|data| data.insert_temp(id.with("expanded"), true));
                }
                ui_kit::CellEditor {
                    response: ui_kit::expandable_cell_editor(ui, id, text, "Comment", &format!("param:comment:{}", r.name)),
                    taken: false,
                }
            }
        }
    }

    fn drawn(&mut self, ui: &egui::Ui, row: usize, col: usize, response: &egui::Response, editing: bool) {
        let col = self.column(col);
        let Some(r) = self.row(row) else { return };
        // The handles the scenarios click, as when every cell was an open box.
        match col {
            EXPRESSION => {
                crate::scenario::publish_target_as(&format!("param:{}", r.name), &format!("param:{}", r.name), response.rect);
                if self.salt == "fav" {
                    crate::scenario::publish_handle(&format!("param:favorite:{}", r.name), response.rect.center());
                }
            }
            COMMENT => crate::scenario::publish_handle(&format!("param:comment:{}", r.name), response.rect.center()),
            _ => {}
        }
        if matches!(col, NAME | VALUE) && !editing {
            redesign::select_row(ui, self.d, self.rows, row, response);
        }
    }

    fn row_selected(&self, row: usize) -> bool {
        self.row(row).is_some_and(|r| self.d.redesign.selected.contains(&r.name))
    }
    fn row_drawn(&mut self, ui: &egui::Ui, row: usize, response: &egui::Response) {
        redesign::select_row(ui, self.d, self.rows, row, response);
    }
}

const ACTIONS: usize = 6;

/// The unit a Unit cell's text names: its label ("none" for no unit) or the unit itself.
fn unit_from(text: &str) -> Result<&'static str, String> {
    let text = text.trim();
    UNITS
        .iter()
        .copied()
        .find(|u| *u == text || unit_label(u) == text)
        .ok_or_else(|| solidcraft_ui_text::format("parameters.grid_unknown_unit", &[("unit", text)]))
}

/// The grid state of section `salt` in this opening of the window: a new opening starts clean.
fn grid_state(d: &ParamsDialog, salt: &str) -> egui::Id {
    egui::Id::unique(("sc_params_grid", d.opening, salt))
}

/// One shared card and header; each band keeps the upstream grid's editing and virtualization.
fn table(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, sections: &Sections<'_>, name_floor: f32, all: &[Row]) {
    let t = solidcraft_theme::Tokens::get();
    let units: Vec<_> = UNITS.iter().map(|unit| unit_label(unit)).collect();
    let unit_width = ui_kit::ContentWidth::dropdown(ui, &units).max(t.space_24 * 3.0 + t.space_6);
    d.redesign.narrow = ui.available_width() < 560.0;
    let cols = redesign::column_order(d);
    let columns = redesign::columns(name_floor, unit_width, cols);
    let column_state = columns_id().with((d.redesign.view == redesign::View::Dimensions, d.redesign.narrow));
    let manual: Vec<Option<f32>> = ui.ctx().data_mut(|m| m.get_persisted(column_state)).unwrap_or_default();
    let layout: Vec<_> = columns.iter().enumerate().map(|(i, c)| c.layout(manual.get(i).copied().flatten())).collect();
    ui_kit::table_custom(ui, "parameters", &layout, solidcraft_ui_text::tr("manage.empty"), |ui, table| {
        redesign::bulk(app, ui, d, all);
        table.headers_resizable(ui, column_state, |ui, index| {
            if cols.get(index) != Some(&NAME) {
                return false;
            }
            let response = ui_kit::sort_header(ui, "Name", &mut d.sort);
            for salt in ["fav", "user"].into_iter().chain(sections.groups.iter().map(|(group, _)| *group)) {
                crate::scenario::publish_handle(&format!("params:sort:{salt}"), response.rect.center());
            }
            true
        });
        if d.redesign.view == redesign::View::Named {
            band(app, ui, table, d, ("fav", solidcraft_ui_text::tr("parameters.redesign.key"), &sections.favorites), &columns);
            band(app, ui, table, d, ("user", solidcraft_ui_text::tr("parameters.redesign.named"), &sections.users), &columns);
        } else {
            // The Dimensions view keeps the component tree and flat-list preference.
            let order = {
                let components: Vec<_> = app.session.doc.components.iter().map(|c| (c.id, c.parent, c.name.as_str())).collect();
                let of_group: Vec<u64> = sections.groups.iter().map(|(_, m)| m.first().map_or(0, |r| r.component)).collect();
                listing_order(&of_group, &components, app.preferences.params_group_by_component)
            };
            draw_entries(app, ui, table, d, (&order, &sections.groups), &columns, 0);
        }
    });
    // The sections now share one horizontal viewport; existing agents may still ask for
    // their original grid's viewport and content width.
    let viewport = crate::scenario::rect_of("dialog:table:parameters:viewport");
    let content_width = crate::scenario::count_of("dialog:table:parameters:content_width");
    for salt in ["fav", "user"].into_iter().chain(sections.groups.iter().map(|(group, _)| *group)) {
        if let Some(rect) = viewport {
            crate::scenario::publish_rect(&format!("dialog:table:{salt}:viewport"), rect);
        }
        if let Some(width) = content_width {
            crate::scenario::publish_count(&format!("dialog:table:{salt}:content_width"), width);
        }
    }
}

fn band(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    table: &mut ui_kit::Table<'_>,
    d: &mut ParamsDialog,
    band: (&str, &str, &[&Row]),
    columns: &[ui_kit::GridColumn],
) {
    band_at(app, ui, table, d, band, columns, 0);
}

/// A band `depth` component folders deep.
fn band_at(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    table: &mut ui_kit::Table<'_>,
    d: &mut ParamsDialog,
    (salt, label, rows): (&str, &str, &[&Row]),
    columns: &[ui_kit::GridColumn],
    depth: usize,
) {
    let open = d.bands.entry(salt.to_owned()).or_insert(!salt.starts_with("model:") || !d.filter.is_empty());
    let response = table.group(ui, salt, &format!("{}{label}", INDENT.repeat(depth)), rows.len(), open);
    if salt.starts_with("model:") && d.redesign.reveal_group && d.redesign.selected_group.as_deref() == salt.strip_prefix("model:") {
        table.reveal_row(response.rect);
        d.redesign.reveal_group = false;
    }
    let section = match salt {
        "fav" => "Favorites",
        "user" => "User Parameters",
        _ => salt.strip_prefix("model:").unwrap_or(label),
    };
    crate::scenario::publish_target(&format!("dialog:section:{section}"), response.rect);
    crate::scenario::publish_count(&format!("dialog:table:parameters:group:{salt}:count"), rows.len() as f64);
    if !*open {
        return;
    }
    if salt == "fav" && rows.is_empty() {
        ui_kit::text(ui, solidcraft_ui_text::tr("parameters.redesign.empty_key"), ui_kit::Text::Hint("empty_key"));
    }
    // The band has a namespaced key, while the grid keeps its original agent-facing id.
    let salt = salt.strip_prefix("model:").unwrap_or(salt);
    let right = ui.max_rect().right().min(ui.clip_rect().right());
    let state = grid_state(d, salt);
    if !d.grids.iter().any(|g| g == salt) {
        d.grids.push(salt.to_owned());
    }
    let mut live = std::collections::HashMap::new();
    for r in rows {
        if !r.read_only
            && let Some(edited) = d.editing.get(&r.name).filter(|e| **e != r.expression)
        {
            let edited = edited.clone();
            live.insert(r.name.clone(), check(app, &edited, &r.unit));
        }
    }
    let cols = redesign::column_order(d);
    if let Some((name, _, started)) = d.redesign.naming.as_mut()
        && !*started
        && let Some((index, r)) = rows.iter().enumerate().find(|(_, r)| r.name == *name)
        && let Some(col) = cols.iter().position(|c| *c == NAME)
    {
        ui_kit::Grid::edit(ui.ctx(), state, &r.name, index, col, redesign::suggestions(r).into_iter().next().unwrap_or_default());
        *started = true;
    }
    let mut grid = ParamGrid { app, d, rows, salt, columns, right, live, cols };
    let out = ui_kit::Grid::new(salt).state(state).show_in(ui, table, &mut grid, |ui, table, grid| {
        if grid.salt == "user" && grid.d.adding {
            let right = grid.right;
            ui.push_id("parameter_draft", |ui| add_row(grid.app, ui, table, grid.d, right));
        }
    });
    if out.add_row {
        d.adding = true;
        d.focus_new = true;
    }
    let out = redesign::project_output(out, cols);
    apply_grid(app, ui.ctx(), d, rows, salt, out);
}

/// Mirror the edit being typed into the dialog's drafts, then apply one gesture's edits as one
/// undo step: expressions in one `parameters.change` (or held for Apply), then units, comments,
/// renames and pasted new rows.
fn apply_grid(app: &mut SolveApp, ctx: &egui::Context, d: &mut ParamsDialog, rows: &[&Row], salt: &str, out: ui_kit::GridOutput) {
    let find = |name: &str| rows.iter().copied().find(|r| r.name == name);
    let typing_here = d.typing.as_ref().is_some_and(|(s, ..)| s == salt);
    match &out.editing {
        Some((name, col, _)) if matches!(*col, EXPRESSION | COMMENT) => {
            let same = d.typing.as_ref().is_some_and(|(s, n, c, _)| s == salt && n == name && c == col);
            if !same {
                let before = if *col == COMMENT { d.comments.get(name) } else { d.editing.get(name) }.cloned();
                d.typing = Some((salt.to_owned(), name.clone(), *col, before));
            }
        }
        _ if typing_here => d.typing = None,
        _ => {}
    }
    match &out.editing {
        Some((name, EXPRESSION, text)) => {
            d.editing.insert(name.clone(), text.clone());
        }
        Some((name, COMMENT, text)) => {
            d.comments.insert(name.clone(), text.clone());
        }
        Some((name, NAME, text)) => {
            d.renaming = Some((name.clone(), text.clone()));
            d.rename_table = salt.to_owned();
            if d.redesign.naming.is_none() && find(name).is_some_and(|r| !redesign::named(r)) {
                d.redesign.naming = Some((name.clone(), false, true));
            }
        }
        _ => {}
    }
    if d.rename_table == salt && !matches!(out.editing, Some((_, NAME, _))) {
        d.renaming = None;
    }
    if let Some((name, col)) = &out.cancelled {
        // Back to the draft the cell held before the edit, if any.
        let before = d.typing.take().filter(|(s, n, c, _)| s == salt && n == name && c == col).and_then(|(.., before)| before);
        let drafts = match *col {
            EXPRESSION => Some(&mut d.editing),
            COMMENT => Some(&mut d.comments),
            _ => None,
        };
        if let Some(drafts) = drafts {
            match before {
                Some(before) => drafts.insert(name.clone(), before),
                None => drafts.remove(name),
            };
        }
    }
    if let Some(message) = out.refused {
        d.error = Some(message);
    }
    if let Some(notice) = out.notice {
        d.error = Some(notice);
    }
    if out.edits.is_empty() {
        return;
    }
    let live = app.preferences.parameters_apply_live;
    let mut changes = serde_json::Map::new();
    let mut steps: Vec<(&str, Value)> = Vec::new();
    let mut renames: Vec<(&str, Value)> = Vec::new();
    let mut new_rows: std::collections::BTreeMap<usize, [String; 4]> = Default::default();
    let mut held_error = None;
    let mut comments = Vec::new();
    let mut renamed = Vec::new();
    for edit in &out.edits {
        let text = edit.text();
        match edit.row() {
            ui_kit::RowRef::New(n) => {
                if let Some(slot) = [NAME, UNIT, EXPRESSION, COMMENT].iter().position(|c| *c == edit.col())
                    && let Some(cell) = new_rows.entry(*n).or_default().get_mut(slot)
                {
                    *cell = text.to_owned();
                }
            }
            ui_kit::RowRef::Existing(name) => {
                let Some(r) = find(name) else { continue };
                match edit.col() {
                    EXPRESSION if r.read_only => {}
                    EXPRESSION if !live => {
                        // Kept in `editing` for Apply or OK, checked now (#9279).
                        held_error = held_error.or(check(app, text, &r.unit).err());
                        d.editing.insert(r.name.clone(), text.to_owned());
                    }
                    EXPRESSION => {
                        changes.insert(r.name.clone(), json!(text));
                    }
                    UNIT => {
                        if let Ok(unit) = unit_from(text) {
                            steps.push(("parameters.change", json!({"name": r.name, "expression": r.expression, "unit": unit})));
                        }
                    }
                    COMMENT => {
                        steps.push(("parameters.comment", json!({"name": r.name, "comment": text})));
                        comments.push(r.name.clone());
                    }
                    NAME => {
                        let new = text.trim();
                        if !new.is_empty() && new != r.name {
                            renames.push(("parameters.rename", json!({"name": r.name, "new_name": new})));
                            if d.redesign.naming.as_ref().is_some_and(|(name, star, _)| name == &r.name && *star) {
                                renames.push(("parameters.favorite", json!({"name":new,"favorite":true})));
                            }
                            renamed.push((r.name.clone(), new.to_owned()));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let applied: Vec<String> = changes.keys().cloned().collect();
    // Expressions by their names as they are now, so they go before the renames.
    let mut all: Vec<(&str, Value)> = Vec::new();
    match changes.len() {
        0 => {}
        1 => {
            if let Some((name, expression)) = changes.into_iter().next() {
                all.push(("parameters.change", json!({"name": name, "expression": expression})));
            }
        }
        _ => all.push(("parameters.change", json!({"changes": changes}))),
    }
    all.extend(steps);
    all.extend(renames);
    let unit = UNITS.get(d.new_unit).copied().unwrap_or("mm");
    for [name, unit_text, expression, comment] in new_rows.into_values() {
        if name.trim().is_empty() || expression.trim().is_empty() {
            let message = solidcraft_ui_text::tr("parameters.grid_new_needs").to_owned();
            ui_kit::Grid::refuse(ctx, grid_state(d, salt), &message);
            d.error = Some(message);
            return;
        }
        let unit = if unit_text.trim().is_empty() { Ok(unit) } else { unit_from(&unit_text) };
        let unit = unit.unwrap_or("mm");
        all.push(("parameters.add", json!({"name": name.trim(), "expression": expression, "unit": unit, "comment": comment})));
    }
    if all.is_empty() {
        // Held edits only: the message is the check's.
        d.error = held_error;
        return;
    }
    let ran = match all.as_slice() {
        [("parameters.change", params)] => change(app, d, params.clone()),
        _ => run_as_one(app, &all),
    };
    match ran {
        Ok(()) => {
            for (old, new) in renamed {
                redesign::renamed(app, d, &old, &new);
            }
            d.error = held_error;
            // Listed once, right after the edit, before anything else can change it (#15377).
            let listed = if applied.is_empty() { Vec::new() } else { self::rows(app) };
            for name in applied {
                d.editing.remove(&name);
                // The first live edit keeps the expression to put back (#15377); the design may
                // write the edit differently (`30` becomes `30 mm`), so what it lists now is what
                // Cancel later checks is still there.
                if let Some(r) = find(&name)
                    && let Some(now) = listed.iter().find(|l| l.name == name)
                {
                    d.applied.entry(name).or_insert_with(|| (r.expression.clone(), String::new())).1 = now.expression.clone();
                }
            }
            for name in comments {
                d.comments.remove(&name);
            }
        }
        Err(error) => {
            d.error = Some(error.clone());
            ui_kit::Grid::refuse(ctx, grid_state(d, salt), &error);
        }
    }
}

/// Run a `parameters.change` as the timeline's commands run (#6777): a rebuild that takes a while
/// computes off the window thread, which goes on drawing meanwhile, and its rows show the
/// expressions being applied until the change is in.
fn change(app: &mut SolveApp, d: &mut ParamsDialog, params: Value) -> Result<(), String> {
    let applying: Vec<(String, String)> = match (params.get("name").and_then(Value::as_str), params.get("expression").and_then(Value::as_str)) {
        (Some(name), Some(expression)) => vec![(name.to_owned(), expression.to_owned())],
        _ => params
            .get("changes")
            .and_then(Value::as_object)
            .map(|changes| changes.iter().filter_map(|(name, e)| Some((name.clone(), e.as_str()?.to_owned()))).collect())
            .unwrap_or_default(),
    };
    app.run_heavy("parameters.change", params)?;
    if app.computing_since().is_some() || !app.deferred_queue.is_empty() {
        d.applying.extend(applying);
    }
    Ok(())
}

/// Run `steps` as one gesture: all of them or none, and one undo step for all. A refused step
/// takes back the ones before it; afterwards only the design before the first step stays in the
/// undo history, so one Undo takes the whole paste or fill back.
fn run_as_one(app: &mut SolveApp, steps: &[(&str, Value)]) -> Result<(), String> {
    let before = app.session.doc.clone();
    let undo = app.session.undo.len();
    for (index, (command, params)) in steps.iter().enumerate() {
        if let Err(error) = app.run(command, params.clone()) {
            if index > 0 {
                while app.session.undo.len() > undo && !Arc::ptr_eq(&app.session.doc, &before) {
                    if app.run("edit.undo", json!({})).is_err() {
                        break;
                    }
                }
                app.session.redo.clear();
            }
            return Err(error);
        }
    }
    if steps.len() > 1
        && let Some(first) = app.session.undo.iter().rposition(|s| Arc::ptr_eq(&s.doc, &before))
    {
        app.session.undo.truncate(first + 1);
    }
    Ok(())
}

fn unit_label(u: &str) -> &str {
    if u.is_empty() { solidcraft_ui_text::tr_label("none") } else { u }
}

/// Only a fresh Enter in this field submits; held keys and unrelated focus do not.
fn enter_for(ui: &mut egui::Ui, focused: bool) -> bool {
    focused
        && ui.input_mut(|i| {
            let fresh = i.raw.events.iter().any(|event| {
                matches!(event,
            egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. } if *modifiers == egui::Modifiers::NONE)
            });
            if fresh {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            }
            fresh
        })
}

/// The expression edits waiting for Apply (#9279): each draft that differs from the design.
fn held(d: &ParamsDialog, rows: &[Row]) -> serde_json::Map<String, Value> {
    rows.iter()
        .filter(|r| !r.read_only)
        .filter_map(|r| d.editing.get(&r.name).filter(|e| **e != r.expression).map(|e| (r.name.clone(), json!(e))))
        .collect()
}

/// The hint at the top of the dialog: when edits apply, as the check box under it says.
fn hint(live: bool) -> &'static str {
    solidcraft_ui_text::tr(if live { "manage.parameters_hint" } else { "parameters.held_hint" })
}

/// The footer's note of the edits waiting for Apply, if any.
fn held_status(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some(solidcraft_ui_text::tr("parameters.held_one").to_string()),
        n => Some(solidcraft_ui_text::format("parameters.held", &[("count", &n.to_string())])),
    }
}

/// Set the edits waiting for Apply together: one `parameters.change`, so the design rebuilds once
/// and one undo takes them all back. A single edit goes as `name, expression`, which can also
/// rename (`new = …`). False, with the error shown and the edits kept, when it is refused.
fn apply_held(app: &mut SolveApp, d: &mut ParamsDialog, rows: &[Row]) -> bool {
    let changes = held(d, rows);
    let params = match changes.iter().next() {
        None => return true,
        Some((name, expression)) if changes.len() == 1 => json!({"name": name, "expression": expression}),
        Some(_) => json!({"changes": changes}),
    };
    d.error = change(app, d, params).err();
    if d.error.is_some() {
        return false;
    }
    for name in changes.keys() {
        d.editing.remove(name);
    }
    true
}

/// Put back the expressions applied live since the dialog opened (#15377): one
/// `parameters.change`, so the design rebuilds once and one undo (or redo) covers them. A
/// parameter changed since by something else (an agent, an undo) or gone (renamed, deleted) is
/// left as it is. False, with the error shown, when the put-back is refused; what to put back is kept for the next try.
/// An edit still computing off the window thread (#6777) is not in the design yet: its put-back
/// waits its turn behind it.
fn revert_applied(app: &mut SolveApp, d: &mut ParamsDialog) -> bool {
    if d.applied.is_empty() {
        return true;
    }
    let now = rows(app);
    let changes: serde_json::Map<String, Value> = d
        .applied
        .iter()
        .filter_map(|(name, (before, listed))| {
            let current = &now.iter().find(|r| r.name == *name)?.expression;
            (d.applying.contains_key(name) || (listed == current && current != before)).then(|| (name.clone(), json!(before)))
        })
        .collect();
    if !changes.is_empty() {
        let params = json!({"changes": changes});
        d.error = if d.applying.is_empty() { app.run("parameters.change", params) } else { app.run_heavy("parameters.change", params) }.err();
        if d.error.is_some() {
            return false;
        }
    }
    d.applied.clear();
    true
}

/// "Apply each change as you leave its field", remembered across sessions (#9279). Turning it
/// back on applies what was waiting.
fn apply_live_checkbox(ui: &mut egui::Ui, live: &mut bool, status: Option<&str>) {
    let response = ui_kit::checkbox(ui, solidcraft_ui_text::tr("parameters.apply_live"), live)
        .backing_on_hover_text(solidcraft_ui_text::tr("parameters.apply_live_tip"))
        .backing_on_hover_text(status.unwrap_or_else(|| hint(*live)));
    crate::scenario::publish_target("params:apply_live", response.rect);
}

/// "Group model parameters by component" (#20648), remembered across sessions; off lists the model
/// parameters flat, in listing order.
fn group_checkbox(app: &mut SolveApp, ui: &mut egui::Ui) {
    let response = ui_kit::checkbox(ui, solidcraft_ui_text::tr("parameters.group_by_component"), &mut app.preferences.params_group_by_component);
    crate::scenario::publish_handle("params:group_by_component", response.rect.center());
}

/// The add row's draft (name, unit, expression, comment) an add was refused with, the design
/// document the refusal was about, and the message. The message is about that draft in that
/// design, as a dialog's refused OK is about its inputs and the design (#641): once either
/// changes, the message goes and the expression cell's own live check says what is wrong now, if
/// anything. A design change replaces the session's document (`Session::execute` keeps the one a
/// command started from, so an edit lands at a new address, and a refused command puts the old
/// one back); a selection or display change keeps it, though it moves the session's revision.
/// The row edits' messages live in `ParamsDialog::error`, apart from this one, so neither can
/// be taken for the other.
#[derive(Debug)]
struct NewRefused {
    draft: (String, usize, String, String),
    doc: Arc<solidcraft_engine::doc::Document>,
    error: String,
}

impl NewRefused {
    /// Whether the add row still holds the draft the refusal was about.
    fn same_draft(&self, d: &ParamsDialog) -> bool {
        self.draft.0 == d.new_name && self.draft.1 == d.new_unit && self.draft.2 == d.new_expr && self.draft.3 == d.new_comment
    }
}

fn new_draft(d: &ParamsDialog) -> (String, usize, String, String) {
    (d.new_name.clone(), d.new_unit, d.new_expr.clone(), d.new_comment.clone())
}

/// Show why the add was refused, for as long as the draft and the design stay as they are. It
/// takes the banner from an earlier action's message, as every action's result does.
fn refuse_add(app: &SolveApp, d: &mut ParamsDialog, error: String) {
    d.new_refused = Some(NewRefused { draft: new_draft(d), doc: app.session.doc.clone(), error });
    d.error = None;
}

/// A refused add's message gives way once its draft changes or the design document is another
/// one (the design document the refusal was about; a selection or display change keeps it).
fn forget_refused_add(app: &SolveApp, d: &mut ParamsDialog) {
    if let Some(was) = &d.new_refused
        && (!Arc::ptr_eq(&was.doc, &app.session.doc) || !was.same_draft(d))
    {
        d.new_refused = None;
    }
}

/// The add row, one cell per column of the user table it ends.
fn add_row(app: &mut SolveApp, ui: &mut egui::Ui, table: &mut ui_kit::Table<'_>, d: &mut ParamsDialog, right: f32) {
    let name_id = ui.make_persistent_id("pnew_name");
    let expr_id = ui.make_persistent_id("pnew_expr");
    let comment_id = ui.make_persistent_id("pnew_comment");
    let focused_id = ui.memory(|m| [name_id, expr_id, comment_id].into_iter().find(|id| m.has_focus(*id) || m.had_focus_last_frame(*id)));
    let mut row_ids = vec![name_id, expr_id, comment_id];
    let menu_was_open = egui::Popup::is_any_open(ui.ctx()) || crate::ui_kit::Menu::any_open(ui.ctx());
    let mut submit = false;
    let mut left = false;
    let mut discard = false;
    let mut comment_rect = egui::Rect::NOTHING;
    let reveal = d.focus_new;
    let order = redesign::column_order(d);
    let response = table.row(ui, "new", false, |row| {
        for &col in order {
            row.cell(|ui| match col {
                NAME => {
                    let response = ui_kit::cell_editor(ui, name_id, &mut d.new_name, "new parameter", false);
                    crate::scenario::publish_handle("param:new:name", response.rect.center());
                    if d.focus_new {
                        response.request_focus();
                        d.focus_new = false;
                    }
                    left |= response.lost_focus();
                }
                UNIT => {
                    let units: Vec<_> = UNITS.iter().map(|u| unit_label(u)).collect();
                    let response = ui_kit::dropdown(ui, "pnew_unit", &mut d.new_unit, &units);
                    row_ids.push(response.id);
                    left |= response.lost_focus();
                    crate::scenario::publish_handle("param:new:unit", response.rect.center());
                }
                EXPRESSION | VALUE => {
                    let unit = UNITS.get(d.new_unit).copied().unwrap_or("mm");
                    let live = if d.new_expr.trim().is_empty() { Ok(String::new()) } else { new_check(app, d, unit) };
                    let (value, invalid) = match live {
                        Ok(v) => (v, false),
                        Err(e) => (e, true),
                    };
                    if col == EXPRESSION {
                        let response = ui_kit::wide_cell_editor(ui, expr_id, &mut d.new_expr, "Expression", invalid, right, (&value, invalid));
                        crate::scenario::publish_handle("param:new:expression", response.rect.center());
                        let completed = complete(ui, &response, &mut d.new_expr);
                        submit = !completed && enter_for(ui, focused_id.is_some());
                        left |= !completed && response.lost_focus();
                    } else {
                        ui_kit::text(ui, &value, ui_kit::Text::Value("new_value", invalid)).backing_on_hover_text(&value);
                    }
                }
                COMMENT => {
                    let response = ui_kit::expandable_cell_editor(ui, comment_id, &mut d.new_comment, "Comment", "param:new:comment");
                    comment_rect = response.rect;
                    crate::scenario::publish_handle("param:new:comment", response.rect.center());
                    left |= response.lost_focus();
                    submit |= enter_for(ui, focused_id.is_some());
                }
                ACTIONS => {
                    let response = ui_kit::bin_button(ui, "Delete");
                    row_ids.push(response.id);
                    crate::scenario::publish_handle("param:new:delete", response.rect.center());
                    left |= response.lost_focus();
                    discard = response.clicked();
                }
                _ => {}
            });
        }
    });
    crate::scenario::publish_rect("dialog:table:user:new", response.rect);
    for label in ["", "Name", "Unit", "Expression", "Value", "Comment", "Actions"] {
        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:table:parameters:new:cell:{label}")) {
            crate::scenario::publish_rect(&format!("dialog:table:user:new:cell:{label}"), rect);
        }
    }
    if reveal {
        table.reveal_row(response.rect);
    }
    let unit = UNITS.get(d.new_unit).copied().unwrap_or("mm");
    let blank = d.new_expr.trim().is_empty();
    let live = if blank { Err(String::new()) } else { new_check(app, d, unit) };
    let valid = !d.new_name.trim().is_empty() && live.is_ok();
    crate::scenario::publish_count("param:new:valid", f64::from(valid));
    let clicked = ui.input(|i| i.pointer.any_click().then(|| i.pointer.interact_pos()).flatten());
    let inside_row = |p| response.rect.contains(p) || comment_rect.contains(p);
    let clicked_outside = !menu_was_open && clicked.is_some_and(|p| !inside_row(p));
    // Losing a cell's focus is not leaving the draft: a click or Tab into another cell
    // must keep its values and geometry (#4461). The unit menu and comment overlay belong to the draft too.
    let left_row = !menu_was_open
        && !(egui::Popup::is_any_open(ui.ctx()) || crate::ui_kit::Menu::any_open(ui.ctx()))
        && (clicked_outside || (left && !ui.memory(|m| row_ids.iter().any(|id| m.has_focus(*id))) && !clicked.is_some_and(inside_row)));
    let abandoned = d.new_name.trim().is_empty() && clicked_outside;
    if discard || abandoned {
        d.adding = false;
        d.new_name.clear();
        d.new_comment.clear();
        d.new_expr.clear();
    } else if submit || (left_row && valid) {
        if blank {
            // Enter after a name, before an expression is typed: on to the expression box. On an
            // empty row (as after an add), the box keeps the focus a single-line Enter gives up.
            let to = if d.new_name.trim().is_empty() { focused_id } else { Some(expr_id) };
            if let Some(id) = to {
                ui.memory_mut(|m| m.request_focus(id));
            }
            return;
        }
        if let Err(error) = live {
            refuse_add(app, d, error);
            if let Some(id) = focused_id {
                ui.memory_mut(|m| m.request_focus(id));
            }
            return;
        }
        let params = json!({"name":d.new_name.trim(),"expression":d.new_expr,"unit":unit,"comment":d.new_comment});
        match app.run("parameters.add", params) {
            Ok(_) => {
                d.new_name.clear();
                d.new_comment.clear();
                d.new_expr.clear();
                d.error = None;
                d.adding = true;
                d.focus_new = submit;
            }
            Err(error) => {
                refuse_add(app, d, error);
                // Enter stays in the field that was submitted. A click into another cell
                // has to keep that cell, or an invalid name cannot be corrected (#3965).
                if submit && let Some(id) = focused_id {
                    ui.memory_mut(|m| m.request_focus(id));
                }
            }
        }
    }
}

/// A completion offered under an expression box.
#[derive(Clone, Debug)]
pub struct Suggestion {
    pub text: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Clone, Default)]
struct Cache {
    revision: u64,
    all: Arc<Vec<Suggestion>>,
    /// How many parameters Down arrow lists (`Prefs::parameter_list_rows`).
    every: usize,
}

fn cache_id() -> egui::Id {
    egui::Id::unique("sc_param_completions")
}

/// Keep the completion list (parameters, functions, constants, units) for this revision of the
/// design where expression boxes can reach it. Called once per frame.
pub fn publish(app: &mut SolveApp, ctx: &egui::Context) {
    let rev = app.session.revision;
    let every =
        usize::from(app.preferences.parameter_list_rows.clamp(*crate::prefs::PARAMETER_LIST_ROWS.start(), *crate::prefs::PARAMETER_LIST_ROWS.end()));
    if ctx.data(|d| d.get_temp::<Cache>(cache_id())).is_some_and(|c| c.revision == rev && c.every == every) {
        return;
    }
    let mut all: Vec<Suggestion> = Vec::new();
    // Every name (no prefix), then the units, which are offered only after a letter is typed:
    // once for each letter a unit starts with.
    let mut prefixes = vec![String::new()];
    for (unit, _, _) in expr::UNITS {
        let first: String = unit.chars().take(1).collect();
        if !prefixes.contains(&first) {
            prefixes.push(first);
        }
    }
    for prefix in prefixes {
        if let Ok(v) = app.session.execute("parameters.complete", &json!({ "prefix": prefix })) {
            for s in v["suggestions"].as_array().into_iter().flatten() {
                let text = s["text"].as_str().unwrap_or_default().to_string();
                if !text.is_empty() && !all.iter().any(|x| x.text == text) {
                    all.push(Suggestion {
                        text,
                        kind: s["kind"].as_str().unwrap_or_default().into(),
                        detail: s["detail"].as_str().unwrap_or_default().into(),
                    });
                }
            }
        }
    }
    ctx.data_mut(|d| d.insert_temp(cache_id(), Cache { revision: rev, all: Arc::new(all), every }));
}

/// The identifier being typed before `cursor` (chars) and where it starts.
pub(crate) fn word_before(text: &str, cursor: usize) -> (usize, String) {
    let chars: Vec<char> = text.chars().collect();
    let end = cursor.min(chars.len());
    let mut start = end;
    while start > 0 && chars.get(start - 1).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
        start -= 1;
    }
    (start, chars.get(start..end).map(|s| s.iter().collect()).unwrap_or_default())
}

/// Suggestions for the word being typed.
#[cfg(test)]
pub fn suggestions(ctx: &egui::Context, word: &str) -> Vec<Suggestion> {
    suggestions_for(ctx, word, false)
}

pub(crate) fn suggestions_for(ctx: &egui::Context, word: &str, explicit: bool) -> Vec<Suggestion> {
    if (!explicit && word.is_empty()) || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Vec::new();
    }
    let all = ctx.data(|d| d.get_temp::<Cache>(cache_id())).map(|c| c.all).unwrap_or_default();
    // A complete token stays out of its own list; longer names that start with it are still
    // offered (passively: see `expression_complete`, #3663), but only those: `in` lists `inner`,
    // not every name with `in` somewhere in it. A typed unit is complete in every sense, so only
    // parameters, functions and constants extend it: `in` does not offer `inch`.
    let exact = !explicit && all.iter().any(|s| s.text == word);
    let w = word.to_ascii_lowercase();
    let offered = |s: &&Suggestion| (explicit || s.text.trim_end_matches('(') != word) && !(exact && s.kind == "unit");
    let starts = |s: &&Suggestion| s.text.to_ascii_lowercase().starts_with(&w);
    // A parameter is also offered for a word anywhere in its name (`side` finds
    // `cab_side_thickness`, #2757), after the names that start with it, and then by the starts
    // of its words in order (`toodia` finds `tooth_outward_flange_diameter`, #11978). That last
    // group comes after the others, so it never pushes an earlier match out of the eight.
    let parameter = |s: &&Suggestion| !exact && s.kind == "parameter" && !starts(s);
    let within = |s: &&Suggestion| parameter(s) && s.text.to_ascii_lowercase().contains(&w);
    let words = |s: &&Suggestion| parameter(s) && !within(s) && solidcraft_engine::doc::name_words_match(&s.text, word);
    all.iter()
        .filter(|s| starts(s) && offered(s))
        .chain(all.iter().filter(|s| within(s) && offered(s)))
        .chain(all.iter().filter(|s| words(s) && offered(s)))
        .take(8)
        .cloned()
        .collect()
}

/// The named parameters, in the order the list offers them (favourites and the design's own
/// first), up to the Preferences' cap rather than a typed word's eight: what Down arrow in an
/// empty expression box shows (#9246).
pub(crate) fn every_parameter(ctx: &egui::Context) -> Vec<Suggestion> {
    let Some(cache) = ctx.data(|d| d.get_temp::<Cache>(cache_id())) else { return Vec::new() };
    cache.all.iter().filter(|s| s.kind == "parameter").take(cache.every).cloned().collect()
}

/// Is `word` itself one of the known tokens (a parameter or dimension name, a constant or a
/// unit)? Such a word is ready to submit as typed; the popup then only offers longer names.
pub(crate) fn exact_token(ctx: &egui::Context, word: &str) -> bool {
    let all = ctx.data(|d| d.get_temp::<Cache>(cache_id())).map(|c| c.all).unwrap_or_default();
    all.iter().any(|s| s.text == word)
}

/// Is `word` a unit (`mm`, `in`, `deg`, ...)?
pub(crate) fn unit_token(ctx: &egui::Context, word: &str) -> bool {
    let all = ctx.data(|d| d.get_temp::<Cache>(cache_id())).map(|c| c.all).unwrap_or_default();
    all.iter().any(|s| s.kind == "unit" && s.text == word)
}

/// Offer keyboard and pointer completions under a focused expression box, and step a plain
/// number in it with Up/Down or the wheel (`value_step`, #2320).
/// Returns true when either changed the text; callers must not submit that frame.
pub fn complete(ui: &mut egui::Ui, resp: &egui::Response, text: &mut String) -> bool {
    let changed = crate::expression_complete::complete(ui, resp, text) || crate::value_step::step(ui, resp, text);
    ui_kit::note_numeric_edit(ui, resp, Some(text), resp.changed(), changed);
    changed
}

/// SolidCraft's expression language for the kit's value fields: literals and units as
/// `doc::expr` reads them, completion and stepping by [`complete`].
pub(crate) struct DocExpressions;

impl ui_kit::Expressions for DocExpressions {
    fn is_literal(&self, text: &str) -> bool {
        solidcraft_engine::doc::expr::is_literal(text)
    }

    fn is_unit(&self, unit: &str) -> bool {
        solidcraft_engine::doc::expr::unit_info(unit).is_some()
    }

    fn unit_is_prefix(&self, unit: &str) -> bool {
        let unit = if unit == "μ" { "µ" } else { unit };
        // Only a unit the typing can still reach with letters counts: `mm` is complete although
        // `mm²` and `mm³` start with it.
        !unit.is_empty()
            && solidcraft_engine::doc::expr::UNITS
                .iter()
                .any(|(name, _, _)| name.strip_prefix(unit).is_some_and(|rest| !rest.is_empty() && rest.chars().all(char::is_alphabetic)))
    }

    fn complete(&self, ui: &mut egui::Ui, response: &egui::Response, text: &mut String) -> bool {
        complete(ui, response, text)
    }

    fn unit_scale(&self, units: &str) -> (f64, &'static str) {
        crate::prefs::unit_scale(units)
    }

    fn length_number(&self, mm: f64, unit: &str, decimals: u8) -> String {
        crate::prefs::length_number(mm, unit, decimals)
    }

    fn parse_length_mm(&self, text: &str, scale: f64) -> Option<f64> {
        crate::prefs::parse_length_mm(text, scale)
    }

    fn scrub_step(&self, unit: &str, half_height_mm: Option<f64>, current_value: f64) -> f64 {
        match solidcraft_engine::doc::expr::unit_info(unit) {
            Some((solidcraft_engine::doc::expr::Kind::Angle, _)) => {
                if unit.trim().eq_ignore_ascii_case("rad") {
                    0.01
                } else {
                    1.0
                }
            }
            Some((solidcraft_engine::doc::expr::Kind::Length, scale)) if scale > 0.0 => {
                let hh_mm = half_height_mm.unwrap_or(100.0);
                let hh_unit = hh_mm / scale;
                ui_kit::snap_to_125((hh_unit * 0.001).max(1e-9))
            }
            _ => ui_kit::scrub_step(current_value),
        }
    }
}

/// Give the kit's value fields in `ctx` SolidCraft's expressions, unless they have them already.
/// The app calls this at the start of each frame entry point, so every context it draws into has them.
pub(crate) fn install_expressions(ctx: &egui::Context) {
    if ui_kit::expressions(ctx).is_none() {
        ui_kit::install_expressions(ctx, Arc::new(DocExpressions));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redesign_model() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.app.run("solid.box", json!({"length":10,"width":20,"height":30})).unwrap();
        h.app.start("parameters.change");
        h.frames(4);
        h
    }

    #[test]
    fn params_redesign_header_and_footer_match_the_reference_layout() {
        let mut h = redesign_model();
        let page = crate::scenario::rect_of("params:view:named").unwrap();
        let filter = crate::scenario::rect_of("dialog:Filter").unwrap();
        assert!((filter.width() - 200.0).abs() < 1.0, "{filter:?}");
        assert!((filter.height() - 28.0).abs() < 1.0, "{filter:?}");
        let import = crate::scenario::rect_of("params:import").unwrap();
        let csv = crate::scenario::rect_of("params:export_csv").unwrap();
        let json = crate::scenario::rect_of("params:export_json").unwrap();
        assert!(page.right() < import.left() && import.right() < csv.left() && csv.right() < json.left() && json.right() < filter.left());
        for rect in [page, import, csv, json] {
            assert!((rect.center().y - filter.center().y).abs() < 1.0);
        }
        let check = crate::scenario::rect_of("params:apply_live").unwrap();
        let footer = crate::scenario::rect_of("dialog:footer").unwrap();
        let cancel = crate::scenario::handle_at("dialog:Cancel").unwrap();
        let ok = crate::scenario::handle_at("dialog:OK").unwrap();
        assert!(footer.contains_rect(check) && check.right() < cancel.x && cancel.x < ok.x);
        assert!((check.center().y - ok.y).abs() < 1.0);
        assert!(crate::scenario::rect_of("dialog:hint:manage").is_none());
        assert!(!h.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == hint(true))));
        for (key, rect) in
            [("parameters.redesign.import", import), ("parameters.redesign.export_csv", csv), ("parameters.redesign.export_json", json)]
        {
            assert!(!solidcraft_ui_text::tr(key).ends_with(['…', '.']));
            assert!(
                h.shapes
                    .iter()
                    .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == solidcraft_ui_text::tr(key) && rect.contains(t.pos))),
                "{key} should have a visible label"
            );
        }
        h.step(&json!({"click":{"handle":"params:header:actions"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"params:cell_ranges"}})).unwrap();
        assert!(h.app.params.as_ref().unwrap().redesign.ranges);
    }

    #[test]
    fn params_redesign_bulk_actions_are_inside_the_table_and_clear_selection() {
        let mut h = redesign_model();
        for name in ["width", "height"] {
            h.app.run("parameters.add", json!({"name":name,"expression":"5 mm"})).unwrap();
            h.app.params.as_mut().unwrap().redesign.selected.insert(name.to_owned());
        }
        h.frames(4);
        let bar = crate::scenario::rect_of("params:bulk").unwrap();
        let table = crate::scenario::rect_of("dialog:table:parameters:viewport").unwrap();
        assert!(table.contains_rect(bar), "{table:?} should contain {bar:?}");
        for index in 0..4 {
            let action = crate::scenario::rect_of(&format!("params:bulk:action:{index}")).unwrap();
            assert!((action.height() - 24.0).abs() < 0.1);
            assert!(bar.contains_rect(action));
        }
        h.step(&json!({"click":{"handle":"params:bulk:action:0"}})).unwrap();
        assert!(h.app.session.doc.favorites.contains("width") && h.app.session.doc.favorites.contains("height"));
        h.step(&json!({"click":{"handle":"params:header:actions"}})).unwrap();
        h.step(&json!({"click":{"handle":"params:name_prefix"}})).unwrap();
        h.run(&json!([{"text":"size_"}])).unwrap();
        h.step(&json!({"click":{"handle":"params:apply_prefix"}})).unwrap();
        h.frames(2);
        assert!(h.app.session.doc.param("size_width").is_some() && h.app.session.doc.param("size_height").is_some());
        h.step(&json!({"click":{"handle":"params:bulk:clear"}})).unwrap();
        h.frames(2);
        assert!(h.app.params.as_ref().unwrap().redesign.selected.is_empty());
        assert!(crate::scenario::rect_of("params:bulk").is_none());
    }

    #[test]
    fn params_redesign_used_by_is_plain_text_and_still_opens_readers() {
        let mut h = redesign_model();
        h.app.run("parameters.add", json!({"name":"width","expression":"5 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name":"double_width","expression":"width * 2"})).unwrap();
        h.frames(4);
        let cell = crate::scenario::rect_of("param:users:width").unwrap();
        assert!(!h.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == cell && (r.fill.a() > 0 || r.stroke.width > 0.0))));
        h.step(&json!({"click":{"handle":"param:users:width"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("param:reader:width:double_width").is_some());
    }

    /// The Used by list follows a reading parameter, selects every reading feature at once, and
    /// names the uses it cannot select (#20879).
    #[test]
    fn used_by_follows_a_parameter_selects_all_and_names_other_uses() {
        let mut h = redesign_model();
        h.app.run("parameters.add", json!({"name":"w","expression":"10 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name":"depth","expression":"w * 2"})).unwrap();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Outline"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.circle.center", json!({"center":[40,0],"radius":2})).unwrap();
        h.app.run("sketch.dimension", json!({"entities":["c1"],"type":"radius","value":"w"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let block = h.app.run("solid.box", json!({"length":"w","width":10,"height":5})).unwrap()["feature"].as_u64().unwrap();
        h.app.start("parameters.change");
        h.frames(4);
        let open = |h: &mut crate::scenario::Harness| {
            h.step(&json!({"click":{"handle":"param:users:w"}})).unwrap();
            h.frames(2);
        };
        open(&mut h);
        h.step(&json!({"click":{"handle":"param:readers:w:all"}})).unwrap();
        h.frames(2);
        let selected = h.app.session.selection.clone();
        for id in [sketch, block] {
            assert!(selected.contains(&solidcraft_engine::Sel::Feature { id }), "{selected:?}");
        }
        open(&mut h);
        h.step(&json!({"click":{"handle":"param:reader:w:depth"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.params.as_ref().map(|d| d.filter.as_str()), Some("depth"));
        assert_eq!(h.app.session.selection, selected, "a parameter reader filters, it doesn't select");

        // A use the list cannot select is still named: a component's quantity reads `span`.
        h.app.run("parameters.add", json!({"name":"span","expression":"1","unit":"m"})).unwrap();
        h.app.run("component.create", json!({"name":"Cable","virtual":true,"bom_quantity":"2 * span"})).unwrap();
        let users = h.app.session.execute("parameters.users", &json!({"name":"span"})).unwrap();
        assert_eq!(redesign::other_uses(&users), vec!["Cable".to_string()]);
    }

    #[test]
    fn params_redesign_narrow_key_section_scrolls_to_its_last_control_above_the_footer() {
        let mut h = redesign_model();
        h.ctx.data_mut(|d| {
            d.insert_persisted(egui::Id::unique("sc_params_width"), 520.0_f32);
            d.insert_persisted(egui::Id::unique("sc_params_height"), 360.0_f32);
        });
        for index in 0..12 {
            let name = format!("key_{index}");
            h.app.run("parameters.add", json!({"name":name,"expression":"5 mm"})).unwrap();
            h.app.run("parameters.favorite", json!({"name":name,"favorite":true})).unwrap();
        }
        h.app.params.as_mut().unwrap().redesign.compact = Some(Default::default());
        h.frames(6);
        let last = h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().rfind(|r| r.favorite).unwrap().name.clone();
        let filter = crate::scenario::rect_of("dialog:Filter").unwrap();
        assert!((filter.width() - 130.0).abs() < 1.0);
        let viewport = crate::scenario::rect_of("dialog:body:viewport").unwrap();
        h.call("ui.scroll", json!({"x":viewport.center().x,"y":viewport.center().y,"delta":-100_000}));
        h.frames(40);
        assert!(crate::scenario::count_of("dialog:body:scroll_y").unwrap() > 0.0);
        let viewport = crate::scenario::rect_of("dialog:body:viewport").unwrap();
        let reset = crate::scenario::rect_of("params:key:reset").unwrap();
        let footer = crate::scenario::rect_of("dialog:footer").unwrap();
        assert!(viewport.contains_rect(reset), "{viewport:?} should reveal {reset:?}");
        let last_field = crate::scenario::rect_of(&format!("params:key:{last}")).unwrap();
        let last_slider = crate::scenario::rect_of(&format!("params:key:slider:{last}")).unwrap();
        assert!(viewport.contains_rect(last_field) && viewport.contains_rect(last_slider));
        assert!(reset.bottom() < footer.top());
        h.step(&json!({"click":{"handle":"params:key:reset"}})).unwrap();
        assert!(h.app.params.as_ref().unwrap().error.is_none());
    }

    #[test]
    fn params_redesign_promote_and_star_are_one_undo_step() {
        let mut h = redesign_model();
        let all = rows(&mut h.app);
        let row = all.iter().find(|r| r.source == "feature").unwrap();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let mut d = h.app.params.take().unwrap();
        d.redesign.start_name(row, true);
        apply_grid(
            &mut h.app,
            &h.ctx,
            &mut d,
            &[row],
            &row.group,
            ui_kit::GridOutput {
                edits: vec![ui_kit::Edit::Set { row: ui_kit::RowRef::Existing(row.name.clone()), col: NAME, text: "overall_length".into() }],
                ..Default::default()
            },
        );
        assert!(d.error.is_none(), "{:?}", d.error);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(h.app.session.doc.favorites.contains("overall_length"));
        assert!(d.redesign.view == redesign::View::Named);
        assert!(d.redesign.selected.contains("overall_length"));
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn params_redesign_taken_and_invalid_name_leave_the_dimension_alone() {
        let mut h = redesign_model();
        h.app.run("parameters.add", json!({"name":"width","expression":"5 mm"})).unwrap();
        let all = rows(&mut h.app);
        let row = all.iter().find(|r| r.source == "feature").unwrap();
        let before = h.app.session.doc.to_json();
        for name in ["width", "1bad", "sin", "d99", ""] {
            assert!(redesign::validate_name(&h.app, &row.name, name, true).is_err(), "{name}");
        }
        assert!(redesign::validate_name(&h.app, &row.name, "overall_width", true).is_ok());
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn params_redesign_name_it_keeps_a_taken_name_inline_then_promotes() {
        let mut h = redesign_model();
        h.app.run("parameters.add", json!({"name":"width","expression":"5 mm"})).unwrap();
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.frames(3);
        let row = h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().find(|r| r.source == "feature").unwrap().clone();
        let band = format!("model:{}", row.group);
        if !h.app.params.as_ref().unwrap().bands[&band] {
            h.step(&json!({"click":{"handle":format!("dialog:table:parameters:group:{band}")}})).unwrap();
        }
        h.frames(3);
        h.step(&json!({"click":{"handle":format!("param:name_it:{}", row.name)}})).unwrap();
        h.frames(3);
        let undo = h.app.session.undo.len();
        h.run(&json!([{"key":"A","cmd":true},{"text":"width"},{"key":"Enter"}])).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(h.app.params.as_ref().unwrap().banner().unwrap().contains("already a parameter"));
        assert!(h.app.params.as_ref().unwrap().redesign.naming.is_some());
        h.run(&json!([{"key":"A","cmd":true},{"text":"overall_width"},{"key":"Enter"}])).unwrap();
        h.frames(3);
        assert!(h.app.params.as_ref().unwrap().banner().is_none());
        assert!(h.app.params.as_ref().unwrap().redesign.view == redesign::View::Named);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(crate::scenario::handle_at("param:overall_width").is_some());
    }

    #[test]
    fn params_redesign_star_asks_for_a_name_and_escape_cancels_both() {
        let mut h = redesign_model();
        let row = rows(&mut h.app).into_iter().find(|r| r.source == "feature").unwrap();
        let before = h.app.session.doc.to_json();
        h.app.params.as_mut().unwrap().redesign.start_name(&row, true);
        assert!(!h.app.session.doc.favorites.contains(&row.name));
        h.run(&json!([{"key":"Escape"}])).unwrap();
        assert!(h.app.params.as_ref().unwrap().redesign.naming.is_none());
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn params_redesign_demote_updates_readers_and_unpins_in_one_undo() {
        let mut h = redesign_model();
        let old = rows(&mut h.app).into_iter().find(|r| r.source == "feature").unwrap().name;
        h.app.run("parameters.rename", json!({"name":old,"new_name":"overall_width"})).unwrap();
        h.app.run("parameters.favorite", json!({"name":"overall_width"})).unwrap();
        h.app.run("parameters.add", json!({"name":"double_width","expression":"overall_width * 2"})).unwrap();
        let all = rows(&mut h.app);
        let row = all.iter().find(|r| r.name == "overall_width").unwrap();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let mut d = h.app.params.take().unwrap();
        redesign::demote(&mut h.app, &mut d, row, &all).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(!h.app.session.doc.favorites.contains("overall_width"));
        assert!(!h.app.session.doc.param("double_width").unwrap().expr.contains("overall_width"));
        assert!(d.redesign.view == redesign::View::Dimensions);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn params_redesign_counts_deduplicate_graph_edges_in_one_pass() {
        let graph = json!({"edges":[["height","width"],["height","width"],["Feature:Box1","width"],["d1","wall"]]});
        let counts = redesign::readers(&graph);
        assert_eq!(counts["width"], ["Feature:Box1", "height"]);
        assert_eq!(counts["wall"], ["d1"]);
        assert!(!counts.contains_key("height"));
    }

    #[test]
    fn params_redesign_view_switch_is_remembered_across_openings() {
        let mut h = redesign_model();
        assert!(h.app.params.as_ref().unwrap().redesign.view == redesign::View::Named);
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.frames(3);
        assert!(h.app.params.as_ref().unwrap().redesign.view == redesign::View::Dimensions);
        h.app.params = None;
        h.frames(2);
        h.app.start("parameters.change");
        h.frames(3);
        assert!(h.app.params.as_ref().unwrap().redesign.view == redesign::View::Dimensions);
        h.app.run("parameters.add", json!({"name":"width","expression":"10 mm"})).unwrap();
        h.step(&json!({"click":{"handle":"params:view:named"}})).unwrap();
        h.frames(3);
        h.run(&json!([{"click":{"handle":"param:width"}},{"key":"A","cmd":true},{"text":"20 mm"}])).unwrap();
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "20 mm", "switching tables commits a live field as focus leaves it");
    }

    #[test]
    fn params_redesign_key_band_contains_each_named_row_once() {
        let all = [
            Row { name: "width".into(), source: "user".into(), favorite: true, ..Default::default() },
            Row { name: "d1".into(), source: "sketch".into(), group: "Sketch1".into(), ..Default::default() },
            Row { name: "hole_radius".into(), source: "sketch".into(), group: "Sketch1".into(), ..Default::default() },
        ];
        let visible: Vec<_> = all.iter().collect();
        let shown = sections(&visible, ui_kit::Sort::Listed);
        assert_eq!(shown.favorites.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["width"]);
        assert_eq!(shown.users.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["hole_radius"]);
        assert_eq!(shown.groups[0].1.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["d1"]);
    }

    #[test]
    fn params_redesign_measures_comes_from_the_constraint_and_entity() {
        use solidcraft_engine::sketch::{ConstraintKind, Sketch};
        crate::i18n::install();
        let mut sketch = Sketch::default();
        sketch.add_circle(solidcore_record::Vec2::new(0.0, 0.0), 2.0, None, Some("Circle2")).unwrap();
        assert_eq!(redesign::measure(&sketch, &ConstraintKind::Radius { c: 0, value: 2.0 }), "Radius, Circle2");
        assert!(redesign::measure(&sketch, &ConstraintKind::Radius { c: usize::MAX, value: 2.0 }).starts_with("Radius"));
    }

    #[test]
    fn params_redesign_selection_follows_and_opens_its_group() {
        let mut h = redesign_model();
        h.app.run("solid.box", json!({"length":5,"width":6,"height":7})).unwrap();
        let doc = Arc::make_mut(&mut h.app.session.doc);
        let parent = doc.add_component(Some("Frame"), 0).unwrap();
        let child = doc.add_component(Some("Lid"), parent).unwrap();
        doc.features.last_mut().unwrap().component = child;
        let all = rows(&mut h.app);
        let visible: Vec<_> = all.iter().collect();
        let mut sections = sections(&visible, ui_kit::Sort::Listed);
        let feature = h.app.session.doc.features.last().unwrap();
        let (id, name) = (feature.id, feature.name.clone());
        h.app.run("select.set", json!({"items":[{"type":"feature","id":id}]})).unwrap();
        let mut d = h.app.params.take().unwrap();
        d.bands.insert(format!("component:{parent}"), false);
        d.bands.insert(format!("component:{child}"), false);
        redesign::follow_selection(&h.app, &mut d, &mut sections);
        assert_eq!(sections.groups[0].0, name);
        assert!(d.bands[&format!("model:{name}")]);
        assert!(d.bands[&format!("component:{parent}")] && d.bands[&format!("component:{child}")], "selection opens its component ancestors");
        h.app.run("select.clear", json!({})).unwrap();
        redesign::follow_selection(&h.app, &mut d, &mut sections);
        assert!(d.bands[&format!("model:{name}")], "clearing selection keeps the group's open state");
    }

    #[test]
    fn params_redesign_narrow_columns_preserve_edit_semantics() {
        let mut d = ParamsDialog::new();
        d.redesign.narrow = true;
        let order = redesign::column_order(&d);
        assert_eq!(order, [STAR, NAME, EXPRESSION, VALUE, ACTIONS]);
        let out = redesign::project_output(ui_kit::GridOutput { editing: Some(("width".into(), 2, "30".into())), ..Default::default() }, order);
        assert_eq!(out.editing.unwrap().1, EXPRESSION);
    }

    #[test]
    fn numeric_field_waits_for_unit_completion() {
        let ctx = crate::fonts::test_context();
        install_expressions(&ctx);
        let mut value = String::new();
        let mut id = egui::Id::NULL;
        let mut draw = |time, text: &str| {
            let events = if text.is_empty() { vec![] } else { vec![egui::Event::Text(text.into())] };
            let mut ready = true;
            let output = ui_kit::test_support::raw_frame(&ctx, egui::RawInput { time: Some(time), events, ..Default::default() }, |ui| {
                id = ui_kit::checked_field(ui, &mut value, "", false).1.id;
                ready = ui_kit::numeric_preview_ready(ui.ctx());
            });
            ctx.memory_mut(|m| m.request_focus(id));
            (value.clone(), ready, output)
        };
        draw(0.0, "");
        assert_eq!(draw(0.01, "2").0, "2", "a complete number updates immediately");
        let (text, ready, output) = draw(0.02, "m");
        assert_eq!(text, "2m", "completion and caret handling retain the draft");
        assert!(!ready, "m may still become mm or mil; do not preview two metres");
        assert!(ui_kit::test_support::text_shapes(&output).any(|s| s.galley.text() == "2m"), "the draft remains visible");
        let (text, ready, _) = draw(0.03, "m");
        assert_eq!(text, "2mm");
        assert!(ready, "a completed unit updates immediately");
    }

    #[test]
    fn numeric_unit_prefixes_follow_the_expression_languages_units() {
        let host: &dyn ui_kit::Expressions = &DocExpressions;
        for unit in ["m", "mi", "i", "in", "r", "µ", "μ"] {
            assert!(host.unit_is_prefix(unit), "{unit}");
        }
        for unit in ["", "mm", "cm", "rad", "inch", "R"] {
            assert!(!host.unit_is_prefix(unit), "{unit}");
        }
    }

    #[test]
    fn parameter_names_show_one_tooltip_and_only_repeat_clipped_names() {
        fn text_in(shape: &egui::Shape, within: egui::Rect, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) if within.contains(text.pos) => texts.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text_in(shape, within, texts);
                    }
                }
                _ => {}
            }
        }

        let long = "Driver_MountHole_pitch_diameter_".repeat(2);
        for name in ["rip_length", long.as_str()] {
            for favorite in [false, true] {
                let mut h = crate::scenario::Harness::new();
                h.app.home.open = false;
                h.app.run("parameters.add", json!({"name":name,"expression":"140 mm"})).unwrap();
                h.app.run("parameters.favorite", json!({"name":name,"favorite":favorite})).unwrap();
                h.step(&json!({"start":"parameters.change"})).unwrap();
                h.frames(6);
                let at = crate::scenario::handle_at(&format!("param:name:{name}")).unwrap();
                h.step(&json!({"move":[at.x,at.y]})).unwrap();
                h.frames(90);
                let tooltips: Vec<_> = h.ctx.memory(|m| {
                    m.areas()
                        .visible_layer_ids()
                        .into_iter()
                        .filter(|layer| layer.order == egui::Order::Tooltip)
                        .filter_map(|layer| m.area_rect(layer.id))
                        .collect()
                });
                assert_eq!(tooltips.len(), 1, "one name must not stack tooltips: {name}, favorite={favorite}");
                let mut texts = Vec::new();
                for shape in &h.shapes {
                    text_in(&shape.shape, tooltips[0], &mut texts);
                }
                let hint = solidcraft_ui_text::tr_label("Double-click to rename");
                let expected = if name == "rip_length" { hint.to_owned() } else { format!("{name}\n{hint}") };
                // The flattened paint stream includes table labels behind the tooltip (Used by).
                // The tooltip paints last; its own text must still be exactly this one message.
                assert_eq!(texts.last(), Some(&expected), "only a clipped name needs repeating");
                assert_eq!(texts.iter().filter(|text| **text == expected).count(), 1);
                h.step(&json!({"click":{"handle":format!("param:name:{name}")},"double":true})).unwrap();
                assert!(h.app.params.as_ref().unwrap().renaming.is_some(), "the name still renames by double-click");
            }
        }
    }

    fn escape_dialog() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.add", json!({"name":"width","expression":"10 mm","comment":"Saved comment"})).unwrap();
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(3);
        h
    }

    fn escape_state(h: &crate::scenario::Harness) -> Value {
        let history = |steps: &[solidcraft_engine::Snapshot]| {
            steps
                .iter()
                .map(|s| json!({"label":s.label,"document":s.doc.to_json(),"sketch":s.active_sketch,"moves":s.pending_moves}))
                .collect::<Vec<_>>()
        };
        json!({
            "document":h.app.session.doc.to_json(), "revision":h.app.session.revision,
            "undo":history(&h.app.session.undo), "redo":history(&h.app.session.redo),
            "dirty":h.app.session.is_dirty(), "selection":h.app.session.selection
        })
    }

    fn escape_type(h: &mut crate::scenario::Harness, handle: &str, text: &str) {
        h.run(&json!([
            {"click":{"handle":handle}}, {"key":"A","cmd":true}, {"text":text}
        ]))
        .unwrap();
    }

    fn escape_key(h: &mut crate::scenario::Harness, pressed: bool) {
        h.app.synthetic.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.frame();
    }

    fn escape_closed(h: &mut crate::scenario::Harness, before: &Value, saved_expression: &str) {
        escape_key(h, true);
        assert!(h.app.params.is_none(), "the first Escape press closes Parameters");
        assert_eq!(&escape_state(h), before, "cancelling does not change the document, history or selection");
        assert!(
            !h.app.preview.active && !h.app.preview.busy && h.app.preview.replaced.is_empty(),
            "the parameter preview is cleared in the same frame"
        );
        escape_key(h, false);
        h.step(&json!({"start":"parameters.change"})).unwrap();
        let d = h.app.params.as_ref().unwrap();
        assert!(d.editing.is_empty() && d.comments.is_empty() && d.renaming.is_none() && d.preview.is_none());
        assert!(d.new_name.is_empty() && d.new_comment.is_empty() && !d.adding);
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, saved_expression);
    }

    #[test]
    fn escape_closes_parameters_without_clearing_selection_or_history() {
        let mut h = escape_dialog();
        h.app.run("parameters.change", json!({"name":"width","expression":"12 mm"})).unwrap();
        h.app.run("edit.undo", json!({})).unwrap();
        h.app.session.selection = vec![solidcraft_engine::Sel::Plane { name: "XY".into() }];
        h.step(&json!({"click":{"handle":"params:title"}})).unwrap();
        assert!(!h.app.session.undo.is_empty() && !h.app.session.redo.is_empty());
        let before = escape_state(&h);
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_discards_valid_parameter_expression_and_preview() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        escape_type(&mut h, "param:width", "20 mm");
        h.frames(2);
        assert_eq!(h.app.params.as_ref().unwrap().preview.as_ref().map(|(_, e)| e.as_str()), Some("20 mm"));
        assert!(h.app.preview.active || h.app.preview.busy);
        assert_eq!(escape_state(&h), before, "typing only previews the edit");
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_discards_invalid_parameter_expression() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        escape_type(&mut h, "param:width", "missing + 1 mm");
        assert_eq!(h.app.params.as_ref().unwrap().editing.get("width").map(String::as_str), Some("missing + 1 mm"));
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_discards_parameter_rename() {
        let mut h = escape_dialog();
        h.frames(30);
        let before = escape_state(&h);
        h.run(&json!([
            {"click":{"handle":"param:name:width"},"double":true}, {"key":"A","cmd":true}, {"text":"renamed_width"}
        ]))
        .unwrap();
        assert_eq!(h.app.params.as_ref().unwrap().renaming.as_ref().map(|(_, n)| n.as_str()), Some("renamed_width"));
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_discards_parameter_comment() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        h.run(&json!([
            {"widget":"Saved comment"}, {"key":"A","cmd":true}, {"text":"Pending comment"}
        ]))
        .unwrap();
        assert_eq!(h.app.params.as_ref().unwrap().comments.get("width").map(String::as_str), Some("Pending comment"));
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_discards_valid_new_parameter_from_name() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
        escape_type(&mut h, "param:new:expression", "25 mm");
        escape_type(&mut h, "param:new:name", "height");
        assert_eq!(h.app.params.as_ref().unwrap().new_name, "height");
        assert_eq!(crate::scenario::count_of("param:new:valid"), Some(1.0));
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_discards_valid_new_parameter_from_expression() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
        escape_type(&mut h, "param:new:expression", "missing + 1 mm");
        escape_type(&mut h, "param:new:name", "height");
        escape_type(&mut h, "param:new:expression", "25 mm");
        assert_eq!(h.app.params.as_ref().unwrap().new_expr, "25 mm");
        assert_eq!(crate::scenario::count_of("param:new:valid"), Some(1.0));
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_discards_new_parameter_from_comment() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
        escape_type(&mut h, "param:new:expression", "missing + 1 mm");
        escape_type(&mut h, "param:new:name", "height");
        escape_type(&mut h, "param:new:comment", "Pending new comment");
        assert_eq!(h.app.params.as_ref().unwrap().new_comment, "Pending new comment");
        escape_closed(&mut h, &before, "10 mm");
    }

    /// #3399, #15377: Escape drops what is being typed and puts back an edit already applied, in
    /// one undo step.
    #[test]
    fn escape_drops_the_typed_value_and_puts_back_a_previously_committed_parameter_edit() {
        let mut h = escape_dialog();
        escape_type(&mut h, "param:width", "20 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "20 mm");
        let undo = h.app.session.undo.len();
        escape_type(&mut h, "param:width", "30 mm");
        escape_key(&mut h, true);
        assert!(h.app.params.is_none(), "the first Escape press closes Parameters");
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "10 mm", "30 mm is never applied and 20 mm goes back");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one step puts the edit back");
        escape_key(&mut h, false);
    }

    #[test]
    fn escape_dismisses_parameter_completion_before_closing_window() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        escape_type(&mut h, "param:width", "wi");
        assert!(!suggestions(&h.ctx, "wi").is_empty());
        let editor = h.ctx.memory(|m| m.focused()).unwrap();
        escape_key(&mut h, true);
        assert!(h.app.params.is_some(), "the completion popup owns the first Escape");
        assert!(h.ctx.memory(|m| m.has_focus(editor)));
        assert_eq!(h.app.params.as_ref().unwrap().editing.get("width").map(String::as_str), Some("wi"));
        assert_eq!(escape_state(&h), before);
        escape_key(&mut h, false);
        escape_closed(&mut h, &before, "10 mm");
    }

    #[test]
    fn escape_shortcut_capture_has_priority_over_parameters() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        h.call("ui.prefs", json!({"open":"Keyboard Shortcuts"}));
        escape_type(&mut h, "keymap:filter", "solid.extrude");
        h.step(&json!({"click":{"handle":"keymap:capture:solid.extrude"}})).unwrap();
        assert!(crate::keymap::capturing(&h.app));
        escape_key(&mut h, true);
        assert!(!crate::keymap::capturing(&h.app));
        assert!(h.app.prefs_window.shell.open && h.app.params.is_some(), "Escape cancels only shortcut capture");
        assert_eq!(escape_state(&h), before);
    }

    /// #9279: a box driven by `w` and `h`, the Parameters dialog open with live updates off.
    fn held_dialog() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.parameters_apply_live = false;
        h.app.run("parameters.add", json!({"name":"w","expression":"10 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name":"h","expression":"20 mm"})).unwrap();
        h.app.run("solid.box", json!({"length":"w","width":"w","height":"h"})).unwrap();
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(3);
        h
    }

    fn expression(h: &crate::scenario::Harness, name: &str) -> String {
        h.app.session.doc.param(name).map(|p| p.expr.clone()).unwrap_or_default()
    }

    /// Type into a parameter's expression and leave the field, as the user moves on.
    fn hold(h: &mut crate::scenario::Harness, name: &str, text: &str) {
        escape_type(h, &format!("param:{name}"), text);
        h.step(&json!({"click":{"handle":"dialog:Filter"}})).unwrap();
    }

    #[test]
    fn held_edits_wait_for_apply_and_go_in_together_as_one_undo_step() {
        let mut h = held_dialog();
        let (revision, undo) = (h.app.session.revision, h.app.session.undo.len());
        hold(&mut h, "w", "30 mm");
        hold(&mut h, "h", "w * 2");
        assert_eq!((expression(&h, "w").as_str(), expression(&h, "h").as_str()), ("10 mm", "20 mm"), "leaving a field changes nothing yet");
        assert_eq!((h.app.session.revision, h.app.session.undo.len()), (revision, undo), "no rebuild and no undo step while held");
        let d = h.app.params.as_ref().unwrap();
        assert!(d.preview.is_none() && d.error.is_none());
        assert_eq!(d.editing.len(), 2);
        h.step(&json!({"click":{"handle":"dialog:Apply"}})).unwrap();
        assert_eq!((expression(&h, "w").as_str(), expression(&h, "h").as_str()), ("30 mm", "w * 2"));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for both");
        let d = h.app.params.as_ref().expect("Apply keeps the dialog open");
        assert!(d.editing.is_empty() && d.error.is_none());
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!((expression(&h, "w").as_str(), expression(&h, "h").as_str()), ("10 mm", "20 mm"), "one undo takes both back");
    }

    /// #15377: the box of `held_dialog`, the dialog open with live updates on (the default).
    fn live_dialog() -> crate::scenario::Harness {
        let mut h = held_dialog();
        h.app.preferences.parameters_apply_live = true;
        h.frames(2);
        h
    }

    fn reopen(h: &mut crate::scenario::Harness) {
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(2);
    }

    #[test]
    fn live_edits_are_put_back_by_cancel_or_escape_in_one_undo_step_and_kept_by_ok_or_the_close_button() {
        let mut h = live_dialog();
        let undo = h.app.session.undo.len();
        hold(&mut h, "w", "30 mm");
        hold(&mut h, "h", "w * 2");
        hold(&mut h, "w", "35 mm");
        assert_eq!((expression(&h, "w").as_str(), expression(&h, "h").as_str()), ("35 mm", "w * 2"), "live edits apply as each field is left");
        h.step(&json!({"click":{"handle":"dialog:Cancel"}})).unwrap();
        assert!(h.app.params.is_none(), "Cancel closes the dialog");
        assert_eq!((expression(&h, "w").as_str(), expression(&h, "h").as_str()), ("10 mm", "20 mm"), "Cancel puts back what the dialog opened with");
        assert_eq!(h.app.session.undo.len(), undo + 4, "three live edits, then one step that puts both parameters back");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!((expression(&h, "w").as_str(), expression(&h, "h").as_str()), ("35 mm", "w * 2"), "one undo brings the edits back");
        h.app.run("edit.redo", json!({})).unwrap();

        reopen(&mut h);
        hold(&mut h, "w", "30 mm");
        escape_key(&mut h, true);
        escape_key(&mut h, false);
        assert!(h.app.params.is_none());
        assert_eq!(expression(&h, "w"), "10 mm", "Escape puts live edits back");

        reopen(&mut h);
        escape_type(&mut h, "param:w", "30 mm");
        h.step(&json!({"click":{"handle":"dialog:Cancel"}})).unwrap();
        assert!(h.app.params.is_none());
        assert_eq!(expression(&h, "w"), "10 mm", "an edit applied by the Cancel click that leaves its field goes back too");

        reopen(&mut h);
        hold(&mut h, "w", "30 mm");
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        assert!(h.app.params.is_none());
        assert_eq!(expression(&h, "w"), "30 mm", "OK keeps live edits");

        reopen(&mut h);
        hold(&mut h, "w", "40 mm");
        h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
        assert!(h.app.params.is_none());
        assert_eq!(expression(&h, "w"), "40 mm", "the close button only closes, keeping live edits");
    }

    #[test]
    fn cancel_leaves_a_parameter_changed_elsewhere() {
        let mut h = live_dialog();
        hold(&mut h, "w", "30 mm");
        hold(&mut h, "h", "25 mm");
        h.frames(1);
        // An agent changes `w` while the dialog is open: Cancel must not undo that.
        h.app.run("parameters.change", json!({"name":"w","expression":"40 mm"})).unwrap();
        h.frames(1);
        h.step(&json!({"click":{"handle":"dialog:Cancel"}})).unwrap();
        assert_eq!((expression(&h, "w").as_str(), expression(&h, "h").as_str()), ("40 mm", "20 mm"), "only the dialog's own edit goes back");
    }

    /// An agent can change a parameter between the frame that applies a live edit and the next
    /// one that lists the rows: Escape and Cancel leave the agent's expression (#15377 review).
    #[test]
    fn cancel_and_escape_leave_a_change_made_before_the_rows_are_listed_again() {
        for escape in [true, false] {
            let mut h = live_dialog();
            escape_type(&mut h, "param:w", "30 mm");
            // Enter applies the edit in this one frame; nothing lists the rows after it.
            h.app.synthetic.push(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.frame();
            assert_eq!(expression(&h, "w"), "30 mm", "Enter applied the edit live");
            h.app.run("parameters.change", json!({"name":"w","expression":"40 mm"})).unwrap();
            if escape {
                escape_key(&mut h, true);
                escape_key(&mut h, false);
            } else {
                h.step(&json!({"click":{"handle":"dialog:Cancel"}})).unwrap();
            }
            assert!(h.app.params.is_none(), "escape {escape}: the dialog closes");
            assert_eq!(expression(&h, "w"), "40 mm", "escape {escape}: the agent's change stays");
        }
    }

    /// A refused put-back keeps the dialog open with its error, and the next Cancel tries again
    /// (#15377 review).
    #[test]
    fn a_refused_put_back_is_tried_again_by_the_next_cancel() {
        let mut h = live_dialog();
        h.app.run("parameters.add", json!({"name":"k","expression":"5 mm"})).unwrap();
        h.app.run("parameters.change", json!({"name":"w","expression":"k * 2"})).unwrap();
        h.frames(2);
        hold(&mut h, "w", "30 mm");
        assert_eq!(expression(&h, "w"), "30 mm");
        // Without `k`, `k * 2` can't go back.
        h.app.run("parameters.delete", json!({"name":"k"})).unwrap();
        h.frames(1);
        h.step(&json!({"click":{"handle":"dialog:Cancel"}})).unwrap();
        assert!(h.app.params.as_ref().is_some_and(|d| d.error.is_some()), "the refused put-back keeps the dialog open with its error");
        assert_eq!(expression(&h, "w"), "30 mm");
        h.app.run("parameters.add", json!({"name":"k","expression":"5 mm"})).unwrap();
        h.frames(1);
        h.step(&json!({"click":{"handle":"dialog:Cancel"}})).unwrap();
        assert!(h.app.params.is_none(), "the second Cancel puts it back and closes");
        assert_eq!(expression(&h, "w"), "k * 2");
    }

    #[test]
    fn held_edits_go_in_with_ok_and_are_dropped_by_cancel_or_escape() {
        let mut h = held_dialog();
        let before = escape_state(&h);
        hold(&mut h, "w", "30 mm");
        h.step(&json!({"click":{"handle":"dialog:Cancel"}})).unwrap();
        assert!(h.app.params.is_none());
        assert_eq!(escape_state(&h), before, "Cancel drops held edits");
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(2);
        hold(&mut h, "w", "30 mm");
        escape_key(&mut h, true);
        assert!(h.app.params.is_none());
        assert_eq!(escape_state(&h), before, "Escape drops held edits");
        escape_key(&mut h, false);
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(2);
        hold(&mut h, "w", "30 mm");
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        assert!(h.app.params.is_none());
        assert_eq!(expression(&h, "w"), "30 mm", "OK applies held edits");
    }

    #[test]
    fn a_refused_held_edit_keeps_the_dialog_open_with_its_error() {
        let mut h = held_dialog();
        hold(&mut h, "w", "30 mm");
        hold(&mut h, "h", "nonexistent * 2");
        assert!(h.app.params.as_ref().unwrap().error.is_some(), "a held edit is checked when its field is left");
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        let d = h.app.params.as_ref().expect("OK keeps the dialog open when the edits are refused");
        assert!(d.error.is_some());
        assert_eq!(d.editing.len(), 2, "the held edits are kept to correct");
        assert_eq!(expression(&h, "w"), "10 mm", "all or nothing");
    }

    #[test]
    fn the_apply_live_choice_is_remembered_and_turning_it_on_applies_what_waits() {
        crate::i18n::install();
        assert!(crate::prefs::Prefs::default().parameters_apply_live, "live updates stay the default");
        // The footer check box's tooltip says when edits apply, whichever way it is set.
        assert_eq!(hint(true), solidcraft_ui_text::tr("manage.parameters_hint"));
        assert_ne!(hint(false), hint(true));
        assert!(hint(false).contains("Apply") && !hint(false).contains("leaving"), "{}", hint(false));
        let mut h = held_dialog();
        hold(&mut h, "w", "30 mm");
        h.step(&json!({"click":{"handle":"params:apply_live"}})).unwrap();
        assert!(h.app.preferences.parameters_apply_live);
        assert_eq!(expression(&h, "w"), "30 mm");
        assert!(h.app.params.as_ref().unwrap().editing.is_empty());
        h.step(&json!({"click":{"handle":"params:apply_live"}})).unwrap();
        assert!(!h.app.preferences.parameters_apply_live);
        let saved = h.app.prefs();
        let mut other = crate::scenario::Harness::new();
        other.app.load_prefs(&saved);
        assert!(!other.app.preferences.parameters_apply_live, "the choice persists with the preferences");
    }

    /// #20648: the Parameters window's own option turns the grouping on and off, and the choice is kept
    /// with the stored preferences; there is no Preferences page row for it.
    #[test]
    fn the_windows_option_toggles_the_grouping_and_is_remembered() {
        crate::i18n::install();
        assert!(!crate::prefs::Prefs::default().params_group_by_component, "grouping stays off by default");
        let mut h = held_dialog();
        h.frames(3);
        h.step(&json!({"click":{"handle":"params:header:actions"}})).unwrap();
        h.step(&json!({"click":{"handle":"params:group_by_component"}})).unwrap();
        assert!(h.app.preferences.params_group_by_component, "the option turns the grouping on");
        let saved = h.app.prefs();
        let mut other = crate::scenario::Harness::new();
        other.app.load_prefs(&saved);
        assert!(other.app.preferences.params_group_by_component, "the choice persists with the preferences");
        // A new harness clears the published handles: draw the first window again before clicking.
        h.frames(2);
        h.step(&json!({"click":{"handle":"params:group_by_component"}})).unwrap();
        assert!(!h.app.preferences.params_group_by_component, "and off again");
    }

    #[test]
    fn words_and_suggestions() {
        assert_eq!(word_before("2 * wid", 7), (4, "wid".to_string()));
        assert_eq!(word_before("width + 1", 5), (0, "width".to_string()));
        assert_eq!(word_before("1 + ", 4), (4, String::new()));
        assert_eq!(word_before("ü", 9), (0, "ü".to_string()));
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let _ = app.run("parameters.add", json!({"name": "width", "expression": "40 mm"}));
        let _ = app.run("parameters.add", json!({"name": "radius", "expression": "10 mm"}));
        let _ = app.run("parameters.add", json!({"name": "rounding", "expression": "3 mm"}));
        publish(&mut app, &ctx);
        let s = suggestions(&ctx, "wi");
        assert_eq!(s.first().map(|x| x.text.as_str()), Some("width"));
        // A name that extends a unit is still offered at the unit's own prefix (#3663); the
        // prefix itself is a complete token, so the caller knows Enter should submit it.
        assert!(suggestions(&ctx, "rad").iter().any(|x| x.text == "radius"));
        assert!(!suggestions(&ctx, "rad").iter().any(|x| x.text == "rad"));
        // A typed unit is not extended by other units: `in` offers no `inch` (a popup there sat
        // over dialog buttons); a prefix that is not itself a unit still does. A complete token
        // is extended only by names that start with it: `in` does not list `rounding` (a fillet
        // dialog opening on `2 in` showed it over the OK button), while `und` still does (#2757).
        assert!(suggestions(&ctx, "in").is_empty() && suggestions(&ctx, "mm").is_empty());
        assert!(suggestions(&ctx, "und").iter().any(|x| x.text == "rounding"));
        assert!(unit_token(&ctx, "in") && unit_token(&ctx, "rad") && !unit_token(&ctx, "radius") && !unit_token(&ctx, "i"));
        let m: Vec<String> = suggestions(&ctx, "m").into_iter().map(|x| x.kind).collect();
        assert!(!m.is_empty() && m.iter().all(|k| k == "function"), "{m:?}");
        assert!(suggestions(&ctx, "i").iter().any(|x| x.text == "inch"));
        assert!(exact_token(&ctx, "rad") && exact_token(&ctx, "width") && !exact_token(&ctx, "radi") && !exact_token(&ctx, "wi"));
        assert!(suggestions(&ctx, "ata").iter().any(|x| x.text == "atan2("));
        assert!(suggestions(&ctx, "1").is_empty());
        // The dialog's rows come from the engine.
        let r = rows(&mut app);
        assert!(r.iter().any(|x| x.name == "width" && x.source == "user"));
        assert!(check(&mut app, "width / 2", "mm").is_ok());
        assert!(check(&mut app, "width + 1 deg", "mm").is_err());
    }

    /// #4811: a mass parameter's row shows its value in its unit, a check reads the unit picked,
    /// every unit is offered once its first letter is typed (`µm` too), and the Unit cell lists
    /// every unit there is, then none, and reads the new ones back.
    #[test]
    fn mass_volume_and_area_units_in_the_dialog() {
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("parameters.add", json!({"name": "grease", "expression": "50 g"})).unwrap();
        app.run("parameters.add", json!({"name": "span", "expression": "90", "unit": "degrees"})).unwrap();
        let names: Vec<&str> = expr::UNITS.iter().map(|(name, _, _)| *name).chain(["", "text"]).collect();
        assert_eq!((UNITS.to_vec(), UNITS.first().copied()), (names, Some("mm")));
        let r = rows(&mut app);
        let row = |n: &str| r.iter().find(|x| x.name == n).cloned().unwrap_or_default();
        assert_eq!(show_value(&row("grease")), "0.0500 kg");
        assert_eq!(check(&mut app, "grease * 4", "kg"), Ok("0.2000 kg".to_string()));
        assert_eq!(check(&mut app, "500 ml", "cm³"), Ok("500.0000 cm³".to_string()));
        assert!(check(&mut app, "grease", "mm").is_err());
        publish(&mut app, &ctx);
        for (typed, unit) in [("k", "kg"), ("l", "lb"), ("c", "cm²"), ("µ", "µm")] {
            assert!(suggestions(&ctx, typed).iter().any(|s| s.text == unit && s.kind == "unit"), "{typed} offers {unit}");
        }
        for unit in ["kg", "lb", "cm³", "m²"] {
            assert_eq!(unit_from(unit), Ok(unit), "the Unit cell reads {unit}");
        }
        assert_eq!(row("span").unit, "degrees");
    }

    /// #2757: typing part of a parameter's name offers it, not only its start; names that start
    /// with the word come first, and functions still match from the start.
    #[test]
    fn a_parameter_is_offered_for_any_part_of_its_name() {
        let ctx = egui::Context::default();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        for (name, e) in [("cab_side_thickness", "18 mm"), ("sides", "4"), ("inner_diameter", "6 mm")] {
            let _ = app.run("parameters.add", json!({"name": name, "expression": e}));
        }
        // A favourite is listed first in the cache, but a match by word starts alone still
        // comes after every name that contains the word (#11978).
        let _ = app.run("parameters.favorite", json!({"name": "inner_diameter", "favorite": true}));
        publish(&mut app, &ctx);
        let texts = |w: &str| suggestions(&ctx, w).into_iter().map(|s| s.text).collect::<Vec<_>>();
        assert_eq!(texts("side"), ["sides", "cab_side_thickness"]);
        assert_eq!(texts("Thick"), ["cab_side_thickness"]);
        // #11978: the starts of the name's words, in order.
        assert_eq!(texts("cabthi"), ["cab_side_thickness"]);
        assert_eq!(texts("sidthi"), ["cab_side_thickness"]);
        assert!(texts("thicab").is_empty());
        assert_eq!(texts("id"), ["cab_side_thickness", "sides", "inner_diameter"]);
        assert!(!texts("in").iter().any(|t| t == "sin("), "functions match from the start");
    }

    /// A favourite is listed twice, under Favorites and in its own table; typing into either of its
    /// expression boxes enters each key once (#1767: `1.1` came out `1.11.1`).
    #[test]
    fn a_favourite_parameter_takes_each_key_once() {
        for (favourite, row) in [(false, 0), (true, 0), (true, 1)] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.run("parameters.add", json!({"name": "width", "expression": "10 mm"})).unwrap();
            if favourite {
                h.app.run("parameters.favorite", json!({"name": "width", "favorite": true})).unwrap();
            }
            h.step(&json!({"start": "parameters.change"})).unwrap();
            h.frames(4);
            // `param:width` is the box in the parameter's own table, `param:favorite:width` the one
            // under Favorites.
            h.step(&json!({"click": {"handle": if row == 0 { "param:width" } else { "param:favorite:width" }}})).unwrap();
            h.frames(2);
            h.step(&json!({"key": "A", "cmd": true})).unwrap();
            h.step(&json!({"text": "1.1"})).unwrap();
            h.frames(2);
            let editing = h.app.params.as_ref().unwrap().editing.get("width").cloned();
            assert_eq!(editing.as_deref(), Some("1.1"), "favourite {favourite}, row {row}");
        }
    }

    #[test]
    fn a_parameter_draft_stays_in_place_when_switching_fields() {
        for keyboard in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.step(&json!({"start": "parameters.change"})).unwrap();
            h.step(&json!({"click": {"handle": "dialog:table:user:add"}})).unwrap();
            h.step(&json!({"text": "width"})).unwrap();
            let before = crate::scenario::rect_of("dialog:table:user:new").unwrap();
            if keyboard {
                h.step(&json!({"key": "Tab"})).unwrap(); // Expression
            } else {
                h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
            }
            assert!(h.app.session.doc.param("width").is_none(), "moving between cells must not save the draft");
            assert_eq!(crate::scenario::rect_of("dialog:table:user:new"), Some(before), "the draft row must not move");
            h.step(&json!({"key": "A", "cmd": true})).unwrap();
            h.step(&json!({"text": "25 mm"})).unwrap();
            if keyboard {
                h.step(&json!({"key": "Tab"})).unwrap(); // Unit, now after Expression and Value
                h.step(&json!({"key": "Tab"})).unwrap(); // Comment
            } else {
                h.step(&json!({"click": {"handle": "param:new:comment"}})).unwrap();
            }
            h.step(&json!({"text": "Overall width"})).unwrap();
            assert!(h.app.session.doc.param("width").is_none());
            assert_eq!(crate::scenario::rect_of("dialog:table:user:new"), Some(before));
            h.step(&json!({"key": "Enter"})).unwrap();
            let parameter = h.app.session.doc.param("width").unwrap();
            assert_eq!(parameter.expr, "25 mm");
            assert_eq!(parameter.comment, "Overall width");
        }
    }

    #[test]
    fn a_parameter_draft_keeps_unit_picks_and_saves_when_leaving_the_row() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:table:user:add"}})).unwrap();
        h.step(&json!({"text": "width"})).unwrap();
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"text": "25 mm"})).unwrap();
        h.step(&json!({"click": {"handle": "param:new:unit"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Unit:cm"}})).unwrap();
        assert!(h.app.session.doc.param("width").is_none(), "choosing the draft's unit must not submit it");
        assert_eq!(h.app.params.as_ref().unwrap().new_unit, 1);
        h.step(&json!({"click": {"handle": "dialog:Filter"}})).unwrap();
        assert_eq!(h.app.session.doc.param("width").unwrap().unit, "cm");
        assert!(h.app.params.as_ref().unwrap().banner().is_none());
    }

    /// A refused add's message is about the draft it ran with: correcting the draft takes the
    /// message away before any second Enter, for a refused `parameters.add` and for the
    /// expression's own live check alike.
    #[test]
    fn correcting_a_refused_new_parameter_clears_its_error() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.add", json!({"name": "width", "expression": "10 mm"})).unwrap();
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:table:user:add"}})).unwrap();
        h.step(&json!({"text": "width"})).unwrap();
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"text": "25 mm"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        let error = h.app.params.as_ref().unwrap().banner().map(str::to_owned);
        assert!(error.as_deref().is_some_and(|e| e.contains("already a parameter")), "a taken name is refused: {error:?}");
        h.step(&json!({"click": {"handle": "param:new:name"}})).unwrap();
        h.step(&json!({"text": "2"})).unwrap();
        h.frames(2);
        let d = h.app.params.as_ref().unwrap();
        assert_eq!(d.new_name, "width2");
        assert_eq!(d.banner(), None, "the message was about the name `width`, which is gone");
        // The expression's live check: Enter on an unfinished expression shows it in the banner,
        // and correcting the expression takes it away.
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "10 +"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        let error = h.app.params.as_ref().unwrap().banner().map(str::to_owned);
        assert!(error.is_some(), "an unfinished expression is refused: {error:?}");
        assert!(h.app.session.doc.param("width2").is_none());
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": "10 + 5"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.params.as_ref().unwrap().banner(), None, "the message was about `10 +`, which is gone");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.param("width2").unwrap().expr, "10 + 5");
    }

    /// The dialog with `width` and `depth` saved, and the add row holding the draft `width`,
    /// `25 mm`, not yet submitted.
    fn taken_name_draft() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.add", json!({"name": "width", "expression": "10 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name": "depth", "expression": "5 mm"})).unwrap();
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:table:user:add"}})).unwrap();
        h.step(&json!({"text": "width"})).unwrap();
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"text": "25 mm"})).unwrap();
        h
    }

    fn banner(h: &crate::scenario::Harness) -> Option<String> {
        h.app.params.as_ref().unwrap().banner().map(str::to_owned)
    }

    #[test]
    fn discarding_a_refused_new_parameter_takes_its_message_away() {
        let mut h = taken_name_draft();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(banner(&h).is_some_and(|e| e.contains("already a parameter")), "{:?}", banner(&h));
        h.step(&json!({"click": {"handle": "param:new:delete"}})).unwrap();
        h.frames(2);
        let d = h.app.params.as_ref().unwrap();
        assert!(!d.adding && d.new_name.is_empty(), "the row is gone");
        assert_eq!(d.banner(), None, "and its message with it");
    }

    #[test]
    fn a_new_parameter_refused_on_leaving_the_row_clears_its_message_when_corrected() {
        let mut h = taken_name_draft();
        h.step(&json!({"click": {"handle": "dialog:Filter"}})).unwrap();
        h.frames(2);
        assert!(banner(&h).is_some_and(|e| e.contains("already a parameter")), "{:?}", banner(&h));
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "10 mm", "nothing was added or changed");
        h.step(&json!({"click": {"handle": "param:new:name"}})).unwrap();
        h.step(&json!({"text": "2"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.params.as_ref().unwrap().new_name, "width2");
        assert_eq!(banner(&h), None, "the message was about the name `width`, which is gone");
    }

    /// A design change can make a refused draft fine (here `width` is removed by a command, as an
    /// agent or Undo would): the message goes with the design it was about, and Enter adds.
    #[test]
    fn a_refused_new_parameter_is_checked_again_when_the_design_changes() {
        let mut h = taken_name_draft();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(banner(&h).is_some_and(|e| e.contains("already a parameter")), "{:?}", banner(&h));
        let doc = h.app.session.doc.clone();
        h.app.run("parameters.delete", json!({"name": "width"})).unwrap();
        h.frames(2);
        assert!(!Arc::ptr_eq(&doc, &h.app.session.doc), "a design change replaces the session's document");
        assert_eq!(banner(&h), None, "the message was about a design that had `width`");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "25 mm");
    }

    /// A row's own refusal with the very same words (renaming `depth` to the taken `width`) is
    /// not the add row's: it leaves the add row's refusal in place (the refused command puts the
    /// document it started from back), and correcting the draft leaves the rename's message
    /// where it is. A rename's message clears only through a rename that succeeds, which changes
    /// the design, so the refusal showing again once a row's message is cleared is in
    /// `the_banner_shows_the_newer_of_a_rows_message_and_the_add_rows_refusal`.
    #[test]
    fn correcting_a_refused_new_parameter_keeps_a_rows_message() {
        let mut h = taken_name_draft();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(banner(&h).is_some_and(|e| e.contains("already a parameter")), "{:?}", banner(&h));
        let (doc, revision) = (h.app.session.doc.clone(), h.app.session.revision);
        // Let the clicks that filled the draft age, or egui counts the double-click as a triple.
        h.frames(30);
        h.run(&json!([{"click": {"handle": "param:name:depth"}, "double": true}, {"key": "A", "cmd": true}, {"text": "width"}])).unwrap();
        assert!(h.app.params.as_ref().unwrap().renaming.is_some(), "double click enters rename mode");
        // The rename commits when its box loses the focus, here to the add row's name box.
        h.step(&json!({"click": {"handle": "param:new:name"}})).unwrap();
        h.frames(2);
        let d = h.app.params.as_ref().unwrap();
        assert!(d.error.as_deref().is_some_and(|e| e.contains("already a parameter `width`")), "the rename is refused: {:?}", d.error);
        assert_ne!(revision, h.app.session.revision, "a refused rename restores the design and refreshes it, which moves the revision");
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc), "but the restore puts the document the rename started from back");
        assert!(d.new_refused.is_some(), "so the draft's own refusal stays, under the rename's message");
        assert_eq!(h.app.session.doc.param("depth").unwrap().expr, "5 mm");
        h.step(&json!({"text": "2"})).unwrap();
        h.frames(2);
        let d = h.app.params.as_ref().unwrap();
        assert_eq!(d.new_name, "width2");
        assert!(d.new_refused.is_none(), "the draft's own refusal goes with the draft");
        assert!(d.error.as_deref().is_some_and(|e| e.contains("already a parameter `width`")), "the rename's message stays: {:?}", d.error);
        assert_eq!(d.banner(), d.error.as_deref(), "and is what the banner shows");
    }

    /// A click on a body in the viewport selects it (`select.set`), which moves the session's
    /// revision but not the design: the message is about the draft in the design, so it stays.
    #[test]
    fn selecting_a_body_keeps_a_refused_new_parameters_message() {
        let mut h = taken_name_draft();
        h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some("there is already a parameter `width`"));
        let (doc, revision) = (h.app.session.doc.clone(), h.app.session.revision);
        h.app.run("select.set", json!({"items": [solidcraft_engine::Sel::Body { name: "Body1".into() }]})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.selection.len(), 1, "the body is selected");
        assert_ne!(revision, h.app.session.revision, "a selection moves the revision");
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc), "and leaves the document alone");
        assert_eq!(banner(&h).as_deref(), Some("there is already a parameter `width`"), "so the message stays");
        assert!(h.app.params.as_ref().unwrap().new_refused.is_some());
    }

    /// A unit or comment edit changes the draft the message was about, so the message goes with
    /// it, although neither cures a taken name: Enter checks again and says so again.
    #[test]
    fn a_unit_or_comment_edit_on_a_refused_new_parameter_clears_its_message() {
        let mut h = taken_name_draft();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some("there is already a parameter `width`"));
        h.step(&json!({"click": {"handle": "param:new:unit"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Unit:cm"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.params.as_ref().unwrap().new_unit, 1);
        assert_eq!(banner(&h), None, "the message was about the draft in mm");
        // The unit menu holds the focus after a pick; Enter submits from one of the row's boxes.
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some("there is already a parameter `width`"), "Enter checks the draft again");
        h.step(&json!({"click": {"handle": "param:new:comment"}})).unwrap();
        h.step(&json!({"text": "Overall width"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.params.as_ref().unwrap().new_comment, "Overall width");
        assert_eq!(banner(&h), None, "the message was about the draft without a comment");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some("there is already a parameter `width`"), "Enter checks the draft again");
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "10 mm", "nothing was added");
    }

    /// With a row's message and the add row's refusal both alive, the banner shows the newer one.
    /// A row's expression that fails its check runs no design-changing command, so the document
    /// stays and the add row's refusal stays with it, under the row's message; once the row is
    /// corrected, the refusal shows again with no new Enter; and a refusal after a row's message
    /// takes the banner back, as any action's result did. The edits are held (#9279): in the live
    /// mode a row's message clears only through a `parameters.change` that succeeds, which changes
    /// the design and drops the refusal too. The draft's expression is unfinished, so leaving the
    /// add row attempts nothing (a valid draft is attempted on every click outside the row).
    #[test]
    fn the_banner_shows_the_newer_of_a_rows_message_and_the_add_rows_refusal() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.parameters_apply_live = false;
        h.app.run("parameters.add", json!({"name": "depth", "expression": "5 mm"})).unwrap();
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:table:user:add"}})).unwrap();
        h.step(&json!({"text": "height"})).unwrap();
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"text": "10 +"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        let refusal = banner(&h).expect("an unfinished expression is refused");
        assert!(h.app.session.doc.param("height").is_none(), "nothing was added");
        let doc = h.app.session.doc.clone();
        h.run(&json!([{"click": {"handle": "param:depth"}}, {"key": "A", "cmd": true}, {"text": "missing + 1 mm"}, {"key": "Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some("unknown parameter `missing`"), "the row's message is the newer one");
        assert!(h.app.params.as_ref().unwrap().new_refused.is_some(), "the add row's refusal is alive under it");
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc), "a failed check leaves the document alone");
        h.run(&json!([{"click": {"handle": "param:depth"}}, {"key": "A", "cmd": true}, {"text": "6 mm"}, {"key": "Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some(refusal.as_str()), "the correction clears the row's message and the refusal shows again, no Enter");
        assert_eq!(h.app.params.as_ref().unwrap().editing.get("depth").map(String::as_str), Some("6 mm"), "the held edit waits for Apply");
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc), "and the design is as it was");
        h.run(&json!([{"click": {"handle": "param:depth"}}, {"key": "A", "cmd": true}, {"text": "missing + 1 mm"}, {"key": "Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some("unknown parameter `missing`"), "the row's message is the newer one again");
        h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(banner(&h).as_deref(), Some(refusal.as_str()), "the add row's refusal is the newer one");
        assert_eq!(h.app.params.as_ref().unwrap().editing.get("depth").map(String::as_str), Some("missing + 1 mm"), "the row's edit is kept");
        assert_eq!(h.app.session.doc.param("depth").unwrap().expr, "5 mm");
    }

    /// #8563: sorting orders each section, and each model group, by itself.
    #[test]
    fn sorting_orders_each_section_and_model_group_on_its_own() {
        let row = |name: &str, source: &str, group: &str, favorite: bool| Row {
            name: name.into(),
            source: source.into(),
            group: group.into(),
            favorite,
            ..Default::default()
        };
        let listed = [
            row("d10", "user", "", true),
            row("width", "user", "", false),
            row("d2", "user", "", false),
            row("Alpha", "user", "", true),
            row("d1", "user", "", false),
            row("s_d3", "sketch", "Sketch1", false),
            row("s_d1", "sketch", "Sketch1", true),
            row("b_len", "feature", "Box1", false),
            row("b_h", "feature", "Box1", false),
            row("s_d2", "sketch", "Sketch1", false),
        ];
        let visible: Vec<&Row> = listed.iter().collect();
        fn names<'a>(rows: &[&'a Row]) -> Vec<&'a str> {
            rows.iter().map(|r| r.name.as_str()).collect()
        }
        let shown = |sort| {
            let s = sections(&visible, sort);
            (names(&s.favorites), names(&s.users), s.groups.iter().map(|(g, m)| (g.to_string(), names(m))).collect::<Vec<_>>())
        };
        let (fav, users, groups) = shown(ui_kit::Sort::Listed);
        assert_eq!(
            (fav, users),
            (vec!["d10", "Alpha", "s_d1"], vec!["width", "d2", "d1", "s_d3", "b_len", "b_h", "s_d2"]),
            "unsorted: the listed order"
        );
        assert!(groups.is_empty(), "renamed model dimensions belong in Named");
        let (fav, users, groups) = shown(ui_kit::Sort::Ascending);
        assert_eq!(fav, ["Alpha", "d10", "s_d1"]);
        assert_eq!(users, ["b_h", "b_len", "d1", "d2", "s_d2", "s_d3", "width"]);
        assert!(groups.is_empty());
        let (fav, users, groups) = shown(ui_kit::Sort::Descending);
        assert_eq!(fav, ["s_d1", "d10", "Alpha"]);
        assert_eq!(users, ["width", "s_d3", "s_d2", "d2", "d1", "b_len", "b_h"]);
        assert!(groups.is_empty());
    }

    /// #9706: the Filter box reads expressions too, so a parameter's name lists every parameter
    /// that uses it, and a value finds the parameter that holds it. #11978: the starts of a
    /// name's words, in order, find it.
    #[test]
    fn the_filter_lists_the_parameters_that_use_a_name() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1600, "height": 1400}));
        for (name, expression) in [("width", "40 mm"), ("height", "width * 2"), ("depth", "5 mm"), ("tooth_outward_flange_diameter", "8 mm")] {
            h.app.run("parameters.add", json!({"name": name, "expression": expression})).unwrap();
        }
        h.app.start("parameters.change");
        h.frames(6);
        let all = ["width", "height", "depth", "tooth_outward_flange_diameter"];
        let tooth = vec!["tooth_outward_flange_diameter"];
        for (filter, expected) in [
            ("width", vec!["width", "height"]),
            ("WIDTH *", vec!["height"]),
            ("5 mm", vec!["depth"]),
            ("toodia", tooth.clone()),
            ("OutDia", tooth.clone()),
            ("diatoo", vec![]),
            ("", all.to_vec()),
        ] {
            h.app.params.as_mut().unwrap().filter = filter.into();
            h.frames(4);
            let shown: Vec<&str> = all.into_iter().filter(|n| crate::scenario::rect_of(&format!("dialog:table:user:{n}")).is_some()).collect();
            assert_eq!(shown, expected, "filter {filter:?}");
        }
    }

    /// #8563: clicking the Name heading sorts every band in the window, the choice outlives the
    /// window, and moving a user parameter waits for the listed order.
    #[test]
    fn the_name_heading_sorts_the_window_and_is_remembered() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1600, "height": 1400}));
        h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
        for name in ["t10", "span", "t2", "Alpha", "t1"] {
            h.app.run("parameters.add", json!({"name": name, "expression": "10 mm"})).unwrap();
        }
        h.app.start("parameters.change");
        h.frames(6);
        let order = |table: &str, names: &[String]| {
            let mut rows: Vec<_> =
                names.iter().map(|n| (crate::scenario::rect_of(&format!("dialog:table:{table}:{n}")).expect(n).top(), n.clone())).collect();
            rows.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows.into_iter().map(|(_, n)| n).collect::<Vec<_>>()
        };
        let users: Vec<String> = ["t10", "span", "t2", "Alpha", "t1"].map(String::from).to_vec();
        let group = h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().find(|r| r.source != "user").unwrap().group.clone();
        let model: Vec<String> =
            h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().filter(|r| r.group == group).map(|r| r.name.clone()).collect();
        // Dimensions start collapsed; sorting Named also sorts those groups.
        let model_band = format!("dialog:table:parameters:group:model:{group}");
        assert_eq!(order("user", &users), users, "the listed order by default");
        assert!(model.len() > 1, "the box drives several parameters: {model:?}");
        let mut sorted_model = model.clone();
        ui_kit::Sort::Ascending.apply(&mut sorted_model, |s| s.as_str());

        h.step(&json!({"click": {"handle": "params:sort:user"}})).unwrap();
        h.frames(3);
        assert_eq!(order("user", &users), ["Alpha", "span", "t1", "t2", "t10"]);
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        // Filter to a model group so all of its cells are in view.
        h.app.params.as_mut().unwrap().filter = group.clone();
        h.frames(4);
        h.step(&json!({"click":{"handle":model_band}})).unwrap();
        h.frames(3);
        assert_eq!(order(&group, &model), sorted_model, "a model group sorts too");
        h.app.params.as_mut().unwrap().filter.clear();
        h.frames(4);
        h.step(&json!({"click":{"handle":model_band}})).unwrap();
        h.frames(3);
        // Closed and opened again: still sorted.
        h.app.params = None;
        h.frames(2);
        h.app.start("parameters.change");
        h.frames(4);
        h.step(&json!({"click":{"handle":model_band}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.params.as_ref().unwrap().sort, ui_kit::Sort::Ascending);
        h.step(&json!({"click":{"handle":"params:view:named"}})).unwrap();
        h.frames(3);
        h.step(&json!({"click": {"handle": "params:sort:user"}})).unwrap();
        h.frames(3);
        assert_eq!(order("user", &users), ["t10", "t2", "t1", "span", "Alpha"]);
        h.step(&json!({"click": {"handle": "params:sort:user"}})).unwrap();
        h.frames(3);
        assert_eq!(order("user", &users), users, "the third click goes back to the listed order");
    }

    /// A design with hundreds of chained parameters.
    fn big_design() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.add", json!({"name": "p0", "expression": "10 mm"})).unwrap();
        for i in 1..400 {
            h.app.run("parameters.add", json!({"name": format!("p{i}"), "expression": format!("p{} + 1 mm", i - 1)})).unwrap();
        }
        h
    }

    /// Mean milliseconds per frame over `n` frames.
    fn frame_ms(h: &mut crate::scenario::Harness, n: usize) -> f64 {
        let start = std::time::Instant::now();
        h.frames(n);
        start.elapsed().as_secs_f64() * 1000.0 / n as f64
    }

    /// Frame times with the window closed and open (`cargo test -p solidcraft-ui-egui params_frame_time -- --nocapture`).
    #[test]
    fn params_frame_time() {
        let mut h = big_design();
        h.frames(5);
        let closed = frame_ms(&mut h, 20);
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.frames(5);
        let open = frame_ms(&mut h, 20);
        assert!(h.app.params.is_some());
        eprintln!("params frame time: closed {closed:.2} ms, open {open:.2} ms");
    }

    /// #2733: many feature groups, rather than only many user parameters.
    fn many_feature_groups() -> crate::scenario::Harness {
        use solidcraft_engine::doc::{Document, FeatureKind, PlaneRef};
        let mut doc = Document::default();
        for i in 0..51 {
            doc.add_user_param(&format!("p{i}"), "10 mm", None, None).unwrap();
        }
        for i in 0..740 {
            doc.add_feature(
                FeatureKind::ConstructionPlane {
                    plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XY".into() }), distance: format!("{} mm", i + 1).into() },
                },
                Some(&format!("Plane{i}")),
            )
            .unwrap();
        }
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.session = solidcraft_engine::Session::new(doc);
        h.app.set_origin_shown(false);
        h
    }

    #[test]
    fn params_many_feature_groups_frame_time() {
        let mut h = many_feature_groups();
        h.frames(5);
        let closed = frame_ms(&mut h, 20);
        h.app.start("parameters.change");
        h.frames(5);
        let open = frame_ms(&mut h, 20);
        let listed = &h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1;
        assert_eq!(listed.iter().filter(|r| r.source == "feature").count(), 740);
        let model_rows = crate::scenario::rects("dialog:table:parameters:Plane");
        assert!(model_rows.is_empty(), "user parameters precede the offscreen model groups");
        eprintln!("740 feature groups: closed {closed:.2} ms, open {open:.2} ms");
    }

    #[test]
    fn model_groups_keep_scroll_geometry_and_editability() {
        let mut h = many_feature_groups();
        h.app.start("parameters.change");
        h.frames(3);
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.frames(5);
        let height = crate::scenario::count_of("dialog:body:content_height").unwrap();
        h.frames(20);
        assert_eq!(crate::scenario::count_of("dialog:body:content_height"), Some(height), "idle rows keep their scroll geometry");
        let last = h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().find(|r| r.group == "Plane739").unwrap().name.clone();
        let viewport = crate::scenario::rect_of("dialog:body:viewport").unwrap();
        h.call("ui.scroll", json!({"x":viewport.center().x,"y":viewport.center().y,"delta":-100_000}));
        h.frames(40);
        assert!((crate::scenario::count_of("dialog:body:content_height").unwrap() - height).abs() < 1.0);
        let band = crate::scenario::rect_of("dialog:table:parameters:group:model:Plane739").unwrap();
        h.step(&json!({"click":{"handle":"dialog:table:parameters:group:model:Plane739"}})).unwrap();
        h.frames(3);
        let row_y = band.bottom() + ui_kit::table_row_height() * 0.5;
        // Centre the last model row in the viewport before editing it.
        h.call("ui.scroll", json!({"x":viewport.center().x,"y":viewport.center().y,"delta":viewport.center().y - row_y}));
        h.frames(40);
        let at = crate::scenario::handle_at(&format!("param:{last}")).unwrap();
        assert!(crate::scenario::rect_of("dialog:body:viewport").unwrap().contains(at), "the last model parameter remains reachable");
        let model_rows = crate::scenario::rects("dialog:table:parameters:Plane");
        assert!(!model_rows.is_empty() && model_rows.len() < 80, "only visible model rows build their cells");
        h.step(&json!({"click":{"handle":format!("param:{last}")}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"900 mm"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.settle();
        let value = |h: &crate::scenario::Harness| {
            h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().find(|r| r.name == last).unwrap().value.unwrap()
        };
        assert_eq!(value(&h), 900.0);
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(3);
        assert_eq!(value(&h), 740.0);
        h.app.run("edit.redo", json!({})).unwrap();
        h.frames(3);
        assert_eq!(value(&h), 900.0);
        h.app.params.as_mut().unwrap().filter = "Plane739".into();
        h.frames(5);
        assert_eq!(crate::scenario::rects("dialog:table:parameters:group:model:").len(), 1, "filtering keeps only the matching group");
        assert!(crate::scenario::handle_at(&format!("param:{last}")).is_some());
    }

    /// #20648: the order the list is drawn in. Top level first, components in the design's order,
    /// folders nested like the components, empty components skipped, flat when grouping is off.
    #[test]
    fn the_model_list_is_ordered_by_component() {
        use super::Entry::{Folder, Group};
        // Components: 1 Frame, 2 Lid, 3 Hinge (in Frame), 4 Empty, 5 Pin (in Hinge).
        let components = [(1, 0, "Frame"), (2, 0, "Lid"), (3, 1, "Hinge"), (4, 0, "Empty"), (5, 3, "Pin")];
        // Groups in listing order: Lid, Pin, top level, Frame, Frame.
        let of_group = [2, 5, 0, 1, 1];
        let folder = |id: u64, name: &str, entries| Folder { id, name: name.into(), entries };
        assert_eq!(
            listing_order(&of_group, &components, true),
            vec![
                Group(2),
                folder(1, "Frame", vec![Group(3), Group(4), folder(3, "Hinge", vec![folder(5, "Pin", vec![Group(1)])])]),
                folder(2, "Lid", vec![Group(0)]),
            ]
        );
        assert_eq!(listing_order(&of_group, &components, false), (0..5).map(Group).collect::<Vec<_>>(), "flat when grouping is off");
        assert_eq!(listing_order(&[], &components, true), vec![], "nothing listed, nothing drawn");
        // A group whose component is unknown, or whose parents loop, is still drawn at the top.
        let broken = [(1, 2, "A"), (2, 1, "B")];
        assert_eq!(listing_order(&[1, 9, 0], &broken, true), vec![Group(0), Group(1), Group(2)]);
    }

    /// #20648: model parameters sit under the component their feature belongs to: top-level
    /// features first, then a folder per component; a design without components stays flat.
    #[test]
    fn model_parameters_group_under_their_component() {
        use solidcraft_engine::doc::{Document, FeatureKind, PlaneRef};
        let plane = |i: u32| FeatureKind::ConstructionPlane {
            plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XY".into() }), distance: format!("{} mm", i + 1).into() },
        };
        let mut doc = Document::default();
        let a = doc.add_component(Some("Frame"), 0).unwrap();
        let b = doc.add_component(Some("Lid"), 0).unwrap();
        for (name, component) in [("PlaneB", b), ("PlaneA", a), ("PlaneTop", 0)] {
            doc.add_feature(plane(1), Some(name)).unwrap();
            if let Some(f) = doc.features.iter_mut().find(|f| f.name == name) {
                f.component = component;
            }
        }
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.session = solidcraft_engine::Session::new(doc);
        // Grouping is opt-in (the Parameters window's own option).
        h.app.preferences.params_group_by_component = true;
        h.app.start("parameters.change");
        h.frames(5);
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:table:parameters:group:model:PlaneB"}})).unwrap();
        h.frames(3);
        let listed = h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.clone();
        let row_of = |group: &str| listed.iter().find(|r| r.group == group).unwrap().clone();
        assert_eq!((row_of("PlaneTop").component, row_of("PlaneA").component, row_of("PlaneB").component), (0, a, b));
        // A group off the dialog's visible part is not drawn, so scroll the body to the end: the
        // last folder's row must then be drawn, inside the dialog.
        let viewport = crate::scenario::rect_of("dialog:body:viewport").unwrap();
        h.call("ui.scroll", json!({"x":viewport.center().x,"y":viewport.center().y,"delta":-100_000}));
        h.frames(10);
        let at = crate::scenario::handle_at(&format!("param:{}", row_of("PlaneB").name)).unwrap();
        assert!(crate::scenario::rect_of("dialog:body:viewport").unwrap().contains(at), "a row inside a component folder is drawn");
        h.app.preferences.params_group_by_component = true;

        let mut flat = Document::default();
        flat.add_feature(plane(1), Some("PlaneTop")).unwrap();
        h.app.session = solidcraft_engine::Session::new(flat);
        h.app.params = None;
        h.app.start("parameters.change");
        h.frames(5);
        h.step(&json!({"click":{"handle":"dialog:table:parameters:group:model:PlaneTop"}})).unwrap();
        h.frames(3);
        let listed = &h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1;
        assert!(listed.iter().all(|r| r.component == 0), "a design without components lists every row at the top level");
        assert!(crate::scenario::handle_at(&format!("param:{}", listed.iter().find(|r| r.group == "PlaneTop").unwrap().name)).is_some());
    }

    /// #20648: in the one Parameters table, grouping on draws the top-level features' bands first,
    /// then a header band per component with that component's bands after it, in tree order; off,
    /// the bands are in listing order and there are no component headers.
    #[test]
    fn the_table_draws_component_bands_in_tree_order_when_grouped() {
        use solidcraft_engine::doc::{Document, FeatureKind, PlaneRef};
        let plane = FeatureKind::ConstructionPlane {
            plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XY".into() }), distance: "1 mm".into() },
        };
        let mut doc = Document::default();
        let a = doc.add_component(Some("Frame"), 0).unwrap();
        let b = doc.add_component(Some("Lid"), 0).unwrap();
        for (name, component) in [("PlaneB", b), ("PlaneA", a), ("PlaneTop", 0)] {
            doc.add_feature(plane.clone(), Some(name)).unwrap();
            if let Some(f) = doc.features.iter_mut().find(|f| f.name == name) {
                f.component = component;
            }
        }
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1600}));
        h.app.session = solidcraft_engine::Session::new(doc);
        let order = || {
            let mut bands = crate::scenario::rects("dialog:table:parameters:group:");
            bands.sort_by(|x, y| x.1.top().total_cmp(&y.1.top()));
            bands.into_iter().map(|(key, _)| key.trim_start_matches("dialog:table:parameters:group:").to_owned()).collect::<Vec<_>>()
        };
        h.app.start("parameters.change");
        h.frames(6);
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.frames(3);
        assert_eq!(order(), ["model:PlaneB", "model:PlaneA", "model:PlaneTop"], "off: the listing order, no folders");
        h.app.preferences.params_group_by_component = true;
        h.frames(6);
        let model = |name: &str| format!("model:{name}");
        assert_eq!(
            order(),
            [model("PlaneTop"), format!("component:{a}"), model("PlaneA"), format!("component:{b}"), model("PlaneB")],
            "on: top level first, then a header band per component with its own bands after it"
        );
    }

    /// A component's name is the user's text, so its folder in the Model Parameters list shows
    /// the name as written in every language: "Frame" is also a catalog word, and in German the
    /// folder read "BILDAUSSCHNITT".
    #[test]
    fn a_component_folder_shows_the_components_name_in_german() {
        use solidcraft_engine::doc::{Document, FeatureKind, PlaneRef};
        crate::i18n::install();
        assert_ne!(solidcraft_ui_text::Locale::for_language("de").label("Frame"), "Frame", "the German catalog translates the word Frame");
        let mut doc = Document::default();
        let a = doc.add_component(Some("Frame"), 0).unwrap();
        let plane = FeatureKind::ConstructionPlane {
            plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XY".into() }), distance: "1 mm".into() },
        };
        doc.add_feature(plane, Some("PlaneA")).unwrap();
        if let Some(f) = doc.features.iter_mut().find(|f| f.name == "PlaneA") {
            f.component = a;
        }
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.session = solidcraft_engine::Session::new(doc);
        h.app.preferences.params_group_by_component = true;
        h.app.preferences.language = "de".into();
        h.app.start("parameters.change");
        h.frames(5);
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.frames(3);
        let texts: Vec<String> =
            h.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.job.text.to_string()) } else { None }).collect();
        assert!(texts.iter().any(|t| t.eq_ignore_ascii_case("Frame")), "the folder is named Frame: {texts:?}");
        assert!(!texts.iter().any(|t| t.eq_ignore_ascii_case("Bildausschnitt")), "the name was not translated: {texts:?}");
    }

    /// #2919: the add row starts blank; a blank expression shows no error and adds nothing, and
    /// Enter after the name goes on to it; after an add the row is blank again; Tab into a box
    /// selects its text, so typing replaces it.
    #[test]
    fn the_add_row_starts_blank_and_tab_selects_a_box() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.start", json!({"command": "parameters.change"}));
        h.frames(3);
        h.step(&json!({"click": {"handle": "dialog:table:user:add"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.params.as_ref().unwrap().new_expr, "", "a new parameter's expression starts blank");
        h.step(&json!({"click": {"handle": "param:new:name"}})).unwrap();
        h.step(&json!({"text": "width"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.session.doc.params.iter().all(|p| p.name != "width"), "no expression: nothing added");
        assert_eq!(h.app.params.as_ref().unwrap().banner(), None, "and nothing wrong is said");
        h.step(&json!({"text": "25 mm"})).unwrap();
        assert_eq!(h.app.params.as_ref().unwrap().new_expr, "25 mm", "Enter went on to the expression");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.session.doc.params.iter().any(|p| p.name == "width" && p.expr == "25 mm"));
        let d = h.app.params.as_ref().unwrap();
        assert_eq!((d.new_name.as_str(), d.new_expr.as_str()), ("", ""), "blank again after an add");
        // A value already in the expression box: Tab into it from Name and type, and the
        // value is replaced.
        h.app.params.as_mut().unwrap().new_expr = "5 mm".into();
        h.step(&json!({"click": {"handle": "param:new:name"}})).unwrap();
        h.step(&json!({"key": "Tab"})).unwrap();
        h.step(&json!({"text": "2"})).unwrap();
        assert_eq!(h.app.params.as_ref().unwrap().new_expr, "2", "Tab selected the expression, typing replaced it");
    }

    /// Enter commits a cell and moves to the cell below in the same column, its text selected;
    /// Shift+Enter moves up; in the last row Enter only commits; a click keeps the caret.
    #[test]
    fn enter_moves_down_a_column_and_shift_enter_up() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        for (name, expression) in [("a", "1 mm"), ("b", "2 mm"), ("c", "3 mm")] {
            h.app.run("parameters.add", json!({"name":name,"expression":expression,"comment":format!("note {name}")})).unwrap();
        }
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(3);
        let expr = |h: &crate::scenario::Harness, name: &str| h.app.session.doc.param(name).map(|p| p.expr.clone()).unwrap_or_default();
        let comment = |h: &crate::scenario::Harness, name: &str| h.app.session.doc.param(name).map(|p| p.comment.clone()).unwrap_or_default();
        h.run(&json!([{"click":{"handle":"param:a"}}, {"key":"A","cmd":true}, {"text":"10 mm"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(expr(&h, "a"), "10 mm", "Enter commits the cell");
        h.run(&json!([{"text":"20 mm"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(expr(&h, "b"), "20 mm", "and the cell below took the keyboard with its text selected");
        h.run(&json!([{"text":"30 mm"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(expr(&h, "c"), "30 mm");
        h.run(&json!([{"click":{"handle":"param:c"}}, {"key":"A","cmd":true}, {"text":"31 mm"}, {"key":"Enter","shift":true}])).unwrap();
        h.frames(2);
        assert_eq!(expr(&h, "c"), "31 mm", "Shift+Enter commits too");
        h.run(&json!([{"text":"21 mm"}, {"key":"Enter","shift":true}])).unwrap();
        h.frames(2);
        assert_eq!(expr(&h, "b"), "21 mm", "Shift+Enter moved up the column");
        h.run(&json!([{"text":"11 mm"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(expr(&h, "a"), "11 mm");
        // The comment column moves the same way, and Escape still discards the edit it is in.
        h.run(&json!([{"click":{"handle":"param:comment:a"}}, {"key":"A","cmd":true}, {"text":"first"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        h.run(&json!([{"text":"second"}])).unwrap();
        h.frames(1);
        assert_eq!(comment(&h, "a"), "first");
        assert_eq!(h.app.params.as_ref().and_then(|d| d.comments.get("b").cloned()).as_deref(), Some("second"), "typing replaced b's comment");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert_eq!(comment(&h, "b"), "note b", "Escape discards the edit");
    }

    /// A block pasted from a spreadsheet (Excel's tab-separated text) over Expression, Value and
    /// Comment goes in as one gesture: one undo step takes all of it back. The read-only Value
    /// column keeps its text.
    #[test]
    fn a_pasted_block_is_one_undo_step() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        for (name, expression) in [("a", "1 mm"), ("b", "2 mm"), ("c", "3 mm")] {
            h.app.run("parameters.add", json!({"name": name, "expression": expression})).unwrap();
        }
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.app.params.as_mut().unwrap().redesign.ranges = true;
        h.frames(3);
        // The cursor on a's expression without editing it: click its Value, then Left.
        h.run(&json!([{"click": {"handle": "grid:user:a:value"}}])).unwrap();
        let state = grid_state(h.app.params.as_ref().unwrap(), "user");
        h.run(&json!([{"key": "ArrowLeft"}])).unwrap();
        assert!(h.app.params.as_ref().unwrap().editing.is_empty(), "the cursor moved; nothing is edited");
        let expression_column = redesign::column_order(h.app.params.as_ref().unwrap()).iter().position(|c| *c == EXPRESSION).unwrap();
        assert_eq!(ui_kit::Grid::selection(&h.ctx, state), Some((0, 0, expression_column, expression_column)), "the cursor is on a's expression");
        assert!(h.ctx.memory(|m| m.focused()).is_some_and(|f| f == state.with("focus")), "and the grid has the keyboard");
        let undo = h.app.session.undo.len();
        h.app.synthetic.push(egui::Event::Paste("20 mm\tignored\tmm\tfirst\r\n30 mm\tignored\tmm\tsecond\r\n".into()));
        h.frames(3);
        let param = |h: &crate::scenario::Harness, name: &str| {
            let p = h.app.session.doc.param(name).unwrap();
            (p.expr.clone(), p.comment.clone())
        };
        assert_eq!(param(&h, "a"), ("20 mm".into(), "first".into()));
        assert_eq!(param(&h, "b"), ("30 mm".into(), "second".into()));
        assert_eq!(param(&h, "c"), ("3 mm".into(), String::new()), "the block ends at b");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one paste, one undo step");
        assert!(h.app.params.as_ref().unwrap().error.is_none());
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(2);
        assert_eq!(param(&h, "a"), ("1 mm".into(), String::new()));
        assert_eq!(param(&h, "b"), ("2 mm".into(), String::new()), "one undo takes the whole paste back");
        // Ctrl+D fills a's expression down to b and c, again as one step.
        h.run(&json!([{"click": {"handle": "grid:user:a:value"}}, {"key": "ArrowLeft"}, {"key": "ArrowDown", "shift": true}, {"key": "ArrowDown", "shift": true}, {"key": "D", "cmd": true}])).unwrap();
        h.frames(2);
        assert_eq!((param(&h, "b").0, param(&h, "c").0), ("1 mm".into(), "1 mm".into()));
        assert_eq!(h.app.session.undo.len(), undo + 1, "the fill is one step too (the paste was undone)");
    }

    /// An open window lists and checks once per revision of the design, not once per frame
    /// (#2723), and shows a change at once.
    #[test]
    fn the_window_does_not_re_evaluate_every_frame() {
        let mut h = big_design();
        h.step(&json!({"start": "parameters.change"})).unwrap();
        // The redesign only checks a new expression while its inline draft is open, and a blank
        // one isn't checked (#2919).
        h.app.params.as_mut().unwrap().adding = true;
        h.app.params.as_mut().unwrap().new_expr = "10 mm".into();
        h.frames(30);
        let d = h.app.params.as_ref().unwrap();
        assert_eq!((d.lists, d.checks), (1, 1), "30 frames of an unchanged design");
        h.app.run("parameters.change", json!({"name": "p0", "expression": "20 mm"})).unwrap();
        h.frames(30);
        let d = h.app.params.as_ref().unwrap();
        assert_eq!((d.lists, d.checks), (2, 2), "one more for the change");
        let shown = d.listed.as_ref().unwrap().1.iter().find(|r| r.name == "p399").and_then(|r| r.value);
        assert_eq!(shown, Some(20.0 + 399.0));
        // A comment is a change too.
        h.app.run("parameters.comment", json!({"name": "p1", "comment": "hello"})).unwrap();
        h.frames(2);
        let d = h.app.params.as_ref().unwrap();
        assert!(d.listed.as_ref().unwrap().1.iter().any(|r| r.name == "p1" && r.comment == "hello"));
    }

    /// #8551 review: a long name only in User cannot shift its Expression column relative to Favorites.
    #[test]
    fn parameter_bands_share_the_same_columns_and_have_distinct_row_keys() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.app.run("parameters.add", json!({"name":"w","expression":"1 mm","favorite":true})).unwrap();
        h.app.run("parameters.add", json!({"name":"Driver_MountHole_to_Cover_Shared_Base_Layout_Angle","expression":"8 deg"})).unwrap();
        h.app.start("parameters.change");
        h.frames(6);
        let fav = crate::scenario::handle_at("param:favorite:w").unwrap();
        let named = crate::scenario::handle_at("param:Driver_MountHole_to_Cover_Shared_Base_Layout_Angle").unwrap();
        assert!((fav.x - named.x).abs() < 1.0, "all bands share their expression column");
        assert!(crate::scenario::rect_of("dialog:table:parameters:fav:w").is_some());
        assert!(crate::scenario::rect_of("dialog:table:user:w").is_none(), "key rows appear only once");
        assert_eq!(crate::scenario::rects("dialog:column:parameters:").iter().filter(|(key, _)| key.ends_with(":Name")).count(), 1);
    }

    #[test]
    fn one_table_has_ordered_counted_bands_that_collapse_independently() {
        let mut h = redesign_model();
        h.app.run("solid.box", json!({"length":40,"width":50,"height":60})).unwrap();
        h.app.run("parameters.add", json!({"name":"w","expression":"1 mm","favorite":true})).unwrap();
        h.app.run("parameters.add", json!({"name":"height","expression":"2 mm"})).unwrap();
        h.frames(4);
        assert_eq!(crate::scenario::count_of("dialog:table:parameters:group:fav:count"), Some(1.0));
        assert_eq!(crate::scenario::count_of("dialog:table:parameters:group:user:count"), Some(1.0));
        h.step(&json!({"click":{"handle":"dialog:table:parameters:group:fav"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at("param:favorite:w").is_none());
        assert!(crate::scenario::handle_at("param:height").is_some());
        h.step(&json!({"click":{"handle":"params:view:dimensions"}})).unwrap();
        h.frames(3);
        assert_eq!(crate::scenario::count_of("dialog:table:parameters:group:model:Box1:count"), Some(3.0));
        assert_eq!(crate::scenario::count_of("dialog:table:parameters:group:model:Box2:count"), Some(3.0));
        h.step(&json!({"click":{"handle":"dialog:table:parameters:group:model:Box1"}})).unwrap();
        h.frames(3);
        let row = h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().find(|r| r.group == "Box1").unwrap().name.clone();
        assert!(crate::scenario::handle_at(&format!("param:{row}")).is_some());
        h.app.params.as_mut().unwrap().filter = "w".into();
        h.frames(3);
        h.app.params.as_mut().unwrap().filter.clear();
        h.frames(3);
        assert!(h.app.params.as_ref().unwrap().bands["model:Box1"]);
        assert!(!h.app.params.as_ref().unwrap().bands["model:Box2"]);
        h.step(&json!({"click":{"handle":"params:view:named"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at("param:favorite:w").is_none(), "each band's state survives switching views");
        assert!(crate::scenario::handle_at("param:height").is_some());
    }

    #[test]
    fn model_groups_and_user_bands_fit_the_table_viewport() {
        for screen_width in [760, 900, 1100] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width": screen_width, "height": 1200}));
            h.app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
            h.app.run("solid.box", json!({"length": 40, "width": 50, "height": 60})).unwrap();
            h.app.run("parameters.add", json!({"name": "width", "expression": "10 mm"})).unwrap();
            h.app.run("parameters.favorite", json!({"name": "width", "favorite": true})).unwrap();
            h.app.start("parameters.change");
            h.frames(6);
            for filter in ["", "width", "Box1", "Box2"] {
                h.app.params.as_mut().unwrap().filter = filter.into();
                h.frames(6);
                let viewport = crate::scenario::rect_of("dialog:table:parameters:viewport").unwrap();
                let content = crate::scenario::count_of("dialog:table:parameters:content_width").unwrap();
                assert!(content <= f64::from(viewport.width().ceil()), "{filter} on {screen_width} screen: {content} outside {viewport:?}");
            }
        }
    }

    /// #5078: a long list scrolls in a bounded card; moving the card does not stretch it
    /// down to the bottom of the app. A short viewport still keeps the actions visible.
    #[test]
    fn a_long_parameter_list_keeps_its_height_when_moved() {
        for layout in ["floating", "docked"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.set", json!({"layout": layout}));
            h.call("ui.resize", json!({"width": 1600, "height": 1400}));
            for i in 0..60 {
                h.app.run("parameters.add", json!({"name": format!("p{i:02}"), "expression": "10 mm"})).unwrap();
            }
            h.step(&json!({"start": "parameters.change"})).unwrap();
            h.frames(6);
            let bounds = || crate::scenario::rect_of("dialog:header").unwrap().union(crate::scenario::rect_of("dialog:footer").unwrap());
            let before = bounds();
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            h.step(&json!({"drag": [{"handle": "dialog:title"}, [title.x - 90.0, title.y + 120.0]]})).unwrap();
            h.frames(6);
            let moved = bounds();
            assert!((moved.top() - before.top() - 120.0).abs() < 2.0, "{layout}: the header moved: {before:?} -> {moved:?}");
            assert!((moved.height() - before.height()).abs() < 2.0, "{layout}: moving must preserve height: {before:?} -> {moved:?}");
            assert!(moved.bottom() < 1300.0, "{layout}: a long list must leave room below the card: {moved:?}");

            let viewport = crate::scenario::rect_of("dialog:body:viewport").unwrap();
            h.call("ui.scroll", json!({"x": viewport.center().x, "y": viewport.center().y, "delta": -100_000}));
            h.frames(40);
            let last = crate::scenario::handle_at("param:p59").unwrap();
            assert!(crate::scenario::rect_of("dialog:body:viewport").unwrap().contains(last), "{layout}: the last parameter remains reachable");
            assert!((bounds().height() - moved.height()).abs() < 2.0, "{layout}: scrolling must preserve height");

            h.call("ui.resize", json!({"width": 1600, "height": 600}));
            h.frames(6);
            let small = bounds();
            assert!(small.bottom() <= 600.0, "{layout}: the actions must remain on screen: {small:?}");
            assert!(small.contains(crate::scenario::handle_at("dialog:ok").unwrap()));
            assert!(crate::scenario::rect_of("dialog:body:viewport").unwrap().height() > 0.0);
        }
    }
    #[test]
    fn expanded_comment_opens_explicitly_wraps_and_saves_on_click_away() {
        let evidence = |h: &mut crate::scenario::Harness, name: &str| {
            let Some(directory) = std::env::var_os("SOLIDCRAFT_COMMENT_EVIDENCE") else { return };
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            crate::icons::clear_texture_cache(&h.ctx);
            let output = h.ctx.run_ui(egui::RawInput { screen_rect: Some(h.ctx.content_rect()), focused: true, ..Default::default() }, |ui| {
                h.app.logic(ui.ctx());
                h.app.ui(ui);
            });
            let mut textures = std::collections::HashMap::from([(egui::TextureId::default(), h.ctx.fonts(|fonts| fonts.image()))]);
            crate::ui_kit::update_test_textures(&mut textures, &output.textures_delta);
            let dialog = crate::scenario::rect_of("dialog:header").unwrap().union(crate::scenario::rect_of("dialog:footer").unwrap());
            let dialog = crate::scenario::rect_of("param:comment:width:popup").map_or(dialog, |popup| dialog.union(popup));
            for (suffix, bounds) in [("", h.ctx.content_rect()), ("-dialog", dialog)] {
                let mut image = crate::ui_kit::test_raster(&h.ctx, output.shapes.clone(), h.ctx.pixels_per_point(), bounds, &textures);
                let background = solidcraft_theme::Tokens::get().dialog_body.to_array();
                for pixel in image.pixels_mut() {
                    let remaining = 1.0 - f32::from(pixel.0[3]) / 255.0;
                    for (dest, back) in pixel.0.iter_mut().zip(background) {
                        *dest = (f32::from(*dest) + f32::from(back) * remaining).round() as u8;
                    }
                }
                image.save(directory.join(format!("solidcraft-comment-{name}{suffix}.png"))).unwrap();
            }
        };
        let mut h = escape_dialog();
        evidence(&mut h, "before");
        h.step(&json!({"click":{"handle":"param:comment:width:expand"}, "shift":true})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none(), "Shift keeps the grid's selection gesture");
        h.step(&json!({"click":{"handle":"param:comment:width"}})).unwrap();
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none(), "ordinary editing stays compact");
        h.run(&json!([{"key":"A","cmd":true}, {"text":"Pending inline comment"}])).unwrap();
        h.step(&json!({"click":{"handle":"param:comment:width:expand"}})).unwrap();
        h.frames(3);
        let popup = crate::scenario::rect_of("param:comment:width:popup").unwrap();
        let tokens = solidcraft_theme::Tokens::get();
        let font = egui::FontId::monospace(tokens.font_label);
        let width = h.ctx.fonts_mut(|fonts| fonts.layout_no_wrap("M".repeat(30), font.clone(), tokens.form_ink)).size().x;
        let line_height = h.ctx.fonts_mut(|fonts| fonts.row_height(&font));
        assert!(popup.width() >= width, "expanded comment fits thirty characters: {popup:?}");
        assert!(popup.height() >= line_height * 5.0, "expanded comment shows five lines: {popup:?}");
        assert!(h.ctx.content_rect().contains_rect(popup), "popup stays within the window: {popup:?}");
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, "Saved comment", "opening does not prematurely save the inline draft");
        assert_eq!(h.app.params.as_ref().unwrap().comments.get("width").map(String::as_str), Some("Pending inline comment"));

        let long = "A longer comment with enough words to wrap across several visible rows while editing the parameter";
        h.run(&json!([{"key":"A","cmd":true}, {"text":long}])).unwrap();
        h.frames(2);
        evidence(&mut h, "after");
        assert!(
            h.shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == long && text.galley.rows.len() > 1)),
            "the expanded text wraps"
        );
        h.step(&json!({"click":{"handle":"dialog:Filter"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none());
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, long);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, "Saved comment", "the entire edit is one undoable command");
    }

    #[test]
    fn expanded_comment_paste_stays_single_line_and_enter_commits() {
        let mut h = escape_dialog();
        h.step(&json!({"click":{"handle":"param:comment:width:expand"}})).unwrap();
        h.frames(3);
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.app.synthetic.push(egui::Event::Paste("Mounting\r\n尺寸\n🙂\rClearance".into()));
        h.frame();
        let draft = h.app.params.as_ref().unwrap().comments.get("width").cloned().unwrap();
        assert_eq!(draft, "Mounting 尺寸 🙂 Clearance");
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none());
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, draft);
        assert!(h.app.params.is_some(), "Enter finishes the field without closing Parameters");
    }

    #[test]
    fn expanded_comment_escape_discards_draft_and_closes_parameters() {
        let mut h = escape_dialog();
        let before = escape_state(&h);
        h.step(&json!({"click":{"handle":"param:comment:width:expand"}})).unwrap();
        h.frames(3);
        h.run(&json!([{"key":"A","cmd":true}, {"text":"Expanded pending comment"}])).unwrap();
        assert_eq!(h.app.params.as_ref().unwrap().comments.get("width").map(String::as_str), Some("Expanded pending comment"));
        escape_closed(&mut h, &before, "10 mm");
        h.frames(3);
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none(), "reopening Parameters starts with the compact editor");
    }

    #[test]
    fn expanded_comment_scrolls_long_text_and_reopens_after_tab() {
        let mut h = escape_dialog();
        let long = "A mounting clearance note with dimensions and installation details. ".repeat(40);
        h.step(&json!({"click":{"handle":"param:comment:width:expand"}})).unwrap();
        h.frames(3);
        h.run(&json!([{"key":"A","cmd":true}, {"text":long}])).unwrap();
        h.frames(2);
        let (painted, text) = h
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == long && text.galley.rows.len() > 1 => Some((shape, text)),
                _ => None,
            })
            .unwrap();
        assert!(text.galley.size().y > painted.clip_rect.height(), "very long text scrolls within its editor");
        assert!(painted.clip_rect.height() < h.ctx.content_rect().height() / 2.0, "the editor leaves the dialog reachable");
        h.step(&json!({"key":"Tab"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none(), "Tab leaves the expanded editor");
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, long);
        h.step(&json!({"click":{"handle":"param:comment:width:expand"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_some());
        h.run(&json!([{"key":"A","cmd":true}, {"text":"Reopened"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, "Reopened");
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none());
    }

    #[test]
    fn expanded_comment_context_menu_keeps_draft_until_edit_finishes() {
        let mut h = escape_dialog();
        h.step(&json!({"click":{"handle":"param:comment:width:expand"}})).unwrap();
        h.frames(3);
        h.run(&json!([{"key":"A","cmd":true}, {"text":"Pending expanded comment"}])).unwrap();
        h.step(&json!({"click":{"handle":"param:comment:width:popup"},"button":"right"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("menu:text_select_all").is_some());
        h.step(&json!({"click":{"handle":"menu:text_select_all"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_some(), "the context menu returns to the expanded editor");
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, "Saved comment", "using the context menu does not commit the draft");
        h.run(&json!([{"text":"Replacement from menu selection"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.param("width").unwrap().comment, "Replacement from menu selection");
        assert!(crate::scenario::rect_of("param:comment:width:popup").is_none());
    }

    #[test]
    fn expanded_comment_draft_stays_in_row_until_click_away() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
        h.step(&json!({"text":"width"})).unwrap();
        h.step(&json!({"click":{"handle":"param:new:expression"}})).unwrap();
        h.step(&json!({"text":"25 mm"})).unwrap();
        h.step(&json!({"click":{"handle":"param:new:comment:expand"}})).unwrap();
        h.frames(3);
        let popup = crate::scenario::rect_of("param:new:comment:popup").unwrap();
        let row = crate::scenario::rect_of("dialog:table:user:new").unwrap();
        let inside = egui::pos2(popup.center().x, popup.bottom() - 8.0);
        assert!(!row.contains(inside), "the click is below the compact draft row");
        assert!(h.app.session.doc.param("width").is_none(), "opening the expanded draft field stays in its row");
        h.step(&json!({"click":[inside.x, inside.y]})).unwrap();
        h.frames(2);
        assert!(h.app.session.doc.param("width").is_none(), "clicking inside the expanded editor keeps the parameter as a draft");
        h.step(&json!({"text":"Overall width"})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Filter"}})).unwrap();
        h.frames(2);
        let parameter = h.app.session.doc.param("width").unwrap();
        assert_eq!(parameter.expr, "25 mm");
        assert_eq!(parameter.comment, "Overall width");
    }

    /// #9358: a formula longer than its column opens wide while it is edited, over the cells to
    /// its right, so all of it shows; Enter applies it and the field goes back into its column.
    /// A formula that fits stays in its cell.
    #[test]
    fn a_long_expression_opens_wide_while_edited_and_enter_applies_it() {
        let long = "width + width + width + width + width + width + width + width + width + width";
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.add", json!({"name":"width","expression":"1 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name":"length","expression":long})).unwrap();
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(3);
        let cell = crate::scenario::rect_of("param:length").unwrap();
        assert!(crate::scenario::rect_of("param:width").unwrap().width() <= cell.width() + 0.5);

        h.step(&json!({"click":{"handle":"param:length"}})).unwrap();
        h.frames(2);
        let wide = crate::scenario::rect_of("param:length").unwrap();
        assert!(wide.width() > cell.width() * 1.5, "the edited formula opens wide: {wide:?} vs {cell:?}");
        assert!((wide.left() - cell.left()).abs() < 1.0, "it opens from its own cell: {wide:?} vs {cell:?}");

        // The open field wraps, so the end of the text is Command+Down. Not Ctrl+End:
        // egui ignores that on macOS, and the typed tail would land where the click put the cursor.
        h.run(&json!([{"key":"ArrowDown","cmd":true,"ctrl":false}, {"text":" * 2"}, {"key":"Enter"}])).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.param("length").unwrap().expr, format!("{long} * 2"));
        let after = crate::scenario::rect_of("param:length").unwrap();
        assert!(after.width() <= cell.width() + 0.5, "applied, the field is back in its column: {after:?}");

        h.step(&json!({"click":{"handle":"param:width"}})).unwrap();
        h.frames(2);
        let short = crate::scenario::rect_of("param:width").unwrap();
        assert!(short.width() <= cell.width() + 0.5, "a formula that fits stays in its cell: {short:?} vs {cell:?}");
    }

    /// #14426 + #21892: a named driven sketch dimension appears in Named (and moves to Dimensions
    /// if demoted to `dN`), keeps its Expression and Unit cells read-only under every column
    /// layout, reserves its name against renames/promotions, and renders read-only in Key view.
    #[test]
    fn driven_dimension_parameters_stay_read_only_across_named_and_dimensions_views() {
        use crate::ui_kit::GridSource as _;

        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1600, "height": 1200}));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 40]})).unwrap();
        h.app.run("sketch.constraint.coincident", json!({"a": "p1", "b": "origin"})).unwrap();
        h.app.run("sketch.dimension", json!({"entities": ["l1"], "value": 30})).unwrap();
        h.app.run("sketch.dimension", json!({"entities": ["l2"], "value": 40})).unwrap();
        h.app.run("sketch.dimension", json!({"entities": ["p1", "p3"], "driven": true, "name": "diag"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.start("parameters.change");
        h.frames(4);

        let all = rows(&mut h.app);
        let diag = all.iter().find(|r| r.name == "diag").expect("driven parameter listed").clone();
        assert_eq!(diag.source, "driven");
        assert!(diag.read_only);
        assert_eq!(diag.group, "Sketch1");
        assert!(diag.measures.contains("Distance"), "measures comes from the sketch constraint: {}", diag.measures);
        assert!(redesign::validate_name(&h.app, "d1", "diag", true).is_err(), "driven name is taken");

        let visible: Vec<&Row> = all.iter().collect();
        let shown = sections(&visible, ui_kit::Sort::Listed);
        assert!(shown.users.iter().any(|r| r.name == "diag"), "custom-named driven dimension belongs in Named");

        // Column remapping in Named (wide & narrow) and Dimensions keeps Expression and Unit read-only.
        for (view, narrow) in [(redesign::View::Named, false), (redesign::View::Named, true), (redesign::View::Dimensions, false)] {
            let mut d = ParamsDialog::new();
            d.redesign.view = view;
            d.redesign.narrow = narrow;
            let cols = redesign::column_order(&d);
            let columns = redesign::columns(120.0, 60.0, cols);
            let row_slice = [&diag];
            let grid = ParamGrid {
                app: &mut h.app,
                d: &mut d,
                rows: &row_slice,
                salt: "user",
                columns: &columns,
                right: 800.0,
                live: Default::default(),
                cols,
            };
            let expr_col = cols.iter().position(|c| *c == EXPRESSION).unwrap();
            assert_eq!(grid.kind(0, expr_col), Some(ui_kit::CellKind::ReadOnly));
            if let Some(unit_col) = cols.iter().position(|c| *c == UNIT) {
                assert_eq!(grid.kind(0, unit_col), Some(ui_kit::CellKind::ReadOnly));
            }
        }

        // Expression edits are ignored; comment edits still apply cleanly.
        let mut d = h.app.params.take().unwrap();
        apply_grid(
            &mut h.app,
            &h.ctx,
            &mut d,
            &[&diag],
            "user",
            ui_kit::GridOutput {
                edits: vec![
                    ui_kit::Edit::Set { row: ui_kit::RowRef::Existing("diag".into()), col: EXPRESSION, text: "99 mm".into() },
                    ui_kit::Edit::Set { row: ui_kit::RowRef::Existing("diag".into()), col: COMMENT, text: "diagonal".into() },
                ],
                ..Default::default()
            },
        );
        assert!(d.error.is_none(), "{:?}", d.error);
        assert_eq!(h.app.session.doc.param_comments.get("diag").map(String::as_str), Some("diagonal"));
        h.app.params = Some(d);

        // Starring a driven dimension puts it in Key parameters and compact mode keeps it read-only.
        h.app.run("parameters.favorite", json!({"name": "diag", "favorite": true})).unwrap();
        h.frames(4);
        h.step(&json!({"click": {"handle": "params:key:toggle"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("params:key:diag").is_some(), "compact Key view shows the driven parameter");
        assert!(crate::scenario::rect_of("params:key:slider:diag").is_none(), "read-only driven parameter has no slider");
        assert!(!h.app.params.as_ref().unwrap().redesign.compact.as_ref().unwrap().contains_key("diag"));

        // Demoting a driven dimension moves it to Dimensions under its owning sketch group.
        let mut d = h.app.params.take().unwrap();
        let all = rows(&mut h.app);
        let diag_row = all.iter().find(|r| r.name == "diag").unwrap();
        redesign::demote(&mut h.app, &mut d, diag_row, &all).unwrap();
        assert!(d.redesign.view == redesign::View::Dimensions);
        let after = rows(&mut h.app);
        let demoted = after.iter().find(|r| r.source == "driven").expect("demoted driven parameter");
        assert!(demoted.read_only && demoted.group == "Sketch1");
        let after_visible: Vec<&Row> = after.iter().collect();
        let after_sections = sections(&after_visible, ui_kit::Sort::Listed);
        assert!(after_sections.groups.iter().any(|(g, m)| *g == "Sketch1" && m.iter().any(|r| r.name == demoted.name)));
    }
}
