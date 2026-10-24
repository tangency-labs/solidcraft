//! 3D sketching in the viewport (the Sketch Palette's 3D Sketch option): the Line, Arc, Spline
//! and Point tools take points off the sketch plane by snapping to model vertices, edges and
//! faces and to points already drawn in 3D; segments that stay on the plane are ordinary sketch
//! lines and arcs. The 3D Arc command has its own tool: every point it takes is a world point.
//! A drawn 3D point (or curve end) clicked without a tool gets a move triad:
//! drag an arrow to move along X, Y or Z, or the square to move parallel to the sketch plane.
//! Everything runs `sketch.line3d`, `sketch.arc3d`, `sketch.spline3d`, `sketch.point3d`,
//! `sketch.move3d`, `sketch.line` and `sketch.arc.three_point`.

use std::cell::RefCell;

use egui::{Pos2, Rect, Shape, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::geom::{Vec2, Vec3};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::tools::Tool;
use crate::viewport::{Hit, Proj, pick_3d};

thread_local! {
    /// Points of the curve being drawn in 3D (with the tool they belong to).
    static CHAIN: RefCell<(Option<&'static str>, Vec<Vec3>)> = const { RefCell::new((None, Vec::new())) };
    /// The drawn point with the triad: wire id and fit index.
    static TRIAD: RefCell<Option<(String, usize)>> = const { RefCell::new(None) };
    /// A triad drag: axis (None: in the sketch plane), undo depth, grab offset.
    static DRAG: RefCell<Option<(Option<Vec3>, usize, Vec3)>> = const { RefCell::new(None) };
    /// Where the cursor would put a point (for the rubber band), and its snap label.
    static HOVER: RefCell<Option<(Vec3, &'static str)>> = const { RefCell::new(None) };
}

/// Triad arrow length (screen px).
const ARROW: f32 = 60.0;

/// Is the sketch being edited a 3D sketch?
pub fn active(app: &SolveApp) -> bool {
    let st = app.session.world_state();
    app.session.active_sketch.and_then(|s| st.sketch(s)).is_some_and(|ss| ss.sketch.view.three_d)
}

fn takes(cmd: &str) -> bool {
    matches!(cmd, "sketch.line" | "sketch.arc.three_point" | "sketch.arc3d" | "sketch.spline.fit_point" | "sketch.point")
}

/// Only 3D point-placing tools broaden sketch-point picking; ordinary selection stays local.
pub fn drawing(app: &SolveApp) -> bool {
    active(app) && app.tool.as_ref().is_some_and(|tool| takes(tool.cmd))
}

/// Points drawn in 3D: (wire id, fit index, point).
fn drawn_points(app: &SolveApp) -> Vec<(String, usize, Vec3)> {
    let st = app.session.world_state();
    let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return Vec::new() };
    ss.sketch.wires.iter().flat_map(|w| w.fit.iter().enumerate().map(|(i, p)| (w.id.clone(), i, *p))).collect()
}

/// Closest point of segment a–b to the ray (o, d).
fn seg_ray_closest(a: Vec3, b: Vec3, o: Vec3, d: Vec3) -> Vec3 {
    let u = b - a;
    let w0 = a - o;
    let (aa, bb, cc) = (u.dot(u), u.dot(d), d.dot(d));
    let (dd, ee) = (u.dot(w0), d.dot(w0));
    let den = aa * cc - bb * bb;
    let s = if den.abs() < 1e-12 { 0.0 } else { ((bb * ee - cc * dd) / den).clamp(0.0, 1.0) };
    a + u * s
}

/// The 3D point under the cursor and what it snapped to: a drawn 3D point, a model vertex, a
/// point on a model edge or face, a sketch point, or the sketch plane.
pub fn point_at(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(Vec3, &'static str)> {
    for (_, _, p) in drawn_points(app) {
        if proj.to_screen(p).is_some_and(|s| s.distance(pos) < 8.0) {
            return Some((p, "Pt"));
        }
    }
    let st = app.session.world_state();
    let ss = app.session.active_sketch.and_then(|s| st.sketch(s))?;
    let (o, d) = proj.ray(pos);
    for h in pick_3d(app, proj, pos) {
        match h {
            Hit::SketchPoint { sketch, at, .. } => {
                if let Some(source) = st.sketch(sketch) {
                    return Some((source.plane.to_world(at), "Pt"));
                }
            }
            Hit::Vertex { point, .. } => return Some((point, "Vertex")),
            Hit::Edge { body, index, .. } => {
                let ws = app.session.world_state();
                let m = ws.body(&body)?.mesh();
                let e = m.edges.get(index)?;
                let best = e
                    .windows(2)
                    .map(|w| seg_ray_closest(w[0], w[1], o, d))
                    .min_by(|p, q| (*p - o).cross(d).len().total_cmp(&(*q - o).cross(d).len()))?;
                return Some((best, "Edge"));
            }
            Hit::Face { point, .. } => return Some((point, "Face")),
            _ => {}
        }
    }
    ss.plane.intersect_ray(o, d).map(|w| (w, "Plane"))
}

/// The sketch-plane coordinates of `p` when it lies on the plane.
fn on_plane(app: &SolveApp, p: Vec3) -> Option<Vec2> {
    let st = app.session.world_state();
    let ss = app.session.active_sketch.and_then(|s| st.sketch(s))?;
    let q = ss.plane.to_local(p);
    (ss.plane.to_world(q).dist(p) < 1e-6).then_some(q)
}

fn chain() -> Vec<Vec3> {
    CHAIN.with(|c| c.borrow().1.clone())
}

fn set_chain(cmd: Option<&'static str>, pts: Vec<Vec3>) {
    CHAIN.with(|c| *c.borrow_mut() = (cmd, pts));
}

/// The pointer moved: remember where a click would put the point.
pub fn on_hover(app: &SolveApp, proj: &Proj, pos: Pos2) {
    let h = if app.tool.as_ref().is_some_and(|t| takes(t.cmd)) && active(app) { point_at(app, proj, pos) } else { None };
    HOVER.with(|x| *x.borrow_mut() = h);
}

/// A click while a drawing tool is active in a 3D sketch (true: handled here).
pub fn on_click(app: &mut SolveApp, tool: &mut Tool, proj: &Proj, pos: Pos2) -> bool {
    if !takes(tool.cmd) || !active(app) {
        return false;
    }
    let Some((p, _)) = point_at(app, proj, pos) else { return true };
    if CHAIN.with(|c| c.borrow().0) != Some(tool.cmd) {
        set_chain(Some(tool.cmd), Vec::new());
    }
    match tool.cmd {
        "sketch.point" => {
            let r = match on_plane(app, p) {
                Some(q) => app.run("sketch.point", json!({"point": [q.x, q.y]})),
                None => app.run("sketch.point3d", json!({"point": [p.x, p.y, p.z]})),
            };
            report(app, r);
        }
        "sketch.line" => {
            if let Some(a) = chain().last().copied() {
                let r = match (on_plane(app, a), on_plane(app, p)) {
                    (Some(qa), Some(qb)) => app.run("sketch.line", json!({"points": [[qa.x, qa.y], [qb.x, qb.y]], "infer": true})),
                    _ => app.run("sketch.line3d", json!({"points": [[a.x, a.y, a.z], [p.x, p.y, p.z]]})),
                };
                report(app, r);
            }
            let mut c = chain();
            c.push(p);
            set_chain(Some(tool.cmd), c);
        }
        "sketch.arc.three_point" | "sketch.arc3d" => {
            let mut c = chain();
            c.push(p);
            if c.len() >= 3 {
                // Same click order as the 2D tool: start, end, then a point on the arc.
                let (a, e, t) = (c[0], c[1], c[2]);
                let r = match (on_plane(app, a), on_plane(app, e), on_plane(app, t)) {
                    (Some(qa), Some(qe), Some(qt)) if tool.cmd == "sketch.arc.three_point" => {
                        app.run("sketch.arc.three_point", json!({"start": [qa.x, qa.y], "end": [qe.x, qe.y], "through": [qt.x, qt.y]}))
                    }
                    _ => app.run("sketch.arc3d", json!({"start": [a.x, a.y, a.z], "through": [t.x, t.y, t.z], "end": [e.x, e.y, e.z]})),
                };
                report(app, r);
                c.clear();
            }
            set_chain(Some(tool.cmd), c);
        }
        _ => {
            let mut c = chain();
            c.push(p);
            set_chain(Some(tool.cmd), c);
        }
    }
    true
}

fn report(app: &mut SolveApp, r: Result<Value, String>) {
    if let Err(e) = r {
        app.set_status(e, true);
    }
}

/// Right-click / Esc while drawing in 3D: a spline is made from its points, a line chain ends
/// (true: handled here; the tool stays for the next curve).
pub fn finish(app: &mut SolveApp) -> bool {
    let (cmd, pts) = CHAIN.with(|c| c.borrow().clone());
    let Some(cmd) = cmd.filter(|c| app.tool.as_ref().is_some_and(|t| t.cmd == *c)) else {
        set_chain(None, Vec::new());
        return false;
    };
    if pts.is_empty() {
        set_chain(None, Vec::new());
        return false;
    }
    if cmd == "sketch.spline.fit_point" && pts.len() >= 2 {
        let r = if pts.iter().all(|p| on_plane(app, *p).is_some()) {
            let q: Vec<Value> = pts.iter().filter_map(|p| on_plane(app, *p)).map(|q| json!([q.x, q.y])).collect();
            app.run("sketch.spline.fit_point", json!({ "points": q }))
        } else {
            let q: Vec<Value> = pts.iter().map(|p| json!([p.x, p.y, p.z])).collect();
            app.run("sketch.spline3d", json!({ "points": q }))
        };
        report(app, r);
    }
    set_chain(Some(cmd), Vec::new());
    true
}

/// Rubber band and snap marker of the curve being drawn in 3D (true: drawn here).
pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) -> bool {
    if !takes(t.cmd) || !active(app) {
        return false;
    }
    let tk = Tokens::get();
    let stroke = Stroke::new(1.5, tk.sketch_accent);
    let pts = if CHAIN.with(|c| c.borrow().0) == Some(t.cmd) { chain() } else { Vec::new() };
    let hover = HOVER.with(|h| *h.borrow());
    let mut all: Vec<Pos2> = pts.iter().filter_map(|p| proj.to_screen(*p)).collect();
    if let Some((h, label)) = hover
        && let Some(s) = proj.to_screen(h)
    {
        if t.cmd != "sketch.point" && !pts.is_empty() {
            all.push(s);
        }
        painter.circle_stroke(s, if label == "Plane" { 3.0 } else { 6.0 }, stroke);
        if label != "Plane" {
            painter.text(
                s + vec2(10.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(crate::theme::Tokens::get().font_micro),
                tk.text,
            );
        }
        painter.text(
            s + vec2(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            format!("{}, {}, {}", crate::prefs::show_mm(app, h.x, 1), crate::prefs::show_mm(app, h.y, 1), crate::prefs::show_mm(app, h.z, 1)),
            egui::FontId::proportional(crate::theme::Tokens::get().font_small),
            tk.text_dim,
        );
    }
    if matches!(t.cmd, "sketch.arc.three_point" | "sketch.arc3d") && pts.len() == 2 {
        // The arc the third click would finish: from the first point to the second, through the cursor.
        let poly = match hover.map(|(h, _)| h) {
            Some(h) => match solidcraft_engine::sketch::arc_from_3_points(pts[0], h, pts[1]) {
                Some(a) => solidcraft_engine::sketch::arc_polyline(&a, 0.2),
                None => vec![pts[0], h, pts[1]],
            },
            None => vec![pts[0], pts[1]],
        };
        let screen: Vec<Pos2> = poly.into_iter().filter_map(|p| proj.to_screen(p)).collect();
        painter.add(Shape::line(screen, stroke));
    } else if t.cmd == "sketch.spline.fit_point" && all.len() >= 2 {
        let mut w: Vec<Vec3> = pts.clone();
        if let Some((h, _)) = hover {
            w.push(h);
        }
        let poly: Vec<Pos2> = solidcraft_engine::sketch::fit_polyline(&w).into_iter().filter_map(|p| proj.to_screen(p)).collect();
        painter.add(Shape::line(poly, stroke));
    } else {
        painter.add(Shape::line(all, stroke));
    }
    true
}

// ---------------------------------------------------------------------------------------------
// The move triad

fn triad_point(app: &SolveApp) -> Option<(String, usize, Vec3)> {
    let (w, i) = TRIAD.with(|t| t.borrow().clone())?;
    drawn_points(app).into_iter().find(|(id, k, _)| *id == w && *k == i)
}

fn unit(v: egui::Vec2) -> Option<egui::Vec2> {
    (v.length() > 1e-3).then(|| v / v.length())
}

/// The triad's handles on screen: (axis or None for the plane square, tip, base).
fn handles(app: &SolveApp, proj: &Proj) -> Vec<(Option<Vec3>, Pos2, Pos2)> {
    let Some((_, _, p)) = triad_point(app) else { return Vec::new() };
    let Some(base) = proj.to_screen(p) else { return Vec::new() };
    let mut out = Vec::new();
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        if let Some(t) = proj.to_screen(p + axis)
            && let Some(dir) = unit(t - base)
        {
            out.push((Some(axis), base + dir * ARROW, base));
        }
    }
    out.push((None, base + vec2(14.0, -14.0), base));
    out
}

/// A click without a tool: a drawn 3D point gets the triad (true: handled).
pub fn click(app: &mut SolveApp, proj: &Proj, pos: Pos2) -> bool {
    if !active(app) {
        TRIAD.with(|t| *t.borrow_mut() = None);
        return false;
    }
    let hit = drawn_points(app).into_iter().find(|(_, _, p)| proj.to_screen(*p).is_some_and(|s| s.distance(pos) < 8.0));
    let on_handle = handles(app, proj).iter().any(|(_, tip, _)| tip.distance(pos) < 9.0);
    if on_handle {
        return true;
    }
    TRIAD.with(|t| *t.borrow_mut() = hit.as_ref().map(|(w, i, _)| (w.clone(), *i)));
    if let Some((w, _, p)) = hit {
        app.set_status(
            format!(
                "{w} at {}, {}, {}: drag an arrow (X, Y, Z) or the square (in the sketch plane); Delete removes it",
                crate::prefs::show_mm(app, p.x, 1),
                crate::prefs::show_mm(app, p.y, 1),
                crate::prefs::show_mm(app, p.z, 1)
            ),
            false,
        );
        return true;
    }
    false
}

/// The drawn curve or point with the triad, for Delete.
pub fn delete_selected(app: &mut SolveApp) -> bool {
    let Some((w, _)) = TRIAD.with(|t| t.borrow_mut().take()) else { return false };
    let r = app.run("sketch.delete", json!({ "entities": [w] }));
    report(app, r);
    true
}

/// A drag starting on a triad handle (true: the triad took it).
pub fn drag_start(app: &SolveApp, proj: &Proj, press: Pos2) -> bool {
    let Some((axis, _, _)) = handles(app, proj).into_iter().find(|(_, tip, _)| tip.distance(press) < 9.0) else { return false };
    let Some((_, _, p)) = triad_point(app) else { return false };
    let grab = target(app, proj, press, axis, p).map(|g| p - g).unwrap_or(Vec3::ZERO);
    DRAG.with(|d| *d.borrow_mut() = Some((axis, app.session.undo.len(), grab)));
    true
}

/// Where the pointer puts the point: along `axis` through `p`, or in the plane through `p`
/// parallel to the sketch plane.
fn target(app: &SolveApp, proj: &Proj, pos: Pos2, axis: Option<Vec3>, p: Vec3) -> Option<Vec3> {
    let (o, d) = proj.ray(pos);
    match axis {
        Some(u) => {
            let w0 = p - o;
            let (a, b, c) = (u.dot(u), u.dot(d), d.dot(d));
            let den = a * c - b * b;
            if den.abs() < 1e-12 {
                return None;
            }
            Some(p + u * ((b * d.dot(w0) - c * u.dot(w0)) / den))
        }
        None => {
            let st = app.session.world_state();
            let n = app.session.active_sketch.and_then(|s| st.sketch(s))?.plane.normal();
            let den = d.dot(n);
            (den.abs() > 1e-12).then(|| o + d * ((p - o).dot(n) / den))
        }
    }
}

pub fn dragging() -> bool {
    DRAG.with(|d| d.borrow().is_some())
}

pub fn drag_to(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    let Some((axis, depth, grab)) = DRAG.with(|d| *d.borrow()) else { return };
    let Some((w, i, p)) = triad_point(app) else { return };
    let Some(to) = target(app, proj, pos, axis, p).map(|t| t + grab) else { return };
    if app.session.execute("sketch.move3d", &json!({"wire": w, "index": i, "to": [to.x, to.y, to.z]})).is_ok() {
        app.session.undo.truncate(depth + 1);
    }
}

pub fn drag_end() {
    DRAG.with(|d| *d.borrow_mut() = None);
}

/// Draw the drawn 3D points and the triad.
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.session.active_sketch.is_none() {
        return;
    }
    let tk = Tokens::get();
    let sel = TRIAD.with(|t| t.borrow().clone());
    for (w, i, p) in drawn_points(app) {
        if let Some(s) = proj.to_screen(p) {
            let on = sel.as_ref().is_some_and(|(sw, si)| *sw == w && *si == i);
            painter.rect_filled(Rect::from_center_size(s, vec2(6.0, 6.0)), 0.0, if on { tk.accent } else { tk.sketch_point_free });
        }
    }
    if !active(app) {
        return;
    }
    for (name, tip) in handle_points(app, proj) {
        crate::scenario::publish_handle(&format!("triad_{name}"), tip);
    }
    for (axis, tip, base) in handles(app, proj) {
        match axis {
            Some(a) => {
                let col = if a == Vec3::X {
                    tk.axis_x
                } else if a == Vec3::Y {
                    tk.axis_y
                } else {
                    tk.axis_z
                };
                painter.line_segment([base, tip], Stroke::new(2.0, col));
                if let Some(dir) = unit(tip - base) {
                    let n = vec2(-dir.y, dir.x);
                    painter.add(Shape::convex_polygon(vec![tip + dir * 8.0, tip + n * 4.0, tip - n * 4.0], col, Stroke::NONE));
                }
            }
            None => {
                painter.rect_stroke(Rect::from_center_size(tip, vec2(10.0, 10.0)), 0.0, Stroke::new(1.5, tk.accent), egui::StrokeKind::Inside);
            }
        }
    }
}

/// Test hook: screen positions of the triad handles (axis name or "plane", tip).
pub fn handle_points(app: &SolveApp, proj: &Proj) -> Vec<(&'static str, Pos2)> {
    handles(app, proj)
        .into_iter()
        .map(|(a, tip, _)| {
            let name = match a {
                Some(v) if v == Vec3::X => "x",
                Some(v) if v == Vec3::Y => "y",
                Some(_) => "z",
                None => "plane",
            };
            (name, tip)
        })
        .collect()
}
