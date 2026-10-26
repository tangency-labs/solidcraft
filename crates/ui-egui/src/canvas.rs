//! On-canvas command input: while a feature dialog is open, its main value (extrude distance,
//! fillet radius…) gets a value box in the viewport next to the geometry, and an arrow
//! (extrude, fillet, shell, move) that can be dragged: values snap to round steps (hold Alt or
//! Ctrl for a free drag) and the dialog follows. The box takes the keyboard as soon as the
//! command has its geometry, so typing a number sets the value at once; Enter applies, Tab
//! moves on, Esc cancels.

use crate::ui_kit::{Container, Surface};
use egui::{Color32, Pos2, Stroke, vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::{Mesh, Vec2, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::theme::Tokens;
use crate::viewport::{Proj, projected_polygon};

/// Where the dialog's geometry is: a base point and, for values that can be dragged, the
/// direction a growing value points.
pub fn anchor(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Option<Vec3>)> {
    let st = app.session.world_state();
    let first = d.inputs.first()?.items.first()?;
    let (at, normal) = match first {
        Sel::Profile { sketch, index } => {
            let ss = st.sketch(*sketch)?;
            let plane = profile_plane(app, *sketch)?;
            let p = ss.profiles.get(*index)?;
            let centre = if matches!(d.kind, Kind::Extrude { .. }) {
                let regions: Vec<_> = d
                    .inputs
                    .first()?
                    .items
                    .iter()
                    .filter_map(|item| {
                        if let Sel::Profile { sketch: id, index } = item { (*id == *sketch).then(|| ss.profiles.get(*index)).flatten() } else { None }
                    })
                    .map(|p| p.region.clone())
                    .collect();
                let regions = solidcraft_engine::sketch::merge_regions(&regions);
                let points: Vec<_> = regions.iter().flat_map(|r| r.outer.polyline(1e-2)).collect();
                points.iter().fold(solidcraft_engine::geom::Vec2::default(), |sum, p| sum + *p) * (1.0 / points.len().max(1) as f64)
            } else {
                p.region.interior_point()
            };
            (plane.to_world(centre), Some(plane.normal()))
        }
        Sel::Face { body, index, point } => (*point, crate::dialogs::planar_face(&app.session, body, *index).map(|(_, n)| n)),
        Sel::Edge { body, index, point } => (*point, st.body(body).and_then(|b| edge_bisector(&b.mesh(), *index, *point))),
        Sel::Vertex { point, .. } => (*point, None),
        Sel::SketchCurve { id, sketch } => {
            let ss = match sketch {
                Some(k) => st.sketch(*k)?,
                None => st.sketches.iter().rev().find(|ss| ss.sketch.curve_index(id).is_some())?,
            };
            let pts = ss.sketch.polyline(ss.sketch.curve_index(id)?);
            let (mid, tangent) = poly_mid(&pts)?;
            // An offset's drag arrow: the in-plane direction the distance grows along. The
            // command offsets a picked curve to its left, and the window's Flip row reverses it.
            // Symmetric hides Flip and offsets both ways, so the arrow keeps the left side.
            let dir = match &d.kind {
                Kind::SketchModify(crate::sketch_modify_ui::Mod::Offset { flip, mode, .. }) => {
                    let n = if *flip && !crate::sketch_modify_ui::offset_symmetric(*mode) { -tangent.perp() } else { tangent.perp() };
                    Some(ss.plane.to_world(mid + n) - ss.plane.to_world(mid))
                }
                // A thin extrude's curves extrude along their sketch's normal.
                Kind::Extrude { .. } => Some(ss.plane.normal()),
                _ => None,
            };
            (ss.plane.to_world(mid), dir)
        }
        // A corner picked into a sketch window ("<sketch id>:<point id>", or a bare id when it
        // was selected outside one): the value box sits on the corner itself.
        Sel::SketchPoint { id } => (sketch_point_at(app, &st, id)?, None),
        Sel::Body { name } => (st.body(name)?.mesh().bounds().center(), None),
        // A feature picked as a whole (a pattern or mirror of features): the middle of what it
        // made, as a body sits at the middle of its box.
        Sel::Feature { id } => (crate::viewport::feature_bounds(app, *id)?.center(), None),
        Sel::Plane { name } => {
            let pl = solidcraft_engine::geom::Plane::named(name).or_else(|| {
                solidcraft_engine::view::construction_planes(&app.session).into_iter().find(|(_, n, _)| n == name).map(|(_, _, pl)| pl)
            })?;
            (pl.origin, Some(pl.normal()))
        }
        _ => return None,
    };
    Some(match &d.kind {
        Kind::Extrude { start, .. } => {
            let n = normal?;
            let plane = solidcraft_engine::geom::Plane::from_normal(at, n)?;
            let from = reference_height(app, d, "From Object", &plane, at).unwrap_or(0.0);
            let offset = if start.trim().is_empty() { 0.0 } else { crate::length_input::eval(&app.session.doc, start, ValueKind::Length).ok()? };
            (at + n * (from + offset), Some(n))
        }
        // A hole on a curved face drills against the face's normal where it was picked.
        Kind::Hole { .. } if matches!(first, Sel::Face { .. }) => {
            (at, normal.or_else(|| solidcraft_engine::doc::hole_direction_at(&st, at).map(|d| -d)))
        }
        // Holes at sketch points go into the part the sketch touches (or away, flipped): the
        // arrow shows which way.
        Kind::Hole { .. } => {
            let mut by_sketch: Vec<(u64, Vec<String>)> = Vec::new();
            for x in &d.inputs.first()?.items {
                if let Sel::SketchPoint { id } = x
                    && let Some((sk, pid)) = id.split_once(':')
                    && let Ok(sk) = sk.parse::<u64>()
                {
                    match by_sketch.iter_mut().find(|(s, _)| *s == sk) {
                        Some((_, v)) => v.push(pid.to_string()),
                        None => by_sketch.push((sk, vec![pid.to_string()])),
                    }
                }
            }
            let flip = d.extra.get("flip").and_then(serde_json::Value::as_bool).unwrap_or(false);
            let dir = by_sketch.first().and_then(|(sk, ids)| solidcraft_engine::doc::sketch_hole_direction(&st, *sk, ids, flip));
            (at, normal.or(dir.map(|d| -d)))
        }
        // A curved face offsets along its outward normal where it was picked.
        Kind::OffsetFaces { .. } => (at, normal.or_else(|| solidcraft_engine::doc::hole_direction_at(&st, at).map(|d| -d))),
        Kind::Section { .. } | Kind::OffsetPlane { .. } => (at, normal),
        // Fillet radii and chamfer distances grow the way `blend_grow_dir` gives (into the body
        // on an outside edge, into the open corner on an inside one); shell thickness grows into
        // the body.
        // A chamfer of two distances measures Distance 1 along its own face, not along the bisector.
        Kind::Fillet { .. } if matches!(first, Sel::Edge { .. }) => {
            let Sel::Edge { body, index, .. } = first else { return None };
            let along_face = chamfer_face_dirs(app, d, at).map(|(first_face, _)| first_face);
            (at, along_face.or_else(|| st.body(body).and_then(|b| blend_grow_dir(&b.mesh(), *index, at))))
        }
        Kind::Fillet { .. } | Kind::Shell { .. } => (at, normal.map(|n| -n)),
        // A hem's handle starts on the centre line of its layer and runs to its tip (#5828).
        Kind::Sheet(crate::dialogs_sheet::Sm::Hem { .. }) => match crate::dialogs_sheet::hem_handle(app, d) {
            Some((start, dir)) => (start, Some(dir)),
            None => (at, normal),
        },
        Kind::Sheet(_) => (at, crate::dialogs_sheet::arrow(app, d).or(normal)),
        Kind::Plastic(_) => (at, normal),
        Kind::PatternRect { .. } => (at, d.inputs.get(1).and_then(|i| i.items.first()).and_then(|x| crate::dialogs::axis_of(app, x)).map(|a| a.1)),
        // A sketch move's X Distance grows along the sketch's own x axis, so its drag arrow
        // lies in the sketch plane. Scale has no arrow: a factor is not a length, and the
        // square handle it wants is its own change.
        Kind::SketchModify(crate::sketch_modify_ui::Mod::Move { .. }) => {
            (at, app.session.active_sketch.and_then(|id| st.sketch(id)).and_then(|ss| ss.plane.x.normalized()))
        }
        // The rectangular pattern's spacing grows along the direction it was given, which the
        // module works out per variant. Offset's arrow is already computed above, per picked
        // curve, and `arrow` returns None for it so that one stands.
        Kind::SketchModify(_) => (at, crate::sketch_modify_ui::arrow(app, d).or(normal)),
        _ => (at, None),
    })
}

/// The direction a rectangular pattern's Spacing 2 grows along (#2462): a solid pattern's picked
/// (or kept) second direction, none until it has one; a sketch pattern's second direction, which
/// is square to the first unless one is picked.
fn pattern_dir2(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    match &d.kind {
        Kind::PatternRect { .. } => match d.inputs.get(2).and_then(|i| i.items.first()) {
            Some(x) => crate::dialogs::axis_of(app, x).map(|a| a.1),
            None => d.extra.get("dir2").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()).map(|a| Vec3::new(a[0], a[1], a[2])),
        },
        Kind::SketchModify(_) => crate::sketch_modify_ui::arrow2(app, d),
        _ => None,
    }
    .and_then(Vec3::normalized)
}

/// The point halfway along a polyline, with the unit tangent there. Indexing the middle sample
/// instead would land on a two-point line's far end, putting the value box and its arrow on a
/// corner. None for a polyline with no length.
fn poly_mid(poly: &[Vec2]) -> Option<(Vec2, Vec2)> {
    let total: f64 = poly.windows(2).map(|w| w[0].dist(w[1])).sum();
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let half = total / 2.0;
    let mut walked = 0.0;
    let mut last = None;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let Some(dir) = (b - a).normalized() else { continue };
        let len = a.dist(b);
        if walked + len >= half {
            return Some((a + dir * (half - walked), dir));
        }
        walked += len;
        last = Some((b, dir));
    }
    last
}

pub(crate) fn profile_plane(app: &SolveApp, sketch: u64) -> Option<solidcraft_engine::geom::Plane> {
    let component = app.session.doc.feature(sketch)?.component;
    let state = app.session.model.state();
    let plane = state.sketch(sketch)?.plane;
    let frame = crate::dialogs::feature_frame(&app.session, component);
    solidcraft_engine::geom::Plane::new(
        solidcraft_engine::doc::apply_point(&frame, plane.origin),
        solidcraft_engine::doc::apply_vector(&frame, plane.x),
        solidcraft_engine::doc::apply_vector(&frame, plane.y),
    )
}

fn reference_height(app: &SolveApp, d: &Dialog, label: &str, plane: &solidcraft_engine::geom::Plane, centre: Vec3) -> Option<f64> {
    let point = crate::dialogs::object_point(&d.inputs, label)?;
    let p = point.as_array()?;
    let p = Vec3::new(p.first()?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?);
    Some(solidcraft_engine::doc::object_height(&app.session.world_state(), plane, centre, p))
}

