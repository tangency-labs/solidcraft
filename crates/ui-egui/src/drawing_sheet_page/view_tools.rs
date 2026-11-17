//! Selection and move interaction for saved drawing views.
use egui::{Align2, PointerButton, Rect, Stroke, pos2};
use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::{
    doc::{drawing::Drawing, ids::encode},
    drawing_scene::{self, SheetObject},
    geom::{Aabb2, paper::ObjectKind},
};
use solidcraft_theme::Tokens;

use super::{HOVER_BAND, OnScreen};
use crate::{
    SolveApp, drawing_workspace,
    ui_kit::{self, BackingTooltipExt as _, Container, toast},
};

#[derive(Default)]
pub(super) struct State {
    selected: Option<u64>,
    drag: Option<Drag>,
    delete: Option<u64>,
    undo: Option<(u64, f64, &'static str)>,
}

#[derive(Clone, Copy)]
struct Drag {
    view: u64,
    from: Vec2,
    pointer: Vec2,
    revision: u64,
}

fn edge(objects: &[SheetObject], point: Vec2, at: OnScreen) -> Option<u64> {
    drawing_scene::hit(objects, point, HOVER_BAND / at.scale).filter(|hit| hit.kind == ObjectKind::View && hit.on_edge).map(|hit| hit.object)
}

fn origin(app: &SolveApp, response: &egui::Response, at: OnScreen, objects: &[SheetObject]) -> Option<(u64, Vec2)> {
    let screen = response.ctx.input(|input| input.pointer.press_origin())?;
    if crate::shell::covered(app, screen) {
        return None;
    }
    let pointer = at.paper(screen);
    Some((edge(objects, pointer, at)?, pointer))
}

pub(super) fn owns_drag(app: &SolveApp, response: &egui::Response, at: OnScreen, objects: &[SheetObject]) -> bool {
    !drawing_workspace::placing_view(app)
        && (app.drawing_workspace.page.view_tools.drag.is_some()
            || (response.dragged_by(PointerButton::Primary) && origin(app, response, at, objects).is_some()))
}

pub(super) fn clear(app: &mut SolveApp) -> bool {
    let state = &mut app.drawing_workspace.page.view_tools;
    let selected = state.selected.take().is_some();
    let dragging = state.drag.take().is_some();
    let deleting = state.delete.take().is_some();
    selected || dragging || deleting
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
        state.selected = state.selected.filter(|id| self.drawing.view(*id).is_some_and(|view| view.sheet == self.sheet));
        if placing {
            state.drag = None;
        } else if self.response.drag_started_by(PointerButton::Primary)
            && let Some((view, pointer)) = origin(app, self.response, self.at, self.objects)
            && let Ok(from) = self.drawing.view_centre(view)
        {
            state.selected = Some(view);
            state.drag = Some(Drag { view, from, pointer, revision: app.session.revision });
        } else if self.response.clicked() && state.delete.is_none() {
            let hover = self.response.hover_pos().filter(|point| !crate::shell::covered(app, *point));
            state.selected = hover.and_then(|point| edge(self.objects, self.at.paper(point), self.at));
        }

        if let Some(drag) = state.drag {
            if drag.revision != app.session.revision || state.selected != Some(drag.view) {
                state.drag = None;
            } else if let Some(pointer) = self.response.interact_pointer_pos() {
                let alt = ui.input(|input| input.modifiers.alt);
                let cursor = drag.from + (self.at.paper(pointer) - drag.pointer);
                let target = if alt { cursor } else { self.snap(drag.view, cursor) };
                self.preview(drag, target, cursor);
                if self.response.drag_stopped_by(PointerButton::Primary) {
                    state.drag = None;
                    let revision = app.session.revision;
                    match app.run("drawing.view.move", json!({"view":encode(drag.view),"to_mm":[target.x,target.y]})) {
                        Ok(_) if app.session.revision != revision => self.notice(app, &mut state, "ui.drawing_view_selection.moved"),
                        Ok(_) => {}
                        Err(error) => app.set_status(error, true),
                    }
                }
            } else if !ui.input(|input| input.pointer.primary_down()) {
                state.drag = None;
            }
        } else if !placing
            && let Some(selected) = state.selected
            && let Some(object) = self.objects.iter().find(|object| object.object == selected && object.kind == ObjectKind::View)
        {
            self.card(app, &mut state, *object);
        }

        self.confirm_delete(app, ui, &mut state);
        self.undo(app, ui, &mut state);
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

    fn card(&self, app: &mut SolveApp, state: &mut State, object: SheetObject) {
        let t = Tokens::get();
        let r = self.at.rect(&object.boundary_mm.unwrap_or(object.bounds_mm)).expand(t.space_2);
        self.painter.rect_stroke(r, t.radius_sm, Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
        for handle in [r.left_center(), r.right_center(), r.center_bottom()] {
            self.painter.circle_filled(handle, t.space_4, t.panel);
            self.painter.circle_stroke(handle, t.space_4, Stroke::new(1.0, t.accent));
        }
        if !r.intersects(self.free) {
            return;
        }
        let tr = solidcraft_ui_text::tr;
        let index = self.drawing.views.iter().filter(|view| view.sheet == self.sheet).position(|view| view.id == object.object).unwrap_or_default();
        let name = self.drawing.view(object.object).and_then(|view| view.label.as_ref()).map_or_else(
            || solidcraft_ui_text::format("ui.drawing_browser_view", &[("number", &(index + 1).to_string())]),
            |label| label.name.clone(),
        );
        let can_project = self.drawing.can_project_from(object.object).is_ok();
        let ctx = self.painter.ctx();
        let anchor = pos2(r.center().x.clamp(self.free.left(), self.free.right()), r.top() - t.space_8);
        let response = Container::selection_card(ctx, egui::Id::unique("drawing_view_selection_card"), anchor, self.free, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.space_2;
                ui.add_space(t.space_4);
                ui_kit::workspace_chip(ui, &name);
                ui.separator();
                for (action, icon, label, enabled) in [
                    ("edit", "edit", tr("ui.drawing_panel.edit"), true),
                    ("project", "plus", tr("ui.drawing_view_selection.project"), can_project),
                    ("section", "drawing_section_view", tr("ui.drawing_view_selection.section"), false),
                    ("delete", "delete", tr("ui.drawing_view_selection.delete"), true),
                ] {
                    let response = ui
                        .add_enabled_ui(enabled, |ui| {
                            let size = t.control_height - t.space_2;
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                            ui_kit::icon_button_at(ui, rect, icon, label)
                        })
                        .inner;
                    crate::scenario::publish_target(&format!("drawing:view_selection:{action}"), response.rect);
                    if response.clicked() {
                        return Some(action);
                    }
                    if !enabled {
                        response.backing_on_disabled_hover_text(tr("ui.drawing_panel.planned"));
                    }
                }
                ui.add_space(t.space_4);
                None
            })
            .inner
        });
        match response.inner {
            Some("edit") => drawing_workspace::edit_object(app, "view", object.object),
            Some("project") => {
                drawing_workspace::start_projected(app, object.object);
            }
            Some("delete") => state.delete = Some(object.object),
            _ => {}
        }
    }

    fn confirm_delete(&self, app: &mut SolveApp, ui: &egui::Ui, state: &mut State) {
        let Some(view) = state.delete else { return };
        let tr = solidcraft_ui_text::tr;
        let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("drawing_delete_view"), "delete", tr("ui.drawing_view_selection.delete"))
            .actions(tr("ui.drawing_view_selection.delete"), tr("ui.cancel"), None)
            .show(ui.ctx(), true, |ui| {
                ui.label(tr("ui.drawing_view_selection.delete_prompt"));
            });
        if matches!(action, ui_kit::Action::Ok) {
            match app.run(
                "selection.delete",
                json!({
                    "items":[{"type":"drawing","drawing":encode(self.drawing.id),"object":encode(view)}],
                    "contents":"delete"
                }),
            ) {
                Ok(_) => {
                    state.selected = None;
                    self.notice(app, state, "ui.drawing_view_selection.deleted");
                }
                Err(error) => app.set_status(error, true),
            }
        }
        if closed || !matches!(action, ui_kit::Action::None) {
            state.delete = None;
        }
    }

    fn notice(&self, app: &SolveApp, state: &mut State, message: &'static str) {
        state.undo = Some((app.session.revision, self.painter.ctx().input(|input| input.time) + 6.0, message));
    }

    fn undo(&self, app: &mut SolveApp, ui: &egui::Ui, state: &mut State) {
        let Some((revision, expires, message)) = state.undo else { return };
        let remaining = expires - ui.input(|input| input.time);
        if revision != app.session.revision || remaining <= 0.0 {
            state.undo = None;
            return;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
        let actions = [(solidcraft_ui_text::tr("ui.undo"), toast::Action::Primary)];
        let extras = toast::Extras { close: Some("drawing-view"), countdown: Some(((remaining / 6.0) as f32, 6.0)), ..Default::default() };
        if let Some(shown) = toast::show(ui, self.free, toast::Place::Centre, app.tr(message).to_owned(), toast::Lead::Dot(false), &actions, extras) {
            if shown.clicked == Some(0) {
                if let Err(error) = app.run("edit.undo", json!({})) {
                    app.set_status(error, true);
                }
                state.undo = None;
            } else if shown.closed {
                state.undo = None;
            }
        }
    }
}
