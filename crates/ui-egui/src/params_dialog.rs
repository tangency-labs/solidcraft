//! The Change Parameters dialog and parameter-name autocomplete for expression boxes.
//!
//! A window over the engine's `parameters.*` commands: favourites first, then user parameters
//! (name, unit, expression, value, comment; add, rename, delete) and model parameters grouped by
//! the sketch or feature they drive (editable). The Name heading sorts each section, and each
//! model group, by name in natural order (#8563). Expressions validate as you type
//! (`expr.evaluate`) with a live model preview; Enter or leaving the field applies them.
//! With "Apply each change as you leave its field" off (a remembered preference, #9279), they
//! wait instead, without a preview, until Apply or OK sets them in one `parameters.change`.
//! Escape closes the window and discards pending edits and their preview.
//! Import and export go through `parameters.import` / `parameters.export`.
//!
//! [`complete`] adds suggestions (parameters, functions, units) under any expression box: the
//! names come from `parameters.complete`, cached per document revision by [`publish`].

use crate::ui_kit::BackingTooltipExt as _;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit::{self, Column};

/// What Import reads. The picker must list them: on macOS, files of a type no filter names
/// can't be picked (#3216).
pub(crate) const IMPORT_TYPES: &[&str] = &["csv", "json"];

const UNITS: [&str; 10] = ["mm", "cm", "m", "in", "ft", "deg", "rad", "", "um", "text"];

/// Dialog state (what is being typed; the parameters themselves live in the document).
#[derive(Default)]
pub struct ParamsDialog {
    pub filter: String,
    /// The order by name inside each section (#8563); remembered across openings like the
    /// sections' open state.
    pub(crate) sort: ui_kit::Sort,
    reset_scroll: bool,
    /// Expressions being edited, by parameter name.
    pub editing: std::collections::BTreeMap<String, String>,
    /// Comments being edited, by parameter name.
    pub comments: std::collections::BTreeMap<String, String>,
    /// A parameter being renamed: (old name, new name).
    pub renaming: Option<(String, String)>,
    /// The table the rename was started in. A favourite is listed twice (Favourites and User),
    /// and two editors on one name took each other's focus, so the rename closed at once (#4952).
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
    /// The rows as the engine listed them at one revision of the design (#2723): listing
    /// evaluates every parameter, so it happens when the design changes, not every frame.
    listed: Option<(u64, Arc<Vec<Row>>)>,
    /// The add row's live check, by (revision, unit, expression).
    new_check: Option<((u64, String, String), Result<String, String>)>,
    /// How many times the rows were listed and the add row checked (for the tests).
    pub lists: u32,
    pub checks: u32,
    /// Measured model groups keep their scroll space without rebuilding offscreen tables.
    group_layout: Option<GroupLayout>,
    group_sizes: std::collections::HashMap<String, egui::Vec2>,
    #[cfg(test)]
    groups_drawn: usize,
}

#[derive(PartialEq)]
struct GroupLayout {
    revision: u64,
    filter: String,
    width: f32,
    pixels_per_point: f32,
    style: Arc<egui::Style>,
    font: String,
    system_fonts: bool,
    language: &'static str,
}

