//! The design tree over the view while a dialog holds the left column (#6710). With Dialogs ›
//! In the left column (`dialog_column`) the browser and the History make way for the dialog, so
//! the view's top-left corner shows a folded row with the design's name instead. A click opens
//! the tree over the view on a translucent card: the browser's Origin, Bodies and Construction
//! rows, then the History's feature rows with its folders when the History is at the left (at
//! the bottom its strip stays on screen). They are the browser's and the History's own rows, so
//! a click picks for the dialog's active input exactly as there: planes, axes and bodies, and
//! features where the input takes them (`timeline::pick_feature_into_dialog`). Another click on
//! the header folds it, and it goes with the dialog. Its card is one of `shell::covered_rects`, so
//! navigation and picking in the view stop at its edge and nowhere else.

use egui::{Rect, pos2};

use crate::SolveApp;
use crate::shell::SHELL_GAP;
use crate::theme::Tokens;
use crate::ui_kit::{Container, DockEdges, Surface};

/// Show the tree (folded or open) while a dialog holds the left column; forget it otherwise.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    app.shell.dialog_tree = None;
    if !crate::dialog_column::holds(app) {
        app.shell.dialog_tree_open = false;
        return;
    }
    let Some(viewport) = app.viewport.rect.filter(|r| r.is_finite() && r.is_positive()) else { return };
    let t = Tokens::get();
    let content = crate::shell::content_rect(app, viewport);
    let panel = Container::new(ctx, Surface::Panel).joined(DockEdges::default());
    // A card away from the column and the ribbon in either layout; while a sketch is edited its
    // status line keeps the corner.
    let inset = (SHELL_GAP - panel.gap()).max(0.0);
    let status = if app.session.active_sketch.is_some() { t.space_24 + t.space_12 } else { 0.0 };
    let min = pos2(content.left() + inset, content.top() + inset + status);
    let bottom = app
        .viewport
        .layout
        .as_ref()
        .map(|l| l.navigation)
        .filter(|n| n.is_finite() && n.is_positive())
        .map_or(content.bottom(), |n| n.top().min(content.bottom()))
        - SHELL_GAP;
    // OK and Cancel in the view's corner (#6709) keep the top right on a narrow view.
    let right = crate::view_confirm::rect(app).map_or(content.right(), |pair| pair.left().min(content.right()));
    let width = crate::browser::browser_width(app.preferences.browser_width).min(right - SHELL_GAP - min.x);
    let header = crate::ui_kit::TREE_ROW_HEIGHT + 2.0 * t.space_4;
    if !(width.is_finite() && width >= t.space_24 * 4.0 && bottom - min.y >= header) {
        return;
    }
    let bounds = Rect::from_min_max(min, pos2(min.x + width, bottom));
    let frame = panel.frame(ctx, t.panel.gamma_multiply(crate::browser::BROWSER_FLOAT_ALPHA), crate::frame::Side::Left).inner_margin(t.space_4);
    let inner = (bounds.size() - frame.total_margin().sum()).max(egui::Vec2::ZERO);
    let history = app.ui.show_timeline && app.preferences.timeline_left && !crate::render_workspace::active(app);
    let card = panel.area(egui::Id::new("sc_dialog_tree")).fixed_pos(min).order(egui::Order::Middle).show(ctx, |ui| {
        panel.show_frame(ui, frame, bounds, |ui| {
            ui.set_width(inner.x);
            ui.set_max_height(inner.y);
            ui.spacing_mut().item_spacing.y = 0.0;
            let open = app.shell.dialog_tree_open;
            let name = app.session.doc.name.clone();
            let row = crate::browser::Row { fold: Some(open), icon: "component", label: &name, name: true, ..Default::default() };
            let r = crate::browser::draw_row(ui, ui.id().with("sc_dialog_tree_header"), &row);
            if let Some(rect) = r.rect {
                crate::scenario::publish_handle("dialog_tree:header", rect.center());
            }
            if let Some(resp) = r.resp {
                resp.on_hover_text(crate::i18n::tr(if open { "ui.collapse" } else { "ui.expand" }));
            }
            if r.fold || r.clicked {
                app.shell.dialog_tree_open = !open;
                ui.ctx().request_repaint();
            }
            if !open {
                return;
            }
            t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top(), t.border);
            ui.add_space(t.space_3);
            egui::ScrollArea::vertical()
                .id_salt("sc_dialog_tree_rows")
                .min_scrolled_height(0.0)
                .max_height((inner.y - ui.min_rect().height()).max(0.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    crate::browser::tree_rows(app, ui, 0);
                    if history {
                        ui.add_space(t.space_4);
                        crate::timeline_left::vertical_features(app, ui, true);
                    }
                });
        })
    });
    let rect = card.response.rect;
    app.shell.dialog_tree = Some(rect);
    crate::scenario::publish_rect("dialog_tree:outer", rect);
}
