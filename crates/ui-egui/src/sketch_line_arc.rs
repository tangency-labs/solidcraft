//! The Line tool's switch to tangent arcs and back (#9489), with Preferences › Sketch asking for
//! it: leave the chain's last point, come back and hold the pointer still on it. While switched
//! the tool is `sketch.arc.tangent`, so the arc is that command's and whatever asks which tool is
//! out gets the true answer; [`LineChain`] is what makes it part of the chain.

use egui::{Pos2, Stroke};
use serde_json::{Value, json};
use solidcraft_engine::geom::Vec2;
use solidcraft_engine::sketch::Sketch;

use crate::SolveApp;
use crate::tools::Tool;
use crate::viewport::Proj;

const LINE: &str = "sketch.line";
const TANGENT_ARC: &str = "sketch.arc.tangent";
/// How long the pointer rests on the point before the mode switches (seconds).
const REST_SECONDS: f64 = 0.35;
/// How near the point counts as on it (screen points).
const ON_POINT: f32 = 8.0;
/// How far the pointer must go from the point first (screen points). A click leaves the pointer on
/// the point it placed, and a pause there to think must not switch anything.
const AWAY: f32 = 20.0;
/// How far a resting pointer may drift, as a hand does (screen points).
const STILL: f32 = 2.0;

/// Where a Line tool's chain has got to.
#[derive(Clone, Debug, Default)]
pub struct LineChain {
    /// The curve the chain's last point ends. `None` until the chain has drawn one.
    curve: Option<String>,
    /// The pointer has been away from that point since it was placed or the mode last switched.
    left: bool,
    /// Where the pointer came to rest on the point, and when (the frame clock).
    rest: Option<(Pos2, f64)>,
    /// Just switched back to lines: says so until the pointer leaves.
    back_to_lines: bool,
    /// Where each segment started and the curve the chain was on there, oldest first: where it
    /// goes back to when Undo takes that segment away (#10016).
    back: Vec<((Vec2, Option<String>), Option<String>)>,
}

impl LineChain {
    /// Curve `id`, drawn from `start`, now ends the chain, with the pointer still on its end.
    pub fn placed(&mut self, id: &str, start: (Vec2, Option<String>)) {
        let mut back = std::mem::take(&mut self.back);
        back.push((start, self.curve.take()));
        *self = LineChain { curve: Some(id.to_string()), back, ..LineChain::default() };
    }
}

/// Undo can take away the segment a chain is drawing on from (#10016). The chain then goes back to
/// the end of the last segment still there, or to where it began (as lines: there is nothing to
/// be tangent to), and ends if that point has gone too. Clicking on from a point that no longer
/// exists would only be refused.
pub fn rejoin(app: &SolveApp, t: &mut Tool) {
    if t.cmd != LINE && !in_arc_mode(t) {
        return;
    }
    let Some(Some(current)) = t.pts.last().map(|p| p.1.clone()) else { return };
    let Ok(state) = app.session.world_state() else { return };
    let Some(ss) = app.session.active_sketch.and_then(|id| state.sketch(id)) else { return };
    let at = |id: &str| ss.sketch.resolve_point(id).and_then(|p| ss.sketch.point(p));
    if at(&current).is_some() {
        return;
    }
    let mut back = std::mem::take(&mut t.chain.back);
    let mut to = None;
    while let Some(((point, id), curve)) = back.pop() {
        match id {
            None => to = Some(((point, None), curve)),
            Some(id) => match at(&id) {
                Some(point) => to = Some(((point, Some(id)), curve)),
                None => continue,
            },
        }
        break;
    }
    let (pts, curve) = to.map_or((Vec::new(), None), |(point, curve)| (vec![point], curve));
    if curve.is_none()
        && t.cmd != LINE
        && let Some(line) = Tool::for_command(LINE)
    {
        *t = line;
    }
    t.pts = pts;
    t.lined.clear();
    crate::sketch_dims::reset(t);
    t.chain = LineChain { curve, back, ..LineChain::default() };
}

/// Whether the tool is a Line tool's chain drawing tangent arcs, not the Tangent Arc tool itself.
pub fn in_arc_mode(t: &Tool) -> bool {
    t.cmd == TANGENT_ARC && t.chain.curve.is_some()
}

/// Whether the tool has a chain to switch: a segment drawn, and nothing typed for the next one.
fn can_switch(t: &Tool) -> bool {
    (t.cmd == LINE || in_arc_mode(t)) && t.chain.curve.is_some() && t.pts.len() == 1 && !t.dims.iter().any(|b| b.locked)
}

/// Per frame, with `pointer` free over the view (nothing held, no camera move): notes where it has
/// been against the chain's last point, and switches after a rest there.
pub fn follow_pointer(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj, pointer: Option<Pos2>) {
    if !app.preferences.line_hover_tangent_arc || !app.tool.as_ref().is_some_and(can_switch) {
        return;
    }
    if crate::sketch3d::active(app) || app.dialog.is_some() || crate::dim_view::editing() {
        return;
    }
    let screen = {
        let state = crate::open_meshing::session_world(&app.session);
        let plane = app.session.active_sketch.and_then(|s| state.sketch(s)).map(|ss| ss.plane);
        let last = app.tool.as_ref().and_then(|t| t.pts.first()).map(|p| p.0);
        plane.zip(last).and_then(|(plane, last)| proj.to_screen(plane.to_world(last)))
    };
    let now = ctx.input(|i| i.time);
    let Some(chain) = app.tool.as_mut().map(|t| &mut t.chain) else { return };
    let (Some(p), Some(point)) = (pointer, screen) else {
        chain.rest = None;
        return;
    };
    let off = p.distance(point);
    if off >= AWAY {
        chain.left = true;
        chain.back_to_lines = false;
    }
    if off > ON_POINT || !chain.left {
        chain.rest = None;
        return;
    }
    match chain.rest {
        Some((at, since)) if at.distance(p) <= STILL => {
            if now - since >= REST_SECONDS {
                switch(app);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(REST_SECONDS - (now - since) + 0.01));
            }
        }
        _ => {
            chain.rest = Some((p, now));
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(REST_SECONDS + 0.01));
        }
    }
}

