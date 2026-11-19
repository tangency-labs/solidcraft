//! Fillet and chamfer by dragging, in Creative Mode: press on or near a body edge and drag across
//! it. Pulling the pointer out, away from the material, rounds the edge (Fillet); pushing it in,
//! into the material, cuts it flat (Chamfer). The direction is read along the bisector of the
//! edge's faces on screen ([`crate::canvas::edge_bisector`]) once the pointer has moved far
//! enough; the value then follows the distance dragged that way, in steps that suit the zoom
//! (hold Alt or Ctrl for a free value), and the release applies it. A press that goes along the
//! edge instead stays a box selection.
//!
//! To round or cut every edge of a face at once, select the face first and drag any one of its
//! edges: the selection goes into the window as it is, and a selected face stands for all its
//! edges. The same holds for several selected edges: dragging one of them takes them all.

use egui::{Pos2, Vec2};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::viewport::{Hit, Proj};

/// Screen distance the pointer must travel before a press counts as a drag across the edge.
const START_PX: f32 = 6.0;
/// How closely (cosine, either way) a drag must follow the edge's bisector on screen.
const ACROSS: f32 = 0.5;
/// How far from the press, on screen, an edge may be and still be grabbed: the pick's own band
/// is narrow, and a face or a corner beside the edge would otherwise take the press.
const REACH_PX: [f32; 2] = [4.0, 9.0];

/// A press on an edge that may turn into a fillet or a chamfer.
#[derive(Clone, Debug)]
pub struct Gesture {
    press: Pos2,
    sel: Sel,
    /// The edge's point under the press, and the unit direction out of the material there.
    base: Vec3,
    out: Vec3,
    /// Where along `out` the press was: the value is measured from here.
    grab: f64,
    /// The edge's direction on screen at the press (its nearest segment), when it shows.
    along: Option<Vec2>,
    /// The drag went in (Chamfer) rather than out (Fillet); set when it starts.
    push: bool,
    /// The window is open and follows the pointer.
    started: bool,
    /// The edge is part of the selection (one of its edges, or an edge of a selected face): the
    /// drag takes the whole selection rather than this edge alone.
    keep: bool,
}

#[derive(Debug, Default)]
pub struct State {
    gesture: Option<Gesture>,
}

/// A fillet or chamfer drag has started: the box selection stays out of it.
pub fn started(app: &SolveApp) -> bool {
    app.viewport.drag_fillet.gesture.as_ref().is_some_and(|g| g.started)
}

/// The edge at or near `at`: the hits there first, then a few points around it, nearest first.
fn edge_near(app: &SolveApp, proj: &Proj, at: Pos2) -> Option<(Hit, Sel)> {
    let edge_at = |p: Pos2| {
        let hits = crate::viewport::pick(app, proj, p);
        crate::viewport::candidates(app, &hits).into_iter().find(|(h, s)| matches!((h, s), (Hit::Edge { .. }, Sel::Edge { .. })))
    };
    edge_at(at).or_else(|| {
        REACH_PX.iter().find_map(|r| {
            (0..8).find_map(|k| {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                edge_at(at + Vec2::angled(a) * *r)
            })
        })
    })
}

/// Note a press at `at` when Creative Mode is on and it is on or near a body edge. The edge wins
/// over a face pull from the same press.
pub fn press(app: &mut SolveApp, proj: &Proj, at: Pos2) {
    app.viewport.drag_fillet.gesture = None;
    if !crate::creative_mode::on(app) || app.dialog.is_some() {
        return;
    }
    let Some((Hit::Edge { body, index, point, .. }, sel)) = edge_near(app, proj, at) else { return };
    let Some(out) = app.session.world_state().ok().and_then(|st| st.body(&body).and_then(|b| crate::canvas::edge_bisector(&b.mesh(), index, point)))
    else {
        return;
    };
    let Some(grab) = crate::canvas::along(proj, at, point, out) else { return };
    let along = app.session.world_state().ok().and_then(|st| edge_on_screen(&st.body(&body)?.mesh(), index, proj, at));
    crate::drag_extrude::forget(app);
    let keep = in_selection(app, &body, index);
    app.viewport.drag_fillet.gesture = Some(Gesture { press: at, sel, base: point, out, grab, along, push: false, started: false, keep });
}

