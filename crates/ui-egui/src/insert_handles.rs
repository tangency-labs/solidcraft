//! Handles on the drawing being placed by Insert SVG / DXF: its box with a corner at each end
//! to scale (the opposite corner stays put, the aspect is kept), the box itself to move, and a
//! knob above it to turn (Shift snaps to 15 degrees). They change the same X, Y, Scale and Angle
//! the window shows.

use egui::{Pos2, Sense, Stroke, Vec2 as Screen, vec2};
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::dialogs_insert_drawing::{Align, MAX_SCALE, Place};
use crate::viewport::Proj;

/// Half the side of a corner handle, in screen points.
const HANDLE: f32 = 6.0;
/// How far above the box the turning knob sits, in screen points.
const KNOB_GAP: f32 = 28.0;
/// Smallest scale a drag makes.
const MIN_SCALE: f64 = 0.0001;

/// What a drag took hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Move,
    /// A corner, by its signs in the box's own frame.
    Corner(f64, f64),
    Turn,
}

/// A drag in progress.
#[derive(Clone, Debug)]
pub struct Drag {
    part: Part,
    /// The pointer on the sketch where it began, and the placement then.
    from: Vec2,
    at: (f64, f64, f64, f64),
}

/// The box's half sizes (mm), unturned.
fn half(p: &Place) -> Option<Vec2> {
    let (w, h) = p.natural?;
    Some(Vec2::new(w * p.scale / 2.0, h * p.scale / 2.0))
}

fn turn(v: Vec2, degrees: f64) -> Vec2 {
    let (s, c) = degrees.to_radians().sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Where the box's middle is from the point X and Y place, in the file's units before scaling and
/// turning: nothing when the middle is placed, the middle itself when the file's origin is (#22057).
fn offset(p: &Place) -> Option<Vec2> {
    match p.align {
        Align::Middle => Some(Vec2::ZERO),
        Align::Origin => p.middle.map(|(x, y)| Vec2::new(x, y)),
    }
}

/// A spot of the box in its own frame (`u`, `v` in half sizes) on the sketch.
fn spot(p: &Place, u: f64, v: f64) -> Option<Vec2> {
    let h = half(p)?;
    let off = offset(p)?;
    Some(Vec2::new(p.x, p.y) + turn(Vec2::new(off.x * p.scale + u * h.x, off.y * p.scale + v * h.y), p.angle))
}

fn on_screen(plane: &Plane, proj: &Proj, q: Vec2) -> Option<Pos2> {
    proj.to_screen(plane.to_world(q))
}

fn pointer_on_sketch(plane: &Plane, proj: &Proj, at: Pos2) -> Option<Vec2> {
    let (o, d) = proj.ray(at);
    plane.intersect_ray(o, d).map(|w| plane.to_local(w))
}

fn place_of(app: &SolveApp) -> Option<Place> {
    match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::InsertDrawing(p)) => Some(p.clone()),
        _ => None,
    }
}

