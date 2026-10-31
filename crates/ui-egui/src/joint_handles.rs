//! Explicit assembly Move: only the selected part's allowed joint axes are shown. Distances
//! and angles are relative to the current drag, and each drag is one cancellable undo step.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use egui::{Pos2, Shape, Stroke, vec2};
use serde_json::json;
use solidcraft_engine::{
    Sel,
    doc::{Document, apply_point, joints, mat_inverse, mat_mul},
    geom::Vec3,
};

use crate::{
    SolveApp,
    theme::Tokens,
    ui_kit::{Container, Surface},
    viewport::Proj,
};

#[derive(Clone, Debug)]
pub(crate) struct Mode {
    occurrence: u64,
    axes: Vec<joints::MotionAxis>,
    drag: Option<Drag>,
    radii: BTreeMap<(u64, usize), f64>,
    local_anchor: Vec3,
    doc: Arc<Document>,
}

#[derive(Clone, Debug)]
struct Drag {
    axis: joints::MotionAxis,
    initial: f64,
    world: solidcraft_engine::doc::Mat,
    last_world: solidcraft_engine::doc::Mat,
    start: f64,
    last: f64,
    raw: f64,
    snapped: f64,
    anchor_raw: f64,
    anchor_value: f64,
    fine: bool,
    radius: f64,
    actual: f64,
    step: Option<Arc<Document>>,
    checkpoint: Option<u64>,
}

pub(crate) fn selected_occurrence(app: &SolveApp) -> Option<u64> {
    let mut selected = None;
    for item in &app.session.selection {
        let occurrence = match item {
            Sel::Occurrence { path } => *path.last()?,
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => {
                crate::dialogs_assembly::occurrence_of_body(&app.session, body)?
            }
            Sel::Body { name } => crate::dialogs_assembly::occurrence_of_body(&app.session, name)?,
            _ => return None,
        };
        if occurrence == 0 || selected.is_some_and(|s| s != occurrence) {
            return None;
        }
        selected = Some(occurrence);
    }
    selected
}

pub(crate) fn selected_path(app: &SolveApp) -> Option<Vec<u64>> {
    match app.session.selection.first()? {
        Sel::Occurrence { path } => Some(path.clone()),
        item => {
            let body = match item {
                Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => body,
                Sel::Body { name } => name,
                _ => return None,
            };
            app.session
                .doc
                .body_frames_with_source(&app.session.model.state())
                .into_iter()
                .find(|(world, ..)| world == body)
                .map(|(_, _, _, path, _)| path)
        }
    }
}

/// Both Assemble > Move and the context menu enter here; picking a face alone never does.
pub(crate) fn begin(app: &mut SolveApp, occurrence: u64) -> bool {
    close(app);
    if !app.preferences.assembly_move_handles || app.session.active_sketch.is_some() || app.applying.is_some() {
        return false;
    }
    let world = joints::occurrence_world(&app.session.doc, occurrence);
    let anchor = app
        .session
        .selection
        .iter()
        .find_map(|s| match s {
            Sel::Face { point, .. } | Sel::Edge { point, .. } | Sel::Vertex { point, .. } => Some(*point),
            _ => None,
        })
        .unwrap_or_else(|| apply_point(&world, Vec3::ZERO));
    let Some(inverse) = mat_inverse(&world) else { return false };
    let mut mode = Mode {
        occurrence,
        axes: Vec::new(),
        drag: None,
        radii: BTreeMap::new(),
        local_anchor: apply_point(&inverse, anchor),
        doc: app.session.doc.clone(),
    };
    mode.refresh(app);
    if mode.axes.is_empty() {
        return false;
    }
    crate::dialogs::cancel(app);
    app.tool = None;
    app.tree.occurrence_move = None;
    app.viewport.nav = None;
    app.viewport.primary_nav = None;
    app.home.open = false;
    app.viewport.joint_handles = Some(mode);
    true
}

impl Mode {
    fn refresh(&mut self, app: &SolveApp) {
        self.axes = joints::motion_axes(&app.session.doc, &app.session.model.state(), self.occurrence);
        let anchor = apply_point(&joints::occurrence_world(&app.session.doc, self.occurrence), self.local_anchor);
        // Keep the rotation ring on the selected face's depth, on the true joint axis.
        for a in &mut self.axes {
            a.origin += a.axis * (anchor - a.origin).dot(a.axis);
        }
        let names: BTreeSet<_> = app
            .session
            .doc
            .body_frames_with_source(&app.session.model.state())
            .into_iter()
            .filter(|(_, _, _, path, _)| path.last() == Some(&self.occurrence))
            .map(|(name, ..)| name)
            .collect();
        let world = app.session.world_state();
        self.radii = self
            .axes
            .iter()
            .map(|a| {
                let mut extent = 0.0_f64;
                for body in world.bodies.iter().filter(|b| names.contains(&b.name)) {
                    let mesh = body.mesh();
                    for p in mesh.positions.iter().step_by((mesh.positions.len() / 1024).max(1)) {
                        let radius = (*p - a.origin).cross(a.axis).len();
                        if radius.is_finite() {
                            extent = extent.max(radius);
                        }
                    }
                }
                ((a.joint, a.index), extent)
            })
            .collect();
        self.doc = app.session.doc.clone();
    }
}

/// A new selection or changed constraints must not leave handles for a stale part or pose.
pub(crate) fn sync(app: &mut SolveApp) {
    if !app.preferences.assembly_move_handles {
        close(app);
        return;
    }
    let Some(mut mode) = app.viewport.joint_handles.take() else { return };
    if mode.drag.is_none() {
        if selected_occurrence(app) != Some(mode.occurrence) {
            return;
        }
        if !Arc::ptr_eq(&mode.doc, &app.session.doc) {
            mode.refresh(app);
        }
        if mode.axes.is_empty() {
            return;
        }
    }
    app.viewport.joint_handles = Some(mode);
}

pub(crate) fn start(app: &mut SolveApp) {
    let Some(occurrence) = selected_occurrence(app) else {
        app.set_status(app.tr("joint_move.select").to_string(), false);
        return;
    };
    if begin(app, occurrence) {
        return;
    }
    let jointed = app.session.doc.assembly.joints.iter().any(|j| !j.suppressed && (j.a.occurrence == occurrence || j.b.occurrence == occurrence));
    if (!app.preferences.assembly_move_handles || !jointed)
        && let Some(path) = selected_path(app)
        && let Some(item) = crate::context_menu::items(app, &crate::context_menu::Target::Occurrence { path })
            .into_iter()
            .find(|i| i.id == "ui.moveOccurrence" && i.enabled)
    {
        crate::context_menu::run_item(app, &item, Pos2::ZERO);
    } else {
        let blocked = Blocked::of(app, occurrence);
        app.set_status(blocked.reason(app), false);
        app.menu.joint_move_warning = Some(blocked);
    }
}

