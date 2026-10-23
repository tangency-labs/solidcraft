//! Handles that place a feature on a face and turn it in the face.
//!
//! A dashed crosshair at the feature's place drags it across the model, near-white so the blue
//! preview it sits on does not hide it, cased in a darker edge so it also carries on a light body.
//! Under the pointer its lines grow, it shows the move cursor and a tooltip. While it drags, the
//! feature's footprint is outlined where the marker is: the solid preview is recomputed off the
//! window thread and trails the pointer by its boolean, the outline does not. A feature that can
//! turn in its face also gets a knob past its end; dragging it turns the feature about the face
//! normal. Nothing runs until OK: the handles edit the open dialog, which the live preview
//! follows, so a new feature and an Edit Feature behave alike.
//!
//! The mechanics live here; each tool that uses them is one file under `place_gizmo/` that gives a
//! [`Spec`]: where the place is, how a pointer re-places it, the footprint, and the turn if any.
//! [`spec`] picks the file by the dialog kind. The handles are published as `<name>:position` and
//! `<name>:rotate` for `ui.handles`.

mod boss;
mod look;
mod rest;
mod snap_fit;

use egui::{Pos2, vec2};
use look::{CrossStyle, HANDLE, HOT, angle_label, ball, cased_closed, cased_line, crosshair};
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::Dialog;
pub(crate) use crate::dialogs::vec3_of;
pub(crate) use crate::selection::point_value as pt;
use crate::theme::Tokens;
use crate::viewport::Proj;

/// One tool's use of the handles.
pub(crate) struct Spec {
    /// Handle names (`<name>:position`, `<name>:rotate`) and widget ids.
    pub name: &'static str,
    /// Is this dialog one of the tool's?
    pub serves: fn(&Dialog) -> bool,
    /// The tool's commands whose face place also snaps to sketch points on the face while the
    /// handles are on (the place input then takes points, as a Single hole's does).
    pub snap_commands: &'static [&'static str],
    /// The move marker's tooltip.
    pub move_tip: &'static str,
    /// The place and the direction the feature grows out of its face.
    pub frame: fn(&SolveApp, &Dialog) -> Option<(Vec3, Vec3)>,
    /// Re-place the feature where a click at the pointer would put it; false when nothing there
    /// can take it.
    pub move_to: fn(&SolveApp, &mut Dialog, &Proj, Pos2) -> bool,
    /// The footprint outlined while dragging, in world points (empty: no outline).
    pub footprint: fn(&SolveApp, &Dialog, Vec3, Vec3) -> Vec<Vec3>,
    pub turn: Option<Turn>,
}

/// A knob that turns the feature about its face normal.
pub(crate) struct Turn {
    pub tip: &'static str,
    /// How far the feature reaches from its place towards the knob (world): the knob sits a fixed
    /// screen gap past it. `None` hides the knob (a round Rest has nothing to turn).
    pub reach: fn(&SolveApp, &Dialog) -> Option<f64>,
    /// The current turn about `n`, from [`axes`]'s x towards its y.
    pub angle: fn(&SolveApp, &Dialog, Vec3) -> f64,
    /// Turn to the pointer's angle (`free` when Alt or Ctrl is held); returns the angle taken.
    pub set: fn(&SolveApp, &mut Dialog, Vec3, f64, bool) -> f64,
}

/// Every tool with handles, one file each under `place_gizmo/`, one per line in name order so
/// tools added on separate branches merge without touching each other's line.
#[rustfmt::skip]
static TOOLS: &[&Spec] = &[
    &boss::SPEC,
    &rest::SPEC,
    &snap_fit::SPEC,
];

thread_local! {
    /// Where the handles were drawn last frame (screen), for [`contains_pointer`]: navigation and
    /// Select Other's press-and-hold run before the handles are drawn.
    static DRAWN: std::cell::RefCell<Vec<Pos2>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Is `p` on a placement handle of the open dialog (as drawn last frame)?
pub(crate) fn contains_pointer(app: &SolveApp, p: Pos2) -> bool {
    app.dialog.as_ref().is_some_and(|d| spec(d).is_some()) && DRAWN.with(|h| h.borrow().iter().any(|c| c.distance(p) <= REACH))
}

/// The tool whose dialog this is, if it has handles.
fn spec(d: &Dialog) -> Option<&'static Spec> {
    TOOLS.iter().copied().find(|s| (s.serves)(d))
}