/// The placement window's drawing, with its handles. A drag on them edits the window's values.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, proj: &Proj) {
    let Some(place) = place_of(app) else {
        app.viewport.insert_drag = None;
        return;
    };
    let Some(plane) = app.session.active_sketch.and_then(|id| crate::canvas::profile_plane(app, id)) else { return };
    let Some(corners) = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|(u, v)| spot(&place, *u, *v).and_then(|q| on_screen(&plane, proj, q)))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let (Some(top), Some(knob_from)) =
        (spot(&place, 0.0, 1.0).and_then(|q| on_screen(&plane, proj, q)), spot(&place, 0.0, 0.0).and_then(|q| on_screen(&plane, proj, q)))
    else {
        return;
    };
    let away = (top - knob_from).normalized();
    let knob = top + if away.length() > 0.0 { away * KNOB_GAP } else { vec2(0.0, -KNOB_GAP) };
    let t = solidcraft_theme::Tokens::get();
    let painter = ui.painter().clone();
    let stroke = Stroke::new(1.5, t.accent);
    painter.add(egui::Shape::closed_line(corners.clone(), stroke));
    painter.line_segment([top, knob], stroke);
    painter.circle(knob, HANDLE, t.overlay, stroke);

    // Whole box first, so the corners and the knob above it win where they overlap.
    let mut parts: Vec<(egui::Rect, Part, egui::CursorIcon)> = Vec::new();
    let bounds = egui::Rect::from_points(&corners);
    parts.push((bounds, Part::Move, egui::CursorIcon::Grab));
    for (c, (u, v)) in corners.iter().zip([(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]) {
        painter.rect(egui::Rect::from_center_size(*c, Screen::splat(HANDLE * 2.0)), 1.0, t.overlay, stroke, egui::StrokeKind::Inside);
        parts.push((egui::Rect::from_center_size(*c, Screen::splat(HANDLE * 3.0)), Part::Corner(u, v), egui::CursorIcon::ResizeNwSe));
    }
    parts.push((egui::Rect::from_center_size(knob, Screen::splat(HANDLE * 3.0)), Part::Turn, egui::CursorIcon::Crosshair));
    for (i, (rect, part, cursor)) in parts.into_iter().enumerate() {
        let response = ui.interact(rect, egui::Id::unique(("sc_insert_handle", i)), Sense::drag()).on_hover_cursor(cursor);
        crate::scenario::publish_handle(&format!("insert:handle:{i}"), rect.center());
        if response.drag_started()
            && let Some(from) = response.interact_pointer_pos().and_then(|p| pointer_on_sketch(&plane, proj, p))
        {
            app.viewport.insert_drag = Some(Drag { part, from, at: (place.x, place.y, place.scale, place.angle) });
        }
        // The release frame carries the last move too: it counts, or a quick drag stops short.
        if (response.dragged() || response.drag_stopped())
            && let Some(drag) = app.viewport.insert_drag.clone()
            && drag.part == part
            && let Some(p) =
                response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos())).and_then(|p| pointer_on_sketch(&plane, proj, p))
        {
            let snap = ui.input(|i| i.modifiers.shift);
            if let Some(Kind::InsertDrawing(target)) = app.dialog.as_mut().map(|d| &mut d.kind) {
                apply(target, &drag, p, snap);
            }
        }
        if response.drag_stopped() {
            app.viewport.insert_drag = None;
        }
    }
}

