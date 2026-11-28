//! Transform Face is authored in the selected body's world frame. Its default pivot is the
//! actual pick, so a name's representative point is not a substitute for the viewport hit.
use super::{Dialog, FACES, Kind, Sel, SelInput, Session, Value, ValueKind, Vec3, json, length_default, pt, shown_expr, tr};
use crate::SolveApp;
use crate::gizmo::{self, Change};
use solidcraft_engine::doc::{Feature, FeatureKind, ModelState, apply_point, apply_vector, is_identity, map_point_expr, map_vector_expr, naming};
use solidcraft_engine::geom::prelude::*;
use solidcraft_kernel::affine;
use std::cell::RefCell;
use std::sync::{Arc, Weak};

const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

fn input() -> SelInput {
    SelInput::new("Face", FACES, false)
}

pub(super) const PICK_ON_FACE: &str = "ui.transform_face.pick_on_face";

/// Only the one-shot picker adds a second input. Its point never becomes a feature reference.
pub(crate) fn pivot_input(d: &Dialog) -> Option<&SelInput> {
    if matches!(d.kind, Kind::TransformFace { .. }) { d.inputs.get(1) } else { None }
}

pub(crate) fn pivot_face(d: &Dialog) -> Option<&Sel> {
    if d.active != 1 || pivot_input(d).is_none() {
        return None;
    }
    let [face @ Sel::Face { .. }] = super::sels(d, 0) else { return None };
    Some(face)
}

pub(super) fn set_pivot_mode(d: &mut Dialog, mode: usize) {
    let Kind::TransformFace { explicit_pivot, .. } = &mut d.kind else { return };
    d.inputs.truncate(1);
    *explicit_pivot = mode == 1;
    d.active = 0;
    d.awaiting_pick = mode == 2;
    d.face_drag = None;
    d.pick_refused = None;
    d.error = None;
    if mode == 2 {
        // With no target yet, take the face first. The shared picker then advances to Point.
        d.active = usize::from(!super::sels(d, 0).is_empty());
        d.inputs.push(SelInput::new("Point", crate::selection::VERTICES | crate::selection::SNAP_POINTS, false).tip(PICK_ON_FACE));
    }
}

/// All pick routes, including the Browser, must supply a snap of this face, not just a vertex.
pub(super) fn take_pivot(s: &Session, d: &mut Dialog, sel: &Sel) -> bool {
    if d.active != 1 || pivot_input(d).is_none() {
        return false;
    }
    if let Sel::Vertex { point, .. } = sel
        && pivot_face(d).is_some_and(|face| crate::snap_wake::is_face_snap(s, face, sel))
        && let Kind::TransformFace { origin, .. } = &mut d.kind
    {
        let coords = [point.x, point.y, point.z];
        if coords.into_iter().all(valid_length) {
            // Explicit mm: a displayed inch value could round on the way back to millimetres.
            *origin = coords.map(|v| format!("{v} mm"));
            set_pivot_mode(d, 1);
            d.focus = true;
        } else {
            d.error = Some(tr("ui.transform_face.length_range").into());
        }
    }
    true
}

pub(super) fn start(s: &Session) -> Dialog {
    let kind = Kind::TransformFace {
        translate: std::array::from_fn(|_| length_default(s, "0")),
        axis: 2,
        custom_axis: ["0".into(), "0".into(), "1".into()],
        angle: "0 deg".into(),
        explicit_pivot: false,
        origin: std::array::from_fn(|_| length_default(s, "0")),
    };
    Dialog::new(kind, vec![input()])
}

struct CheckedPick {
    world: Weak<ModelState>,
    pick: Sel,
    at: Option<Vec3>,
}

thread_local! {
    static CHECKED_PICK: RefCell<Option<CheckedPick>> = const { RefCell::new(None) };
}

/// A curved face's display triangle can be off its exact surface. Project only onto the face
/// picked, and never let a point on a shared boundary silently choose the other face. The OK
/// button and the preview both ask on every frame; a pick needs only one exact query pass.
pub(super) fn pick_point(s: &Session, pick: &Sel) -> Option<Vec3> {
    let Sel::Face { body, index, point } = pick else { return None };
    let world = crate::open_meshing::session_world(s);
    CHECKED_PICK.with_borrow_mut(|cached| {
        if let Some(c) = cached.as_ref().filter(|c| c.world.as_ptr() == Arc::as_ptr(&world) && &c.pick == pick) {
            return c.at;
        }
        let at = world.body(body).and_then(|b| {
            if *index >= b.body.face_count() || b.body.is_mesh() || !point.is_finite() {
                return None;
            }
            if b.body.face_at_point(*point).ok()? == Some(*index) {
                return Some(*point);
            }
            let reach = crate::viewport::pick_slack(b.body.size());
            let q = b.body.point_lies_on_face(*index, *point, reach).ok()??;
            (b.body.face_at_point(q).ok()? == Some(*index)).then_some(q)
        });
        *cached = Some(CheckedPick { world: Arc::downgrade(&world), pick: pick.clone(), at });
        at
    })
}

