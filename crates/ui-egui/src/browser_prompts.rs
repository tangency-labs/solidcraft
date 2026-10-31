//! The two prompts the Browser opens over the viewport: Move/Copy of an occurrence (offsets and
//! turns, pending until Capture Position) and the Capture Position question asked before a
//! command or a save while components are moved but not captured.

use serde_json::json;

use crate::SolveApp;
use crate::browser::{answer_capture, apply_occurrence_move, occurrence_origin, panel_pos, revert_occurrence_dialog};
use crate::ui_kit;

/// Move/Copy of an occurrence: offsets and a rotation about Z, pending until Capture Position.
pub(crate) fn occurrence_panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut m) = app.tree.occurrence_move.clone() else { return };
    if !m.path.is_empty()
        && let Err(error) = app.session.occurrence_placement(&m.path)
    {
        app.tree.occurrence_move = None;
        app.set_status(error.to_string(), true);
        return;
    }
    if m.baseline.is_none() {
        m.had_pending = app.session.pending_moves.contains_key(&m.occurrence);
        m.baseline = app
            .session
            .pending_moves
            .get(&m.occurrence)
            .copied()
            .or_else(|| app.session.doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.transform));
    }
    let ctx = ui.ctx().clone();
    if m.pivot.is_none() {
        m.pivot = if m.path.is_empty() {
            occurrence_origin(app, m.occurrence)
        } else {
            app.session.occurrence_placement(&m.path).ok().map(|p| solidcraft_engine::doc::apply_point(&p.world, solidcraft_engine::geom::Vec3::ZERO))
        };
    }
    let name = app.session.doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.name.clone()).unwrap_or_default();
    let before = (m.translate, m.angles);
    let shared = !m.path.is_empty() && app.session.occurrence_scope_count(&m.path).is_ok_and(|n| n > 1);
    let title = crate::i18n::format("browser.move_title", &[("name", &name)]);
    let at = panel_pos(app);
    let (units, decimals) = (app.session.doc.units.clone(), app.preferences.length_decimals);
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_occ_move"), "move", &title)
        .placed(egui::Align2::LEFT_TOP, at)
        .fields()
        .cards(2.0)
        .actions("Capture All Positions", "Cancel", Some("Revert All Positions"))
        .show(&ctx, true, |ui| {
            if shared {
                ui_kit::text(
                    ui,
                    crate::i18n::tr_label("This child is shared. Position changes apply to every copy of its parent."),
                    ui_kit::Text::Note("shared"),
                );
            }
            for (k, axis) in ["X distance", "Y distance", "Z distance"].into_iter().enumerate() {
                if let Some(v) = m.translate.get_mut(k) {
                    let response = ui_kit::row(ui, crate::i18n::tr_label(axis), |ui| ui_kit::length_field(ui, v, &units, decimals));
                    crate::scenario::publish_handle(&format!("occurrence-move:distance:{k}"), response.rect.center());
                }
            }
            for (k, axis) in ["X angle", "Y angle", "Z angle"].into_iter().enumerate() {
                if let Some(v) = m.angles.get_mut(k) {
                    let response = ui_kit::row(ui, crate::i18n::tr_label(axis), |ui| ui_kit::angle_field(ui, v));
                    crate::scenario::publish_handle(&format!("occurrence-move:angle:{k}"), response.rect.center());
                }
            }
        });
    if (m.translate, m.angles) != before {
        apply_occurrence_move(app, &m);
    }
    match action {
        ui_kit::Action::Ok => {
            let _ = app.run("component.capture_position", json!({}));
            app.tree.occurrence_move = None;
        }
        ui_kit::Action::Apply => {
            let _ = app.run("component.revert_position", json!({}));
            app.tree.occurrence_move = None;
        }
        ui_kit::Action::Cancel => {
            revert_occurrence_dialog(app, &m);
            app.tree.occurrence_move = None;
        }
        ui_kit::Action::None if closed => {
            revert_occurrence_dialog(app, &m);
            app.tree.occurrence_move = None;
        }
        ui_kit::Action::None => app.tree.occurrence_move = Some(m),
    }
}

/// The Capture Position question: capture, revert or cancel the waiting command (or save).
pub(crate) fn capture_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    if app.tree.capture_prompt.is_none() {
        return;
    }
    let n = app.session.pending_moves.len();
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_capture_prompt"), "capture", crate::i18n::tr_label("Capture Position"))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -80.0))
        .actions("Capture Position", "Cancel", Some("Revert"))
        .show(ctx, true, |ui| {
            let question = if n == 1 {
                crate::i18n::tr("browser.capture_one").to_string()
            } else {
                crate::i18n::format("browser.capture_many", &[("count", &n.to_string())])
            };
            ui_kit::text(ui, &question, ui_kit::Text::Report("question", false));
            ui_kit::text(ui, crate::i18n::tr("browser.capture_note"), ui_kit::Text::Note("capture"));
        });
    let answer = match action {
        ui_kit::Action::Ok => Some("capture"),
        ui_kit::Action::Apply => Some("revert"),
        ui_kit::Action::Cancel => Some("cancel"),
        ui_kit::Action::None if closed => Some("cancel"),
        ui_kit::Action::None => None,
    };
    if let Some(answer) = answer {
        let _ = answer_capture(app, answer);
    }
}
