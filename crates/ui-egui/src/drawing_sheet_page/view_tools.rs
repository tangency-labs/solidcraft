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
    /// The box round a dimension's text on paper, as of a revision: looked up once while the
    /// pointer stays over that dimension with the Dimension tool in hand ([`over_text`]).
    text_box: Option<(u64, u64, Vec<Aabb2>)>,
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
    // With the Dimension tool in hand a placed dimension or note still drags, as in a sketch; a
    // view does not.
    let tool = drawing_workspace::placing_dimension(app);
    !matches!(app.viewport.nav, Some(crate::viewport::NavMode::Pan | crate::viewport::NavMode::Zoom))
        && !drawing_workspace::placing_view(app)
        && !drawing_workspace::placing_note(app)
        && ((!tool && app.drawing_workspace.page.view_tools.drag.is_some())
            || dragging_dimension()
            || (response.dragged_by(PointerButton::Primary)
                && ((!tool && (origin(app, response, at, objects).is_some() || selected_origin(app, response, at, objects).is_some()))
                    || dimension_origin(app, response, at, objects)
                        .is_some_and(|(id, pointer)| !tool || in_tool(app, id, pointer, HOVER_BAND / at.scale)))))
}

/// With the Dimension tool in hand, whether a press at `point` on dimension or note `id` is on
/// what drags there: a note, or the dimension's text as the pointer's hover last found it
/// ([`over_text`] keeps it). Elsewhere on a dimension the press is the tool's, or navigation's.
fn in_tool(app: &SolveApp, id: u64, point: Vec2, band: f64) -> bool {
    let texts = app.drawing_workspace.page.view_tools.text_box.as_ref().filter(|(of, at, _)| *of == id && *at == app.session.revision);
    app.session.drawing(drawing_scene::CMD).is_ok_and(|drawing| drawing.notes.by_id(id).is_some())
        || texts.is_some_and(|(.., texts)| texts.iter().any(|text| text.inflated(band).contains(point)))
}

/// Whether something on the sheet is being dragged by the drag sketch dimensions use: a
/// dimension's text, a note, or a selection of things together.
pub(super) fn dragging_dimension() -> bool {
    crate::dim_view::dragged().is_some_and(|item| !matches!(item, crate::dim_view::Dragged::Sketch(_)))
}

/// A selected object of the sheet a drag starts on, and where on paper.
fn selected_origin(app: &SolveApp, response: &egui::Response, at: OnScreen, objects: &[SheetObject]) -> Option<(u64, Vec2)> {
    let screen = response.ctx.input(|input| input.pointer.press_origin())?;
    if crate::shell::covered(app, screen) {
        return None;
    }
    let pointer = at.paper(screen);
    let hit = drawing_scene::hit(objects, pointer, HOVER_BAND / at.scale)?;
    let chosen = app.session.selection.iter().any(|s| matches!(s, Sel::Drawing { object, .. } if *object == hit.object));
    chosen.then_some((hit.object, pointer))
}

/// The dimension or note a drag starts on, and where on paper: a press on a dimension's text or
/// lines, or on a note.
fn dimension_origin(app: &SolveApp, response: &egui::Response, at: OnScreen, objects: &[SheetObject]) -> Option<(u64, Vec2)> {
    let screen = response.ctx.input(|input| input.pointer.press_origin())?;
    if crate::shell::covered(app, screen) {
        return None;
    }
    let pointer = at.paper(screen);
    let hit =
        drawing_scene::hit(objects, pointer, HOVER_BAND / at.scale).filter(|hit| matches!(hit.kind, ObjectKind::Dimension | ObjectKind::Note))?;
    Some((hit.object, pointer))
}

