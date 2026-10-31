//! Transform in the viewport: the picked sketch curves and points get a box with a handle at each
//! corner to scale (the opposite corner stays put, the aspect is kept), the box itself to move,
//! and a knob above it to turn (Shift snaps to 15 degrees). The handles edit the same Position,
//! Scale and Angle the window shows, and the entities are drawn where OK would put them.

use egui::{Pos2, Sense, Stroke, Vec2 as Screen, vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::{Plane, Vec2};

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::sketch_modify_ui::Mod;
use crate::viewport::Proj;

/// Half the side of a corner handle, in screen points.
const HANDLE: f32 = 6.0;
/// How far above the box the turning knob sits, in screen points.
const KNOB_GAP: f32 = 28.0;
const MIN_FACTOR: f64 = 0.0001;
const MAX_FACTOR: f64 = 1e5;

/// A number as the window's fields show it: up to three decimals, none trailing.
pub(crate) fn plain(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// What a drag took hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Move,
    /// A corner, by its signs in the box's own frame.
    Corner(f64, f64),
    Turn,
}

/// A drag in progress: the part, where the pointer began on the sketch, and the placement then.
#[derive(Clone, Debug)]
pub struct Drag {
    part: Part,
    from: Vec2,
    at: Frame,
}

/// The box as placed: where its middle is, how much it is scaled, how far turned, and its own
/// size before scaling.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    x: f64,
    y: f64,
    scale: f64,
    angle: f64,
    natural: (f64, f64),
}

fn turn(v: Vec2, degrees: f64) -> Vec2 {
    let (s, c) = degrees.to_radians().sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// A spot of the box in its own frame (`u`, `v` in half sizes) on the sketch.
fn spot(f: &Frame, u: f64, v: f64) -> Vec2 {
    Vec2::new(f.x, f.y) + turn(Vec2::new(u * f.natural.0 * f.scale / 2.0, v * f.natural.1 * f.scale / 2.0), f.angle)
}

fn on_screen(plane: &Plane, proj: &Proj, q: Vec2) -> Option<Pos2> {
    proj.to_screen(plane.to_world(q))
}

fn pointer_on_sketch(plane: &Plane, proj: &Proj, at: Pos2) -> Option<Vec2> {
    let (o, d) = proj.ray(at);
    plane.intersect_ray(o, d).map(|w| plane.to_local(w))
}

/// The picked curves (as polylines on the sketch) and the box around them and the picked points:
/// its middle and its size. None when nothing usable is picked or the box has no extent.
fn picked_box(app: &SolveApp, d: &crate::dialogs::Dialog) -> Option<(Vec<Vec<Vec2>>, [f64; 2], [f64; 2])> {
    let state = app.session.world_state();
    let sk = &state.sketch(app.session.active_sketch?)?.sketch;
    let mut lines: Vec<Vec<Vec2>> = Vec::new();
    let mut points: Vec<Vec2> = Vec::new();
    for sel in &d.inputs.first()?.items {
        match sel {
            Sel::SketchCurve { id, .. } => {
                if let Some(i) = sk.curve_index(id) {
                    lines.push(sk.polyline(i));
                }
            }
            Sel::SketchPoint { .. } => {
                if let Some(p) = crate::sketch_modify_ui::point_id(&app.session, sel).and_then(|id| sk.resolve_point(id)).and_then(|i| sk.point(i)) {
                    points.push(p);
                }
            }
            _ => {}
        }
    }
    let all: Vec<Vec2> = lines.iter().flatten().copied().chain(points).collect();
    let first = *all.first()?;
    let (lo, hi) = all.iter().fold((first, first), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))));
    let size = [hi.x - lo.x, hi.y - lo.y];
    // A single point or a straight line along an axis has no area to take hold of.
    (size[0] > 1e-9 && size[1] > 1e-9).then(|| (lines, [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0], size))
}

/// Keep the Transform window's box up to date with what is picked.
pub fn refresh(app: &mut SolveApp) {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::SketchModify(Mod::Transform { .. }))) else { return };
    let boxed = picked_box(app, d).map(|(_, middle, size)| (middle, size));
    if let Some(Kind::SketchModify(Mod::Transform { frame, .. })) = app.dialog.as_mut().map(|d| &mut d.kind) {
        *frame = boxed;
    }
}