/// Does this command's face place also snap to sketch points on the face, so its marker can be
/// dragged onto one?
pub(crate) fn snaps_to_points(id: &str) -> bool {
    TOOLS.iter().any(|s| s.snap_commands.contains(&id))
}

/// Reach of a handle around its centre (pixels): wide, so a finger finds it.
const REACH: f32 = 18.0;
/// The knob sits this far past the feature's end (pixels).
const KNOB_GAP_PX: f64 = 28.0;
/// How far, and how many times at most, the knob slides out from under the value box (pixels).
const KNOB_SLIDE_PX: f64 = 8.0;
const KNOB_SLIDE_STEPS: usize = 40;
/// Points on a round footprint's outline.
const FOOTPRINT_SEGS: usize = 48;

/// A length expression's value (mm), zero when it does not evaluate.
pub(crate) fn len(app: &SolveApp, e: &str) -> f64 {
    app.session.doc.eval(e, ValueKind::Length).ok().filter(|v| v.is_finite()).unwrap_or(0.0).abs()
}

/// In-plane axes about `n`: x is the engine's default in-face direction (its `any_perp`), y
/// completes them, so angle 0 is an untouched feature.
pub(crate) fn axes(n: Vec3) -> (Vec3, Vec3) {
    let x = n.any_perp();
    (x, n.cross(x))
}

/// The direction in the face at `angle` about `n` from [`axes`]'s x.
pub(crate) fn dir_at(n: Vec3, angle: f64) -> Vec3 {
    let (x, y) = axes(n);
    x * angle.cos() + y * angle.sin()
}

/// The angle of `v` about `n` from [`axes`]'s x.
pub(crate) fn angle_of(v: Vec3, n: Vec3) -> f64 {
    let (x, y) = axes(n);
    v.dot(y).atan2(v.dot(x))
}

/// `deg` rounded to `step` degrees unless `free`.
pub(crate) fn stepped(deg: f64, step: f64, free: bool) -> f64 {
    if free { deg } else { (deg / step).round() * step }
}

/// A circle of radius `r` about `base` across `n`.
pub(crate) fn circle(base: Vec3, n: Vec3, r: f64) -> Vec<Vec3> {
    let (x, y) = axes(n);
    (0..FOOTPRINT_SEGS)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / FOOTPRINT_SEGS as f64;
            base + (x * a.cos() + y * a.sin()) * r
        })
        .collect()
}

/// A rectangle about `base` across `n`: `hl` either way along `a`, `hw` either way across it.
pub(crate) fn rect(base: Vec3, n: Vec3, a: Vec3, hl: f64, hw: f64) -> Vec<Vec3> {
    let b = n.cross(a);
    [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)].into_iter().map(|(sa, sb)| base + a * (sa * hl) + b * (sb * hw)).collect()
}

/// Where the pointer's ray meets the plane through `base` across `n`, none near edge-on, where a
/// plane intersection magnifies subpixel pointer error.
fn on_plane(proj: &Proj, pos: Pos2, base: Vec3, n: Vec3) -> Option<Vec3> {
    if proj.ray(pos).1.dot(n).abs() < 0.05 {
        return None;
    }
    crate::gizmo::on_plane(proj, pos, base, n).filter(|p| p.is_finite())
}

