//! 3D sketch drawing: with the sketch's 3D Sketch option on (Sketch Palette), lines, arcs,
//! splines and points may leave the sketch plane. They are kept as drawn 3D curves (wires with
//! fit points, exact arcs keeping their analytic form, control point splines keeping their
//! control points): drawn and snapped to, movable point by point, and usable as sweep and pipe
//! paths.
//!
//! Their fit points take constraints of their own (#10608), solved after the planar sketch:
//! Coincident and Fix take 3D refs (`<curve>.start`, `.end`, `.through`, `.<n>`, or a 3D point's
//! id), and Along Axis holds a 3D line parallel to an axis.

use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{Axis, CurveKind, OriginPlane, PointExprs3, WireConstraintKind, WirePoint, WireSpace};

use super::sketch::edit;
use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, vec3};
use crate::schema::sketch::Constrained;
use crate::schema::sketch_3d::{
    AlongAxisParams, AlongAxisResult, Arc3dParams, AxisName, ControlSpline3dParams, Create3dParams, Create3dResult, Curves3dResult, Line3dParams,
    Move3dParams, Move3dResult, OnPlaneParams, OnPlaneResult, POINT3_MUST, Point3dParams, Point3dResult, Spline3dParams, point3_coordinates,
};
use crate::schema::sketch_dimension::DimensionResult;
use crate::schema::sketch_modify::FilletResult;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CREATE3D_CONTRACT: TypedContract<Create3dParams> = TypedContract::new();
const LINE3D_CONTRACT: TypedContract<Line3dParams> = TypedContract::new();
const SPLINE3D_CONTRACT: TypedContract<Spline3dParams> = TypedContract::new();
const CONTROL_SPLINE3D_CONTRACT: TypedContract<ControlSpline3dParams> = TypedContract::new();
const ARC3D_CONTRACT: TypedContract<Arc3dParams> = TypedContract::new();
const POINT3D_CONTRACT: TypedContract<Point3dParams> = TypedContract::new();
const MOVE3D_CONTRACT: TypedContract<Move3dParams> = TypedContract::new();
const ALONG_AXIS_CONTRACT: TypedContract<AlongAxisParams> = TypedContract::new();
const ON_PLANE_CONTRACT: TypedContract<OnPlaneParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.create3d", "3D Sketch", crate::checked!("sketch.create3d", create3d))
        .begins_edit()
        .icon("sketch3d")
        .contract(&CREATE3D_CONTRACT)
        .output(output_schema_for::<Create3dResult>)
        .params("name?; view?: open Model view id (defaults to focused or most recent Model). Starts a 3D sketch with no plane to choose: a new sketch with 3D Sketch on, opened for editing, so sketch.line3d, sketch.spline3d, sketch.arc3d and sketch.point3d draw in it at once. It rests on the active component's XY origin plane and its curves may leave that plane. One undo step"),
    CommandSpec::new("sketch.line3d", "3D Line", crate::checked!("sketch.line3d", line3d))
        .icon("line")
        .enabled(in_sketch_3d)
        .contract(&LINE3D_CONTRACT)
        .output(output_schema_for::<Curves3dResult>)
        .params(concat!(
            "points: [[x,y,z]…] world points (2 or more; a coordinate may be an expression over the design's parameters, the point then held where it comes to and following them, in a sketch whose component is not moved): one 3D line per segment, joined end to end with a Coincident at each joint and to the drawn 3D ends it starts or ends on (needs 3D Sketch on); constraints lists the joints and placed points",
            created_doc!()
        )),
    CommandSpec::new("sketch.spline3d", "3D Spline", crate::checked!("sketch.spline3d", spline3d))
        .icon("spline")
        .enabled(in_sketch_3d)
        .contract(&SPLINE3D_CONTRACT)
        .output(output_schema_for::<Curves3dResult>)
        .params(concat!("points: [[x,y,z]…] world fit points (2 or more, coordinates numbers or expressions as for sketch.line3d): a smooth 3D curve through them (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.spline3d.control_point", "3D Control Point Spline", crate::checked!("sketch.spline3d.control_point", control_spline3d))
        .icon("spline_cv")
        .enabled(in_sketch_3d)
        .contract(&CONTROL_SPLINE3D_CONTRACT)
        .output(output_schema_for::<Curves3dResult>)
        .params(concat!(
            "points: [[x,y,z]…] world control points (3 or more, coordinates numbers or expressions as for sketch.line3d): a smooth cubic 3D curve that starts on the first control point, ends on the last and is pulled towards the ones between without passing through them (needs 3D Sketch on). Stored exactly (the B-spline over its control points), so a path follows the curve itself; sketch.move3d moves a control point",
            created_doc!()
        )),
    CommandSpec::new("sketch.arc3d", "3D Arc", crate::checked!("sketch.arc3d", arc3d))
        .icon("arc3")
        .enabled(in_sketch_3d)
        .contract(&ARC3D_CONTRACT)
        .output(output_schema_for::<Curves3dResult>)
        .params(concat!(
            "start, through, end: [x,y,z] world points (coordinates numbers or expressions as for sketch.line3d): an exact circular arc through them, from start through through to end (needs 3D Sketch on). Stored exactly (centre, radius, angles), so path consumers can treat it as a true arc",
            created_doc!()
        )),
    CommandSpec::new("sketch.point3d", "3D Point", crate::checked!("sketch.point3d", point3d))
        .icon("point")
        .enabled(in_sketch_3d)
        .contract(&POINT3D_CONTRACT)
        .output(output_schema_for::<Point3dResult>)
        .params(concat!("point: [x,y,z] world point, coordinates numbers or expressions as for sketch.line3d (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.move3d", "Move 3D Point", crate::checked!("sketch.move3d", move3d))
        .icon("sketch_move")
        .enabled(in_sketch)
        .contract(&MOVE3D_CONTRACT)
        .output(output_schema_for::<Move3dResult>)
        .params("wire: drawn 3D curve or point id, index?: fit point, or control point of a control point spline (default 0) | point: a point on the sketch plane (`l1.end`, `p3`: a line's end, a fit point of a spline, a lone point; needs 3D Sketch on); to: [x,y,z] | by: [dx,dy,dz] (world); points joined to it move too, and the sketch's 3D constraints are solved with the moved point held where it was put (refused when they cannot hold it there, or when it is fixed). A `point` on the plane first turns the lines and fit-point splines through it (or the lone point) into drawn 3D curves through the same places, which `curves` lists (`wire` and `index` say where the point is now); their constraints and dimensions go. Arcs, circles, ellipses, conics and control-point or closed splines through it are refused"),
    CommandSpec::new("sketch.constraint.along_axis", "Along Axis", crate::checked!("sketch.constraint.along_axis", along_axis))
        .icon("c_hv")
        .enabled(in_sketch)
        .contract(&ALONG_AXIS_CONTRACT)
        .output(output_schema_for::<AlongAxisResult>)
        .params("line: a drawn 3D line's id | lines: [ids]; axis: x|y|z (the axes of the sketch's 3D space: its component's, for a sketch in a component); sketch?: id|name. Holds each line parallel to the axis; refused when that conflicts with the sketch's other 3D constraints. Returns one constraint id per line"),
    CommandSpec::new("sketch.constraint.on_plane", "On Plane", crate::checked!("sketch.constraint.on_plane", on_plane))
        .icon("c_coincident")
        .enabled(in_sketch)
        .contract(&ON_PLANE_CONTRACT)
        .output(output_schema_for::<OnPlaneResult>)
        .params("point: a 3D point ref (`w1.start`, `.end`, `.through`, `.<n>`, or a 3D point's id) | points: [refs]; plane: xy|yz|xz (the origin planes of the sketch's 3D space), a construction plane's name, or {face: [x,y,z], body?} (the planar model face through that point, kept by its face name); sketch?: id|name. Holds each point on the plane; refused when that conflicts with the sketch's other 3D constraints. Returns one constraint id per point"),
];

/// A command's reply as JSON: built from the type its output schema describes, so the two can't
/// drift apart.
fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// The `points` of a 3D drawing command as given (each `[x, y, z]` or `{x, y, z}`, checked by
/// [`expr_point3`]), each with the message for something that is no point.
fn point_values<'a>(cmd: &str, p: &'a Value, min: usize) -> Result<Vec<(&'a Value, String)>> {
    let a = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must list [x,y,z] points"))?;
    if a.len() < min || a.len() > solidcraft_sketch::MAX_FIT_POINTS {
        return Err(bad(cmd, format!("`points` needs {min} to {} points", solidcraft_sketch::MAX_FIT_POINTS)));
    }
    Ok(a.iter().map(|v| (v, "a point must be [x, y, z]".to_string())).collect())
}

/// A 3D point `[x, y, z]` or `{x, y, z}` whose coordinates may be expressions over the design's
/// parameters: where it is now and, when a coordinate names a parameter, the expressions that
/// place it. `shape` is the message for something that is no point.
fn expr_point3(s: &Session, v: &Value, cmd: &str, shape: &str) -> Result<(Vec3, Option<PointExprs3>)> {
    let Some([x, y, z]) = point3_coordinates(v) else { return Err(bad(cmd, shape)) };
    let mut at = [0.0; 3];
    let mut exprs = [None, None, None];
    for (k, c) in [x, y, z].into_iter().enumerate() {
        let (value, e) = super::sketch_create::text_number(&s.doc, c, cmd, ["point x", "point y", "point z"][k], POINT3_MUST, false)?;
        at[k] = value;
        exprs[k] = e;
    }
    let [ex, ey, ez] = exprs;
    let placed = (ex.is_some() || ey.is_some() || ez.is_some()).then_some(PointExprs3 { x: ex, y: ey, z: ez });
    Ok((Vec3::new(at[0], at[1], at[2]), placed))
}

