//! The move/rotate triad shared by Move/Copy and occurrence moves: X/Y/Z arrows, XY/YZ/XZ plane
//! squares, X/Y/Z rotation rings and a centre ball for a free drag in the view plane. It is a
//! constant size on screen and lies along any axes it is given. A drag reports its change since
//! the drag began (distances snap to round steps at the current zoom, angles to 5°; hold Alt or
//! Ctrl for a free drag), so the caller applies it to the values it had when the drag started:
//! [`ring_order`] says how a turn goes on top of the angles.

use std::cell::RefCell;

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, vec2};
use solidcraft_engine::geom::{Ring, Vec3};

use crate::theme::Tokens;
use crate::viewport::{Proj, projected_polygon};

/// A part of the triad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    /// Arrow along X, Y or Z (0, 1, 2).
    Axis(usize),
    /// Square in the plane across that axis (Plane(2) is XY).
    Plane(usize),
    /// Ring turning about that axis.
    Ring(usize),
    /// Centre ball: free drag in the view plane.
    Ball,
}

/// What a drag has done so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Translate(Vec3),
    /// About the triad's X, Y or Z through the centre, in radians.
    Rotate {
        axis: usize,
        angle: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub handle: Handle,
    pub change: Change,
    /// The first frame of the drag (take a snapshot of the values now).
    pub started: bool,
    /// The drag ended this frame.
    pub done: bool,
}

/// Where the triad is and which handles it shows.
#[derive(Clone, Copy, Debug)]
pub struct Triad {
    pub center: Vec3,
    /// Its X, Y and Z in the world (the columns).
    pub axes: Axes,
    pub translate: bool,
    pub rotate: bool,
}

#[derive(Clone, Copy, Debug)]
struct State {
    handle: Handle,
    center: Vec3,
    /// The axes when the drag began: a ring turns them, and the drag stays on the ones it took.
    axes: Axes,
    /// Grab point on the plane (planes, ball) or parameter along the axis.
    grab: Vec3,
    t0: f64,
    /// Ring: last angle seen and the angle turned so far.
    last: f64,
    turned: f64,
}

/// Three directions as the columns of a rotation (rows, as [`rotation`] gives them).
pub type Axes = [[f64; 3]; 3];

/// The world's X, Y and Z.
pub const WORLD: Axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The default order of a move's angles, left to right in the product (see [`turn_of`]): about
/// X first, then Y, then Z.
pub const ANGLE_ORDER: [usize; 3] = [2, 1, 0];

/// Arrow length on screen (px).
pub const LEN_PX: f64 = 110.0;

/// How near a handle a press still takes it (px): the widest, the centre ball.
pub const HIT_TOLERANCE: f32 = 9.0;

thread_local! {
    /// The last triad drawn: centre, arrow length in mm and axes (for scenarios and tests).
    static LAST: RefCell<Option<(Vec3, f64, Axes)>> = const { RefCell::new(None) };
}

/// A world point on a handle of the last triad drawn: "x"/"y"/"z" on an arrow, "xy"/"yz"/"xz"
/// in a square, "rx"/"ry"/"rz" on a ring, "ball". `by` moves it, `turn` (degrees) turns a ring
/// point about its axis.
pub fn handle_point(name: &str, by: Vec3, turn: f64) -> Option<Vec3> {
    let (c, l, axes) = LAST.with(|x| *x.borrow())?;
    let [ex, ey, ez] = [0, 1, 2].map(|i| axis(&axes, i));
    let index = |s: &str| match s {
        "x" => Some(0),
        "y" => Some(1),
        "z" => Some(2),
        _ => None,
    };
    let p = match name {
        "ball" => c,
        "xy" => c + (ex + ey) * (0.35 * l),
        "yz" => c + (ey + ez) * (0.35 * l),
        "xz" => c + (ex + ez) * (0.35 * l),
        r if r.starts_with('r') => {
            let i = index(&r[1..])?;
            let (e1, e2) = ring_basis(&axes, i);
            let th = turn.to_radians();
            return Some(c + (e1 * th.cos() + e2 * th.sin()) * (RING * l) + by);
        }
        a => c + axis(&axes, index(a)?) * (0.75 * l),
    };
    Some(p + by)
}