/// Lines to tangent arcs, or back. The chain's point and curve go across; the pointer has to leave
/// the point before it can switch again.
fn switch(app: &mut SolveApp) {
    let Some(old) = app.tool.as_ref() else { return };
    let to_arc = old.cmd == LINE;
    let next = if to_arc { crate::sketch_tools::tool_for(TANGENT_ARC) } else { Tool::for_command(LINE) };
    let Some(mut next) = next else { return };
    next.pts = old.pts.clone();
    next.hover = old.hover.clone();
    next.chain = LineChain { curve: old.chain.curve.clone(), back_to_lines: !to_arc, back: old.chain.back.clone(), ..LineChain::default() };
    app.tool = Some(next);
    let hint = if to_arc { app.tr("ui.line_arc_hint").to_string() } else { crate::tools::hint(LINE) };
    app.set_status(hint, false);
}

/// Right-click or Esc while drawing tangent arcs: the chain ends as a line chain does, and the Line
/// tool stays out. False when the tool isn't that.
pub fn finish(app: &mut SolveApp) -> bool {
    if !app.tool.as_ref().is_some_and(in_arc_mode) {
        return false;
    }
    app.tool = Tool::for_command(LINE);
    app.set_status(crate::tools::hint(LINE), false);
    true
}

/// A click while drawing tangent arcs: the arc, by the tangent arc command, tangent to the chain's
/// last curve, its end the chain's new last point.
pub fn place_arc(app: &mut SolveApp, tool: &mut Tool, end: &(Vec2, Option<String>)) {
    let (Some(start), Some(curve)) = (tool.pts.first().and_then(|p| p.1.clone()), tool.chain.curve.clone()) else { return };
    // On an existing point the arc ends on that point, so an outline closes. The other snaps
    // (a midpoint, a place on a curve) name no point: their coordinates.
    let on_point = |id: &&String| {
        let state = crate::open_meshing::session_world(&app.session);
        app.session.active_sketch.and_then(|s| state.sketch(s)).is_some_and(|ss| ss.sketch.resolve_point(id).is_some())
    };
    let to = end.1.as_ref().filter(on_point).map_or_else(|| json!([end.0.x, end.0.y]), |id| json!(id));
    // A failure (the end straight ahead) has said why in the status line; the chain stays as it is.
    let made = crate::tools::draw(app, tool, json!({"start": start, "curve": curve, "end": to}));
    if let Some(id) = made.as_ref().ok().and_then(|v| v["curves"].get(0)).and_then(Value::as_str) {
        let drawn_from = tool.pts.first().cloned();
        tool.pts = vec![(end.0, Some(format!("{id}.end")))];
        if let Some(start) = drawn_from {
            tool.chain.placed(id, start);
        }
    }
}

/// The arc a click would make with the pointer at `hover`, for the preview.
pub fn preview_arc(sketch: &Sketch, t: &Tool, hover: Vec2) -> Option<Sketch> {
    let start = t.pts.first()?.1.as_deref()?;
    solidcraft_engine::cmd::tangent_arc_preview(sketch, t.chain.curve.as_deref()?, start, hover)
}

/// What the cursor says the next click makes (command, icon): while drawing tangent arcs, and just
/// after switching back to lines.
fn shown_mode(t: &Tool) -> Option<(&'static str, &'static str)> {
    if in_arc_mode(t) {
        Some((TANGENT_ARC, "arc_tangent"))
    } else if t.cmd == LINE && t.chain.back_to_lines {
        Some((LINE, "line"))
    } else {
        None
    }
}

/// The words next to the cursor, in the interface language.
pub(crate) fn shown_label(t: &Tool) -> Option<&'static str> {
    shown_mode(t).map(|(command, _)| crate::i18n::tr_label(crate::command_label(command)))
}

/// The label next to the cursor (below the point's coordinates).
pub fn paint_mode(app: &SolveApp, ui: &egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let Some((command, icon)) = app.tool.as_ref().and_then(shown_mode) else { return };
    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()).filter(|p| proj.rect.contains(*p)) else { return };
    let tk = crate::theme::Tokens::view();
    let Some(label) = app.tool.as_ref().and_then(shown_label) else { return };
    let galley = painter.layout_no_wrap(label.to_string(), egui::FontId::proportional(tk.font_small), tk.text);
    let side = galley.size().y.max(14.0);
    let at = pointer + egui::vec2(16.0, 40.0);
    let icon_rect = egui::Rect::from_min_size(at, egui::vec2(side, side));
    let text_at = at + egui::vec2(side + 4.0, (side - galley.size().y) * 0.5);
    let chip = egui::Rect::from_min_max(at, at + egui::vec2(side + 4.0 + galley.size().x, side)).expand(3.0);
    painter.rect_filled(chip, tk.radius_sm, tk.panel);
    painter.rect_stroke(chip, tk.radius_sm, Stroke::new(1.0, tk.sketch_accent), egui::StrokeKind::Inside);
    crate::icons::paint_icon(painter, icon_rect, icon, 1.0);
    painter.galley(text_at, galley, tk.text);
    crate::scenario::publish_count("line_arc_mode_chip", if command == TANGENT_ARC { 1.0 } else { 2.0 });
}
