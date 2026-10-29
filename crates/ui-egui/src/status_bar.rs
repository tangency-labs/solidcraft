//! The status bar (Preferences › Interface › Status bar, #7243; off by default): a thin bar
//! along the window's bottom edge, below the History strip, with what is being edited, the open
//! sketch's state in that state's colour, and the design's units. A click on the units opens
//! the Units menu, whose rows run Change Design Units (`document.units`, one undo step). While
//! the bar is on, the sketch's state is not also painted in the view's top-left corner.
//!
//! The bar is window chrome like the application bar: full width and flat in either workspace
//! layout. It is allocated before the viewport, so the view and everything anchored to its
//! bottom (History, the navigation bar, toasts) sit above it, and it is one of the shell's
//! covered rects.

use egui::{Color32, FontId, Rect, Sense, pos2, vec2};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit::{Container, Edge, Surface};

/// The bar's height (logical pixels).
pub(crate) const HEIGHT: f32 = 24.0;

/// The bar is drawn: the preference is on, outside the compact browser embed (which has no
/// application chrome).
pub(crate) fn shown(app: &SolveApp) -> bool {
    app.preferences.status_bar && !app.embedded
}

/// The open sketch's state as its solve reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SketchState {
    /// Nothing drawn in it yet.
    Empty,
    /// Degrees of freedom remain.
    Under(usize),
    Full,
    /// The constraints conflict (the solve failed).
    Conflict,
}

impl SketchState {
    pub(crate) fn of(ss: &solidcraft_engine::doc::SolvedSketch) -> SketchState {
        if !ss.report.ok() {
            SketchState::Conflict
        } else if !crate::sketch_palette::has_geometry(&ss.sketch) {
            SketchState::Empty
        } else if ss.report.dof == 0 {
            SketchState::Full
        } else {
            SketchState::Under(ss.report.dof)
        }
    }

    pub(crate) fn text(self) -> String {
        match self {
            SketchState::Conflict => crate::i18n::tr("ui.constraints_conflict").to_string(),
            SketchState::Empty => crate::i18n::tr("ui.no_geometry_yet").to_string(),
            SketchState::Full => crate::i18n::tr("ui.fully_constrained").to_string(),
            SketchState::Under(dof) => crate::i18n::format(
                if dof == 1 { "status.under_constrained_one" } else { "status.under_constrained" },
                &[("count", &dof.to_string())],
            ),
        }
    }

    /// The colour the sketch's curves take in this state, in the window's theme: the bar is
    /// window chrome, not part of the view.
    pub(crate) fn colour(self, t: &Tokens) -> Color32 {
        match self {
            SketchState::Conflict => t.error,
            SketchState::Empty => t.text_dim,
            SketchState::Full => t.sketch_fixed,
            SketchState::Under(_) => t.sketch,
        }
    }
}

/// What the bar shows now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Contents {
    /// "Editing Sketch1": the feature whose dialog edits it, else the sketch whose plane is being
    /// redefined, else the open sketch; nothing while idle or while a new feature is set up.
    pub editing: Option<String>,
    /// The open sketch's state.
    pub sketch: Option<SketchState>,
    /// The design's units (`mm`, `cm`, `m`, `in`, `ft`).
    pub units: String,
}

pub(crate) fn contents(app: &SolveApp) -> Contents {
    let st = app.session.world_state();
    let sketch = app.session.active_sketch.and_then(|id| st.sketch(id));
    let edited = app
        .dialog
        .as_ref()
        .and_then(|d| d.editing)
        .map(|(id, _)| id)
        .or(app.tree.redefine)
        .or(app.session.active_sketch)
        .and_then(|id| app.session.doc.feature(id).map(|f| f.name.clone()).or_else(|| st.sketch(id).map(|ss| ss.name.clone())));
    Contents {
        editing: edited.map(|name| crate::i18n::format("status.editing", &[("name", &crate::i18n::tr_name(&name))])),
        sketch: sketch.map(SketchState::of),
        units: app.session.doc.units.clone(),
    }
}

/// Draw the bar along the window's bottom edge. Call it before the panels and the viewport take
/// the rest of the window: the bar then spans the whole width and sits below History.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    app.shell.status_bar = None;
    if !shown(app) {
        return;
    }
    let t = Tokens::get();
    let contents = contents(app);
    let host = Container::new(ui.ctx(), Surface::ApplicationBar);
    let response = host
        .edge("sc_status_bar", Edge::Bottom)
        .exact_size(HEIGHT)
        .show_separator_line(false)
        .frame(host.edge_frame(t.toolbar))
        .show(ui, |ui| paint(app, ui, &contents))
        .response;
    app.shell.status_bar = Some(response.rect);
    crate::scenario::publish_target("status:bar", response.rect);
}