/// Draw the handles and take their drags (after the dialog's own arrow, so the marker wins
/// the pointer near the base).
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog) {
    DRAWN.with(|h| h.borrow_mut().clear());
    let Some(sp) = spec(d) else { return };
    let Some((base, _)) = (sp.frame)(app, d) else { return };
    let Some(bs) = proj.to_screen(base) else { return };
    let t = Tokens::get();
    let area = |c: Pos2| egui::Rect::from_center_size(c, vec2(2.0 * REACH, 2.0 * REACH));
    let id = |what: &str| egui::Id::new(("sc_place", sp.name, what));

    // The crosshair that moves the feature.
    let r = ui.interact(area(bs), id("position"), egui::Sense::drag()).on_hover_text(crate::i18n::tr_label(sp.move_tip));
    // Grab offset: the press lands where the finger is, not always the centre. Remember how far
    // the press was from the centre and keep that gap for the whole drag, so the feature follows
    // the pointer from where it was grabbed instead of jumping its centre under the first move.
    if r.drag_started()
        && let Some(p) = r.interact_pointer_pos()
    {
        ui.data_mut(|m| m.insert_temp(id("grab"), p - bs));
    }
    if r.dragged() {
        crate::tool_modifiers::set_manipulator_dragging(true);
        if let Some(p) = r.interact_pointer_pos() {
            let off = ui.data(|m| m.get_temp::<egui::Vec2>(id("grab"))).unwrap_or_default();
            (sp.move_to)(app, d, proj, p - off);
        }
    }
    // Read the frame again: a drag moved it this frame.
    let Some((base, n)) = (sp.frame)(app, d) else { return };
    let Some(bs) = proj.to_screen(base) else { return };
    crate::scenario::publish_handle(&format!("{}:position", sp.name), bs);
    DRAWN.with(|h| h.borrow_mut().push(bs));
    let (hovered, dragging) = (r.hovered(), r.dragged());
    // Ease the two state factors; animate_bool_with_time repaints only while they are moving.
    let style = CrossStyle {
        hot: ui.ctx().animate_bool_with_time(id("hot"), hovered || dragging, 0.12),
        active: ui.ctx().animate_bool_with_time(id("active"), dragging, 0.12),
    };
    if dragging {
        let pts: Option<Vec<Pos2>> = (sp.footprint)(app, d, base, n).into_iter().map(|p| proj.to_screen(p)).collect();
        if let Some(pts) = pts.filter(|p| p.len() >= 3) {
            cased_closed(painter, &pts, 1.5, HANDLE);
        }
    }
    crosshair(painter, bs, &style, &t);
    if hovered || dragging {
        ui.ctx().set_cursor_icon(if dragging { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Move });
    }

    // The knob that turns it.
    let Some(turn) = &sp.turn else { return };
    let Some(reach) = (turn.reach)(app, d) else { return };
    // World length of one pixel at this zoom (orthographic views; near enough in perspective).
    let per_px = 2.0 * app.cam.half_height() / f64::from(proj.rect.height().max(1.0));
    let mut angle = (turn.angle)(app, d, n);
    let (x, y) = axes(n);
    let at = |a: f64, radius: f64| proj.to_screen(base + (x * a.cos() + y * a.sin()) * radius);
    // The dialog's value box floats over the view beside the place (seen straight down, its arrow
    // collapses onto the place and the box sits just right of it) and takes the pointer first, so
    // a knob under it could not be grabbed. Slide the knob out along its ray until it is clear of
    // where the box was last frame.
    let value_box = ui.ctx().memory(|m| m.area_rect(egui::Id::new("sc_canvas_area")));
    let mut radius = reach + KNOB_GAP_PX * per_px;
    for _ in 0..KNOB_SLIDE_STEPS {
        match (value_box, at(angle, radius)) {
            (Some(b), Some(k)) if b.expand(REACH).contains(k) => radius += KNOB_SLIDE_PX * per_px,
            _ => break,
        }
    }
    let knob_at = |a: f64| at(a, radius);
    let Some(mut ks) = knob_at(angle) else { return };
    let r = ui.interact(area(ks), id("rotate"), egui::Sense::drag()).on_hover_text(crate::i18n::tr_label(turn.tip));
    if r.dragged()
        && let Some(p) = r.interact_pointer_pos()
    {
        crate::tool_modifiers::set_manipulator_dragging(true);
        let raw = match on_plane(proj, p, base, n) {
            Some(q) => Some((q - base).dot(y).atan2((q - base).dot(x))),
            None => crate::canvas::projected_rotation_angle(proj, p, base, x, y, radius, angle, (-std::f64::consts::PI, std::f64::consts::PI)),
        };
        if let Some(raw) = raw.filter(|a| a.is_finite()) {
            angle = (turn.set)(app, d, n, raw, crate::tool_modifiers::manipulator_free());
            d.error = None;
            if let Some(k) = knob_at(angle) {
                ks = k;
            }
        }
    }
    crate::scenario::publish_handle(&format!("{}:rotate", sp.name), ks);
    DRAWN.with(|h| h.borrow_mut().push(ks));
    let hot = r.hovered() || r.dragged();
    cased_line(painter, bs, ks, 1.5, if hot { HOT } else { HANDLE });
    ball(painter, ks, hot, &t);
    if hot {
        ui.ctx().set_cursor_icon(if r.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        angle_label(painter, ks, angle, &t);
    }
}