fn direction(axis: usize, custom: &[String; 3]) -> Option<Vec3> {
    if axis != 3 {
        return AXES.get(axis).copied();
    }
    let [x, y, z] = custom.each_ref().map(|s| crate::ui_kit::parse_number(s));
    affine::directed_unit([x?, y?, z?]).map(Vec3::from)
}

fn valid_length(v: f64) -> bool {
    v.abs() < 1e9
}

pub(super) fn motion(
    s: &Session,
    translate: &[String; 3],
    axis: usize,
    custom: &[String; 3],
    angle: &str,
    origin: Option<&[String; 3]>,
) -> Result<Value, String> {
    // The point contract accepts lengths of magnitude less than 1e9 mm. Hidden pivot fields
    // neither enter the command nor prevent a picked-pivot edit from being accepted.
    for e in translate.iter().chain(origin.into_iter().flatten()) {
        let length = s.doc.eval(e, ValueKind::Length).map_err(|err| err.to_string())?;
        if !valid_length(length) {
            return Err(tr("ui.transform_face.length_range").to_owned());
        }
    }
    s.doc.eval(angle, ValueKind::Angle).map_err(|err| err.to_string())?;
    let dir = direction(axis, custom).ok_or_else(|| tr("ui.transform_face.invalid_axis").to_owned())?;
    let mut p = json!({"translate": translate, "axis": pt(dir), "angle": angle});
    if let Some(origin) = origin {
        p["origin"] = json!(origin);
    }
    Ok(p)
}

/// Resolve editable fields with the same design-unit defaults that Apply will use.
fn lengths(s: &Session, fields: &[String; 3]) -> Option<Vec3> {
    let [x, y, z] = fields.each_ref().map(|v| crate::length_input::eval(&s.doc, v, ValueKind::Length).ok().filter(|v| valid_length(*v)));
    Some(Vec3::new(x?, y?, z?))
}

pub(crate) fn triad(app: &SolveApp, d: &Dialog) -> Option<gizmo::Triad> {
    let Kind::TransformFace { translate, explicit_pivot, origin, .. } = &d.kind else { return None };
    if pivot_input(d).is_some() {
        return None;
    }
    let [pick] = super::sels(d, 0) else { return None };
    let at = pick_point(&app.session, pick)?;
    let pivot = if *explicit_pivot { lengths(&app.session, origin)? } else { at };
    let at = affine::translated_point(pivot.into(), lengths(&app.session, translate)?.into());
    Some(gizmo::Triad::world(at.into(), true, true))
}

/// A handle reports the total motion since the press, not the change since the last frame.
/// Save the expressions as well as their evaluated values so dragging back restores them.
#[derive(Debug)]
pub(super) enum DragStart {
    Translate { fields: [String; 3], mm: Vec3 },
    Turn { axis: usize, custom: [String; 3], angle: String, direction: Vec3, radians: f64 },
}

impl DragStart {
    fn capture(s: &Session, kind: &Kind, change: Change) -> Option<Self> {
        let Kind::TransformFace { translate, axis, custom_axis, angle, .. } = kind else { return None };
        Some(match change {
            Change::Translate(_) => Self::Translate { mm: lengths(s, translate)?, fields: translate.clone() },
            Change::Rotate { .. } => Self::Turn {
                direction: direction(*axis, custom_axis)?,
                radians: s.doc.eval(angle, ValueKind::Angle).ok().filter(|v| v.is_finite())?,
                axis: *axis,
                custom: custom_axis.clone(),
                angle: angle.clone(),
            },
        })
    }

