//! Selection and move interaction on a drawing sheet. What is selected is the session's
//! selection (`Sel::Drawing`), picked with the same click, Shift/Ctrl and box rules as the model,
//! so Delete, Escape and Select All work on it through the shared paths.
use egui::{Align2, PointerButton, Rect, Stroke, pos2};
use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::{
    Sel,
    doc::{drawing::Drawing, ids::encode},
    drawing_scene::{self, SheetObject},
    geom::{Aabb2, paper::ObjectKind},
};
use solidcraft_theme::Tokens;

use super::{HOVER_BAND, OnScreen};
use crate::{SolveApp, drawing_workspace};

#[derive(Default)]
pub(super) struct State {
    drag: Option<Drag>,
}

#[derive(Clone, Copy)]
struct Drag {
    view: u64,
    from: Vec2,
    pointer: Vec2,
    revision: u64,
}

fn view_at(objects: &[SheetObject], point: Vec2, at: OnScreen) -> Option<u64> {
    drawing_scene::hit(objects, point, HOVER_BAND / at.scale).filter(|hit| hit.kind == ObjectKind::View).map(|hit| hit.object)
}

fn origin(app: &SolveApp, response: &egui::Response, at: OnScreen, objects: &[SheetObject]) -> Option<(u64, Vec2)> {
    let screen = response.ctx.input(|input| input.pointer.press_origin())?;
    if crate::shell::covered(app, screen) {
        return None;
    }
    let pointer = at.paper(screen);
    Some((view_at(objects, pointer, at)?, pointer))
}

pub(super) fn owns_drag(app: &SolveApp, response: &egui::Response, at: OnScreen, objects: &[SheetObject]) -> bool {
    !drawing_workspace::placing_view(app)
        && !drawing_workspace::placing_note(app)
        && !drawing_workspace::placing_dimension(app)
        && (app.drawing_workspace.page.view_tools.drag.is_some()
            || (response.dragged_by(PointerButton::Primary) && origin(app, response, at, objects).is_some()))
}

/// Cancel a view's drag; the selection stays (Escape clears it next).
pub(super) fn clear(app: &mut SolveApp) -> bool {
    app.drawing_workspace.page.view_tools.drag.take().is_some()
}

/// The objects of drawing `drawing` selected now.
pub(super) fn selected(app: &SolveApp, drawing: u64) -> Vec<u64> {
    app.session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::Drawing { drawing: d, object } if *d == drawing => Some(*object),
            _ => None,
        })
        .collect()
}

fn pick(drawing: u64, object: u64) -> Sel {
    Sel::Drawing { drawing, object }
}

/// The model's box selection on the sheet: a primary drag from empty paper, a window to the right
/// and a crossing box to the left (or the lasso, when it is on), adding with Shift or Ctrl.
#[allow(clippy::too_many_arguments)]
pub(super) fn box_select(
    app: &mut SolveApp,
    ui: &egui::Ui,
    painter: &egui::Painter,
    response: &egui::Response,
    at: OnScreen,
    drawing: &Drawing,
    objects: &[SheetObject],
    canvas: Rect,
    free: bool,
) {
    if !free || app.drawing_workspace.page.view_tools.drag.is_some() {
        app.viewport.boxsel = None;
        return;
    }
    let origin = ui.input(|input| input.pointer.press_origin());
    if response.dragged_by(PointerButton::Primary)
        && let (Some(a), Some(b)) = (origin, response.interact_pointer_pos())
        && canvas.contains(a)
        && !crate::shell::covered(app, a)
        && drawing_scene::hit(objects, at.paper(a), HOVER_BAND / at.scale).is_none()
    {
        let lasso = app.ui.lasso_select;
        app.viewport.boxsel = Some(match app.viewport.boxsel.take() {
            Some(mut drawn) if lasso && drawn.is_lasso() => {
                drawn.extend(b);
                drawn
            }
            _ if lasso => crate::selection::BoxSel::lasso(&[a, b]),
            _ => crate::selection::BoxSel::from_drag(a, b),
        });
    }
    if let Some(bx) = &app.viewport.boxsel {
        crate::viewport::paint_box_selection(painter, bx, false);
    }
    if response.drag_stopped()
        && let Some(bx) = app.viewport.boxsel.take()
    {
        let mods = ui.input(|input| input.modifiers);
        let (add, _) =
            crate::tool_modifiers::selection_click(crate::tool_modifiers::Held::from_modifiers(mods), app.preferences.selection_shift_add_only);
        let items: Vec<Sel> = objects
            .iter()
            .filter(|object| drawing.has_object(object.object))
            .filter(|object| {
                let r = at.rect(&object.boundary_mm.unwrap_or(object.bounds_mm));
                bx.takes(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()])
            })
            .map(|object| pick(drawing.id, object.object))
            .collect();
        if let Err(error) = app.run("select.set", json!({"items": items, "add": add})) {
            app.set_status(error, true);
        }
    } else if !ui.input(|input| input.pointer.primary_down()) {
        app.viewport.boxsel = None;
    }
}

pub(super) struct Canvas<'a> {
    pub at: OnScreen,
    pub painter: &'a egui::Painter,
    pub response: &'a egui::Response,
    pub drawing: &'a Drawing,
    pub sheet: u64,
    pub objects: &'a [SheetObject],
    pub free: Rect,
}

