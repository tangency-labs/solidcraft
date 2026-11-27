//! The Detail View tool (Drawing › Layout › Views › Detail View): three clicks on the sheet. The
//! first, inside a view, is the centre of the region; the region follows the pointer, a circle
//! through it (or a rectangle with a corner on it), and the second click fixes it; the detail, at
//! twice its parent's scale, follows the pointer, and the third click places it there
//! (`drawing.view.detail`). The right panel chooses Circle or Rectangle and says what the next
//! click does. Esc, like every tool's, puts the tool away; nothing is made until the last click.

use egui::{Pos2, Stroke};
use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::doc::ids;
use solidcraft_engine::drawing_scene::SheetObject;
use solidcraft_engine::geom::paper::ObjectKind;
use solidcraft_engine::geom::prelude::*;
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::drawing_sheet_page::OnScreen;
use crate::ui_kit;

/// What the tool has so far, kept while it is in hand.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    /// The region is a rectangle, not a circle.
    pub(crate) rectangle: bool,
    /// The view picked, and the region's centre on the sheet.
    pub(crate) centre: Option<(u64, Vec2)>,
    /// The region's half size on the sheet, once fixed: its radius, or its half width and height.
    pub(crate) half: Option<Vec2>,
    /// The sheet the clicks are on: another sheet shown starts the tool again.
    pub(crate) sheet: Option<u64>,
}

