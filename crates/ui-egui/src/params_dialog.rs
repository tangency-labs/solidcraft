//! The Change Parameters dialog and parameter-name autocomplete for expression boxes.
//!
//! A window over the engine's `parameters.*` commands: favourites first, then user parameters
//! (name, unit, expression, value, comment; add, rename, delete) and model parameters grouped by
//! the sketch or feature they drive (editable). Expressions validate as you type
//! (`expr.evaluate`) with a live model preview; Enter or leaving the field applies them.
//! Escape closes the window and discards pending edits and their preview.
//! Import and export go through `parameters.import` / `parameters.export`.
//!
//! [`complete`] adds suggestions (parameters, functions, units) under any expression box: the
//! names come from `parameters.complete`, cached per document revision by [`publish`].

use std::sync::Arc;

use serde_json::{Value, json};

use crate::SolveApp;
#[cfg(not(target_arch = "wasm32"))]
use crate::ui_kit::floating::NativeDockDrag;
use crate::ui_kit::{self, Column};

/// What Import reads. The picker must list them: on macOS, files of a type no filter names
/// can't be picked (#3216).
pub(crate) const IMPORT_TYPES: &[&str] = &["csv", "json"];

const UNITS: [&str; 10] = ["mm", "cm", "m", "in", "ft", "deg", "rad", "", "um", "text"];

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct ParameterWindow {
    detached: bool,
    place: Option<[f32; 4]>,
}

/// Dialog state (what is being typed; the parameters themselves live in the document).
#[derive(Default)]
pub struct ParamsDialog {
    pub filter: String,
    #[cfg(not(target_arch = "wasm32"))]
    window: ParameterWindow,
    #[cfg(not(target_arch = "wasm32"))]
    window_loaded: bool,
    #[cfg(not(target_arch = "wasm32"))]
    window_placed: bool,
    #[cfg(not(target_arch = "wasm32"))]
    drag_offset: Option<egui::Vec2>,
    #[cfg(not(target_arch = "wasm32"))]
    tear_off: Option<ui_kit::floating::TearOff>,
    #[cfg(not(target_arch = "wasm32"))]
    native_drag: NativeDockDrag,
    #[cfg(not(target_arch = "wasm32"))]
    dock_at: Option<egui::Pos2>,
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
    fit_columns: bool,
}

impl ParamsDialog {
    /// The add row starts blank, its expression too (#2919): a value put there for the user was
    /// almost never the one wanted, and had to be erased first.
    pub fn new() -> Self {
        ParamsDialog { reset_scroll: true, ..Default::default() }
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
                value: r.get("value").and_then(Value::as_f64),
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
    if app.params.is_some() || ctx.memory(|m| m.focused().is_some()) {
        publish(app, ctx);
    }
    let Some(mut d) = app.params.take() else { return };
    #[cfg(not(target_arch = "wasm32"))]
    let close = {
        if !d.window_loaded {
            d.window = ctx.data_mut(|data| data.get_persisted::<ParameterWindow>(egui::Id::new("parameters_window"))).unwrap_or_default();
            d.window_loaded = true;
        }
        let close = if d.window.detached { native_window(app, ctx, &mut d) } else { embedded_window(app, ctx, &mut d) };
        ctx.data_mut(|data| data.insert_persisted(egui::Id::new("parameters_window"), d.window.clone()));
        close
    };
    #[cfg(target_arch = "wasm32")]
    let close = embedded_window(app, ctx, &mut d);
    if !close {
        app.params = Some(d);
    }
}

fn publish_handles() {
    if let Some(at) = crate::scenario::handle_at("dialog:close") {
        crate::scenario::publish_handle("params:close", at);
    }
    if let Some(rect) = crate::scenario::rect_of("dialog:header") {
        crate::scenario::publish_handle("params:title", rect.center());
    }
}

fn embedded_window(app: &mut SolveApp, ctx: &egui::Context, d: &mut ParamsDialog) -> bool {
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let viewport = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    if !viewport.is_finite() || !viewport.is_positive() {
        return false;
    }
    let palette = app.viewport.palette_rect.filter(|r| r.is_finite() && r.is_positive());
    let side =
        if palette.is_some() && crate::sketch_palette::fits_beside_a_dialog(app, ctx) { crate::frame::Side::Left } else { crate::frame::Side::Right };
    let top = crate::shell::panel_top(app, viewport, viewport.bottom(), side);
    let docked = egui::pos2(side.anchor_x(viewport), top);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(position) = d.dock_at.take() {
        let size = ctx.data_mut(|data| data.get_persisted::<egui::Vec2>(egui::Id::new("sc_params_dialog").with("size"))).unwrap_or_default();
        let anchor = if side == crate::frame::Side::Right { position + egui::vec2(size.x, 0.0) } else { position };
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(("sc_dock_offset", "PARAMETERS")), anchor - docked));
    }
    let moved = crate::frame::dock_offset(ctx, "PARAMETERS");
    let drag_bounds = crate::shell::tool_drag_rect(app, viewport);
    let anchor = crate::frame::offset_anchor(docked, moved, drag_bounds, side);
    let title = crate::i18n::tr_label("Parameters");
    let panel = ui_kit::Panel::new(
        title,
        egui::Id::new("sc_params_dialog"),
        anchor,
        drag_bounds,
        side,
        ui_kit::Panel::feature_frame(ctx, app.ui.layout, side),
    )
    .wide()
    .resizable()
    .body_rows(16.0)
    .reset_scroll(d.reset_scroll);
    let panel = if side == crate::frame::Side::Left {
        let available = palette.map_or(viewport.width(), |r| (r.left() - viewport.left() - crate::theme::Tokens::get().space_8).max(0.0));
        panel.width((crate::theme::Tokens::get().form_card_width * 3.0).min(available))
    } else {
        panel
    };
    let mut close = false;
    let response = panel.window(ctx).show(ctx, |ui| {
        let (closed, action) = panel.feature(
            ui,
            ui_kit::FeatureOptions {
                icon: solidcraft_engine::find_command("parameters.change").map_or("", |c| c.icon),
                title,
                drag_key: "PARAMETERS",
                moved,
                docked,
                apply_label: None,
            },
            false,
            true,
            |ui| contents(app, ui, d),
        );
        d.reset_scroll = false;
        close = closed || matches!(action, ui_kit::Action::Ok | ui_kit::Action::Cancel);
    });
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(response) = response {
        detach_when_dragged_out(ctx, d, response.response.rect);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = response;
    publish_handles();
    close
}