impl ParamsDialog {
    /// The add row starts blank, its expression too (#2919): a value put there for the user was
    /// almost never the one wanted, and had to be erased first.
    pub fn new() -> Self {
        ParamsDialog { reset_scroll: true, ..Default::default() }
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
    favorite: bool,
    /// A text parameter's text.
    text: Option<String>,
}

impl Row {
    /// Whether the Filter box's text (lower case) is in the row's name, expression, text,
    /// comment or group. The expression lets a parameter's name find every one that uses it
    /// (#9706). The name also matches by the starts of its words, in order: `toodia` finds
    /// `tooth_outward_flange_diameter` (#11978).
    fn matches(&self, filter: &str) -> bool {
        filter.is_empty()
            || [&self.name, &self.expression, &self.comment, &self.group]
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
    for f in &app.session.doc.features {
        if let solidcraft_engine::doc::FeatureKind::Sketch { sketch, .. } = &f.kind {
            for c in &sketch.constraints {
                if let Some(p) = &c.param {
                    owner.insert(p.clone(), f.name.clone());
                }
            }
        }
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
                "feature" => s(r, "feature"),
                "sketch" => owner.get(&name).cloned().unwrap_or_else(|| "Sketch".into()),
                _ => String::new(),
            };
            Row {
                expression: s(r, "expression"),
                unit: s(r, "unit"),
                // The dialog shows the parameter's own unit, so it reads `display_value`
                // (`value` is mm / degrees in every reply, #10747).
                value: r.get("display_value").and_then(Value::as_f64).or_else(|| r.get("value").and_then(Value::as_f64)),
                error: r.get("error").and_then(Value::as_str).map(str::to_string),
                comment: s(r, "comment"),
                favorite: r.get("favorite").and_then(Value::as_bool).unwrap_or(false),
                text: r.get("text").and_then(Value::as_str).map(str::to_string),
                source,
                group,
                name,
            }
        })
        .collect()
}

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
    if app.home.open || app.params.is_none() || !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        return false;
    }
    ctx.input_mut(|i| {
        i.raw.events.retain(|event| !matches!(event, egui::Event::Key { key: egui::Key::Escape, modifiers, .. } if modifiers.matches_logically(egui::Modifiers::NONE)));
    });
    // Dropping the dialog discards every draft. The regular preview update clears its scene.
    app.params = None;
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
    .body_rows(16.0)
    .reset_scroll(d.reset_scroll);
    // A width or body height the user dragged the card to replaces that default; each axis stays
    // automatic until the user drags it (review of #13422).
    let chosen_width: Option<f32> = ctx.data_mut(|m| m.get_persisted(width_id())).filter(|w: &f32| w.is_finite());
    let chosen_height: Option<f32> = ctx.data_mut(|m| m.get_persisted(height_id())).filter(|h: &f32| h.is_finite());
    let width = chosen_width.map_or(solidcraft_theme::Tokens::get().form_card_width * 3.0, |w| w.max(crate::frame::DIALOG_MIN_W));
    let panel = panel.body_height(chosen_height);
    let panel = panel.width(width.min((docked.x - drag_bounds.left()).max(crate::frame::DIALOG_MIN_W)));
    let all = if let Some((_, rows)) = d.listed.as_ref().filter(|(rev, _)| *rev == app.session.revision) {
        rows.clone()
    } else {
        let rows = Arc::new(rows(app));
        d.listed = Some((app.session.revision, rows.clone()));
        d.lists += 1;
        rows
    };
    let mut outcome = (false, ui_kit::Action::None);
    let live = app.preferences.parameters_apply_live;
    let held_status = if live { None } else { held_status(held(&d, &all).len()) };
    let mut resize = Resize::None;
    let shown = panel.window(ctx).show(ctx, |ui| {
        outcome = panel.feature(
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
            true,
            |ui| {
                ui_kit::text(ui, hint(live), ui_kit::Text::Hint("manage"));
                apply_live_checkbox(app, ui, &mut d, &all);
                ui_kit::row(ui, "Filter", |ui| {
                    let id = ui.make_persistent_id("parameter_filter");
                    ui_kit::search_field(ui, id, &mut d.filter, "Filter");
                });
                ui.horizontal_wrapped(|ui| {
                    if ui_kit::icon_label_button(ui, "Import…", "import_tray", true, false)
                        .backing_on_hover_text(solidcraft_ui_text::tr_label("CSV or JSON: updates existing parameters, adds new ones"))
                        .clicked()
                    {
                        app.pick_open_command("parameters.import", json!({}), IMPORT_TYPES);
                    }
                    for (label, ext) in [("Export CSV…", "csv"), ("Export JSON…", "json")] {
                        if ui_kit::icon_label_button(ui, label, "export_tray", true, false).clicked() {
                            app.pick_save_command(&format!("parameters.{ext}"), &[ext], "parameters.export", json!({"format": ext}));
                        }
                    }
                });
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
                let Sections { favorites, users, groups } = sections(&visible, d.sort);
                if !favorites.is_empty() {
                    ui_kit::section_count(ui, "favorites", "Favorites", true, Some(favorites.len()), |ui| {
                        table(app, ui, &mut d, &favorites, "fav", name_floor)
                    });
                }
                ui_kit::section_count(ui, "users", "User Parameters", true, Some(users.len()), |ui| {
                    table(app, ui, &mut d, &users, "user", name_floor);
                });
                ui_kit::section(ui, "models", "Model Parameters", true, |ui| {
                    ui.scope(|ui| {
                        ui.spacing_mut().indent = solidcraft_theme::Tokens::get().space_14;
                        ui.indent("model_groups", |ui| {
                            let layout = GroupLayout {
                                revision: app.session.revision,
                                filter: filter.clone(),
                                width: ui.available_width(),
                                pixels_per_point: ui.ctx().pixels_per_point(),
                                style: ui.style().clone(),
                                font: app.applied_ui_font.clone(),
                                system_fonts: app.applied_system_fonts,
                                language: solidcraft_ui_text::language(),
                            };
                            if d.group_layout.as_ref() != Some(&layout) {
                                d.group_sizes.clear();
                                d.group_layout = Some(layout);
                            }
                            #[cfg(test)]
                            {
                                d.groups_drawn = 0;
                            }
                            for (group, members) in groups {
                                model_group(app, ui, &mut d, group, &members, name_floor);
                            }
                        });
                    });
                });
                ui_kit::ComputeStatus { text: held_status, busy: false, applying: false }
            },
        );
        resize = resize_edges(ui, side, panel.last_body_scrollbar(ui.ctx()));
        d.reset_scroll = false;
    });
    match resize {
        Resize::None => {}
        Resize::Reset(axes) => ctx.data_mut(|m| {
            if axes.width {
                m.remove::<f32>(width_id());
            }
            if axes.height {
                m.remove::<f32>(height_id());
            }
        }),
        Resize::By(axes, delta) => {
            let t = solidcraft_theme::Tokens::get();
            if axes.width
                && let Some(card) = shown.as_ref().map(|r| r.response.rect)
            {
                let w = (card.width() + delta.x).clamp(crate::frame::DIALOG_MIN_W, viewport.width().max(crate::frame::DIALOG_MIN_W));
                if w.is_finite() {
                    ctx.data_mut(|m| m.insert_persisted(width_id(), w));
                }
            }
            if axes.height
                && let Some(body) = panel.last_body_height(ctx)
            {
                let h = (body + delta.y).clamp(t.control_height * 2.0, viewport.height().max(t.control_height * 2.0));
                if h.is_finite() {
                    ctx.data_mut(|m| m.insert_persisted(height_id(), h));
                }
            }
        }
    }
    // Held edits (#9279) are set by Apply and OK; Cancel, the close button and Escape drop them.
    let (closed, action) = outcome;
    let applied = live || !matches!(action, ui_kit::Action::Ok | ui_kit::Action::Apply) || apply_held(app, &mut d, &all);
    let close = closed || action == ui_kit::Action::Cancel || (action == ui_kit::Action::Ok && applied);
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
    if !close {
        app.params = Some(d);
    }
}

