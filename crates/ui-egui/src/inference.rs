//! Drawing-time inference for the Line tool: while the next point is placed the
//! cursor snaps to directions and places taken from the existing geometry (parallel or
//! perpendicular to a line, on the extension of a line, tangent to a circle or arc, or through
//! the centre of a circle or arc). A dashed guide and a label show it; placing the point adds
//! the matching constraint.

use std::cell::RefCell;

use egui::{Shape, Stroke};
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
        painter.add(Shape::dashed_line(&[a, b], Stroke::new(1.0, tk.rubber_band), 5.0, 4.0));
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