/// The two faces at an edge, near `p`: for each, the normal and centroid of its triangle nearest
/// `p`.
fn edge_sides(m: &Mesh, index: usize, p: Vec3) -> Option<Vec<(Vec3, Vec3)>> {
    let faces = m.edge_faces.get(index)?;
    faces
        .iter()
        .map(|f| {
            m.triangles
                .iter()
                .zip(&m.tri_face)
                .filter(|(_, tf)| *tf == f)
                .filter_map(|(t, _)| m.tri(t))
                .min_by(|a, b| ((a[0] + a[1] + a[2]) / 3.0).dist(p).total_cmp(&((b[0] + b[1] + b[2]) / 3.0).dist(p)))
                .and_then(|[a, b, c]| Some(((b - a).cross(c - a).normalized()?, (a + b + c) / 3.0)))
        })
        .collect()
}

/// The outward direction between the two faces at an edge: the mean of their normals.
fn edge_bisector(m: &Mesh, index: usize, p: Vec3) -> Option<Vec3> {
    edge_sides(m, index, p)?.iter().fold(Vec3::ZERO, |sum, (n, _)| sum + *n).normalized()
}

/// Whether the edge is an inside (concave) one: each face near it lies in front of the other's
/// plane, where on an outside edge it lies behind. Only the offset across the edge counts, since
/// each normal is square to the edge.
fn edge_concave(m: &Mesh, index: usize, p: Vec3) -> Option<bool> {
    let sides = edge_sides(m, index, p)?;
    let [(n1, c1), (n2, c2)] = sides.as_slice() else { return None };
    let s = (*c1 - p).dot(*n2) + (*c2 - p).dot(*n1);
    Some(s > 0.0)
}

/// The direction a fillet's radius or a chamfer's distance grows from an edge, along the bisector
/// of its faces: into the body on an outside edge, where the blend takes material away, and out
/// into the open corner on an inside edge, where it adds material (#1384). Dragging along it
/// increases the value either way.
pub(crate) fn blend_grow_dir(m: &Mesh, index: usize, p: Vec3) -> Option<Vec3> {
    let n = edge_bisector(m, index, p)?;
    Some(if edge_concave(m, index, p).unwrap_or(false) { n } else { -n })
}

/// The directions Distance 1 and Distance 2 of a chamfer of two distances run from its edge at
/// `at`: along the face that faces up most, or the other one with Flip, and along the remaining
/// face (the kernel's `chamfer_tool_sides` picks them the same way). None for any other dialog.
pub(crate) fn chamfer_face_dirs(app: &SolveApp, d: &Dialog, at: Vec3) -> Option<(Vec3, Vec3)> {
    let Kind::Fillet { chamfer: true, ctype: 1, flip, .. } = &d.kind else { return None };
    let Some(Sel::Edge { body, index, .. }) = d.inputs.first()?.items.first() else { return None };
    let st = app.session.world_state();
    let mesh = st.body(body)?.mesh();
    let sides = edge_sides(&mesh, *index, at)?;
    let [(n1, c1), (n2, c2)] = sides.as_slice() else { return None };
    let along = n1.cross(*n2).normalized()?;
    // From the edge into each face: the way to its centroid, square to the edge.
    let into = |c: Vec3| {
        let v = c - at;
        (v - along * v.dot(along)).normalized()
    };
    let (t1, t2) = (into(*c1)?, into(*c2)?);
    let up = Vec3::new(1e-6, 1e-3, 1.0);
    Some(if (n2.dot(up) > n1.dot(up)) != *flip { (t2, t1) } else { (t1, t2) })
}

/// Whether a dynamically registered canvas handle owns this point before navigation.
pub(crate) fn owns_pointer(app: &SolveApp, proj: &Proj, p: Pos2) -> bool {
    // The triad only registers an egui hit widget after a handle is under the pointer.
    // Test its geometry before navigation, including the first frame entering a handle.
    app.dialog.as_ref().and_then(|d| crate::dialogs_move::triad(app, d)).is_some_and(|t| crate::gizmo::contains_pointer(proj, &t, p))
        || app
            .dialog
            .as_ref()
            .and_then(|d| move_rotation(app, d, proj))
            .and_then(|r| proj.to_screen(r.point(r.angle)))
            .is_some_and(|end| egui::Rect::from_center_size(end, vec2(20.0, 20.0)).contains(p))
        || (app.dialog.is_none()
            && app.tool.is_none()
            && crate::browser::occurrence_triad_model(app).is_some_and(|t| crate::gizmo::contains_pointer(proj, &t, p)))
        || crate::place_gizmo::contains_pointer(app, p)
}

/// Draw the manipulator and the value box (called by the viewport after the model is drawn).
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    // While OK computes, the inputs hold what it computes: no arrow to drag, no box to type in.
    if app.applying.is_some() {
        return;
    }
    if app.dialog.is_none() && app.tool.is_none() {
        crate::browser::occurrence_triad(app, ui, painter, proj);
    }
    let Some(mut d) = app.dialog.take() else { return };
    if matches!(d.kind, Kind::Assembly(_)) {
        crate::dialogs_assembly::overlay(app, painter, proj, &mut d);
        app.dialog = Some(d);
        return;
    }
    if crate::dialogs_sheet::overlay(app, painter, proj.rect, &d) {
        app.dialog = Some(d);
        return;
    }
    if let Kind::Measure { result: Some(r), show, .. } = &d.kind {
        measure_line(app, painter, proj, r, *show);
        app.dialog = Some(d);
        return;
    }
    // Move: the triad at the pivot; dragging it sets the distances and angles. Its value box
    // sits beside it and reads the move's main value, so a drag shows its number and typing
    // one sets it. The triad is the manipulator here, so the box comes without the drag arrow
    // every other dialog's value gets (`dir` is None below).
    if matches!(d.kind, Kind::Move { .. }) {
        let mut dragging = false;
        if let Some(t) = crate::dialogs_move::triad(app, &d)
            && let Some(g) = crate::gizmo::show(ui, painter, proj, egui::Id::new("sc_move_triad"), &t, snap_step(app.cam.half_height()))
        {
            crate::dialogs_move::drag(app, &mut d, g);
            dragging = true;
        }
        // Read the triad again after the drag, so the box follows it in the same frame. Move
        // types with no triad put the box on the picked bodies, or beside Rotate's handle
        // once an axis is selected.
        let triad = crate::dialogs_move::triad(app, &d);
        let rotation = draw_move_rotation(app, ui, painter, proj, &mut d);
        dragging |= rotation.is_some_and(|(_, changed)| changed);
        // The box stays clear of the triad's handles: it is a foreground area, so a box over a
        // handle takes the press and the drag never reaches the model (#6608).
        let mut place = box_place(app, proj);
        if let Some(t) = triad.as_ref() {
            place.handles = crate::gizmo::handles_region(proj, t);
        }
        let at = triad.map(|t| t.center).or_else(|| anchor(app, &d).map(|(base, _)| base));
        // Tab and Shift+Tab in the box show the next or previous value (Y after X; #4776),
        // keeping the keyboard in the box.
        // egui moves the keyboard on with Tab before this runs: the box had it last frame.
        let id = egui::Id::new("sc_canvas_value");
        let boxed = ui.ctx().memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
        let back = boxed && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
        let tab = boxed && !back && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
        if tab || back {
            crate::dialogs_move::step_value_field(&mut d, if back { -1 } else { 1 });
            d.focus = true;
        }
        let focus = std::mem::take(&mut d.focus);
        if let Some(bs) = at.and_then(|base| proj.to_screen(base))
            && let Some((label, _, value)) = d.primary()
        {
            // Clear of the triad: its arrows reach LEN_PX and its rings RING of that, so a box
            // beside the centre would cover the handles the user is reaching for.
            let off = if triad.is_some() { vec2(crate::gizmo::LEN_PX as f32 + 16.0, -12.0) } else { vec2(16.0, -30.0) };
            let box_at = rotation.map_or(bs + off, |(end, _)| end + vec2(16.0, -12.0));
            value_box_with(ui, label, value, box_at, focus || dragging, true, place);
        }
        app.dialog = Some(d);
        return;
    }
    if let Some((base, dir)) = anchor(app, &d)
        && let Some(bs) = proj.to_screen(base)
    {
        draw(app, ui, painter, proj, &mut d, base, bs, dir);
    }
    crate::place_gizmo::show(app, ui, painter, proj, &mut d);
    app.dialog = Some(d);
    // A Single hole's handles, over the depth arrow's stem: a press on a hole moves it.
    crate::hole_place::show(app, ui, painter, proj);
}

struct MoveRotation {
    centre: Vec3,
    axis: Vec3,
    radius: f64,
    angle: f64,
}

impl MoveRotation {
    fn point(&self, angle: f64) -> Vec3 {
        let e1 = self.axis.any_perp();
        let e2 = self.axis.cross(e1);
        self.centre + (e1 * angle.cos() + e2 * angle.sin()) * self.radius
    }
}

fn move_rotation(app: &SolveApp, d: &Dialog, proj: &Proj) -> Option<MoveRotation> {
    let Kind::Move { angle, .. } = &d.kind else { return None };
    let (centre, axis) = crate::dialogs_move::rotation_axis(app, d)?;
    let angle = crate::length_input::eval(&app.session.doc, angle, ValueKind::Angle).ok().filter(|a| a.is_finite()).unwrap_or(0.0);
    Some(MoveRotation { centre, axis, radius: crate::gizmo::ring_radius(proj, centre)?, angle })
}

/// Reuse the angular handle's snapping and edge-on projection for Move's selected axis.
fn draw_move_rotation(app: &SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog) -> Option<(Pos2, bool)> {
    let r = move_rotation(app, d, proj)?;
    let points: Vec<_> = (0..=64).filter_map(|i| proj.to_screen(r.point(i as f64 / 64.0 * std::f64::consts::TAU))).collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.8, Tokens::get().manipulator)));
    let Kind::Move { angle, .. } = &mut d.kind else { return None };
    let before = angle.clone();
    let e1 = r.axis.any_perp();
    // Keep full turns in the expression while the shared handle works near zero.
    let boundary = r.angle % std::f64::consts::TAU;
    let offset = r.angle - boundary;
    let span = 2.0 * std::f64::consts::TAU;
    let end = rotation_handle(
        ui,
        painter,
        proj,
        r.centre,
        e1,
        r.axis.cross(e1),
        r.axis,
        r.radius,
        boundary,
        1.0,
        offset,
        (offset - span, offset + span),
        "move_rotate",
        angle,
    )?;
    Some((end, *angle != before))
}

/// A round step for dragged values at this zoom (1, 2 or 5 times a power of ten).
pub fn snap_step(half_height: f64) -> f64 {
    let raw = (half_height / 40.0).max(1e-4);
    let p = 10f64.powf(raw.log10().floor());
    if raw / p < 2.0 {
        p
    } else if raw / p < 5.0 {
        2.0 * p
    } else {
        5.0 * p
    }
}

/// Parameter along the line `base + dir * t` closest to the view ray through `pos`.
pub(crate) fn along(proj: &Proj, pos: Pos2, base: Vec3, dir: Vec3) -> Option<f64> {
    let (o, r) = proj.ray(pos);
    let w0 = base - o;
    let (b, d, e) = (dir.dot(r), dir.dot(w0), r.dot(w0));
    let den = 1.0 - b * b;
    // Looking straight along the line: nothing to drag.
    if den.abs() < 1e-6 {
        return None;
    }
    Some((b * e - d) / den)
}