impl Canvas<'_> {
    pub(super) fn show(self, app: &mut SolveApp, ui: &mut egui::Ui, placing: bool) {
        let mut state = std::mem::take(&mut app.drawing_workspace.page.view_tools);
        let mods = ui.input(|input| input.modifiers);
        let (add, toggle) =
            crate::tool_modifiers::selection_click(crate::tool_modifiers::Held::from_modifiers(mods), app.preferences.selection_shift_add_only);
        if placing {
            state.drag = None;
        } else if self.response.drag_started_by(PointerButton::Primary)
            && let Some((view, pointer)) = origin(app, self.response, self.at, self.objects)
            && let Ok(from) = self.drawing.view_centre(view)
        {
            // A drag from a view moves it, as a drag from selected geometry moves it in the model.
            if !selected(app, self.drawing.id).contains(&view) {
                crate::viewport::select(app, Some(pick(self.drawing.id, view)), false, false);
            }
            state.drag = Some(Drag { view, from, pointer, revision: app.session.revision });
        } else if self.response.clicked() {
            // The model's click: replace, or add and toggle with Shift and Ctrl; nothing clears.
            let hover = self.response.hover_pos().filter(|point| !crate::shell::covered(app, *point));
            let hit = hover
                .and_then(|point| drawing_scene::hit(self.objects, self.at.paper(point), HOVER_BAND / self.at.scale))
                .filter(|hit| self.drawing.has_object(hit.object));
            crate::viewport::select(app, hit.map(|hit| pick(self.drawing.id, hit.object)), add, toggle);
        }

        if let Some(drag) = state.drag {
            if drag.revision != app.session.revision || !selected(app, self.drawing.id).contains(&drag.view) {
                state.drag = None;
            } else if let Some(pointer) = self.response.interact_pointer_pos() {
                let alt = ui.input(|input| input.modifiers.alt);
                let cursor = drag.from + (self.at.paper(pointer) - drag.pointer);
                let target = if alt { cursor } else { self.snap(drag.view, cursor) };
                self.preview(drag, target, cursor);
                if self.response.drag_stopped_by(PointerButton::Primary) {
                    state.drag = None;
                    // One undo step, undone with Ctrl+Z as any other.
                    if let Err(error) = app.run("drawing.view.move", json!({"view":encode(drag.view),"to_mm":[target.x,target.y]})) {
                        app.set_status(error, true);
                    }
                }
            } else if !ui.input(|input| input.pointer.primary_down()) {
                state.drag = None;
            }
        }
        app.drawing_workspace.page.view_tools = state;
    }

    fn snap(&self, selected: u64, point: Vec2) -> Vec2 {
        let limit = f64::from(Tokens::get().space_8) / self.at.scale;
        let mut best = [limit; 2];
        let mut target = [point.x, point.y];
        for view in self.drawing.views.iter().filter(|view| view.sheet == self.sheet && view.id != selected) {
            let Ok(centre) = self.drawing.view_centre(view.id) else { continue };
            for (axis, (value, cursor)) in [(centre.x, point.x), (centre.y, point.y)].into_iter().enumerate() {
                let distance = (value - cursor).abs();
                if distance < best[axis] {
                    best[axis] = distance;
                    target[axis] = value;
                }
            }
        }
        Vec2::new(target[0], target[1])
    }

    fn preview(&self, drag: Drag, to: Vec2, cursor: Vec2) {
        let mut draft = self.drawing.clone();
        let Ok(moved) = draft.move_view(drag.view, to, false) else { return };
        let t = Tokens::get();
        let line = Stroke::new(1.0, t.accent);
        for object in self.objects.iter().filter(|object| object.kind == ObjectKind::View && moved.contains(&object.object)) {
            let (Ok(before), Ok(after)) = (self.drawing.view_centre(object.object), draft.view_centre(object.object)) else { continue };
            let offset = after - before;
            let bounds = object.boundary_mm.unwrap_or(object.bounds_mm);
            let r = self.at.rect(&Aabb2 { min: bounds.min + offset, max: bounds.max + offset });
            self.painter.rect_stroke(r, t.radius_sm, line, egui::StrokeKind::Outside);
        }
        let Ok(after) = draft.view_centre(drag.view) else { return };
        let from = self.at.screen(drag.from);
        let destination = self.at.screen(after);
        let guide = Stroke::new(1.0, t.accent.gamma_multiply(0.45));
        if to.x != cursor.x && after.x == to.x {
            self.painter.vline(destination.x, self.free.y_range(), guide);
        }
        if to.y != cursor.y && after.y == to.y {
            self.painter.hline(self.free.x_range(), destination.y, guide);
        }
        let corners = [from, pos2(destination.x, from.y), destination];
        self.painter.extend(egui::Shape::dashed_line(&corners, line, t.space_4, t.space_4));
        let offset = after - drag.from;
        let text = solidcraft_ui_text::format(
            "ui.drawing_view_selection.offset",
            &[("x", &format!("{:+.1}", offset.x)), ("y", &format!("{:+.1}", offset.y))],
        );
        self.painter.text(destination + egui::vec2(t.space_8, t.space_8), Align2::LEFT_TOP, text, egui::FontId::proportional(t.font_label), t.accent);
    }
}
