//! 3D sketch drawing: with the sketch's 3D Sketch option on (Sketch Palette), lines, arcs,
//! splines and points may leave the sketch plane. They are kept as drawn 3D curves (wires with
//! fit points, exact arcs keeping their analytic form): drawn and snapped to, movable point by
//! point, and usable as sweep and pipe paths.
//!
//! Their fit points take constraints of their own (#10608), solved after the planar sketch:
//! Coincident and Fix take 3D refs (`<curve>.start`, `.end`, `.through`, `.<n>`, or a 3D point's
//! id), and Along Axis holds a 3D line parallel to an axis.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;
use solidcraft_sketch::{Axis, WireConstraintKind, WirePoint, WireSpace};

use super::sketch::edit;
use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, req_vec3, vec3};
use crate::schema::sketch_3d::{AlongAxisParams, AlongAxisResult, AxisName};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const ALONG_AXIS_CONTRACT: TypedContract<AlongAxisParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.line3d", "3D Line", line3d).icon("line").enabled(in_sketch_3d).params(concat!(
        "points: [[x,y,z]…] world points (2 or more): one 3D line per segment, joined end to end (needs 3D Sketch on)",
        created_doc!()
    )),
    CommandSpec::new("sketch.spline3d", "3D Spline", spline3d)
        .icon("spline")
        .enabled(in_sketch_3d)
        .params(concat!("points: [[x,y,z]…] world fit points (2 or more): a smooth 3D curve through them (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.arc3d", "3D Arc", arc3d)
        .icon("arc3")
        .enabled(in_sketch_3d)
        .params(concat!(
            "start, through, end: [x,y,z] world points: an exact circular arc through them, from start through through to end (needs 3D Sketch on). Stored exactly (centre, radius, angles), so path consumers can treat it as a true arc",
            created_doc!()
        )),
    CommandSpec::new("sketch.point3d", "3D Point", point3d)
        .icon("point")
        .enabled(in_sketch_3d)
        .params(concat!("point: [x,y,z] world point (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.move3d", "Move 3D Point", move3d)
        .icon("sketch_move")
        .enabled(in_sketch)
        .params("wire: drawn 3D curve or point id, index?: fit point (default 0), to: [x,y,z] | by: [dx,dy,dz]; points joined to it move too, and the sketch's 3D constraints are solved with the moved point held where it was put (refused when they cannot hold it there, or when it is fixed)"),
    CommandSpec::new("sketch.constraint.along_axis", "Along Axis", crate::checked!("sketch.constraint.along_axis", along_axis))
        .icon("c_hv")
        .enabled(in_sketch)
        .contract(&ALONG_AXIS_CONTRACT)
        .output(output_schema_for::<AlongAxisResult>)
        .params("line: a drawn 3D line's id | lines: [ids]; axis: x|y|z (the axes of the sketch's 3D space: its component's, for a sketch in a component); sketch?: id|name. Holds each line parallel to the axis; refused when that conflicts with the sketch's other 3D constraints. Returns one constraint id per line"),
];

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
    let ((ids, joints), info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        let ids = pts.windows(2).map(|w| sk.add_drawn_wire_in(w, WireSpace::Component).map_err(Into::into)).collect::<Result<Vec<String>>>()?;
        // Lines drawn end to end are joined: a Coincident at each joint (#10608).
        let joints = sk.join_drawn_wires(&ids)?;
        Ok((ids, joints))
    })?;
    Ok(json!({"curves": ids, "constraints": joints, "sketch": info}))
}

fn spline3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.spline3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let pts: Vec<Vec3> = points(cmd, p, 2)?.into_iter().map(|at| solidcraft_doc::apply_point(&inverse, at)).collect();
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        Ok(sk.add_drawn_wire_in(&pts, WireSpace::Component)?)
    })?;
    Ok(json!({"curves": [id], "sketch": info}))
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
    Ok(json!({"point": id, "sketch": info}))
}