pub(crate) fn format_len(doc: &solidcraft_engine::doc::Document, v: f64) -> String {
    crate::length_input::model(doc, v)
}

/// Where a sketch point picked as "<sketch id>:<point id>" (or a bare id of the active sketch) is.
fn sketch_point_at(app: &SolveApp, st: &solidcraft_engine::doc::ModelState, id: &str) -> Option<Vec3> {
    let (sketch, point) = match id.split_once(':') {
        Some((sketch, point)) => (sketch.parse().ok()?, point),
        None => (app.session.active_sketch?, id),
    };
    let ss = st.sketch(sketch)?;
    Some(ss.plane.to_world(ss.sketch.point(ss.sketch.resolve_point(point)?)?))
}

/// The offset plane preview: a square of half side `h` around `c`, square to `n`.
fn offset_plane_quad(proj: &Proj, c: Vec3, n: Vec3, h: f64) -> Option<Vec<Pos2>> {
    let u = n.cross(if n.z.abs() < 0.9 { Vec3::Z } else { Vec3::X }).normalized()?;
    let v = n.cross(u);
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(a, b)| proj.to_screen(c + u * (a * h) + v * (b * h))).collect()
}

#[allow(clippy::too_many_arguments)]
fn draw(app: &SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog, base: Vec3, bs: Pos2, dir: Option<Vec3>) {
    let t = Tokens::get();
    let to_span = dir.and_then(|n| {
        let plane = solidcraft_engine::geom::Plane::from_normal(base, n)?;
        reference_height(app, d, "To Object", &plane, base)
    });
    let all_span = if matches!(d.kind, Kind::Extrude { all: true, .. }) {
        dir.and_then(|n| {
            let state = app.session.world_state();
            let plane = match d.inputs.first()?.items.first()? {
                Sel::Profile { sketch, .. } => profile_plane(app, *sketch)?,
                _ => solidcraft_engine::geom::Plane::from_normal(base, n)?,
            };
            let targets: Vec<String> = d
                .extra
                .get("targets")
                .and_then(serde_json::Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            solidcraft_engine::doc::through_all_distance(&state, &plane, &targets).ok()
        })
    } else {
        None
    };
    let sign = match (to_span, &d.kind) {
        (Some(span), _) => {
            if span < 0.0 {
                -1.0
            } else {
                1.0
            }
        }
        (_, Kind::Extrude { direction: 1 | crate::dialogs::TWO_SIDES_REVERSED, .. }) => -1.0,
        _ => 1.0,
    };
    let symmetric = to_span.is_none() && matches!(d.kind, Kind::Extrude { direction: 2, .. });
    let symmetric_scale = if symmetric {
        d.extra
            .get("convention")
            .cloned()
            .and_then(|v| serde_json::from_value::<solidcraft_engine::doc::ExtrudeConvention>(v).ok())
            .unwrap_or(solidcraft_engine::doc::ExtrudeConvention::TotalDistance)
            .symmetric_half_distance(1.0)
    } else {
        1.0
    };
    // Extrudes, moves, pattern spacings and one-sided sketch offsets go either way (#5497);
    // radii, thicknesses, depths and heights stay positive. A symmetric offset takes the
    // distance's size only, so a drag past its curve would just turn the arrow round.
    let signed = match &d.kind {
        Kind::Extrude { .. }
        | Kind::Move { .. }
        | Kind::Section { .. }
        | Kind::OffsetPlane { .. }
        | Kind::OffsetFaces { .. }
        | Kind::PatternRect { .. } => true,
        Kind::SketchModify(crate::sketch_modify_ui::Mod::Move { .. } | crate::sketch_modify_ui::Mod::PatternRect { .. }) => true,
        Kind::SketchModify(crate::sketch_modify_ui::Mod::Offset { mode, .. }) => !crate::sketch_modify_ui::offset_symmetric(*mode),
        _ => false,
    };
    let mut focus = std::mem::take(&mut d.focus);
    let half_height = app.cam.half_height();
    let axis = revolve_axis(app, d);
    if matches!(d.kind, Kind::Revolve { .. }) {
        if let Some(axis) = axis {
            draw_revolve(app, ui, painter, proj, d, base, bs, axis, focus);
        }
        return;
    }
    // Two sides: the second distance gets its own arrow, the other way from the first side.
    if let (Kind::Extrude { direction: crate::dialogs::TWO_SIDES | crate::dialogs::TWO_SIDES_REVERSED, distance2, .. }, Some(n)) = (&mut d.kind, dir)
        && let Some(l2) = crate::length_input::eval(&app.session.doc, distance2, ValueKind::Length).ok().filter(|v| v.is_finite())
    {
        drag_arrow(
            &app.session.doc,
            ui,
            painter,
            proj,
            base,
            bs,
            -(n * sign),
            l2,
            true,
            egui::Id::new("sc_manipulator2"),
            half_height,
            0.0,
            distance2,
        );
    }
    // A chamfer of two distances: Distance 2 gets its own arrow along the other face.
    let faces = chamfer_face_dirs(app, d, base);
    if let (Kind::Fillet { distance2, .. }, Some((_, t2))) = (&mut d.kind, faces)
        && let Some(l2) = crate::length_input::eval(&app.session.doc, distance2, ValueKind::Length).ok().filter(|v| v.is_finite())
    {
        drag_arrow(&app.session.doc, ui, painter, proj, base, bs, t2, l2, false, egui::Id::new("sc_manipulator2"), half_height, 0.0, distance2);
    }
    // A rectangular pattern's second direction gets its own arrow for Spacing 2 (#2462), as the
    // first direction's arrow sets Spacing.
    if let Some(n2) = pattern_dir2(app, d) {
        let spacing2 = match &mut d.kind {
            Kind::PatternRect { spacing2, .. } | Kind::SketchModify(crate::sketch_modify_ui::Mod::PatternRect { spacing2, .. }) => Some(spacing2),
            _ => None,
        };
        if let Some(spacing2) = spacing2
            && let Some(l2) = crate::length_input::eval(&app.session.doc, spacing2, ValueKind::Length).ok().filter(|v| v.is_finite())
        {
            drag_arrow(&app.session.doc, ui, painter, proj, base, bs, n2, l2, true, egui::Id::new("sc_manipulator2"), half_height, 0.0, spacing2);
        }
    }
    // An offset plane shows where it will be: at the distance, or through the picked point.
    let through = crate::dialogs::offset_through_point(d).and_then(|p| match p {
        Sel::Vertex { point, .. } => Some(*point),
        Sel::SketchPoint { id } => sketch_point_at(app, &app.session.world_state(), id),
        _ => None,
    });
    if let (Kind::OffsetPlane { offset }, Some(n)) = (&d.kind, dir)
        && let Some(off) = through.map(|p| (p - base).dot(n)).or_else(|| crate::length_input::eval(&app.session.doc, offset, ValueKind::Length).ok())
        && let Some(q) = offset_plane_quad(proj, base + n * off, n, half_height * 0.25)
    {
        painter.extend(projected_polygon(&q, t.construction_plane, Stroke::new(1.2, t.origin_plane_edge)));
    }
    // Holes: the depth arrow points into the material (empty depth: through all); the
    // diameter is typed in the box.
    let mut dir = dir;
    // Extrude taper: a handle beside the arrow tip; dragging it sideways tilts the walls.
    let cam_right = app.cam.basis().0;
    if let (Kind::Extrude { direction: 0 | 1, distance, taper, taper_present, to_offset, .. }, Some(n)) = (&mut d.kind, dir) {
        let n = n * sign;
        let span =
            to_span.map(|span| span.abs() + crate::length_input::eval(&app.session.doc, to_offset, ValueKind::Length).unwrap_or(0.0)).or(all_span);
        if let Ok(l) = span.map(Ok).unwrap_or_else(|| crate::length_input::eval(&app.session.doc, distance, ValueKind::Length))
            && l.is_finite()
            && l.abs() > 1e-9
            && let Some(e) = (cam_right - n * cam_right.dot(n)).normalized()
        {
            let px = 2.0 * half_height / f64::from(proj.rect.height().max(1.0));
            let ang = crate::length_input::eval(&app.session.doc, taper, ValueKind::Angle).ok().filter(|v| v.is_finite()).unwrap_or(0.0);
            let before = taper.clone();
            let st = app.session.world_state();
            let regions: Vec<&solidcraft_engine::geom::Region2> = d
                .inputs
                .first()
                .map(|i| &i.items[..])
                .unwrap_or(&[])
                .iter()
                .filter_map(|x| match x {
                    Sel::Profile { sketch, index } => st.sketch(*sketch).and_then(|ss| ss.profiles.get(*index)).map(|pr| &pr.region),
                    _ => None,
                })
                .collect();
            let limits = taper_limits(&regions, l.abs());
            taper_handle(ui, painter, proj, base - e * (40.0 * px), n * l, -e, ang, limits, taper);
            if *taper != before {
                *taper_present = true;
            }
        }
    }
    if let (Kind::Hole { depth, opts, .. }, Some(n)) = (&mut d.kind, dir) {
        let l = if opts.all {
            0.0
        } else {
            crate::length_input::eval(&app.session.doc, depth, ValueKind::Length).ok().filter(|v| v.is_finite()).unwrap_or(0.0)
        };
        let before = depth.clone();
        drag_arrow(&app.session.doc, ui, painter, proj, base, bs, -n, l, false, egui::Id::new("sc_manipulator_depth"), half_height, 0.0, depth);
        // Dragging the depth turns "through all" into a distance.
        if *depth != before {
            opts.all = false;
        }
        dir = None;
        // A Single hole's distances from its reference edges (#37); a newly picked edge's box
        // takes the keyboard.
        if crate::hole_refs::canvas(app, ui, painter, proj, d) {
            focus = false;
        }
    }
    // A fillet with a radius spline is shaped by its handles along the edge; the arrow would sit
    // on one of them.
    if radius_spline_handles(app, ui, painter, proj, d) {
        dir = None;
    }
    // Two-sided sketch offset: Distance 2 is its own dimension, the other way from Distance.
    // One Side and Symmetric keep the single Distance box.
    if let Kind::SketchModify(crate::sketch_modify_ui::Mod::Offset { distance2, mode, .. }) = &mut d.kind
        && crate::sketch_modify_ui::offset_two_sides(*mode)
        && let Some(n) = dir
        && let Some(l2) = app.session.doc.eval(distance2, ValueKind::Length).ok().filter(|v| v.is_finite())
    {
        let before = distance2.clone();
        if let Some(ts) = drag_arrow(
            &app.session.doc,
            ui,
            painter,
            proj,
            base,
            bs,
            -n,
            l2.abs(),
            false,
            egui::Id::new("sc_manipulator_offset2"),
            half_height,
            0.0,
            distance2,
        ) {
            // Its own area and field, so it does not share Distance's focus or text.
            value_box_in(
                ui,
                (egui::Id::new("sc_canvas_area2"), egui::Id::new("sc_canvas_value2")),
                "Distance 2",
                distance2,
                (ts + vec2(14.0, 8.0), egui::Align2::LEFT_TOP),
                *distance2 != before,
                false,
                box_place(app, proj),
            );
        }
    }
    // A placement marker at the base: the arrow stands above it and the base dot is the marker's.
    let clear = crate::place_gizmo::arrow_clearance(app, d);
    let Some((label, kind, value)) = d.primary() else { return };
    let len = all_span.or_else(|| {
        if to_span.is_some() && value.trim().is_empty() {
            Some(0.0)
        } else {
            crate::length_input::eval(&app.session.doc, value, kind).ok().filter(|v| v.is_finite())
        }
    });
    let before = value.clone();
    let mut box_at = bs + vec2(16.0, -30.0);
    // Where the box hangs from: the arrow's tip, or the base when there is no arrow.
    let mut anchor = bs;
    if let (Some(n), Some(l)) = (dir, len.filter(|_| kind == ValueKind::Length)) {
        let n = n * sign;
        let baseline = to_span.map(f64::abs).unwrap_or(0.0);
        // Through All already supplies a reach per side; dragging switches to an entered distance.
        let half = if all_span.is_some() { l } else { l * symmetric_scale };
        let mut shown_value = format_len(&app.session.doc, half + baseline);
        let original_shown = shown_value.clone();
        let dragged = if to_span.is_some() || symmetric_scale != 1.0 { &mut shown_value } else { &mut *value };
        if let Some(ts) = drag_arrow(
            &app.session.doc,
            ui,
            painter,
            proj,
            base,
            bs,
            n,
            half + baseline,
            signed,
            egui::Id::new("sc_manipulator"),
            half_height,
            clear,
            dragged,
        ) {
            if (to_span.is_some() || symmetric_scale != 1.0)
                && shown_value != original_shown
                && let Ok(v) = crate::length_input::eval(&app.session.doc, &shown_value, ValueKind::Length)
            {
                *value = format_len(&app.session.doc, (v - baseline) / symmetric_scale);
            }
            if symmetric && let Some(bt) = proj.to_screen(base - n * half) {
                arrow(painter, bs, bt, t.manipulator);
            }
            if clear == 0.0 {
                painter.circle(bs, 3.5, t.manipulator, Stroke::new(1.0, Color32::WHITE));
            }
            box_at = ts + vec2(14.0, -12.0);
            anchor = ts;
        }
    }
    let note = crate::drag_extrude::step_note(app);
    // Also without an arrow (the value does not evaluate yet): the box still keeps off the handles.
    let box_at = crate::place_gizmo::yield_box(ui, anchor, box_at);
    value_box_noted(ui, label, value, box_at, focus || *value != before, note.as_deref(), box_place(app, proj));
    if *value != before
        && let Kind::Extrude { all, .. } = &mut d.kind
    {
        *all = false;
    }
}

/// Where a value box may sit on the canvas: clear of the command window and of the
/// manipulator's handles, inside the drawing area.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BoxPlace {
    /// The command window's rectangle: a box that would cover it moves out of it (#6608).
    pub dialog: Option<egui::Rect>,
    /// The drawing area the box stays in.
    pub view: Option<egui::Rect>,
    /// The manipulator's handles: a box that would cover them moves out of them (#6608).
    pub handles: Option<egui::Rect>,
}