/// Ring radius as a share of the arrow length.
const RING: f64 = 0.62;

/// In-plane directions of the ring about an axis (right-handed: e1 × e2 = axis).
fn ring_basis(axes: &Axes, i: usize) -> (Vec3, Vec3) {
    match i {
        0 => (axis(axes, 1), axis(axes, 2)),
        1 => (axis(axes, 2), axis(axes, 0)),
        _ => (axis(axes, 0), axis(axes, 1)),
    }
}

/// Where round a ring (degrees) a press takes that ring, clear of the handles over it.
#[cfg(test)]
pub(crate) fn ring_grip(proj: &Proj, t: &Triad, ring: usize) -> Option<f64> {
    let l = mm_per_px(proj, t.center)? * LEN_PX;
    let sh = shapes(proj, t, l)?;
    let (e1, e2) = ring_basis(&t.axes, ring);
    (0..72).map(|k| f64::from(k) * 5.0).find(|a| {
        let th = a.to_radians();
        proj.to_screen(t.center + (e1 * th.cos() + e2 * th.sin()) * (RING * l)).is_some_and(|p| hit(&sh, p) == Some(Handle::Ring(ring)))
    })
}

/// Distance from `p` to the screen segment a–b.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

fn in_quad(p: Pos2, q: &Ring<Pos2>) -> bool {
    if q.len() < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for (&a, &b) in q.edges() {
        let c = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if c.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

/// Where the view ray through `pos` meets the plane through `o` with normal `n`.
pub(crate) fn on_plane(proj: &Proj, pos: Pos2, o: Vec3, n: Vec3) -> Option<Vec3> {
    let (ro, rd) = proj.ray(pos);
    let den = rd.dot(n);
    if den.abs() < 1e-9 {
        return None;
    }
    Some(ro + rd * ((o - ro).dot(n) / den))
}

/// Parameter along the line `o + d t` closest to the view ray through `pos`.
fn along(proj: &Proj, pos: Pos2, o: Vec3, d: Vec3) -> Option<f64> {
    let (ro, r) = proj.ray(pos);
    let w0 = o - ro;
    let (b, dd, e) = (d.dot(r), d.dot(w0), r.dot(w0));
    let den = 1.0 - b * b;
    if den.abs() < 1e-6 {
        return None;
    }
    Some((b * e - dd) / den)
}

/// The triad's shapes: (handle, screen polygon or polyline, is it a filled area).
struct Shapes {
    arrows: Vec<(usize, Pos2, Pos2)>,
    planes: Vec<(usize, Ring<Pos2>)>,
    rings: Vec<(usize, Vec<Pos2>)>,
    ball: Pos2,
}

fn shapes(proj: &Proj, t: &Triad, l: f64) -> Option<Shapes> {
    let c = t.center;
    let ball = proj.to_screen(c)?;
    let mut s = Shapes { arrows: Vec::new(), planes: Vec::new(), rings: Vec::new(), ball };
    if t.translate {
        for i in 0..3 {
            let d = axis(&t.axes, i);
            if let (Some(a), Some(b)) = (proj.to_screen(c + d * (0.18 * l)), proj.to_screen(c + d * l)) {
                s.arrows.push((i, a, b));
            }
        }
        for i in 0..3 {
            let (e1, e2) = ring_basis(&t.axes, i);
            let q: Ring<Pos2> = [(0.25, 0.25), (0.45, 0.25), (0.45, 0.45), (0.25, 0.45)]
                .iter()
                .filter_map(|(a, b)| proj.to_screen(c + e1 * (a * l) + e2 * (b * l)))
                .collect();
            if q.len() == 4 {
                s.planes.push((i, q));
            }
        }
    }
    if t.rotate {
        for i in 0..3 {
            let (e1, e2) = ring_basis(&t.axes, i);
            let pts: Vec<Pos2> = (0..=64)
                .map(|k| k as f64 / 64.0 * std::f64::consts::TAU)
                .filter_map(|th| proj.to_screen(c + (e1 * th.cos() + e2 * th.sin()) * (RING * l)))
                .collect();
            if pts.len() > 2 {
                s.rings.push((i, pts));
            }
        }
    }
    Some(s)
}

fn hit(s: &Shapes, p: Pos2) -> Option<Handle> {
    if s.ball.distance(p) < HIT_TOLERANCE {
        return Some(Handle::Ball);
    }
    if let Some((i, ..)) = s.arrows.iter().find(|(_, a, b)| seg_dist(p, *a, *b) < 6.0) {
        return Some(Handle::Axis(*i));
    }
    if let Some((i, _)) = s.planes.iter().find(|(_, q)| in_quad(p, q)) {
        return Some(Handle::Plane(*i));
    }
    // From some views two rings lie almost on top of each other: the nearest takes the press.
    let near = |pts: &Vec<Pos2>| pts.windows(2).map(|w| seg_dist(p, w[0], w[1])).fold(f32::MAX, f32::min);
    s.rings.iter().map(|(i, pts)| (*i, near(pts))).filter(|r| r.1 < 5.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| Handle::Ring(i))
}

/// Millimetres per screen pixel at a point.
fn mm_per_px(proj: &Proj, c: Vec3) -> Option<f64> {
    let (_, view) = proj.ray(proj.to_screen(c)?);
    let u = view.any_perp().normalized()?;
    let (a, b) = (proj.to_screen(c)?, proj.to_screen(c + u)?);
    let px = f64::from(a.distance(b));
    (px > 1e-9).then(|| 1.0 / px)
}

/// Radius shared by the triad rings and the selected-axis Rotate handle.
pub(crate) fn ring_radius(proj: &Proj, centre: Vec3) -> Option<f64> {
    Some(mm_per_px(proj, centre)? * LEN_PX * RING)
}

fn snap(v: f64, step: f64, free: bool) -> f64 {
    if free || step <= 0.0 { v } else { (v / step).round() * step }
}

/// Discard a suspended pointer gesture without reporting a final model change.
pub(crate) fn cancel(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.remove::<State>(id));
}

/// Hit-test before registering the dynamic egui drag widget or changing the camera.
pub(crate) fn contains_pointer(proj: &Proj, t: &Triad, p: Pos2) -> bool {
    mm_per_px(proj, t.center).and_then(|scale| shapes(proj, t, scale * LEN_PX)).is_some_and(|sh| hit(&sh, p).is_some())
}

/// The region the triad's handles occupy on screen: where a press takes one of them. A value
/// box over it takes the press before the gizmo sees it (#6608).
pub fn handles_region(proj: &Proj, t: &Triad) -> Option<egui::Rect> {
    let l = mm_per_px(proj, t.center)? * LEN_PX;
    let sh = shapes(proj, t, l)?;
    let mut pts = vec![sh.ball];
    for (_, a, b) in &sh.arrows {
        pts.push(*a);
        pts.push(*b);
    }
    for (_, ring) in &sh.rings {
        pts.extend(ring.iter().copied());
    }
    for (_, q) in &sh.planes {
        for (&a, &b) in q.edges() {
            pts.push(a);
            pts.push(b);
        }
    }
    let mut min = sh.ball;
    let mut max = sh.ball;
    for p in pts {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    Some(egui::Rect::from_min_max(min, max).expand(HIT_TOLERANCE))
}

/// Draw the triad and run a drag on it. `step` is the distance snap step.
pub fn show(ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, id: egui::Id, t: &Triad, step: f64) -> Option<Drag> {
    let tk = Tokens::get();
    let l = mm_per_px(proj, t.center)? * LEN_PX;
    LAST.with(|x| *x.borrow_mut() = Some((t.center, l, t.axes)));
    let sh = shapes(proj, t, l)?;
    // Handles by name for scenarios ("move_x", "move_rz", "move_ball"…).
    for n in ["x", "y", "z", "xy", "yz", "xz", "rx", "ry", "rz", "ball"] {
        if let Some(q) = handle_point(n, Vec3::ZERO, 60.0).and_then(|p| proj.to_screen(p)) {
            crate::scenario::publish_handle(&format!("move_{n}"), q);
        }
    }
    let state: Option<State> = ui.data(|d| d.get_temp(id));
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hovered = if state.is_none() { pointer.filter(|_| ui.rect_contains_pointer(proj.rect)).and_then(|p| hit(&sh, p)) } else { None };
    let active = state.map(|s| s.handle).or(hovered);
    let hot = |h: Handle| active == Some(h);
    let col = |h: Handle, i: usize| if hot(h) { tk.accent } else { tk.axes()[i.min(2)] };
    for (i, pts) in &sh.rings {
        painter.add(Shape::line(pts.clone(), Stroke::new(if hot(Handle::Ring(*i)) { 3.0 } else { 1.8 }, col(Handle::Ring(*i), *i))));
    }
    for (i, q) in &sh.planes {
        let c = col(Handle::Plane(*i), *i);
        painter.extend(projected_polygon(q, c.gamma_multiply(if hot(Handle::Plane(*i)) { 0.7 } else { 0.35 }), Stroke::new(1.0, c)));
    }
    for (i, a, b) in &sh.arrows {
        arrow(painter, *a, *b, col(Handle::Axis(*i), *i), hot(Handle::Axis(*i)));
    }
    let bc = if hot(Handle::Ball) { tk.accent } else { Color32::from_gray(235) };
    painter.circle(sh.ball, 6.0, bc, Stroke::new(1.5, Color32::from_gray(60)));
    let p = pointer?;
    active?;
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    let r = ui.interact(Rect::from_center_size(p, vec2(24.0, 24.0)), id.with("hit"), Sense::drag());
    // Free drag is driven by the per-frame thread-local set from prefs::manipulator_free_held.
    let free = crate::tool_modifiers::manipulator_free();
    if state.is_some() || r.dragged() || r.drag_started() {
        crate::tool_modifiers::set_manipulator_dragging(true);
    }
    let view = proj.ray(sh.ball).1;
    let axes = state.map_or(t.axes, |s| s.axes);
    let plane_normal = |h: Handle| match h {
        Handle::Plane(i) | Handle::Ring(i) => axis(&axes, i),
        _ => view,
    };
    let mut started = false;
    let mut st = match state {
        Some(s) => s,
        None => {
            if !r.drag_started() {
                return None;
            }
            let h = hovered?;
            let from = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
            let c = t.center;
            let mut s = State { handle: h, center: c, axes, grab: c, t0: 0.0, last: 0.0, turned: 0.0 };
            match h {
                Handle::Axis(i) => s.t0 = along(proj, from, c, axis(&axes, i))?,
                Handle::Ring(i) => {
                    let q = on_plane(proj, from, c, axis(&axes, i))? - c;
                    let (e1, e2) = ring_basis(&axes, i);
                    s.last = q.dot(e2).atan2(q.dot(e1));
                }
                _ => s.grab = on_plane(proj, from, c, plane_normal(h))?,
            }
            started = true;
            s
        }
    };
    let done = r.drag_stopped() || !r.dragged() && !started;
    // The frame's events can hold the release and further movement — the hand keeps moving as
    // the button goes up, or input queued during a busy frame — and `p` is already past the
    // release then. The release event itself carries where the button was let go (#8265): take
    // the final change from there, so the drag ends where the button was released, not where
    // the mouse has since got to.
    let p = if done {
        ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::PointerButton { pos, pressed: false, .. } => Some(*pos),
                _ => None,
            })
        })
        .unwrap_or(p)
    } else {
        p
    };
    let change = match st.handle {
        Handle::Axis(i) => {
            let v = along(proj, p, st.center, axis(&axes, i)).map(|v| v - st.t0).unwrap_or(0.0);
            Change::Translate(axis(&axes, i) * snap(v, step, free))
        }
        Handle::Ring(i) => {
            if let Some(q) = on_plane(proj, p, st.center, axis(&axes, i)).map(|q| q - st.center) {
                let (e1, e2) = ring_basis(&axes, i);
                let th = q.dot(e2).atan2(q.dot(e1));
                let mut d = th - st.last;
                while d > std::f64::consts::PI {
                    d -= std::f64::consts::TAU;
                }
                while d < -std::f64::consts::PI {
                    d += std::f64::consts::TAU;
                }
                st.turned += d;
                st.last = th;
            }
            Change::Rotate { axis: i, angle: snap(st.turned, 5f64.to_radians(), free) }
        }
        h => {
            let d = on_plane(proj, p, st.center, plane_normal(h)).map(|q| q - st.grab).unwrap_or(Vec3::ZERO);
            Change::Translate((0..3).fold(Vec3::ZERO, |sum, i| sum + axis(&axes, i) * snap(d.dot(axis(&axes, i)), step, free)))
        }
    };
    if done {
        ui.data_mut(|d| d.remove::<State>(id));
    } else {
        ui.data_mut(|d| d.insert_temp(id, st));
    }
    Some(Drag { handle: st.handle, change, started, done })
}

fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, col: Color32, hot: bool) {
    let v = b - a;
    let len = v.length();
    if len < 1.0 {
        return;
    }
    let u = v / len;
    let n = vec2(-u.y, u.x);
    painter.line_segment([a, b], Stroke::new(if hot { 3.0 } else { 2.0 }, col));
    let head = 12.0f32.min(len * 0.5);
    painter.add(Shape::convex_polygon(vec![b + u * 2.0, b - u * head + n * head * 0.4, b - u * head - n * head * 0.4], col, Stroke::NONE));
}

/// A 3×3 rotation (rows) about a unit axis.
pub fn rotation(axis: Vec3, angle: f64) -> [[f64; 3]; 3] {
    let (c, s) = (angle.cos(), angle.sin());
    let t = 1.0 - c;
    let (x, y, z) = (axis.x, axis.y, axis.z);
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}

pub fn mul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub fn apply(m: &[[f64; 3]; 3], p: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z,
        m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z,
        m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z,
    )
}

/// Direction `i` (X, Y or Z) of `axes`.
pub fn axis(axes: &Axes, i: usize) -> Vec3 {
    let i = i.min(2);
    Vec3::new(axes[0][i], axes[1][i], axes[2][i])
}

/// The opposite turn.
pub fn inverse(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| m[c][r]))
}