    fn apply(&self, app: &SolveApp, kind: &mut Kind, change: Change) {
        let Kind::TransformFace { translate, axis, custom_axis, angle, .. } = kind else { return };
        match (self, change) {
            (Self::Translate { fields, mm }, Change::Translate(delta)) => {
                let values = affine::translated_point((*mm).into(), delta.into());
                if !values.into_iter().all(valid_length) {
                    return;
                }
                for (((field, original), value), delta) in translate.iter_mut().zip(fields).zip(values).zip([delta.x, delta.y, delta.z]) {
                    if delta == 0.0 {
                        if field != original {
                            field.clone_from(original);
                        }
                    } else {
                        *field = crate::length_input::model(&app.session.doc, value);
                    }
                }
            }
            (Self::Turn { axis: start_axis, custom, angle: start_angle, direction, radians }, Change::Rotate { axis: ring, angle: turn }) => {
                let Some(&world) = AXES.get(ring) else { return };
                if !turn.is_finite() {
                    return;
                }
                if turn % std::f64::consts::TAU == 0.0 {
                    *axis = *start_axis;
                    if custom_axis != custom {
                        custom_axis.clone_from(custom);
                    }
                    angle.clone_from(start_angle);
                    return;
                }
                let Some((dir, result)) = affine::compose_turn((*direction).into(), *radians, world.into(), turn) else { return };
                if *direction != world && *direction != -world {
                    if *radians == 0.0 {
                        *axis = ring;
                    } else {
                        *axis = 3;
                        *custom_axis = dir.map(|v| v.to_string());
                    }
                }
                let degrees = result.to_degrees();
                *angle = if degrees.is_finite() { format!("{degrees} deg") } else { format!("{result} rad") };
            }
            _ => {}
        }
    }
}

pub(crate) fn drag(app: &SolveApp, d: &mut Dialog, g: gizmo::Drag) {
    if pivot_input(d).is_some() {
        return;
    }
    if g.started {
        d.face_drag = DragStart::capture(&app.session, &d.kind, g.change).map(Into::into);
    }
    if let Some(start) = &d.face_drag {
        start.apply(app, &mut d.kind, g.change);
    }
    if g.done {
        d.face_drag = None;
    }
}

/// The edited feature stores the face, distances, axis and pivot in its input body's definition.
/// Show all four through that same body's placement, including any body offset. A missing or
/// split named face leaves the picker empty for repair instead of attaching to a nearby face.
pub(super) fn reopen(s: &Session, feature: &Feature) -> Option<Dialog> {
    let FeatureKind::TransformFace { face, body, translate, rotate_axis, origin, angle } = &feature.kind else { return None };
    let local = s.model.state();
    let owner = local.body(body)?;
    let placed = s.placed_document();
    let component = placed.model_body_component(owner);
    let path = s.active_occurrence_path.as_deref().filter(|_| s.active_component == component);
    let first_path = path.is_none().then(|| placed.first_path(component));
    let path = path.or(first_path.as_deref())?;
    let frame = placed.body_frame_at(owner, path).ok()?;
    let world_name = placed.world_body_name(&local, &owner.name, component, path);
    let original_face = owner.body.face_at_point(face.0).ok().flatten();
    let resolved = match feature.face_names.first().filter(|n| !n.is_empty()) {
        Some(name) => naming::locate_face_on_body(&local, Some(body), name, face.0)
            .filter(|hit| local.bodies.get(hit.body).is_some_and(|b| b.name == owner.name))
            .map(|hit| (hit.face, if original_face == Some(hit.face) { face.0 } else { hit.pick_point(&local, face.0) })),
        None => original_face.map(|index| (index, face.0)),
    };
    let mut input = input();
    input.items = resolved.map(|(index, p)| Sel::Face { body: world_name, index, point: apply_point(&frame, p) }).into_iter().collect();
    let mut translate = translate.clone();
    let mut origin = origin.clone();
    let mut dir = rotate_axis.0;
    if !is_identity(&frame) {
        map_vector_expr(&frame, &mut translate);
        if let Some(p) = &mut origin {
            map_point_expr(&frame, p);
        }
        dir = apply_vector(&frame, dir);
    }
    let kind = Kind::TransformFace {
        translate: translate.map(|e| shown_expr(&e, ValueKind::Length)),
        axis: AXES.iter().position(|a| dir == *a || (!is_identity(&frame) && dir.dist(*a) <= 8.0 * f64::EPSILON)).unwrap_or(3),
        custom_axis: [dir.x, dir.y, dir.z].map(|v| v.to_string()),
        angle: angle.as_ref().map_or_else(|| "0 deg".into(), |e| shown_expr(e, ValueKind::Angle)),
        explicit_pivot: origin.is_some(),
        origin: origin.map_or_else(|| std::array::from_fn(|_| length_default(s, "0")), |o| o.map(|e| shown_expr(&e, ValueKind::Length))),
    };
    Some(Dialog::new(kind, vec![input]))
}