#[cfg(not(target_arch = "wasm32"))]
fn detach_when_dragged_out(ctx: &egui::Context, d: &mut ParamsDialog, rect: egui::Rect) {
    if ctx.input(|i| i.pointer.primary_pressed()) {
        d.drag_offset = ctx.input(|i| i.pointer.press_origin()).filter(|p| rect.contains(*p)).map(|p| p - rect.min);
    }
    if !crate::frame::title_dragging(ctx, "PARAMETERS") {
        if !ctx.input(|i| i.pointer.primary_down()) {
            d.drag_offset = None;
        }
        return;
    }
    let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) else { return };
    let grab = *d.drag_offset.get_or_insert_with(|| ctx.input(|i| i.pointer.press_origin()).unwrap_or(pointer) - rect.min);
    let desired = egui::Rect::from_min_size(pointer - grab, rect.size());
    // Hand off while the pointer is still captured, before the OS clips pointer movement
    // at the app edge. Use the intended position, not the card's on-screen clamped rectangle.
    if !ui_kit::floating::crosses_border(ctx.content_rect(), desired, pointer, grab) {
        return;
    }
    let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)).unwrap_or_default();
    let position = origin + pointer.to_vec2() - grab;
    d.window.place = Some([position.x, position.y, rect.width(), rect.height()]);
    d.window.detached = true;
    d.window_placed = false;
    d.tear_off = Some(ui_kit::floating::TearOff::new(position, grab, ctx.input(|i| i.pointer.primary_down())));
    d.native_drag = NativeDockDrag::default();
    ui_kit::floating::mark_handoff(ctx);
    ctx.request_repaint();
}

#[cfg(not(target_arch = "wasm32"))]
fn snap_native_back(ctx: &egui::Context, d: &mut ParamsDialog, outer: egui::Rect, inner: egui::Rect, parent: egui::Rect, now: f64) {
    if d.native_drag.update(outer, inner, parent, now) {
        d.window.detached = false;
        d.window_placed = false;
        d.tear_off = None;
        d.drag_offset = None;
        d.dock_at = Some(outer.min - parent.min.to_vec2());
        d.native_drag = NativeDockDrag::default();
        ui_kit::floating::end_handoff(ctx);
        ctx.data_mut(|data| {
            data.insert_persisted(egui::Id::new("sc_params_dialog").with("size"), inner.size());
            data.remove::<u64>(egui::Id::new(("sc_title_drag_frame", "PARAMETERS")));
        });
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    } else if d.native_drag.moving {
        // Native drags can stop without a pointer-up event; observe settled geometry too.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_window_builder(app: &SolveApp, ctx: &egui::Context, d: &ParamsDialog) -> egui::ViewportBuilder {
    let t = crate::theme::Tokens::get();
    let title = format!("{} — {}", crate::i18n::tr_label("Parameters"), app.session.doc.name);
    let builder = egui::ViewportBuilder::default().with_title(title).with_min_inner_size([t.form_card_width, 200.0]).with_resizable(true);
    if d.window_placed {
        return builder;
    }
    if let Some([x, y, w, h]) = d.window.place
        && [x, y, w, h].iter().all(|v| v.is_finite())
        && x.abs() <= 100_000.0
        && y.abs() <= 100_000.0
        && (t.form_card_width..=10_000.0).contains(&w)
        && (200.0..=10_000.0).contains(&h)
    {
        return builder.with_position([x, y]).with_inner_size([w, h]);
    }
    let size = ctx
        .data_mut(|data| data.get_persisted::<egui::Vec2>(egui::Id::new("sc_params_dialog").with("size")))
        .filter(|s| s.is_finite() && s.x >= t.form_card_width && s.y >= 200.0)
        .unwrap_or(egui::vec2(t.form_card_width * 3.0, 640.0));
    builder.with_inner_size(size)
}

#[cfg(not(target_arch = "wasm32"))]
fn native_window(app: &mut SolveApp, ctx: &egui::Context, d: &mut ParamsDialog) -> bool {
    let parent = ctx.input_for(egui::ViewportId::ROOT, |i| i.viewport().inner_rect);
    let builder = native_window_builder(app, ctx, d);
    let builder = d.tear_off.as_ref().map_or_else(|| builder.clone(), |drag| drag.prepare(builder.clone()));
    let opening = !d.window_placed;
    d.window_placed = true;
    let mut close = false;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("sc_parameters_window"), builder, |ui, class| {
        let child = ui.ctx().clone();
        crate::expression_complete::begin_frame(&child);
        if child.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
            child.request_repaint_of(egui::ViewportId::ROOT);
            return;
        }
        child.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let frame = ui_kit::Container::new(&child, ui_kit::Surface::Panel).edge_frame(crate::theme::Tokens::get().dialog_body);
        let response = ui_kit::Container::new(&child, ui_kit::Surface::Viewport).central().frame(frame).show(ui, |ui| {
            let rect = ui.max_rect();
            let title = crate::i18n::tr_label("Parameters");
            let panel =
                ui_kit::Panel::new(title, egui::Id::new("sc_parameters_native_content"), rect.left_top(), rect, crate::frame::Side::Left, frame)
                    .native_content()
                    .reset_scroll(d.reset_scroll);
            let (closed, action) = panel.feature(
                ui,
                ui_kit::FeatureOptions {
                    icon: solidcraft_engine::find_command("parameters.change").map_or("", |c| c.icon),
                    title,
                    drag_key: "PARAMETERS_NATIVE",
                    moved: egui::Vec2::ZERO,
                    docked: rect.left_top(),
                    apply_label: None,
                },
                false,
                true,
                |ui| contents(app, ui, d),
            );
            close |= closed || matches!(action, ui_kit::Action::Ok | ui_kit::Action::Cancel);
        });
        d.reset_scroll = false;
        crate::scenario::publish_rect("params:native_window", response.response.rect);
        publish_handles();
        if class != egui::ViewportClass::EmbeddedWindow {
            let (outer, inner, closing) = child.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().close_requested()));
            if d.tear_off.is_none() {
                ui_kit::floating::native_resize(ui, egui::vec2(crate::theme::Tokens::get().form_card_width, 200.0));
            }
            let following = ui_kit::floating::follow_pointer(&child, &mut d.tear_off, outer, inner);
            if opening && following.is_none() {
                child.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if let (Some(outer), Some(inner)) = (outer, inner) {
                let position = following.unwrap_or(outer.min);
                d.window.place = Some([position.x, position.y, inner.width(), inner.height()]);
                if !closing
                    && following.is_none()
                    && d.window.detached
                    && let Some(parent) = parent
                {
                    snap_native_back(&child, d, outer, inner, parent, child.input(|i| i.time));
                }
            }
            close |= closing;
        }
        if close || !d.window.detached {
            child.request_repaint_of(egui::ViewportId::ROOT);
        }
    });
    close
}

