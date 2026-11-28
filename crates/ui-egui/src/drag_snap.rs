//! Snapping while a sketch point or a line is dragged: the dragged point snaps to other points
//! (ends, centres, the origin, projected points), to the middle of a line and onto curves, and
//! lines up horizontally or vertically with other points (a dashed guide). Letting go on a point,
//! a midpoint or a curve adds the matching constraint; lining up adds none. The suppress-snap
//! chord is configurable (default Ctrl/Cmd or Alt/Option; see `prefs::suppress_sketch_snap`).

use solidcraft_engine::geom::prelude::*;
use std::cell::{Cell, RefCell};

use egui::Stroke;
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Sketch};

use crate::SolveApp;
use crate::viewport::Proj;
use solidcraft_theme::Tokens;

/// What the dragged point or curve snapped to.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Another point (its id): coincident on release.
    Point(String),
    /// The middle of a line (its id): midpoint on release.
    Mid(String),
    /// On a curve (its id): point on curve on release.
    On(String),
    /// Tangent between a dragged curve and another curve: tangent on release.
    Tangent { dragged: String, target: String },
    /// Lined up with other points: no constraint.
    Align,
}

/// A snap: the moving point or curve (its id) and where it goes, the move added to the cursor's,
/// the target, and the dashed guides (sketch coordinates).
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub point: String,
    pub at: Vec2,
    pub offset: Vec2,
    pub target: Target,
    pub guides: Vec<(Vec2, Vec2)>,
}

impl Snap {
    pub fn label(&self) -> &'static str {
        match self.target {
            Target::Point(_) => "Pt",
            Target::Mid(_) => "Mid",
            Target::On(_) => "On",
            Target::Tangent { .. } => "T",
            Target::Align => "",
        }
    }
}

/// Snap distances, in screen pixels.
const POINT_PX: f64 = 8.0;
const CURVE_PX: f64 = 6.0;
const ALIGN_PX: f64 = 6.0;
/// Maximum distance from the mouse cursor for a drag snap candidate to activate.
const CURSOR_PX: f64 = 24.0;

thread_local! {
    static CURRENT: RefCell<Option<Snap>> = const { RefCell::new(None) };
    static SUPPRESSED: Cell<bool> = const { Cell::new(false) };
}

/// Whether sketch snap is suppressed this frame (set from the viewport via `prefs::suppress_sketch_snap_held`).
pub fn set_suppressed(on: bool) {
    SUPPRESSED.with(|s| s.set(on));
}

pub fn suppressed() -> bool {
    SUPPRESSED.with(Cell::get)
}

pub fn current() -> Option<Snap> {
    CURRENT.with(|c| c.borrow().clone())
}

pub fn clear() {
    CURRENT.with(|c| *c.borrow_mut() = None);
}

fn keep(s: Option<Snap>) -> Option<Snap> {
    CURRENT.with(|c| *c.borrow_mut() = s.clone());
    s
}

/// The best snap for points moving to `moving` (point index, position after the cursor's move),
/// `mm_px` millimetres per pixel. Points in `exclude` (the ones moving with the drag) are not
/// targets, nor are curves using them.
pub fn find(sk: &Sketch, moving: &[(usize, Vec2)], exclude: &[usize], mm_px: f64) -> Option<Snap> {
    find_inner(sk, None, None, Vec2::ZERO, moving, exclude, mm_px)
}

/// Cursor-scoped drag snap for an interactive viewport drag of `dragged_entity` (a point or curve
/// id) with the pointer at `cursor` (moving by `delta`). Only snaps near `cursor` activate, and
/// curve-to-curve tangent snaps are evaluated for dragged lines and resized circles/arcs.
pub fn find_for_drag(
    sk: &Sketch,
    dragged_entity: &str,
    cursor: Vec2,
    delta: Vec2,
    moving: &[(usize, Vec2)],
    exclude: &[usize],
    mm_px: f64,
) -> Option<Snap> {
    find_inner(sk, Some(dragged_entity), Some(cursor), delta, moving, exclude, mm_px)
}

fn already_coincident(sk: &Sketch, mi: usize, ti: usize) -> bool {
    mi == ti
        || sk
            .constraints
            .iter()
            .any(|c| !c.driven && matches!(c.kind, ConstraintKind::Coincident { p, q } if (p == mi && q == ti) || (p == ti && q == mi)))
}

