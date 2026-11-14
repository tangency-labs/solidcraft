//! 3D sketch drawing: with the sketch's 3D Sketch option on (Sketch Palette), lines, arcs,
//! splines and points may leave the sketch plane. They are kept as drawn 3D curves (wires with
//! fit points, exact arcs keeping their analytic form): drawn and snapped to, movable point by
//! point, and usable as sweep and pipe paths.

use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{CurveKind, WireSpace};

use super::sketch::edit;
use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, req_vec3, vec3};
use crate::schema::sketch_3d::{Curves3dResult, Line3dParams, Move3dParams, Move3dResult, Point3dParams, Point3dResult, Spline3dParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const LINE3D_CONTRACT: TypedContract<Line3dParams> = TypedContract::new();
const SPLINE3D_CONTRACT: TypedContract<Spline3dParams> = TypedContract::new();
const POINT3D_CONTRACT: TypedContract<Point3dParams> = TypedContract::new();
const MOVE3D_CONTRACT: TypedContract<Move3dParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.line3d", "3D Line", crate::checked!("sketch.line3d", line3d))
        .icon("line")
        .enabled(in_sketch_3d)
        .contract(&LINE3D_CONTRACT)
        .output(output_schema_for::<Curves3dResult>)
        .params(concat!(
            "points: [[x,y,z]…] world points (2 or more): one 3D line per segment, joined end to end (needs 3D Sketch on)",
            created_doc!()
        )),
    CommandSpec::new("sketch.spline3d", "3D Spline", crate::checked!("sketch.spline3d", spline3d))
        .icon("spline")
        .enabled(in_sketch_3d)
        .contract(&SPLINE3D_CONTRACT)
        .output(output_schema_for::<Curves3dResult>)
        .params(concat!("points: [[x,y,z]…] world fit points (2 or more): a smooth 3D curve through them (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.arc3d", "3D Arc", arc3d)
        .icon("arc3")
        .enabled(in_sketch_3d)
        .params(concat!(
            "start, through, end: [x,y,z] world points: an exact circular arc through them, from start through through to end (needs 3D Sketch on). Stored exactly (centre, radius, angles), so path consumers can treat it as a true arc",
            created_doc!()
        )),
    CommandSpec::new("sketch.point3d", "3D Point", crate::checked!("sketch.point3d", point3d))
        .icon("point")
        .enabled(in_sketch_3d)
        .contract(&POINT3D_CONTRACT)
        .output(output_schema_for::<Point3dResult>)
        .params(concat!("point: [x,y,z] world point (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.move3d", "Move 3D Point", crate::checked!("sketch.move3d", move3d))
        .icon("sketch_move")
        .enabled(in_sketch)
        .contract(&MOVE3D_CONTRACT)
        .output(output_schema_for::<Move3dResult>)
        .params("wire: drawn 3D curve or point id, index?: fit point (default 0) | point: a point on the sketch plane (`l1.end`, `p3`: a line's end, a fit point of a spline, a lone point; needs 3D Sketch on); to: [x,y,z] | by: [dx,dy,dz] (world); points joined to it move too. A `point` on the plane first turns the lines and fit-point splines through it (or the lone point) into drawn 3D curves through the same places, which `curves` lists (`wire` and `index` say where the point is now); their constraints and dimensions go. Arcs, circles, ellipses, conics and control-point or closed splines through it are refused"),
];

/// A command's reply as JSON: built from the type its output schema describes, so the two can't
/// drift apart.
fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

fn points(cmd: &str, p: &Value, min: usize) -> Result<Vec<Vec3>> {
    let a = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must list [x,y,z] points"))?;
    if a.len() < min || a.len() > solidcraft_sketch::MAX_FIT_POINTS {
        return Err(bad(cmd, format!("`points` needs {min} to {} points", solidcraft_sketch::MAX_FIT_POINTS)));
    }
    a.iter().map(|v| vec3(v).ok_or_else(|| bad(cmd, "a point must be [x, y, z]"))).collect()
}

fn needs_3d(sk: &solidcraft_sketch::Sketch, cmd: &str) -> Result<()> {
    if sk.view.three_d { Ok(()) } else { Err(bad(cmd, "turn on 3D Sketch (sketch.options sketch_3d: true) to draw off the sketch plane")) }
}

/// The 3D Line, Spline, Arc and Point buttons start interactive tools, so they are only offered
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

fn line3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.line3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let pts: Vec<Vec3> = points(cmd, p, 2)?.into_iter().map(|at| solidcraft_doc::apply_point(&inverse, at)).collect();
    let (ids, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        pts.windows(2).map(|w| sk.add_drawn_wire_in(w, WireSpace::Component).map_err(Into::into)).collect::<Result<Vec<String>>>()
    })?;
    reply(cmd, Curves3dResult { curves: ids, sketch: info, created: None, param_warnings: Vec::new() })
}

