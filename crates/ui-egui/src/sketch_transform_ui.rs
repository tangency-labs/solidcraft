//! Transform in the viewport: the picked sketch curves and points get a box with a handle at each
//! corner to scale (the opposite corner stays put, the aspect is kept), the box itself to move,
//! and a knob above it to turn (Shift snaps to 15 degrees). The handles edit the same Position,
//! Scale and Angle the window shows, and the entities are drawn where OK would put them. Scale and
//! Angle work about the window's Center point when one is picked (#11718), else the box middle.

use egui::{Pos2, Sense, Stroke, Vec2 as Screen, vec2};
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::geom::prelude::*;

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

/// The box as placed: where its pivot is (the point it scales and turns about), how much it is
/// scaled, how far turned, its own size before scaling, and where the pivot and the box middle
/// were before the move.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    x: f64,
    y: f64,
    scale: f64,
    angle: f64,
    natural: (f64, f64),
    pivot: Vec2,
    middle: Vec2,
}

/// Where the box's middle is placed.
fn center(f: &Frame) -> Vec2 {
    Vec2::new(f.x, f.y) + turn((f.middle - f.pivot) * f.scale, f.angle)
}

fn turn(v: Vec2, degrees: f64) -> Vec2 {
    let (s, c) = degrees.to_radians().sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// A spot of the box in its own frame (`u`, `v` in half sizes) on the sketch.
fn spot(f: &Frame, u: f64, v: f64) -> Vec2 {
    center(f) + turn(Vec2::new(u * f.natural.0 * f.scale / 2.0, v * f.natural.1 * f.scale / 2.0), f.angle)
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
    let state = crate::open_meshing::session_world(&app.session);
    let sk = &state.sketch(app.session.active_sketch?)?.sketch;
    let mut lines: Vec<Vec<Vec2>> = Vec::new();
    let mut points: Vec<Vec2> = Vec::new();
    let mut drawn: Vec<usize> = Vec::new();
    for id in crate::sketch_modify_ui::entity_ids(app, &d.inputs, 0) {
        if let Some(i) = sk.curve_index(&id) {
            // An ellipse's axes go with it when OK applies (`sketch.transform`), so they are
            // drawn with it.
            let (major, minor) = sk.ellipse_axis_lines(i).unwrap_or((None, None));
            for c in std::iter::once(i).chain(major.map(|(l, _)| l)).chain(minor.map(|(l, ..)| l)) {
                if !drawn.contains(&c) {
                    drawn.push(c);
                    lines.push(sk.polyline(c));
                }
            }
        } else if let Some(p) = sk.resolve_point(&id).and_then(|i| sk.point(i)) {
            points.push(p);
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

/// The window's Center point on the sketch, when one is picked.
fn picked_pivot(app: &SolveApp, d: &crate::dialogs::Dialog) -> Option<Vec2> {
    let state = crate::open_meshing::session_world(&app.session);
    let sk = &state.sketch(app.session.active_sketch?)?.sketch;
    let sel = d.inputs.get(1)?.items.first()?;
    crate::sketch_modify_ui::point_id(&app.session, sel).and_then(|id| sk.resolve_point(id)).and_then(|i| sk.point(i))
}

/// The placement the window's fields describe, as a frame.
fn frame_of(app: &SolveApp, d: &crate::dialogs::Dialog) -> Option<Frame> {
    let Kind::SketchModify(Mod::Transform { x, y, factor, angle, frame }) = &d.kind else { return None };
    let (middle, size) = (*frame)?;
    let middle = Vec2::new(middle[0], middle[1]);
    let pivot = picked_pivot(app, d).unwrap_or(middle);
    let (dx, dy) =
        (crate::sketch_modify_ui::value(app, x, ValueKind::Length, "X").ok()?, crate::sketch_modify_ui::value(app, y, ValueKind::Length, "Y").ok()?);
    let scale = crate::sketch_modify_ui::factor(app, factor).ok()?;
    let turned = crate::sketch_modify_ui::value(app, angle, ValueKind::Angle, "Angle").ok()?.to_degrees();
    Some(Frame { x: pivot.x + dx, y: pivot.y + dy, scale, angle: turned, natural: (size[0], size[1]), pivot, middle })
}

use solidcraft_engine::doc::expr::Kind as ValueKind;

/// Write a frame back into the window's fields.
fn store(k: &mut Mod, f: &Frame) {
    if let Mod::Transform { x, y, factor, angle, .. } = k {
        *x = format!("{} mm", plain(f.x - f.pivot.x));
        *y = format!("{} mm", plain(f.y - f.pivot.y));
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
            // Turned about the pivot by as much as the pointer has turned about it since the drag
            // began; with the box middle as the pivot that keeps the knob under the pointer.
            let pivot = Vec2::new(start.x, start.y);
            let (to, from) = (p - pivot, drag.from - pivot);
            if to.len() > 1e-9 && from.len() > 1e-9 {
                let a = start.angle + (to.angle() - from.angle()).to_degrees();
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
                // The pivot goes where it keeps the box middle at `fixed + reach`.
                let pivot = fixed + reach - turn((start.middle - start.pivot) * k, start.angle);
                now.x = pivot.x;
                now.y = pivot.y;
            }
        }
    }
    now
}

/// Where a point of the original box goes: scaled and turned about the pivot, then moved.
fn moved(f: &Frame, q: Vec2) -> Vec2 {
    Vec2::new(f.x, f.y) + turn((q - f.pivot) * f.scale, f.angle)
}

/// The Transform window's picks with their handles; a drag on them edits the window's values.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, proj: &Proj) {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::SketchModify(Mod::Transform { .. }))) else {
        app.viewport.transform_drag = None;
        return;
    };
    let Some((lines, _, _)) = picked_box(app, d) else { return };
    let Some(frame) = frame_of(app, d) else { return };
    let Some(plane) = app.session.active_sketch.and_then(|id| crate::canvas::profile_plane(app, id)) else { return };
    let t = solidcraft_theme::Tokens::get();
    let painter = ui.painter().clone();
    let stroke = Stroke::new(1.5, t.accent);
    // The picks where OK would leave them.
    for line in &lines {
        let pts: Vec<Pos2> = line.iter().filter_map(|q| on_screen(&plane, proj, moved(&frame, *q))).collect();
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
    // While Center waits for its pick, a click goes to the sketch: a rectangle's corner, the
    // point most often picked there, is under a corner handle (#11718). Once it has one, the
    // handles work again; Clear (×) empties it to pick another point there.
    if d.active == 1 && d.inputs.get(1).is_some_and(|i| i.items.is_empty()) && app.viewport.transform_drag.is_none() {
        return;
    }
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
                store(k, &now);
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
        let middle = Vec2::new(10.0, 5.0);
        Frame { x: 10.0, y: 5.0, scale: 1.0, angle: 0.0, natural: (20.0, 10.0), pivot: middle, middle }
    }

    /// The box of `frame()` with its lower left corner (0, 0) as the pivot.
    fn cornered() -> Frame {
        Frame { x: 0.0, y: 0.0, pivot: Vec2::ZERO, ..frame() }
    }

    /// #11718: with a picked Center, the knob turns and a corner scales about it, and the moved
    /// entities and the box agree.
    #[test]
    fn a_picked_center_stays_put_while_the_box_turns_and_scales() {
        let f = cornered();
        assert!(center(&f).dist(Vec2::new(10.0, 5.0)) < 1e-9 && spot(&f, -1.0, -1.0).dist(Vec2::ZERO) < 1e-9, "unmoved, the box is where it was");
        // The pointer goes a quarter turn round the pivot: from (10, 0) up to (0, 10).
        let turned = apply(&drag(Part::Turn, Vec2::new(10.0, 0.0), f), Vec2::new(0.0, 10.0), false);
        assert!((turned.angle - 90.0).abs() < 1e-9, "{}", turned.angle);
        assert!(moved(&turned, Vec2::ZERO).dist(Vec2::ZERO) < 1e-9, "the pivot stays");
        assert!(moved(&turned, Vec2::new(20.0, 0.0)).dist(Vec2::new(0.0, 20.0)) < 1e-9, "the far corner turns about it");
        assert!(spot(&turned, 1.0, -1.0).dist(Vec2::new(0.0, 20.0)) < 1e-9, "the box turns with the entities");
        // A corner drag still keeps the opposite corner, the pivot following the box.
        let fixed = spot(&turned, -1.0, -1.0);
        let pulled = spot(&turned, 1.0, 1.0) + turn(Vec2::new(20.0, 10.0), 90.0);
        let scaled = apply(&drag(Part::Corner(1.0, 1.0), spot(&turned, 1.0, 1.0), turned), pulled, false);
        assert!((scaled.scale - 2.0).abs() < 1e-9 && spot(&scaled, -1.0, -1.0).dist(fixed) < 1e-9, "{scaled:?}");
        for (u, v, q) in [(-1.0, -1.0, Vec2::ZERO), (1.0, 1.0, Vec2::new(20.0, 10.0))] {
            assert!(moved(&scaled, q).dist(spot(&scaled, u, v)) < 1e-9, "a corner of the entities is a corner of the box");
        }
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

    /// An ellipse picked alone is drawn where OK would leave it with its axes, as OK moves,
    /// turns and scales them with it.
    #[test]
    fn an_ellipse_picked_alone_is_previewed_with_its_axes() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", serde_json::json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.ellipse", serde_json::json!({"center": [0, 0], "major": [20, 0], "minor_radius": 8})).unwrap();
        h.call("ui.view", serde_json::json!({"view": "top"}));
        h.settle();
        h.step(&serde_json::json!({"click": {"sketch": [10.0, 6.928]}})).unwrap();
        h.step(&serde_json::json!({"start": "sketch.transform"})).unwrap();
        h.frames(3);
        let d = h.app.dialog.as_ref().expect("the Transform window");
        let (lines, middle, size) = picked_box(&h.app, d).expect("a box round the ellipse");
        assert_eq!(lines.len(), 3, "the ellipse and its two axes");
        assert!(
            middle[0].abs() < 1e-6 && middle[1].abs() < 1e-6 && (size[0] - 40.0).abs() < 1e-3 && (size[1] - 16.0).abs() < 1e-3,
            "{middle:?} {size:?}"
        );
        // With its minor axis deleted, the major one that is left.
        h.step(&serde_json::json!({"key": "Escape"})).unwrap();
        let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let minor = sk.curves.iter().rev().find(|c| c.construction).unwrap().id.clone();
        h.app.run("sketch.delete", serde_json::json!({"entities": [minor]})).unwrap();
        h.settle();
        h.step(&serde_json::json!({"click": {"sketch": [10.0, 6.928]}})).unwrap();
        h.step(&serde_json::json!({"start": "sketch.transform"})).unwrap();
        h.frames(3);
        let d = h.app.dialog.as_ref().expect("the Transform window");
        let (lines, _, size) = picked_box(&h.app, d).expect("a box round the ellipse");
        assert_eq!(lines.len(), 2, "the ellipse and its major axis");
        assert!((size[0] - 40.0).abs() < 1e-3, "{size:?}");
    }
}