/// The place the value boxes of a command may take: clear of the command window, inside the
/// drawing area. The handles are added by the caller that draws a manipulator.
pub(crate) fn box_place(app: &SolveApp, proj: &Proj) -> BoxPlace {
    BoxPlace { dialog: app.viewport.dialog_rect, view: Some(proj.rect), handles: None }
}

/// The command's main value in the viewport, next to the geometry: the keyboard goes here while
/// the window is open, so a number typed straight after a pick sets the value. `at` is its
/// top-left corner. `select_all` re-selects the whole field, both right after a pick and on every
/// frame a drag is changing the value underneath it: without the latter, a drag that grows the
/// text (e.g. "4 mm" -> "15 mm") would leave the stale, shorter selection from the pick in place,
/// covering only part of the new text. With value stepping on, the box stays where it is while
/// the pointer is over it: the wheel steps its value (`value_step`), which moves the arrow tip
/// the box follows, and a box sliding out from under the pointer would hand the next notch to
/// the camera.
fn value_box(ui: &mut egui::Ui, label: &str, value: &mut String, at: Pos2, select_all: bool, place: BoxPlace) {
    value_box_with(ui, label, value, at, select_all, false, place);
}

/// [`value_box`]; with `keep_tab` Tab stays in the box for the caller (the Move box steps
/// through its values with it).
fn value_box_with(ui: &mut egui::Ui, label: &str, value: &mut String, at: Pos2, select_all: bool, keep_tab: bool, place: BoxPlace) {
    let ids = (egui::Id::new("sc_canvas_area"), egui::Id::new("sc_canvas_value"));
    value_box_in(ui, ids, label, value, (at, egui::Align2::LEFT_TOP), select_all, keep_tab, place);
}

/// [`value_box`] with a note after the field (a pull's step size).
fn value_box_noted(ui: &mut egui::Ui, label: &str, value: &mut String, at: Pos2, select_all: bool, note: Option<&str>, place: BoxPlace) {
    let ids = (egui::Id::new("sc_canvas_area"), egui::Id::new("sc_canvas_value"));
    value_box_full(ui, ids, label, value, (at, egui::Align2::LEFT_TOP), (select_all, false), note, place);
}

/// [`value_box_with`] under its own ids (its area's and its field's), for a value beside the main
/// one (a hole's reference distances), with the point of the box that sits at `at`. Returns where
/// the field is.
pub(crate) fn value_box_in(
    ui: &mut egui::Ui,
    ids: (egui::Id, egui::Id),
    label: &str,
    value: &mut String,
    at: (Pos2, egui::Align2),
    select_all: bool,
    keep_tab: bool,
    place: BoxPlace,
) -> Option<egui::Rect> {
    value_box_full(ui, ids, label, value, at, (select_all, keep_tab), None, place)
}

/// [`value_box_in`] with a short dimmed note after the field (a pull's step size).
fn value_box_full(
    ui: &mut egui::Ui,
    (area, id): (egui::Id, egui::Id),
    label: &str,
    value: &mut String,
    (at, pivot): (Pos2, egui::Align2),
    (select_all, keep_tab): (bool, bool),
    note: Option<&str>,
    place: BoxPlace,
) -> Option<egui::Rect> {
    let t = Tokens::get();
    // Held in place while the pointer is over it and value stepping is on (see `value_box`): by
    // its pivot, the point of the box that sits at `at`, which is not always its top-left corner.
    let pointer = ui.ctx().pointer_hover_pos();
    let at = ui
        .ctx()
        .memory(|m| m.area_rect(area))
        .filter(|r| crate::value_step::enabled(ui.ctx()) && pointer.is_some_and(|p| r.contains(p)))
        .map_or(at, |r| pivot.pos_in_rect(&r));
    // #6608: the box moves out of the command window it would otherwise cover, and out of the
    // manipulator's handles: it keeps its place by its geometry, but a box that would cover
    // either of them moves out of it.
    let at = if place.dialog.is_some() || place.handles.is_some() {
        let kept = clear_of_dialog(rect_at(at, pivot, box_size(ui, area, label, note)), place);
        pivot.pos_in_rect(&kept)
    } else {
        at
    };
    let mut field = None;
    Container::new(ui.ctx(), Surface::Canvas).area(area).fixed_pos(at).pivot(pivot).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(crate::i18n::tr_label(label)).color(t.text_dim).size(crate::theme::Tokens::get().font_caption));
                let r = crate::ui_kit::canvas_value_field(ui, id, value, keep_tab);
                crate::params_dialog::complete(ui, &r, value);
                if select_all {
                    r.request_focus();
                    let n = value.chars().count();
                    let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                    st.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                    st.store(ui.ctx(), id);
                }
                if let Some(note) = note {
                    ui.label(egui::RichText::new(note).color(t.text_dim).size(crate::theme::Tokens::get().font_caption));
                }
                field = Some(r.rect);
            });
        });
    });
    field
}

/// The gap left between a value box and what it must not cover (px).
const DIALOG_GAP: f32 = 4.0;

/// Where a value box goes so it does not cover the command window nor the manipulator's
/// handles: whole, beside what it would cover (leaving a gap of `DIALOG_GAP`), and inside the
/// drawing area (`view`), like a box kept in view (#6608). The sides of the command window
/// are tried first, then the sides of the handles, and the first side that leaves the box
/// clear of both wins. A box that misses them keeps its place, and one that no side clears
/// keeps it too, unless a side leaves the handles free: a box over a handle takes the press
/// the user meant for the handle.
fn clear_of_dialog(rect: egui::Rect, place: BoxPlace) -> egui::Rect {
    let mut obstacles: Vec<egui::Rect> = Vec::new();
    if let Some(dialog) = place.dialog {
        obstacles.push(dialog);
    }
    if let Some(handles) = place.handles {
        obstacles.push(handles);
    }
    if obstacles.iter().all(|o| !rect.intersects(*o)) {
        return rect;
    }
    // The sides the box could go to: each moves it whole, just past the obstacle's edge. The
    // left one comes first: it slides the box back over the part that overlaps it, instead of
    // jumping it over the whole obstacle.
    let mut free = None;
    for o in &obstacles {
        let sides = [
            egui::vec2(o.left() - DIALOG_GAP - rect.right(), 0.0),
            egui::vec2(o.right() + DIALOG_GAP - rect.left(), 0.0),
            egui::vec2(0.0, o.top() - DIALOG_GAP - rect.bottom()),
            egui::vec2(0.0, o.bottom() + DIALOG_GAP - rect.top()),
        ];
        for d in sides {
            let moved = keep_in_view(rect.translate(d), place.view);
            // This side leaves the box clear of everything: the next one may clear it too.
            if obstacles.iter().all(|o| !moved.intersects(*o)) {
                return moved;
            }
            // A box that still covers the command window but leaves the handles free is
            // better than one over a handle: the handle takes the press (#6608).
            if free.is_none() && place.handles.is_some_and(|h| !moved.intersects(h)) {
                free = Some(moved);
            }
        }
    }
    free.unwrap_or(rect)
}

/// A rectangle kept inside the drawing area: one that would leave it moves back to its edge.
fn keep_in_view(rect: egui::Rect, view: Option<egui::Rect>) -> egui::Rect {
    let Some(view) = view else { return rect };
    let size = rect.size();
    let min = rect.min.clamp(view.min, (view.max - size).max(view.min));
    egui::Rect::from_min_size(min, size)
}

/// The rectangle a value box of `size` takes when its pivot sits at `at`.
fn rect_at(at: Pos2, pivot: egui::Align2, size: egui::Vec2) -> egui::Rect {
    let offset = pivot.pos_in_rect(&egui::Rect::from_min_size(Pos2::ZERO, size));
    egui::Rect::from_min_size(at - egui::vec2(offset.x, offset.y), size)
}

