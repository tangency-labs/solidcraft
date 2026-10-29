//! Drawing-time inference for the Line tool: while the next point is placed the
//! cursor snaps to directions and places taken from the existing geometry (parallel or
//! perpendicular to a line, on the extension of a line, tangent to a circle or arc, or through
//! the centre of a circle or arc). A dashed guide and a label show it; placing the point adds
//! the matching constraint.

use std::cell::RefCell;

use egui::Stroke;
use serde_json::json;
use solidcraft_engine::geom::Vec2;
use solidcraft_engine::sketch::{CurveKind, Sketch};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::Proj;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Parallel,
    Perpendicular,
    Extension,
    Tangent,
    /// The new line passes through the centre of a circle or arc.
    Center,
}

/// A snap taken from existing geometry: what, to which curve, where the point goes, how far the
/// cursor was moved, and the guide to draw (sketch coordinates).
#[derive(Clone, Debug, PartialEq)]
pub struct Inference {
    pub kind: Kind,
    pub curve: String,
    pub at: Vec2,
    pub dist: f64,
    pub guide: (Vec2, Vec2),
    /// Point argument for the new endpoint: `"<id>.center"` when it lands on the centre, or
    /// `"on:<id>:x,y"` when it lands on the circle. None: a free point on the radial line.
    pub arg: Option<String>,
}

impl Inference {
    pub fn label(&self) -> &'static str {
        match self.kind {
            Kind::Parallel => "//",
            Kind::Perpendicular => "\u{22a5}",
            Kind::Extension => "Ext",
            Kind::Tangent => "T",
            Kind::Center => "Ctr",
        }
    }
}

thread_local! {
    static PENDING: RefCell<Option<Inference>> = const { RefCell::new(None) };
}

/// Forget the current inference (another snap won, or the tool ended).
pub fn clear() {
    PENDING.with(|p| *p.borrow_mut() = None);
}

pub fn current() -> Option<Inference> {
    PENDING.with(|p| p.borrow().clone())
}

fn keep(i: Option<Inference>) -> Option<Inference> {
    PENDING.with(|p| *p.borrow_mut() = i.clone());
    i
}

fn line_ends(sk: &Sketch, c: usize) -> Option<(Vec2, Vec2)> {
    match sk.curves.get(c)?.kind {
        CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
        _ => None,
    }
}

/// Tangent from the previous point `prev` to a circle or arc, when the cursor `lp` is near the
/// tangent point (within `tol`).
pub fn tangent(sk: &Sketch, prev: Vec2, lp: Vec2, tol: f64) -> Option<Inference> {
    let mut best: Option<Inference> = None;
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction || !matches!(c.kind, CurveKind::Circle { .. } | CurveKind::Arc { .. }) {
            continue;
        }
        let (Some(o), Some(r)) = (sk.center(i), sk.radius(i)) else { continue };
        let d = prev.dist(o);
        if d <= r + 1e-9 {
            continue;
        }
        // The two tangent points seen from `prev`.
        let base = (prev - o).angle();
        let half = (r / d).acos();
        for s in [-1.0, 1.0] {
            let t = o + Vec2::from_angle(base + s * half) * r;
            let on_arc = sk.shape(i).is_none_or(|sh| sh.dist(t) < 1e-6);
            let e = t.dist(lp);
            if on_arc && e < tol && best.as_ref().is_none_or(|b| e < b.dist) {
                best = Some(Inference { kind: Kind::Tangent, curve: c.id.clone(), at: t, dist: e, guide: (prev, t), arg: None });
            }
        }
    }
    keep(best)
}

/// The line from `prev` passes near the centre of a circle or arc (within `tol`).
///
/// The point lands on that radial line, so the segment contains the centre. On the circle it
/// lands on the rim (a diameter, or a radius when the other end is already the centre's side).
/// On the centre point itself it lands on that point. A chord that misses the centre, or that
/// stops before reaching it, is left alone.
pub fn through_center(sk: &Sketch, prev: Vec2, lp: Vec2, tol: f64) -> Option<Inference> {
    let mut best: Option<Inference> = None;
    for (i, c) in sk.curves.iter().enumerate() {
        if !matches!(c.kind, CurveKind::Circle { .. } | CurveKind::Arc { .. }) {
            continue;
        }
        let (Some(o), Some(r)) = (sk.center(i), sk.radius(i)) else { continue };
        if r <= 1e-9 {
            continue;
        }
        // Already standing on the centre: every direction goes through it.
        let Some(dir) = (o - prev).normalized() else { continue };
        let off = (lp - prev).cross(dir).abs();
        if off > tol {
            continue;
        }
        let along = (lp - prev).dot(dir);
        if along < tol {
            continue;
        }
        let d = prev.dist(o);
        let past = along + tol >= d;
        let far = prev + dir * (d + r);
        let on_arc = sk.shape(i).is_none_or(|sh| sh.dist(far) < 1e-6);
        let near_curve = sk.shape(i).is_some_and(|sh| sh.dist(lp) < tol * 0.8);
        let rim = near_curve && on_arc && past && far.dist(lp) <= tol;
        let at_centre = lp.dist(o) <= tol;
        let (at, arg) = if rim && (!at_centre || far.dist(lp) <= lp.dist(o)) {
            (far, Some(format!("on:{}:{},{}", c.id, far.x, far.y)))
        } else if at_centre {
            (o, Some(format!("{}.center", c.id)))
        } else if past {
            let at = prev + dir * along;
            if at.dist(o) <= tol { (o, Some(format!("{}.center", c.id))) } else { (at, None) }
        } else {
            continue;
        };
        if at.dist(prev) < 1e-6 {
            continue;
        }
        let dist = at.dist(lp);
        if dist <= tol && best.as_ref().is_none_or(|b| dist < b.dist) {
            best = Some(Inference { kind: Kind::Center, curve: c.id.clone(), at, dist, guide: (prev, at), arg });
        }
    }
    keep(best)
}