/// The points of a 3D drawing command, each with the expressions that place it (`values`: each
/// point with the message for something that is no point). Expressions are in the sketch's own
/// coordinates, so they are only taken where those are the world's (the sketch's component is not
/// moved), as a plain number would be.
fn points3(s: &Session, cmd: &str, values: &[(&Value, String)], inverse: &solidcraft_doc::Mat) -> Result<Vec<(Vec3, Option<PointExprs3>)>> {
    let pts: Vec<(Vec3, Option<PointExprs3>)> = values.iter().map(|(v, shape)| expr_point3(s, v, cmd, shape)).collect::<Result<_>>()?;
    let placed = pts.iter().any(|(_, e)| e.is_some());
    if placed && !solidcraft_doc::is_identity(inverse) {
        return Err(bad(
            cmd,
            "expressions in 3D point coordinates need a sketch whose component is not moved (give numbers, or edit the sketch at the component's own placement)",
        ));
    }
    Ok(pts.into_iter().map(|(at, e)| (solidcraft_doc::apply_point(inverse, at), e)).collect())
}

/// Hold each point that came with expressions where they place it: a Fix that follows them.
fn place_points(sk: &mut solidcraft_sketch::Sketch, cmd: &str, placed: &[(WirePoint, Option<PointExprs3>)]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for (point, exprs) in placed {
        let Some(exprs) = exprs else { continue };
        let at = sk.wire_point_at(point).ok_or_else(|| bad(cmd, "a drawn 3D point went missing"))?;
        out.push(sk.add_wire_constraint(WireConstraintKind::Fix { p: point.clone(), at, exprs: Some(Box::new(exprs.clone())) })?);
    }
    Ok(out)
}

fn needs_3d(sk: &solidcraft_sketch::Sketch, cmd: &str) -> Result<()> {
    if sk.view.three_d { Ok(()) } else { Err(bad(cmd, "turn on 3D Sketch (sketch.options sketch_3d: true) to draw off the sketch plane")) }
}

/// The 3D Line, Spline, Control Point Spline, Arc and Point buttons start interactive tools, so they are only offered
/// while a 3D sketch is being edited; otherwise the tools could only fail on their first click.
fn in_sketch_3d(s: &Session) -> std::result::Result<(), String> {
    let id = sketch_store::active(s).ok_or("no sketch is being edited (Create Sketch or Edit Sketch first)")?;
    match sketch_store::read(&s.doc, id) {
        Ok(sk) if sk.view.three_d => Ok(()),
        Ok(_) => Err("turn on 3D Sketch (sketch.options sketch_3d: true) to draw 3D curves".into()),
        Err(_) => Err("no sketch is being edited (Create Sketch or Edit Sketch first)".into()),
    }
}

/// World command inputs belong to the target sketch's owner, which may differ from the
/// active component when a script supplies an explicit sketch.
fn inverse_owner_frame(s: &Session, p: &Value, cmd: &str) -> Result<solidcraft_doc::Mat> {
    let id = sketch_store::target(s, p, cmd)?;
    let owner = s.doc.feature(id).map(|f| f.component).ok_or_else(|| bad(cmd, "target sketch is missing"))?;
    if owner != 0 && owner != s.active_component {
        if s.doc.occurrences.len() > 100_000 {
            return Err(bad(cmd, "sketch owner placement expansion exceeds supported bounds"));
        }
        let mut children: std::collections::BTreeMap<u64, Vec<&solidcraft_doc::Occurrence>> = std::collections::BTreeMap::new();
        for occurrence in &s.doc.occurrences {
            children.entry(occurrence.parent).or_default().push(occurrence);
        }
        let mut stack = vec![(0, 0usize)];
        let mut count = 0;
        let mut visited = 0;
        while let Some((component, depth)) = stack.pop() {
            for o in children.get(&component).into_iter().flatten() {
                visited += 1;
                if depth >= 64 || visited > 100_000 {
                    return Err(bad(cmd, "sketch owner placement expansion exceeds supported bounds"));
                }
                if o.component == owner {
                    count += 1;
                    if count > 1 {
                        return Err(bad(cmd, "activate the desired sketch owner occurrence before editing world 3D points"));
                    }
                }
                stack.push((o.component, depth + 1));
            }
        }
    }
    let frame = if owner == s.active_component { s.active_component_frame() } else { s.placed_document().component_transform(owner) };
    solidcraft_doc::mat_inverse(&frame).ok_or_else(|| bad(cmd, "the sketch owner placement is not invertible"))
}

/// `sketch.create3d`: a new sketch with 3D Sketch on, with no plane to choose. A sketch always
/// has a plane, so this one rests on the XY origin plane of the active component: a click in
/// empty space lands there, and nothing new is saved in the design.
fn create3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.create3d";
    s.requested_sketch_view(p, cmd)?;
    // Only the keys this command takes reach `sketch.create`'s builder: a stray `plane` or
    // `project_edges` must not turn this into a sketch on a face.
    let mut keys = serde_json::Map::new();
    for key in ["name", "view"] {
        if let Some(value) = p.get(key) {
            keys.insert(key.into(), value.clone());
        }
    }
    let made = super::sketch::sketch_on_plane(s, &Value::Object(keys), solidcraft_doc::PlaneRef::Origin { name: "XY".into() }, None)?;
    sketch_store::update(s, made.sketch, |_, _, sk| {
        sk.view.three_d = true;
        Ok(())
    })?;
    reply(cmd, Create3dResult { sketching_view: made.sketching_view, sketch: made.sketch, name: made.name, param_warnings: Vec::new() })
}

fn line3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.line3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let target = sketch_store::target(s, p, cmd)?;
    let given = point_values(cmd, p, 2)?;
    let pts = points3(s, cmd, &given, &inverse)?;
    let ((ids, joints), info) = edit(s, p, cmd, false, |sk, doc| {
        needs_3d(sk, cmd)?;
        let at: Vec<Vec3> = pts.iter().map(|(at, _)| *at).collect();
        let ids = at.windows(2).map(|w| sk.add_drawn_wire_in(w, WireSpace::Component).map_err(Into::into)).collect::<Result<Vec<String>>>()?;
        // Joined end to end, and to the drawn ends they start or end on (#10608).
        let mut joints = join_in(sk, doc, target, &ids)?;
        // Point k is line k's start, and the last line's end.
        let placed: Vec<(WirePoint, Option<PointExprs3>)> = pts
            .iter()
            .enumerate()
            .filter_map(|(k, (_, e))| {
                let (wire, index) = if k < ids.len() { (ids.get(k)?, 0) } else { (ids.last()?, 1) };
                Some((WirePoint { wire: wire.clone(), index }, e.clone()))
            })
            .collect();
        joints.extend(place_points(sk, cmd, &placed)?);
        Ok((ids, joints))
    })?;
    reply(cmd, Curves3dResult { curves: ids, sketch: info, constraints: joints, created: None, param_warnings: Vec::new() })
}

fn spline3d(s: &mut Session, p: &Value) -> Result<Value> {
    drawn_spline(s, p, "sketch.spline3d", 2, |sk, at| sk.add_drawn_wire_in(at, WireSpace::Component))
}

/// Degree of the curve `sketch.spline3d.control_point` draws.
const CONTROL_DEGREE: u8 = 3;

fn control_spline3d(s: &mut Session, p: &Value) -> Result<Value> {
    drawn_spline(s, p, "sketch.spline3d.control_point", solidcraft_sketch::MIN_CONTROL_POINTS, |sk, at| {
        sk.add_drawn_control_in(at, CONTROL_DEGREE, WireSpace::Component)
    })
}

/// A 3D spline command: `add` makes the curve from the `points` (at least `min`), which is then
/// joined to the drawn 3D ends it starts or ends on and held where its expressions place it.
fn drawn_spline(
    s: &mut Session,
    p: &Value,
    cmd: &'static str,
    min: usize,
    add: impl FnOnce(&mut solidcraft_sketch::Sketch, &[Vec3]) -> std::result::Result<String, solidcraft_sketch::SketchError>,
) -> Result<Value> {
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let target = sketch_store::target(s, p, cmd)?;
    let given = point_values(cmd, p, min)?;
    let pts = points3(s, cmd, &given, &inverse)?;
    let ((id, placed), info) = edit(s, p, cmd, false, |sk, doc| {
        needs_3d(sk, cmd)?;
        let at: Vec<Vec3> = pts.iter().map(|(at, _)| *at).collect();
        let id = add(sk, &at)?;
        let held: Vec<(WirePoint, Option<PointExprs3>)> =
            pts.iter().enumerate().map(|(index, (_, e))| (WirePoint { wire: id.clone(), index }, e.clone())).collect();
        let mut placed = join_in(sk, doc, target, std::slice::from_ref(&id))?;
        placed.extend(place_points(sk, cmd, &held)?);
        Ok((id, placed))
    })?;
    reply(cmd, Curves3dResult { curves: vec![id], sketch: info, constraints: placed, created: None, param_warnings: Vec::new() })
}