/// The size of a value box: the one it had last frame, or measured from the fonts when it is
/// new (its label, its field, and the margin the popup frame puts around them).
fn box_size(ui: &egui::Ui, area: egui::Id, label: &str, note: Option<&str>) -> egui::Vec2 {
    if let Some(r) = ui.ctx().memory(|m| m.area_rect(area)) {
        let size = r.size();
        if size.is_finite() && size.x > 0.0 && size.y > 0.0 {
            return size;
        }
    }
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_caption);
    let width = |text: String| ui.painter().layout_no_wrap(text, font.clone(), t.text_dim).size().x;
    let mut w = width(crate::i18n::tr_label(label).to_string()) + t.space_24 * 3.0;
    let mut parts = 1;
    if let Some(note) = note {
        w += width(note.to_string());
        parts += 1;
    }
    // The gap between the row's parts, and the popup frame's margin around them.
    let gap = ui.spacing().item_spacing.x;
    let height = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let size = egui::vec2(w + gap * parts as f32 + 12.0, height + 6.0);
    if size.is_finite() && size.x > 0.0 && size.y > 0.0 { size } else { crate::sketch_dims::BOX }
}

/// How far (logical points) from an arrow's stem a press still takes the arrow.
const ARROW_STEM_REACH: f32 = 6.0;

/// The distance from `p` to the segment from `a` to `b` on screen.
fn to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let v = b - a;
    let t = if v.length_sq() > 0.0 { ((p - a).dot(v) / v.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + v * t)
}

/// A draggable arrow from `base` along `n` showing `l` (at least a short stub); dragging writes
/// the snapped length into `value`. The tip and the stem take the drag (#1740): pressed on the
/// stem, the arrow keeps its length until the pointer moves, and then moves with it as far as the
/// pointer does. Returns the tip's screen position. `clear_px` keeps that many screen points at
/// the base free: the arrow starts past them and its shortest length grows by them, so it stands
/// above a placement marker drawn at the base instead of through it.
#[allow(clippy::too_many_arguments)]
fn drag_arrow(
    doc: &solidcraft_engine::doc::Document,
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    proj: &Proj,
    base: Vec3,
    bs: Pos2,
    n: Vec3,
    l: f64,
    signed: bool,
    id: egui::Id,
    half_height: f64,
    clear_px: f32,
    value: &mut String,
) -> Option<Pos2> {
    let t = Tokens::get();
    let min = f64::from(40.0 + clear_px) * 2.0 * half_height / f64::from(proj.rect.height().max(1.0));
    let tip = |length: f64| {
        let shown = if length.abs() < min { min * if length < 0.0 { -1.0 } else { 1.0 } } else { length };
        proj.to_screen(base + n * shown)
    };
    let mut ts = tip(l)?;
    // Where the drawn arrow starts: the base, or past the cleared marker.
    let start = |ts: Pos2| if clear_px > 0.0 && (ts - bs).length() > clear_px { bs + (ts - bs).normalized() * clear_px } else { bs };
    let at_tip = egui::Rect::from_center_size(ts, vec2(20.0, 20.0));
    // Over the stem, the arrow answers where the pointer is. A placement handle the arrow crosses
    // (the turn ring's grip) keeps the pointer: the arrow then answers nowhere.
    let hover = ui.input(|i| i.pointer.hover_pos());
    let on_handle = hover.is_some_and(crate::place_gizmo::drawn_at);
    let on_stem = hover.filter(|p| !on_handle && !at_tip.contains(*p) && to_segment(*p, start(ts), ts) <= ARROW_STEM_REACH);
    let area = if on_handle {
        egui::Rect::NOTHING
    } else {
        on_stem.map_or(at_tip, |p| egui::Rect::from_center_size(p, vec2(2.0 * ARROW_STEM_REACH, 2.0 * ARROW_STEM_REACH)))
    };
    let r = ui.interact(area, id, egui::Sense::drag());
    // How far along the arrow the press was short of its end: the drag keeps that gap.
    let grab = id.with("stem grab");
    if r.drag_started() {
        let gap = on_stem.zip(r.interact_pointer_pos()).and_then(|(_, p)| along(proj, p, base, n)).map(|v| v - l);
        ui.data_mut(|d| {
            if let Some(g) = gap.filter(|g| g.is_finite()) {
                d.insert_temp(grab, g);
            } else {
                d.remove::<f64>(grab);
            }
        });
    }
    if r.dragged()
        && let Some(p) = r.interact_pointer_pos()
        && let Some(v) = along(proj, p, base, n)
    {
        crate::tool_modifiers::set_manipulator_dragging(true);
        let v = v - ui.data(|d| d.get_temp::<f64>(grab)).unwrap_or(0.0);
        let free = crate::tool_modifiers::manipulator_free();
        let step = snap_step(half_height);
        let mut v = if free { v } else { (v / step).round() * step };
        if !signed {
            v = v.max(if free { 1e-3 } else { step });
        }
        *value = format_len(doc, v);
        ts = tip(v)?;
    }
    // Paint the value just read from this frame's pointer, including the value box anchor.
    // Using the old distance here made both trail the drag by another frame.
    crate::scenario::publish_handle(if id == egui::Id::new("sc_manipulator") { "arrow" } else { "arrow2" }, ts);
    let hot = r.hovered() || r.dragged();
    let col = if hot { t.accent } else { t.manipulator };
    arrow(painter, start(ts), ts, col);
    if hot {
        painter.circle_stroke(ts, 8.0, Stroke::new(1.5, col));
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    Some(ts)
}

/// A fillet's radius spline on each of its edges (a spline fillet, or the splines a taper gave
/// them): the radius curve drawn into the body from the edge, and a handle per control radius at
/// its Greville position, that far from the edge. Dragging a handle sets its control radius
/// (snapped unless Alt/Ctrl); double-clicking the curve adds a control radius there without
/// changing it, double-clicking a handle between the ends removes it. A spline fillet's edges
/// share one spline, so every edge's handles edit it. A taper's handles show on a single edge
/// only: the first edit makes the fillet a spline fillet starting from that edge's taper, which
/// would put the same radius on every other edge. Whether the handles are shown.
fn radius_spline_handles(app: &SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog) -> bool {
    use serde_json::{Value, json};
    use solidcraft_engine::doc::kernel::RadiusSpline;
    enum Edit {
        Set(usize, f64),
        Insert(f64),
        Remove(usize),
    }
    let t = Tokens::get();
    let Kind::Fillet { chamfer: false, .. } = d.kind else { return false };
    let st = app.session.world_state();
    let edges: Vec<(std::sync::Arc<Mesh>, usize, Vec3, Vec3)> = d
        .inputs
        .first()
        .into_iter()
        .flat_map(|i| &i.items)
        .filter_map(|x| match x {
            Sel::Edge { body, index, .. } => {
                let m = st.body(body)?.mesh();
                let (a, b) = m.edges.get(*index).and_then(|e| Some((*e.first()?, *e.last()?)))?;
                Some((m, *index, a, b))
            }
            _ => None,
        })
        .collect();
    let Some(&(_, _, a0, b0)) = edges.first() else { return false };
    let eval = |e: &str| crate::length_input::eval(&app.session.doc, e, ValueKind::Length).ok().filter(|v| v.is_finite());
    let style = d.extra.get("type").and_then(Value::as_str).unwrap_or("constant").to_string();
    // Each edge's spline with the end its positions start from; the spline the handles edit,
    // whether it starts at the first edge's far end, and whether it is still the taper's.
    let (laws, base, swap, taper): (Vec<Option<(RadiusSpline, Vec3)>>, Option<RadiusSpline>, bool, bool) = if style == "spline" {
        let Kind::Fillet { radius, .. } = &d.kind else { return false };
        let knots: Vec<f64> = d.extra.get("knots").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_f64).collect();
        let exprs: Vec<String> =
            d.extra.get("radii").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        let Some(radii) = std::iter::once(radius.as_str()).chain(exprs.iter().map(String::as_str)).map(eval).collect::<Option<Vec<f64>>>() else {
            return false;
        };
        let spline = solidcraft_engine::doc::FilletStyle::spline(&knots, radii);
        let swap = d.extra.get("swap_ends").and_then(Value::as_bool).unwrap_or(false);
        let start = if swap { b0 } else { a0 };
        // Each edge runs from its end nearest the start, as the fillet builds it.
        let laws = edges.iter().map(|(_, _, a, b)| Some((spline.clone(), if start.dist(*a) <= start.dist(*b) { *a } else { *b }))).collect();
        (laws, Some(spline), swap, false)
    } else if style == "constant" && d.extra.get("fit").and_then(Value::as_str) == Some("taper") {
        // The taper of an edge: its point lies on the edge's chord. An edge that fits has none.
        let on_edge = |a: Vec3, b: Vec3, p: Vec3| {
            let ab = b - a;
            let s = ((p - a).dot(ab) / ab.dot(ab).max(1e-300)).clamp(0.0, 1.0);
            (a + ab * s).dist(p) <= 1e-6 * (1.0 + ab.len())
        };
        let laws: Vec<Option<(RadiusSpline, Vec3)>> = edges
            .iter()
            .map(|(_, _, a, b)| app.preview.tapered.iter().find(|(p, _)| on_edge(*a, *b, *p)).map(|(_, tp)| (tp.spline.clone(), tp.start)))
            .collect();
        let base = laws.first().cloned().flatten().filter(|_| edges.len() == 1);
        let swap = base.as_ref().is_some_and(|(_, s)| s.dist(b0) < s.dist(a0));
        (laws, base.map(|(l, _)| l), swap, true)
    } else {
        return false;
    };
    // Each drawn edge: its spline, the ends its positions run between, the direction into the
    // body, and the curve on screen with its positions.
    struct Drawn<'a> {
        spline: &'a RadiusSpline,
        start: Vec3,
        other: Vec3,
        n: Vec3,
        curve: Vec<(f64, Pos2)>,
    }
    let drawn: Vec<Drawn> = edges
        .iter()
        .zip(&laws)
        .filter_map(|((mesh, index, a, b), law)| {
            let (spline, start) = law.as_ref()?;
            let other = if start.dist(*a) <= start.dist(*b) { *b } else { *a };
            let mid = *start + (other - *start) * 0.5;
            // Between the two faces, the way this edge's radius grows: into the body on an outside
            // edge, into the corner on an inside one (the edge is straight between planar faces).
            let n = blend_grow_dir(mesh, *index, mid)?;
            let curve = (0..=48)
                .filter_map(|k| {
                    let u = f64::from(k) / 48.0;
                    Some((u, proj.to_screen(*start + (other - *start) * u + n * spline.eval(u))?))
                })
                .collect();
            Some(Drawn { spline, start: *start, other, n, curve })
        })
        .collect();
    for e in &drawn {
        painter.add(egui::Shape::line(e.curve.iter().map(|c| c.1).collect(), Stroke::new(1.5, t.manipulator)));
    }
    let Some(base) = base.filter(|_| !drawn.is_empty()) else { return false };
    let mut edit = None;
    // Double-clicking the curve adds a control radius at the nearest position along it. Under
    // the handles, which are registered after it and so take the pointer first.
    let near = ui.input(|i| i.pointer.hover_pos()).and_then(|p| {
        drawn
            .iter()
            .flat_map(|e| e.curve.windows(2))
            .map(|w| {
                let ((u0, p0), (u1, p1)) = (w[0], w[1]);
                let seg = p1 - p0;
                let s = ((p - p0).dot(seg) / seg.length_sq().max(1e-6)).clamp(0.0, 1.0);
                (p0.lerp(p1, s).distance(p), u0 + (u1 - u0) * f64::from(s))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .filter(|(dist, _)| *dist < 6.0)
            .map(|(_, u)| (p, u))
    });
    if let Some((p, u)) = near {
        crate::scenario::publish_handle("fillet_curve", p);
        if ui.interact(egui::Rect::from_center_size(p, vec2(12.0, 12.0)), egui::Id::new("sc_fillet_curve"), egui::Sense::click()).double_clicked() {
            edit = Some(Edit::Insert(u));
        }
    }
    let half_height = app.cam.half_height();
    let last = base.radii.len().saturating_sub(1);
    for (i, e) in drawn.iter().enumerate() {
        let at = |u: f64| e.start + (e.other - e.start) * u;
        let g = e.spline.greville();
        let tips: Vec<Option<Pos2>> = g.iter().zip(&e.spline.radii).map(|(u, r)| proj.to_screen(at(*u) + e.n * *r)).collect();
        painter.add(egui::Shape::dashed_line(&tips.iter().flatten().copied().collect::<Vec<_>>(), Stroke::new(1.0, t.manipulator), 4.0, 3.0));
        for (k, (u, ts)) in g.iter().zip(&tips).enumerate() {
            let Some(ts) = *ts else { continue };
            crate::scenario::publish_handle(&if i == 0 { format!("fillet_r{k}") } else { format!("fillet_e{i}_r{k}") }, ts);
            let r =
                ui.interact(egui::Rect::from_center_size(ts, vec2(16.0, 16.0)), egui::Id::new(("sc_fillet_r", i, k)), egui::Sense::click_and_drag());
            let hot = r.hovered() || r.dragged();
            let col = if hot { t.accent } else { t.manipulator };
            if let Some(fs) = proj.to_screen(at(*u)) {
                painter.line_segment([fs, ts], Stroke::new(1.0, col));
            }
            painter.circle(ts, if hot { 6.5 } else { 5.0 }, t.overlay, Stroke::new(2.0, col));
            if hot {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if r.double_clicked() {
                edit = Some(Edit::Remove(k));
            } else if r.dragged()
                && let Some(p) = r.interact_pointer_pos()
                && let Some(v) = along(proj, p, at(*u), e.n)
            {
                crate::tool_modifiers::set_manipulator_dragging(true);
                let free = crate::tool_modifiers::manipulator_free();
                let step = snap_step(half_height);
                let v = if free { v } else { (v / step).round() * step };
                // Zero only at an end, where the round closes to a point.
                let floor = if k == 0 || k == last {
                    0.0
                } else if free {
                    1e-3
                } else {
                    step
                };
                edit = Some(Edit::Set(k, v.max(floor)));
            }
        }
    }
    // A dragged radius on a spline fillet changes its own expression; anything else writes the
    // whole spline (inserting or removing a knot moves its neighbours too).
    let whole = match edit {
        Some(Edit::Set(k, v)) if !taper => {
            if k == 0 {
                if let Kind::Fillet { radius, .. } = &mut d.kind {
                    *radius = format_len(&app.session.doc, v);
                }
            } else if let Some(Value::Array(rs)) = d.extra.get_mut("radii")
                && let Some(slot) = rs.get_mut(k - 1)
            {
                *slot = json!(format_len(&app.session.doc, v));
            }
            None
        }
        Some(Edit::Set(k, v)) => {
            let mut law = base.clone();
            if let Some(r) = law.radii.get_mut(k) {
                *r = v;
            }
            Some(law)
        }
        Some(Edit::Insert(u)) => base.insert(u),
        Some(Edit::Remove(k)) => base.remove(k),
        None => None,
    };
    if let Some(law) = whole {
        d.extra.insert("type".into(), json!("spline"));
        d.extra.remove("fit");
        d.extra.insert("knots".into(), json!(law.knots));
        d.extra.insert("swap_ends".into(), json!(swap));
        d.extra.insert("radii".into(), json!(law.radii.iter().skip(1).map(|r| format_len(&app.session.doc, *r)).collect::<Vec<_>>()));
        if let Kind::Fillet { radius, .. } = &mut d.kind {
            *radius = format_len(&app.session.doc, law.radii.first().copied().unwrap_or(0.0));
        }
    }
    true
}

/// The measured distance: a line between the ends of the shown measure (the centres for
/// centre to centre, the closest or farthest points) with its length.
fn measure_line(app: &SolveApp, painter: &egui::Painter, proj: &Proj, r: &serde_json::Value, show: Option<crate::dialogs::Shown>) {
    let t = Tokens::get();
    let Some((_, d, a, b)) = crate::dialogs::shown_distance(r, show) else { return };
    if !a.is_finite() || !b.is_finite() || !d.is_finite() {
        return;
    }
    let (Some(sa), Some(sb)) = (proj.to_screen(a), proj.to_screen(b)) else { return };
    if !sa.is_finite() || !sb.is_finite() {
        return;
    }
    painter.line_segment([sa, sb], Stroke::new(2.0, t.manipulator));
    for s in [sa, sb] {
        painter.circle(s, 3.5, t.manipulator, Stroke::new(1.0, Color32::WHITE));
    }
    let mid = sa + (sb - sa) * 0.5 + vec2(10.0, -14.0);
    let galley = painter.layout_no_wrap(crate::prefs::show_mm(app, d, 1), egui::FontId::proportional(crate::theme::Tokens::get().font_label), t.text);
    let rect = egui::Rect::from_min_size(mid, galley.size() + vec2(10.0, 6.0));
    crate::scenario::publish_target("measure:label", rect);
    if app.preferences.measure_axis_distances {
        measure_components(app, painter, proj, a, b, rect);
    }
    painter.rect(rect, t.radius_sm.saturating_sub(1), t.overlay, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(5.0, 3.0), galley, t.text);
}

/// A world-axis-aligned chain between the same two measured points. Zero-length and
/// view-aligned legs still get a label; labels move apart when their projections coincide.
fn measure_components(app: &SolveApp, painter: &egui::Painter, proj: &Proj, a: Vec3, b: Vec3, total: egui::Rect) {
    let t = Tokens::get();
    let x = Vec3::new(b.x, a.y, a.z);
    let y = Vec3::new(b.x, b.y, a.z);
    let mut occupied = vec![total];
    for (axis, start, end, color) in [("x", a, x, t.axis_x), ("y", x, y, t.axis_y), ("z", y, b, t.axis_z)] {
        let color = color.lerp_to_gamma(t.text, 0.25);
        let distance = start.dist(end);
        if !distance.is_finite() {
            continue;
        }
        let screen =
            proj.to_screen(start).zip(proj.to_screen(end)).filter(|(sa, sb)| sa.is_finite() && sb.is_finite() && (*sb - *sa).length_sq().is_finite());
        let (at, offset) = if let Some((sa, sb)) = screen {
            let leg = sb - sa;
            if leg.length_sq() > 1.0 {
                painter.add(egui::Shape::dashed_line(&[sa, sb], Stroke::new(1.5, color), 5.0, 3.0));
                let normal = vec2(-leg.y, leg.x).normalized();
                for p in [sa, sb] {
                    painter.line_segment([p - normal * 3.0, p + normal * 3.0], Stroke::new(1.0, color));
                }
                (sa + leg * 0.5, normal * 20.0)
            } else {
                (sa, vec2(20.0, 20.0))
            }
        } else {
            (total.center(), vec2(20.0, 20.0))
        };
        let text = format!("{}: {}", axis.to_uppercase(), crate::prefs::show_mm(app, distance, 1));
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(crate::theme::Tokens::get().font_label), t.text);
        let size = galley.size() + vec2(10.0, 6.0);
        let bounds = proj.rect.shrink(4.0);
        let clamp = |rect: egui::Rect| {
            egui::Rect::from_min_size(
                egui::pos2(
                    rect.left().clamp(bounds.left(), (bounds.right() - size.x).max(bounds.left())),
                    rect.top().clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top())),
                ),
                size,
            )
        };
        let preferred = clamp(egui::Rect::from_center_size(at + offset, size));
        let mut rect = preferred;
        // Four labels at most. Search both sides of the anchor, including at viewport edges.
        'place: for row in 0..12 {
            for side in [1.0, -1.0] {
                let candidate = clamp(preferred.translate(vec2(0.0, side * row as f32 * (size.y + 4.0))));
                if occupied.iter().all(|other| !other.expand(3.0).intersects(candidate)) {
                    rect = candidate;
                    break 'place;
                }
            }
        }
        painter.line_segment([at, rect.center()], Stroke::new(0.8, color));
        painter.rect(rect, t.radius_sm.saturating_sub(1), t.overlay, Stroke::new(1.0, color), egui::StrokeKind::Inside);
        painter.galley(rect.min + vec2(5.0, 3.0), galley, t.text);
        let handle = format!("measure:label:{axis}");
        crate::scenario::publish_target(&handle, rect);
        occupied.push(rect);
    }
}