fn move3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move3d";
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
        let point = WirePoint { wire: wire.clone(), index };
        if sk.wire_constraints.iter().any(|c| matches!(&c.kind, WireConstraintKind::Fix { p, .. } if *p == point)) {
            return Err(bad(cmd, format!("`{wire}` point {index} is fixed (unfix it to move it)")));
        }
        let n = sk.move_drawn_point(&wire, index, to)?;
        // The 3D constraints follow the move, the moved point held where it was put.
        if !sk.wire_constraints.is_empty() {
            let rep = sk.solve_wires(Some(&point));
            let held = sk.wire_point_at(&point).is_some_and(|at| at.dist(to) < 1e-6);
            if !rep.ok || !held {
                return Err(bad(cmd, "the sketch's 3D constraints cannot hold the point there"));
            }
        }
        Ok(n)
    })?;
    Ok(json!({"moved": n, "sketch": info}))
}

/// `sketch.constraint.coincident` on 3D refs: two drawn 3D points, or a 3D point and a drawn 3D
/// line (the point on the line). `None` when neither operand is 3D, for the planar command.
pub(super) fn coincident_3d(s: &mut Session, p: &Value, cmd: &str) -> Result<Option<Value>> {
    let a = p.get("a").and_then(Value::as_str).map(str::to_string);
    let b = p.get("b").and_then(Value::as_str).map(str::to_string);
    let (Some(a), Some(b)) = (a, b) else { return Ok(None) };
    let id = sketch_store::target(s, p, cmd)?;
    let sk = sketch_store::read(&s.doc, id)?;
    if !is_3d(sk, &a) && !is_3d(sk, &b) {
        return Ok(None);
    }
    if is_3d(sk, &a) != is_3d(sk, &b) {
        return Err(bad(cmd, "joining 3D sketch geometry to planar sketch geometry is not supported yet (#10616)"));
    }
    let line = |r: &str| sk.is_drawn_line(r).then(|| r.to_string());
    let kind = match (sk.wire_point(&a), sk.wire_point(&b), line(&a), line(&b)) {
        (Some(x), Some(y), _, _) => WireConstraintKind::Coincident { a: x, b: y },
        (Some(x), None, _, Some(l)) | (None, Some(x), Some(l), _) => WireConstraintKind::PointOnLine { p: x, line: l },
        _ => {
            return Err(bad(
                cmd,
                "give two 3D points (`<curve>.start`, `.end`, `.through`, `.<n>`, or a 3D point), or a 3D point and a drawn 3D line",
            ));
        }
    };
    let (ids, info) = add_3d(s, p, cmd, vec![kind])?;
    Ok(Some(json!({"constraint": ids.first(), "sketch": info})))
}

/// Whether a ref names drawn 3D geometry: a 3D point, or a drawn 3D curve.
fn is_3d(sk: &solidcraft_sketch::Sketch, r: &str) -> bool {
    sk.wire_point(r).is_some() || sk.wire_index(r).is_some()
}

/// `sketch.constraint.fix` on 3D refs: each 3D point (or every fit point of a drawn 3D curve)
/// held where it is, or released. `None` when no ref is 3D, for the planar command.
pub(super) fn fix_3d(s: &mut Session, p: &Value, cmd: &str, refs: &[String], want: Option<bool>) -> Result<Option<Value>> {
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
    let (n, info) = edit(s, p, cmd, true, |sk, _| {
        let mut n = 0;
        for q in &points {
            let has = fixed(sk, q);
            if fix && !has {
                let at = sk.wire_point_at(q).ok_or_else(|| bad(cmd, format!("`{}` has no point {}", q.wire, q.index)))?;
                sk.add_wire_constraint(WireConstraintKind::Fix { p: q.clone(), at })?;
                n += 1;
            } else if !fix && has {
                sk.wire_constraints.retain(|c| !matches!(&c.kind, WireConstraintKind::Fix { p, .. } if p == q));
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(Some(json!({"result": {"fixed": fix, "changed": n}, "sketch": info})))
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
/// constraints can no longer all hold.
fn add_3d(s: &mut Session, p: &Value, cmd: &str, kinds: Vec<WireConstraintKind>) -> Result<(Vec<String>, Value)> {
    edit(s, p, cmd, true, |sk, _| {
        kinds.into_iter().map(|k| sk.add_wire_constraint(k).map_err(|e| bad(cmd, e.to_string()))).collect::<Result<Vec<String>>>()
    })
}

#[cfg(test)]
#[path = "sketch_3d_frame_tests.rs"]
mod frame_tests;