/// Whether `point` on paper is on dimension `id`'s text, within `band`: with the Dimension tool in
/// hand the text is the dimension's, as in a sketch, and the rest of its box (its lines, and the
/// edge it measures between them) stays the tool's to pick.
pub(super) fn over_text(app: &mut SolveApp, id: u64, point: Vec2, band: f64) -> bool {
    let revision = app.session.revision;
    let kept = app.drawing_workspace.page.view_tools.text_box.take().filter(|(of, at, _)| *of == id && *at == revision);
    let (.., texts) = kept.unwrap_or_else(|| {
        // Each of its texts' boxes: a chain has one for each member.
        let read = app.run("drawing.dimension.inspect", json!({"dimension": encode(id)})).ok();
        let boxes = read.as_ref().and_then(|read| read["layout"]["text_boxes"].as_array()).into_iter().flatten();
        let texts = boxes
            .filter_map(|corners| {
                let corners: Vec<Vec2> =
                    corners.as_array()?.iter().filter_map(|p| serde_json::from_value::<[f64; 2]>(p.clone()).ok().map(Vec2::from)).collect();
                (corners.len() == 4).then(|| corners.iter().fold(Aabb2::EMPTY, |all, p| all.union(&Aabb2 { min: *p, max: *p })))
            })
            .collect();
        (id, revision, texts)
    });
    let over = texts.iter().any(|text| text.inflated(band).contains(point));
    app.drawing_workspace.page.view_tools.text_box = Some((id, revision, texts));
    over
}

