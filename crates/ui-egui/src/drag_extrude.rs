//! Extrude by dragging: press on a planar face (or a sketch profile) and pull. Once the pointer
//! has moved far enough upward, or along the surface's normal on screen, Extrude starts on the
//! pick; the distance follows the pointer along the normal in steps. Moving the pointer sideways
//! while pulling sets the step: to the right it grows (1, 2, 3 ... 10 mm), to the left it
//! shrinks (0.8, 0.5 ... 0.1 mm). The step and the distance show beside the pointer. Letting go
//! leaves the Extrude window open to adjust and confirm with OK. A press that goes any other
//! way stays a box selection.

use egui::Pos2;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Kind, planar_face};
use crate::viewport::{Hit, Proj};

/// Screen distance the pointer must travel before a press counts as a pull.
const START_PX: f32 = 6.0;
/// How closely (cosine) a drag must follow the normal's screen direction to start a pull.
const ALIGNED: f32 = 0.8;
/// Screen distance the pointer travels sideways for one step of the ladder.
const PX_PER_STEP: f32 = 44.0;
/// The step sizes (mm) a pull can snap to, finest first.
const STEPS: [f64; 25] =
    [0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.65, 0.7, 0.8, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];

/// The ladder position nearest to `step`.
fn nearest_step(step: f64) -> usize {
    (0..STEPS.len()).min_by(|a, b| (STEPS[*a] / step).ln().abs().total_cmp(&(STEPS[*b] / step).ln().abs())).unwrap_or(0)
}

/// The ladder position `dx` screen points sideways from `base`, clamped to the ladder.
fn step_index(base: usize, dx: f32) -> usize {
    let moved = (dx / PX_PER_STEP).round() as i64;
    (base as i64 + moved).clamp(0, STEPS.len() as i64 - 1) as usize
}

/// A press on a surface that may turn into a pull.
#[derive(Clone, Debug)]
pub struct Gesture {
    press: Pos2,
    sel: Sel,
    /// A point on the surface where it was pressed, and its unit normal.
    base: Vec3,
    normal: Vec3,
    /// Where along the normal the press was: the distance is measured from here.
    grab: f64,
    /// Extrude has started (the window is open and follows the pointer).
    started: bool,
    /// The step at the start (a ladder position) and the pointer's x there: sideways moves
    /// from it change the step.
    step0: usize,
    x0: f32,
    /// The step now, and where the pointer is, for the label.
    step: f64,
    at: Pos2,
}

#[derive(Debug, Default)]
pub struct State {
    gesture: Option<Gesture>,
    /// The step of the last pull, shown in the window until it closes.
    last_step: Option<f64>,
}

/// A pull has started: the box selection stays out of it.
pub fn started(app: &SolveApp) -> bool {
    app.viewport.drag_extrude.gesture.as_ref().is_some_and(|g| g.started)
}

/// Note a press at `at` when it is on a planar face or a profile.
pub fn press(app: &mut SolveApp, proj: &Proj, at: Pos2) {
    app.viewport.drag_extrude.gesture = None;
    if !app.preferences.direct_interaction {
        return;
    }
    let hits = crate::viewport::pick(app, proj, at);
    let Some((hit, Some(sel))) = crate::viewport::candidate(app, &hits).map(|(h, s)| (h, Some(s))) else { return };
    let (o, d) = proj.ray(at);
    let (base, normal) = match (&hit, &sel) {
        (Hit::Face { point, .. }, Sel::Face { body, index, .. }) => match planar_face(&app.session, body, *index) {
            Some((_, n)) => (*point, n),
            None => return,
        },
        (Hit::Profile { sketch, .. }, Sel::Profile { .. }) => {
            let Some(plane) = crate::canvas::profile_plane(app, *sketch) else { return };
            match plane.intersect_ray(o, d) {
                Some(p) => (p, plane.normal()),
                None => return,
            }
        }
        _ => return,
    };
    let Some(grab) = crate::canvas::along(proj, at, base, normal) else { return };
    app.viewport.drag_extrude.gesture = Some(Gesture {
        press: at,
        sel,
        base,
        normal,
        grab,
        started: false,
        step0: nearest_step(crate::canvas::snap_step(app.cam.half_height())),
        x0: at.x,
        step: 0.0,
        at,
    });
}

/// Whether the move from the press to `p` is a pull: upward, or along the normal on screen.
fn pulls(proj: &Proj, g: &Gesture, p: Pos2) -> bool {
    let delta = p - g.press;
    if delta.length() < START_PX {
        return false;
    }
    let unit = delta / delta.length();
    let upward = delta.y < 0.0 && -unit.y > ALIGNED;
    let along_normal = proj
        .to_screen(g.base)
        .zip(proj.to_screen(g.base + g.normal))
        .map(|(a, b)| b - a)
        .filter(|n| n.length() > 1e-3)
        .is_some_and(|n| (n / n.length()).dot(unit).abs() > ALIGNED);
    upward || along_normal
}