/// Directions and places from lines: parallel or perpendicular to a line (from `prev`), or on
/// the extension of a line beyond its ends.
pub fn lines(sk: &Sketch, prev: Vec2, lp: Vec2, tol: f64) -> Option<Inference> {
    let mut best: Option<Inference> = None;
    let mut offer = |i: Inference| {
        if best.as_ref().is_none_or(|b| i.dist < b.dist) {
            best = Some(i);
        }
    };
    let v = lp - prev;
    if v.len() < tol * 2.0 {
        return keep(None);
    }
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction && c.link.is_none() {
            continue;
        }
        let Some((a, b)) = line_ends(sk, i) else { continue };
        let Some(dir) = (b - a).normalized() else { continue };
        // Axis-aligned directions are the H/V snaps' business.
        let axis = dir.x.abs() < 1e-9 || dir.y.abs() < 1e-9;
        let touches = a.dist(prev) < 1e-9 || b.dist(prev) < 1e-9;
        for (kind, d) in [(Kind::Parallel, dir), (Kind::Perpendicular, dir.perp())] {
            if axis || (kind == Kind::Parallel && touches) {
                continue;
            }
            let t = v.dot(d);
            let at = prev + d * t;
            let e = at.dist(lp);
            if e < tol {
                offer(Inference {
                    kind,
                    curve: c.id.clone(),
                    at,
                    dist: e,
                    guide: (prev - d * (t.signum() * tol * 6.0), at + d * (t.signum() * tol * 6.0)),
                    arg: None,
                });
            }
        }
        // The extension: beyond the ends, on the line's own (infinite) line.
        let s = (lp - a).dot(dir);
        let len = a.dist(b);
        if !(-tol..=len + tol).contains(&s) {
            let at = a + dir * s;
            let e = at.dist(lp);
            if e < tol * 0.8 && !touches {
                let from = if s < 0.0 { a } else { b };
                offer(Inference { kind: Kind::Extension, curve: c.id.clone(), at, dist: e * 0.9, guide: (from, at), arg: None });
            }
        }
    }
    keep(best)
}

/// The Line tool placed a segment ending at `end` (the new line `line`): add the constraint of
/// the inference it was snapped by.
pub fn commit(app: &mut SolveApp, line: &str, end: Vec2) {
    let Some(i) = current().filter(|i| i.at.dist(end) < 1e-9) else { return };
    clear();
    // The endpoint is the circle's centre: the line already ends on that point.
    if matches!(i.kind, Kind::Center) && i.arg.as_deref().is_some_and(|a| a.ends_with(".center")) {
        return;
    }
    let (cmd, mut params) = match i.kind {
        Kind::Parallel => ("sketch.constraint.parallel", json!({"a": i.curve, "b": line})),
        Kind::Perpendicular => ("sketch.constraint.perpendicular", json!({"a": i.curve, "b": line})),
        Kind::Tangent => ("sketch.constraint.tangent", json!({"a": i.curve, "b": line})),
        Kind::Extension => ("sketch.constraint.coincident", json!({"a": format!("{line}.end"), "b": i.curve})),
        Kind::Center => ("sketch.constraint.coincident", json!({"a": format!("{}.center", i.curve), "b": line})),
    };
    // An inference only adds what the sketch does not already have: one that is already implied
    // (redundant) or would conflict is simply not kept.
    params["if_redundant"] = json!("refuse");
    let _ = app.session.execute(cmd, &params);
}