/// Why Move found no axis to show for a part, kept for the warning until it is acknowledged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Blocked {
    pub(crate) part: String,
    /// Names of this part's joints the solver reports as conflicting (it can't close their
    /// loop). Empty when the part simply has no free joint value.
    pub(crate) conflicts: Vec<String>,
}

impl Blocked {
    /// A conflicting joint also leaves no axis, but "no movable joint axis" would send the user
    /// to the joint types and limits instead of to the joint that disagrees (#10331). Only
    /// joints on this part count: a conflict elsewhere in the design is not why it is blocked.
    fn of(app: &SolveApp, occurrence: u64) -> Self {
        let doc = &app.session.doc;
        let part = doc.occurrences.iter().find(|o| o.id == occurrence).map(|o| o.name.clone()).unwrap_or_default();
        let on_part: BTreeSet<u64> = doc
            .assembly
            .joints
            .iter()
            .filter(|j| !j.suppressed && (j.a.occurrence == occurrence || j.b.occurrence == occurrence))
            .map(|j| j.id)
            .collect();
        let mut conflicts = Vec::new();
        if !on_part.is_empty() {
            let solution = joints::solve_holding_with_analysis(doc, &app.session.model.state(), &joints::Held::new(), joints::Analysis::Placement);
            for (id, name, _) in solution.conflicts {
                if on_part.contains(&id) && !conflicts.contains(&name) {
                    conflicts.push(name);
                }
            }
        }
        Self { part, conflicts }
    }

    fn reason(&self, app: &SolveApp) -> String {
        if self.conflicts.is_empty() {
            app.tr("joint_move.fixed").to_string()
        } else {
            crate::i18n::format("joint_move.conflict", &[("joint", &self.conflicts.join(", "))])
        }
    }
}

/// A blocked Move is a persistent warning, acknowledged with OK, Enter, Escape or Close.
/// It never changes the joint type, grounding, limits or the selected geometry.
pub(crate) fn show_warning(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(blocked) = app.menu.joint_move_warning.clone() else { return };
    let reason = blocked.reason(app);
    let hint = if blocked.conflicts.is_empty() { "joint_move.blocked_hint" } else { "joint_move.conflict_hint" };
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.menu.joint_move_warning = None;
        return;
    }
    let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::new("sc_joint_move_blocked"), "status_warning", app.tr("joint_move.blocked_title"))
        .tone(crate::ui_kit::Tone::Warning)
        .actions(app.tr("ui.heading_ok"), "", None)
        .show(ctx, true, |ui| {
            if !blocked.part.is_empty() {
                crate::ui_kit::text(ui, &blocked.part, crate::ui_kit::Text::Report("part", false));
            }
            crate::ui_kit::text(ui, &reason, crate::ui_kit::Text::Report("reason", false));
            crate::ui_kit::text(ui, app.tr(hint), crate::ui_kit::Text::Hint("remedy"));
        });
    app.viewport.dialog_rect = crate::scenario::rect_of("dialog:window");
    if closed || action != crate::ui_kit::Action::None {
        app.menu.joint_move_warning = None;
    }
}

pub(crate) fn visible(app: &SolveApp) -> bool {
    app.viewport.joint_handles.is_some()
        && app.dialog.is_none()
        && app.tool.is_none()
        && app.session.active_sketch.is_none()
        && app.applying.is_none()
        && !app.home.open
}

pub(crate) fn active(app: &SolveApp) -> bool {
    app.viewport.joint_handles.as_ref().is_some_and(|m| m.drag.is_some())
}

pub(crate) fn close(app: &mut SolveApp) {
    end_drag(app);
    app.viewport.joint_handles = None;
    app.menu.joint_move_warning = None;
}

pub(crate) fn cancel(app: &mut SolveApp) -> bool {
    if app.menu.joint_move_warning.take().is_some() {
        return true;
    }
    let Some(mut mode) = app.viewport.joint_handles.take() else { return false };
    if let Some(d) = mode.drag.take() {
        if let Some(checkpoint) = d.checkpoint {
            let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint}));
        } else if d.step.is_some() {
            let _ = app.run("edit.undo", json!({}));
        }
        mode.refresh(app);
        app.viewport.joint_handles = Some(mode);
    }
    true
}

fn basis(axis: Vec3) -> Option<(Vec3, Vec3)> {
    let u = axis.any_perp().normalized()?;
    Some((u, axis.cross(u)))
}

fn mm_per_px(proj: &Proj, origin: Vec3) -> Option<f64> {
    let c = proj.to_screen(origin)?;
    let u = proj.ray(c).1.any_perp().normalized()?;
    let pixels = f64::from(c.distance(proj.to_screen(origin + u)?));
    (pixels.is_finite() && pixels > 1e-8).then_some(1.0 / pixels)
}

fn radius(app: &SolveApp, proj: &Proj, a: &joints::MotionAxis) -> Option<f64> {
    let scale = mm_per_px(proj, a.origin)?;
    let extent = app.viewport.joint_handles.as_ref()?.radii.get(&(a.joint, a.index)).copied().unwrap_or(0.0);
    // Keep the arcs outside the part even when zoomed in.
    Some((extent + scale * 18.0).max(scale * 72.0))
}

fn arc(proj: &Proj, a: &joints::MotionAxis, radius: f64, start: f64, sweep: f64) -> Vec<Pos2> {
    let Some((u, v)) = basis(a.axis) else { return Vec::new() };
    (0..=72)
        .filter_map(|i| {
            let th = (start + f64::from(i) * sweep / 72.0).to_radians();
            proj.to_screen(a.origin + (u * th.cos() + v * th.sin()) * radius)
        })
        .collect()
}

const SHAFT_WIDTH: f32 = 10.0;

/// The same geometry paints and picks the whole arrow, including its broad shoulders.
struct Arrow {
    shaft: Vec<Pos2>,
    head: [Pos2; 3],
}