/// Where the chosen order is kept: egui's persisted memory, like a section's open state.
fn sort_id() -> egui::Id {
    egui::Id::unique("sc_params_sort")
}

/// Where the outer width the user dragged the card to is kept; absent while the width is automatic.
fn width_id() -> egui::Id {
    egui::Id::unique("sc_params_width")
}

/// Where the body height the user dragged the card to is kept; absent while the height is automatic.
fn height_id() -> egui::Id {
    egui::Id::unique("sc_params_height")
}

/// Where the column widths the user dragged are kept, shared by every section's table.
fn columns_id() -> egui::Id {
    egui::Id::unique("sc_params_columns")
}

/// Which of the card's two sizes a resize edge changes: the side edge its width, the bottom edge
/// its body height, the corner both. The other axis keeps its automatic size.
#[derive(Clone, Copy)]
struct Axes {
    width: bool,
    height: bool,
}

/// What the card's resize edges asked for this frame.
enum Resize {
    None,
    /// Grow the card by this much on these axes: x widens it, y lengthens its body.
    By(Axes, egui::Vec2),
    /// A double-click on an edge returns these axes to their automatic size.
    Reset(Axes),
}

/// Drag areas on the card's free side edge (the one away from its dock side), its bottom edge
/// and the corner between them. The header stays the title drag area. When the free edge is the
/// right one and the body scrolls, `scrollbar` is the body viewport whose right edge carries the
/// scrollbar: the side grip skips that span so scrollbar drags still scroll (#13422 review).
fn resize_edges(ui: &mut egui::Ui, side: crate::frame::Side, scrollbar: Option<egui::Rect>) -> Resize {
    let t = solidcraft_theme::Tokens::get();
    let card = ui.min_rect();
    let grip = t.space_6;
    if !card.is_finite() || card.width() < grip * 4.0 || card.height() < t.form_header_height + grip * 2.0 {
        return Resize::None;
    }
    // A card docked right is held by its right edge, so it grows from its left edge.
    let free_left = side == crate::frame::Side::Right;
    let x_edge = if free_left { card.left()..=card.left() + grip } else { card.right() - grip..=card.right() };
    let corner_x = if free_left { card.left()..=card.left() + grip * 2.0 } else { card.right() - grip * 2.0..=card.right() };
    // The scrollbar sits on the body's right edge, so only a right-hand grip can cover it.
    let side_top = match scrollbar {
        Some(body) if !free_left && body.is_finite() => body.bottom().max(card.top() + t.form_header_height),
        _ => card.top() + t.form_header_height,
    };
    let mut edges = vec![(
        "bottom",
        egui::Rect::from_x_y_ranges(card.left() + grip * 2.0..=card.right() - grip * 2.0, card.bottom() - grip..=card.bottom()),
        egui::CursorIcon::ResizeVertical,
        Axes { width: false, height: true },
    )];
    if side_top < card.bottom() - grip * 2.0 {
        edges.push((
            "side",
            egui::Rect::from_x_y_ranges(x_edge, side_top..=card.bottom() - grip * 2.0),
            egui::CursorIcon::ResizeHorizontal,
            Axes { width: true, height: false },
        ));
    }
    edges.push((
        "corner",
        egui::Rect::from_x_y_ranges(corner_x, card.bottom() - grip * 2.0..=card.bottom()),
        if free_left { egui::CursorIcon::ResizeNeSw } else { egui::CursorIcon::ResizeNwSe },
        Axes { width: true, height: true },
    ));
    let mut out = Resize::None;
    for (name, rect, cursor, axes) in edges {
        let response = ui.interact(rect, ui.make_persistent_id(("params_resize", name)), egui::Sense::click_and_drag()).on_hover_cursor(cursor);
        crate::scenario::publish_target(&format!("params:resize:{name}"), rect);
        if response.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if response.double_clicked() {
            out = Resize::Reset(axes);
        } else if response.dragged() {
            let delta = response.drag_delta();
            let delta =
                egui::vec2(if axes.width { if free_left { -delta.x } else { delta.x } } else { 0.0 }, if axes.height { delta.y } else { 0.0 });
            if delta.is_finite() && delta != egui::Vec2::ZERO {
                out = Resize::By(axes, delta);
            }
        }
    }
    out
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
    let mut favorites: Vec<_> = visible.iter().copied().filter(|r| r.favorite).collect();
    sort.apply(&mut favorites, by_name);
    let mut users: Vec<_> = visible.iter().copied().filter(|r| r.source == "user").collect();
    sort.apply(&mut users, by_name);
    // Group once, in listing order, rather than scanning every parameter for each feature on
    // every frame (#2733).
    let mut groups: Vec<(&str, Vec<&Row>)> = Vec::new();
    let mut indices = std::collections::HashMap::new();
    for r in visible.iter().copied().filter(|r| r.source != "user") {
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

fn model_group(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, group: &str, rows: &[&Row], name_floor: f32) {
    // Focused editors and pending drafts must still receive focus-loss/commit events after
    // scrolling away. The measured size is used only while the group is idle.
    let active = ui.ctx().egui_wants_keyboard_input()
        || rows.iter().any(|r| {
            d.editing.contains_key(&r.name) || d.comments.contains_key(&r.name) || d.renaming.as_ref().is_some_and(|(old, _)| *old == r.name)
        });
    let measured = d.group_sizes.get(group).copied();
    let response = ui.push_id(("parameter_group", group), |ui| {
        if !active
            && let Some(size) = measured
            && !ui.is_rect_visible(egui::Rect::from_min_size(ui.next_widget_position(), size))
        {
            ui.allocate_space(size);
            return;
        }
        #[cfg(test)]
        {
            d.groups_drawn += 1;
        }
        ui_kit::section_count(ui, ("pgroup", group), group, true, Some(rows.len()), |ui| table(app, ui, d, rows, group, name_floor));
    });
    d.group_sizes.insert(group.to_owned(), response.response.rect.size());
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

/// One section's table of parameter rows with its sortable, resizable headings and add row.
/// `salt` keeps each section's widgets apart; `name_floor` is the shared Name column minimum.
fn table(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, rows: &[&Row], salt: &str, name_floor: f32) {
    let units: Vec<_> = UNITS.iter().map(|unit| unit_label(unit)).collect();
    // Name keeps a larger share and a shared content floor so long identifiers stay readable
    // and every section's columns line up (#8533, #8551 review). Full window/column resize is #5097.
    // Widths the user dragged a header divider to (#5097); the rest keep their automatic widths.
    let manual: Vec<Option<f32>> = ui.ctx().data_mut(|m| m.get_persisted(columns_id())).unwrap_or_default();
    let columns = [
        Column::new("", 0.0).min_content_width(solidcraft_theme::Tokens::get().space_24 * 3.0),
        Column::new("Name", 2.0).flex().min_content_width(name_floor),
        Column::new("Unit", 0.65).min_content_width(
            ui_kit::ContentWidth::dropdown(ui, &units).max(solidcraft_theme::Tokens::get().space_24 * 3.0 + solidcraft_theme::Tokens::get().space_6),
        ),
        Column::new("Expression", 1.5).flex(),
        Column::new("Value", 1.1).flex(),
        Column::new("Comment", 1.0).flex(),
        Column::new("Actions", 0.0).icon_only().min_content_width(ui_kit::table_row_height()),
    ];
    let columns: Vec<_> = columns.into_iter().enumerate().map(|(i, c)| c.manual_width(manual.get(i).copied().flatten())).collect();
    // An expression being edited may open over the cells to its right, up to the table's edge.
    let right = ui.max_rect().right().min(ui.clip_rect().right());
    // Enter commits a cell and moves to the one below in the same column, Shift+Enter above.
    let cells = egui::Id::unique(("params_cells", salt));
    let neighbour = |index: usize, key: Option<ui_kit::CellKey>| match key {
        Some(ui_kit::CellKey::Down) => rows.get(index + 1).map(|r| r.name.clone()),
        Some(ui_kit::CellKey::Up) => index.checked_sub(1).and_then(|i| rows.get(i)).map(|r| r.name.clone()),
        _ => None,
    };
    ui_kit::table_custom(ui, salt, &columns, solidcraft_ui_text::tr("manage.empty"), |ui, table| {
        // The Name heading sorts every section at once, each inside itself (#8563).
        table.headers_resizable(ui, columns_id(), |ui, index| {
            if index != 1 {
                return false;
            }
            let response = ui_kit::sort_header(ui, "Name", &mut d.sort);
            crate::scenario::publish_handle(&format!("params:sort:{salt}"), response.rect.center());
            true
        });
        for (index, r) in rows.iter().enumerate() {
            // Skip cells outside the clip while preserving the table's row geometry (#2723).
            // Drafts remain live until their edits have been committed.
            let busy = d.editing.contains_key(&r.name)
                || d.comments.contains_key(&r.name)
                || d.renaming.as_ref().is_some_and(|(old, _)| *old == r.name)
                || ui_kit::landing_in_row(ui.ctx(), cells, &r.name);
            table.row_visible(ui, &r.name, false, busy, |row| {
                row.cell(|ui| {
                    let star = if r.favorite { "★" } else { "☆" };
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let favorite = ui_kit::compact_button(ui, star, "Favourite", true, r.favorite);
                    crate::scenario::publish_handle(&format!("param:star:{}", r.name), favorite.rect.center());
                    if favorite.clicked() {
                        d.error = app.run("parameters.favorite", json!({"name": r.name, "favorite": !r.favorite})).err();
                    }
                    if salt == "user" {
                        // A sorted list doesn't show the stored order, so moving a row in it
                        // would jump to a place nobody sees; moving waits for the listed order.
                        let listed = d.sort == ui_kit::Sort::Listed;
                        for (glyph, tip, neighbour, side) in [
                            (ui_kit::MOVE_UP, "Move up", index.checked_sub(1).and_then(|i| rows.get(i)), "before"),
                            (ui_kit::MOVE_DOWN, "Move down", rows.get(index + 1), "after"),
                        ] {
                            let response = ui_kit::compact_button(ui, glyph, tip, listed && neighbour.is_some(), false);
                            crate::scenario::publish_handle(&format!("param:{side}:{}", r.name), response.rect.center());
                            if response.clicked()
                                && let Some(neighbour) = neighbour
                            {
                                d.error = app.run("parameters.move", json!({"name":r.name, side:neighbour.name})).err();
                            }
                        }
                    }
                });
                let renaming_here = d.rename_table == salt;
                row.cell(|ui| match &mut d.renaming {
                    Some((old, new)) if *old == r.name && renaming_here => {
                        let id = ui.make_persistent_id(("rename", salt, &r.name));
                        let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                        let response = ui_kit::cell_editor(ui, id, new, "Name", false);
                        ui_kit::landing(ui, cells, &r.name, "name", &response, new);
                        let key = ui_kit::cell_key(ui, focused);
                        let entered = enter_for(ui, focused) || key == Some(ui_kit::CellKey::Up);
                        if response.lost_focus() || entered {
                            let (old, new) = (old.clone(), new.trim().to_owned());
                            if !new.is_empty() && new != old {
                                d.error = app.run("parameters.rename", json!({"name":old,"new_name":new})).err();
                            }
                            d.renaming = None;
                            // The name below (or above) opens for renaming, its text selected.
                            if let Some(next) = neighbour(index, key) {
                                ui_kit::land_in_cell(ui.ctx(), cells, &next, "name");
                                d.renaming = Some((next.clone(), next));
                                d.rename_table = salt.to_string();
                            }
                        } else if !response.has_focus() {
                            response.request_focus();
                        }
                    }
                    _ => {
                        let rename = solidcraft_ui_text::tr_label("Double-click to rename");
                        // Keep long identifiers readable (#8533) without stacking the cell's own tooltip.
                        let response = ui_kit::name_cell_with_hint(ui, &r.name, false, rename);
                        crate::scenario::publish_handle(&format!("param:name:{}", r.name), response.rect.center());
                        if response.double_clicked() {
                            d.renaming = Some((r.name.clone(), r.name.clone()));
                            d.rename_table = salt.to_string();
                        }
                    }
                });
                row.cell(|ui| {
                    if r.source == "user" {
                        let mut selected = UNITS.iter().position(|u| *u == r.unit).unwrap_or(0);
                        let before = selected;
                        ui_kit::dropdown(ui, &format!("punit:{salt}:{}", r.name), &mut selected, &units);
                        if selected != before
                            && let Some(unit) = UNITS.get(selected)
                        {
                            d.error = app.run("parameters.change", json!({"name":r.name,"expression":r.expression,"unit":unit})).err();
                        }
                    } else {
                        ui_kit::text(ui, unit_label(&r.unit), ui_kit::Text::MonoCell);
                    }
                });
                let mut expression = d.editing.get(&r.name).cloned().unwrap_or_else(|| r.expression.clone());
                let live = (expression != r.expression).then(|| check(app, &expression, &r.unit));
                let invalid = matches!(live, Some(Err(_))) || (live.is_none() && r.error.is_some());
                let (shown, error) = match (&live, &r.error) {
                    (Some(Err(message)), _) | (None, Some(message)) => (message.clone(), true),
                    (Some(Ok(value)), _) => (value.clone(), false),
                    (None, None) => (show_value(r), false),
                };
                row.cell(|ui| {
                    let id = ui.make_persistent_id(("pexpr", salt, &r.name));
                    let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                    let response = ui_kit::wide_cell_editor(ui, id, &mut expression, "Expression", invalid, right, (&shown, error));
                    crate::scenario::publish_target_as(&format!("param:{}", r.name), &format!("param:{}", r.name), response.rect);
                    if salt == "fav" {
                        crate::scenario::publish_handle(&format!("param:favorite:{}", r.name), response.rect.center());
                    }
                    ui_kit::landing(ui, cells, &r.name, "expression", &response, &expression);
                    let completed = complete(ui, &response, &mut expression);
                    if response.changed() || completed {
                        d.editing.insert(r.name.clone(), expression.clone());
                    }
                    let key = if completed { None } else { ui_kit::cell_key(ui, focused) };
                    // Held edits (#9279) preview nothing: the preview is a rebuild too.
                    let apply_live = app.preferences.parameters_apply_live;
                    if apply_live && response.has_focus() && matches!(live, Some(Ok(_))) {
                        d.preview = Some((r.name.clone(), expression.clone()));
                    }
                    let entered = !completed && (enter_for(ui, focused) || key == Some(ui_kit::CellKey::Up));
                    if !completed && (response.lost_focus() || entered) {
                        d.preview = None;
                        if expression != r.expression && !apply_live {
                            // Kept in `editing` for Apply or OK, checked now.
                            d.error = check(app, &expression, &r.unit).err();
                        } else if expression != r.expression {
                            d.error = check(app, &expression, &r.unit)
                                .and_then(|_| app.run("parameters.change", json!({"name":r.name,"expression":expression})))
                                .err();
                            if d.error.is_none() {
                                d.editing.remove(&r.name);
                            }
                        } else {
                            d.editing.remove(&r.name);
                        }
                        if let Some(next) = neighbour(index, key) {
                            ui_kit::land_in_cell(ui.ctx(), cells, &next, "expression");
                        }
                    }
                });
                row.cell(|ui| {
                    ui_kit::text(ui, &shown, ui_kit::Text::Value(&format!("value:{}", r.name), error)).backing_on_hover_text(&shown);
                });
                row.cell(|ui| {
                    let mut comment = d.comments.get(&r.name).cloned().unwrap_or_else(|| r.comment.clone());
                    let id = ui.make_persistent_id(("comment", &r.name));
                    let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                    let response = ui_kit::cell_editor(ui, id, &mut comment, "Comment", false);
                    crate::scenario::publish_handle(&format!("param:comment:{}", r.name), response.rect.center());
                    ui_kit::landing(ui, cells, &r.name, "comment", &response, &comment);
                    let key = ui_kit::cell_key(ui, focused);
                    let entered = enter_for(ui, focused) || key == Some(ui_kit::CellKey::Up);
                    if response.changed() {
                        d.comments.insert(r.name.clone(), comment.clone());
                    }
                    if response.lost_focus() || entered {
                        if comment != r.comment {
                            d.error = app.run("parameters.comment", json!({"name":r.name,"comment":comment})).err();
                        }
                        d.comments.remove(&r.name);
                        if let Some(next) = neighbour(index, key) {
                            ui_kit::land_in_cell(ui.ctx(), cells, &next, "comment");
                        }
                    }
                });
                row.cell(|ui| {
                    if r.source == "user" && ui_kit::bin_button(ui, "Delete").clicked() {
                        d.error = app.run("parameters.delete", json!({"name":r.name})).err();
                    }
                });
            });
        }
        if salt == "user" {
            if d.adding {
                ui.push_id("parameter_draft", |ui| add_row(app, ui, table, d, right));
            }
            if table.add_prompt(ui, "Add parameter", "Add parameter").clicked() {
                d.adding = true;
                d.focus_new = true;
            }
        }
    });
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
    rows.iter().filter_map(|r| d.editing.get(&r.name).filter(|e| **e != r.expression).map(|e| (r.name.clone(), json!(e)))).collect()
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
    d.error = app.run("parameters.change", params).err();
    if d.error.is_some() {
        return false;
    }
    for name in changes.keys() {
        d.editing.remove(name);
    }
    true
}

/// "Apply each change as you leave its field", remembered across sessions (#9279). Turning it
/// back on applies what was waiting.
fn apply_live_checkbox(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, rows: &[Row]) {
    let response = ui_kit::checkbox(ui, solidcraft_ui_text::tr("parameters.apply_live"), &mut app.preferences.parameters_apply_live)
        .backing_on_hover_text(solidcraft_ui_text::tr("parameters.apply_live_tip"));
    crate::scenario::publish_handle("params:apply_live", response.rect.center());
    if response.changed() && app.preferences.parameters_apply_live {
        apply_held(app, d, rows);
    }
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
    let menu_was_open = egui::Popup::is_any_open(ui.ctx());
    let mut submit = false;
    let mut left = false;
    let mut discard = false;
    let reveal = d.focus_new;
    let response = table.row(ui, "new", false, |row| {
        row.cell(|_| {});
        row.cell(|ui| {
            let response = ui_kit::cell_editor(ui, name_id, &mut d.new_name, "new parameter", false);
            crate::scenario::publish_handle("param:new:name", response.rect.center());
            if d.focus_new {
                response.request_focus();
                d.focus_new = false;
            }
            left |= response.lost_focus();
        });
        row.cell(|ui| {
            let units: Vec<_> = UNITS.iter().map(|u| unit_label(u)).collect();
            let response = ui_kit::dropdown(ui, "pnew_unit", &mut d.new_unit, &units);
            row_ids.push(response.id);
            left |= response.lost_focus();
            crate::scenario::publish_handle("param:new:unit", response.rect.center());
        });
        let unit = UNITS.get(d.new_unit).copied().unwrap_or("mm");
        // A blank expression is not yet an error: nothing to show, and nothing to add.
        let live = if d.new_expr.trim().is_empty() { Ok(String::new()) } else { new_check(app, d, unit) };
        let (value, invalid) = match live {
            Ok(v) => (v, false),
            Err(e) => (e, true),
        };
        row.cell(|ui| {
            let response = ui_kit::wide_cell_editor(ui, expr_id, &mut d.new_expr, "Expression", invalid, right, (&value, invalid));
            crate::scenario::publish_handle("param:new:expression", response.rect.center());
            let completed = complete(ui, &response, &mut d.new_expr);
            submit = !completed && enter_for(ui, focused_id.is_some());
            left |= !completed && response.lost_focus();
        });
        row.cell(|ui| {
            ui_kit::text(ui, &value, ui_kit::Text::Value("new_value", invalid)).backing_on_hover_text(&value);
        });
        row.cell(|ui| {
            let response = ui_kit::cell_editor(ui, comment_id, &mut d.new_comment, "Comment", false);
            crate::scenario::publish_handle("param:new:comment", response.rect.center());
            left |= response.lost_focus();
            submit |= enter_for(ui, focused_id.is_some());
        });
        row.cell(|ui| {
            let response = ui_kit::bin_button(ui, "Delete");
            row_ids.push(response.id);
            crate::scenario::publish_handle("param:new:delete", response.rect.center());
            left |= response.lost_focus();
            discard = response.clicked();
        });
    });
    if reveal {
        table.reveal_row(response.rect);
    }
    let unit = UNITS.get(d.new_unit).copied().unwrap_or("mm");
    let blank = d.new_expr.trim().is_empty();
    let live = if blank { Err(String::new()) } else { new_check(app, d, unit) };
    let valid = !d.new_name.trim().is_empty() && live.is_ok();
    crate::scenario::publish_count("param:new:valid", f64::from(valid));
    let clicked = ui.input(|i| i.pointer.any_click().then(|| i.pointer.interact_pos()).flatten());
    let clicked_outside = !menu_was_open && clicked.is_some_and(|p| !response.rect.contains(p));
    // Losing a cell's focus is not leaving the draft: a click or Tab into another cell
    // must keep its values and geometry (#4461). The unit menu belongs to the draft too.
    let left_row = !menu_was_open
        && !egui::Popup::is_any_open(ui.ctx())
        && (clicked_outside
            || (left && !ui.memory(|m| row_ids.iter().any(|id| m.has_focus(*id))) && !clicked.is_some_and(|p| response.rect.contains(p))));
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
    // Every name (no prefix), then the units (offered only after a letter is typed).
    for prefix in ["", "m", "c", "i", "f", "d", "r", "u", "y", "k", "n"] {
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
    crate::expression_complete::complete(ui, resp, text) || crate::value_step::step(ui, resp, text)
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
                assert_eq!(texts, [expected], "only a clipped name needs repeating");
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

    #[test]
    fn escape_keeps_previously_committed_parameter_edit() {
        let mut h = escape_dialog();
        escape_type(&mut h, "param:width", "20 mm");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "20 mm");
        let before = escape_state(&h);
        escape_type(&mut h, "param:width", "30 mm");
        escape_closed(&mut h, &before, "20 mm");
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
        // The hint above the check box says when edits apply, whichever way it is set.
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
                h.step(&json!({"key": "Tab"})).unwrap(); // Unit
                h.step(&json!({"key": "Tab"})).unwrap(); // Expression
            } else {
                h.step(&json!({"click": {"handle": "param:new:expression"}})).unwrap();
            }
            assert!(h.app.session.doc.param("width").is_none(), "moving between cells must not save the draft");
            assert_eq!(crate::scenario::rect_of("dialog:table:user:new"), Some(before), "the draft row must not move");
            h.step(&json!({"key": "A", "cmd": true})).unwrap();
            h.step(&json!({"text": "25 mm"})).unwrap();
            if keyboard {
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
        assert_eq!((fav, users), (vec!["d10", "Alpha", "s_d1"], vec!["d10", "width", "d2", "Alpha", "d1"]), "unsorted: the listed order");
        assert_eq!(groups, [("Sketch1".into(), vec!["s_d3", "s_d1", "s_d2"]), ("Box1".into(), vec!["b_len", "b_h"])]);
        let (fav, users, groups) = shown(ui_kit::Sort::Ascending);
        assert_eq!(fav, ["Alpha", "d10", "s_d1"]);
        assert_eq!(users, ["Alpha", "d1", "d2", "d10", "width"]);
        assert_eq!(
            groups,
            [("Sketch1".into(), vec!["s_d1", "s_d2", "s_d3"]), ("Box1".into(), vec!["b_h", "b_len"])],
            "groups keep the design's order"
        );
        let (fav, users, groups) = shown(ui_kit::Sort::Descending);
        assert_eq!(fav, ["s_d1", "d10", "Alpha"]);
        assert_eq!(users, ["width", "d10", "d2", "d1", "Alpha"]);
        assert_eq!(groups, [("Sketch1".into(), vec!["s_d3", "s_d2", "s_d1"]), ("Box1".into(), vec!["b_len", "b_h"])]);
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

    /// #8563: clicking a Name heading sorts every table in the window, the choice outlives the
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
        assert_eq!(order("user", &users), users, "the listed order by default");
        assert!(model.len() > 1, "the box drives several parameters: {model:?}");
        let mut sorted_model = model.clone();
        ui_kit::Sort::Ascending.apply(&mut sorted_model, |s| s.as_str());

        h.step(&json!({"click": {"handle": "params:sort:user"}})).unwrap();
        h.frames(3);
        assert_eq!(order("user", &users), ["Alpha", "span", "t1", "t2", "t10"]);
        // The bounded card may not show the model group below the user table: filter to it.
        h.app.params.as_mut().unwrap().filter = group.clone();
        h.frames(4);
        assert_eq!(order(&group, &model), sorted_model, "a model group sorts too");
        h.app.params.as_mut().unwrap().filter.clear();
        h.frames(4);
        assert!(crate::scenario::handle_at("param:after:Alpha").is_some());
        let before = h.app.session.doc.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        h.step(&json!({"click": {"handle": "param:after:Alpha"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), before, "no move while sorted");

        // Closed and opened again: still sorted.
        h.app.params = None;
        h.frames(2);
        h.app.start("parameters.change");
        h.frames(4);
        assert_eq!(h.app.params.as_ref().unwrap().sort, ui_kit::Sort::Ascending);
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
        assert!(h.app.params.as_ref().unwrap().groups_drawn < 10, "offscreen feature groups must not build tables on idle frames");
        eprintln!("740 feature groups: closed {closed:.2} ms, open {open:.2} ms");
    }

    #[test]
    fn model_groups_keep_scroll_geometry_and_editability() {
        let mut h = many_feature_groups();
        h.app.start("parameters.change");
        h.frames(5);
        let height = crate::scenario::count_of("dialog:body:content_height").unwrap();
        let sizes = h.app.params.as_ref().unwrap().group_sizes.clone();
        h.frames(20);
        assert_eq!(h.app.params.as_ref().unwrap().group_sizes, sizes, "skipping groups must preserve their measured size");
        let last = h.app.params.as_ref().unwrap().listed.as_ref().unwrap().1.iter().find(|r| r.group == "Plane739").unwrap().name.clone();
        let viewport = crate::scenario::rect_of("dialog:body:viewport").unwrap();
        h.call("ui.scroll", json!({"x":viewport.center().x,"y":viewport.center().y,"delta":-100_000}));
        h.frames(40);
        assert!((crate::scenario::count_of("dialog:body:content_height").unwrap() - height).abs() < 1.0);
        let at = crate::scenario::handle_at(&format!("param:{last}")).unwrap();
        assert!(crate::scenario::rect_of("dialog:body:viewport").unwrap().contains(at), "the last parameter remains reachable");
        assert!(h.app.params.as_ref().unwrap().groups_drawn < 10);
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
        assert_eq!(h.app.params.as_ref().unwrap().group_sizes.len(), 1, "filtering invalidates the group layout");
        assert!(crate::scenario::handle_at(&format!("param:{last}")).is_some());
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
        // A value already in the expression box: Tab into it (past the unit) and type, and the
        // value is replaced.
        h.app.params.as_mut().unwrap().new_expr = "5 mm".into();
        h.step(&json!({"click": {"handle": "param:new:name"}})).unwrap();
        h.step(&json!({"key": "Tab"})).unwrap();
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

    /// #8551 review: Favorites and User Parameters share one Name-column floor, so a long name
    /// only in User does not shift that section's Expression column relative to Favorites.
    #[test]
    fn parameter_tables_share_one_name_column_floor() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1600, "height": 1200}));
        h.app.run("parameters.add", json!({"name": "w", "expression": "1 mm"})).unwrap();
        h.app.run("parameters.add", json!({"name": "Driver_MountHole_to_Cover_Shared_Base_Layout_Angle", "expression": "8 deg"})).unwrap();
        h.app.run("parameters.favorite", json!({"name": "w", "favorite": true})).unwrap();
        h.app.start("parameters.change");
        h.frames(6);
        let fav = crate::scenario::handle_at("param:favorite:w").expect("Favorites expression");
        let user = crate::scenario::handle_at("param:w").expect("User Parameters expression");
        assert!((fav.x - user.x).abs() < 1.0, "Expression columns lined up: favorites {fav:?} user {user:?}");
    }

    #[test]
    fn model_groups_and_user_tables_fit_their_viewports() {
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
            for id in ["fav", "user", "Box1", "Box2"] {
                if id.starts_with("Box") {
                    // The bounded card no longer shows every group at once. Bring each
                    // model table into view before checking its horizontal layout.
                    h.app.params.as_mut().unwrap().filter = id.into();
                    h.frames(6);
                }
                let viewport = crate::scenario::rect_of(&format!("dialog:table:{id}:viewport")).unwrap();
                let content = crate::scenario::count_of(&format!("dialog:table:{id}:content_width")).unwrap();
                assert!(content <= f64::from(viewport.width().ceil()), "{id} on {screen_width} screen: {content} outside {viewport:?}");
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
}