fn spline3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.spline3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let pts: Vec<Vec3> = points(cmd, p, 2)?.into_iter().map(|at| solidcraft_doc::apply_point(&inverse, at)).collect();
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_wire_in(&pts, WireSpace::Component)?)
    })?;
    reply(cmd, Curves3dResult { curves: vec![id], sketch: info, created: None, param_warnings: Vec::new() })
}

fn arc3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.arc3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let pts: Vec<Vec3> = ["start", "through", "end"]
        .iter()
        .map(|k| req_vec3(cmd, p, k))
        .collect::<Result<Vec<Vec3>>>()?
        .into_iter()
        .map(|at| solidcraft_doc::apply_point(&inverse, at))
        .collect();
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_arc_in(&pts, WireSpace::Component)?)
    })?;
    Ok(json!({"curves": [id], "sketch": info}))
}

fn point3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.point3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let at = solidcraft_doc::apply_point(&inverse, req_vec3(cmd, p, "point")?);
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_wire_in(&[at], WireSpace::Component)?)
    })?;
    reply(cmd, Point3dResult { point: id, sketch: info, created: None, param_warnings: Vec::new() })
}

fn move3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move3d";
    if let Some(point) = p.get("point") {
        if p.get("wire").is_some() {
            return Err(bad(cmd, "give `wire` or `point`, not both"));
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
        Ok(sk.move_drawn_point(&wire, index, to)?)
    })?;
    reply(cmd, Move3dResult { moved: n, curves: Vec::new(), wire: None, index: None, sketch: info, param_warnings: Vec::new() })
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
    let ((n, curves, k), info) = edit(s, p, cmd, false, |sk, _| {
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
        let through: Vec<usize> = sk.curves.iter().enumerate().filter(|(_, c)| c.kind.uses(pi)).map(|(i, _)| i).collect();
        let mut fits = Vec::new();
        for &ci in &through {
            let ids = liftable(&sk.curves[ci], cmd)?;
            let fit = ids
                .iter()
                .map(|i| sk.points.get(*i).map(|q| place(q.pos)).ok_or_else(|| bad(cmd, "a curve's point is missing")))
                .collect::<Result<Vec<Vec3>>>()?;
            fits.push(fit);
        }
        if through.is_empty() {
            fits.push(vec![from]);
            sk.remove_points(&[pi]);
        } else {
            sk.remove_curves(&through);
        }
        let mut made = Vec::new();
        for fit in &fits {
            made.push(sk.add_drawn_wire_in(fit, WireSpace::Component)?);
        }
        let k = fits[0].iter().position(|q| q.dist(from) < 1e-9).unwrap_or(0);
        let n = sk.move_drawn_point(&made[0], k, goal)?;
        Ok((n, made, k))
    })?;
    let wire = curves.first().cloned();
    reply(cmd, Move3dResult { moved: n, curves, wire, index: Some(k), sketch: info, param_warnings: Vec::new() })
}

#[cfg(test)]
#[path = "sketch_3d_frame_tests.rs"]
mod frame_tests;