/// The tapers (degrees, least and most) a drag of the taper handle may reach for `regions`
/// extruded `height` (#1602): going in, the sides close the material where they have moved its
/// inradius; going out, they close the smallest hole. Each stops just short, so the drag ends at
/// the largest taper that builds instead of one that fails. A typed angle is not limited.
pub(crate) fn taper_limits(regions: &[&solidcraft_engine::geom::Region2], height: f64) -> (f64, f64) {
    const MAX: f64 = 89.0;
    if !(height > 1e-9) || regions.is_empty() {
        return (-MAX, MAX);
    }
    let at = |r: f64| (0.99 * r / height).atan().to_degrees().min(MAX);
    let inward = regions.iter().map(|r| r.inradius(1e-3)).fold(f64::INFINITY, f64::min);
    let outward = regions.iter().flat_map(|r| &r.holes).map(|h| h.inradius(1e-3)).fold(f64::INFINITY, f64::min);
    (if inward.is_finite() { -at(inward) } else { -MAX }, if outward.is_finite() { at(outward) } else { MAX })
}

/// The taper handle: a line from `foot` along the extrude (`span`) leaning by `angle` towards
/// `e`, with a ring at its end; dragging the ring sideways sets the angle (1° steps, Alt/Ctrl:
/// free), within `limits` ([`taper_limits`]).
#[allow(clippy::too_many_arguments)]
fn taper_handle(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    proj: &Proj,
    foot: Vec3,
    span: Vec3,
    e: Vec3,
    angle: f64,
    limits: (f64, f64),
    value: &mut String,
) {
    let t = Tokens::get();
    let l = span.len();
    let end = foot + span + e * (l * angle.tan().clamp(-10.0, 10.0));
    let (Some(fs), Some(es)) = (proj.to_screen(foot), proj.to_screen(end)) else { return };
    let r = ui.interact(egui::Rect::from_center_size(es, vec2(16.0, 16.0)), egui::Id::new("sc_taper"), egui::Sense::drag());
    crate::scenario::publish_handle("taper", es);
    let hot = r.hovered() || r.dragged();
    let col = if hot { t.accent } else { t.manipulator };
    painter.add(egui::Shape::dashed_line(&[fs, es], Stroke::new(1.2, col), 5.0, 3.0));
    painter.circle(es, if hot { 6.5 } else { 5.0 }, t.overlay, Stroke::new(2.0, col));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if r.dragged()
        && let Some(p) = r.interact_pointer_pos()
        && let Some(s) = along(proj, p, foot + span, e)
    {
        crate::tool_modifiers::set_manipulator_dragging(true);
        let mut deg = s.atan2(l).to_degrees();
        let (lo, hi) = limits;
        if crate::tool_modifiers::manipulator_free() {
            deg = deg.clamp(lo, hi);
        } else {
            // Whole degrees, rounded towards zero at a limit so they stay inside it.
            deg = deg.round().clamp(lo.ceil().min(0.0), hi.floor().max(0.0));
        }
        let txt = format!("{deg:.1}");
        *value = format!("{} deg", txt.trim_end_matches('0').trim_end_matches('.'));
    }
}