/// The placement the window's fields describe, as a frame.
fn frame_of(app: &SolveApp, k: &Mod) -> Option<(Frame, [f64; 2])> {
    let Mod::Transform { x, y, factor, angle, frame } = k else { return None };
    let (middle, size) = (*frame)?;
    let (dx, dy) =
        (crate::sketch_modify_ui::value(app, x, ValueKind::Length, "X").ok()?, crate::sketch_modify_ui::value(app, y, ValueKind::Length, "Y").ok()?);
    let scale = crate::sketch_modify_ui::factor(app, factor).ok()?;
    let turned = crate::sketch_modify_ui::value(app, angle, ValueKind::Angle, "Angle").ok()?.to_degrees();
    Some((Frame { x: middle[0] + dx, y: middle[1] + dy, scale, angle: turned, natural: (size[0], size[1]) }, middle))
}

use solidcraft_engine::doc::expr::Kind as ValueKind;

/// Write a frame back into the window's fields.
fn store(k: &mut Mod, f: &Frame, middle: [f64; 2]) {
    if let Mod::Transform { x, y, factor, angle, .. } = k {
        *x = format!("{} mm", plain(f.x - middle[0]));
        *y = format!("{} mm", plain(f.y - middle[1]));
        *factor = format!("{}", (f.scale * 1e6).round() / 1e6);
        *angle = format!("{} deg", plain(f.angle));
    }
}

/// The frame after the pointer has moved to `p`, from where `drag` began.
fn apply(drag: &Drag, p: Vec2, snap: bool) -> Frame {
    let start = drag.at;
    let mut now = start;
    match drag.part {
        Part::Move => {
            now.x = start.x + (p.x - drag.from.x);
            now.y = start.y + (p.y - drag.from.y);
        }
        Part::Turn => {
            let to = p - Vec2::new(start.x, start.y);
            if to.len() > 1e-9 {
                let a = to.angle().to_degrees() - 90.0;
                let a = if snap { (a / 15.0).round() * 15.0 } else { a };
                now.angle = (a + 180.0).rem_euclid(360.0) - 180.0;
            }
        }
        Part::Corner(u, v) => {
            let fixed = spot(&start, -u, -v);
            // The pointer in the box's own frame, measured from the corner that stays.
            let local = turn(p - fixed, -start.angle);
            let k = (local.x.abs() / start.natural.0).max(local.y.abs() / start.natural.1).clamp(MIN_FACTOR, MAX_FACTOR);
            if k.is_finite() {
                let reach = turn(Vec2::new(u * k * start.natural.0 / 2.0, v * k * start.natural.1 / 2.0), start.angle);
                now.scale = k;
                now.x = fixed.x + reach.x;
                now.y = fixed.y + reach.y;
            }
        }
    }
    now
}

/// Where a point of the original box goes: scaled and turned about the middle, then moved.
fn moved(f: &Frame, middle: [f64; 2], q: Vec2) -> Vec2 {
    let m = Vec2::new(middle[0], middle[1]);
    Vec2::new(f.x, f.y) + turn((q - m) * f.scale, f.angle)
}