fn already_midpoint(sk: &Sketch, mi: usize, ci: usize) -> bool {
    sk.constraints.iter().any(|c| !c.driven && matches!(c.kind, ConstraintKind::Midpoint { p, l } if p == mi && l == ci))
}

fn already_on_curve(sk: &Sketch, mi: usize, ci: usize) -> bool {
    sk.constraints.iter().any(|c| {
        !c.driven
            && matches!(
                c.kind,
                ConstraintKind::PointOnCurve { p, c: curve }
                    | ConstraintKind::PointAlong { p, c: curve, .. }
                    | ConstraintKind::Midpoint { p, l: curve }
                    if p == mi && curve == ci
            )
    })
}

/// Returns `(x_constrained, y_constrained)` for point `mi` against non-moving geometry so `Align`
/// does not fight an existing constraint and cause drag flickering.
fn point_axis_constrained(sk: &Sketch, mi: usize, moving: &[(usize, Vec2)], exclude: &[usize]) -> (bool, bool) {
    let is_moving = |pt: usize| moving.iter().any(|&(m, _)| m == pt);
    let mut x_locked = false;
    let mut y_locked = false;
    for c in &sk.constraints {
        if c.driven {
            continue;
        }
        match c.kind {
            ConstraintKind::Fix { p } | ConstraintKind::Pierce { p, .. } if p == mi => {
                return (true, true);
            }
            ConstraintKind::PointOnCurve { p, c: curve }
            | ConstraintKind::PointAlong { p, c: curve, .. }
            | ConstraintKind::Midpoint { p, l: curve }
                if p == mi =>
            {
                let moves_with_drag = sk.curves.get(curve).is_some_and(|cu| cu.kind.point_ids().iter().all(|pt| exclude.contains(pt)));
                if !moves_with_drag {
                    return (true, true);
                }
            }
            ConstraintKind::Horizontal { l } => {
                if let Some(CurveKind::Line { a, b }) = sk.curves.get(l).map(|cu| &cu.kind)
                    && ((*a == mi && !is_moving(*b)) || (*b == mi && !is_moving(*a)))
                {
                    y_locked = true;
                }
            }
            ConstraintKind::Vertical { l } => {
                if let Some(CurveKind::Line { a, b }) = sk.curves.get(l).map(|cu| &cu.kind)
                    && ((*a == mi && !is_moving(*b)) || (*b == mi && !is_moving(*a)))
                {
                    x_locked = true;
                }
            }
            ConstraintKind::HorizontalPoints { p, q } | ConstraintKind::DistanceY { p, q, .. }
                if (p == mi && !is_moving(q)) || (q == mi && !is_moving(p)) =>
            {
                y_locked = true;
            }
            ConstraintKind::VerticalPoints { p, q } | ConstraintKind::DistanceX { p, q, .. }
                if (p == mi && !is_moving(q)) || (q == mi && !is_moving(p)) =>
            {
                x_locked = true;
            }
            _ => {}
        }
    }
    (x_locked, y_locked)
}

fn already_aligned_with(sk: &Sketch, mi: usize, ti: usize, horizontal: bool) -> bool {
    sk.constraints.iter().any(|c| {
        if c.driven {
            return false;
        }
        match c.kind {
            ConstraintKind::HorizontalPoints { p, q } if horizontal => (p == mi && q == ti) || (p == ti && q == mi),
            ConstraintKind::VerticalPoints { p, q } if !horizontal => (p == mi && q == ti) || (p == ti && q == mi),
            ConstraintKind::Horizontal { l } if horizontal => matches!(
                sk.curves.get(l).map(|cu| &cu.kind),
                Some(CurveKind::Line { a, b }) if (*a == mi && *b == ti) || (*a == ti && *b == mi)
            ),
            ConstraintKind::Vertical { l } if !horizontal => matches!(
                sk.curves.get(l).map(|cu| &cu.kind),
                Some(CurveKind::Line { a, b }) if (*a == mi && *b == ti) || (*a == ti && *b == mi)
            ),
            _ => false,
        }
    })
}

