//! Construction planes resized in the view. A selected construction plane shows handles at its
//! corners and the middles of its edges; dragging one moves that corner or edge across the
//! plane, and letting go keeps the size (`view.plane_size`, one undo step). A plane never
//! resized is a square around its origin that follows the zoom.

use std::cell::RefCell;

use egui::{CursorIcon, Painter, Pos2, Rect, Stroke, StrokeKind, vec2};
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::{Plane, Vec2, Vec3};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::Proj;

/// How near the pointer must be to a handle to take it (screen points).
const GRAB_PX: f32 = 8.0;
/// The smallest a plane is dragged to, mm.
const MIN_SIDE: f64 = 1.0;

/// A handle: which side it moves in x and y (-1 the low side, 1 the high side, 0 neither).
type Side = (i8, i8);

const HANDLES: [Side; 8] = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)];

#[derive(Clone, Debug)]
struct Drag {
    id: u64,
    name: String,
    plane: Plane,
    side: Side,
    start: [f64; 4],
    /// Where the pointer took the handle, from the handle (plane coordinates).
    offset: Vec2,
    live: [f64; 4],
}

thread_local! {
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
}

/// The rectangle plane `id` is drawn as, `[x0, y0, x1, y1]` in its own coordinates: while its
/// handle is dragged, where the drag has it; else the size it was given; else a square of half
/// side `half` (the size that follows the zoom).
pub(crate) fn extent(app: &SolveApp, id: u64, half: f64) -> [f64; 4] {
    if let Some(live) = DRAG.with(|d| d.borrow().as_ref().filter(|d| d.id == id).map(|d| d.live)) {
        return live;
    }
    app.session.doc.plane_extents.get(&id).copied().unwrap_or([-half, -half, half, half])
}

/// The construction plane to show handles on: the only thing selected, with nothing else going
/// on (no dialog, tool or sketch being edited).
fn target(app: &SolveApp) -> Option<(u64, String, Plane)> {
    if app.dialog.is_some() || app.tool.is_some() || app.session.active_sketch.is_some() {
        return None;
    }
    let [Sel::Plane { name }] = app.session.selection.as_slice() else { return None };
    if !app.session.construction_visible(name) {
        return None;
    }
    solidcraft_engine::view::construction_planes(&app.session).into_iter().find(|(_, n, _)| n == name)
}

fn half(app: &SolveApp) -> f64 {
    crate::selection::origin_size(app.cam.half_height()) * 1.2
}

/// The point of `e` a handle sits at, in plane coordinates.
fn handle_at(e: [f64; 4], (sx, sy): Side) -> Vec2 {
    let pick = |s: i8, lo: f64, hi: f64| match s {
        -1 => lo,
        1 => hi,
        _ => (lo + hi) * 0.5,
    };
    Vec2::new(pick(sx, e[0], e[2]), pick(sy, e[1], e[3]))
}

fn world(plane: &Plane, p: Vec2) -> Vec3 {
    plane.origin + plane.x * p.x + plane.y * p.y
}

/// The handles on screen: (side, screen point).
fn handles(app: &SolveApp, proj: &Proj) -> Option<(u64, String, Plane, [f64; 4], Vec<(Side, Pos2)>)> {
    let (id, name, plane) = target(app)?;
    let e = extent(app, id, half(app));
    let at = HANDLES.iter().filter_map(|s| proj.to_screen(world(&plane, handle_at(e, *s))).map(|p| (*s, p))).collect();
    Some((id, name, plane, e, at))
}

fn hit(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<Side> {
    let (_, _, _, _, at) = handles(app, proj)?;
    at.into_iter().filter(|(_, p)| p.distance(pos) <= GRAB_PX).min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos))).map(|(s, _)| s)
}

/// Is a handle under the pointer? (A click there does not change the selection.)
pub(crate) fn over(app: &SolveApp, proj: &Proj, pos: Pos2) -> bool {
    hit(app, proj, pos).is_some()
}

fn on_plane(plane: &Plane, proj: &Proj, pos: Pos2) -> Option<Vec2> {
    let (o, d) = proj.ray(pos);
    plane.intersect_ray(o, d).map(|w| plane.to_local(w))
}

/// A drag starting on a handle (true: the handles took it).
pub(crate) fn drag_start(app: &SolveApp, proj: &Proj, press: Pos2) -> bool {
    let Some(side) = hit(app, proj, press) else { return false };
    let Some((id, name, plane, e, _)) = handles(app, proj) else { return false };
    let Some(at) = on_plane(&plane, proj, press) else { return false };
    let offset = at - handle_at(e, side);
    DRAG.with(|d| *d.borrow_mut() = Some(Drag { id, name, plane, side, start: e, offset, live: e }));
    true
}