fn arc3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.arc3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let target = sketch_store::target(s, p, cmd)?;
    let given: Vec<(&Value, String)> = ["start", "through", "end"]
        .iter()
        .map(|k| {
            let shape = format!("`{k}` must be a point [x, y, z]");
            p.get(*k).map(|v| (v, shape.clone())).ok_or_else(|| bad(cmd, shape))
        })
        .collect::<Result<_>>()?;
    let pts = points3(s, cmd, &given, &inverse)?;
    let ((id, placed), info) = edit(s, p, cmd, false, |sk, doc| {
        needs_3d(sk, cmd)?;
        let at: Vec<Vec3> = pts.iter().map(|(at, _)| *at).collect();
        let id = sk.add_drawn_arc_in(&at, WireSpace::Component)?;
        let held: Vec<(WirePoint, Option<PointExprs3>)> =
            pts.iter().enumerate().map(|(index, (_, e))| (WirePoint { wire: id.clone(), index }, e.clone())).collect();
        let mut placed = join_in(sk, doc, target, std::slice::from_ref(&id))?;
        placed.extend(place_points(sk, cmd, &held)?);
        Ok((id, placed))
    })?;
    reply(cmd, Curves3dResult { curves: vec![id], sketch: info, constraints: placed, created: None, param_warnings: Vec::new() })
}

fn point3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.point3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let target = sketch_store::target(s, p, cmd)?;
    const SHAPE: &str = "`point` must be a point [x, y, z]";
    let given = p.get("point").ok_or_else(|| bad(cmd, SHAPE))?;
    let pts = points3(s, cmd, &[(given, SHAPE.to_string())], &inverse)?;
    let ((id, placed), info) = edit(s, p, cmd, false, |sk, doc| {
        needs_3d(sk, cmd)?;
        let (at, e) = pts.first().cloned().ok_or_else(|| bad(cmd, "`point` must be [x,y,z]"))?;
        let id = sk.add_drawn_wire_in(&[at], WireSpace::Component)?;
        let mut placed = join_in(sk, doc, target, std::slice::from_ref(&id))?;
        placed.extend(place_points(sk, cmd, &[(WirePoint { wire: id.clone(), index: 0 }, e)])?);
        Ok((id, placed))
    })?;
    reply(cmd, Point3dResult { point: id, sketch: info, constraints: placed, created: None, param_warnings: Vec::new() })
}

fn move3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move3d";
    if let Some(point) = p.get("point") {
        if p.get("wire").is_some() {
            return Err(bad(cmd, "give `wire` or `point`, not both"));
        }
        if p.get("index").is_some() {
            return Err(bad(cmd, "`index` goes with `wire`"));
        }
        let point = point.as_str().ok_or_else(|| bad(cmd, "`point` must be a sketch point id"))?.to_string();
        return lift_point(s, p, cmd, &point);
    }
    let wire = p.get("wire").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`wire` must be a 3D curve or point id"))?.to_string();
    let index = match p.get("index") {
        None => 0,
        Some(v) => v
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v < solidcraft_sketch::MAX_FIT_POINTS)
            .ok_or_else(|| bad(cmd, "`index` must be a fit point index below 500"))?,
    };
    let parse = |name| p.get(name).map(|v| vec3(v).ok_or_else(|| bad(cmd, format!("`{name}` must be [x,y,z]")))).transpose();
    let to = parse("to")?;
    let by = parse("by")?;
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        let drawn = sk
            .wire_index(&wire)
            .and_then(|w| sk.wires.get(w))
            .filter(|w| !w.fit.is_empty() && w.link.is_none())
            .ok_or_else(|| bad(cmd, format!("`{wire}` is not an editable drawn 3D curve")))?;
        let from = drawn.fit.get(index).copied().ok_or_else(|| bad(cmd, format!("`{wire}` has no drawn point {index}")))?;
        let to = match (to, by) {
            (Some(t), _) => {
                if drawn.space == WireSpace::Component {
                    solidcraft_doc::apply_point(&inverse, t)
                } else {
                    t
                }
            }
            (None, Some(d)) => from + if drawn.space == WireSpace::Component { solidcraft_doc::apply_vector(&inverse, d) } else { d },
            _ => return Err(bad(cmd, "give `to` or `by`")),
        };
        move_held(sk, &wire, index, to, cmd)
    })?;
    reply(cmd, Move3dResult { moved: n, curves: Vec::new(), wire: None, index: None, sketch: info, param_warnings: Vec::new() })
}

/// Move fit point `index` of drawn 3D curve `wire` to `to` (the curve's own coordinates), the
/// points joined to it with it, then solve the sketch's 3D constraints with the moved point held
/// where it was put. Returns how many points moved.
fn move_held(sk: &mut solidcraft_sketch::Sketch, wire: &str, index: usize, to: Vec3, cmd: &str) -> Result<usize> {
    let point = WirePoint { wire: wire.to_string(), index };
    if sk.wire_point_fixed(&point) {
        return Err(bad(cmd, format!("`{wire}` point {index} is fixed, or joined to a fixed point (unfix it to move it)")));
    }
    // The 3D constraints follow the move, the moved point held where it was put. Refused when
    // it breaks a constraint that held before it (a failure elsewhere does not stop it), or
    // squeezes a curve away.
    let held_before = sk.wire_constraints_holding();
    let n = sk.move_drawn_point(wire, index, to)?;
    if !sk.wire_constraints.is_empty() {
        let moved = sk.clone();
        let rep = sk.solve_wires_with(Some(&point), false);
        let held = sk.wire_point_at(&point).is_some_and(|at| at.dist(to) < 1e-6);
        // A group too large to solve, or out of time, leaves its points where the move put
        // them without listing its constraints: check them again.
        let held_after = sk.wire_constraints_holding();
        if !held || rep.failing.iter().any(|id| held_before.contains(id)) || held_before.iter().any(|id| !held_after.contains(id)) {
            return Err(bad(cmd, "the sketch's 3D constraints cannot hold the point there"));
        }
        if let Some(why) = super::sketch::wire_collapse(&moved, sk) {
            return Err(bad(cmd, why));
        }
    }
    Ok(n)
}

/// What a plane curve through a lifted point is as a drawn 3D curve: its point indices, in the
/// order the drawn curve takes them as fit points. Lines and open fit-point splines only.
fn liftable(c: &solidcraft_sketch::Curve, cmd: &str) -> Result<Vec<usize>> {
    if c.link.is_some() {
        return Err(bad(cmd, format!("`{}` is projected geometry; it stays where it was projected", c.id)));
    }
    let what = match &c.kind {
        CurveKind::Line { a, b } => return Ok(vec![*a, *b]),
        CurveKind::Spline { pts, control: false, periodic: false, .. } => return Ok(pts.clone()),
        CurveKind::Spline { .. } => "a control-point or closed spline",
        CurveKind::Circle { .. } => "a circle",
        CurveKind::Arc { .. } => "an arc",
        CurveKind::Ellipse { .. } => "an ellipse",
        CurveKind::Conic { .. } => "a conic",
    };
    Err(bad(cmd, format!("`{}` is {what}: only lines, fit-point splines and lone points can leave the sketch plane", c.id)))
}