/// The same turn with its axes laid along `frame`, in the world.
pub fn turn_in(frame: &Axes, m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    mul(&mul(frame, m), &inverse(frame))
}

/// The turn three angles about X, Y and Z make, as the product in `order`: the rightmost goes
/// first. Read the other way round, each turns about the axes the ones left of it leave.
pub fn turn_of(a: [f64; 3], order: [usize; 3]) -> [[f64; 3]; 3] {
    order.iter().fold(WORLD, |m, i| mul(&m, &rotation(axis(&WORLD, *i), a[(*i).min(2)])))
}

/// The turn a move's X, Y and Z angles make in the default order.
pub fn angles_rotation(a: [f64; 3]) -> [[f64; 3]; 3] {
    turn_of(a, ANGLE_ORDER)
}

/// The X, Y and Z angles that make a turn (see [`angles_rotation`]).
pub fn rotation_angles(m: &[[f64; 3]; 3]) -> [f64; 3] {
    // R = Rz(rz) * Ry(ry) * Rx(rx): row 2 is (-sin ry, cos ry sin rx, cos ry cos rx).
    let ry = (-m[2][0]).clamp(-1.0, 1.0).asin();
    if m[2][1].abs() + m[2][2].abs() < 1e-9 {
        // Y at a quarter turn: only rx + rz shows; keep rz at zero.
        [(-m[1][2]).atan2(m[1][1]), ry, 0.0]
    } else {
        [m[2][1].atan2(m[2][2]), ry, m[1][0].atan2(m[0][0])]
    }
}