impl Arrow {
    fn new(shaft: Vec<Pos2>) -> Option<Self> {
        let base = *shaft.last()?;
        let previous = *shaft.get(shaft.len().checked_sub(2)?)?;
        let length: f32 = shaft.windows(2).map(|w| w[0].distance(w[1])).sum();
        let tangent = base - previous;
        if !length.is_finite() || length < 6.0 || tangent.length() < 1e-3 {
            return None;
        }
        let direction = tangent.normalized();
        let normal = vec2(-direction.y, direction.x);
        // Use the complete shaft length, never a short tessellation segment of the arc.
        let head = 32.0_f32.min(length * 0.6);
        Some(Self { shaft, head: [base + direction * head, base + normal * head * 0.58, base - normal * head * 0.58] })
    }

    fn distance(&self, p: Pos2) -> f32 {
        let [a, b, c] = self.head;
        let cross = |u: egui::Vec2, v: egui::Vec2| u.x * v.y - u.y * v.x;
        let sides = [cross(b - a, p - a), cross(c - b, p - b), cross(a - c, p - c)];
        if sides.iter().all(|s| *s >= 0.0) || sides.iter().all(|s| *s <= 0.0) {
            return 0.0;
        }
        self.shaft
            .windows(2)
            .map(|w| segment_distance(p, w[0], w[1]))
            .chain([[a, b], [b, c], [c, a]].map(|w| segment_distance(p, w[0], w[1])))
            .fold(f32::INFINITY, f32::min)
    }

    fn paint(&self, painter: &egui::Painter, color: egui::Color32) {
        let outline = Tokens::view().icons.marker_halo;
        painter.add(Shape::line(self.shaft.clone(), Stroke::new(SHAFT_WIDTH + 4.0, outline)));
        painter.add(Shape::line(self.shaft.clone(), Stroke::new(SHAFT_WIDTH, color)));
        painter.add(Shape::convex_polygon(self.head.to_vec(), color, Stroke::NONE));
        let [tip, left, right] = self.head;
        painter.add(Shape::line(vec![left, tip, right], Stroke::new(2.0, outline)));
        // Outline the shoulders without drawing a seam across the head/shaft join.
        let base = left + (right - left) * 0.5;
        let normal = (left - base).normalized();
        painter.line_segment([left, base + normal * SHAFT_WIDTH * 0.5], Stroke::new(2.0, outline));
        painter.line_segment([right, base - normal * SHAFT_WIDTH * 0.5], Stroke::new(2.0, outline));
    }
}

fn arrows(proj: &Proj, a: &joints::MotionAxis, radius: f64) -> Vec<Arrow> {
    if a.angular {
        // Two separate tangent arrows, with opposing tips as in the approved design.
        [204.0, 384.0].into_iter().filter_map(|start| Arrow::new(arc(proj, a, radius, start, -136.0))).collect()
    } else {
        let Some(center) = proj.to_screen(a.origin) else { return Vec::new() };
        [-1.0, 1.0].into_iter().filter_map(|sign| Arrow::new(vec![center, proj.to_screen(a.origin + a.axis * radius * 1.3 * sign)?])).collect()
    }
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + d * t)
}

fn hit(app: &SolveApp, proj: &Proj, p: Pos2) -> Option<joints::MotionAxis> {
    if !visible(app) {
        return None;
    }
    app.viewport
        .joint_handles
        .as_ref()?
        .axes
        .iter()
        .rev()
        .filter_map(|a| {
            let r = radius(app, proj, a)?;
            let distance = arrows(proj, a, r).iter().map(|arrow| arrow.distance(p)).fold(f32::INFINITY, f32::min);
            (distance < 12.0).then_some((distance, a))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, a)| a.clone())
}

pub(crate) fn owns_pointer(app: &SolveApp, proj: &Proj, p: Pos2) -> bool {
    active(app) || hit(app, proj, p).is_some()
}

fn along(proj: &Proj, p: Pos2, a: &joints::MotionAxis) -> Option<f64> {
    let (o, d) = proj.ray(p);
    let w = a.origin - o;
    let b = a.axis.dot(d);
    let den = 1.0 - b * b;
    (den.abs() > 1e-6).then_some((b * d.dot(w) - a.axis.dot(w)) / den)
}

fn polar(proj: &Proj, p: Pos2, a: &joints::MotionAxis) -> Option<(f64, f64)> {
    let (o, d) = proj.ray(p);
    let den = a.axis.dot(d);
    if den.abs() < 1e-5 {
        return None;
    }
    let q = o + d * ((a.origin - o).dot(a.axis) / den) - a.origin;
    let (u, v) = basis(a.axis)?;
    // Ignore the singular centre rather than making an arbitrary half turn through it.
    if q.len() < mm_per_px(proj, a.origin)? * 6.0 {
        return None;
    }
    Some((q.dot(v).atan2(q.dot(u)), q.len()))
}

pub(crate) fn start_drag(app: &mut SolveApp, proj: &Proj, p: Pos2) -> bool {
    let Some(axis) = hit(app, proj, p) else { return false };
    let initial = joints::joint_values(&app.session.doc.assembly).get(&axis.joint).and_then(|q| q.get(axis.index)).copied();
    let Some(initial) = initial else { return false };
    let start = if axis.angular { polar(proj, p, &axis).map(|q| q.0) } else { along(proj, p, &axis) };
    let (Some(start), Some(radius)) = (start, radius(app, proj, &axis)) else { return false };
    let checkpoint = app.session.execute("edit.checkpoint", &json!({})).ok().and_then(|v| v["checkpoint"].as_u64());
    let Some(mode) = app.viewport.joint_handles.as_mut() else { return false };
    let world = joints::occurrence_world(&app.session.doc, mode.occurrence);
    mode.drag = Some(Drag {
        axis,
        initial,
        world,
        last_world: world,
        start,
        last: start,
        raw: 0.0,
        snapped: 0.0,
        anchor_raw: 0.0,
        anchor_value: 0.0,
        fine: false,
        radius,
        actual: 0.0,
        step: None,
        checkpoint,
    });
    true
}

