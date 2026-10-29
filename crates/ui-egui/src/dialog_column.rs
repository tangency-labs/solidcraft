//! Command dialogs in the left column (#6707). With Preferences › Interface › Dialogs set to
//! *In the left column* (`dialog_place: "left_column"`), an open command dialog takes the place
//! of the browser and of the History at the left, whichever of them the column shows, at the
//! column's width and never narrower than the dialog's own card. The column is only not drawn
//! while the dialog holds it: no width, fold or collapsed state changes, so OK, Cancel or Esc
//! bring it back exactly as it was.
//!
//! - History at the bottom: the dialog takes the browser's place and the strip stays.
//! - No column at the left (the browser hidden, or translucent over the view, and the History
//!   not at the left): the dialog docks at the left edge in a column of its own width.
//! - The Assistant docked on the left keeps the outside edge: the column starts beside it, as
//!   the browser does.
//! - Compact layout with the History at the left: the History's edge panel stays allocated
//!   (empty) under the dialog, so the view keeps its size and the model doesn't move.
//! - The Sketch Palette keeps the right edge; Delete confirmations stay where they are.

use egui::{Rect, pos2};
use serde::{Deserialize, Serialize};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit::{Container, Surface};

/// Where command dialogs open (Preferences › Interface › Dialogs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogPlace {
    /// Docked at the view's edge: the right, or the left while the Sketch Palette holds the right.
    #[default]
    Edge,
    /// In the left column, in place of the browser and the History there.
    LeftColumn,
}

/// Does the open dialog hold the left column this frame?
pub(crate) fn holds(app: &SolveApp) -> bool {
    app.preferences.dialog_place == DialogPlace::LeftColumn && app.dialog.as_ref().is_some_and(crate::dialogs::docks)
}

/// The History is drawn at the left this frame (beside or under the browser).
fn left_history(app: &SolveApp) -> bool {
    app.ui.show_timeline && app.preferences.timeline_left && !crate::render_workspace::active(app)
}

/// The column a dialog of `width` holds, laid out in `available` (the space the browser and a
/// left History are laid out in this frame): from the left edge, beside the Assistant, to the
/// right of what the column shows, at least `width` wide, and from the top down to a bottom
/// History card. It mirrors where `timeline_left::left_panel` and `browser::browser` put
/// their cards.
pub(crate) fn rect(app: &SolveApp, available: Rect, width: f32) -> Rect {
    let t = Tokens::get();
    let panel = Container::for_layout(app.ui.layout, Surface::Panel);
    let gap = panel.gap();
    let left = (available.left() + crate::agent_chat::left_inset(app, available) + gap).min(available.right());
    let history = left_history(app);
    let stacked = history && app.preferences.timeline_under_browser;
    let browser_width = crate::browser::browser_width(app.preferences.browser_width);
    let mut right = left;
    if stacked {
        // Browser and History share one column of the browser's width.
        right = left + browser_width;
    } else if history {
        right = left + crate::timeline_left::strip_width(app);
    }
    let translucent = panel.floating() && app.preferences.browser_float;
    if app.ui.show_browser && !stacked && !translucent {
        let from = if history { right + gap } else { left };
        let collapsed = history && app.preferences.timeline_left_browser_collapsed;
        right = from + if collapsed { crate::browser::collapsed_width() } else { browser_width };
    }
    let bottom = app.shell.history.filter(|_| app.ui.show_timeline && !app.preferences.timeline_left).map_or(available.bottom(), |h| h.top()) - gap;
    let width = if width.is_finite() { width.max(t.space_24) } else { t.form_card_width };
    let right = right.max(left + width).min(available.right() - gap).max(left);
    Rect::from_min_max(pos2(left, available.top()), pos2(right, bottom.max(available.top())))
}

/// The column the dialog `d` holds, or `None` when it docks at the view's edge. The column
/// laid out with the browser this frame, or (a dialog opened after that, from a key in the view)
/// the same column worked out now from the space the browser had; the next frame then leaves
/// the browser out.
pub(crate) fn place(app: &SolveApp, ctx: &egui::Context, d: &crate::dialogs::Dialog) -> Option<Rect> {
    if app.preferences.dialog_place != DialogPlace::LeftColumn || !crate::dialogs::docks(d) {
        return None;
    }
    let column = app.shell.dialog_column.or_else(|| {
        ctx.request_repaint();
        app.shell.left_space.map(|available| rect(app, available, crate::dialogs::card_width(d)))
    });
    column.filter(|r| r.is_finite() && r.is_positive())
}

/// Lay out this frame's column before the browser and the History are drawn: they skip
/// themselves while a dialog holds it, and the view's content area starts beside it.
pub(crate) fn layout(app: &mut SolveApp, available: Rect) {
    app.shell.left_space = Some(available);
    app.shell.dialog_column = app.dialog.as_ref().filter(|_| holds(app)).map(|d| rect(app, available, crate::dialogs::card_width(d)));
}

/// The bounds the dialog's card is placed in: the column's left and top, the space's right edge,
/// and the column's bottom plus the gap the card keeps to it.
pub(crate) fn bounds(app: &SolveApp, column: Rect, viewport: Rect) -> Rect {
    let gap = Container::for_layout(app.ui.layout, Surface::Feature).gap();
    Rect::from_min_max(column.min, pos2(viewport.right().max(column.right()), column.bottom() + gap))
}