/// The placement after the pointer has moved to `p`, from where `drag` began.
fn apply(target: &mut Place, drag: &Drag, p: Vec2, snap: bool) {
    let (x, y, scale, angle) = drag.at;
    let start = Place { x, y, scale, angle, ..target.clone() };
    match drag.part {
        Part::Move => {
            target.x = x + (p.x - drag.from.x);
            target.y = y + (p.y - drag.from.y);
        }
        Part::Turn => {
            let to = p - Vec2::new(x, y);
            let from = drag.from - Vec2::new(x, y);
            if to.len() > 1e-9 {
                let a = match start.align {
                    // The knob is straight above the point it turns about: it points the way up goes.
                    Align::Middle => to.angle().to_degrees() - 90.0,
                    // It turns about the file's origin, away from the knob: by as much as the
                    // pointer has gone round that point since the drag began.
                    Align::Origin if from.len() > 1e-9 => angle + (to.angle() - from.angle()).to_degrees(),
                    Align::Origin => return,
                };
                let a = if snap { (a / 15.0).round() * 15.0 } else { a };
                // Into -180..180, the range the field takes.
                target.angle = (a + 180.0).rem_euclid(360.0) - 180.0;
            }
        }
        Part::Corner(u, v) => {
            let (Some((nw, nh)), Some(fixed)) = (start.natural, spot(&start, -u, -v)) else { return };
            // The pointer in the box's own frame, measured from the corner that stays.
            let local = turn(p - fixed, -angle);
            let k = (local.x.abs() / nw).max(local.y.abs() / nh).clamp(MIN_SCALE, MAX_SCALE);
            let Some(off) = offset(&start) else { return };
            if k.is_finite() {
                // From the corner that stays back to the point X and Y place, at the new scale.
                let reach = turn(Vec2::new(k * (u * nw / 2.0 - off.x), k * (v * nh / 2.0 - off.y)), angle);
                target.scale = k;
                target.x = fixed.x + reach.x;
                target.y = fixed.y + reach.y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> Place {
        Place {
            command: "sketch.insert_svg".into(),
            path: "a.svg".into(),
            x: 10.0,
            y: 5.0,
            scale: 1.0,
            angle: 0.0,
            natural: Some((20.0, 10.0)),
            dpi: 96.0,
            measured_dpi: 96.0,
            middle: Some((40.0, 25.0)),
            align: Align::Middle,
            simplify: 0.0,
            auto: false,
            auto_value: None,
        }
    }

    fn drag(part: Part, from: Vec2, p: &Place) -> Drag {
        Drag { part, from, at: (p.x, p.y, p.scale, p.angle) }
    }

    /// A corner drag scales about the opposite corner, which does not move.
    #[test]
    fn a_corner_scales_about_the_opposite_corner() {
        let mut p = place();
        let fixed = spot(&p, -1.0, -1.0).unwrap();
        let d = drag(Part::Corner(1.0, 1.0), spot(&p, 1.0, 1.0).unwrap(), &p);
        // The corner at (20, 10) goes to (40, 20) from the fixed one at (0, 0): twice the size.
        apply(&mut p, &d, Vec2::new(40.0, 20.0), false);
        assert!((p.scale - 2.0).abs() < 1e-9, "{}", p.scale);
        let now = spot(&p, -1.0, -1.0).unwrap();
        assert!(now.dist(fixed) < 1e-9, "the opposite corner stayed: {now:?} vs {fixed:?}");
    }

    #[test]
    fn a_turned_corner_keeps_its_opposite_corner_too() {
        let mut p = Place { angle: 30.0, ..place() };
        let fixed = spot(&p, 1.0, -1.0).unwrap();
        let d = drag(Part::Corner(-1.0, 1.0), spot(&p, -1.0, 1.0).unwrap(), &p);
        let pulled = spot(&p, -1.0, 1.0).unwrap() + turn(Vec2::new(-10.0, 5.0), 30.0);
        apply(&mut p, &d, pulled, false);
        assert!(p.scale > 1.0);
        assert!(spot(&p, 1.0, -1.0).unwrap().dist(fixed) < 1e-9);
    }

    #[test]
    fn dragging_the_box_moves_it_and_the_knob_turns_it() {
        let mut p = place();
        let d = drag(Part::Move, Vec2::new(10.0, 5.0), &p);
        apply(&mut p, &d, Vec2::new(14.0, 1.0), false);
        assert_eq!((p.x, p.y), (14.0, 1.0));
        // The knob is straight above the middle: dragging it to the left turns a quarter.
        let d = drag(Part::Turn, Vec2::new(14.0, 10.0), &p);
        apply(&mut p, &d, Vec2::new(4.0, 1.0), false);
        assert!((p.angle - 90.0).abs() < 1e-9, "{}", p.angle);
        apply(&mut p, &d, Vec2::new(4.0, 3.0), true);
        assert!(p.angle == 90.0 || (p.angle % 15.0).abs() < 1e-9, "snapped: {}", p.angle);
    }

    /// #22057: placed by the file's origin, the box stands where the drawing is in the file, moved by
    /// X and Y; a corner still scales about the opposite corner, and the knob turns it about the origin.
    #[test]
    fn placed_by_its_origin_the_box_is_where_the_drawing_is_and_scales_and_turns_about_the_origin() {
        let mut p = Place { align: Align::Origin, x: 0.0, y: 0.0, ..place() };
        // The drawing spans 30..50 by 20..30 in its file: its middle is (40, 25).
        assert!(spot(&p, 0.0, 0.0).unwrap().dist(Vec2::new(40.0, 25.0)) < 1e-9);
        assert!(spot(&p, -1.0, -1.0).unwrap().dist(Vec2::new(30.0, 20.0)) < 1e-9);
        let fixed = spot(&p, -1.0, -1.0).unwrap();
        let d = drag(Part::Corner(1.0, 1.0), spot(&p, 1.0, 1.0).unwrap(), &p);
        apply(&mut p, &d, Vec2::new(70.0, 40.0), false);
        assert!((p.scale - 2.0).abs() < 1e-9, "{}", p.scale);
        assert!(spot(&p, -1.0, -1.0).unwrap().dist(fixed) < 1e-9, "the opposite corner stayed");
        // The origin moves to (-30, -20), so the middle, twice (40, 25) from it, is at (50, 30).
        assert!(Vec2::new(p.x, p.y).dist(Vec2::new(-30.0, -20.0)) < 1e-9, "{} {}", p.x, p.y);
        // The knob turns the drawing about the origin by as much as the pointer goes round it.
        let mut q = Place { align: Align::Origin, x: 0.0, y: 0.0, ..place() };
        let d = drag(Part::Turn, Vec2::new(10.0, 0.0), &q);
        apply(&mut q, &d, Vec2::new(0.0, 10.0), false);
        assert!((q.angle - 90.0).abs() < 1e-9, "{}", q.angle);
        assert_eq!((q.x, q.y), (0.0, 0.0), "the origin stays put");
    }
}