/// An angle within one turn of zero, keeping its sign; a full turn is 0.
pub fn within_turn(a: f64) -> f64 {
    let r = a % std::f64::consts::TAU;
    if std::f64::consts::TAU - r.abs() < 1e-6 { 0.0 } else { r }
}

/// The order that lets a drag on `ring` add to its own angle, so every angle stays what was
/// dragged. A ring that turns the triad with the bodies (`local`) adds at the right end of the
/// product, one that holds still at the left. A ring whose angle is still zero can go to that
/// end; `None` when it has an angle and a later turn lies on top of it, and no sum of angles
/// makes the turn: the turns so far have to be kept and its angle start again.
pub fn ring_order(angles: [f64; 3], order: [usize; 3], ring: usize, local: bool) -> Option<[usize; 3]> {
    let i = ring.min(2);
    let at = order.iter().position(|o| *o == i)?;
    let beyond = if local { &order[at + 1..] } else { &order[..at] };
    if beyond.iter().all(|o| angles[(*o).min(2)].abs() < 1e-12) {
        return Some(order);
    }
    if angles[i].abs() >= 1e-12 {
        return None;
    }
    let mut rest = order.into_iter().filter(|o| *o != i);
    let [a, b] = [rest.next()?, rest.next()?];
    Some(if local { [a, b, i] } else { [i, a, b] })
}