fn wrapped(angle: f64) -> f64 {
    (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

impl Drag {
    fn rotate(&mut self, angle: f64, distance: f64, coarse: f64, fine: f64) -> f64 {
        self.raw += wrapped(angle - self.last);
        self.last = angle;
        // Hysteresis prevents flicker at the 2-radius boundary. Rebase at a switch, so
        // moving radially changes precision without moving the part or losing its pose.
        let next = if self.fine { distance > self.radius * 1.85 } else { distance > self.radius * 2.15 };
        if next != self.fine {
            self.fine = next;
            self.anchor_raw = self.raw;
            self.anchor_value = self.actual;
            self.snapped = self.actual;
        }
        let step = if self.fine { fine } else { coarse }.to_radians();
        self.snapped = self.anchor_value + ((self.raw - self.anchor_raw) / step).round() * step;
        self.snapped
    }
}

pub(crate) fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(mut mode) = app.viewport.joint_handles.take() else { return };
    let Some(mut d) = mode.drag.take() else {
        app.viewport.joint_handles = Some(mode);
        return;
    };
    if d.step.as_ref().is_some_and(|doc| !app.session.undo.last().is_some_and(|s| Arc::ptr_eq(&s.doc, doc))) {
        mode.drag = Some(d);
        app.viewport.joint_handles = Some(mode);
        end_drag(app);
        return;
    }
    let amount = if d.axis.angular {
        polar(proj, p, &d.axis).map(|(a, r)| d.rotate(a, r, app.preferences.joint_rotation_coarse, app.preferences.joint_rotation_fine))
    } else {
        along(proj, p, &d.axis).map(|v| v - d.start)
    };
    if let Some(amount) = amount.filter(|v| v.is_finite() && v.abs() < 1e8) {
        let current = joints::joint_values(&app.session.doc.assembly);
        if let (Some(joint), Some(values)) =
            (app.session.doc.assembly.joints.iter().find(|j| j.id == d.axis.joint), current.get(&d.axis.joint).cloned())
        {
            let mut wanted = d.initial + amount / d.axis.scale;
            if let Some((lo, hi)) = joint.limits.get(d.axis.index).copied().flatten() {
                wanted = wanted.clamp(lo.min(hi), lo.max(hi));
            }
            let changed = values.get(d.axis.index).is_some_and(|v| (v - wanted).abs() > 1e-10);
            if changed {
                let value = if joint.kind.is_angle(d.axis.index) { wanted.to_degrees() } else { wanted };
                let own = if d.step.is_some() { app.session.undo.pop() } else { None };
                let before = app.session.doc.clone();
                let reply = app.run("joint.drive", json!({"joint":d.axis.joint,"index":d.axis.index,"value":value,"quiet":true,"continuous":true}));
                let pushed = reply.is_ok() && !Arc::ptr_eq(&before, &app.session.doc);
                if let Some(step) = own {
                    if pushed {
                        app.session.undo.pop();
                    }
                    app.session.undo.push(step);
                } else if pushed {
                    d.step = app.session.undo.last().map(|s| s.doc.clone());
                }
                if let Ok(reply) = reply
                    && let Some(why) = crate::dialogs_assembly::drive_stop(&reply)
                {
                    app.set_status(why, false);
                }
            }
        }
        let now = joints::occurrence_world(&app.session.doc, mode.occurrence);
        if d.axis.angular {
            if let Some(inv) = mat_inverse(&d.last_world) {
                let delta = mat_mul(&now, &inv);
                let (u, v) = basis(d.axis.axis).unwrap_or((Vec3::X, Vec3::Y));
                let turn = solidcraft_engine::doc::apply_vector(&delta, u);
                d.actual += turn.dot(v).atan2(turn.dot(u));
            }
        } else {
            d.actual = (apply_point(&now, Vec3::ZERO) - apply_point(&d.world, Vec3::ZERO)).dot(d.axis.axis);
        }
        d.last_world = now;
    }
    mode.drag = Some(d);
    app.viewport.joint_handles = Some(mode);
}

pub(crate) fn end_drag(app: &mut SolveApp) {
    let Some(mut mode) = app.viewport.joint_handles.take() else { return };
    if let Some(d) = mode.drag.take() {
        if let Some(checkpoint) = d.checkpoint {
            let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint,"forget":true}));
        }
        mode.refresh(app);
    }
    app.viewport.joint_handles = Some(mode);
}

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    if !visible(app) {
        return;
    }
    let Some(mode) = app.viewport.joint_handles.clone() else { return };
    let tk = Tokens::get();
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hover = pointer.filter(|p| ui.rect_contains_pointer(proj.rect) && !crate::shell::covered(app, *p)).and_then(|p| hit(app, proj, p));
    let axes = if let Some(d) = &mode.drag {
        let mut axis = d.axis.clone();
        if let Some(inverse) = mat_inverse(&d.world) {
            let delta = mat_mul(&joints::occurrence_world(&app.session.doc, mode.occurrence), &inverse);
            axis.origin = apply_point(&delta, axis.origin);
        }
        vec![axis]
    } else {
        mode.axes.clone()
    };
    for a in &axes {
        let Some(r) = radius(app, proj, a) else { continue };
        let hot = mode.drag.as_ref().is_some_and(|d| d.axis.joint == a.joint && d.axis.index == a.index)
            || hover.as_ref().is_some_and(|d| d.joint == a.joint && d.index == a.index);
        let color = if hot { tk.joint_manipulator.gamma_multiply(1.2) } else { tk.joint_manipulator };
        if let (Some(start), Some(end)) = (proj.to_screen(a.origin - a.axis * r * 2.1), proj.to_screen(a.origin + a.axis * r * 2.1)) {
            painter.add(Shape::dashed_line(&[start, end], Stroke::new(2.0, tk.joint_axis), 10.0, 7.0));
        }
        let arrows = arrows(proj, a, r);
        for arrow in &arrows {
            arrow.paint(painter, color);
        }
        if let Some(arrow) = arrows.last()
            && let Some(p) = arrow.shaft.get(arrow.shaft.len() / 2)
        {
            crate::scenario::publish_handle(&format!("joint-move:{}:{}", a.joint, a.index), *p);
        }
        if let Some(center) = proj.to_screen(a.origin) {
            painter.circle_filled(center, 5.5, tk.joint_axis);
        }
    }
    if active(app) || hover.is_some() {
        ui.ctx().set_cursor_icon(if active(app) { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    if let Some(d) = &mode.drag {
        crate::tool_modifiers::set_manipulator_dragging(true);
        if d.axis.angular {
            let guide = arc(proj, &d.axis, d.radius * 2.0, 0.0, 360.0);
            painter.add(Shape::dashed_line(&guide, Stroke::new(1.2, tk.joint_manipulator), 5.0, 4.0));
            if let (Some(c), Some(p)) = (proj.to_screen(d.axis.origin), pointer) {
                painter.add(Shape::dashed_line(&[c, p], Stroke::new(1.0, tk.joint_manipulator), 4.0, 4.0));
            }
        } else if let (Some(a), Some(b)) = (proj.to_screen(d.axis.origin), proj.to_screen(d.axis.origin + d.axis.axis * d.actual)) {
            painter.add(Shape::dashed_line(&[a, b], Stroke::new(2.0, tk.joint_manipulator), 5.0, 4.0));
            painter.circle_stroke(a, 5.0, Stroke::new(1.5, tk.joint_manipulator));
        }
        if let Some(p) = pointer {
            let at = (p + vec2(20.0, 22.0)).clamp(proj.rect.min, proj.rect.max - vec2(260.0, 82.0).min(proj.rect.size()));
            let host = Container::new(ui.ctx(), Surface::Canvas);
            host.area(egui::Id::new("joint_move_readout")).interactable(false).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                let frame = host.frame(ui.ctx(), tk.overlay, crate::frame::Side::Right).inner_margin(tk.space_8);
                Container::show_backing(ui, frame, |ui| {
                    let value = if d.axis.angular {
                        format!(
                            "{}{:.precision$}°",
                            if d.actual >= 0.0 { "+" } else { "" },
                            d.actual.to_degrees(),
                            precision = usize::from(app.preferences.angle_decimals)
                                .max(
                                    (if d.fine { app.preferences.joint_rotation_fine } else { app.preferences.joint_rotation_coarse })
                                        .to_string()
                                        .split_once('.')
                                        .map_or(0, |(_, fraction)| fraction.len())
                                )
                                .clamp(1, 6)
                        )
                    } else {
                        format!("{}{}", if d.actual >= 0.0 { "+" } else { "" }, crate::prefs::show_mm(app, d.actual, 1))
                    };
                    ui.set_min_width(200.0);
                    ui.set_max_width(240.0);
                    ui.label(app.tr(if d.axis.angular { "joint_move.rotation" } else { "joint_move.distance" }));
                    ui.label(egui::RichText::new(value).strong());
                    if d.axis.angular {
                        let step = if d.fine { app.preferences.joint_rotation_fine } else { app.preferences.joint_rotation_coarse };
                        ui.label(format!("{} {step}°", app.tr("joint_move.step")));
                    }
                });
            });
        }
    }
    let host = Container::new(ui.ctx(), Surface::Canvas);
    let mut done = false;
    host.area(egui::Id::new("joint_move_help")).fixed_pos(proj.rect.left_bottom() + vec2(16.0, -148.0)).show(ui.ctx(), |ui| {
        let frame = host.frame(ui.ctx(), tk.overlay, crate::frame::Side::Right).inner_margin(tk.space_8);
        Container::show_backing(ui, frame, |ui| {
            ui.label(app.tr("joint_move.help"));
            if mode.axes.iter().any(|a| a.angular) {
                ui.label(app.tr("joint_move.precision"));
            }
            done = crate::ui_kit::button(ui, app.tr("joint_move.done"), crate::ui_kit::Button::Normal).clicked();
        });
    });
    if done {
        close(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scenario::Harness, test_support::play};

    fn assembly(kind: &str, axis: Vec3, reverse: bool) -> (Harness, u64) {
        let mut h = Harness::new();
        h.app.home.open = false;
        for (name, corner, size) in [("Base", [-5, -5, 0], [10, 10, 5]), ("Arm", [-3, -3, 5], [63, 6, 5])] {
            h.app.run("component.activate", json!({"component":"root"})).unwrap();
            h.app.run("component.create", json!({"name":name,"grounded":false})).unwrap();
            h.app.run("solid.box", json!({"length":size[0],"width":size[1],"height":size[2],"corner":corner})).unwrap();
        }
        h.app.run("component.activate", json!({"component":"root"})).unwrap();
        h.app.run("occurrence.ground", json!({"occurrence":"Base:1","grounded":true})).unwrap();
        let origin = |name: &str| json!({"occurrence":name,"point":[0,0,5],"z":[axis.x,axis.y,axis.z]});
        let (a, b) = if reverse { (origin("Arm"), origin("Base")) } else { (origin("Base"), origin("Arm")) };
        let joint =
            h.app.run("joint.create", json!({"type":kind,"a":a,"b":b,"flip":true,"angle":0,"name":"Guide"})).unwrap()["joint"].as_u64().unwrap();
        let component = h.app.session.doc.find_component("Arm").unwrap();
        let world = h.app.session.world_state();
        let body = world.bodies.iter().find(|b| h.app.session.doc.model_body_component(b) == component).unwrap().name.clone();
        h.app.session.selection = vec![Sel::Face { body, index: 0, point: Vec3::new(30., 0., 10.) }];
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 200.0;
        h.frames(4);
        (h, joint)
    }

    fn projection(h: &Harness) -> Proj {
        crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap())
    }
    fn pointer(h: &mut Harness, p: Pos2, pressed: bool) {
        h.app.synthetic.extend(crate::test_support::pointer(p, pressed));
        play(h);
    }
    fn move_to(h: &mut Harness, p: Pos2) {
        h.app.synthetic.push(egui::Event::PointerMoved(p));
        play(h);
    }
    fn angle_point(h: &Harness, axis: &joints::MotionAxis, rad: f64, deg: f64) -> Pos2 {
        let (u, v) = basis(axis.axis).unwrap();
        projection(h).to_screen(axis.origin + (u * deg.to_radians().cos() + v * deg.to_radians().sin()) * rad).unwrap()
    }
    fn drag_data(h: &Harness) -> &Drag {
        h.app.viewport.joint_handles.as_ref().unwrap().drag.as_ref().unwrap()
    }

    #[test]
    fn move_is_explicit_and_both_entry_points_hide_when_measuring() {
        let (mut h, _) = assembly("revolute", Vec3::Z, false);
        assert!(!visible(&h.app));
        assert!(
            crate::workspace::layout("ASSEMBLE", "POSITION", &solidcraft_engine::command_specs())
                .unwrap()
                .0
                .iter()
                .any(|c| c.id == "occurrence.move")
        );
        let selection = h.app.session.selection.clone();
        h.step(&json!({"click":{"handle":"tab:ASSEMBLE"}})).unwrap();
        h.step(&json!({"click":{"handle":"button:occurrence.move"}})).unwrap();
        h.frames(3);
        assert!(visible(&h.app));
        assert_eq!(h.app.viewport.joint_handles.as_ref().unwrap().axes.len(), 1);
        h.app.start("inspect.measure");
        h.frames(3);
        assert!(!visible(&h.app));
        assert!(h.app.viewport.joint_handles.is_none());
        crate::dialogs::cancel(&mut h.app);
        h.app.session.selection = selection;
        h.frames(2);
        let item =
            crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport).into_iter().find(|i| i.id == "occurrence.move").unwrap();
        assert_eq!(item.label, "Move");
        crate::context_menu::run_item(&mut h.app, &item, Pos2::ZERO);
        h.frames(3);
        assert!(visible(&h.app));
        h.app.start("inspect.measure");
        h.frames(2);
        assert!(h.app.viewport.joint_handles.is_none());
    }

    #[test]
    fn slider_readout_is_actual_distance_with_limits_one_undo_and_redo() {
        let (mut h, joint) = assembly("slider", Vec3::X, false);
        h.app.run("joint.limits", json!({"joint":joint,"min":-5,"max":12})).unwrap();
        h.app.start("occurrence.move");
        h.frames(3);
        let axes = &h.app.viewport.joint_handles.as_ref().unwrap().axes;
        assert_eq!(axes.len(), 1);
        let a = axes[0].clone();
        assert!(!a.angular && a.axis.dot(Vec3::X) > 0.999);
        let proj = projection(&h);
        let r = radius(&h.app, &proj, &a).unwrap();
        let start = a.origin + a.axis * r * 0.7;
        let from = proj.to_screen(start).unwrap();
        let to = proj.to_screen(start + a.axis * 30.).unwrap();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let camera = h.app.cam;
        pointer(&mut h, from, true);
        for k in 1..=12 {
            move_to(&mut h, from + (to - from) * (k as f32 / 12.));
        }
        assert!(active(&h.app));
        assert!((drag_data(&h).actual - 12.).abs() < 1e-5, "readout must stop at the real limit: {:?}", drag_data(&h));
        assert_eq!(h.app.cam, camera, "a handle must own the pointer ahead of navigation");
        assert!(h.app.viewport.boxsel.is_none());
        pointer(&mut h, to, false);
        assert!(!active(&h.app));
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(!h.app.session.log.iter().any(|s| s.contains("Drive Joints")), "no echo for every frame");
        let moved = h.app.session.doc.to_json();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), moved);
    }

    #[test]
    fn radial_rotation_switches_precision_without_jumps_and_escape_restores() {
        for (coarse, fine) in [(10., 0.1), (15., 0.25)] {
            let (mut h, _) = assembly("revolute", Vec3::Z, false);
            crate::prefs::set(&mut h.app, &json!({"joint_rotation_coarse":coarse,"joint_rotation_fine":fine})).unwrap();
            h.app.start("occurrence.move");
            h.frames(3);
            let a = h.app.viewport.joint_handles.as_ref().unwrap().axes[0].clone();
            let r = radius(&h.app, &projection(&h), &a).unwrap();
            let from = angle_point(&h, &a, r, 180.);
            let near = angle_point(&h, &a, r, 180. + coarse * 0.7);
            let before = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            pointer(&mut h, from, true);
            move_to(&mut h, near);
            assert!(active(&h.app), "the visible ring starts a drag");
            assert!((drag_data(&h).actual.to_degrees() - coarse).abs() < 1e-4, "{:?}", drag_data(&h));
            let far = angle_point(&h, &a, r * 2.8, 180. + coarse * 0.7);
            move_to(&mut h, far);
            assert!(drag_data(&h).fine);
            assert!((drag_data(&h).actual.to_degrees() - coarse).abs() < 1e-4, "radial motion must not rotate");
            let fine_point = angle_point(&h, &a, r * 2.8, 180. + coarse * 0.7 + fine * 2.2);
            move_to(&mut h, fine_point);
            assert!((drag_data(&h).actual.to_degrees() - (coarse + fine * 2.)).abs() < 1e-4, "{:?}", drag_data(&h));
            // Hysteresis: just inside the nominal guide still keeps fine adjustment.
            let boundary = angle_point(&h, &a, r * 1.95, 180. + coarse * 0.7 + fine * 2.2);
            move_to(&mut h, boundary);
            assert!(drag_data(&h).fine);
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(!active(&h.app));
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!(h.app.session.undo.len(), undo);
            pointer(&mut h, boundary, false);
            h.step(&json!({"key":"Escape"})).unwrap();
            assert!(h.app.viewport.joint_handles.is_none());
        }
    }

    #[test]
    fn fine_rotation_resumes_at_the_actual_limit_instead_of_the_unreached_target() {
        let (mut h, joint) = assembly("revolute", Vec3::Z, false);
        h.app.run("joint.limits", json!({"joint":joint,"min":-5,"max":12})).unwrap();
        h.app.start("occurrence.move");
        h.frames(3);
        let a = h.app.viewport.joint_handles.as_ref().unwrap().axes[0].clone();
        let radius = radius(&h.app, &projection(&h), &a).unwrap();
        let from = angle_point(&h, &a, radius, 180.);
        let near = angle_point(&h, &a, radius, 207.);
        pointer(&mut h, from, true);
        move_to(&mut h, near);
        assert!((drag_data(&h).actual.to_degrees() - 12.).abs() < 1e-4);
        let far = angle_point(&h, &a, radius * 2.8, 207.);
        move_to(&mut h, far);
        assert!((drag_data(&h).actual.to_degrees() - 12.).abs() < 1e-4);
        let back = angle_point(&h, &a, radius * 2.8, 206.8);
        move_to(&mut h, back);
        assert!((drag_data(&h).actual.to_degrees() - 11.8).abs() < 1e-4, "{:?}", drag_data(&h));
        pointer(&mut h, back, false);
    }

    #[test]
    fn axes_follow_reversed_and_oblique_joints_and_reject_locked_parts() {
        for reverse in [false, true] {
            let expected = Vec3::new(1., 2., 3.).normalized().unwrap();
            let (mut h, joint) = assembly("cylindrical", expected, reverse);
            let occurrence = selected_occurrence(&h.app).unwrap();
            let axes = joints::motion_axes(&h.app.session.doc, &h.app.session.model.state(), occurrence);
            assert_eq!(axes.len(), 2, "{axes:?}");
            assert!(axes.iter().all(|a| a.axis.dot(expected).abs() > 0.999));
            assert_eq!(axes.iter().filter(|a| a.angular).count(), 1);
            h.app.run("joint.limits", json!({"joint":joint,"index":0,"min":0,"max":0})).unwrap();
            h.app.run("joint.limits", json!({"joint":joint,"index":1,"min":0,"max":0})).unwrap();
            assert!(joints::motion_axes(&h.app.session.doc, &h.app.session.model.state(), occurrence).is_empty());
        }
        let (h, _) = assembly("rigid", Vec3::Z, false);
        assert!(joints::motion_axes(&h.app.session.doc, &h.app.session.model.state(), selected_occurrence(&h.app).unwrap()).is_empty());
    }

    #[test]
    fn empty_space_handles_own_the_drag_in_every_navigation_preset() {
        for preset in crate::prefs::NAV_PRESETS {
            let (mut h, _) = assembly("slider", Vec3::X, false);
            h.app.preferences.nav = preset.id.into();
            // Anchor at the far face so the outward arrow lies beyond the long arm.
            if let Some(Sel::Face { point, .. }) = h.app.session.selection.first_mut() {
                *point = Vec3::new(60., 0., 10.);
            }
            h.app.start("occurrence.move");
            h.frames(3);
            let a = h.app.viewport.joint_handles.as_ref().unwrap().axes[0].clone();
            let proj = projection(&h);
            let radius = radius(&h.app, &proj, &a).unwrap();
            let grab = a.origin + a.axis * radius * 1.1;
            let from = proj.to_screen(grab).unwrap();
            let to = proj.to_screen(grab + a.axis * 8.).unwrap();
            assert!(crate::viewport::pick(&h.app, &proj, from).is_empty(), "handle must be beyond the model");
            let camera = h.app.cam;
            pointer(&mut h, from, true);
            move_to(&mut h, to);
            assert!(active(&h.app), "{}: handle owns the drag", preset.id);
            assert_eq!(h.app.cam, camera, "{} must not navigate on a handle", preset.id);
            assert!((drag_data(&h).actual - 8.).abs() < 1e-4, "{}: {:?}", preset.id, drag_data(&h));
            pointer(&mut h, to, false);
        }
    }

    #[test]
    fn cylindrical_handles_pick_the_nearest_stroke_where_their_hit_areas_overlap() {
        let (mut h, _) = assembly("cylindrical", Vec3::new(1., 2., 1.).normalized().unwrap(), false);
        h.app.start("occurrence.move");
        h.frames(3);
        let mode = h.app.viewport.joint_handles.as_ref().unwrap();
        let axis = mode.axes.iter().find(|a| !a.angular).unwrap();
        let angular = mode.axes.iter().find(|a| a.angular).unwrap();
        let proj = projection(&h);
        let r = radius(&h.app, &proj, axis).unwrap();
        let ring = arrows(&proj, angular, radius(&h.app, &proj, angular).unwrap());
        let mut overlaps = 0;
        for k in -130..=130 {
            let p = proj.to_screen(axis.origin + axis.axis * r * f64::from(k) / 100.).unwrap();
            let distance = ring.iter().map(|arrow| arrow.distance(p)).fold(f32::INFINITY, f32::min);
            if distance > 1.0 && distance < 11.0 {
                assert!(!hit(&h.app, &proj, p).unwrap().angular, "a line closer than the ring must win");
                overlaps += 1;
            }
        }
        assert!(overlaps > 0, "exercise overlapping handle hit areas");
    }

    #[test]
    fn broad_arrowheads_are_pickable_and_rotation_stays_outside_a_zoomed_part() {
        for kind in ["revolute", "slider"] {
            let (mut h, _) = assembly(kind, Vec3::Z, false);
            if kind == "slider" {
                h.call("ui.view", json!({"view":"iso"}));
            }
            h.app.cam.distance = 90.0;
            h.app.start("occurrence.move");
            h.frames(3);
            let axis = h.app.viewport.joint_handles.as_ref().unwrap().axes[0].clone();
            let proj = projection(&h);
            let radius = radius(&h.app, &proj, &axis).unwrap();
            let extent = h.app.viewport.joint_handles.as_ref().unwrap().radii[&(axis.joint, axis.index)];
            assert!(radius > extent, "rotation must not shrink inside the selected part on zoom");
            let arrows = arrows(&proj, &axis, radius);
            assert_eq!(arrows.len(), 2);
            for arrow in arrows {
                let [tip, left, right] = arrow.head;
                assert!(left.distance(right) >= 30.0, "heads must have broad visible shoulders");
                let shoulder = tip + (left - tip) * 0.82 + (right - tip) * 0.08;
                assert!(hit(&h.app, &proj, shoulder).is_some(), "the full visible shoulder must be draggable");
                assert!(start_drag(&mut h.app, &proj, shoulder));
                end_drag(&mut h.app);
            }
        }
    }

    #[test]
    fn disabling_handles_restores_the_previous_move_panel_from_both_entry_points() {
        let (mut h, _) = assembly("revolute", Vec3::Z, false);
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.app.start("occurrence.move");
        h.frames(2);
        assert!(visible(&h.app));
        crate::prefs::set(&mut h.app, &json!({"assembly_move_handles":false})).unwrap();
        h.frames(2);
        assert!(h.app.viewport.joint_handles.is_none());
        for context in [false, true] {
            h.app.tree.occurrence_move = None;
            if context {
                let path = selected_path(&h.app).unwrap();
                let item = crate::context_menu::items(&h.app, &crate::context_menu::Target::Occurrence { path })
                    .into_iter()
                    .find(|item| item.id == "ui.moveOccurrence" && item.enabled)
                    .unwrap();
                crate::context_menu::run_item(&mut h.app, &item, Pos2::ZERO);
            } else {
                h.app.start("occurrence.move");
            }
            h.frames(2);
            assert!(h.app.viewport.joint_handles.is_none());
            assert!(h.app.tree.occurrence_move.is_some());
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!(h.app.session.undo.len(), undo);
        }
        h.app.tree.occurrence_move = None;
        crate::prefs::set(&mut h.app, &json!({"assembly_move_handles":true})).unwrap();
        h.app.start("occurrence.move");
        h.frames(2);
        assert!(visible(&h.app));
    }

    #[test]
    fn precision_settings_persist_validate_and_read_old_preferences() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        for key in ["joint_rotation_coarse", "joint_rotation_fine"] {
            for bad in [json!(0), json!(-1), json!(181), json!("broken"), json!(null)] {
                assert!(crate::prefs::set(&mut app, &json!({key:bad})).is_err());
            }
        }
        crate::prefs::set(&mut app, &json!({"joint_rotation_coarse":12.5,"joint_rotation_fine":0.25})).unwrap();
        crate::prefs::set(&mut app, &json!({"assembly_move_handles":false})).unwrap();
        let stored = serde_json::to_value(&app.preferences).unwrap();
        let read = crate::prefs::lenient(&stored);
        assert_eq!((read.joint_rotation_coarse, read.joint_rotation_fine), (12.5, 0.25));
        assert!(!read.assembly_move_handles);
        let old = crate::prefs::lenient(&json!({}));
        assert_eq!((old.joint_rotation_coarse, old.joint_rotation_fine), (10., 0.1));
        assert!(old.assembly_move_handles);
        let invalid = crate::prefs::lenient(&json!({"joint_rotation_coarse":0,"joint_rotation_fine":-3}));
        assert_eq!((invalid.joint_rotation_coarse, invalid.joint_rotation_fine), (10., 0.1));
    }
    /// Two boards held by two planar joints whose planes cross (#10331): the second joint can't
    /// close its loop, so Move names it instead of saying the part has no movable axis.
    #[test]
    fn blocked_move_names_a_conflicting_joint_of_the_part() {
        let mut h = Harness::new();
        h.app.home.open = false;
        for (name, y) in [("Yellow", 0), ("Red", 300)] {
            h.app.run("component.activate", json!({"component":"root"})).unwrap();
            h.app.run("component.create", json!({"name":name,"grounded":false})).unwrap();
            h.app.run("solid.box", json!({"length":400,"width":100,"height":20,"corner":[0,y,0]})).unwrap();
        }
        h.app.run("component.activate", json!({"component":"root"})).unwrap();
        h.app.run("occurrence.ground", json!({"occurrence":"Yellow:1","grounded":true})).unwrap();
        let planar = |name: &str, a: [i32; 3], b: [i32; 3]| json!({"type":"planar","a":{"occurrence":"Yellow","face":a},"b":{"occurrence":"Red","face":b},"name":name});
        h.app.run("joint.create", planar("Top", [200, 50, 20], [200, 350, 0])).unwrap();
        h.app.run("joint.create", planar("Front", [200, 0, 10], [200, 0, 30])).unwrap();
        let solved = joints::solve(&h.app.session.doc, &h.app.session.model.state());
        let conflicting: Vec<_> = solved.conflicts.iter().map(|(_, name, _)| name.as_str()).collect();
        assert!(!conflicting.is_empty(), "the scenario must conflict: {:?}", solved.conflicts);
        let component = h.app.session.doc.find_component("Red").unwrap();
        let world = h.app.session.world_state();
        let body = world.bodies.iter().find(|b| h.app.session.doc.model_body_component(b) == component).unwrap().name.clone();
        h.app.session.selection = vec![Sel::Body { name: body }];
        h.frames(3);
        h.step(&json!({"click":{"handle":"tab:ASSEMBLE"}})).unwrap();
        h.step(&json!({"click":{"handle":"button:occurrence.move"}})).unwrap();
        h.frames(3);
        let blocked = h.app.menu.joint_move_warning.clone().expect("Move must be blocked");
        assert_eq!(blocked.part, "Red:1");
        assert_eq!(blocked.conflicts, conflicting);
        let reason = blocked.reason(&h.app);
        assert!(reason.contains("conflicts") && conflicting.iter().all(|n| reason.contains(n)), "{reason}");
        assert_ne!(reason, h.app.tr("joint_move.fixed"));
        assert!(crate::scenario::rect_of("dialog:window").is_some());
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(2);
        assert!(h.app.menu.joint_move_warning.is_none());
        // Without the conflicting joint the part moves again.
        for name in conflicting {
            let id = h.app.session.doc.assembly.joints.iter().find(|j| j.name == name).unwrap().id;
            h.app.run("joint.delete", json!({"joint":id})).unwrap();
        }
        h.frames(2);
        h.step(&json!({"click":{"handle":"button:occurrence.move"}})).unwrap();
        h.frames(3);
        assert!(h.app.menu.joint_move_warning.is_none());
        assert!(visible(&h.app));
    }

    #[test]
    fn blocked_move_warns_from_both_entry_points_and_dismisses_without_editing() {
        for reason in ["rigid", "grounded", "limited", "parameter"] {
            let (mut h, joint) = assembly(if reason == "rigid" { "rigid" } else { "revolute" }, Vec3::Z, false);
            match reason {
                "grounded" => {
                    h.app.run("occurrence.ground", json!({"occurrence":"Arm:1","grounded":true})).unwrap();
                }
                "limited" => {
                    h.app.run("joint.limits", json!({"joint":joint,"min":0,"max":0})).unwrap();
                }
                "parameter" => {
                    h.app.run("parameters.add", json!({"name":"MoveAngle","expression":"0 deg"})).unwrap();
                    h.app.run("joint.delete", json!({"joint":joint})).unwrap();
                    let at = |name: &str| json!({"occurrence":name,"point":[0,0,5],"z":[0,0,1]});
                    h.app
                        .run("joint.create", json!({"type":"revolute","a":at("Base"),"b":at("Arm"),"flip":true,"angle":0,"values":["MoveAngle"]}))
                        .unwrap();
                }
                _ => {}
            }
            h.frames(3);
            let before = h.app.session.doc.to_json();
            let selection = h.app.session.selection.clone();
            let undo = h.app.session.undo.len();
            h.step(&json!({"click":{"handle":"tab:ASSEMBLE"}})).unwrap();
            h.step(&json!({"click":{"handle":"button:occurrence.move"}})).unwrap();
            h.frames(3);
            assert_eq!(h.app.menu.joint_move_warning.as_ref().map(|b| (b.part.as_str(), b.conflicts.len())), Some(("Arm:1", 0)), "{reason}");
            assert!(crate::scenario::rect_of("dialog:window").is_some(), "a modal must be visible: {reason}");
            assert!(!visible(&h.app));
            h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(2);
            assert!(h.app.menu.joint_move_warning.is_none());
            let item =
                crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport).into_iter().find(|i| i.id == "occurrence.move").unwrap();
            crate::context_menu::run_item(&mut h.app, &item, Pos2::ZERO);
            h.frames(3);
            assert!(h.app.menu.joint_move_warning.is_some(), "context Move: {reason}");
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.menu.joint_move_warning.is_none());
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!(h.app.session.selection, selection);
            assert_eq!(h.app.session.undo.len(), undo);
        }
    }
}