/// The screen direction of edge `index`'s segment nearest the press at `at`.
fn edge_on_screen(m: &solidcraft_engine::geom::Mesh, index: usize, proj: &Proj, at: Pos2) -> Option<Vec2> {
    let line: Vec<Pos2> = m.edges.get(index)?.iter().filter_map(|p| proj.to_screen(*p)).collect();
    line.windows(2)
        .map(|w| (w[0], w[1]))
        .filter(|(a, b)| (*b - *a).length() > 1e-3)
        .min_by(|x, y| crate::viewport::seg_dist(at, x.0, x.1).total_cmp(&crate::viewport::seg_dist(at, y.0, y.1)))
        .map(|(a, b)| b - a)
}

/// Whether edge `index` of `body` is selected, or lies on a selected face, while the selection is
/// only edges and faces (anything else would not go into a fillet).
fn in_selection(app: &SolveApp, body: &str, index: usize) -> bool {
    let sel = &app.session.selection;
    if sel.is_empty() || !sel.iter().all(|x| matches!(x, Sel::Edge { .. } | Sel::Face { .. })) {
        return false;
    }
    let st = crate::open_meshing::session_world(&app.session);
    sel.iter().any(|x| match x {
        Sel::Edge { body: b, index: i, .. } => b == body && *i == index,
        Sel::Face { body: b, index: f, .. } => {
            b == body && st.body(b).is_some_and(|bd| bd.mesh().face_edges(u32::try_from(*f).unwrap_or(u32::MAX)).into_iter().any(|e| e == index))
        }
        _ => false,
    })
}

/// Whether the move from the press to `p` crosses the edge on screen, and if so whether it goes
/// in (a push) or out (a pull).
fn crossing(proj: &Proj, g: &Gesture, p: Pos2) -> Option<bool> {
    let delta = p - g.press;
    if delta.length() < START_PX {
        return None;
    }
    let n = proj.to_screen(g.base).zip(proj.to_screen(g.base + g.out)).map(|(a, b)| b - a).filter(|n| n.length() > 1e-3)?;
    let unit = delta / delta.length();
    let c = (n / n.length()).dot(unit);
    // A drag that follows the edge more than it crosses it is a box selection, as is one where
    // the edge and the way out of the material show the same way on screen.
    if g.along.is_some_and(|t| (t / t.length()).dot(unit).abs() >= c.abs()) {
        return None;
    }
    (c.abs() > ACROSS).then_some(c < 0.0)
}

/// The pointer is at `p` with the button down.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(mut g) = app.viewport.drag_fillet.gesture.clone() else { return };
    if !g.started {
        if app.dialog.is_some() || crate::drag_extrude::started(app) {
            return;
        }
        let Some(push) = crossing(proj, &g, p) else { return };
        if !g.keep {
            crate::viewport::select(app, Some(g.sel.clone()), false, false);
        }
        app.start(if push { "solid.chamfer" } else { "solid.fillet" });
        if !matches!(app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Fillet { chamfer, .. }) if *chamfer == push) {
            app.viewport.drag_fillet.gesture = None;
            return;
        }
        g.push = push;
        g.started = true;
    }
    let Some(v) = crate::canvas::along(proj, p, g.base, g.out) else {
        app.viewport.drag_fillet.gesture = Some(g);
        return;
    };
    let step = crate::canvas::snap_step(app.cam.half_height());
    let free = crate::tool_modifiers::manipulator_free();
    let travelled = if g.push { g.grab - v } else { v - g.grab }.max(0.0);
    let value = if free { travelled } else { (travelled / step).round() * step };
    // A fillet or chamfer needs some size: the smallest step keeps the window valid until the
    // pointer moves on.
    let value = if value > 0.0 { value } else { step };
    app.viewport.drag_fillet.gesture = Some(g);
    let text = if free {
        crate::canvas::format_len(&app.session.doc, value)
    } else {
        crate::canvas::format_len_drag(&app.session.doc, value, step, app.preferences.length_decimals)
    };
    if let Some(Kind::Fillet { radius, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) {
        *radius = text;
    }
}

/// The button is up: the release applies the fillet or chamfer as OK would.
pub fn end(app: &mut SolveApp) {
    let dragged = app.viewport.drag_fillet.gesture.take().is_some_and(|g| g.started);
    if dragged && let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, Kind::Fillet { .. })) {
        d.confirm = true;
    }
}
