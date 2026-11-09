//! Thin presentation adapter: persistent effects and activation order belong to the session.
use crate::{
    SolveApp,
    ui_kit::{self, StatusAction, StatusItem, StatusKind},
};
use solidcraft_engine::status_items::{StatusId, StatusItem as Effect, StatusSetting};

fn allowed(app: &SolveApp) -> Vec<StatusId> {
    let mut ids = Vec::new();
    if app.preferences.viewport_analysis_status {
        ids.extend([StatusId::SurfaceAnalysis, StatusId::Continuity]);
    }
    if app.preferences.viewport_section_status {
        ids.push(StatusId::Section);
    }
    ids
}

fn eligible(app: &SolveApp) -> Vec<StatusId> {
    app.session.status_items().iter().filter(|effect| presentation(app, effect).is_some()).map(|effect| effect.id).collect()
}

pub(crate) fn enabled(app: &SolveApp) -> bool {
    app.preferences.viewport_analysis_status || app.preferences.viewport_section_status
}

/// Canonical plane coordinates: reversing the clipping side never reverses the offset.
/// Axis planes use world coordinates; other planes use signed distance to the origin.
pub(crate) fn section_coordinates(origin: [f64; 3], normal: [f64; 3]) -> Option<(Option<&'static str>, f64, [f64; 3])> {
    let scale = normal.iter().fold(0.0_f64, |a, b| a.max(b.abs()));
    if !normal.iter().chain(origin.iter()).all(|v| v.is_finite()) || !scale.is_finite() || scale == 0.0 {
        return None;
    }
    let scaled = normal.map(|n| n / scale);
    let length = scaled.iter().map(|n| n * n).sum::<f64>().sqrt();
    let mut n = scaled.map(|n| n / length);
    if n.iter().find(|v| v.abs() > 1e-8).is_some_and(|v| *v < 0.0) {
        n = n.map(|v| -v);
    }
    for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
        if n.iter().enumerate().all(|(j, v)| (*v - if i == j { 1.0 } else { 0.0 }).abs() < 1e-8) {
            return Some((Some(axis), *origin.get(i)?, n));
        }
    }
    let distance = origin.iter().zip(n).map(|(o, n)| o * n).sum::<f64>();
    distance.is_finite().then_some((None, distance, n))
}

pub(crate) fn section_setting(app: &SolveApp, origin: [f64; 3], normal: [f64; 3], hatch: bool) -> Option<String> {
    let (axis, offset, _) = section_coordinates(origin, normal)?;
    let distance = crate::prefs::show_mm(app, offset, 1);
    let mut text = match axis {
        Some(axis) => format!("{axis} {distance}"),
        None => solidcraft_ui_text::format("ui.viewport_status_section_tilted", &[("offset", &distance)]),
    };
    if hatch {
        text = solidcraft_ui_text::format("ui.viewport_status_section_hatch", &[("setting", &text)]);
    }
    Some(text)
}

pub(crate) fn identity(effect: &Effect) -> String {
    format!("{:?}:{}", effect.id, effect.command)
}

pub(crate) fn direction(value: [f64; 3]) -> String {
    for (axis, vector) in [("X", [1.0, 0.0, 0.0]), ("Y", [0.0, 1.0, 0.0]), ("Z", [0.0, 0.0, 1.0])] {
        if value.iter().zip(vector).all(|(a, b)| (*a - b).abs() < 1e-8) {
            return axis.into();
        }
        if value.iter().zip(vector).all(|(a, b)| (*a + b).abs() < 1e-8) {
            return format!("−{axis}");
        }
    }
    let [x, y, z] = value;
    format!("({x:.2}, {y:.2}, {z:.2})")
}

pub(crate) fn presentation(app: &SolveApp, effect: &Effect) -> Option<StatusItem> {
    if !allowed(app).contains(&effect.id) {
        return None;
    }
    let command = solidcraft_engine::find_command(&effect.command)?;
    let setting = match effect.setting {
        StatusSetting::Zebra { stripes } => solidcraft_ui_text::format("ui.viewport_status_stripes", &[("count", &stripes.to_string())]),
        StatusSetting::Draft { pull, angle_deg } => format!("{} · {angle_deg}°", direction(pull)),
        StatusSetting::Environment => String::new(),
        StatusSetting::Accessibility { direction: from } => solidcraft_ui_text::format("ui.viewport_status_from", &[("direction", &direction(from))]),
        StatusSetting::Curvature { radius_mm } => format!("R {}", crate::prefs::show_mm(app, radius_mm, 1)),
        StatusSetting::Continuity { edge_count } => solidcraft_ui_text::format("ui.viewport_status_edges", &[("count", &edge_count.to_string())]),
        StatusSetting::Section { origin_mm, normal, hatch } => section_setting(app, origin_mm, normal, hatch)?,
    };
    Some(StatusItem {
        id: identity(effect),
        kind: StatusKind::Mode,
        name: crate::toolbar::persistent_button_name(app, command),
        setting,
        has_key: if app.preferences.viewport_status_details { effect.has_details() } else { effect.has_key },
        editable: if app.preferences.viewport_status_details { effect.has_details() } else { effect.id == StatusId::Section },
    })
}