fn find_inner(
    sk: &Sketch,
    dragged_entity: Option<&str>,
    cursor: Option<Vec2>,
    delta: Vec2,
    moving: &[(usize, Vec2)],
    exclude: &[usize],
    mm_px: f64,
) -> Option<Snap> {
    if suppressed() || !(mm_px > 0.0 && mm_px.is_finite()) || (moving.is_empty() && dragged_entity.is_none()) {
        return keep(None);
    }
    let cursor_tol = CURSOR_PX * mm_px;
    let near_cursor = |pt: Vec2| cursor.is_none_or(|c| pt.dist(c) <= cursor_tol);
    let id = |i: usize| sk.points.get(i).map(|p| p.id.clone()).unwrap_or_default();
    let visible = solidcraft_engine::view::SketchPointVisibility::new(sk);
    let targets: Vec<(usize, Vec2)> =
        sk.points.iter().enumerate().filter(|(i, p)| !exclude.contains(i) && visible.takes(p)).map(|(i, p)| (i, p.pos)).collect();
    let free_curves: Vec<usize> =
        (0..sk.curves.len()).filter(|&c| sk.curves.get(c).is_some_and(|cu| !exclude.iter().any(|&p| cu.kind.uses(p)))).collect();
    let offer = |best: &mut Option<(f64, Snap)>, d: f64, s: Snap| {
        if best.as_ref().is_none_or(|b| d < b.0) {
            *best = Some((d, s));
        }
    };
    let snap_points = |filter_near: bool| -> Option<(f64, Snap)> {
        let mut best: Option<(f64, Snap)> = None;
        let matches = |pt: Vec2| if filter_near { near_cursor(pt) } else { !near_cursor(pt) };
        for &(mi, p) in moving {
            if !matches(p) {
                continue;
            }
            for &(ti, t) in &targets {
                if already_coincident(sk, mi, ti) {
                    continue;
                }
                let d = p.dist(t);
                if d < POINT_PX * mm_px {
                    offer(&mut best, d, Snap { point: id(mi), at: t, offset: t - p, target: Target::Point(id(ti)), guides: Vec::new() });
                }
            }
        }
        if best.is_none() {
            for &(mi, p) in moving {
                if !matches(p) {
                    continue;
                }
                for &c in &free_curves {
                    if already_midpoint(sk, mi, c) {
                        continue;
                    }
                    let Some(cu) = sk.curves.get(c) else { continue };
                    let CurveKind::Line { a, b } = cu.kind else { continue };
                    let (Some(a), Some(b)) = (sk.point(a), sk.point(b)) else { continue };
                    let m = (a + b) * 0.5;
                    let d = p.dist(m);
                    if d < POINT_PX * mm_px {
                        offer(&mut best, d, Snap { point: id(mi), at: m, offset: m - p, target: Target::Mid(cu.id.clone()), guides: Vec::new() });
                    }
                }
            }
        }
        if filter_near
            && best.is_none()
            && let (Some(entity), Some(c)) = (dragged_entity, cursor)
            && let Some(hit) = solidcraft_engine::sketch::find_tangent_drag(sk, entity, c, delta, exclude, CURVE_PX * mm_px, cursor_tol)
        {
            offer(
                &mut best,
                hit.gap,
                Snap {
                    point: entity.to_string(),
                    at: hit.at,
                    offset: hit.offset,
                    target: Target::Tangent { dragged: entity.to_string(), target: hit.target },
                    guides: Vec::new(),
                },
            );
        }
        if best.is_none() {
            for &(mi, p) in moving {
                if !matches(p) {
                    continue;
                }
                for &c in &free_curves {
                    if already_on_curve(sk, mi, c) {
                        continue;
                    }
                    let (Some(cu), Some(sh)) = (sk.curves.get(c), sk.shape(c)) else { continue };
                    let d = sh.dist(p);
                    if d < CURVE_PX * mm_px {
                        let q = sh.project(p);
                        offer(&mut best, d, Snap { point: id(mi), at: q, offset: q - p, target: Target::On(cu.id.clone()), guides: Vec::new() });
                    }
                }
            }
        }
        best
    };
    // Snaps near the cursor take priority (points, midpoints, curve-to-curve tangents, on-curve)
    // so far-away endpoints never hijack a drag near the pointer; if nothing snapped near the
    // cursor, moving points farther from the pointer still snap (e.g. a line dragged by its body).
    if let Some((_, s)) = snap_points(true).or_else(|| cursor.and_then(|_| snap_points(false))) {
        return keep(Some(s));
    }
    // Lining up: the nearest horizontal and the nearest vertical alignment, each on its own axis,
    // preferring points near the cursor and skipping axes already locked by driving constraints so
    // existing constraints never flicker.
    let tol = ALIGN_PX * mm_px;
    let align_pass = |filter_near: bool| -> (Option<(f64, usize, Vec2, Vec2)>, Option<(f64, usize, Vec2, Vec2)>) {
        let matches = |pt: Vec2| if filter_near { near_cursor(pt) } else { !near_cursor(pt) };
        let (mut hx, mut vy): (Option<(f64, usize, Vec2, Vec2)>, Option<(f64, usize, Vec2, Vec2)>) = (None, None);
        for &(mi, p) in moving {
            if !matches(p) {
                continue;
            }
            let (x_locked, y_locked) = point_axis_constrained(sk, mi, moving, exclude);
            for &(ti, t) in &targets {
                let (dy, dx) = ((p.y - t.y).abs(), (p.x - t.x).abs());
                if !y_locked && !already_aligned_with(sk, mi, ti, true) && dy < tol && dx > tol && hx.is_none_or(|h| dy < h.0) {
                    hx = Some((dy, mi, p, t));
                }
                if !x_locked && !already_aligned_with(sk, mi, ti, false) && dx < tol && dy > tol && vy.is_none_or(|v| dx < v.0) {
                    vy = Some((dx, mi, p, t));
                }
            }
        }
        (hx, vy)
    };
    let (mut hx, mut vy) = align_pass(true);
    if cursor.is_some() && hx.is_none() && vy.is_none() {
        (hx, vy) = align_pass(false);
    }
    let mut offset = Vec2::ZERO;
    let mut guides = Vec::new();
    let mut point = None;
    if let Some((_, mi, p, t)) = hx {
        offset.y = t.y - p.y;
        point = Some(mi);
        guides.push((t, Vec2::new(p.x, t.y)));
    }
    if let Some((_, mi, p, t)) = vy {
        offset.x = t.x - p.x;
        point = point.or(Some(mi));
        guides.push((t, Vec2::new(t.x, p.y)));
    }
    // The guides end at the snapped point (both alignments may move it).
    let Some(mi) = point else { return keep(None) };
    let at = moving.iter().find(|m| m.0 == mi).map_or(Vec2::ZERO, |m| m.1) + offset;
    for g in &mut guides {
        g.1 = if (g.0.y - g.1.y).abs() < 1e-12 { Vec2::new(at.x, g.0.y) } else { Vec2::new(g.0.x, at.y) };
    }
    keep(Some(Snap { point: id(mi), at, offset, target: Target::Align, guides }))
}