/// The dashed guide of the current inference (its label is the snap hint) (while the Line tool is active).
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.as_ref().is_none_or(|t| t.cmd != "sketch.line") {
        return;
    }
    let Some(i) = current() else { return };
    let st = app.session.world_state();
    let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    let tk = Tokens::get();
    if let (Some(a), Some(b)) = (to(i.guide.0), to(i.guide.1)) {
        painter.extend(crate::viewport::dashed_line(&[a, b], painter.clip_rect(), Stroke::new(1.0, tk.rubber_band), 5.0, 4.0));
    }
}

/// What a tracking guide starts from: a curve's centre or straight-edge midpoint, or, while a
/// drawing tool waits for a point, a sketch point itself (a line or arc end, a rectangle corner,
/// a free point; #9105).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TrackRef {
    Curve(String),
    Point(String),
}

/// The two most recent hovered references ([`TrackRef`]). Transient, per viewport, never a
/// constraint. A third hover drops the oldest.
#[derive(Default)]
pub(crate) struct Tracking {
    sketch: Option<u64>,
    document: std::sync::Weak<solidcraft_engine::doc::Document>,
    anchor: Option<TrackRef>,
    previous: Option<TrackRef>,
    pub(crate) guide: Option<(Vec2, Vec2)>,
    pub(crate) remembered_guide: Option<(Vec2, Vec2)>,
}

impl Tracking {
    fn reference(sk: &Sketch, index: usize) -> Option<Vec2> {
        if !solidcraft_engine::view::sketch_curve_visible(sk, index) {
            return None;
        }
        match sk.curves.get(index)?.kind {
            CurveKind::Line { a, b } => Some((sk.point(a)? + sk.point(b)?) * 0.5),
            CurveKind::Circle { .. } | CurveKind::Arc { .. } => sk.center(index),
            _ => None,
        }
    }

    /// The sketch points a guide may start from: shown points other than the origin (the sketch
    /// axes already run through it) that are free or belong to at least one shown curve.
    fn point_references(sk: &Sketch) -> Vec<bool> {
        let visible = solidcraft_engine::view::SketchPointVisibility::new(sk);
        let mut used = vec![false; sk.points.len()];
        let mut shown = vec![false; sk.points.len()];
        for (index, curve) in sk.curves.iter().enumerate() {
            let on = solidcraft_engine::view::sketch_curve_visible(sk, index);
            for p in curve.kind.point_ids() {
                if let Some(u) = used.get_mut(p) {
                    *u = true;
                }
                if on && let Some(s) = shown.get_mut(p) {
                    *s = true;
                }
            }
        }
        sk.points
            .iter()
            .enumerate()
            .map(|(i, p)| i != 0 && visible.takes(p) && (shown.get(i).copied().unwrap_or(false) || !used.get(i).copied().unwrap_or(true)))
            .collect()
    }

    fn point_of(sk: &Sketch, reference: &TrackRef) -> Option<Vec2> {
        match reference {
            TrackRef::Curve(id) => Self::reference(sk, sk.curve_index(id)?),
            TrackRef::Point(id) => {
                let index = sk.point_index(id)?;
                Self::point_references(sk).get(index).copied().unwrap_or(false).then(|| sk.point(index)).flatten()
            }
        }
    }

    /// Keep `reference` as one of the two remembered references. Hovering the older one swaps
    /// them, so going back does not drop it. A third, different reference drops the oldest.
    fn remember(&mut self, reference: TrackRef) {
        if self.anchor.as_ref() == Some(&reference) {
            return;
        }
        if self.previous.as_ref() == Some(&reference) {
            std::mem::swap(&mut self.anchor, &mut self.previous);
            return;
        }
        self.previous = self.anchor.replace(reference);
    }