fn clear(app: &mut SolveApp, effect: &Effect) {
    if let Err(error) = app.run(&effect.clear.command, serde_json::json!({"clear": effect.clear.params.clear})) {
        app.set_status(error, true);
    }
}

pub(crate) fn has_effect(app: &SolveApp) -> bool {
    app.session.newest_status_item(&eligible(app)).is_some()
}

/// Called only after existing Escape owners have yielded. A repeat cannot cascade clears.
pub(crate) fn escape(app: &mut SolveApp, repeat: bool) -> bool {
    if repeat {
        return app.viewport_status.open_key.is_some() || has_effect(app);
    }
    if let Some(open) = app.viewport_status.open_key.clone()
        && app.session.status_items().iter().any(|effect| presentation(app, effect).is_some_and(|item| item.has_key && item.id == open))
    {
        app.viewport_status.close_details();
        return true;
    }
    if let Some(effect) = app.session.newest_status_item(&eligible(app)) {
        clear(app, &effect);
        return true;
    }
    false
}

/// Reconcile before shortcut routing so a vanished native popup cannot own Escape invisibly.
pub(crate) fn reconcile(app: &mut SolveApp, ctx: &egui::Context) {
    app.viewport_status.reconcile_session(ctx, app.session.uid);
    // A visible native field must retain focus through a completed input frame before
    // consuming the request; hidden Area measurement is not a focus acknowledgement.
    if ui_kit::details_field_owns_focus(ctx) || ui_kit::details_control_owns_focus(ctx) {
        app.viewport_status.focus_field = None;
    }
    if app.viewport_status.live_details != app.preferences.viewport_status_details && app.viewport_status.open_key.is_some() {
        ui_kit::dismiss_details_popups(ctx);
        if ui_kit::details_field_owns_focus(ctx) || ui_kit::details_control_owns_focus(ctx) {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        app.viewport_status.open_key = None;
        app.viewport_status.focus_field = None;
    }
    app.viewport_status.live_details = app.preferences.viewport_status_details;
    let mut items: Vec<_> = app.session.status_items().iter().filter_map(|effect| presentation(app, effect)).collect();
    items.extend(crate::dialogs_insert_drawing::integrity(app).as_ref().map(svg_item));
    if app.viewport_status.open_key.as_ref().is_some_and(|open| !items.iter().any(|item| item.has_key && &item.id == open)) {
        ui_kit::dismiss_details_popups(ctx);
        if ui_kit::details_field_owns_focus(ctx) || ui_kit::details_control_owns_focus(ctx) {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        app.viewport_status.open_key = None;
        app.viewport_status.focus_field = None;
    }
    let members =
        if app.preferences.viewport_status_summary && items.len() >= 3 { items.into_iter().map(|item| item.id).collect() } else { Vec::new() };
    app.viewport_status.reconcile_summary(ctx, members);
    // The popup is a separate foreground host: the badge's Background layer cannot
    // shield it. Tools deliberately remain eligible for their existing row actions.
    // The SVG import panel owns this row's integrity item, so its own dialog does not dismiss it.
    let svg_dialog = matches!(app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::InsertDrawing(p)) if p.command.ends_with("svg"));
    let higher_owner = app.prefs_window.shell.open
        || (app.dialog.is_some() && !svg_dialog)
        || app.keymap.open
        || app.ui.palette_open
        || ctx.memory(|memory| memory.top_modal_layer().is_some());
    app.viewport_status.suspend_summary(ctx, higher_owner);
}

/// SVG import shows one status item for all of its integrity facts, whatever the status preferences.
fn svg_item(integrity: &crate::dialogs_insert_drawing::Integrity) -> ui_kit::StatusItem {
    ui_kit::StatusItem {
        id: crate::dialogs_insert_drawing::INTEGRITY_ID.into(),
        kind: if integrity.attention { ui_kit::StatusKind::Attention } else { ui_kit::StatusKind::Result },
        name: solidcraft_ui_text::tr("ui.svg_integrity").into(),
        setting: integrity.setting.clone(),
        has_key: true,
        editable: false,
    }
}

fn svg_card(ui: &mut egui::Ui, integrity: &crate::dialogs_insert_drawing::Integrity) {
    use crate::ui_kit::AssistantTone as Tone;
    let t = solidcraft_theme::Tokens::get();
    ui.set_min_width(t.space_24 * 12.0);
    ui.set_max_width(t.space_24 * 12.0);
    ui.label(egui::RichText::new(solidcraft_ui_text::tr("ui.svg_integrity")).font(crate::ui_kit::type_face::semibold(t.font_label)));
    for (label, value, tone) in &integrity.rows {
        let (glyph, ink) = match tone {
            Tone::Success => ("✓", t.form_success),
            Tone::Warning => ("!", t.warning),
            Tone::Error => ("×", t.form_error),
            Tone::Working => ("…", t.selected_ink),
            Tone::Neutral => ("—", t.form_dim),
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(glyph).color(ink));
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(value).monospace().size(t.font_small));
            });
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(solidcraft_ui_text::tr("ui.svg_integrity_view"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(&integrity.view).monospace().size(t.font_small));
        });
    });
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context, viewport: egui::Rect) {
    reconcile(app, ctx);
    let svg = crate::dialogs_insert_drawing::integrity(app);
    if !enabled(app) && svg.is_none() {
        app.viewport_status.open_key = None;
        return;
    }
    let effects = if enabled(app) { app.session.status_items() } else { Vec::new() };
    if !app.preferences.viewport_analysis_status && !app.preferences.viewport_section_status && svg.is_none() {
        app.viewport_status.open_key = None;
    }
    let mut items: Vec<_> = effects.iter().filter_map(|effect| presentation(app, effect)).collect();
    items.extend(svg.as_ref().map(svg_item));
    if items.is_empty() {
        app.viewport_status = Default::default();
        return;
    }
    let content = crate::shell::content_rect(app, viewport);
    if !content.is_finite() || !content.is_positive() {
        return;
    }
    let mut free = crate::shell::workspace_content(app, content);
    for card in &app.shell.cards {
        free.min.y = free.top().max(card.bottom()).min(free.bottom());
    }
    if let Some(layout) = &app.viewport.layout {
        free.max.x = free.right().min(layout.axis_view.left()).max(free.left());
    }
    if svg.is_some() {
        // The import panel floats at the right edge; the status row centres on what is left.
        let t = solidcraft_theme::Tokens::get();
        free.max.x = free.right().min(free.right() - t.svg_panel_width - t.space_16 * 2.0 - t.space_4).max(free.left());
    }
    if !free.is_finite() || !free.is_positive() {
        return;
    }
    let modern = app.preferences.viewport_status_details;
    let descriptions: Vec<_> = effects
        .iter()
        .filter_map(|effect| {
            presentation(app, effect).map(|item| {
                let details =
                    if modern { crate::viewport_details::description(app, effect) } else { crate::viewport_details::legacy_description(app) };
                (item.id, details)
            })
        })
        .collect();
    if let Some(field) = app.viewport_status.focus_field.as_deref()
        && !descriptions.iter().any(|(owner, details)| {
            Some(owner.as_str()) == app.viewport_status.open_key.as_deref()
                && details.settings.iter().any(|setting| setting.id == field && setting.enabled)
        })
    {
        app.viewport_status.focus_field = None;
    }
    let focus = app.viewport_status.focus_field.clone();
    let session_uid = app.session.uid;
    let mut edits = Vec::new();
    let key_contents = |ui: &mut egui::Ui, id: &str| {
        if let (Some(integrity), true) = (svg.as_ref(), id == crate::dialogs_insert_drawing::INTEGRITY_ID) {
            svg_card(ui, integrity);
        } else if let Some((_, description)) = descriptions.iter().find(|(owner, _)| owner == id) {
            ui.push_id(session_uid, |ui| {
                edits.extend(
                    ui_kit::details(ui, id, description, free.height() * 0.6, focus.as_deref()).into_iter().map(|event| (id.to_owned(), event)),
                );
            });
        }
    };
    let newest = app.session.newest_status_item(&eligible(app)).map(|effect| identity(&effect));
    let actions = if app.preferences.viewport_status_summary && items.len() >= 3 {
        ui_kit::viewport_status_summary(ctx, free, &items, newest.as_deref().unwrap_or_default(), &mut app.viewport_status, key_contents)
    } else {
        ui_kit::viewport_status_row(ctx, free, &items, &mut app.viewport_status, key_contents)
    };
    dispatch(app, actions);
    for (owner, event) in edits {
        crate::viewport_details::dispatch(app, ctx, &owner, event);
    }
}