/// The revolve axis (a point on it and its unit direction).
pub(crate) fn revolve_axis(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    if !matches!(d.kind, Kind::Revolve { .. }) {
        return None;
    }
    if let Some(selected) = d.inputs.get(1)?.items.first() {
        if let Sel::Axis { name } = selected
            && matches!(name.as_str(), "X" | "Y" | "Z")
            && let Sel::Profile { sketch, .. } = d.inputs.first()?.items.first()?
        {
            let component = app.session.doc.feature(*sketch)?.component;
            let frame = crate::dialogs::feature_frame(&app.session, component);
            let dir = match name.as_str() {
                "X" => Vec3::X,
                "Y" => Vec3::Y,
                _ => Vec3::Z,
            };
            return Some((solidcraft_engine::doc::apply_point(&frame, Vec3::ZERO), solidcraft_engine::doc::apply_vector(&frame, dir)));
        }
        if let Sel::Axis { name } = selected
            && matches!(name.as_str(), "x" | "y")
            && let Sel::Profile { sketch, .. } = d.inputs.first()?.items.first()?
        {
            let plane = profile_plane(app, *sketch)?;
            return Some((plane.origin, if name == "x" { plane.x } else { plane.y }));
        }
        return crate::dialogs::axis_of(app, selected);
    }
    let Kind::Revolve { axis: Some(axis), .. } = &d.kind else { return None };
    let vector = |key: &str| -> Option<Vec3> {
        let p = axis.get(key)?.as_array()?;
        Some(Vec3::new(p.first()?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?))
    };
    Some((vector("origin")?, vector("dir")?.normalized()?))
}

/// Draw angular extents in the same clockwise sense as the modeled solid.
#[allow(clippy::too_many_arguments)]
fn draw_revolve(
    app: &SolveApp,
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    proj: &Proj,
    d: &mut Dialog,
    base: Vec3,
    bs: Pos2,
    axis: (Vec3, Vec3),
    focus: bool,
) {
    let to_mode = d.inputs.iter().any(|input| input.label == "To Object");
    let extent = revolve_display_extent(app, d, axis).unwrap_or(solidcraft_engine::doc::RevolveExtent {
        start: 0.0,
        sweep: f64::NAN,
        rotation_axis: axis.1,
        display_start: 0.0,
        curved_to: false,
    });
    let mut box_at = bs + vec2(16.0, -30.0);
    let angle_before = if let Kind::Revolve { angle, .. } = &d.kind { angle.clone() } else { String::new() };
    if !extent.curved_to && extent.sweep.is_finite() {
        let (origin, direction) = axis;
        let centre = origin + direction * (base - origin).dot(direction);
        let radius = (base - centre).len();
        if radius > 1e-9 {
            let e1 = (base - centre) / radius;
            let e2 = extent.rotation_axis.cross(e1);
            let handed = match revolve_convention(d) {
                solidcraft_engine::doc::RevolveConvention::LegacyAxisStart => -direction.dot(extent.rotation_axis),
                solidcraft_engine::doc::RevolveConvention::SketchNormal => 1.0,
            };
            let at = |angle: f64| centre + (e1 * angle.cos() + e2 * angle.sin()) * radius;
            let points: Vec<_> = (0..=48).filter_map(|i| proj.to_screen(at(extent.display_start + extent.sweep * i as f64 / 48.0))).collect();
            painter.add(egui::Shape::line(points, Stroke::new(2.0, Tokens::get().manipulator)));
            if let Some(end) = proj.to_screen(at(extent.display_start + extent.sweep)) {
                box_at = end + vec2(14.0, -12.0);
            }
            if !to_mode {
                let symmetric = !d.extra.contains_key("angle2") && d.extra.get("direction").and_then(serde_json::Value::as_str) == Some("symmetric");
                let other_angle = d
                    .extra
                    .get("angle2")
                    .and_then(|value| value.as_str())
                    .and_then(|value| crate::length_input::eval(&app.session.doc, value, ValueKind::Angle).ok())
                    .unwrap_or(0.0);
                let range = if extent.sweep < 0.0 { (-std::f64::consts::TAU, 0.0) } else { (0.0, std::f64::consts::TAU) };
                if let Kind::Revolve { angle, .. } = &mut d.kind {
                    let boundary = extent.display_start + extent.sweep;
                    if let Some(end) = rotation_handle(
                        ui,
                        painter,
                        proj,
                        centre,
                        e1,
                        e2,
                        direction,
                        radius,
                        boundary,
                        if symmetric { 1.0 / (1.0 - handed * 0.5) } else { 1.0 },
                        if symmetric { 0.0 } else { -other_angle * (1.0 - handed) },
                        (range.0 - other_angle, range.1 - other_angle),
                        "revolve",
                        angle,
                    ) {
                        box_at = end + vec2(14.0, -12.0);
                    }
                    if symmetric {
                        rotation_handle(
                            ui,
                            painter,
                            proj,
                            centre,
                            e1,
                            e2,
                            direction,
                            radius,
                            extent.display_start,
                            -2.0 * handed,
                            0.0,
                            range,
                            "revolve2",
                            angle,
                        );
                    }
                }
                if let Some(other) = d.extra.get_mut("angle2") {
                    let mut value = other.as_str().map(str::to_owned).unwrap_or_else(|| other.to_string());
                    let before = value.clone();
                    let first_angle = extent.sweep - other_angle;
                    rotation_handle(
                        ui,
                        painter,
                        proj,
                        centre,
                        e1,
                        e2,
                        direction,
                        radius,
                        extent.display_start,
                        -handed,
                        0.0,
                        (range.0 - first_angle, range.1 - first_angle),
                        "revolve2",
                        &mut value,
                    );
                    if value != before {
                        *other = serde_json::Value::String(value);
                    }
                }
            }
        }
    }
    if to_mode {
        // This badge is read-only. Its placement follows the model and can overlap the
        // command dialog, whose buttons must keep receiving clicks.
        let area = Container::new(ui.ctx(), Surface::Canvas)
            .area(egui::Id::new("sc_canvas_area"))
            .fixed_pos(box_at)
            .order(egui::Order::Foreground)
            .interactable(false);
        area.show(ui.ctx(), |ui| {
            Container::popup_frame(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                let text = if extent.curved_to {
                    "To surface".into()
                } else if extent.sweep.is_finite() {
                    format!("Sweep {:.1}°", extent.sweep.to_degrees())
                } else {
                    "Choose target".into()
                };
                ui.label(text).on_hover_text(crate::i18n::tr_label("Determined by To Object. Choose Angle to edit the angular extent."));
            });
        });
    } else if let Kind::Revolve { angle, .. } = &mut d.kind {
        let dragging = *angle != angle_before;
        value_box(ui, crate::i18n::tr_label("Angle"), angle, box_at, focus || dragging, box_place(app, proj));
    }
}

fn revolve_convention(d: &Dialog) -> solidcraft_engine::doc::RevolveConvention {
    d.extra
        .get("convention")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or(solidcraft_engine::doc::RevolveConvention::SketchNormal)
}

fn revolve_display_extent(app: &SolveApp, d: &Dialog, axis: (Vec3, Vec3)) -> Option<solidcraft_engine::doc::RevolveExtent> {
    let Sel::Profile { sketch, .. } = d.inputs.first()?.items.first()? else { return None };
    let state = app.session.world_state();
    let ss = state.sketch(*sketch)?;
    let regions: Vec<_> = d
        .inputs
        .first()?
        .items
        .iter()
        .filter_map(|item| match item {
            Sel::Profile { sketch: id, index } if id == sketch => ss.profiles.get(*index).map(|profile| profile.region.clone()),
            _ => None,
        })
        .collect();
    let regions = solidcraft_engine::sketch::merge_regions(&regions);
    let plane = profile_plane(app, *sketch)?;
    let to_mode = d.inputs.iter().any(|input| input.label == "To Object");
    let to = if to_mode {
        let point = crate::dialogs::object_point(&d.inputs, "To Object")?;
        Some(Vec3::new(point.get(0)?.as_f64()?, point.get(1)?.as_f64()?, point.get(2)?.as_f64()?))
    } else {
        None
    };
    let Kind::Revolve { angle, .. } = &d.kind else { return None };
    let angles = if to_mode {
        (0.0, None)
    } else {
        let angle = crate::length_input::eval(&app.session.doc, angle, ValueKind::Angle).ok()?;
        let other = d
            .extra
            .get("angle2")
            .map(|value| crate::length_input::eval(&app.session.doc, value.as_str().unwrap_or_default(), ValueKind::Angle))
            .transpose()
            .ok()?;
        (angle, other)
    };
    solidcraft_engine::doc::revolve_extent_with_convention(
        &state,
        &plane,
        &regions,
        axis,
        angles,
        d.extra.get("direction").and_then(serde_json::Value::as_str) == Some("symmetric"),
        to,
        revolve_convention(d),
    )
    .ok()
}

#[allow(clippy::too_many_arguments)]
fn rotation_handle(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    proj: &Proj,
    centre: Vec3,
    e1: Vec3,
    e2: Vec3,
    axis: Vec3,
    radius: f64,
    boundary: f64,
    scale: f64,
    offset: f64,
    range: (f64, f64),
    name: &str,
    value: &mut String,
) -> Option<Pos2> {
    let end = proj.to_screen(centre + (e1 * boundary.cos() + e2 * boundary.sin()) * radius)?;
    crate::scenario::publish_handle(name, end);
    let response = ui.interact(egui::Rect::from_center_size(end, vec2(20.0, 20.0)), egui::Id::new(("sc_rotator", name)), egui::Sense::drag());
    let hot = response.hovered() || response.dragged();
    let color = if hot { Tokens::get().accent } else { Tokens::get().manipulator };
    painter.circle(end, if hot { 7.0 } else { 5.5 }, color, Stroke::new(1.5, Color32::WHITE));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let (origin, direction) = proj.ray(pointer);
        let den = direction.dot(axis);
        let (a, b) = ((range.0 - offset) / scale, (range.1 - offset) / scale);
        let bounds = (a.min(b), a.max(b));
        // Near edge-on, a plane intersection magnifies subpixel pointer error.
        let raw = if den.abs() > 0.05 {
            let point = origin + direction * ((centre - origin).dot(axis) / den) - centre;
            Some(point.dot(e2).atan2(point.dot(e1)))
        } else {
            projected_rotation_angle(proj, pointer, centre, e1, e2, radius, boundary, bounds)
        };
        if let Some(raw) = raw {
            let boundary_angle = (-4..=4)
                .map(|k| raw + k as f64 * std::f64::consts::TAU)
                .filter(|angle| *angle >= bounds.0 - 1e-4 && *angle <= bounds.1 + 1e-4)
                .min_by(|a, b| (a - boundary).abs().total_cmp(&(b - boundary).abs()))
                .unwrap_or_else(|| raw.clamp(bounds.0, bounds.1));
            let mut angle = (boundary_angle.clamp(bounds.0, bounds.1) * scale + offset).to_degrees();
            if !crate::tool_modifiers::manipulator_free() {
                angle = (angle / 5.0).round() * 5.0;
            }
            if angle.abs() < 0.05 {
                angle = 0.0;
            }
            let text = format!("{angle:.1}");
            *value = format!("{} deg", text.trim_end_matches('0').trim_end_matches('.'));
        }
    }
    Some(end)
}

