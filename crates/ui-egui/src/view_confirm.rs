//! OK and Cancel in the view's corner (Preferences › Interface, #6709). While a dialog is open,
//! a check and a cross at the top right of the view do what the dialog's own OK and Cancel do,
//! and the check greys out whenever the dialog's OK does. While a sketch is open without a
//! dialog, they are Finish Sketch and Cancel Sketch, as on the ribbon. The axis view moves down
//! below the pair while it is shown (`viewport_layout::Layout::with_content`), and the pair is
//! one of the shell's covered rects, so a click on it never picks or navigates.

use egui::{Rect, Sense, vec2};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit::{Action, Container, Surface};

/// Each target's side: large enough to hit without aiming.
const TARGET: f32 = 40.0;

/// What the pair answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answers {
    /// The open dialog's OK and Cancel (also inside a sketch).
    Dialog,
    /// Finish Sketch and Cancel Sketch.
    Sketch,
}

fn answers(app: &SolveApp) -> Option<Answers> {
    if !app.preferences.view_confirm_corner || app.home.open || crate::render_workspace::active(app) {
        return None;
    }
    match &app.dialog {
        Some(dialog) => crate::dialogs::corner_confirms(dialog).then_some(Answers::Dialog),
        None => app.session.active_sketch.is_some().then_some(Answers::Sketch),
    }
}

/// The pair's size while it is shown, else zero: the view's layout keeps the corner for it.
pub(crate) fn size(app: &SolveApp) -> egui::Vec2 {
    if answers(app).is_none() {
        return egui::Vec2::ZERO;
    }
    let t = Tokens::get();
    vec2(TARGET * 2.0 + t.space_4 * 3.0, TARGET + t.space_4 * 2.0)
}

/// Where the pair is while it is shown.
pub(crate) fn rect(app: &SolveApp) -> Option<Rect> {
    answers(app)?;
    app.viewport.layout.as_ref().map(|layout| layout.confirm).filter(|r| r.is_finite() && r.is_positive())
}

/// One of the pair: what it answers, where, its icon, the action its tooltip names and whether
/// it can be clicked.
struct Choice {
    action: Action,
    key: &'static str,
    icon: &'static str,
    label: &'static str,
    enabled: bool,
}

/// Draws the pair and answers a click. A dialog's answer waits for `dialogs::show` in this frame
/// (`ViewportState::corner`), which acts on it as on its own OK and Cancel; a sketch's runs at
/// once, the way the ribbon's Finish Sketch and Cancel do (a refusal shows as their errors do).
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let (Some(answers), Some(rect)) = (answers(app), rect(app)) else { return };
    let choices = match answers {
        Answers::Dialog => {
            let (ok, cancel) = app.dialog.as_ref().map_or(("OK", "Cancel"), |dialog| crate::dialogs::action_labels(&dialog.kind));
            [
                Choice {
                    action: Action::Ok,
                    key: "view:confirm:ok",
                    icon: "finish",
                    label: if ok.is_empty() { "OK" } else { ok },
                    enabled: app.viewport.corner_ok,
                },
                Choice { action: Action::Cancel, key: "view:confirm:cancel", icon: "close", label: cancel, enabled: true },
            ]
        }
        Answers::Sketch => {
            let command = |id: &str| solidcraft_engine::find_command(id).map(|spec| (spec.label, spec.info(&app.session).enabled));
            let (finish, finish_enabled) = command("sketch.finish").unwrap_or(("Finish Sketch", false));
            let (cancel, cancel_enabled) = command("sketch.cancel").unwrap_or(("Cancel Sketch", false));
            [
                Choice { action: Action::Ok, key: "view:confirm:ok", icon: "finish", label: finish, enabled: finish_enabled },
                Choice { action: Action::Cancel, key: "view:confirm:cancel", icon: "sketch_cancel", label: cancel, enabled: cancel_enabled },
            ]
        }
    };
    let t = Tokens::get();
    let container = Container::new(ctx, Surface::Navigation);
    let frame = container.frame(ctx, t.overlay, crate::frame::Side::Right);
    let mut clicked = None;
    container.area(egui::Id::unique("sc_view_confirm")).order(egui::Order::Middle).fixed_pos(rect.min).fade_in(false).show(ctx, |ui| {
        let (outer, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
        Container::paint_frame(ui.painter(), frame, outer.shrink(frame.stroke.width));
        // The check, then the cross in the very corner, where a close sits on a title bar.
        for (index, choice) in choices.iter().enumerate() {
            let at = Rect::from_min_size(outer.min + vec2(t.space_4 + index as f32 * (TARGET + t.space_4), t.space_4), vec2(TARGET, TARGET));
            let response = ui.interact(at, ui.scope_id().with(choice.key), if choice.enabled { Sense::click() } else { Sense::hover() });
            if choice.enabled && response.hovered() {
                ui.painter().rect_filled(at, t.radius_sm, t.hover);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            crate::icons::paint_icon(ui.painter(), at.shrink(t.space_6), choice.icon, if choice.enabled { 1.0 } else { 0.35 });
            let label = crate::i18n::tr_label(choice.label);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, choice.enabled, label));
            crate::scenario::publish_target(choice.key, at);
            if response.on_hover_text(label).clicked() && choice.enabled {
                clicked = Some(choice.action);
            }
        }
    });
    crate::scenario::publish_rect("view:confirm", rect);
    crate::scenario::publish_count("view:confirm:ok:enabled", f64::from(choices.first().is_some_and(|choice| choice.enabled)));
    match (answers, clicked) {
        (Answers::Dialog, Some(action)) => app.viewport.corner = Some(action),
        (Answers::Sketch, Some(Action::Ok)) => app.start_pinned("sketch.finish", None),
        (Answers::Sketch, Some(Action::Cancel)) => app.start_pinned("sketch.cancel", None),
        _ => {}
    }
}