/// `sketch.move3d` with `point`: the lines and fit-point splines through a point on the sketch
/// plane (or the lone point) become drawn 3D curves through the same places, then the point
/// moves to `to` or by `by` (world), the curves through it staying joined there.
fn lift_point(s: &mut Session, p: &Value, cmd: &str, point: &str) -> Result<Value> {
    let parse = |name| p.get(name).map(|v| vec3(v).ok_or_else(|| bad(cmd, format!("`{name}` must be [x,y,z]")))).transpose();
    let (to, by) = (parse("to")?, parse("by")?);
    if to.is_none() && by.is_none() {
        return Err(bad(cmd, "give `to` or `by`"));
    }
    let id = sketch_store::target(s, p, cmd)?;
    // The plane in the owner component's coordinates, which drawn curves in `Component`
    // space use.
    let plane = s.model.state().sketch(id).map(|ss| ss.plane).ok_or_else(|| bad(cmd, "the sketch has not been solved"))?;
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let place = |q: Vec2| plane.to_world(q);
    let ((n, curves, k), info) = edit(s, p, cmd, false, |sk, doc| {
        needs_3d(sk, cmd)?;
        let missing = || bad(cmd, format!("no point `{point}` in the sketch"));
        let pi = sk.resolve_point(point).filter(|i| *i != 0).ok_or_else(missing)?;
        let at = sk.points.get(pi).ok_or_else(missing)?;
        if at.link.is_some() {
            return Err(bad(cmd, format!("`{point}` is projected geometry; it stays where it was projected")));
        }
        let from = place(at.pos);
        let goal = match (to, by) {
            (Some(t), _) => solidcraft_doc::apply_point(&inverse, t),
            (None, Some(d)) => from + solidcraft_doc::apply_vector(&inverse, d),
            (None, None) => return Err(bad(cmd, "give `to` or `by`")),
        };
        // The points joined to `pi` by coincident constraints are one place: the curves through any
        // of them lift together.
        let roots = super::sketch::coincident_points(sk);
        let group: Vec<usize> = (0..sk.points.len()).filter(|i| roots.get(*i) == roots.get(pi)).collect();
        let through: Vec<usize> = sk.curves.iter().enumerate().filter(|(_, c)| group.iter().any(|g| c.kind.uses(*g))).map(|(i, _)| i).collect();
        let mut fits = Vec::new();
        let mut k = 0;
        for (n, &ci) in through.iter().enumerate() {
            let ids = liftable(&sk.curves[ci], cmd)?;
            if n == 0 {
                k = ids
                    .iter()
                    .position(|i| *i == pi)
                    .or_else(|| ids.iter().position(|i| group.contains(i)))
                    .ok_or_else(|| bad(cmd, format!("`{point}` is not a point of `{}`", sk.curves[ci].id)))?;
            }
            let fit = ids
                .iter()
                .map(|i| sk.points.get(*i).map(|q| place(q.pos)).ok_or_else(|| bad(cmd, "a curve's point is missing")))
                .collect::<Result<Vec<Vec3>>>()?;
            fits.push(fit);
        }
        // A joined point at the origin is not lifted: the origin stays put.
        if group.contains(&0) {
            return Err(bad(cmd, format!("`{point}` is joined to the origin; it stays")));
        }
        // Joined points that no curve uses at all, by id: the removal below may shift indices.
        let lone: Vec<String> =
            group.iter().copied().filter(|g| *g != pi && !sk.curves.iter().any(|c| c.kind.uses(*g))).map(|g| sk.points[g].id.clone()).collect();
        let mut doomed = Vec::new();
        if through.is_empty() {
            fits.push(vec![from]);
            doomed.push(pi);
        } else {
            sk.remove_curves(&through);
        }
        doomed.extend(lone.iter().filter_map(|id| sk.resolve_point(id)));
        sk.remove_points(&doomed);
        let mut made = Vec::new();
        for fit in &fits {
            made.push(sk.add_drawn_wire_in(fit, WireSpace::Component)?);
        }
        // Joined where they meet, as drawn curves are, then moved as `wire` and `index` would be:
        // the sketch's 3D constraints follow (#10608).
        join_in(sk, doc, id, &made)?;
        let first = made.first().ok_or_else(|| bad(cmd, "nothing to lift"))?;
        let n = move_held(sk, first, k, goal, cmd)?;
        Ok((n, made, k))
    })?;
    let wire = curves.first().cloned();
    reply(cmd, Move3dResult { moved: n, curves, wire, index: Some(k), sketch: info, param_warnings: Vec::new() })
}