    /// Where the horizontal through one reference meets the vertical through the other, when
    /// that point is within a few pixels of the cursor and is not either reference itself.
    pub(crate) fn meeting(&self, sk: &Sketch, plane: &solidcraft_engine::geom::Plane, proj: &Proj, cursor: egui::Pos2) -> Option<Vec2> {
        let a = self.anchor.as_ref().and_then(|r| Self::point_of(sk, r))?;
        let b = self.previous.as_ref().and_then(|r| Self::point_of(sk, r))?;
        if a.dist(b) < 1e-6 {
            return None;
        }
        let to = |p: Vec2| proj.to_screen(plane.to_world(p));
        let mut best: Option<(f32, Vec2)> = None;
        for at in [Vec2::new(a.x, b.y), Vec2::new(b.x, a.y)] {
            if at.dist(a) < 1e-6 || at.dist(b) < 1e-6 {
                continue;
            }
            let Some(screen) = to(at).filter(|p| proj.rect.contains(*p)) else { continue };
            let distance = cursor.distance(screen);
            if distance < 8.0 && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, at));
            }
        }
        best.map(|(_, at)| at)
    }

    fn axis_guide(anchor: Vec2, at: Vec2, cursor: egui::Pos2, to: &impl Fn(Vec2) -> Option<egui::Pos2>) -> Option<(Vec2, Vec2)> {
        let origin = to(anchor)?;
        let mut distance = 6.0;
        let mut end = None;
        for candidate in [Vec2::new(at.x, anchor.y), Vec2::new(anchor.x, at.y)] {
            let Some(screen) = to(candidate) else { continue };
            let off_axis = cursor.distance(screen);
            if origin.distance(screen) > 8.0 && off_axis < distance {
                distance = off_axis;
                end = Some(candidate);
            }
        }
        end.map(|end| (anchor, end))
    }

    /// Follow the cursor: remember the reference under it and lay out the guides. With
    /// `points`, sketch points are references too (a drawing tool waits for a point);
    /// without, remembered points are let go.
    fn update(&mut self, sk: &Sketch, plane: &solidcraft_engine::geom::Plane, proj: &Proj, cursor: egui::Pos2, points: bool) {
        self.guide = None;
        self.remembered_guide = None;
        if !points {
            for slot in [&mut self.anchor, &mut self.previous] {
                if matches!(slot, Some(TrackRef::Point(_))) {
                    *slot = None;
                }
            }
            if self.anchor.is_none() {
                self.anchor = self.previous.take();
            }
        }
        let to = |p: Vec2| proj.to_screen(plane.to_world(p));
        // Screen-space tolerances stay the same at any zoom; only visible references wake.
        // Centres and midpoints come first, so a point on one of them names the curve.
        let mut nearest = 8.0;
        let mut nearest_ref = None;
        for (index, curve) in sk.curves.iter().enumerate() {
            let Some(p) = Self::reference(sk, index).and_then(to).filter(|p| proj.rect.contains(*p)) else { continue };
            let distance = cursor.distance(p);
            if distance < nearest {
                nearest = distance;
                nearest_ref = Some(TrackRef::Curve(curve.id.clone()));
            }
        }
        if points {
            for (point, usable) in sk.points.iter().zip(Self::point_references(sk)) {
                let Some(p) = usable.then_some(point.pos).and_then(to).filter(|p| proj.rect.contains(*p)) else { continue };
                let distance = cursor.distance(p);
                if distance < nearest {
                    nearest = distance;
                    nearest_ref = Some(TrackRef::Point(point.id.clone()));
                }
            }
        }
        if let Some(reference) = nearest_ref {
            self.remember(reference);
        }
        let Some(anchor) = self.anchor.as_ref().and_then(|r| Self::point_of(sk, r)).or_else(|| {
            self.anchor = self.previous.take();
            self.anchor.as_ref().and_then(|r| Self::point_of(sk, r))
        }) else {
            self.anchor = None;
            self.previous = None;
            return;
        };
        if self.previous.as_ref().and_then(|r| Self::point_of(sk, r)).is_none() {
            self.previous = None;
        }
        let Some(origin) = to(anchor) else { return };
        if !proj.rect.contains(origin) {
            return;
        }
        let (ray, direction) = proj.ray(cursor);
        let Some(world) = plane.intersect_ray(ray, direction) else { return };
        let at = plane.to_local(world);
        if !at.x.is_finite() || !at.y.is_finite() {
            return;
        }
        // Near the crossing, both guides end on that point. Otherwise each reference keeps
        // its own axis, and a single guide stays visual (it does not move the cursor).
        if let Some(meet) = self.meeting(sk, plane, proj, cursor) {
            let span = |from: Vec2| to(from).zip(to(meet)).filter(|(a, b)| a.distance(*b) > 8.0).map(|_| (from, meet));
            self.guide = span(anchor);
            if let Some(previous) = self.previous.as_ref().and_then(|r| Self::point_of(sk, r)) {
                self.remembered_guide = span(previous);
            }
            return;
        }
        // Finite guides connect the reference to the cursor's projection onto its axis.
        // No model-space length cap: the viewport clips them, as it does other sketch guides.
        self.guide = Self::axis_guide(anchor, at, cursor, &to);
        if let Some(previous) = self.previous.as_ref().and_then(|r| Self::point_of(sk, r)) {
            self.remembered_guide = Self::axis_guide(previous, at, cursor, &to);
        }
    }
}