/// Where dimension `id`'s text's centre is on paper now, as inspect lays it out.
fn text_centre(app: &mut SolveApp, id: u64) -> Option<Vec2> {
    let read = app.run("drawing.dimension.inspect", json!({"dimension": encode(id)})).ok()?;
    let corners: Vec<Vec2> = read["layout"]["text_boxes"]
        .get(0)?
        .as_array()?
        .iter()
        .filter_map(|p| serde_json::from_value::<[f64; 2]>(p.clone()).ok().map(Vec2::from))
        .collect();
    (corners.len() == 4).then(|| corners.iter().fold(Vec2::ZERO, |sum, p| sum + *p) * 0.25)
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
    pub interactive: bool,
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
        if !self.interactive {
            app.drawing_workspace.page.view_tools.drag = None;
            return;
        }
        let mut state = std::mem::take(&mut app.drawing_workspace.page.view_tools);
        let mods = ui.input(|input| input.modifiers);
        let (add, toggle) =
            crate::tool_modifiers::selection_click(crate::tool_modifiers::Held::from_modifiers(mods), app.preferences.selection_shift_add_only);
        // The Dimension tool leaves a placed dimension or note draggable, as the sketch's does.
        let tool = placing && drawing_workspace::placing_dimension(app);
        let navigating = matches!(app.viewport.nav, Some(crate::viewport::NavMode::Pan | crate::viewport::NavMode::Zoom));
        if navigating {
            state.drag = None;
            if dragging_dimension() {
                crate::dim_view::drag_end();
            }
        }
        if placing && !tool {
            state.drag = None;
        } else if !navigating
            && !tool
            && self.response.drag_started_by(PointerButton::Primary)
            && let Some((_, pointer)) = selected_origin(app, self.response, self.at, self.objects)
            && let Some(items) = Some(self.group(app)).filter(|items| items.len() > 1)
        {
            // A drag from one of several selected things moves them all together by the pointer's
            // way, as a sketch drags its selected geometry: one undo step.
            crate::dim_view::begin_group_drag(app, items, pointer);
        } else if !navigating
            && self.response.drag_started_by(PointerButton::Primary)
            && let Some((dimension, pointer)) = dimension_origin(app, self.response, self.at, self.objects)
            // With the tool in hand only its text is the dimension's: the rest is the tool's to pick.
            && (!tool || self.drawing.notes.by_id(dimension).is_some() || over_text(app, dimension, pointer, HOVER_BAND / self.at.scale))
        {
            // A drag from a dimension moves its text, and one from a note the note, by the drag that
            // moves a sketch dimension's text.
            if !selected(app, self.drawing.id).contains(&dimension) {
                crate::viewport::select(app, Some(pick(self.drawing.id, dimension)), false, false);
            }
            if let Some(note) = self.drawing.notes.by_id(dimension) {
                // A note keeps where it sits: the drag moves that, by the same drag.
                crate::dim_view::begin_drag(app, crate::dim_view::Dragged::Note(dimension), Vec2::new(note.at[0], note.at[1]), pointer);
            } else if let Some(text) = text_centre(app, dimension) {
                crate::dim_view::begin_drag(app, crate::dim_view::Dragged::Drawing(dimension), text, pointer);
            }
        } else if tool {
            // Nothing else of the sheet's own with the tool in hand: its clicks are picks.
            state.drag = None;
        } else if !navigating
            && self.response.drag_started_by(PointerButton::Primary)
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

        if dragging_dimension() {
            if let Some(pointer) = self.response.interact_pointer_pos()
                && (self.response.dragged_by(PointerButton::Primary) || self.response.drag_stopped())
            {
                crate::dim_view::drag_to(app, self.at.paper(pointer));
            }
            if self.response.drag_stopped() || !ui.input(|input| input.pointer.primary_down()) {
                crate::dim_view::drag_end();
            }
        } else if let Some(drag) = state.drag {
            if drag.revision != app.session.revision || !selected(app, self.drawing.id).contains(&drag.view) {
                state.drag = None;
            } else if let Some(pointer) = self.response.interact_pointer_pos() {
                let alt = ui.input(|input| input.modifiers.alt);
                let cursor = drag.from + (self.at.paper(pointer) - drag.pointer);
                let target = if alt { cursor } else { self.snap(drag.view, cursor) };
                self.preview(app, drag, target, cursor);
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

    /// The selection as the shared drag moves it: each selected view, dimension text, note,
    /// balloon and table from where it is now. A dimension, note or balloon on a selected view
    /// is left to the view, which takes it along.
    fn group(&self, app: &mut SolveApp) -> Vec<(crate::dim_view::Dragged, Vec2)> {
        use crate::dim_view::Dragged;
        let chosen = selected(app, self.drawing.id);
        let on_chosen = |view: Option<u64>| view.is_some_and(|view| chosen.contains(&view));
        let mut items = Vec::new();
        for &id in &chosen {
            if self.drawing.view(id).is_some() {
                if let Ok(centre) = self.drawing.view_centre(id) {
                    items.push((Dragged::View(id), centre));
                }
            } else if let Some(dimension) = self.drawing.dimension(id) {
                if !on_chosen(Some(dimension.view))
                    && let Some(text) = text_centre(app, id)
                {
                    items.push((Dragged::Drawing(id), text));
                }
            } else if let Some(note) = self.drawing.notes.by_id(id) {
                if !on_chosen(note.anchor.view()) {
                    items.push((Dragged::Note(id), Vec2::new(note.at[0], note.at[1])));
                }
            } else if let Some(balloon) = self.drawing.balloons.by_id(id) {
                if !on_chosen(Some(balloon.view)) {
                    items.push((Dragged::Balloon(id), Vec2::new(balloon.at[0], balloon.at[1])));
                }
            } else if let Some(table) = self.drawing.tables.by_id(id) {
                // A table is placed by its outline's top left: where it says, or, anchored to a
                // corner, where the sheet draws it.
                let at = match table.place {
                    solidcraft_engine::doc::drawing::table::Place::At([x, y]) => Some(Vec2::new(x, y)),
                    _ => self.objects.iter().find(|object| object.object == id).map(|o| Vec2::new(o.bounds_mm.min.x, o.bounds_mm.max.y)),
                };
                if let Some(at) = at {
                    items.push((Dragged::Table(id), at));
                }
            }
        }
        // Views first: what moves with a view is placed after it.
        items.sort_by_key(|(item, _)| !matches!(item, Dragged::View(_)));
        items
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

    fn preview(&self, app: &SolveApp, drag: Drag, to: Vec2, cursor: Vec2) {
        let mut draft = self.drawing.clone();
        let Ok(moved) = draft.move_view(drag.view, to, false) else { return };
        let t = Tokens::get();
        let line = Stroke::new(1.0, t.accent);
        // Each moving view's own lines where it goes, in the accent colour, as a sketch shows what a
        // drag moves: no box round it. A detail's callout on its parent stays where the parent is.
        let page = &app.drawing_workspace.page;
        let ppp = self.painter.ctx().pixels_per_point();
        for object in self.objects.iter().filter(|object| object.kind == ObjectKind::View && moved.contains(&object.object)) {
            let (Ok(before), Ok(after)) = (self.drawing.view_centre(object.object), draft.view_centre(object.object)) else { continue };
            let offset = after - before;
            let shifted = OnScreen { target: self.at.target - offset, ..self.at };
            if let Some(marks) = &page.marks {
                marks.paint_moving(
                    self.painter,
                    &shifted,
                    shifted.visible(self.painter.clip_rect()),
                    ppp,
                    &page.pictures,
                    &[object.object],
                    t.accent,
                );
            }
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
        let text = tr!("ui.drawing_view_selection.offset", x = &format!("{:+.1}", offset.x), y = &format!("{:+.1}", offset.y));
        self.painter.text(destination + egui::vec2(t.space_8, t.space_8), Align2::LEFT_TOP, text, egui::FontId::proportional(t.font_label), t.accent);
    }
}