/// `sketch.constraint.coincident` on 3D refs, including model edges and projected vertices.
/// `None` when neither operand is 3D, for the planar command.
pub(super) fn coincident_3d(s: &mut Session, p: &Value, cmd: &str) -> Result<Option<Value>> {
    if s.sheet_sketching() {
        return Ok(None);
    }
    let pick = |key| p.get(key).filter(|v| v.is_string() || v.get("edge").is_some() || v.get("vertex").is_some());
    let (Some(a), Some(b)) = (pick("a"), pick("b")) else { return Ok(None) };
    let id = sketch_store::target(s, p, cmd)?;
    let sk = sketch_store::read(&s.doc, id)?;
    if ![a, b].iter().any(|v| v.as_str().is_some_and(|r| is_3d(sk, r))) {
        return Ok(None);
    }
    let projected;
    let (a, b) = match (a.as_str(), b.as_str()) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            projected = super::sketch_project::project_3d_coincident_operands(s, p, cmd)?;
            let operand = |key| projected.get(key).and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{key}` must be a point or curve")));
            (operand("a")?, operand("b")?)
        }
    };
    let sk = sketch_store::read(&s.doc, id)?;
    // Either side may be fixed geometry the drawn 3D one is held to (#10616): a model curve
    // included in the sketch (its ends, its line or its circle), or a 2D point or curve.
    let line = |r: &str| sk.is_line_operand(r).then(|| r.to_string());
    let curve = |r: &str| (!sk.is_line_operand(r) && sk.is_curve_operand(r)).then(|| r.to_string());
    let kind = match (sk.point_operand(a), sk.point_operand(b)) {
        (Some(x), Some(y)) => WireConstraintKind::Coincident { a: x, b: y },
        (Some(x), None) | (None, Some(x)) => {
            let other = if sk.point_operand(a).is_some() { b } else { a };
            match (line(other), curve(other)) {
                (Some(l), _) => WireConstraintKind::PointOnLine { p: x, line: l },
                (None, Some(c)) => WireConstraintKind::PointOnCurve { p: x, curve: c },
                _ => return Err(bad(cmd, format!("`{other}` is not a point, line or circle a 3D point can be held to"))),
            }
        }
        _ => {
            return Err(bad(
                cmd,
                "give two points, or a point and a line or circle, at least one of them drawn 3D geometry: a 3D point (`<curve>.start`, `.end`, `.through`, `.<n>`, or a 3D point's id), a model curve included with sketch.include_3d (`<curve>.start`, `.end`, the curve), or a 2D point or curve of the sketch",
            ));
        }
    };
    let (ids, info) = add_3d(s, p, cmd, vec![kind])?;
    Ok(Some(json!({"constraint": ids.first(), "sketch": info})))
}

/// Whether a ref names 3D sketch geometry: a 3D point, a drawn or included 3D curve, or an
/// included curve's end.
fn is_3d(sk: &solidcraft_sketch::Sketch, r: &str) -> bool {
    sk.wire_point(r).is_some() || sk.wire_index(r).is_some() || r.rsplit_once('.').is_some_and(|(id, _)| sk.wire_index(id).is_some())
}

/// The plane a 3D constraint of sketch `id` is held to, from a command's value (#10616): an
/// origin plane of the sketch's 3D space (`xy`, `yz`, `xz`, any case), a construction plane by
/// name, or `{face: [x,y,z], body?}`, the planar model face through that point (picked exactly,
/// kept by its face name). A model plane comes resolved, so the edit's own solve can use it.
/// `None` when the value names none of these.
fn plane_target(s: &Session, id: u64, v: &Value, cmd: &str) -> Result<Option<solidcraft_sketch::PlaneOf>> {
    use solidcraft_sketch::{LinkSource, ModelPlane, PlaneOf};
    if let Some(origin) = v.as_str().and_then(OriginPlane::from_name) {
        return Ok(Some(PlaneOf::Origin(origin)));
    }
    let source = match v {
        Value::String(name) if s.doc.find_feature(name).is_some_and(|f| matches!(f.kind, solidcraft_doc::FeatureKind::ConstructionPlane { .. })) => {
            (LinkSource::Plane { name: name.clone() }, None)
        }
        Value::Object(o) if o.contains_key("face") => {
            let at = o.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be [x,y,z], a point on a planar face"))?;
            let body = o.get("body").and_then(Value::as_str);
            let (_, st, _, _) = super::sketch_project::wire_context(s, id, &body.into_iter().collect::<Vec<_>>())
                .ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
            let (body, face, _) = solidcraft_doc::project::pick_planar_face(&st, body, at).map_err(|why| bad(cmd, why))?;
            (LinkSource::Face { body, at }, Some(face))
        }
        _ => return Ok(None),
    };
    let mut m = ModelPlane { source: source.0, face_name: source.1, resolved: None, resolved_face: None };
    let (vals, st, _, frame) =
        super::sketch_project::wire_context(s, id, &m.source.bodies()).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
    solidcraft_doc::project::resolve_model_plane(&s.design_document(), &vals, &st, &mut m, Some(&frame)).map_err(|why| bad(cmd, why))?;
    Ok(Some(PlaneOf::Model(Box::new(m))))
}

/// `sketch.constraint.fix` on 3D refs: each 3D point (or every fit point of a drawn 3D curve)
/// held where it is, or released. `None` when no ref is 3D, for the planar command.
pub(super) fn fix_3d(s: &mut Session, p: &Value, cmd: &str, refs: &[String], want: Option<bool>) -> Result<Option<Value>> {
    if s.sheet_sketching() {
        return Ok(None);
    }
    let id = sketch_store::target(s, p, cmd)?;
    let sk = sketch_store::read(&s.doc, id)?;
    let three: Vec<bool> = refs.iter().map(|r| is_3d(sk, r)).collect();
    if !three.iter().any(|t| *t) {
        return Ok(None);
    }
    if !three.iter().all(|t| *t) {
        return Err(bad(cmd, "fix 3D and planar sketch geometry in separate steps"));
    }
    let mut points = Vec::new();
    for r in refs {
        match sk.wire_point(r) {
            Some(q) => points.push(q),
            None => {
                let w = sk.wire_index(r).and_then(|i| sk.wires.get(i)).filter(|w| w.link.is_none() && !w.fit.is_empty());
                let w = w.ok_or_else(|| bad(cmd, format!("`{r}` is not a drawn 3D curve or point")))?;
                points.extend((0..w.fit.len()).map(|index| WirePoint { wire: w.id.clone(), index }));
            }
        }
    }
    let fixed = |sk: &solidcraft_sketch::Sketch, q: &WirePoint| {
        sk.wire_constraints.iter().any(|c| matches!(&c.kind, WireConstraintKind::Fix { p, .. } if p == q))
    };
    let fix = want.unwrap_or(!points.iter().all(|q| fixed(sk, q)));
    let (n, info) = edit(s, p, cmd, true, |sk, doc| {
        off_traces(doc, id, points.iter().map(|q| q.wire.as_str()), cmd)?;
        join_all(sk, doc, id)?;
        let (mut n, mut dropped) = (0, 0);
        for q in &points {
            let has = fixed(sk, q);
            if fix && !has {
                let at = sk.wire_point_at(q).ok_or_else(|| bad(cmd, format!("`{}` has no point {}", q.wire, q.index)))?;
                sk.add_wire_constraint(WireConstraintKind::Fix { p: q.clone(), at, exprs: None })?;
                n += 1;
            } else if !fix && has {
                // A point placed by expressions lets go of them too: say so.
                dropped +=
                    sk.wire_constraints.iter().filter(|c| matches!(&c.kind, WireConstraintKind::Fix { p, exprs: Some(_), .. } if p == q)).count();
                sk.wire_constraints.retain(|c| !matches!(&c.kind, WireConstraintKind::Fix { p, .. } if p == q));
                n += 1;
            }
        }
        Ok((n, dropped))
    })?;
    let (n, dropped) = n;
    let mut result = json!({"fixed": fix, "changed": n});
    if dropped > 0 {
        result["expressions_dropped"] = json!(dropped);
    }
    Ok(Some(json!({"result": result, "sketch": info})))
}

fn along_axis(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.along_axis";
    let mut lines: Vec<String> =
        p.get("lines").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
    if let Some(l) = p.get("line").and_then(Value::as_str) {
        lines.insert(0, l.to_string());
    }
    if lines.is_empty() || lines.len() > 1000 {
        return Err(bad(cmd, "`line` or `lines` must name 1 to 1000 drawn 3D lines"));
    }
    let axis = match serde_json::from_value::<AxisName>(p.get("axis").cloned().unwrap_or(Value::Null)) {
        Ok(AxisName::X) => Axis::X,
        Ok(AxisName::Y) => Axis::Y,
        Ok(AxisName::Z) => Axis::Z,
        Err(_) => return Err(bad(cmd, "`axis` must be x, y or z")),
    };
    let kinds = lines.into_iter().map(|line| WireConstraintKind::AlongAxis { line, axis }).collect();
    let (ids, info) = add_3d(s, p, cmd, kinds)?;
    Ok(json!({"constraints": ids, "sketch": info}))
}

/// Add 3D constraints in one strict edit: refused, and the sketch unchanged, when the sketch's
/// 3D constraints can no longer all hold. Drawn 3D curves that meet and are not joined yet (drawn
/// before 3D constraints, #10608) are joined first, so constraining one does not tear it from
/// the next.
fn add_3d(s: &mut Session, p: &Value, cmd: &str, kinds: Vec<WireConstraintKind>) -> Result<(Vec<String>, Value)> {
    let id = sketch_store::target(s, p, cmd)?;
    edit(s, p, cmd, true, |sk, doc| {
        off_traces(doc, id, kinds.iter().flat_map(WireConstraintKind::wires), cmd)?;
        join_all(sk, doc, id)?;
        kinds.into_iter().map(|k| sk.add_wire_constraint(k).map_err(|e| bad(cmd, e.to_string()))).collect::<Result<Vec<String>>>()
    })
}

/// Once per sketch (`Sketch::wires_joined`), every pair of drawn 3D curve ends that meet is
/// joined: curves drawn before 3D constraints were joined by nothing. After that only new curves
/// are joined, as they are drawn, so a joint deleted stays deleted.
fn join_all(sk: &mut solidcraft_sketch::Sketch, doc: &solidcraft_doc::Document, id: u64) -> Result<Vec<String>> {
    if sk.wires_joined {
        return Ok(Vec::new());
    }
    sk.wires_joined = true;
    let ids: Vec<String> = sk.wires.iter().filter(|w| w.link.is_none() && !w.fit.is_empty()).map(|w| w.id.clone()).collect();
    join_in(sk, doc, id, &ids)
}

/// The drawn 3D curves of sketch `id` that motion traces generate: the motion rewrites their
/// points, so they take no 3D constraints.
fn trace_wires(doc: &solidcraft_doc::Document, id: u64) -> std::collections::HashSet<&str> {
    doc.assembly.traces.iter().filter(|t| t.sketch == id).map(|t| t.wire.as_str()).collect()
}

/// Refused when any of `wires` is a motion trace's curve.
fn off_traces<'a>(doc: &solidcraft_doc::Document, id: u64, mut wires: impl Iterator<Item = &'a str>, cmd: &str) -> Result<()> {
    let traces = trace_wires(doc, id);
    match wires.find(|w| traces.contains(w)) {
        Some(w) => Err(bad(cmd, format!("`{w}` is a motion trace's curve, which the motion places: draw a curve on it and constrain that"))),
        None => Ok(()),
    }
}

/// [`solidcraft_sketch::Sketch::join_drawn_ends`] for curves `ids`, leaving out the joins to a
/// motion trace's curve (a solve would move it, and the motion would put it back).
pub(super) fn join_in(sk: &mut solidcraft_sketch::Sketch, doc: &solidcraft_doc::Document, id: u64, ids: &[String]) -> Result<Vec<String>> {
    // A sketch's curves drawn before 3D constraints are joined with the first one drawn after.
    let mut joints = join_all(sk, doc, id)?;
    joints.extend(sk.join_drawn_ends(ids)?);
    let traces = trace_wires(doc, id);
    if traces.is_empty() {
        return Ok(joints);
    }
    let on_traces: Vec<String> = sk
        .wire_constraints
        .iter()
        .filter(|c| joints.contains(&c.id) && c.kind.wires().iter().any(|w| traces.contains(w)))
        .map(|c| c.id.clone())
        .collect();
    sk.remove_wire_constraints(&on_traces);
    Ok(joints.into_iter().filter(|j| !on_traces.contains(j)).collect())
}

/// `sketch.fillet` on corners of drawn 3D lines (#21319), for the bends of a tube path: each
/// corner `{a, b}` of two drawn 3D lines that share an end point is rounded with a 3D arc in
/// the plane of the two lines ([`solidcraft_sketch::Sketch::fillet_drawn_lines`]). The first
/// arc carries the radius dimension, with a parameter of its own, and the others are equal to
/// it. `None` when no corner names two drawn 3D lines, for the planar command (which refuses
/// every other 3D curve).
pub(super) fn fillet_3d(s: &mut Session, p: &Value, cmd: &str, radius: &str, corners: &[Value]) -> Result<Option<FilletResult>> {
    if s.sheet_sketching() {
        return Ok(None);
    }
    // A sketch that cannot be found is the planar command's to report, in its own order.
    let Ok(id) = sketch_store::target(s, p, cmd) else { return Ok(None) };
    let Ok(sk) = sketch_store::read(&s.doc, id) else { return Ok(None) };
    let line = |q: &Value, k: &str| q.get(k).and_then(Value::as_str).filter(|id| sk.is_drawn_line(id)).map(str::to_string);
    let pairs: Vec<Option<(String, String)>> = corners.iter().map(|q| line(q, "a").zip(line(q, "b"))).collect();
    // A 3D point names no corner: lines that meet there are told apart only by naming them.
    fn text<'a>(q: &'a Value, k: &str) -> Option<&'a str> {
        q.get(k).and_then(Value::as_str)
    }
    if corners.iter().filter_map(|q| text(q, "point")).any(|r| sk.resolve_point(r).is_none() && sk.wire_point(r).is_some()) {
        return Err(bad(cmd, "a 3D corner is given as its two 3D lines, `a` and `b`, not as a point"));
    }
    if pairs.iter().all(Option::is_none) {
        return Ok(None);
    }
    if let Some((other, _)) = corners.iter().zip(&pairs).find(|(_, pair)| pair.is_none()) {
        let spatial = ["a", "b"].iter().any(|k| text(other, k).is_some_and(|id| sk.is_spatial_corner_curve(id)));
        return Err(bad(
            cmd,
            if spatial {
                "not supported yet: filleting 3D or linked sketch curves, other than two drawn 3D lines that share an end point"
            } else {
                "round 3D corners and corners on the sketch plane in separate steps"
            },
        ));
    }
    let pairs: Vec<(String, String)> = pairs.into_iter().flatten().collect();
    let said = |e: solidcraft_sketch::SketchError| match e {
        solidcraft_sketch::SketchError::Invalid(why) => bad(cmd, why),
        e => e.into(),
    };
    let ((curves, constraints, param, virtual_corners, moved), info) = edit(s, p, cmd, true, |sk, doc| {
        let r = doc.eval(radius, solidcraft_doc::expr::Kind::Length).map_err(|e| bad(cmd, format!("radius: {e}")))?;
        if !(r > 1e-9 && r < 1e8) {
            return Err(bad(cmd, "radius must be positive"));
        }
        off_traces(doc, id, pairs.iter().flat_map(|(a, b)| [a.as_str(), b.as_str()]), cmd)?;
        join_all(sk, doc, id)?;
        // Rounded as the design's parameters place the lines now: the stored sketch keeps the
        // geometry of its last edit, from before a parameter changed.
        doc.apply_wire_values(&doc.param_values().0, sk)?;
        sk.solve_wires_with(None, false);
        let param = doc.new_model_param(radius, "mm")?;
        let (mut arcs, mut constraints, mut corners, mut moved): (Vec<String>, Vec<String>, Vec<String>, Vec<String>) = Default::default();
        for (a, b) in &pairs {
            let made = sk.fillet_drawn_lines(a, b, r).map_err(said)?;
            constraints.extend(made.tangents);
            // One radius for them all: the first carries the dimension, the rest are equal to it.
            constraints.push(
                match arcs.first() {
                    None => sk.add_wire_dimension(WireConstraintKind::Radius { arc: made.arc.clone(), value: r }, Some(param.clone()), false),
                    Some(first) => sk.add_wire_constraint(WireConstraintKind::Equal { a: first.clone(), b: made.arc.clone(), arcs: true }),
                }
                .map_err(said)?,
            );
            arcs.push(made.arc);
            corners.extend(made.corner);
            // A constraint that held two of the corners (the length of the line between them)
            // is named once.
            for id in made.moved {
                if !moved.contains(&id) {
                    moved.push(id);
                }
            }
        }
        Ok((arcs, constraints, param, corners, moved))
    })?;
    Ok(Some(FilletResult {
        curves,
        constraints,
        param: Some(param),
        expression: None,
        virtual_corners,
        moved_to_virtual_corner: moved,
        offset_released: Vec::new(),
        sketch: info,
        created: None,
        param_warnings: Vec::new(),
    }))
}

/// `sketch.constraint.parallel`, `.perpendicular`, `.collinear`, `.equal` and `.tangent` on 3D
/// refs: two drawn 3D lines (equal: or two drawn 3D arcs; tangent: a line and an arc that meet;
/// parallel: or a line and an origin plane, `xy`, `yz` or `xz`). `None` when neither operand is
/// 3D, for the planar command.
pub(super) fn relation_3d(s: &mut Session, p: &Value, cmd: &str) -> Result<Option<Constrained>> {
    if s.sheet_sketching() {
        return Ok(None);
    }
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
    // A sketch that cannot be found is the planar command's to report, in its own order.
    let Ok(id) = sketch_store::target(s, p, cmd) else { return Ok(None) };
    let Ok(sk) = sketch_store::read(&s.doc, id) else { return Ok(None) };
    let three = |k: &str| text(k).is_some_and(|r| is_3d(sk, &r));
    if !three("a") && !three("b") {
        return Ok(None);
    }
    // The side that is not 3D geometry may be a plane (an origin or construction plane, or a
    // planar model face).
    let (line_key, plane_key) = if three("a") { ("a", "b") } else { ("b", "a") };
    let plane = match p.get(plane_key) {
        Some(v) if !three(plane_key) => plane_target(s, id, v, cmd)?,
        _ => None,
    };
    let sk = sketch_store::read(&s.doc, id)?;
    let line_of = text(line_key).unwrap_or_default();
    let other = p.get(plane_key).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).unwrap_or_default();
    let (Some(a), Some(b)) =
        (text("a").or_else(|| (line_key == "b").then(|| other.clone())), text("b").or_else(|| (line_key == "a").then(|| other.clone())))
    else {
        return Ok(None);
    };
    let kind = match (cmd, plane) {
        ("sketch.constraint.parallel", Some(plane)) if sk.is_drawn_line(&line_of) => WireConstraintKind::ParallelToPlane { line: line_of, plane },
        ("sketch.constraint.parallel", Some(_)) => {
            return Err(bad(cmd, format!("parallel to the plane `{other}` takes a drawn 3D line (sketch.constraint.on_plane holds a point on it)")));
        }
        (_, Some(_)) => {
            return Err(bad(
                cmd,
                format!(
                    "`{other}`: of the relations, only Parallel takes a plane (a line square to an origin plane is sketch.constraint.along_axis; a point on a plane, sketch.constraint.on_plane)"
                ),
            ));
        }
        // One of the lines may be fixed (#10616): a straight included model edge or a 2D line.
        ("sketch.constraint.parallel", None) if sk.is_line_operand(&a) && sk.is_line_operand(&b) => WireConstraintKind::Parallel { a, b },
        ("sketch.constraint.perpendicular", None) if sk.is_line_operand(&a) && sk.is_line_operand(&b) => WireConstraintKind::Perpendicular { a, b },
        ("sketch.constraint.collinear", None) if sk.is_line_operand(&a) && sk.is_line_operand(&b) => WireConstraintKind::Collinear { a, b },
        ("sketch.constraint.equal", None) => sk.equal_kind(&a, &b).ok_or_else(|| bad(cmd, "Equal takes two drawn 3D lines or two drawn 3D arcs"))?,
        ("sketch.constraint.tangent", None) => sk
            .tangent_kind(&a, &b)
            .or_else(|| sk.tangent_kind(&b, &a))
            .ok_or_else(|| bad(cmd, "Tangent takes a drawn 3D line and a drawn 3D arc that meet at an end (join them first)"))?,
        _ => {
            return Err(bad(
                cmd,
                "this takes two lines, at least one a drawn 3D line (the other may be an included straight model edge or a 2D line)",
            ));
        }
    };
    let (ids, info) = add_3d(s, p, cmd, vec![kind])?;
    let constraint = ids.into_iter().next().ok_or_else(|| bad(cmd, "no 3D constraint was added"))?;
    Ok(Some(Constrained { constraint, sketch: info, redundant: None, determined_by: None }))
}

fn on_plane(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.on_plane";
    let mut refs: Vec<String> =
        p.get("points").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
    if let Some(r) = p.get("point").and_then(Value::as_str) {
        refs.insert(0, r.to_string());
    }
    if refs.is_empty() || refs.len() > 1000 {
        return Err(bad(cmd, "`point` or `points` must name 1 to 1000 3D points"));
    }
    let id = sketch_store::target(s, p, cmd)?;
    let plane = plane_target(s, id, p.get("plane").unwrap_or(&Value::Null), cmd)?
        .ok_or_else(|| bad(cmd, "`plane` must be xy, yz or xz, a construction plane's name, or {face: [x,y,z], body?} on a planar model face"))?;
    let sk = sketch_store::read(&s.doc, id)?;
    let kinds = refs
        .iter()
        .map(|r| {
            sk.wire_point(r)
                .map(|q| WireConstraintKind::OnPlane { p: q, plane: plane.clone() })
                .ok_or_else(|| bad(cmd, format!("`{r}` is not a 3D point")))
        })
        .collect::<Result<Vec<_>>>()?;
    let (ids, info) = add_3d(s, p, cmd, kinds)?;
    Ok(json!({"constraints": ids, "sketch": info}))
}

/// `sketch.dimension` on 3D refs: a drawn 3D line's length, the distance between two 3D points
/// (or along X, Y or Z only), the angle between two drawn 3D lines, or a drawn 3D arc's radius.
/// `None` when no entity is 3D, for the planar command.
pub(super) fn dimension_3d(s: &mut Session, p: &Value, cmd: &str) -> Result<Option<DimensionResult>> {
    if s.sheet_sketching() {
        return Ok(None);
    }
    let given: Vec<Value> = p.get("entities").and_then(Value::as_array).cloned().unwrap_or_default();
    let ents: Vec<String> = given.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    // A sketch that cannot be found is the planar command's to report, in its own order.
    let Ok(id) = sketch_store::target(s, p, cmd) else { return Ok(None) };
    let Ok(stored) = sketch_store::read(&s.doc, id) else { return Ok(None) };
    if !ents.iter().any(|r| is_3d(stored, r)) {
        return Ok(None);
    }
    if given.len() > 2 {
        return Err(bad(cmd, "dimension one or two entities, at least one of them 3D sketch geometry"));
    }
    // Beside a 3D entity, the other may be a plane: the distance from it, or the angle to it
    // (an origin or construction plane, or {face: [x,y,z], body?}, a planar model face, #10616).
    let mut to_plane = None;
    if let [x, y] = given.as_slice() {
        let three = |v: &Value| v.as_str().is_some_and(|r| is_3d(stored, r));
        for (entity, plane) in [(x, y), (y, x)] {
            if three(entity) && !three(plane) && to_plane.is_none() {
                to_plane = plane_target(s, id, plane, cmd)?.map(|pl| (entity.as_str().unwrap_or_default().to_string(), pl));
            }
        }
    }
    let stored = sketch_store::read(&s.doc, id)?;
    if to_plane.is_none() && ents.len() != given.len() {
        return Err(bad(
            cmd,
            "a model entity beside a 3D one: include model edges with sketch.include_3d and dimension the included curve, or give a plane",
        ));
    }
    // Measured as the design's parameters solve it now, with the 2D geometry and model planes
    // where they are: the stored sketch keeps the geometry of its last edit.
    let mut solved = stored.clone();
    s.doc.apply_wire_values(&s.doc.param_values().0, &mut solved)?;
    let (vals, st, plane, frame) = super::sketch_project::wire_context(s, id, &[]).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
    let doc = s.design_document();
    let attachment = doc.feature(id).and_then(|feature| match &feature.kind {
        solidcraft_doc::FeatureKind::Sketch { plane, .. } => super::sketch::face::anchor(plane),
        _ => None,
    });
    solidcraft_doc::project::refresh_links_at(&doc, &vals, &st, &plane, &mut solved, attachment, Some(&frame))
        .map_err(|e| bad(cmd, e.to_string()))?;
    if solved.links.iter().any(|link| link.lost) {
        return Err(bad(cmd, "a linked reference is lost; repair it before measuring a new 3D dimension"));
    }
    drop(doc);
    let counted = solved.solve_wires(None);
    // A count the work ran out on says nothing about redundancy.
    let dof_before = (!counted.uncounted).then_some(counted.dof);
    let sk = &solved;
    let ty = p.get("type").and_then(Value::as_str).unwrap_or("auto").to_ascii_lowercase();
    let axis = match ty.as_str() {
        "distance_x" => Some(Axis::X),
        "distance_y" => Some(Axis::Y),
        "distance_z" => Some(Axis::Z),
        _ => None,
    };
    let ends = |l: &str| (sk.wire_point(&format!("{l}.start")), sk.wire_point(&format!("{l}.end")));
    let pair = match ents.as_slice() {
        [l] if sk.is_drawn_line(l) => match ends(l) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        },
        // Either point may be fixed (#10616): an included model point or a 2D point.
        [a, b] => sk.point_operand(a).zip(sk.point_operand(b)),
        _ => None,
    };
    let kind = if let Some((r, plane)) = to_plane.clone() {
        match (sk.wire_point(&r), sk.is_drawn_line(&r)) {
            (Some(q), _) if matches!(ty.as_str(), "auto" | "distance" | "plane_distance") => {
                WireConstraintKind::PlaneDistance { p: q, plane, reversed: false, value: 0.0 }
            }
            (None, true) if matches!(ty.as_str(), "auto" | "angle" | "plane_angle") => {
                WireConstraintKind::PlaneAngle { line: r, plane, reversed: false, value: 0.0 }
            }
            _ => return Err(bad(cmd, "to a plane: a 3D point (distance) or a drawn 3D line (angle)")),
        }
    } else {
        match (ents.as_slice(), pair, axis) {
            (_, Some((a, b)), Some(axis)) if matches!(ty.as_str(), "distance_x" | "distance_y" | "distance_z") => {
                WireConstraintKind::AxisDistance { a, b, axis, reversed: false, value: 0.0 }
            }
            (_, Some((a, b)), None) if matches!(ty.as_str(), "auto" | "distance" | "length") => WireConstraintKind::Distance { a, b, value: 0.0 },
            ([a, b], None, None) if matches!(ty.as_str(), "auto" | "angle") && sk.is_drawn_line(a) && sk.is_drawn_line(b) => {
                sk.angle_kind(a, b).ok_or_else(|| bad(cmd, "an angle needs two drawn 3D lines"))?
            }
            ([arc], None, None) if matches!(ty.as_str(), "auto" | "radius") => WireConstraintKind::Radius { arc: arc.clone(), value: 0.0 },
            _ => {
                return Err(bad(
                    cmd,
                    "3D dimensions: a drawn 3D line (length; distance_x/y/z for its run along an axis), two points at least one a 3D point (distance, or distance_x/y/z; the other may be an included model point or a 2D point), two drawn 3D lines (angle), a drawn 3D arc (radius), or a 3D point or line and a plane (xy/yz/xz, a construction plane's name, or {face: [x,y,z], body?})",
                ));
            }
        }
    };
    let is_angle = kind.is_angle();
    let mut kind = kind;
    let side = sk.wire_dimension_reversed(&kind).ok_or_else(|| bad(cmd, "that cannot be measured"))?;
    match &mut kind {
        WireConstraintKind::AxisDistance { reversed, .. }
        | WireConstraintKind::PlaneDistance { reversed, .. }
        | WireConstraintKind::PlaneAngle { reversed, .. } => *reversed = side,
        _ => {}
    }
    let cur = sk.measure_wire_dimension(&kind).ok_or_else(|| bad(cmd, "that cannot be measured"))?;
    kind.set_value(cur);
    let driven = p.get("driven").and_then(Value::as_bool).unwrap_or(false);
    let value = crate::params::expr(p, "value");
    let defines = match value.as_ref().and_then(|v| solidcraft_doc::expr::definition(v.as_str())) {
        Some((name, rhs)) => {
            if rhs.is_empty() {
                return Err(bad(cmd, format!("value: `{name} =` needs a value after the `=`")));
            }
            if driven {
                return Err(bad(cmd, format!("value: a driven dimension has no value, so it cannot define `{name}`")));
            }
            Some((name.to_string(), rhs.to_string()))
        }
        None => None,
    };
    let start = s.doc.clone();
    let measured = |v: f64| if is_angle { (round6(v.to_degrees()), "deg") } else { (round6(v), "mm") };
    let ((cid, param), info) = edit(s, p, cmd, true, |sk, doc| {
        if driven {
            let cid = sk.add_wire_dimension(kind, None, true).map_err(|e| bad(cmd, e.to_string()))?;
            return Ok((cid, None));
        }
        let unit_kind = if is_angle { solidcraft_doc::expr::Kind::Angle } else { solidcraft_doc::expr::Kind::Length };
        // At full precision: a rounded angle is off by more than an angle on long lines holds to.
        let (unit, default_expr) = if is_angle { ("deg".to_string(), format!("{} deg", cur.to_degrees())) } else { doc.design_length(cur) };
        let expression = match &defines {
            Some((name, rhs)) => {
                if let Some(problem) = solidcraft_doc::Document::param_name_problem(name) {
                    return Err(bad(cmd, format!("value: {problem}")));
                }
                if doc.param(name).is_some() {
                    return Err(bad(
                        cmd,
                        format!("value: there is already a parameter `{name}`: type `{name}` to use it, or change it in Parameters"),
                    ));
                }
                let rhs = doc.with_design_unit(rhs, unit_kind);
                let unit = if is_angle { "deg".to_string() } else { solidcraft_doc::expr::length_unit_or_mm(&doc.units).to_string() };
                doc.add_user_param(name, &rhs, Some(&unit), None).map_err(|e| bad(cmd, format!("value: {e}")))?;
                name.clone()
            }
            None => value.as_ref().map(|e| doc.with_design_unit(e, unit_kind)).unwrap_or(default_expr),
        };
        let pname = doc.new_model_param(&expression, &unit)?;
        let (vals, errs) = doc.param_values();
        if let Some(err) = errs.get(&pname) {
            return Err(bad(cmd, format!("value: {err}")));
        }
        // A value the dimension cannot take, said of the value (the dimension has no id yet).
        if let Some(v) = vals.get(&pname) {
            let v = v.to_kind(unit_kind).map_err(|e| bad(cmd, format!("value: {e}")))?;
            let mut with = kind.clone();
            with.set_value(v);
            sk.validate_wire_constraint(&with).map_err(|e| bad(cmd, format!("value: {e}")))?;
        }
        let cid = sk.add_wire_dimension(kind, Some(pname.clone()), false).map_err(|e| bad(cmd, e.to_string()))?;
        Ok((cid, Some(pname)))
    })?;
    let sk = sketch_store::read(&s.doc, id)?;
    // A dimension the other 3D constraints already determine removes no freedom: refused, or
    // with `driven: true` placed as a reference dimension instead; with `if_redundant: "driven"`
    // it is placed so without asking.
    let after = sk.clone().solve_wires(None);
    if !driven && dof_before.is_some_and(|before| !after.uncounted && after.dof >= before) {
        s.doc = start;
        if p.get("if_redundant").and_then(Value::as_str).is_some_and(|m| m.eq_ignore_ascii_case("driven")) {
            let mut q = p.clone();
            q["driven"] = json!(true);
            if let Some(o) = q.as_object_mut() {
                o.remove("if_redundant");
                o.remove("value");
            }
            return Ok(dimension_3d(s, &q, cmd)?.map(|d| DimensionResult { redundant: Some(true), ..d }));
        }
        return Err(bad(
            cmd,
            "that dimension is already determined by the sketch's other 3D constraints; add it as a driven (reference) dimension with driven: true",
        ));
    }
    let now = sk.wire_constraints.iter().find(|c| c.id == cid).and_then(|c| sk.measure_wire_dimension(&c.kind)).unwrap_or(cur);
    let (m, unit) = measured(now);
    let type_name = match ty.as_str() {
        "distance_x" | "distance_y" | "distance_z" => ty.clone(),
        _ if to_plane.is_some() && is_angle => "plane_angle".into(),
        _ if to_plane.is_some() => "plane_distance".into(),
        _ if is_angle => "angle".into(),
        _ if ents.len() == 1 && ents.first().is_some_and(|e| sk.is_drawn_line(e)) => "length".into(),
        _ if ents.len() == 1 => "radius".into(),
        _ => "distance".into(),
    };
    let expression = param.as_ref().and_then(|n| s.doc.param(n)).map(|x| x.expr.clone());
    Ok(Some(DimensionResult {
        constraint: Some(cid),
        param,
        driven: driven.then_some(true),
        kind: type_name,
        expression,
        measured: m,
        unit: unit.into(),
        sketch: info,
        scaled: None,
        axis: None,
        defined: defines.map(|(name, _)| name),
        redundant: None,
        determined_by: None,
        created: None,
        param_warnings: Vec::new(),
    }))
}

/// `sketch.toggle_driven` on a 3D dimension: driven (a reference that measures) or driving
/// again, with a parameter of the value it measures; one the other 3D constraints already
/// determine stays driven. `None` when `cid` is not a 3D constraint, for the planar command.
pub(super) fn toggle_driven_3d(s: &mut Session, p: &Value, cmd: &str, cid: &str, want: Option<bool>) -> Result<Option<Value>> {
    if s.sheet_sketching() {
        return Ok(None);
    }
    let id = sketch_store::target(s, p, cmd)?;
    let sk = sketch_store::read(&s.doc, id)?;
    let Some(c) = sk.wire_constraints.iter().find(|c| c.id == cid).cloned() else { return Ok(None) };
    let v = c.kind.value().ok_or_else(|| bad(cmd, format!("`{cid}` is not a dimension")))?;
    let driven = want.unwrap_or(!c.driven);
    let freed = {
        let mut without = sk.clone();
        if let Some(slot) = without.wire_constraints.iter_mut().find(|c| c.id == cid) {
            slot.driven = true;
        }
        without.solve_wires(None).dof
    };
    let start = s.doc.clone();
    let (_, info) = edit(s, p, cmd, true, |sk, doc| {
        let current = sk.wire_constraints.iter().find(|c| c.id == cid).ok_or_else(|| bad(cmd, format!("no constraint `{cid}`")))?;
        let side = sk.wire_dimension_reversed(&current.kind).ok_or_else(|| bad(cmd, "that cannot be measured"))?;
        let v = sk.measure_wire_dimension(&current.kind).unwrap_or(v);
        let slot = sk.wire_constraints.iter_mut().find(|c| c.id == cid).ok_or_else(|| bad(cmd, format!("no constraint `{cid}`")))?;
        // Only as it turns driving: a driving dimension keeps its stored side.
        if let (false, true, WireConstraintKind::PlaneAngle { reversed, .. }) = (driven, slot.driven, &mut slot.kind) {
            *reversed = side;
        }
        if driven != slot.driven {
            slot.driven = driven;
            slot.param = if driven {
                None
            } else {
                let (unit, e) = if slot.kind.is_angle() { ("deg".into(), format!("{} deg", v.to_degrees())) } else { doc.design_length(v) };
                Some(doc.new_model_param(&e, &unit)?)
            };
        }
        Ok(())
    })?;
    if !driven && c.driven && sketch_store::read(&s.doc, id)?.clone().solve_wires(None).dof >= freed {
        s.doc = start;
        return Err(bad(cmd, "that dimension is already determined by the sketch's other 3D constraints; it stays driven"));
    }
    s.doc_mut().prune_model_params();
    Ok(Some(json!({"constraint": cid, "driven": driven, "sketch": info})))
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// Turn the curves `curves` of the target 3D sketch into drawn 3D curves through the same places,
/// moved by `offset` (world). The points `points` move by `offset` too, each on its own: a lone
/// point becomes a drawn 3D point, and the lines and fit-point splines through one that isn't on
/// a moved curve become drawn 3D curves with just that point moved. Returns the new drawn curves.
pub(super) fn lift(s: &mut Session, p: &Value, cmd: &str, curves: &[String], points: &[String], offset: Vec3) -> Result<(Vec<String>, Value)> {
    let id = sketch_store::target(s, p, cmd)?;
    let st = s.world_state()?;
    let plane = st.sketch(id).map(|ss| ss.plane).ok_or_else(|| bad(cmd, "the sketch has no plane"))?;
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let comp = move |q: Vec2| solidcraft_doc::apply_point(&inverse, plane.to_world(q));
    let shift = solidcraft_doc::apply_vector(&inverse, offset);
    edit(s, p, cmd, false, |sk, doc| {
        needs_3d(sk, cmd)?;
        let mut which = Vec::new();
        for c in curves {
            which.push(sk.curve_index(c).ok_or_else(|| bad(cmd, format!("no curve `{c}`")))?);
        }
        which.sort_unstable();
        which.dedup();
        // Points moved on their own: lone ones, and ones on curves that stay where they are
        // (those curves are lifted in place, then just the point moves).
        let mut alone: Vec<(String, Vec3)> = Vec::new();
        let mut on_curves: Vec<usize> = Vec::new();
        let mut in_place: Vec<usize> = Vec::new();
        for r in points {
            let pi = sk.resolve_point(r).filter(|i| *i != 0).ok_or_else(|| bad(cmd, format!("no point `{r}`")))?;
            let through: Vec<usize> = sk.curves.iter().enumerate().filter(|(_, c)| c.kind.uses(pi)).map(|(i, _)| i).collect();
            if through.is_empty() {
                let q = sk.points.get(pi).ok_or_else(|| bad(cmd, "the point is missing"))?;
                if !alone.iter().any(|(id, _)| *id == q.id) {
                    alone.push((q.id.clone(), comp(q.pos)));
                }
            } else if !through.iter().all(|c| which.contains(c)) {
                on_curves.push(pi);
                in_place.extend(through.into_iter().filter(|c| !which.contains(c)));
            }
        }
        in_place.sort_unstable();
        in_place.dedup();
        let moved = which.len();
        let mut all = which.clone();
        all.extend(in_place.iter().copied());
        // Each curve as a drawn 3D curve: its point ids, and its fit points in component space.
        let mut lifted: Vec<(Vec<usize>, Vec<Vec3>)> = Vec::new();
        for &ci in &all {
            let ids = liftable(&sk.curves[ci], cmd)?;
            let fit = ids
                .iter()
                .map(|i| sk.points.get(*i).map(|q| comp(q.pos)).ok_or_else(|| bad(cmd, "a curve's point is missing")))
                .collect::<Result<Vec<Vec3>>>()?;
            lifted.push((ids, fit));
        }
        sk.remove_curves(&all);
        let gone: Vec<usize> = alone.iter().filter_map(|(id, _)| sk.point_index(id)).collect();
        sk.remove_points(&gone);
        let mut made = Vec::new();
        // Where each point moved on its own sits on the drawn curves (curve id, fit index).
        let mut placed: Vec<(usize, String, usize)> = Vec::new();
        for (n, (ids, fit)) in lifted.iter().enumerate() {
            let fit: Vec<Vec3> = if n < moved { fit.iter().map(|q| *q + shift).collect() } else { fit.clone() };
            let w = sk.add_drawn_wire_in(&fit, WireSpace::Component)?;
            for (k, q) in ids.iter().enumerate() {
                if n >= moved && on_curves.contains(q) {
                    placed.push((*q, w.clone(), k));
                }
            }
            made.push(w);
        }
        for (_, q) in &alone {
            made.push(sk.add_drawn_wire_in(&[*q + shift], WireSpace::Component)?);
        }
        // Each point on curves lifted in place moves by the offset, once (the first curve it is
        // on; the drawn curves through it stay joined there).
        let mut done: Vec<usize> = Vec::new();
        for (pi, w, k) in &placed {
            if done.contains(pi) {
                continue;
            }
            done.push(*pi);
            if let Some(now) = sk.wire_index(w).and_then(|i| sk.wires.get(i)).and_then(|x| x.fit.get(*k)).copied() {
                sk.move_drawn_point(w, *k, now + shift)?;
            }
        }
        join_in(sk, doc, id, &made)?;
        Ok(made)
    })
}

/// Moves drawn 3D picks along the plane as `f` moves plane geometry, and by `z` along its normal.
pub(super) fn move_drawn(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    picks: &[(String, Option<usize>)],
    z: f64,
    f: &dyn Fn(Vec2) -> Vec2,
) -> Result<(usize, Value)> {
    let id = sketch_store::target(s, p, cmd)?;
    let world = s.world_state()?.sketch(id).map(|ss| ss.plane).ok_or_else(|| bad(cmd, "the sketch has no plane"))?;
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let component = solidcore_record::Plane {
        origin: solidcraft_doc::apply_point(&inverse, world.origin),
        x: solidcraft_doc::apply_vector(&inverse, world.x),
        y: solidcraft_doc::apply_vector(&inverse, world.y),
    };
    edit(s, p, cmd, false, |sk, _| {
        let mut moves = Vec::new();
        for (wire, index) in picks {
            let drawn = sk
                .wire_index(wire)
                .and_then(|w| sk.wires.get(w))
                .filter(|w| !w.fit.is_empty() && w.link.is_none())
                .ok_or_else(|| bad(cmd, format!("`{wire}` is not an editable drawn 3D curve")))?;
            let plane = if drawn.space == WireSpace::Component { component } else { world };
            let which: Vec<usize> = match index {
                Some(i) if *i < drawn.fit.len() => vec![*i],
                Some(i) => return Err(bad(cmd, format!("`{wire}` has no drawn point {i}"))),
                None => (0..drawn.fit.len()).collect(),
            };
            let moved: Vec<(usize, Vec3)> = which
                .into_iter()
                .filter_map(|i| drawn.fit.get(i).map(|q| (i, *q)))
                .map(|(i, q)| (i, plane.to_world(f(plane.to_local(q))) + plane.normal() * (plane.height(q) + z)))
                .collect();
            moves.extend(moved.into_iter().map(|(i, to)| (wire.clone(), i, to)));
        }
        if let Some((wire, index, _)) = moves.iter().find(|(w, i, _)| sk.wire_point_fixed(&WirePoint { wire: w.clone(), index: *i })) {
            return Err(bad(cmd, format!("`{wire}` point {index} is fixed, or joined to a fixed point (unfix it to move it)")));
        }
        let held_before = sk.wire_constraints_holding();
        sk.move_drawn_points(&moves)?;
        if !held_before.is_empty() {
            let held_after = sk.wire_constraints_holding();
            if held_before.iter().any(|id| !held_after.contains(id)) {
                return Err(bad(cmd, "the sketch's 3D constraints cannot hold the moved points there"));
            }
        }
        Ok(moves.len())
    })
}

#[cfg(test)]
#[path = "sketch_3d_frame_tests.rs"]
mod frame_tests;