/// Visual H/V tracking works before a tool's first click and while simply editing a sketch.
/// The crossing of the two remembered references is also a snap. A single guide stays visual.
/// Existing snaps, picks and constraints retain their behaviour. While a drawing tool waits for
/// a point, sketch points (ends, corners, free points) are references too, unless the Sketch
/// preferences turn that off; idle hovering only wakes centres and midpoints (#9105).
pub(crate) fn track_and_show(app: &mut SolveApp, painter: &egui::Painter, proj: &Proj, cursor: Option<egui::Pos2>) {
    crate::scenario::publish_count("sketch_tracking_guides", 0.0);
    let document = std::sync::Arc::downgrade(&app.session.doc);
    let active = app.session.active_sketch;
    let points = app.preferences.sketch_tracking_points && app.tool.as_ref().is_some_and(|t| matches!(t.kind, crate::tools::Kind::Draw(_)));
    let tracking = &mut app.viewport.tracking;
    if tracking.sketch != active || !tracking.document.ptr_eq(&document) {
        *tracking = Tracking { sketch: active, document, ..Default::default() };
    }
    // The world is asked for only with a sketch to track in: outside a sketch, placing it every
    // frame placed every moved body again in each frame of a joint drag (#6325).
    let wanted =
        active.zip(cursor).filter(|_| app.preferences.sketch_tracking_guides && !crate::drag_snap::suppressed() && app.viewport.point_drag.is_none());
    let state = wanted.map(|_| app.session.world_state());
    let Some((ss, cursor)) = wanted.zip(state.as_deref()).and_then(|((id, cursor), state)| state.sketch(id).map(|ss| (ss, cursor))) else {
        *tracking = Tracking::default();
        return;
    };
    tracking.update(&ss.sketch, &ss.plane, proj, cursor, points);
    let shown = tracking.guide.is_some() || tracking.remembered_guide.is_some();
    crate::scenario::publish_count("sketch_tracking_guides", usize::from(shown) as f64);
    let tk = Tokens::get();
    let draw = |guide: (Vec2, Vec2)| {
        let (Some(a), Some(b)) = (proj.to_screen(ss.plane.to_world(guide.0)), proj.to_screen(ss.plane.to_world(guide.1))) else { return None };
        painter.extend(crate::viewport::dashed_line(&[a, b], painter.clip_rect(), Stroke::new(1.0, tk.rubber_band), 5.0, 4.0));
        painter.circle_stroke(a, 4.0, Stroke::new(1.0, tk.rubber_band));
        Some(b)
    };
    let ends = [tracking.guide.and_then(draw), tracking.remembered_guide.and_then(draw)];
    if let (Some(a), Some(b)) = (ends[0], ends[1])
        && a.distance(b) < 1.0
    {
        painter.circle_stroke(a, 4.0, Stroke::new(1.0, tk.rubber_band));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk() -> Sketch {
        let mut s = Sketch::new();
        let _ = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), None, None, Some("l1"));
        let _ = s.add_circle(Vec2::new(30.0, 0.0), 5.0, None, Some("c1"));
        s
    }

    #[test]
    fn tracking_has_pixel_tolerance_and_only_spans_reference_to_cursor() {
        use solidcraft_engine::geom::Plane;
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.cam.yaw = 0.0;
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        let mut sk = Sketch::new();
        sk.add_circle(Vec2::new(-10.0, 3.0), 5.0, None, Some("c")).unwrap();
        sk.add_line(Vec2::new(10.0, -20.0), Vec2::new(30.0, -20.0), None, None, Some("l")).unwrap();
        for zoom in [60.0, 180.0] {
            app.cam.distance = zoom;
            let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
            let to = |p: Vec2| proj.to_screen(Plane::XY.to_world(p)).unwrap();
            let mut tracking = Tracking::default();
            let centre = Vec2::new(-10.0, 3.0);
            tracking.update(&sk, &Plane::XY, &proj, to(centre), false);
            assert!(tracking.guide.is_none(), "no zero-length guide at the centre");
            for end in [Vec2::new(35.0, 3.0), Vec2::new(-35.0, 3.0), Vec2::new(-10.0, 38.0), Vec2::new(-10.0, -38.0)] {
                tracking.update(&sk, &Plane::XY, &proj, to(end), false);
                let (a, b) = tracking.guide.expect("all four directions");
                assert!(a.dist(centre) < 1e-4 && b.dist(end) < 1e-4);
            }
            let end = Vec2::new(35.0, 3.0);
            tracking.update(&sk, &Plane::XY, &proj, to(end) + egui::vec2(0.0, 5.0), false);
            assert!(tracking.guide.is_some(), "5 screen points from the axis");
            tracking.update(&sk, &Plane::XY, &proj, to(end) + egui::vec2(0.0, 7.0), false);
            assert!(tracking.guide.is_none(), "7 screen points from the axis");
            let mid = Vec2::new(20.0, -20.0);
            tracking.update(&sk, &Plane::XY, &proj, to(mid), false);
            tracking.update(&sk, &Plane::XY, &proj, to(Vec2::new(20.0, 20.0)), false);
            assert!(tracking.guide.unwrap().0.dist(mid) < 1e-8, "new MID replaces centre");
            sk.curves.iter_mut().find(|c| c.id == "l").unwrap().id = "renamed".into();
            tracking.update(&sk, &Plane::XY, &proj, to(Vec2::new(20.0, 20.0)), false);
            assert!(tracking.guide.is_none(), "removed references cannot leave a stale guide");
            sk.curves.iter_mut().find(|c| c.id == "renamed").unwrap().id = "l".into();
        }
    }

    #[test]
    fn end_points_corners_and_free_points_start_guides_only_while_drawing() {
        use solidcraft_engine::geom::Plane;
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.cam.yaw = 0.0;
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        app.cam.distance = 180.0;
        let mut sk = Sketch::new();
        sk.add_line(Vec2::new(-30.0, 20.0), Vec2::new(30.0, 20.0), None, None, Some("top")).unwrap();
        sk.add_line(Vec2::new(30.0, 20.0), Vec2::new(30.0, -20.0), None, None, Some("right")).unwrap();
        sk.add_arc(Vec2::new(-20.0, -10.0), Vec2::new(-10.0, -10.0), Vec2::new(-20.0, 0.0), [None; 3], Some("arc")).unwrap();
        sk.add_point(Vec2::new(15.0, -25.0), Some("free")).unwrap();
        let hidden = sk.add_line(Vec2::new(45.0, 35.0), Vec2::new(55.0, 35.0), None, None, Some("hidden")).unwrap();
        sk.curves.get_mut(hidden).unwrap().construction = true;
        sk.view.hide_construction = true;
        let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        let to = |p: Vec2| proj.to_screen(Plane::XY.to_world(p)).unwrap();
        // Each point, then a cursor level with it far to the right: the guide runs from the point.
        for (point, end) in [
            (Vec2::new(30.0, 20.0), Vec2::new(70.0, 20.0)),    // a corner two lines share
            (Vec2::new(-30.0, 20.0), Vec2::new(-30.0, -45.0)), // a line end, plumb below it
            (Vec2::new(-20.0, 0.0), Vec2::new(60.0, 0.0)),     // an arc end
            (Vec2::new(15.0, -25.0), Vec2::new(15.0, 45.0)),   // a free point
        ] {
            let mut drawing = Tracking::default();
            drawing.update(&sk, &Plane::XY, &proj, to(point), true);
            drawing.update(&sk, &Plane::XY, &proj, to(end), true);
            let (a, b) = drawing.guide.unwrap_or_else(|| panic!("a guide from {point:?}"));
            assert!(a.dist(point) < 1e-9 && b.dist(end) < 1e-4, "{a:?} {b:?}");
            let mut idle = Tracking::default();
            idle.update(&sk, &Plane::XY, &proj, to(point), false);
            idle.update(&sk, &Plane::XY, &proj, to(end), false);
            assert!(idle.guide.is_none(), "idle hovering takes no point {point:?}");
        }
        // The origin (the axes run through it) and the ends of a hidden curve start nothing.
        for point in [Vec2::ZERO, Vec2::new(45.0, 35.0)] {
            let mut tracking = Tracking::default();
            tracking.update(&sk, &Plane::XY, &proj, to(point), true);
            assert!(tracking.anchor.is_none(), "{point:?}");
        }
        // A centre or midpoint still names its curve; a point and a midpoint meet as two
        // centres do, and leaving the drawing tool lets the point go but keeps the midpoint.
        let mut tracking = Tracking::default();
        tracking.update(&sk, &Plane::XY, &proj, to(Vec2::new(-20.0, -10.0)), true);
        assert_eq!(tracking.anchor, Some(TrackRef::Curve("arc".into())));
        tracking.update(&sk, &Plane::XY, &proj, to(Vec2::new(30.0, 0.0)), true);
        tracking.update(&sk, &Plane::XY, &proj, to(Vec2::new(15.0, -25.0)), true);
        assert_eq!(tracking.anchor, Some(TrackRef::Point("free".into())));
        assert_eq!(tracking.previous, Some(TrackRef::Curve("right".into())));
        let cross = Vec2::new(15.0, 0.0);
        assert!(tracking.meeting(&sk, &Plane::XY, &proj, to(cross)).unwrap().dist(cross) < 1e-9);
        tracking.update(&sk, &Plane::XY, &proj, to(Vec2::new(60.0, 0.0)), false);
        assert_eq!(tracking.anchor, Some(TrackRef::Curve("right".into())));
        assert!(tracking.previous.is_none());
    }

    #[test]
    fn two_recent_references_meet_and_a_third_drops_the_oldest() {
        use solidcraft_engine::geom::Plane;
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.cam.yaw = 0.0;
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        app.cam.distance = 180.0;
        let mut sk = Sketch::new();
        sk.add_line(Vec2::new(-30.0, 20.0), Vec2::new(30.0, 20.0), None, None, Some("top")).unwrap();
        sk.add_line(Vec2::new(30.0, -20.0), Vec2::new(30.0, 20.0), None, None, Some("right")).unwrap();
        sk.add_circle(Vec2::new(-10.0, 3.0), 5.0, None, Some("c")).unwrap();
        sk.add_line(Vec2::new(50.0, -10.0), Vec2::new(50.0, 10.0), None, None, Some("far")).unwrap();
        let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        let to = |p: Vec2| proj.to_screen(Plane::XY.to_world(p)).unwrap();
        let mut tracking = Tracking::default();
        let hover = |tracking: &mut Tracking, p: Vec2| tracking.update(&sk, &Plane::XY, &proj, to(p), false);
        hover(&mut tracking, Vec2::new(0.0, 20.0));
        hover(&mut tracking, Vec2::new(30.0, 0.0));
        let centre = Vec2::ZERO;
        let near = to(centre) + egui::vec2(4.0, 3.0);
        assert!(tracking.meeting(&sk, &Plane::XY, &proj, near).unwrap().dist(centre) < 1e-6);
        tracking.update(&sk, &Plane::XY, &proj, near, false);
        let (a, b) = tracking.guide.unwrap();
        let (c, d) = tracking.remembered_guide.unwrap();
        assert!(b.dist(centre) < 1e-6 && d.dist(centre) < 1e-6, "both guides end on the crossing");
        assert!(a.dist(Vec2::new(30.0, 0.0)) < 1e-6 && c.dist(Vec2::new(0.0, 20.0)) < 1e-6);
        assert!(tracking.meeting(&sk, &Plane::XY, &proj, to(centre) + egui::vec2(20.0, 0.0)).is_none());
        // Back to the first reference: both stay, in swapped order.
        hover(&mut tracking, Vec2::new(0.0, 20.0));
        assert_eq!(tracking.anchor, Some(TrackRef::Curve("top".into())));
        assert_eq!(tracking.previous, Some(TrackRef::Curve("right".into())));
        // A circle centre and a midpoint meet the same way.
        hover(&mut tracking, Vec2::new(-10.0, 3.0));
        let cross = Vec2::new(0.0, 3.0);
        assert!(tracking.meeting(&sk, &Plane::XY, &proj, to(cross)).unwrap().dist(cross) < 1e-6);
        // A third reference drops the oldest. The far edge shares a horizontal with the right edge,
        // so their axes do not invent the rectangle centre.
        hover(&mut tracking, Vec2::new(30.0, 0.0));
        hover(&mut tracking, Vec2::new(50.0, 0.0));
        assert_eq!(tracking.anchor, Some(TrackRef::Curve("far".into())));
        assert_eq!(tracking.previous, Some(TrackRef::Curve("right".into())));
        assert!(tracking.meeting(&sk, &Plane::XY, &proj, to(centre)).is_none());
        sk.curves.iter_mut().find(|c| c.id == "far").unwrap().id = "gone".into();
        tracking.update(&sk, &Plane::XY, &proj, to(Vec2::new(40.0, 40.0)), false);
        assert_eq!(tracking.anchor, Some(TrackRef::Curve("right".into())));
        assert!(tracking.previous.is_none());
        assert!(tracking.guide.is_none(), "a removed reference leaves no guide");
    }

    #[test]
    fn parallel_and_perpendicular_from_the_previous_point() {
        let s = sk();
        let i = lines(&s, Vec2::new(0.0, 20.0), Vec2::new(10.2, 29.9), 0.5).expect("parallel");
        assert_eq!(i.kind, Kind::Parallel);
        assert!((i.at - Vec2::new(0.0, 20.0)).cross(Vec2::new(1.0, 1.0)).abs() < 1e-9);
        let i = lines(&s, Vec2::new(0.0, 20.0), Vec2::new(10.1, 10.0), 0.5).expect("perpendicular");
        assert_eq!(i.kind, Kind::Perpendicular);
        assert!(lines(&s, Vec2::new(0.0, 20.0), Vec2::new(10.0, 25.0), 0.5).is_none());
    }

    #[test]
    fn extension_beyond_the_end() {
        let s = sk();
        let i = lines(&s, Vec2::new(30.0, 15.0), Vec2::new(20.1, 19.9), 0.5).expect("extension");
        assert_eq!(i.kind, Kind::Extension);
        assert!((i.at.x - i.at.y).abs() < 1e-9 && i.at.x > 10.0);
    }

    #[test]
    fn tangent_point_on_a_circle() {
        let s = sk();
        let prev = Vec2::new(30.0, 20.0);
        // Tangent points from (30, 20) to the circle at (30, 0) r 5.
        let half = (5.0f64 / 20.0).acos();
        let t = Vec2::new(30.0, 0.0) + Vec2::from_angle(std::f64::consts::FRAC_PI_2 + half) * 5.0;
        let i = tangent(&s, prev, t + Vec2::new(0.2, 0.1), 0.5).expect("tangent");
        assert_eq!(i.kind, Kind::Tangent);
        assert!(i.at.dist(t) < 1e-9);
        assert!((i.at - Vec2::new(30.0, 0.0)).dot(i.at - prev).abs() < 1e-9);
    }

    fn circle() -> Sketch {
        let mut s = Sketch::new();
        let _ = s.add_circle(Vec2::ZERO, 10.0, None, Some("c1"));
        s
    }

    /// `o` lies on the segment from `prev` to `at`.
    fn through(prev: Vec2, at: Vec2, o: Vec2) -> bool {
        let d = at - prev;
        let len2 = d.len2();
        if len2 < 1e-18 {
            return false;
        }
        let t = (o - prev).dot(d) / len2;
        (0.0..=1.0).contains(&t) && (prev + d * t).dist(o) < 1e-8
    }

    #[test]
    fn a_near_diameter_lands_on_the_circle_through_its_centre() {
        let s = circle();
        // Start on the left of the circle; the cursor is on the right, a fraction off the diameter.
        let i = through_center(&s, Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.3), 0.5).expect("diameter");
        assert_eq!(i.kind, Kind::Center);
        assert!(i.at.dist(Vec2::new(10.0, 0.0)) < 1e-9, "{:?}", i.at);
        assert!(through(Vec2::new(-10.0, 0.0), i.at, Vec2::ZERO));
        assert_eq!(i.arg.as_deref(), Some("on:c1:10,0"));
    }

    #[test]
    fn a_chord_that_misses_the_centre_stays_free() {
        let s = circle();
        assert!(through_center(&s, Vec2::new(-10.0, 0.0), Vec2::new(10.0, 4.0), 0.5).is_none());
        // Stopped short of the centre: still a free point on the near side.
        assert!(through_center(&s, Vec2::new(-30.0, 0.0), Vec2::new(-15.0, 0.1), 0.5).is_none());
    }

    #[test]
    fn the_endpoint_snaps_onto_the_centre_point() {
        let s = circle();
        let i = through_center(&s, Vec2::new(20.0, 0.0), Vec2::new(0.2, 0.1), 0.5).expect("centre");
        assert!(i.at.dist(Vec2::ZERO) < 1e-9);
        assert_eq!(i.arg.as_deref(), Some("c1.center"));
        // Already standing on the centre: nothing further to snap.
        assert!(through_center(&s, Vec2::ZERO, Vec2::new(10.0, 0.2), 0.5).is_none());
    }

    #[test]
    fn a_line_past_the_centre_locks_onto_the_radial_line() {
        let s = circle();
        let i = through_center(&s, Vec2::new(-20.0, 0.0), Vec2::new(5.0, 0.2), 0.5).expect("radial");
        assert!(i.at.dist(Vec2::new(5.0, 0.0)) < 1e-9, "{:?}", i.at);
        assert!(i.arg.is_none());
        assert!(through(Vec2::new(-20.0, 0.0), i.at, Vec2::ZERO));
    }

    #[test]
    fn an_arc_only_accepts_a_rim_point_that_lies_on_it() {
        let mut s = Sketch::new();
        // Quarter arc in the first quadrant.
        let _ = s.add_arc(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), [None; 3], Some("a1"));
        let on = through_center(&s, Vec2::new(-20.0, 0.0), Vec2::new(10.1, 0.2), 0.5).expect("on the arc");
        assert!(on.at.dist(Vec2::new(10.0, 0.0)) < 1e-9, "{:?}", on.at);
        assert!(on.arg.as_deref().unwrap().starts_with("on:a1:"));
        // The opposite point, (-10, 0), is not on this arc, so the line still locks through the
        // centre but does not claim a point on the arc.
        let off = through_center(&s, Vec2::new(20.0, 0.0), Vec2::new(-10.0, 0.2), 0.5).expect("through, off the arc");
        assert!(off.at.dist(Vec2::new(-10.0, 0.0)) < 1e-9, "{:?}", off.at);
        assert!(off.arg.is_none());
    }

    #[test]
    fn a_construction_circle_still_snaps_and_the_nearer_centre_wins() {
        let mut s = circle();
        s.curves[0].construction = true;
        let i = through_center(&s, Vec2::new(-10.0, 0.0), Vec2::new(9.8, 0.2), 0.5).expect("construction");
        assert!(i.at.dist(Vec2::new(10.0, 0.0)) < 1e-9);
        let _ = s.add_circle(Vec2::new(0.0, 40.0), 10.0, None, Some("c2"));
        let i = through_center(&s, Vec2::new(-20.0, 0.0), Vec2::new(10.0, 0.2), 0.5).expect("nearer");
        assert_eq!(i.curve, "c1");
    }
}
