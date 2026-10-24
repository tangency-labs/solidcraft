//! Round an edge by dragging: press on an edge and pull away. Once the pointer has moved far
//! enough, Fillet starts on the edge (with its tangent chain) and its radius follows the
//! pointer's sideways travel, in steps; moving up or down sets the step (up: coarser, down:
//! finer), as when pulling a face. Holding Shift while pulling makes it a Chamfer
//! instead. Letting go leaves the window open, where a Round / Chamfer switch changes the kind
//! and OK applies it. Off unless "Direct interaction" is on in the preferences.

use egui::Pos2;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::drag_extrude::{STEPS, nearest_step, paint_ladder, step_index};
use crate::viewport::{Hit, Proj};

/// Screen distance the pointer must travel before a press counts as a pull.
const START_PX: f32 = 8.0;
/// The key of `Dialog::extra` that marks a window opened by this gesture.
pub const MARK: &str = "blend_gesture";

/// A press on an edge that may turn into a pull.
#[derive(Clone, Debug)]
pub struct Gesture {
    press: Pos2,
    sel: Sel,
    /// The picked spot on the edge, where the screen scale is measured.
    point: Vec3,
    /// Fillet has started (the window is open and follows the pointer).
    started: bool,
    /// The step at the start (a ladder position): vertical moves from the press change it.
    step0: usize,
    /// The step and size now, whether it is a chamfer, and where the pointer is, for the label.
    step: f64,
    size: f64,
    chamfer: bool,
    at: Pos2,
}

#[derive(Debug, Default)]
pub struct State {
    gesture: Option<Gesture>,
}

/// A pull has started: the box selection stays out of it.
pub fn started(app: &SolveApp) -> bool {
    app.viewport.drag_blend.gesture.as_ref().is_some_and(|g| g.started)
}

/// Note a press at `at` when it is on an edge.
pub fn press(app: &mut SolveApp, proj: &Proj, at: Pos2) {
    app.viewport.drag_blend.gesture = None;
    if !app.preferences.direct_interaction || app.dialog.is_some() {
        return;
    }
    let hits = crate::viewport::pick(app, proj, at);
    let Some((Hit::Edge { mid, .. }, sel @ Sel::Edge { .. })) = crate::viewport::candidate(app, &hits) else { return };
    app.viewport.drag_blend.gesture = Some(Gesture {
        press: at,
        sel,
        point: mid,
        started: false,
        step0: nearest_step(crate::canvas::snap_step(app.cam.half_height())),
        step: 0.0,
        size: 0.0,
        chamfer: false,
        at,
    });
}

/// Millimetres one screen point is worth at `point`, in the plane facing the view.
fn mm_per_px(proj: &Proj, at: Pos2, point: Vec3) -> Option<f64> {
    let on_plane = |p: Pos2| {
        let (o, d) = proj.ray(p);
        let along = d.dot(d);
        (along > 0.0).then(|| o + d * ((point - o).dot(d) / along))
    };
    let (a, b) = (on_plane(at)?, on_plane(at + egui::vec2(1.0, 0.0))?);
    let mm = (b - a).len();
    (mm.is_finite() && mm > 0.0).then_some(mm)
}

/// The pointer is at `p` with the button down; `shift` asks for a chamfer.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2, shift: bool) {
    let Some(mut g) = app.viewport.drag_blend.gesture.clone() else { return };
    if !g.started {
        if app.dialog.is_some() || (p - g.press).length() < START_PX {
            return;
        }
        crate::viewport::select(app, Some(g.sel.clone()), false, false);
        app.start("solid.fillet");
        let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, Kind::Fillet { .. })) else {
            app.viewport.drag_blend.gesture = None;
            return;
        };
        d.extra.insert(MARK.into(), true.into());
        g.started = true;
    }
    g.at = p;
    g.chamfer = shift;
    let Some(scale) = mm_per_px(proj, g.press, g.point) else {
        app.viewport.drag_blend.gesture = Some(g);
        return;
    };
    // Up the ladder is up the screen.
    g.step = STEPS[step_index(g.step0, g.press.y - p.y)];
    let step = g.step;
    let raw = f64::from((p.x - g.press.x).abs()) * scale;
    let size = if crate::tool_modifiers::manipulator_free() { raw } else { ((raw / step).round() * step).max(step) };
    g.size = size;
    app.viewport.drag_blend.gesture = Some(g.clone());
    let Some(Kind::Fillet { radius, chamfer, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) else { return };
    *radius = crate::canvas::format_len(size);
    *chamfer = g.chamfer;
}

/// The button is up: the window stays open for the user to adjust and confirm.
pub fn end(app: &mut SolveApp) {
    app.viewport.drag_blend.gesture = None;
}

/// The step ladder below the pointer and what the pull makes under it.
pub fn show(app: &SolveApp, painter: &egui::Painter) {
    let Some(g) = app.viewport.drag_blend.gesture.as_ref().filter(|g| g.started) else { return };
    let t = crate::theme::Tokens::get();
    let ladder = paint_ladder(painter, g.at, g.step);
    let kind = if g.chamfer { "Chamfer" } else { "Round" };
    let text = format!("{kind} {}   (Shift: chamfer)", crate::canvas::format_len(g.size));
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(t.font_body), t.text);
    let pos = ladder.left_bottom() + egui::vec2(5.0, 10.0);
    let rect = egui::Rect::from_min_size(pos, galley.size()).expand(5.0);
    painter.rect_filled(rect, 4.0, t.overlay);
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    painter.galley(pos, galley, t.text);
}