fn tr(id: &'static str) -> &'static str {
    solidcraft_ui_text::tr(id)
}

/// The view whose region holds `point`, the topmost of those that do.
fn view_at(objects: &[SheetObject], point: Vec2) -> Option<u64> {
    objects.iter().rev().filter(|o| o.kind == ObjectKind::View).find(|o| o.boundary_mm.unwrap_or(o.bounds_mm).contains(point)).map(|o| o.object)
}

/// The region's half size for the pointer at `point`: a circle through it, or a rectangle with a
/// corner on it. Never quite nothing, so a click on the centre still fixes a region.
fn half_to(rectangle: bool, centre: Vec2, point: Vec2) -> Vec2 {
    const LEAST_MM: f64 = 0.5;
    let d = point - centre;
    if rectangle { Vec2::new(d.x.abs().max(LEAST_MM), d.y.abs().max(LEAST_MM)) } else { Vec2::new(d.len().max(LEAST_MM), d.len().max(LEAST_MM)) }
}

/// The `drawing.view.detail` parameters for a region at `centre` of `half` size, on the sheet,
/// placed at `at`.
pub(crate) fn params(parent: u64, rectangle: bool, centre: Vec2, half: Vec2, at: Vec2) -> serde_json::Value {
    let boundary = if rectangle {
        json!({"rectangle": {"min": [centre.x - half.x, centre.y - half.y], "max": [centre.x + half.x, centre.y + half.y]}})
    } else {
        json!({"circle": {"centre": [centre.x, centre.y], "radius": half.x}})
    };
    json!({"parent": ids::encode(parent), "boundary_mm": boundary, "place": {"at": [at.x, at.y]}})
}

/// Draws a region of `half` size round `centre`, a circle or a rectangle, in the preview colour.
fn outline(painter: &egui::Painter, at: OnScreen, rectangle: bool, centre: Vec2, half: Vec2) {
    let t = Tokens::get();
    let stroke = Stroke::new(1.5, t.accent);
    if rectangle {
        let (a, b) = (at.screen(centre - half), at.screen(centre + half));
        painter.rect_stroke(egui::Rect::from_two_pos(a, b), 0.0, stroke, egui::StrokeKind::Middle);
    } else {
        let c: Pos2 = at.screen(centre);
        let r = (at.screen(centre + Vec2::new(half.x, 0.0)).x - c.x).abs();
        painter.circle_stroke(c, r, stroke);
    }
}

/// Starts the tool again when `sheet` isn't the one its clicks were on, keeping the shape: clicks on
/// one sheet never join clicks on another. The sheet page calls it every frame the tool is in hand,
/// wherever the pointer is, so a visit to another sheet and back starts again too.
pub(crate) fn follow_sheet(app: &mut SolveApp, sheet: u64) {
    let draft = app.drawing_workspace.detail.unwrap_or_default();
    if draft.sheet != Some(sheet) {
        app.drawing_workspace.detail = Some(Draft { rectangle: draft.rectangle, sheet: Some(sheet), ..Draft::default() });
    }
}

/// The tool on the sheet, the pointer at `point`: its preview, and what a click does.
pub(crate) fn on_sheet(app: &mut SolveApp, painter: &egui::Painter, at: OnScreen, objects: &[SheetObject], sheet: u64, point: Vec2, clicked: bool) {
    follow_sheet(app, sheet);
    let mut draft = app.drawing_workspace.detail.unwrap_or_default();
    match (draft.centre, draft.half) {
        (None, _) => {
            if clicked {
                match view_at(objects, point) {
                    Some(view) => draft.centre = Some((view, point)),
                    None => app.set_status(tr("ui.drawing_detail.pick_view"), true),
                }
            }
        }
        (Some((_, centre)), None) => {
            let half = half_to(draft.rectangle, centre, point);
            outline(painter, at, draft.rectangle, centre, half);
            if clicked {
                draft.half = Some(half);
            }
        }
        (Some((parent, centre)), Some(half)) => {
            outline(painter, at, draft.rectangle, centre, half);
            // The detail at twice its parent's scale, as the command makes it, where it would go.
            outline(painter, at, draft.rectangle, point, half * 2.0);
            if clicked {
                match app.run("drawing.view.detail", params(parent, draft.rectangle, centre, half, point)) {
                    Ok(_) => {
                        app.drawing_workspace.detail = None;
                        super::close_panel(app);
                        return;
                    }
                    Err(error) => app.set_status(error, true),
                }
            }
        }
    }
    app.drawing_workspace.detail = Some(draft);
}

/// The right panel: the region's shape, and what the next click does. True when it closes.
pub(super) fn panel(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    let mut draft = app.drawing_workspace.detail.unwrap_or_default();
    let cancel = super::inset(ui, |ui| {
        let mut shape = usize::from(draft.rectangle);
        let options = [tr("ui.drawing_detail.circle"), tr("ui.drawing_detail.rectangle")];
        ui_kit::choice_row(ui, tr("ui.drawing_detail.shape"), "drawing:detail:shape", &mut shape, &options);
        // The shape changes the region still being drawn, never one already fixed.
        if draft.half.is_none() {
            draft.rectangle = shape == 1;
        }
        let step = match (draft.centre, draft.half) {
            (None, _) => "ui.drawing_detail.step_centre",
            (Some(_), None) => "ui.drawing_detail.step_size",
            (Some(_), Some(_)) => "ui.drawing_detail.step_place",
        };
        ui_kit::text(ui, tr(step), ui_kit::Text::Note(step));
        ui.separator();
        ui_kit::button(ui, tr("ui.cancel"), ui_kit::Button::Normal).clicked()
    });
    app.drawing_workspace.detail = Some(draft);
    if cancel {
        app.drawing_workspace.detail = None;
    }
    cancel
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    /// Detail View on the ribbon opens its panel; a click inside the front view, a click 10 mm away
    /// and a click beside the sheet's views make a detail of the front view, a circle of 10 mm on
    /// the sheet round the first click, placed at the third, and the tool is put away.
    #[test]
    fn three_clicks_make_a_detail_view() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":100,"width":50,"height":20})).unwrap();
        super::super::enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.app.run("drawing.create", json!({})).unwrap();
        let made = h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [150, 150]})).unwrap();
        let front = ids::decode(made["view"].as_str().unwrap()).unwrap();
        h.frames(6);
        h.app.ui.tab = "DRAWING LAYOUT".into();
        h.frames(4);
        h.step(&json!({"click":{"handle":"button:drawing.view.detail.planned"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.drawing_workspace.options, Some(super::super::Tool::Detail));
        let d = h.app.session.drawing("drawing.inspect").unwrap().clone();
        let sheet = d.sheets[0].id;
        let objects = solidcraft_engine::drawing_scene::build(&h.app.session, &d, sheet).unwrap().objects;
        let painter = egui::Painter::new(egui::Context::default(), egui::LayerId::background(), egui::Rect::EVERYTHING);
        let at = OnScreen::for_test();
        let centre = Vec2::new(150.0, 150.0);
        on_sheet(&mut h.app, &painter, at, &objects, sheet, centre, true);
        on_sheet(&mut h.app, &painter, at, &objects, sheet, centre + Vec2::new(10.0, 0.0), true);
        assert_eq!(h.app.drawing_workspace.detail.and_then(|d| d.half), Some(Vec2::new(10.0, 10.0)));
        on_sheet(&mut h.app, &painter, at, &objects, sheet, Vec2::new(300.0, 100.0), true);
        let d = h.app.session.drawing("drawing.inspect").unwrap().clone();
        let detail =
            d.views.iter().find(|v| matches!(v.kind, solidcraft_engine::doc::drawing::view::ViewKind::Detail { parent, .. } if parent == front));
        assert!(detail.is_some(), "a detail of the front view");
        assert!(h.app.drawing_workspace.options.is_none(), "the tool is put away");
        // Begun on one sheet, the tool starts again on another: no region joins a point there.
        h.step(&json!({"click":{"handle":"button:drawing.view.detail.planned"}})).unwrap();
        on_sheet(&mut h.app, &painter, at, &objects, sheet, centre, true);
        on_sheet(&mut h.app, &painter, at, &objects, sheet + 1, centre + Vec2::new(10.0, 0.0), false);
        assert_eq!(h.app.drawing_workspace.detail.map(|d| (d.centre, d.half, d.sheet)), Some((None, None, Some(sheet + 1))));
        // A region fixed on one sheet, a visit to another with the pointer off its paper, and back:
        // the tool starts again.
        on_sheet(&mut h.app, &painter, at, &objects, sheet, centre, true);
        on_sheet(&mut h.app, &painter, at, &objects, sheet, centre + Vec2::new(10.0, 0.0), true);
        follow_sheet(&mut h.app, sheet + 1);
        follow_sheet(&mut h.app, sheet);
        assert_eq!(h.app.drawing_workspace.detail.map(|d| (d.centre, d.half)), Some((None, None)));
    }

    /// Esc after the centre is picked puts the tool away with its clicks: Detail View started again
    /// asks for a centre, and no region grows from the old one (#22094).
    #[test]
    fn esc_forgets_the_centre_so_the_next_start_asks_again() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":100,"width":50,"height":20})).unwrap();
        super::super::enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.app.run("drawing.create", json!({})).unwrap();
        h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [150, 150]})).unwrap();
        h.frames(6);
        h.app.ui.tab = "DRAWING LAYOUT".into();
        h.frames(4);
        h.step(&json!({"click":{"handle":"button:drawing.view.detail.planned"}})).unwrap();
        h.frames(2);
        let d = h.app.session.drawing("drawing.inspect").unwrap().clone();
        let sheet = d.sheets[0].id;
        let objects = solidcraft_engine::drawing_scene::build(&h.app.session, &d, sheet).unwrap().objects;
        let painter = egui::Painter::new(egui::Context::default(), egui::LayerId::background(), egui::Rect::EVERYTHING);
        on_sheet(&mut h.app, &painter, OnScreen::for_test(), &objects, sheet, Vec2::new(150.0, 150.0), true);
        assert!(h.app.drawing_workspace.detail.and_then(|d| d.centre).is_some(), "the click inside the view picks a centre");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.drawing_workspace.options, None, "Esc puts the tool away");
        h.step(&json!({"click":{"handle":"button:drawing.view.detail.planned"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.drawing_workspace.options, Some(super::super::Tool::Detail));
        assert_eq!(h.app.drawing_workspace.detail.and_then(|d| d.centre), None, "started again, the tool asks for a centre");
    }

    /// The parameters: a circle by its centre and radius, a rectangle by its corners, both on the
    /// sheet, and the place.
    #[test]
    fn each_shape_gives_its_region_on_the_sheet() {
        let (c, h, at) = (Vec2::new(10.0, 20.0), Vec2::new(3.0, 2.0), Vec2::new(50.0, 60.0));
        let id = ids::encode(7);
        assert_eq!(
            params(7, false, c, Vec2::new(3.0, 3.0), at),
            json!({"parent": id, "boundary_mm": {"circle": {"centre": [10.0, 20.0], "radius": 3.0}}, "place": {"at": [50.0, 60.0]}})
        );
        assert_eq!(
            params(7, true, c, h, at),
            json!({"parent": id, "boundary_mm": {"rectangle": {"min": [7.0, 18.0], "max": [13.0, 22.0]}}, "place": {"at": [50.0, 60.0]}})
        );
        assert_eq!(half_to(false, c, c), Vec2::new(0.5, 0.5), "a region is never nothing");
        assert_eq!(half_to(true, c, Vec2::new(14.0, 17.0)), Vec2::new(4.0, 3.0));
    }
}