/// The pointer is at `p` with the button down.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(mut g) = app.viewport.drag_extrude.gesture.clone() else { return };
    if !g.started {
        if app.dialog.is_some() || !pulls(proj, &g, p) {
            return;
        }
        crate::viewport::select(app, Some(g.sel.clone()), false, false);
        app.start("solid.press_pull");
        if !matches!(app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Extrude { .. })) {
            app.viewport.drag_extrude.gesture = None;
            return;
        }
        g = Gesture { started: true, x0: p.x, ..g };
    }
    g.at = p;
    g.step = STEPS[step_index(g.step0, p.x - g.x0)];
    let Some(v) = crate::canvas::along(proj, p, g.base, g.normal) else {
        app.viewport.drag_extrude.gesture = Some(g);
        return;
    };
    let free = crate::tool_modifiers::manipulator_free();
    let travelled = v - g.grab;
    let travelled = if free { travelled } else { (travelled / g.step).round() * g.step };
    app.viewport.drag_extrude.last_step = Some(g.step);
    app.viewport.drag_extrude.gesture = Some(g);
    let Some(Kind::Extrude { distance, direction, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) else { return };
    // A reversed extrude measures its distance against the normal.
    let sign = if matches!(*direction, 1 | crate::dialogs::TWO_SIDES_REVERSED) { -1.0 } else { 1.0 };
    *distance = crate::canvas::format_len(travelled * sign);
}

/// The button is up: the window stays open for the user to adjust and confirm.
pub fn end(app: &mut SolveApp) {
    app.viewport.drag_extrude.gesture = None;
}

/// Forget the last pull's step once the window is gone.
pub fn tick(app: &mut SolveApp) {
    if app.dialog.is_none() {
        app.viewport.drag_extrude.last_step = None;
    }
}

/// The step of the last pull, for the value box next to the distance.
pub fn step_note(app: &SolveApp) -> Option<String> {
    let step = app.viewport.drag_extrude.last_step?;
    Some(format!("Step {}", crate::canvas::format_len(step)))
}

/// The ladder of steps beside the pointer while pulling: the one in use between its neighbours.
/// It sits below the pointer, clear of the distance box at the arrow tip.
pub fn show(app: &SolveApp, painter: &egui::Painter) {
    let Some(g) = app.viewport.drag_extrude.gesture.as_ref().filter(|g| g.started) else { return };
    let t = crate::theme::Tokens::get();
    let now = nearest_step(g.step);
    let galley = |i: usize, current: bool| {
        let size = if current { t.font_body * 1.25 } else { t.font_body };
        let color = if current { t.accent } else { t.text_dim };
        let text = STEPS.get(i).map(|s| crate::canvas::format_len(*s)).unwrap_or_default();
        painter.layout_no_wrap(text, egui::FontId::proportional(size), color)
    };
    let cells: Vec<_> = (now.saturating_sub(2)..=(now + 2).min(STEPS.len() - 1)).map(|i| (galley(i, i == now), i == now)).collect();
    let gap = 12.0;
    let width = cells.iter().map(|(g, _)| g.size().x).sum::<f32>() + gap * cells.len().saturating_sub(1) as f32;
    let height = cells.iter().map(|(g, _)| g.size().y).fold(0.0, f32::max);
    let pos = g.at + egui::vec2(18.0, 22.0);
    let rect = egui::Rect::from_min_size(pos, egui::vec2(width, height)).expand(5.0);
    painter.rect_filled(rect, 4.0, t.overlay);
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    let mut x = pos.x;
    for (g, _) in cells {
        let w = g.size().x;
        let y = pos.y + (height - g.size().y) / 2.0;
        painter.galley(egui::pos2(x, y), g, t.text);
        x += w + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sideways_moves_walk_the_step_ladder() {
        let one = nearest_step(1.0);
        assert_eq!(STEPS[one], 1.0);
        assert_eq!(STEPS[step_index(one, PX_PER_STEP)], 2.0);
        assert_eq!(STEPS[step_index(one, -PX_PER_STEP)], 0.8);
        assert_eq!(STEPS[step_index(one, -8.0 * PX_PER_STEP)], 0.1);
        assert_eq!(step_index(one, -1e6), 0);
        assert_eq!(step_index(one, 1e6), STEPS.len() - 1);
    }
}