/// An edge-on ring has two equally valid depths; retain the branch nearest the current handle.
#[allow(clippy::too_many_arguments)]
pub(crate) fn projected_rotation_angle(
    proj: &Proj,
    pointer: Pos2,
    centre: Vec3,
    e1: Vec3,
    e2: Vec3,
    radius: f64,
    current: f64,
    (lo, hi): (f64, f64),
) -> Option<f64> {
    let distance = |angle: f64| {
        proj.to_screen(centre + (e1 * angle.cos() + e2 * angle.sin()) * radius)
            .map(|point| f64::from(point.distance_sq(pointer)))
            .unwrap_or(f64::INFINITY)
    };
    let step = (hi - lo) / 720.0;
    let mut best = None;
    for i in 0..=720 {
        let angle = lo + step * i as f64;
        let score = distance(angle);
        if !score.is_finite() {
            continue;
        }
        if best.is_none_or(|(old_angle, old_score): (f64, f64)| {
            score < old_score - 1e-4 || (score - old_score).abs() <= 1e-4 && (angle - current).abs() < (old_angle - current).abs()
        }) {
            best = Some((angle, score));
        }
    }
    let (angle, _) = best?;
    let (mut a, mut b) = ((angle - step).max(lo), (angle + step).min(hi));
    for _ in 0..24 {
        let (left, right) = (a + (b - a) / 3.0, b - (b - a) / 3.0);
        if distance(left) <= distance(right) {
            b = right;
        } else {
            a = left;
        }
    }
    Some((a + b) / 2.0)
}
fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, col: Color32) {
    let v = b - a;
    let l = v.length();
    if l < 1.0 {
        return;
    }
    let u = v / l;
    let n = vec2(-u.y, u.x);
    painter.line_segment([a, b], Stroke::new(2.0, col));
    let head = 11.0f32.min(l * 0.6);
    painter.add(egui::Shape::convex_polygon(vec![b, b - u * head + n * head * 0.45, b - u * head - n * head * 0.45], col, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_plane_preview_stays_within_its_square() {
        let rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let mut worst = f32::NEG_INFINITY;
        for n in [Vec3::X, Vec3::Y, Vec3::Z] {
            for dir in crate::viewport::orbit_directions() {
                let cam = solidcraft_engine::render::Camera::default().looking_from(dir);
                let proj = Proj { cam, vp: cam.view_proj(4.0 / 3.0, 100.0), rect };
                let h = cam.half_height() * 0.25;
                let (Some(q), Some(c), Some(side)) =
                    (offset_plane_quad(&proj, Vec3::ZERO, n, h), proj.to_screen(Vec3::ZERO), proj.to_screen(cam.basis().0 * h))
                else {
                    continue;
                };
                // The square's half diagonal on screen (an orthographic view), plus stroke and feathering.
                let limit = c.distance(side) * 2f32.sqrt() + 3.0;
                for p in crate::viewport::drawn_points(projected_polygon(&q, Color32::GRAY, Stroke::new(1.2, Color32::BLACK))) {
                    worst = worst.max(p.distance(c) - limit);
                }
            }
        }
        assert!(worst <= 0.0, "a vertex {worst} px past the square");
    }

    #[test]
    fn snapping_steps_and_formatting() {
        assert_eq!(snap_step(40.0), 1.0);
        assert_eq!(snap_step(100.0), 2.0);
        assert_eq!(snap_step(4.0), 0.1);
        assert_eq!(format_len(&solidcraft_engine::doc::Document::default(), 12.0), "12 mm");
        assert_eq!(format_len(&solidcraft_engine::doc::Document::default(), -2.5), "-2.5 mm");
        assert_eq!(format_len(&solidcraft_engine::doc::Document::default(), -0.0001), "-0.0001 mm");
    }

    /// #6608: a canvas value box never covers the command window: it moves out of it.
    #[test]
    fn a_value_box_keeps_clear_of_the_command_window() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", serde_json::json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.step(&serde_json::json!({"start": "solid.move"})).unwrap();
        h.frames(6);
        let box_rect = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_canvas_area"))).expect("the value box");
        let window = h.app.viewport.dialog_rect.expect("the command window");
        // Drag the command window over the value box: the box must move out of it.
        let title = crate::scenario::handle_at("dialog:title").expect("the window's title");
        let dx = box_rect.center().x - 20.0 - window.left();
        h.call("ui.drag", serde_json::json!({"x0": title.x, "y0": title.y, "x1": title.x + dx, "y1": title.y, "steps": 8}));
        h.frames(6);
        let window = h.app.viewport.dialog_rect.unwrap();
        assert!(window.contains(box_rect.center()), "the window {window:?} did not reach the value box {box_rect:?}");
        let kept = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_canvas_area"))).expect("the value box");
        assert!(!kept.intersects(window), "the value box {kept:?} covers the command window {window:?}");
    }

    /// #6608: a value box that has to move out of the command window does not cover the
    /// manipulator's handles: the box is a foreground area, so it took the press and the drag
    /// on a ring never reached the model.
    #[test]
    fn a_value_box_keeps_clear_of_the_handles() {
        let mut h = crate::scenario::Harness::new();
        h.run(&serde_json::json!([
            {"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 20, "width": 30, "height": 10}}},
            {"call": "ui.view", "params": {"view": "iso"}},
            {"start": "solid.move"},
            {"click": {"world": [10, 15, 10]}},
            {"key": "Enter"},
            {"start": "solid.move"},
            {"click": {"world": [10, 15, 10]}},
            {"drag": [{"gizmo": "x"}, {"gizmo": "x", "by": [20, 0, 0]}]},
            {"key": "Enter"},
            {"start": "solid.move"},
            {"click": {"world": [30, 15, 10]}}
        ]))
        .unwrap();
        let box_rect = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_canvas_area"))).expect("the value box");
        let handles = crate::scenario::handles("move_");
        assert!(!handles.is_empty(), "the triad drew no handles");
        for (name, p) in &handles {
            // A press takes a handle a few px off its line (the ball's is the widest).
            assert!(!box_rect.expand(crate::gizmo::HIT_TOLERANCE).contains(*p), "the value box {box_rect:?} covers the handle {name} at {p:?}");
        }
    }

    /// A box that lands on the command window moves out of it, whole, and a box that misses it
    /// stays where it is (#6608). A box that lands on the handles moves off them too, and one
    /// that cannot clear both keeps off the handles.
    #[test]
    fn a_value_box_moves_out_of_the_command_window() {
        let dialog = egui::Rect::from_min_size(egui::pos2(700.0, 200.0), egui::vec2(280.0, 300.0));
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let on_the_window = egui::Rect::from_min_size(egui::pos2(800.0, 350.0), egui::vec2(140.0, 24.0));
        let kept = clear_of_dialog(on_the_window, BoxPlace { dialog: Some(dialog), view: Some(view), handles: None });
        assert!(!kept.intersects(dialog), "the box {kept:?} still covers the window {dialog:?}");
        assert_eq!(kept.size(), on_the_window.size(), "the box keeps its size");
        let clear = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(140.0, 24.0));
        let place = BoxPlace { dialog: Some(dialog), view: Some(view), handles: None };
        assert_eq!(clear_of_dialog(clear, place), clear, "a box that misses the window stays");
    }

    /// A box that would cover a handle moves off it, and one that cannot clear both the window
    /// and the handles keeps off the handles: the handle takes the press (#6608).
    #[test]
    fn a_value_box_moves_out_of_the_handles() {
        let dialog = egui::Rect::from_min_size(egui::pos2(700.0, 200.0), egui::vec2(280.0, 300.0));
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let handles = egui::Rect::from_min_size(egui::pos2(500.0, 300.0), egui::vec2(120.0, 120.0));
        let on_the_handles = egui::Rect::from_min_size(egui::pos2(540.0, 340.0), egui::vec2(140.0, 24.0));
        let kept = clear_of_dialog(on_the_handles, BoxPlace { dialog: Some(dialog), view: Some(view), handles: Some(handles) });
        assert!(!kept.intersects(handles), "the box {kept:?} still covers the handles {handles:?}");
        assert_eq!(kept.size(), on_the_handles.size(), "the box keeps its size");
        // The window and the handles side by side: no side clears both, so the box keeps off
        // the handles and covers the window instead.
        let side_by_side = egui::Rect::from_min_size(egui::pos2(560.0, 340.0), egui::vec2(280.0, 24.0));
        let kept = clear_of_dialog(side_by_side, BoxPlace { dialog: Some(dialog), view: Some(view), handles: Some(handles) });
        assert!(!kept.intersects(handles), "the box {kept:?} still covers the handles {handles:?}");
    }
}

#[cfg(test)]
mod taper_limit_tests {
    use super::taper_limits;
    use solidcraft_engine::geom::{Loop2, Region2, Vec2};

    /// #1602: a drag stops just short of the taper that closes the profile, either way.
    #[test]
    fn a_taper_drag_stops_before_the_profile_closes() {
        let close = |a: f64, b: f64| (a - b).abs() < 0.05;
        let deg = |r: f64, h: f64| (0.99 * r / h).atan().to_degrees();
        // A 10 × 4 rectangle 26 high: going in closes at its half width; going out never does.
        let rect = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 4.0), Vec2::new(0.0, 4.0)]),
            holes: Vec::new(),
        };
        let (lo, hi) = taper_limits(&[&rect], 26.0);
        assert!(close(lo, -deg(2.0, 26.0)) && hi == 89.0, "{lo} {hi}");
        // A ring 10 to 6, 10 high: in, its 4 mm wall; out, its hole of radius 6.
        let ring = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![Loop2::circle(Vec2::ZERO, 6.0).reversed()] };
        let (lo, hi) = taper_limits(&[&ring], 10.0);
        assert!(close(lo, -deg(2.0, 10.0)) && close(hi, deg(6.0, 10.0)), "{lo} {hi}");
        // The tightest of several profiles; no profile or no height, no limit.
        let (lo, _) = taper_limits(&[&ring, &rect], 10.0);
        assert!(close(lo, -deg(2.0, 10.0)));
        assert_eq!(taper_limits(&[], 10.0), (-89.0, 89.0));
        assert_eq!(taper_limits(&[&rect], 0.0), (-89.0, 89.0));
    }
}