fn contents(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog) -> ui_kit::ComputeStatus {
    let all = if let Some((_, rows)) = d.listed.as_ref().filter(|(rev, _)| *rev == app.session.revision) {
        rows.clone()
    } else {
        let rows = Arc::new(rows(app));
        d.listed = Some((app.session.revision, rows.clone()));
        d.lists += 1;
        rows
    };
    ui_kit::text(ui, crate::i18n::tr("manage.parameters_hint"), ui_kit::Text::Hint("manage"));
    ui_kit::row(ui, "Filter", |ui| {
        let id = ui.make_persistent_id("parameter_filter");
        ui_kit::search_field(ui, id, &mut d.filter, "Filter");
    });
    ui.horizontal_wrapped(|ui| {
        if ui_kit::icon_label_button(ui, "Import…", "import_tray", true, false)
            .on_hover_text(crate::i18n::tr_label("CSV or JSON: updates existing parameters, adds new ones"))
            .clicked()
        {
            app.pick_open_command("parameters.import", json!({}), IMPORT_TYPES);
        }
        for (label, ext) in [("Export CSV…", "csv"), ("Export JSON…", "json")] {
            if ui_kit::icon_label_button(ui, label, "export_tray", true, false).clicked() {
                app.pick_save_command(&format!("parameters.{ext}"), &[ext], "parameters.export", json!({"format": ext}));
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let label = crate::i18n::tr(if d.window.detached { "manage.parameters_dock" } else { "manage.parameters_pop_out" });
            let response = ui_kit::button(ui, label, ui_kit::Button::Normal);
            crate::scenario::publish_handle("params:window_mode", response.rect.center());
            if response.clicked() {
                d.window.detached = !d.window.detached;
                d.window_placed = false;
                d.native_drag = NativeDockDrag::default();
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        }
    });
    if let Some(error) = &d.error {
        ui_kit::text(ui, error, ui_kit::Text::Error("parameters_error"));
    }
    let filter = d.filter.to_ascii_lowercase();
    let visible: Vec<_> = all
        .iter()
        .filter(|r| {
            filter.is_empty()
                || r.name.to_ascii_lowercase().contains(&filter)
                || r.comment.to_ascii_lowercase().contains(&filter)
                || r.group.to_ascii_lowercase().contains(&filter)
        })
        .collect();
    let favorites: Vec<_> = visible.iter().copied().filter(|r| r.favorite).collect();
    if !favorites.is_empty() {
        ui_kit::section_count(ui, "favorites", "Favorites", true, Some(favorites.len()), |ui| table(app, ui, d, &favorites, "fav", "favorites"));
    }
    let users: Vec<_> = visible.iter().copied().filter(|r| r.source == "user").collect();
    ui_kit::section_count(ui, "users", "User Parameters", true, Some(users.len()), |ui| {
        table(app, ui, d, &users, "user", "users");
    });
    ui_kit::section(ui, "models", "Model Parameters", true, |ui| {
        // Group once, in listing order, rather than scanning every parameter for
        // each feature on every frame (#2733).
        let mut groups: Vec<(&str, Vec<&Row>)> = Vec::new();
        let mut indices = std::collections::HashMap::new();
        for r in visible.iter().filter(|r| r.source != "user") {
            let index = *indices.entry(r.group.as_str()).or_insert_with(|| {
                groups.push((&r.group, Vec::new()));
                groups.len() - 1
            });
            if let Some((_, members)) = groups.get_mut(index) {
                members.push(r);
            }
        }
        ui.scope(|ui| {
            ui.spacing_mut().indent = crate::theme::Tokens::get().space_14;
            ui.indent("model_groups", |ui| {
                let layout = GroupLayout {
                    revision: app.session.revision,
                    filter: filter.clone(),
                    width: ui.available_width(),
                    pixels_per_point: ui.ctx().pixels_per_point(),
                    style: ui.style().clone(),
                    font: app.applied_ui_font.clone(),
                    system_fonts: app.applied_system_fonts,
                    language: crate::i18n::language(),
                    fit_columns: app.preferences.parameters_fit_columns,
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
                    model_group(app, ui, d, group, &members);
                }
            });
        });
    });
    ui_kit::ComputeStatus { text: None, busy: false, applying: false }
}

fn model_group(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, group: &str, rows: &[&Row]) {
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
        ui_kit::section_count(ui, ("pgroup", group), group, true, Some(rows.len()), |ui| table(app, ui, d, rows, group, "model"));
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

fn table(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, rows: &[&Row], salt: &str, kind: &str) {
    let units: Vec<_> = UNITS.iter().map(|unit| unit_label(unit)).collect();
    let t = crate::theme::Tokens::get();
    let name_width = rows
        .iter()
        .map(|r| ui.painter().layout_no_wrap(r.name.clone(), crate::fonts::medium(t.font_label), t.form_ink).size().x.ceil())
        .fold(ui_kit::ContentWidth::label(ui, "Name"), f32::max)
        + t.space_12;
    let expression_width = rows
        .iter()
        .map(|r| ui.painter().layout_no_wrap(r.expression.clone(), egui::FontId::monospace(t.font_label), t.form_ink).size().x.ceil())
        .fold(0.0, f32::max)
        + t.space_16;
    let value_width = rows
        .iter()
        .map(|r| ui.painter().layout_no_wrap(show_value(r), egui::FontId::monospace(t.font_body), t.form_ink).size().x.ceil())
        .fold(0.0, f32::max)
        + t.space_8;
    let auto_fit = app.preferences.parameters_fit_columns;
    let fit = |column: Column<'static>, width| if auto_fit { column.fit_content(width) } else { column };
    let mut columns = [
        Column::new("", 0.0).min_content_width(crate::theme::Tokens::get().space_24 * 3.0),
        fit(Column::new("Name", 1.1).flex(), name_width).resizable(t.control_height),
        Column::new("Unit", 0.65)
            .min_content_width(
                ui_kit::ContentWidth::dropdown(ui, &units).max(crate::theme::Tokens::get().space_24 * 3.0 + crate::theme::Tokens::get().space_6),
            )
            .resizable(t.control_height),
        fit(Column::new("Expression", 1.5).flex(), expression_width).resizable(t.control_height * 2.0),
        fit(Column::new("Value", 1.1).flex(), value_width).resizable(t.control_height),
        Column::new("Comment", 1.2).flex().resizable(t.control_height),
        Column::new("Actions", 0.0).icon_only().min_content_width(ui_kit::table_row_height()),
    ];
    for (column, label) in columns.iter_mut().zip(["", "Name", "Unit", "Expression", "Value", "Comment", "Actions"]) {
        column.set_resize_id(egui::Id::new(("parameters_columns", kind, salt, label)));
    }
    ui_kit::table(ui, salt, &columns, crate::i18n::tr("manage.empty"), |ui, table| {
        for (index, r) in rows.iter().enumerate() {
            // Skip cells outside the clip while preserving the table's row geometry (#2723).
            // Drafts remain live until their edits have been committed.
            let busy =
                d.editing.contains_key(&r.name) || d.comments.contains_key(&r.name) || d.renaming.as_ref().is_some_and(|(old, _)| *old == r.name);
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
                        for (glyph, tip, neighbour, side) in [
                            (ui_kit::MOVE_UP, "Move up", index.checked_sub(1).and_then(|i| rows.get(i)), "before"),
                            (ui_kit::MOVE_DOWN, "Move down", rows.get(index + 1), "after"),
                        ] {
                            let response = ui_kit::compact_button(ui, glyph, tip, neighbour.is_some(), false);
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
                        let entered = enter_for(ui, focused);
                        if response.lost_focus() || entered {
                            let (old, new) = (old.clone(), new.trim().to_owned());
                            if !new.is_empty() && new != old {
                                d.error = app.run("parameters.rename", json!({"name":old,"new_name":new})).err();
                            }
                            d.renaming = None;
                        } else if !response.has_focus() {
                            response.request_focus();
                        }
                    }
                    _ => {
                        let response =
                            ui_kit::text(ui, &r.name, ui_kit::Text::NameCell(false)).on_hover_text(crate::i18n::tr_label("Double-click to rename"));
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
                row.cell(|ui| {
                    let id = ui.make_persistent_id(("pexpr", salt, &r.name));
                    let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                    let response = ui_kit::cell_editor(ui, id, &mut expression, "Expression", invalid);
                    crate::scenario::publish_handle(&format!("param:{}", r.name), response.rect.center());
                    if salt == "fav" {
                        crate::scenario::publish_handle(&format!("param:favorite:{}", r.name), response.rect.center());
                    }
                    let completed = complete(ui, &response, &mut expression);
                    if response.changed() || completed {
                        d.editing.insert(r.name.clone(), expression.clone());
                    }
                    if response.has_focus() && matches!(live, Some(Ok(_))) {
                        d.preview = Some((r.name.clone(), expression.clone()));
                    }
                    let entered = !completed && enter_for(ui, focused);
                    if !completed && (response.lost_focus() || entered) {
                        d.preview = None;
                        if expression != r.expression {
                            d.error = check(app, &expression, &r.unit)
                                .and_then(|_| app.run("parameters.change", json!({"name":r.name,"expression":expression})))
                                .err();
                            if d.error.is_none() {
                                d.editing.remove(&r.name);
                            }
                        } else {
                            d.editing.remove(&r.name);
                        }
                    }
                });
                row.cell(|ui| {
                    let (text, error) = match (&live, &r.error) {
                        (Some(Err(message)), _) | (None, Some(message)) => (message.clone(), true),
                        (Some(Ok(value)), _) => (value.clone(), false),
                        (None, None) => (show_value(r), false),
                    };
                    ui_kit::text(ui, &text, ui_kit::Text::Value(&format!("value:{}", r.name), error)).on_hover_text(&text);
                });
                row.cell(|ui| {
                    let mut comment = d.comments.get(&r.name).cloned().unwrap_or_else(|| r.comment.clone());
                    let id = ui.make_persistent_id(("comment", &r.name));
                    let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                    let response = ui_kit::cell_editor(ui, id, &mut comment, "Comment", false);
                    let entered = enter_for(ui, focused);
                    if response.changed() {
                        d.comments.insert(r.name.clone(), comment.clone());
                    }
                    if response.lost_focus() || entered {
                        if comment != r.comment {
                            d.error = app.run("parameters.comment", json!({"name":r.name,"comment":comment})).err();
                        }
                        d.comments.remove(&r.name);
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
                ui.push_id("parameter_draft", |ui| add_row(app, ui, table, d));
            }
            if table.add_prompt(ui, "Add parameter", "Add parameter").clicked() {
                d.adding = true;
                d.focus_new = true;
            }
        }
    });
}

fn unit_label(u: &str) -> &str {
    if u.is_empty() { crate::i18n::tr_label("none") } else { u }
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

/// The add row, one cell per column of the user table it ends.
fn add_row(app: &mut SolveApp, ui: &mut egui::Ui, table: &mut ui_kit::Table<'_>, d: &mut ParamsDialog) {
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
            select_all_on_tab(ui, &response, &d.new_name);
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
        row.cell(|ui| {
            // A blank expression is not yet an error: nothing to show, and nothing to add.
            let invalid = !d.new_expr.trim().is_empty() && new_check(app, d, unit).is_err();
            let response = ui_kit::cell_editor(ui, expr_id, &mut d.new_expr, "Expression", invalid);
            select_all_on_tab(ui, &response, &d.new_expr);
            crate::scenario::publish_handle("param:new:expression", response.rect.center());
            let completed = complete(ui, &response, &mut d.new_expr);
            submit = !completed && enter_for(ui, focused_id.is_some());
            left |= !completed && response.lost_focus();
        });
        row.cell(|ui| {
            let live = if d.new_expr.trim().is_empty() { Ok(String::new()) } else { new_check(app, d, unit) };
            let (value, invalid) = match live {
                Ok(v) => (v, false),
                Err(e) => (e, true),
            };
            ui_kit::text(ui, &value, ui_kit::Text::Value("new_value", invalid)).on_hover_text(&value);
        });
        row.cell(|ui| {
            let response = ui_kit::cell_editor(ui, comment_id, &mut d.new_comment, "Comment", false);
            select_all_on_tab(ui, &response, &d.new_comment);
            crate::scenario::publish_handle("param:new:comment", response.rect.center());
            left |= response.lost_focus();
            submit |= enter_for(ui, focused_id.is_some());
        });
        row.cell(|ui| {
            let response = ui_kit::bin_button(ui, "Delete");
            row_ids.push(response.id);
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
            d.error = Some(error);
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
                d.error = Some(error);
                // Enter stays in the field that was submitted. A click into another cell
                // has to keep that cell, or an invalid name cannot be corrected (#3965).
                if submit && let Some(id) = focused_id {
                    ui.memory_mut(|m| m.request_focus(id));
                }
            }
        }
    }
}

/// A box of the add row entered with Tab selects its text, so typing replaces it (#2919); a click
/// puts the caret where it clicks, as anywhere else. The parameters' own boxes keep the caret at
/// the end, where an edit of the value usually starts.
fn select_all_on_tab(ui: &egui::Ui, resp: &egui::Response, text: &str) {
    if !resp.gained_focus() || ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
        return;
    }
    let n = text.chars().count();
    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), resp.id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
    state.store(ui.ctx(), resp.id);
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
}

fn cache_id() -> egui::Id {
    egui::Id::new("sc_param_completions")
}

/// Keep the completion list (parameters, functions, constants, units) for this revision of the
/// design where expression boxes can reach it. Called once per frame.
pub fn publish(app: &mut SolveApp, ctx: &egui::Context) {
    let rev = app.session.revision;
    if ctx.data(|d| d.get_temp::<Cache>(cache_id())).is_some_and(|c| c.revision == rev) {
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
    ctx.data_mut(|d| d.insert_temp(cache_id(), Cache { revision: rev, all: Arc::new(all) }));
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
    // `cab_side_thickness`, #2757), after the names that start with it.
    let within = |s: &&Suggestion| !exact && s.kind == "parameter" && !starts(s) && s.text.to_ascii_lowercase().contains(&w);
    all.iter().filter(|s| starts(s) && offered(s)).chain(all.iter().filter(|s| within(s) && offered(s))).take(8).cloned().collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_edges_and_corners_resize_their_axes_and_keep_the_size_on_reopening() {
        for layout in ["floating", "docked"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.set", json!({"layout": layout}));
            h.call("ui.resize", json!({"width": 2000, "height": 1400}));
            h.app.run("parameters.add", json!({"name": "width", "expression": "10 mm"})).unwrap();
            h.step(&json!({"start": "parameters.change"})).unwrap();
            h.frames(6);
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            h.step(&json!({"drag": [{"handle": "dialog:title"}, [title.x - 400.0, title.y + 80.0]]})).unwrap();
            h.frames(6);
            let before = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
            let header = crate::scenario::rect_of("dialog:header").unwrap();
            let press = egui::pos2(header.center().x, header.top() + 12.0);
            h.step(&json!({"drag": [[press.x, press.y], [press.x - 30.0, press.y + 40.0]]})).unwrap();
            h.frames(6);
            let moved = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
            assert!((moved.size() - before.size()).length() < 2.0, "the upper title band moves without resizing");
            assert!((moved.min - before.min - egui::vec2(-30.0, 40.0)).length() < 2.0);
            let document = h.app.session.doc.to_json();
            for (corner, delta) in [
                ("bottom_left", egui::vec2(80.0, -40.0)),
                ("top_left", egui::vec2(40.0, 30.0)),
                ("top_right", egui::vec2(-40.0, 20.0)),
                ("bottom_right", egui::vec2(-40.0, -30.0)),
            ] {
                let before = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
                let handle = format!("dialog:resize:PARAMETERS:{corner}");
                let at = crate::scenario::handle_at(&handle).unwrap();
                let hit = crate::scenario::rect_of(&handle).unwrap();
                assert_eq!(hit.size(), egui::vec2(16.0, 16.0));
                assert_eq!(hit.center(), at);
                assert_eq!(at.x, if corner.ends_with("left") { before.left() } else { before.right() });
                assert_eq!(at.y, if corner.starts_with("top") { before.top() } else { before.bottom() });
                // The outside half must be interactive, beyond the card's content clip.
                let press = at + egui::vec2(if corner.ends_with("left") { -6.0 } else { 6.0 }, if corner.starts_with("top") { -6.0 } else { 6.0 });
                h.step(&json!({"drag": [[press.x, press.y], [press.x + delta.x, press.y + delta.y]]})).unwrap();
                h.frames(6);
                let after = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
                assert!(after.width() < before.width() - 20.0, "{layout} {corner}: {before:?} -> {after:?}");
                assert!(after.height() < before.height() - 10.0, "{layout} {corner}: {before:?} -> {after:?}");
                assert!(after.contains(crate::scenario::handle_at("dialog:ok").unwrap()));
            }
            for (edge, delta) in [
                ("left", egui::vec2(40.0, 20.0)),
                ("right", egui::vec2(-40.0, -20.0)),
                ("top", egui::vec2(20.0, 30.0)),
                ("bottom", egui::vec2(-20.0, -30.0)),
            ] {
                let before = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
                let handle = format!("dialog:resize:PARAMETERS:{edge}");
                let at = crate::scenario::handle_at(&handle).unwrap();
                let hit = crate::scenario::rect_of(&handle).unwrap();
                assert_eq!(hit.center(), at);
                let outside = match edge {
                    "left" => egui::vec2(-3.0, 0.0),
                    "right" => egui::vec2(3.0, 0.0),
                    "top" => egui::vec2(0.0, -3.0),
                    _ => egui::vec2(0.0, 3.0),
                };
                if edge == "left" || edge == "right" {
                    assert_eq!(hit.width(), 8.0);
                    assert_eq!(at.x, if edge == "left" { before.left() } else { before.right() });
                } else {
                    assert_eq!(hit.height(), 8.0);
                    assert_eq!(at.y, if edge == "top" { before.top() } else { before.bottom() });
                }
                let press = at + outside;
                h.step(&json!({"drag": [[press.x, press.y], [press.x + delta.x, press.y + delta.y]]})).unwrap();
                h.frames(6);
                let after = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
                if edge == "left" || edge == "right" {
                    assert!((after.width() - before.width() + 40.0).abs() < 2.0, "{layout} {edge}: {before:?} -> {after:?}");
                    assert!((after.height() - before.height()).abs() < 2.0, "{edge} only resizes width");
                    assert!((after.top() - before.top()).abs() < 2.0, "{edge} must not move vertically");
                } else {
                    assert!((after.height() - before.height() + 30.0).abs() < 2.0, "{layout} {edge}: {before:?} -> {after:?}");
                    assert!((after.width() - before.width()).abs() < 2.0, "{edge} only resizes height");
                    assert!((after.left() - before.left()).abs() < 2.0, "{edge} must not move horizontally");
                }
            }
            let size = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap().size();
            h.app.params = None;
            h.frames(2);
            h.step(&json!({"start": "parameters.change"})).unwrap();
            h.frames(6);
            let reopened = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
            assert!((reopened.size() - size).length() < 2.0, "{layout}: {size:?} -> {reopened:?}");
            h.call("ui.resize", json!({"width": 640, "height": 480}));
            h.frames(6);
            let small = crate::scenario::rect_of("dialog:resize:PARAMETERS:bounds").unwrap();
            assert!(small.right() <= 640.0 && small.bottom() <= 480.0, "{layout}: {small:?}");
            assert!(small.contains(crate::scenario::handle_at("dialog:ok").unwrap()));
            assert_eq!(h.app.session.doc.to_json(), document, "resizing is presentation only");
        }
    }

    #[test]
    fn resize_arrows_cover_the_full_inside_and_outside_grab_regions() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 2000, "height": 1400}));
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.frames(6);
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        h.step(&json!({"drag": [[title.x, title.y], [title.x - 400.0, title.y + 80.0]]})).unwrap();
        h.frames(6);
        for (name, x, y) in [
            ("top_left", -1, -1),
            ("top_right", 1, -1),
            ("bottom_left", -1, 1),
            ("bottom_right", 1, 1),
            ("left", -1, 0),
            ("right", 1, 0),
            ("top", 0, -1),
            ("bottom", 0, 1),
        ] {
            let hit = crate::scenario::rect_of(&format!("dialog:resize:PARAMETERS:{name}")).unwrap();
            let cursor = if x == 0 {
                egui::CursorIcon::ResizeVertical
            } else if y == 0 {
                egui::CursorIcon::ResizeHorizontal
            } else if x == y {
                egui::CursorIcon::ResizeNwSe
            } else {
                egui::CursorIcon::ResizeNeSw
            };
            for fraction in [-0.75, 0.0, 0.75] {
                let p = hit.center() + egui::vec2(x as f32 * hit.width() * 0.5 * fraction, y as f32 * hit.height() * 0.5 * fraction);
                h.call("ui.move", json!({"x": p.x, "y": p.y}));
                h.frames(2);
                assert_eq!(h.output.cursor_icon, cursor, "{name} at {p:?}: the entire resize target advertises resizing without a mouse press");
            }
        }
        let header = crate::scenario::rect_of("dialog:header").unwrap();
        h.call("ui.move", json!({"x": header.center().x, "y": header.top() + 12.0}));
        h.frames(2);
        assert_eq!(h.output.cursor_icon, egui::CursorIcon::Grab, "the title keeps its move cursor outside the top resize band");
    }

    #[test]
    fn parameter_names_fit_and_manual_column_widths_survive_reopening() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1900, "height": 1200}));
        let name = "scc_nozzle_contraction_internal_angle";
        h.app.run("parameters.add", json!({"name": name, "expression": "155 deg"})).unwrap();
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.frames(6);
        let at = crate::scenario::handle_at("dialog:resize:PARAMETERS:top_left").unwrap();
        h.step(&json!({"drag": [{"handle": "dialog:resize:PARAMETERS:top_left"}, [at.x - 600.0, at.y]]})).unwrap();
        h.frames(6);
        let name_cell = crate::scenario::rect_of("dialog:column:user:Name").unwrap();
        let t = crate::theme::Tokens::get();
        let measured = h.ctx.fonts_mut(|fonts| fonts.layout_no_wrap(name.into(), crate::fonts::medium(t.font_label), t.form_ink).size().x);
        assert!(name_cell.width() >= measured, "the full name must fit: {measured} in {name_cell:?}");
        let document = h.app.session.doc.to_json();
        for column in ["Comment", "Name", "Expression", "Value", "Unit"] {
            let before = crate::scenario::rect_of(&format!("dialog:column:user:{column}")).unwrap();
            let handle = format!("dialog:column:user:{column}:resize");
            let at = crate::scenario::handle_at(&handle).unwrap();
            for dx in [-3.0, 0.0, 3.0] {
                h.call("ui.move", json!({"x": at.x + dx, "y": at.y}));
                h.frames(2);
                assert_eq!(h.output.cursor_icon, egui::CursorIcon::ResizeHorizontal, "{column}: arrow throughout the divider's grab band");
            }
            h.step(&json!({"drag": [[at.x - 3.0, at.y], [at.x + 45.0, at.y]]})).unwrap();
            h.frames(6);
            let after = crate::scenario::rect_of(&format!("dialog:column:user:{column}")).unwrap();
            assert!((after.width() - before.width() - 48.0).abs() < 2.0, "{column}: {before:?} -> {after:?}");
            let cell = crate::scenario::rect_of(&format!("dialog:table:user:{name}:cell:{column}")).unwrap();
            assert_eq!(cell.x_range(), after.x_range(), "header and editor must align");
        }
        let width = crate::scenario::rect_of("dialog:column:user:Name").unwrap().width();
        h.app.params = None;
        h.frames(2);
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.frames(6);
        assert!((crate::scenario::rect_of("dialog:column:user:Name").unwrap().width() - width).abs() < 1.0);
        h.step(&json!({"click": {"handle": "dialog:column:user:Name:resize"}, "double": true})).unwrap();
        h.frames(6);
        assert!((crate::scenario::rect_of("dialog:column:user:Name").unwrap().width() - name_cell.width()).abs() < 1.0);
        assert_eq!(h.app.session.doc.to_json(), document, "column sizing must not edit parameters");
    }

    #[test]
    fn parameter_column_fit_preference_preserves_proportional_sizing_and_manual_widths() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1900, "height": 1200}));
        h.app
            .run("parameters.add", json!({"name":"secondary_combustion_chamber_nozzle_contraction_internal_angle", "expression":"155 deg"}))
            .unwrap();
        h.step(&json!({"start":"parameters.change"})).unwrap();
        h.frames(6);
        let fitted = crate::scenario::rect_of("dialog:column:user:Name").unwrap().width();
        let document = h.app.session.doc.to_json();
        h.app.preferences.parameters_fit_columns = false;
        h.frames(6);
        let proportional = crate::scenario::rect_of("dialog:column:user:Name").unwrap().width();
        assert!(fitted > proportional + 100.0, "off restores the compact proportional width: {fitted} > {proportional}");
        let prefs: crate::prefs::Prefs = serde_json::from_value(serde_json::to_value(&h.app.preferences).unwrap()).unwrap();
        assert!(!prefs.parameters_fit_columns, "the choice persists");
        h.call("ui.prefs", json!({"open":"Interface"}));
        h.frames(6);
        h.step(&json!({"click":{"handle":"prefs:parameters_fit_columns"}})).unwrap();
        assert!(h.app.preferences.parameters_fit_columns);
        h.call("ui.prefs", json!({"button":"ok"}));
        h.frames(6);
        assert!((crate::scenario::rect_of("dialog:column:user:Name").unwrap().width() - fitted).abs() < 2.0);
        let at = crate::scenario::handle_at("dialog:column:user:Name:resize").unwrap();
        h.step(&json!({"drag":[[at.x,at.y],[at.x+40.0,at.y]]})).unwrap();
        h.frames(6);
        let manual = crate::scenario::rect_of("dialog:column:user:Name").unwrap().width();
        h.app.preferences.parameters_fit_columns = false;
        h.frames(6);
        assert!(
            (crate::scenario::rect_of("dialog:column:user:Name").unwrap().width() - manual).abs() < 2.0,
            "manual widths take precedence in both modes"
        );
        assert_eq!(h.app.session.doc.to_json(), document);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parameters_pop_out_and_dock_with_their_drafts_and_column_widths() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1900, "height": 1200}));
        h.app.run("parameters.add", json!({"name": "width", "expression": "10 mm"})).unwrap();
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.frames(6);
        let at = crate::scenario::handle_at("dialog:column:user:Name:resize").unwrap();
        h.step(&json!({"drag": [[at.x, at.y], [at.x + 50.0, at.y]]})).unwrap();
        h.frames(6);
        let width = crate::scenario::rect_of("dialog:column:user:Name").unwrap().width();
        let d = h.app.params.as_mut().unwrap();
        d.adding = true;
        d.new_name = "unsubmitted".into();
        d.new_expr = "missing + 1 mm".into();
        h.frames(3);
        let document = h.app.session.doc.to_json();
        h.step(&json!({"click": {"handle": "params:window_mode"}})).unwrap();
        h.frames(6);
        assert!(h.app.params.as_ref().unwrap().window.detached);
        assert!(crate::scenario::rect_of("params:native_window").is_some());
        assert!((crate::scenario::rect_of("dialog:column:user:Name").unwrap().width() - width).abs() < 1.0);
        assert_eq!(h.app.params.as_ref().unwrap().new_expr, "missing + 1 mm");
        h.step(&json!({"click": {"handle": "params:window_mode"}})).unwrap();
        h.frames(6);
        assert!(!h.app.params.as_ref().unwrap().window.detached);
        assert!(crate::scenario::rect_of("params:native_window").is_none());
        assert!((crate::scenario::rect_of("dialog:column:user:Name").unwrap().width() - width).abs() < 1.0);
        assert_eq!(h.app.params.as_ref().unwrap().new_name, "unsubmitted");
        assert_eq!(h.app.session.doc.to_json(), document);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dragging_the_parameters_title_outside_the_app_detaches_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1600, "height": 1200}));
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.frames(6);
        let title = crate::scenario::handle_at("params:title").unwrap();
        h.step(&json!({"drag": [[title.x, title.y], [-30.0, title.y]]})).unwrap();
        h.frames(6);
        let d = h.app.params.as_ref().unwrap();
        assert!(d.window.detached);
        assert!(d.window.place.is_some());
        assert!(crate::scenario::rect_of("params:native_window").is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn crossing_each_app_border_detaches_before_the_pointer_leaves_the_app() {
        for edge in ["left", "right", "top", "bottom"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width": 1600, "height": 1200}));
            h.step(&json!({"start": "parameters.change"})).unwrap();
            h.frames(6);
            let title = crate::scenario::handle_at("params:title").unwrap();
            let delta = match edge {
                "left" => egui::vec2(8.0 - title.x, 0.0),
                "right" => egui::vec2(1592.0 - title.x, 0.0),
                "top" => egui::vec2(0.0, 4.0 - title.y),
                _ => egui::vec2(0.0, 1192.0 - title.y),
            };
            let to = title + delta;
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1200.0)).contains(to), "{edge}: {to:?}");
            h.step(&json!({"drag": [[title.x, title.y], [to.x, to.y]]})).unwrap();
            h.frames(6);
            assert!(h.app.params.as_ref().unwrap().window.detached, "{edge} must hand off before pointer capture ends");
            assert!(crate::scenario::rect_of("params:native_window").is_some());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_inner(outer: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_max(outer.min + egui::vec2(0.0, 24.0), outer.max)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_title_drags_snap_back_from_each_side_without_pointer_events() {
        for origin in [egui::pos2(100.0, 50.0), egui::pos2(-2000.0, 50.0)] {
            let parent = egui::Rect::from_min_size(origin, egui::vec2(1600.0, 1000.0));
            let inside = egui::Rect::from_min_size(origin + egui::vec2(250.0, 180.0), egui::vec2(700.0, 500.0));
            for offset in [egui::vec2(-1000.0, 0.0), egui::vec2(1800.0, 0.0), egui::vec2(0.0, -800.0), egui::vec2(0.0, 1200.0)] {
                let outside = inside.translate(offset);
                let mut drag = NativeDockDrag::default();
                assert!(!drag.update(outside, native_inner(outside), parent, 0.0));
                let moved = outside.translate(egui::vec2(10.0, 10.0));
                assert!(!drag.update(moved, native_inner(moved), parent, 0.1));
                assert!(!drag.update(inside, native_inner(inside), parent, 0.2));
                assert!(!drag.update(inside, native_inner(inside), parent, 0.3));
                assert!(drag.update(inside, native_inner(inside), parent, 0.41), "{origin:?} {offset:?}");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn opening_resizing_or_moving_the_main_window_does_not_snap_a_native_panel_back() {
        let parent = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let outer = egui::Rect::from_min_size(egui::pos2(250.0, 180.0), egui::vec2(700.0, 500.0));
        let mut drag = NativeDockDrag::default();
        assert!(!drag.update(outer, native_inner(outer), parent, 0.0));
        let jitter = outer.translate(egui::vec2(2.0, 0.0));
        assert!(!drag.update(jitter, native_inner(jitter), parent, 0.1));
        assert!(!drag.update(jitter, native_inner(jitter), parent, 0.6));
        assert!(!drag.moving, "placement jitter must stop repainting");
        let resized = egui::Rect::from_min_max(jitter.min - egui::vec2(40.0, 0.0), jitter.max);
        assert!(!drag.update(resized, native_inner(resized), parent, 1.0));
        assert!(!drag.update(resized, native_inner(resized), parent, 1.3));
        let moved_parent = parent.translate(egui::vec2(50.0, 50.0));
        assert!(!drag.update(resized, native_inner(resized), moved_parent, 1.5));
        let moved = resized.translate(egui::vec2(50.0, 30.0));
        assert!(!drag.update(moved, native_inner(moved), moved_parent, 2.0));
        assert!(drag.update(moved, native_inner(moved), moved_parent, 2.2), "a later deliberate title drag over the app does snap back");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn snapping_back_keeps_drafts_column_widths_and_the_native_window_size() {
        let mut h = escape_dialog();
        h.call("ui.resize", json!({"width": 1900, "height": 1200}));
        h.frames(6);
        let at = crate::scenario::handle_at("dialog:column:user:Name:resize").unwrap();
        h.step(&json!({"drag": [[at.x, at.y], [at.x + 50.0, at.y]]})).unwrap();
        h.frames(6);
        let width = crate::scenario::rect_of("dialog:column:user:Name").unwrap().width();
        let document = h.app.session.doc.to_json();
        h.step(&json!({"click": {"handle": "params:window_mode"}})).unwrap();
        h.frames(6);
        let parent = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1900.0, 1200.0));
        let outside = egui::Rect::from_min_size(egui::pos2(2100.0, 250.0), egui::vec2(700.0, 524.0));
        let inside = outside.translate(egui::vec2(-1600.0, 0.0));
        let d = h.app.params.as_mut().unwrap();
        d.adding = true;
        d.new_name = "unsubmitted".into();
        d.new_expr = "missing + 1 mm".into();
        snap_native_back(&h.ctx, d, outside, native_inner(outside), parent, 0.0);
        snap_native_back(&h.ctx, d, inside, native_inner(inside), parent, 1.0);
        snap_native_back(&h.ctx, d, inside, native_inner(inside), parent, 1.2);
        assert!(!d.window.detached);
        assert_eq!(d.dock_at, Some(egui::pos2(400.0, 200.0)));
        h.frames(6);
        let d = h.app.params.as_ref().unwrap();
        assert_eq!(d.new_name, "unsubmitted");
        assert_eq!(d.new_expr, "missing + 1 mm");
        assert!(crate::scenario::rect_of("params:native_window").is_none());
        assert!((crate::scenario::rect_of("dialog:column:user:Name").unwrap().width() - width).abs() < 1.0);
        assert_eq!(
            h.ctx.data_mut(|data| data.get_persisted::<egui::Vec2>(egui::Id::new("sc_params_dialog").with("size"))),
            Some(egui::vec2(700.0, 500.0))
        );
        assert_eq!(h.app.session.doc.to_json(), document);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_native_parameters_window_accepts_a_second_monitor_place_once() {
        let app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let ctx = crate::fonts::test_context();
        let mut d = ParamsDialog::new();
        d.window.place = Some([-1600.0, 100.0, 900.0, 700.0]);
        let first = native_window_builder(&app, &ctx, &d);
        assert_eq!(first.position, Some(egui::pos2(-1600.0, 100.0)));
        assert_eq!(first.inner_size, Some(egui::vec2(900.0, 700.0)));
        d.window_placed = true;
        assert!(native_window_builder(&app, &ctx, &d).position.is_none(), "the OS owns subsequent movement");
        d.window_placed = false;
        d.window.place = Some([f32::NAN, 0.0, 900.0, 700.0]);
        assert!(native_window_builder(&app, &ctx, &d).position.is_none(), "invalid saved positions are ignored");
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
        assert!(h.app.prefs_window.open && h.app.params.is_some(), "Escape cancels only shortcut capture");
        assert_eq!(escape_state(&h), before);
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
        for (name, e) in [("cab_side_thickness", "18 mm"), ("sides", "4")] {
            let _ = app.run("parameters.add", json!({"name": name, "expression": e}));
        }
        publish(&mut app, &ctx);
        let texts = |w: &str| suggestions(&ctx, w).into_iter().map(|s| s.text).collect::<Vec<_>>();
        assert_eq!(texts("side"), ["sides", "cab_side_thickness"]);
        assert_eq!(texts("Thick"), ["cab_side_thickness"]);
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
        assert!(h.app.params.as_ref().unwrap().error.is_none());
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
        assert_eq!(h.app.params.as_ref().unwrap().error, None, "and nothing wrong is said");
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
}