/// Returns true if the solved sketch `sk` actually satisfied `s` after `sketch.drag`. When an
/// existing constraint prevents the geometry from reaching `s`, callers fall back to the unsnapped
/// drag target and clear the snap so the geometry follows the existing constraint without flicker.
pub fn satisfied_after_drag(sk: &Sketch, s: &Snap, mm_px: f64) -> bool {
    let tol = (0.5 * mm_px).max(1e-3);
    match &s.target {
        Target::Point(_) | Target::Mid(_) | Target::On(_) => {
            sk.point_index(&s.point).and_then(|pi| sk.point(pi)).is_some_and(|p| p.dist(s.at) <= tol)
        }
        Target::Align => {
            let Some(p) = sk.point_index(&s.point).and_then(|pi| sk.point(pi)) else { return false };
            s.guides.iter().all(|(from, to)| if (from.y - to.y).abs() < 1e-9 { (p.y - from.y).abs() <= tol } else { (p.x - from.x).abs() <= tol })
        }
        Target::Tangent { dragged, target } => solidcraft_engine::sketch::curves_tangent_within(sk, dragged, target, tol),
    }
}

/// Points the drag of `entities` (point or curve ids: the one grabbed and those dragged with it)
/// moves, and the points that move with them.
pub fn moving_points<'a>(sk: &Sketch, entities: impl IntoIterator<Item = &'a str>) -> (Vec<usize>, Vec<usize>) {
    let mut own: Vec<usize> = entities
        .into_iter()
        .flat_map(|entity| {
            if let Some(p) = sk.point_index(entity) {
                vec![p]
            } else {
                match sk.curve_index(entity).and_then(|c| sk.curves.get(c)) {
                    // Only lines slide whole; a circle or arc grabbed on its edge changes its radius.
                    Some(c) if matches!(c.kind, CurveKind::Line { .. }) => c.kind.point_ids(),
                    _ => Vec::new(),
                }
            }
        })
        .collect();
    // A point held at the middle of a line (a centre rectangle's centre on its diagonal) goes
    // where the line's ends go, and dragging it takes the line along. So it moves, and snaps,
    // with a line whose ends both move; and when it is dragged, its line and the shape round it
    // are no targets for it: they held it at their own middle instead of lining it up (#10086).
    let middles: Vec<(usize, usize, usize)> = sk
        .constraints
        .iter()
        .filter(|c| !c.driven)
        .filter_map(|c| match c.kind {
            ConstraintKind::Midpoint { p, l } => match sk.curves.get(l).map(|c| &c.kind) {
                Some(CurveKind::Line { a, b }) => Some((p, *a, *b)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for &(p, a, b) in &middles {
        if own.contains(&a) && own.contains(&b) && !own.contains(&p) {
            own.push(p);
        }
    }
    own.sort_unstable();
    own.dedup();
    let mut moved = own.clone();
    moved.extend(middles.iter().filter(|(p, ..)| own.contains(p)).flat_map(|&(_, a, b)| [a, b]));
    let mut ex = moved.clone();
    for c in &sk.curves {
        if moved.iter().any(|&p| c.kind.uses(p)) {
            ex.extend(c.kind.point_ids());
        }
    }
    ex.sort_unstable();
    ex.dedup();
    (own, ex)
}

/// The drag let go: the constraint of the snap it ended on (none for lining up), as
/// `sketch.drag_end` takes it.
pub fn release() -> Option<Value> {
    let s = current()?;
    clear();
    let (cmd, params) = match &s.target {
        Target::Point(t) => ("sketch.constraint.coincident", json!({"a": s.point, "b": t})),
        Target::Mid(l) => ("sketch.constraint.midpoint", json!({"point": s.point, "line": l})),
        Target::On(c) => ("sketch.constraint.coincident", json!({"a": s.point, "b": c})),
        Target::Tangent { dragged, target } => ("sketch.constraint.tangent", json!({"a": dragged, "b": target})),
        Target::Align => return None,
    };
    Some(json!({"command": cmd, "params": params}))
}

/// The snap of the drag in progress: its dashed guides, target highlight, and constraint icon badge.
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.viewport.point_drag.is_none() {
        return;
    }
    let Some(s) = current() else { return };
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = app.session.active_sketch.and_then(|i| st.sketch(i)) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    let tk = Tokens::get();
    for (a, b) in &s.guides {
        if let (Some(a), Some(b)) = (to(*a), to(*b)) {
            painter.extend(crate::ui_kit::dashed_line(&[a, b], painter.clip_rect(), Stroke::new(1.0, tk.rubber_band), 5.0, 4.0));
        }
    }
    match &s.target {
        Target::Point(id) => {
            if let Some(pt_sp) = ss.sketch.point_index(id).and_then(|pi| ss.sketch.point(pi)).and_then(to) {
                crate::tools::highlight_point(painter, pt_sp);
            }
        }
        Target::Mid(id) | Target::On(id) | Target::Tangent { target: id, .. } => {
            if let Some(ci) = ss.sketch.curve_index(id) {
                let mut display_budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
                crate::tools::highlight_curve(&ss.sketch, ci, &ss.plane, painter, proj, &mut display_budget);
            }
        }
        Target::Align => {
            for (from, _) in &s.guides {
                if let Some(pt_sp) = to(*from) {
                    crate::tools::highlight_point(painter, pt_sp);
                }
            }
        }
    }
    let Some(sp) = to(s.at) else { return };
    let label = s.label();
    if !label.is_empty() {
        crate::sketch_tools::paint_snap_badges(app, painter, proj.rect, sp, &[label], false);
    }
    crate::scenario::publish_count("drag_guides", s.guides.len() as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk() -> Sketch {
        let mut s = Sketch::new();
        let _ = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), None, None, Some("l1"));
        let _ = s.add_line(Vec2::new(20.0, 5.0), Vec2::new(30.0, 15.0), None, None, Some("l2"));
        s
    }

    fn drag(s: &Sketch, entity: &str, p: Vec2) -> Option<Snap> {
        let (own, ex) = moving_points(s, [entity]);
        find(s, &[(own[0], p)], &ex, 0.1)
    }

    /// A centre rectangle 30 × 20 round `center`, as Center Rectangle draws it: the centre `p5`
    /// held at the middle of the diagonal `l5`.
    fn centre_rectangle(center: [f64; 2]) -> Sketch {
        let mut s = solidcraft_engine::Session::default();
        let _ = s.execute("sketch.create", &json!({"plane": "XY"}));
        let corner = [center[0] + 15.0, center[1] + 10.0];
        let _ = s.execute("sketch.rectangle.center", &json!({"center": center, "corner": corner}));
        s.active_sketch.and_then(|id| s.doc.sketch(id).ok()).cloned().unwrap_or_else(Sketch::new)
    }

    /// #10086: dragged by its centre, a centre rectangle comes along with it, and the centre
    /// lines up with the origin (the X axis here), as a circle's centre does. Its own diagonal,
    /// moving with it, held it at the diagonal's middle instead. Each frame of a drag snaps
    /// against the shape where the last frame left it: here just above the axis.
    #[test]
    fn a_centre_rectangles_centre_lines_up_when_dragged() {
        let s = centre_rectangle([30.0, 0.5]);
        let snap = drag(&s, "p5", Vec2::new(30.0, 0.3)).expect("a snap");
        assert_eq!(snap.target, Target::Align, "{snap:?}");
        assert_eq!(snap.at, Vec2::new(30.0, 0.0));
    }

    /// #10086: the whole rectangle dragged (its four sides selected) moves its centre too, and the
    /// centre lines up as its corners do.
    #[test]
    fn a_dragged_centre_rectangle_moves_and_lines_up_its_centre() {
        let s = centre_rectangle([30.0, 5.0]);
        let centre = s.resolve_point("p5").unwrap_or(0);
        let (own, ex) = moving_points(&s, ["l1", "l2", "l3", "l4"]);
        assert!(own.contains(&centre) && ex.contains(&centre), "{own:?} {ex:?}");
        let d = Vec2::new(0.0, -4.7);
        let moving: Vec<(usize, Vec2)> = own.iter().filter_map(|&p| s.point(p).map(|x| (p, x + d))).collect();
        let snap = find(&s, &moving, &ex, 0.1).expect("a snap");
        assert_eq!((snap.point.as_str(), &snap.target), ("p5", &Target::Align), "{snap:?}");
        assert_eq!(snap.at, Vec2::new(30.0, 0.0));
    }

    #[test]
    fn a_dragged_end_snaps_to_points_midpoints_and_curves() {
        let s = sk();
        let end = s.resolve_point("l2.start").map(|i| s.points[i].id.clone()).unwrap_or_default();
        let at = drag(&s, &end, Vec2::new(10.3, 0.4)).expect("point");
        assert_eq!(at.target, Target::Point(s.points[s.resolve_point("l1.end").unwrap_or(0)].id.clone()));
        assert_eq!(at.at, Vec2::new(10.0, 0.0));
        let mid = drag(&s, &end, Vec2::new(5.2, 0.3)).expect("mid");
        assert_eq!(mid.target, Target::Mid("l1".into()));
        let on = drag(&s, &end, Vec2::new(7.5, 0.3)).expect("on");
        assert_eq!(on.target, Target::On("l1".into()));
        assert!(on.at.y.abs() < 1e-12);
    }

    #[test]
    fn lining_up_moves_the_point_but_names_no_constraint() {
        let s = sk();
        let end = s.points[s.resolve_point("l2.end").unwrap_or(0)].id.clone();
        // Level with l1's end (y 0), far from it in x.
        let a = drag(&s, &end, Vec2::new(40.0, 0.35)).expect("aligned");
        assert_eq!(a.target, Target::Align);
        assert_eq!(a.at.y, 0.0);
        assert_eq!(a.guides.len(), 1);
        assert!(drag(&s, &end, Vec2::new(40.0, 3.0)).is_none());
        set_suppressed(true);
        assert!(drag(&s, &end, Vec2::new(10.1, 0.1)).is_none());
        set_suppressed(false);
    }

    #[test]
    fn show_renders_icon_badge_and_selection_highlight_without_text_label() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 0], [10, 0]], "infer": false})).unwrap();
        h.app.run("sketch.line", json!({"points": [[20, 5], [30, 15]], "infer": false})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();

        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let proj = crate::viewport::projection(&h.app, rect);
        h.app.viewport.point_drag = Some(("p3".into(), 0, None, Vec2::ZERO, true));
        CURRENT.with(|c| {
            *c.borrow_mut() =
                Some(Snap { point: "p3".into(), at: Vec2::new(5.0, 0.0), offset: Vec2::ZERO, target: Target::On("l1".into()), guides: Vec::new() });
        });

        let expected_stroke = Stroke::new(3.5, Tokens::view().sel_edge);
        let run_once = |scale: f32, app: &mut SolveApp| {
            app.preferences.constraint_glyph_scale = scale;
            let ctx = crate::fonts::test_context();
            ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                let painter = ui.ctx().layer_painter(egui::LayerId::background());
                show(app, &painter, &proj);
            })
        };

        let out_1x = run_once(1.0, &mut h.app);
        let has_target_curve =
            out_1x.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Path(path) if path.stroke == expected_stroke.into()));
        let has_text = out_1x.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(_)));
        let badge_h_1x = out_1x
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => Some(r.rect.height()),
                _ => None,
            })
            .expect("badge rect at 1x");
        assert!(has_target_curve, "drag target curve should be highlighted with sel_edge");
        assert!(!has_text, "drag snap On badge should use an SVG icon, not text");
        assert!((badge_h_1x - crate::sketch_tools::glyph_px(1.0)).abs() < 0.05);

        let out_2x = run_once(2.0, &mut h.app);
        let badge_h_2x = out_2x
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Rect(r) => Some(r.rect.height()),
                _ => None,
            })
            .expect("badge rect at 2x");
        assert!((badge_h_2x - crate::sketch_tools::glyph_px(2.0)).abs() < 0.05);

        clear();
        h.app.viewport.point_drag = None;
    }

    #[test]
    fn line_drag_ignores_far_endpoint_and_snaps_tangent_near_cursor() {
        let mut s = Sketch::new();
        // Long horizontal line l1 from (0, 10) to (40, 10), another point near its left end (0, 10.2),
        // and a circle c1 centered at (20, 0) with radius 10 (top tangent point at (20, 10)).
        let _ = s.add_line(Vec2::new(0.0, 10.0), Vec2::new(40.0, 10.0), None, None, Some("l1"));
        let _ = s.add_point(Vec2::new(0.0, 10.2), Some("p_far"));
        let _ = s.add_circle(Vec2::new(20.0, 0.0), 10.0, None, Some("c1"));

        let (own, ex) = moving_points(&s, ["l1"]);
        // Dragging l1 near its middle (x = 20, y = 10.3) with mm_px = 0.1 (CURSOR_PX * mm_px = 2.4mm):
        // the far endpoint at x = 0 is 20mm away from the cursor, so it must NOT snap to p_far.
        let delta = Vec2::new(0.0, 0.3);
        let cursor = Vec2::new(20.0, 10.3);
        let moving: Vec<(usize, Vec2)> = own.iter().filter_map(|&p| s.point(p).map(|x| (p, x + delta))).collect();
        let snap = find_for_drag(&s, "l1", cursor, delta, &moving, &ex, 0.1).expect("tangent snap");
        assert_eq!(snap.target, Target::Tangent { dragged: "l1".into(), target: "c1".into() }, "{snap:?}");
        assert_eq!(snap.label(), "T");
        assert!((snap.at.dist(Vec2::new(20.0, 10.0))) < 1e-6);
        assert!((snap.offset.y + 0.3).abs() < 1e-6);
        let released = release().expect("tangent constraint on release");
        assert_eq!(released["command"], "sketch.constraint.tangent");
        assert_eq!(released["params"], json!({"a": "l1", "b": "c1"}));

        // If the cursor is near the right end (x = 35, y = 10.3), far from the tangent point (x = 20),
        // the curve-to-curve tangent snap does not activate; without a near-cursor snap, the line's
        // moving endpoint still falls back to snapping onto p_far (matching s42_drag_snap).
        let far_cursor = Vec2::new(35.0, 10.3);
        let fallback = find_for_drag(&s, "l1", far_cursor, delta, &moving, &ex, 0.1).expect("endpoint fallback snap");
        assert_eq!(fallback.target, Target::Point("p_far".into()));
        // With no endpoint target or alignment nearby, the far tangent point alone does not activate.
        let mut tangent_only = Sketch::new();
        let _ = tangent_only.add_line(Vec2::new(5.0, 10.0), Vec2::new(45.0, 10.0), None, None, Some("l1"));
        let _ = tangent_only.add_circle(Vec2::new(25.0, 0.0), 10.0, None, Some("c1"));
        let (own2, ex2) = moving_points(&tangent_only, ["l1"]);
        let moving2: Vec<(usize, Vec2)> = own2.iter().filter_map(|&p| tangent_only.point(p).map(|x| (p, x + delta))).collect();
        assert!(find_for_drag(&tangent_only, "l1", Vec2::new(40.0, 10.3), delta, &moving2, &ex2, 0.1).is_none());
        clear();
    }

    #[test]
    fn circle_edge_resize_snaps_tangent_to_line_and_circle_near_cursor() {
        let mut s = Sketch::new();
        let _ = s.add_circle(Vec2::new(0.0, 0.0), 8.0, None, Some("c1"));
        let _ = s.add_line(Vec2::new(-10.0, 10.0), Vec2::new(10.0, 10.0), None, None, Some("l1"));
        let _ = s.add_circle(Vec2::new(20.0, 0.0), 10.0, None, Some("c2"));

        let (own, ex) = moving_points(&s, ["c1"]);
        // Resizing c1 edge near top (0, 9.7) -> tangent to l1 at (0, 10).
        let snap_line = find_for_drag(&s, "c1", Vec2::new(0.0, 9.7), Vec2::ZERO, &own.iter().map(|&p| (p, Vec2::ZERO)).collect::<Vec<_>>(), &ex, 0.1)
            .expect("circle-to-line tangent");
        assert_eq!(snap_line.target, Target::Tangent { dragged: "c1".into(), target: "l1".into() });
        assert!((snap_line.at.dist(Vec2::new(0.0, 10.0))) < 1e-6);
        assert!((snap_line.offset.y - 0.3).abs() < 1e-6);

        // Resizing c1 edge near right (9.8, 0) -> externally tangent to c2 at (10, 0).
        let snap_circ = find_for_drag(&s, "c1", Vec2::new(9.8, 0.0), Vec2::ZERO, &[], &ex, 0.1).expect("circle-to-circle tangent");
        assert_eq!(snap_circ.target, Target::Tangent { dragged: "c1".into(), target: "c2".into() });
        assert!((snap_circ.at.dist(Vec2::new(10.0, 0.0))) < 1e-6);
        assert!((snap_circ.offset.x - 0.2).abs() < 1e-6);
        clear();
    }

    #[test]
    fn constrained_point_skips_conflicting_align_and_redundant_snaps() {
        let mut s = Sketch::new();
        let l1 = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), None, None, Some("l1")).unwrap();
        let _ = s.add_point(Vec2::new(30.0, 0.3), Some("p_other"));
        let p_end = s.resolve_point("l1.end").unwrap();
        let p_end_id = s.points[p_end].id.clone();
        let _ = s.add_constraint(ConstraintKind::Horizontal { l: l1 }, None);

        // p_end is constrained Horizontal to l1.start (which is not moving), so horizontal Align
        // to p_other (y = 0.3) must be skipped to prevent fighting the Horizontal constraint.
        let (own, ex) = moving_points(&s, [p_end_id.as_str()]);
        let snap = find_for_drag(&s, &p_end_id, Vec2::new(12.0, 0.1), Vec2::ZERO, &[(own[0], Vec2::new(12.0, 0.1))], &ex, 0.1);
        assert!(snap.is_none(), "expected no conflicting horizontal Align snap, got {snap:?}");

        // Already-coincident point must not re-offer Target::Point.
        let p_dup = s.add_point(Vec2::new(12.0, 0.0), Some("p_dup")).unwrap();
        let _ = s.add_constraint(ConstraintKind::Coincident { p: p_end, q: p_dup }, None);
        let snap_dup = find_for_drag(&s, &p_end_id, Vec2::new(12.0, 0.0), Vec2::ZERO, &[(own[0], Vec2::new(12.0, 0.0))], &ex, 0.1);
        assert!(snap_dup.is_none(), "expected no redundant Coincident snap, got {snap_dup:?}");
        clear();
    }
}