/// The axis and angle of a rotation (angle 0 for none).
pub fn axis_angle(m: &[[f64; 3]; 3]) -> (Vec3, f64) {
    let tr = m[0][0] + m[1][1] + m[2][2];
    let angle = ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos();
    if angle < 1e-12 {
        return (Vec3::Z, 0.0);
    }
    let v = Vec3::new(m[2][1] - m[1][2], m[0][2] - m[2][0], m[1][0] - m[0][1]);
    if let Some(a) = v.normalized().filter(|_| v.len() > 1e-6) {
        return (a, angle);
    }
    // Half a turn: the axis from the diagonal.
    let d = [((m[0][0] + 1.0) / 2.0).max(0.0).sqrt(), ((m[1][1] + 1.0) / 2.0).max(0.0).sqrt(), ((m[2][2] + 1.0) / 2.0).max(0.0).sqrt()];
    let (x, mut y, mut z) = (d[0], d[1], d[2]);
    if m[0][1] + m[1][0] < 0.0 && x > 1e-6 {
        y = -y;
    }
    if m[0][2] + m[2][0] < 0.0 && x > 1e-6 {
        z = -z;
    }
    if x <= 1e-6 && m[1][2] + m[2][1] < 0.0 {
        z = -z;
    }
    (Vec3::new(x, y, z).normalized().unwrap_or(Vec3::Z), angle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_squares_stay_within_the_triad() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let t = Triad { center: Vec3::ZERO, axes: WORLD, translate: true, rotate: false };
        // The squares reach 0.45 √2 of the arrow length from the centre, plus stroke and feathering.
        let limit = (0.45 * 2f64.sqrt() * LEN_PX) as f32 + 3.0;
        let mut worst = 0.0f32;
        for dir in crate::viewport::orbit_directions() {
            let cam = solidcraft_engine::render::Camera::default().looking_from(dir);
            let proj = Proj { cam, vp: cam.view_proj(4.0 / 3.0, 100.0), rect };
            let l = mm_per_px(&proj, t.center).unwrap() * LEN_PX;
            let sh = shapes(&proj, &t, l).unwrap();
            for (_, q) in &sh.planes {
                for p in crate::viewport::drawn_points(projected_polygon(q, Color32::GRAY, Stroke::new(1.0, Color32::BLACK))) {
                    worst = worst.max(p.distance(sh.ball));
                }
            }
        }
        assert!(worst <= limit, "a vertex {worst} px from the centre (limit {limit})");
    }

    /// An angle adds to what it was exactly when the product can take it at the ring's end, and
    /// then the turn is about the axis the ring shows: the one the turns leave it on (local), or
    /// the world's (fixed).
    #[test]
    fn a_drag_adds_to_its_own_angle_and_turns_about_the_ring_shown() {
        let on = |m: &[[f64; 3]; 3], o: &[[f64; 3]; 3]| m.iter().flatten().zip(o.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-9);
        for (a, order) in
            [([0.3, 0.0, 0.0], ANGLE_ORDER), ([0.0, 0.0, 1.5], ANGLE_ORDER), ([0.3, 0.7, 0.0], [2, 0, 1]), ([0.3, 0.7, 1.1], ANGLE_ORDER)]
        {
            for ring in 0..3 {
                for local in [true, false] {
                    let Some(next) = ring_order(a, order, ring, local) else { continue };
                    let mut after = a;
                    after[ring] += 0.4;
                    let by = rotation(axis(&WORLD, ring), 0.4);
                    let before = turn_of(a, order);
                    let want = if local { mul(&before, &by) } else { mul(&by, &before) };
                    assert!(on(&turn_of(after, next), &want), "{a:?} {order:?} ring {ring} local {local}");
                }
            }
        }
    }

    /// The second angle dragged reads what was dragged, whichever came first. Only going back to
    /// an angle with a later one on top has to start over.
    #[test]
    fn only_going_back_under_a_later_turn_starts_an_angle_over() {
        for local in [true, false] {
            for (first, second) in [(0, 1), (1, 0), (2, 1), (1, 2), (0, 2), (2, 0)] {
                let mut a = [0.0; 3];
                a[first] = 1.5;
                let order = ring_order(a, ANGLE_ORDER, second, local).unwrap();
                a[second] = 0.4;
                assert!(ring_order(a, order, second, local).is_some(), "the last ring turned goes on adding");
                assert!(ring_order(a, order, first, local).is_none(), "{first} lies under {second}");
            }
        }
    }

    #[test]
    fn a_turn_in_a_frame_is_about_that_frames_axis() {
        let frame = rotation(Vec3::X, std::f64::consts::FRAC_PI_2);
        let m = turn_in(&frame, &rotation(Vec3::Z, 0.4));
        assert!((apply(&m, axis(&frame, 2)) - axis(&frame, 2)).len() < 1e-12);
        assert_eq!(turn_of([0.3, -0.7, 1.1], ANGLE_ORDER), angles_rotation([0.3, -0.7, 1.1]));
    }

    #[test]
    fn rotations_compose_to_one_axis_angle() {
        let r = mul(&rotation(Vec3::Z, 0.7), &rotation(Vec3::X, 0.4));
        let (a, ang) = axis_angle(&r);
        let back = rotation(a, ang);
        for i in 0..3 {
            for j in 0..3 {
                assert!((back[i][j] - r[i][j]).abs() < 1e-9);
            }
        }
        let h = rotation(Vec3::Y, std::f64::consts::PI);
        let (a, ang) = axis_angle(&h);
        assert!((ang - std::f64::consts::PI).abs() < 1e-9 && (a.y.abs() - 1.0).abs() < 1e-9);
        assert!((apply(&rotation(Vec3::Z, std::f64::consts::FRAC_PI_2), Vec3::X) - Vec3::Y).len() < 1e-12);
    }
}