/// Is a handle being dragged?
pub(crate) fn dragging() -> bool {
    DRAG.with(|d| d.borrow().is_some())
}

/// `e` with the sides `side` moves brought to `to`, never smaller than [`MIN_SIDE`]. With
/// `symmetric` the opposite side moves the other way too (the middle stays).
fn follow(e: [f64; 4], (sx, sy): Side, to: Vec2, symmetric: bool) -> [f64; 4] {
    let mut out = e;
    let mut axis = |s: i8, lo: usize, hi: usize, v: f64| {
        let mid = (e[lo] + e[hi]) * 0.5;
        match s {
            -1 => {
                out[lo] = v.min(e[hi] - MIN_SIDE);
                if symmetric {
                    out[lo] = v.min(mid - MIN_SIDE * 0.5);
                    out[hi] = 2.0 * mid - out[lo];
                }
            }
            1 => {
                out[hi] = v.max(e[lo] + MIN_SIDE);
                if symmetric {
                    out[hi] = v.max(mid + MIN_SIDE * 0.5);
                    out[lo] = 2.0 * mid - out[hi];
                }
            }
            _ => {}
        }
    };
    axis(sx, 0, 2, to.x);
    axis(sy, 1, 3, to.y);
    out
}

/// Follow the pointer (shown at once; kept when the drag ends). Shift resizes about the middle.
pub(crate) fn drag_to(proj: &Proj, pos: Pos2, symmetric: bool) {
    DRAG.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return };
        let Some(at) = on_plane(&d.plane, proj, pos) else { return };
        d.live = follow(d.start, d.side, at - d.offset, symmetric);
    });
}

/// The drag is over: the plane keeps the size it was dragged to, as one undo step.
pub(crate) fn drag_end(app: &mut SolveApp) {
    let Some(d) = DRAG.with(|d| d.borrow_mut().take()) else { return };
    if d.live != d.start {
        let _ = app.run("view.plane_size", json!({"plane": d.name, "extent": d.live}));
    }
}

/// A drag given up (the selection or the mode changed under it): the plane goes back.
pub(crate) fn cancel() {
    DRAG.with(|d| *d.borrow_mut() = None);
}

/// Draw the handles of the selected construction plane.
pub(crate) fn show(app: &SolveApp, ui: &egui::Ui, painter: &Painter, proj: &Proj, hover: Option<Pos2>) {
    let Some((_, _, _, _, at)) = handles(app, proj) else {
        if dragging() {
            cancel();
        }
        return;
    };
    let tk = Tokens::get();
    let drag = DRAG.with(|d| d.borrow().as_ref().map(|d| d.side));
    let under = hover.and_then(|p| hit(app, proj, p));
    for (side, p) in &at {
        let lit = drag == Some(*side) || (drag.is_none() && under == Some(*side));
        let r = Rect::from_center_size(*p, vec2(8.0, 8.0));
        painter.rect_filled(r, 1.0, if lit { tk.accent } else { tk.panel });
        painter.rect_stroke(r, 1.0, Stroke::new(1.5, tk.accent), StrokeKind::Middle);
        crate::scenario::publish_handle(&format!("plane_size_{}_{}", side.0, side.1), *p);
    }
    if let Some(side) = drag.or(under) {
        ui.ctx().set_cursor_icon(match side {
            (0, _) => CursorIcon::ResizeVertical,
            (_, 0) => CursorIcon::ResizeHorizontal,
            _ => CursorIcon::Move,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_moves_its_sides_and_keeps_a_least_size() {
        let e = [-10.0, -10.0, 10.0, 10.0];
        assert_eq!(follow(e, (1, 0), Vec2::new(25.0, 99.0), false), [-10.0, -10.0, 25.0, 10.0]);
        assert_eq!(follow(e, (-1, 1), Vec2::new(-30.0, 40.0), false), [-30.0, -10.0, 10.0, 40.0]);
        // Past the opposite side: it stops short of it.
        assert_eq!(follow(e, (0, -1), Vec2::new(0.0, 50.0), false), [-10.0, 10.0 - MIN_SIDE, 10.0, 10.0]);
        // Shift: about the middle.
        assert_eq!(follow(e, (1, 1), Vec2::new(20.0, 15.0), true), [-20.0, -15.0, 20.0, 15.0]);
    }
}