/// The Transform window's picks with their handles; a drag on them edits the window's values.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, proj: &Proj) {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::SketchModify(Mod::Transform { .. }))) else {
        app.viewport.transform_drag = None;
        return;
    };
    let Some((lines, _, _)) = picked_box(app, d) else { return };
    let Kind::SketchModify(k) = &d.kind else { return };
    let Some((frame, middle)) = frame_of(app, k) else { return };
    let Some(plane) = app.session.active_sketch.and_then(|id| crate::canvas::profile_plane(app, id)) else { return };
    let t = crate::theme::Tokens::get();
    let painter = ui.painter().clone();
    let stroke = Stroke::new(1.5, t.accent);
    // The picks where OK would leave them.
    for line in &lines {
        let pts: Vec<Pos2> = line.iter().filter_map(|q| on_screen(&plane, proj, moved(&frame, middle, *q))).collect();
        if pts.len() > 1 {
            painter.add(egui::Shape::line(pts, Stroke::new(2.5, t.preview_sketch)));
        }
    }
    let signs = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let Some(corners) = signs.iter().map(|(u, v)| on_screen(&plane, proj, spot(&frame, *u, *v))).collect::<Option<Vec<_>>>() else { return };
    let (Some(top), Some(heart)) = (on_screen(&plane, proj, spot(&frame, 0.0, 1.0)), on_screen(&plane, proj, spot(&frame, 0.0, 0.0))) else { return };
    let away = (top - heart).normalized();
    let knob = top + if away.length() > 0.0 { away * KNOB_GAP } else { vec2(0.0, -KNOB_GAP) };
    painter.add(egui::Shape::closed_line(corners.clone(), stroke));
    painter.line_segment([top, knob], stroke);
    painter.circle(knob, HANDLE, t.overlay, stroke);

    // The whole box first, so the corners and the knob above it win where they overlap.
    let mut parts: Vec<(egui::Rect, Part, egui::CursorIcon)> = vec![(egui::Rect::from_points(&corners), Part::Move, egui::CursorIcon::Grab)];
    for (c, (u, v)) in corners.iter().zip(signs) {
        painter.rect(egui::Rect::from_center_size(*c, Screen::splat(HANDLE * 2.0)), 1.0, t.overlay, stroke, egui::StrokeKind::Inside);
        parts.push((egui::Rect::from_center_size(*c, Screen::splat(HANDLE * 3.0)), Part::Corner(u, v), egui::CursorIcon::ResizeNwSe));
    }
    parts.push((egui::Rect::from_center_size(knob, Screen::splat(HANDLE * 3.0)), Part::Turn, egui::CursorIcon::Crosshair));
    for (i, (rect, part, cursor)) in parts.into_iter().enumerate() {
        let response = ui.interact(rect, egui::Id::unique(("sc_transform_handle", i)), Sense::drag()).on_hover_cursor(cursor);
        crate::scenario::publish_handle(&format!("transform:handle:{i}"), rect.center());
        if response.drag_started()
            && let Some(from) = response.interact_pointer_pos().and_then(|p| pointer_on_sketch(&plane, proj, p))
        {
            app.viewport.transform_drag = Some(Drag { part, from, at: frame });
        }
        // The release frame carries the last move too: it counts, or a quick drag stops short.
        if (response.dragged() || response.drag_stopped())
            && let Some(drag) = app.viewport.transform_drag.clone()
            && drag.part == part
            && let Some(p) =
                response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos())).and_then(|p| pointer_on_sketch(&plane, proj, p))
        {
            let now = apply(&drag, p, ui.input(|i| i.modifiers.shift));
            if let Some(Kind::SketchModify(k)) = app.dialog.as_mut().map(|d| &mut d.kind) {
                store(k, &now, middle);
            }
        }
        if response.drag_stopped() {
            app.viewport.transform_drag = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame { x: 10.0, y: 5.0, scale: 1.0, angle: 0.0, natural: (20.0, 10.0) }
    }

    fn drag(part: Part, from: Vec2, at: Frame) -> Drag {
        Drag { part, from, at }
    }

    #[test]
    fn a_corner_scales_about_the_opposite_corner_turned_or_not() {
        for angle in [0.0, 30.0] {
            let f = Frame { angle, ..frame() };
            let fixed = spot(&f, -1.0, -1.0);
            let pulled = spot(&f, 1.0, 1.0) + turn(Vec2::new(20.0, 10.0), angle);
            let now = apply(&drag(Part::Corner(1.0, 1.0), spot(&f, 1.0, 1.0), f), pulled, false);
            assert!((now.scale - 2.0).abs() < 1e-9, "{}", now.scale);
            assert!(spot(&now, -1.0, -1.0).dist(fixed) < 1e-9, "the opposite corner stays");
        }
    }

    #[test]
    fn the_box_moves_and_the_knob_turns() {
        let f = frame();
        let now = apply(&drag(Part::Move, Vec2::new(10.0, 5.0), f), Vec2::new(14.0, 1.0), false);
        assert_eq!((now.x, now.y), (14.0, 1.0));
        // The knob is straight above the middle: to the left of it is a quarter turn.
        let turned = apply(&drag(Part::Turn, Vec2::new(14.0, 10.0), now), Vec2::new(4.0, 1.0), false);
        assert!((turned.angle - 90.0).abs() < 1e-9, "{}", turned.angle);
        let snapped = apply(&drag(Part::Turn, Vec2::new(14.0, 10.0), now), Vec2::new(4.0, 3.0), true);
        assert!((snapped.angle % 15.0).abs() < 1e-9, "{}", snapped.angle);
    }

    #[test]
    fn plain_numbers_keep_three_decimals_at_most() {
        assert_eq!(plain(12.0), "12");
        assert_eq!(plain(1.23456), "1.235");
        assert_eq!(plain(-0.0001), "0");
    }
}