/// Resolve requests against current represented effects, never a menu-open snapshot.
pub(crate) fn dispatch(app: &mut SolveApp, actions: Vec<StatusAction>) {
    for action in actions {
        if matches!(&action, StatusAction::Clear(id) if id == crate::dialogs_insert_drawing::INTEGRITY_ID) {
            crate::dialogs_insert_drawing::hide_integrity(app);
            app.viewport_status.open_key = None;
            continue;
        }
        let effects: Vec<_> = app.session.status_items().into_iter().filter(|effect| presentation(app, effect).is_some()).collect();
        match action {
            StatusAction::ClearAll => {
                for effect in effects {
                    clear(app, &effect);
                }
                app.viewport_status.open_key = None;
            }
            StatusAction::Clear(id) => {
                if let Some(effect) = effects.iter().find(|effect| identity(effect) == id) {
                    clear(app, effect);
                    app.viewport_status.open_key = None;
                }
            }
            StatusAction::Edit(id) => {
                if app.preferences.viewport_status_details {
                    if let Some(effect) = effects.iter().find(|effect| identity(effect) == id) {
                        app.viewport_status.focus_field = Some(crate::viewport_details::primary(effect).into());
                        app.viewport_status.open_key = Some(id);
                    }
                } else if effects.iter().any(|effect| effect.id == StatusId::Section && identity(effect) == id) {
                    crate::dialogs::edit_section(app);
                }
            }
        }
    }
}