/// The items, right-aligned from the window's right edge: units, the sketch's state, what is
/// being edited. The left end stays free (for next-step hints, #2325). An item that would not
/// fit is left out, the leftmost first.
fn paint(app: &mut SolveApp, ui: &mut egui::Ui, contents: &Contents) {
    let t = Tokens::get();
    let r = ui.max_rect();
    let painter = ui.painter().clone();
    painter.hline(r.x_range(), r.top() + 0.5, t.primary_border(t.border));
    let font = FontId::proportional(t.font_body);
    let left = r.left() + t.space_8;
    let mut right = r.right() - t.space_6;

    // The units: a flat button with the kit's dropdown chevron.
    let units = painter.layout_no_wrap(contents.units.clone(), font.clone(), t.text);
    let chevron = t.space_10;
    let width = t.space_6 + units.size().x + t.space_4 + chevron + t.space_6;
    let button = Rect::from_min_max(pos2(right - width, r.top() + t.space_3), pos2(right, r.bottom() - t.space_3));
    let response = ui.interact(button, egui::Id::new("sc_status_units"), Sense::click());
    let open = app.viewport.context_menu.is_some() && app.menu.target == crate::context_menu::Target::Units;
    if response.hovered() || open {
        painter.rect_filled(button, t.control_radius, t.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let units_at = pos2(button.left() + t.space_6, button.center().y - units.size().y / 2.0);
    painter.galley(units_at, units, t.text);
    let chevron_rect = Rect::from_center_size(pos2(button.right() - t.space_6 - chevron / 2.0, button.center().y), vec2(chevron, chevron));
    crate::ui_kit::dropdown_indicator(&painter, chevron_rect, t.text_dim);
    let label = crate::i18n::format("browser.units", &[("units", &contents.units)]);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    crate::scenario::publish_target("status:units", button);
    if response.on_hover_text(crate::i18n::tr("status.units_tip")).clicked() {
        // The menu opens above the bar, its right edge on the button's.
        crate::context_menu::toggle_for(app, button, button.right_top(), crate::context_menu::Target::Units);
    }
    right = button.left() - t.space_6;

    // The sketch's state, after a dot in its colour; then what is being edited.
    let mut items: Vec<(&str, String, Color32, Option<Color32>)> = Vec::new();
    if let Some(state) = contents.sketch {
        let colour = state.colour(&t);
        items.push(("status:sketch", state.text(), colour, Some(colour)));
    }
    if let Some(editing) = &contents.editing {
        items.push(("status:editing", editing.clone(), t.text_dim, None));
    }
    for (handle, text, colour, dot) in items {
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), colour);
        let dot_width = if dot.is_some() { t.space_8 + t.space_6 } else { 0.0 };
        let width = dot_width + galley.size().x;
        // A rule between this item and the one to its right.
        let rule = right - t.space_6;
        if rule - t.space_6 - width < left {
            break;
        }
        t.secondary_vline(&painter, rule, (r.top() + t.space_6)..=(r.bottom() - t.space_6), t.border);
        let item = Rect::from_min_max(pos2(rule - t.space_6 - width, r.top()), pos2(rule - t.space_6, r.bottom()));
        if let Some(dot) = dot {
            painter.circle_filled(pos2(item.left() + t.space_4, item.center().y), t.space_3 + 0.5, dot);
        }
        painter.galley(pos2(item.left() + dot_width, item.center().y - galley.size().y / 2.0), galley, colour);
        ui.interact(item, egui::Id::new(handle), Sense::hover()).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
        crate::scenario::publish_target(handle, item);
        right = item.left();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sketch_states_take_their_texts_and_the_window_themes_sketch_colours() {
        let t = Tokens::light();
        assert_eq!(SketchState::Full.text(), "Fully constrained");
        assert_eq!(SketchState::Conflict.text(), "Constraints conflict");
        assert_eq!(SketchState::Empty.text(), "No geometry yet");
        assert_eq!(SketchState::Under(1).text(), "Under constrained (1 degree of freedom)");
        assert_eq!(SketchState::Under(4).text(), "Under constrained (4 degrees of freedom)");
        assert_eq!(SketchState::Under(4).colour(&t), t.sketch);
        assert_eq!(SketchState::Full.colour(&t), t.sketch_fixed);
        assert_eq!(SketchState::Conflict.colour(&t), t.error);
        // Each state's text reads on the bar in both themes.
        for t in [Tokens::light(), Tokens::dark()] {
            for state in [SketchState::Under(2), SketchState::Full, SketchState::Conflict, SketchState::Empty] {
                let ratio = crate::theme::contrast_ratio(state.colour(&t), t.toolbar);
                assert!(ratio >= 3.0, "{state:?} on the bar: contrast {ratio:.2}");
            }
        }
    }
}
