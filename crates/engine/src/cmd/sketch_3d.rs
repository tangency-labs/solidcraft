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
use solidcraft_sketch::{Axis, OriginPlane, PointExprs3, WireConstraintKind, WirePoint, WireSpace};

use super::sketch::edit;
use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, vec3};
use crate::schema::sketch_3d::{AlongAxisParams, AlongAxisResult, AxisName, OnPlaneParams, OnPlaneResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const ALONG_AXIS_CONTRACT: TypedContract<AlongAxisParams> = TypedContract::new();
const ON_PLANE_CONTRACT: TypedContract<OnPlaneParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.line3d", "3D Line", line3d).icon("line").enabled(in_sketch_3d).params(concat!(
        "points: [[x,y,z]…] world points (2 or more; a coordinate may be an expression over the design's parameters, the point then held where it comes to and following them, in a sketch whose component is not moved): one 3D line per segment, joined end to end with a Coincident at each joint (needs 3D Sketch on); constraints lists the joints and placed points",
        created_doc!()
    )),
    CommandSpec::new("sketch.spline3d", "3D Spline", spline3d)
        .icon("spline")
        .enabled(in_sketch_3d)
        .params(concat!("points: [[x,y,z]…] world fit points (2 or more, coordinates numbers or expressions as for sketch.line3d): a smooth 3D curve through them (needs 3D Sketch on)", created_doc!())),
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
        .params(concat!("point: [x,y,z] world point, coordinates numbers or expressions as for sketch.line3d (needs 3D Sketch on)", created_doc!())),
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
    CommandSpec::new("sketch.constraint.on_plane", "On Plane", crate::checked!("sketch.constraint.on_plane", on_plane))
        .icon("c_coincident")
        .enabled(in_sketch)
        .contract(&ON_PLANE_CONTRACT)
        .output(output_schema_for::<OnPlaneResult>)
        .params("point: a 3D point ref (`w1.start`, `.end`, `.through`, `.<n>`, or a 3D point's id) | points: [refs]; plane: xy|yz|xz (the origin planes of the sketch's 3D space); sketch?: id|name. Holds each point on the plane; refused when that conflicts with the sketch's other 3D constraints. Returns one constraint id per point"),
];

/// The `points` of a 3D drawing command as given (each `[x, y, z]`, checked by [`expr_point3`]).
fn point_values<'a>(cmd: &str, p: &'a Value, min: usize) -> Result<Vec<&'a Value>> {
    let a = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must list [x,y,z] points"))?;
    if a.len() < min || a.len() > solidcraft_sketch::MAX_FIT_POINTS {
        return Err(bad(cmd, format!("`points` needs {min} to {} points", solidcraft_sketch::MAX_FIT_POINTS)));
    }
    Ok(a.iter().collect())
}

const POINT3_MUST: &str = "a 3D point's coordinates must be numbers or expressions over the design's parameters";

/// A 3D point `[x, y, z]` whose coordinates may be expressions over the design's parameters:
/// where it is now and, when a coordinate names a parameter, the expressions that place it.
fn expr_point3(s: &Session, v: &Value, cmd: &str) -> Result<(Vec3, Option<PointExprs3>)> {
    let Some([x, y, z]) = v.as_array().map(Vec::as_slice) else { return Err(bad(cmd, "a point must be [x, y, z]")) };
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

/// The points of a 3D drawing command, each with the expressions that place it. Expressions are
/// in the sketch's own coordinates, so they are only taken where those are the world's (the
/// sketch's component is not moved), as a plain number would be.
fn points3(s: &Session, cmd: &str, values: &[&Value], inverse: &solidcraft_doc::Mat) -> Result<Vec<(Vec3, Option<PointExprs3>)>> {
    let pts: Vec<(Vec3, Option<PointExprs3>)> = values.iter().map(|v| expr_point3(s, v, cmd)).collect::<Result<_>>()?;
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
fn place_points(sk: &mut solidcraft_sketch::Sketch, placed: &[(WirePoint, Option<PointExprs3>)]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for (point, exprs) in placed {
        let Some(exprs) = exprs else { continue };
        let at = sk.wire_point_at(point).ok_or_else(|| bad("sketch.line3d", "a drawn 3D point went missing"))?;
        out.push(sk.add_wire_constraint(WireConstraintKind::Fix { p: point.clone(), at, exprs: Some(Box::new(exprs.clone())) })?);
    }
    Ok(out)
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
    let given = point_values(cmd, p, 2)?;
    let pts = points3(s, cmd, &given, &inverse)?;
    let ((ids, joints), info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        let at: Vec<Vec3> = pts.iter().map(|(at, _)| *at).collect();
        let ids = at.windows(2).map(|w| sk.add_drawn_wire_in(w, WireSpace::Component).map_err(Into::into)).collect::<Result<Vec<String>>>()?;
        // Lines drawn end to end are joined: a Coincident at each joint (#10608).
        let mut joints = sk.join_drawn_wires(&ids)?;
        // Point k is line k's start, and the last line's end.
        let placed: Vec<(WirePoint, Option<PointExprs3>)> = pts
            .iter()
            .enumerate()
            .filter_map(|(k, (_, e))| {
                let (wire, index) = if k < ids.len() { (ids.get(k)?, 0) } else { (ids.last()?, 1) };
                Some((WirePoint { wire: wire.clone(), index }, e.clone()))
            })
            .collect();
        joints.extend(place_points(sk, &placed)?);
        Ok((ids, joints))
    })?;
    Ok(json!({"curves": ids, "constraints": joints, "sketch": info}))
}

fn spline3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.spline3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let given = point_values(cmd, p, 2)?;
    let pts = points3(s, cmd, &given, &inverse)?;
    let ((id, placed), info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        let at: Vec<Vec3> = pts.iter().map(|(at, _)| *at).collect();
        let id = sk.add_drawn_wire_in(&at, WireSpace::Component)?;
        let held: Vec<(WirePoint, Option<PointExprs3>)> =
            pts.iter().enumerate().map(|(index, (_, e))| (WirePoint { wire: id.clone(), index }, e.clone())).collect();
        let placed = place_points(sk, &held)?;
        Ok((id, placed))
    })?;
    Ok(json!({"curves": [id], "constraints": placed, "sketch": info}))
}

fn arc3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.arc3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let given: Vec<&Value> =
        ["start", "through", "end"].iter().map(|k| p.get(*k).ok_or_else(|| bad(cmd, format!("`{k}` must be [x,y,z]")))).collect::<Result<_>>()?;
    let pts = points3(s, cmd, &given, &inverse)?;
    let ((id, placed), info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        let at: Vec<Vec3> = pts.iter().map(|(at, _)| *at).collect();
        let id = sk.add_drawn_arc_in(&at, WireSpace::Component)?;
        let held: Vec<(WirePoint, Option<PointExprs3>)> =
            pts.iter().enumerate().map(|(index, (_, e))| (WirePoint { wire: id.clone(), index }, e.clone())).collect();
        let placed = place_points(sk, &held)?;
        Ok((id, placed))
    })?;
    Ok(json!({"curves": [id], "constraints": placed, "sketch": info}))
}

fn point3d(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.point3d";
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let given = p.get("point").ok_or_else(|| bad(cmd, "`point` must be [x,y,z]"))?;
    let pts = points3(s, cmd, &[given], &inverse)?;
    let ((id, placed), info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        let (at, e) = pts.first().cloned().ok_or_else(|| bad(cmd, "`point` must be [x,y,z]"))?;
        let id = sk.add_drawn_wire_in(&[at], WireSpace::Component)?;
        let placed = place_points(sk, &[(WirePoint { wire: id.clone(), index: 0 }, e)])?;
        Ok((id, placed))
    })?;
    Ok(json!({"point": id, "constraints": placed, "sketch": info}))
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
                sk.add_wire_constraint(WireConstraintKind::Fix { p: q.clone(), at, exprs: None })?;
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

/// `sketch.constraint.parallel`, `.perpendicular`, `.collinear`, `.equal` and `.tangent` on 3D
/// refs: two drawn 3D lines (equal: or two drawn 3D arcs; tangent: a line and an arc that meet;
/// parallel: or a line and an origin plane, `xy`, `yz` or `xz`). `None` when neither operand is
/// 3D, for the planar command.
pub(super) fn relation_3d(s: &mut Session, p: &Value, cmd: &str) -> Result<Option<Value>> {
    let a = p.get("a").and_then(Value::as_str).map(str::to_string);
    let b = p.get("b").and_then(Value::as_str).map(str::to_string);
    let (Some(a), Some(b)) = (a, b) else { return Ok(None) };
    let id = sketch_store::target(s, p, cmd)?;
    let sk = sketch_store::read(&s.doc, id)?;
    if !is_3d(sk, &a) && !is_3d(sk, &b) {
        return Ok(None);
    }
    let plane = OriginPlane::from_name(&b).filter(|_| !is_3d(sk, &b)).or_else(|| OriginPlane::from_name(&a).filter(|_| !is_3d(sk, &a)));
    let (line_of, other) = if OriginPlane::from_name(&a).is_some() && !is_3d(sk, &a) { (b.clone(), a.clone()) } else { (a.clone(), b.clone()) };
    if plane.is_none() && is_3d(sk, &a) != is_3d(sk, &b) {
        return Err(bad(cmd, "joining 3D sketch geometry to planar sketch geometry is not supported yet (#10616)"));
    }
    let kind = match (cmd, plane) {
        ("sketch.constraint.parallel", Some(plane)) if sk.is_drawn_line(&line_of) => WireConstraintKind::ParallelToPlane { line: line_of, plane },
        (_, Some(_)) => {
            return Err(bad(
                cmd,
                format!("`{other}`: of the origin planes, only Parallel takes a plane (use sketch.constraint.on_plane for a point)"),
            ));
        }
        ("sketch.constraint.parallel", None) if sk.is_drawn_line(&a) && sk.is_drawn_line(&b) => WireConstraintKind::Parallel { a, b },
        ("sketch.constraint.perpendicular", None) if sk.is_drawn_line(&a) && sk.is_drawn_line(&b) => WireConstraintKind::Perpendicular { a, b },
        ("sketch.constraint.collinear", None) if sk.is_drawn_line(&a) && sk.is_drawn_line(&b) => WireConstraintKind::Collinear { a, b },
        ("sketch.constraint.equal", None) => sk.equal_kind(&a, &b).ok_or_else(|| bad(cmd, "Equal takes two drawn 3D lines or two drawn 3D arcs"))?,
        ("sketch.constraint.tangent", None) => sk
            .tangent_kind(&a, &b)
            .or_else(|| sk.tangent_kind(&b, &a))
            .ok_or_else(|| bad(cmd, "Tangent takes a drawn 3D line and a drawn 3D arc that meet at an end (join them first)"))?,
        _ => return Err(bad(cmd, "this takes two drawn 3D lines")),
    };
    let (ids, info) = add_3d(s, p, cmd, vec![kind])?;
    Ok(Some(json!({"constraint": ids.first(), "sketch": info})))
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
    let plane = p.get("plane").and_then(Value::as_str).and_then(OriginPlane::from_name).ok_or_else(|| bad(cmd, "`plane` must be xy, yz or xz"))?;
    let id = sketch_store::target(s, p, cmd)?;
    let sk = sketch_store::read(&s.doc, id)?;
    let kinds = refs
        .iter()
        .map(|r| sk.wire_point(r).map(|q| WireConstraintKind::OnPlane { p: q, plane }).ok_or_else(|| bad(cmd, format!("`{r}` is not a 3D point"))))
        .collect::<Result<Vec<_>>>()?;
    let (ids, info) = add_3d(s, p, cmd, kinds)?;
    Ok(json!({"constraints": ids, "sketch": info}))
}

/// `sketch.dimension` on 3D refs: a drawn 3D line's length, the distance between two 3D points
/// (or along X, Y or Z only), the angle between two drawn 3D lines, or a drawn 3D arc's radius.
/// `None` when no entity is 3D, for the planar command.
pub(super) fn dimension_3d(s: &mut Session, p: &Value, cmd: &str) -> Result<Option<Value>> {
    let ents: Vec<String> =
        p.get("entities").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
    let id = sketch_store::target(s, p, cmd)?;
    let stored = sketch_store::read(&s.doc, id)?;
    // An origin plane beside a 3D entity: the distance from it, or the angle to it.
    let planar = |r: &str| OriginPlane::from_name(r).filter(|_| !is_3d(stored, r));
    let three: Vec<bool> = ents.iter().map(|r| is_3d(stored, r) || (planar(r).is_some() && ents.iter().any(|o| is_3d(stored, o)))).collect();
    if !three.iter().any(|t| *t) {
        return Ok(None);
    }
    if three.len() > 2 || !three.iter().all(|t| *t) {
        return Err(bad(cmd, "dimension one or two 3D entities, not a mix of 3D and planar ones (#10616)"));
    }
    // Measured as the design's parameters solve it now: the stored sketch keeps the geometry of
    // its last edit, from before a parameter changed.
    let mut solved = stored.clone();
    s.doc.apply_wire_values(&s.doc.param_values().0, &mut solved)?;
    let dof_before = solved.solve_wires(None).dof;
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
        [a, b] => sk.wire_point(a).zip(sk.wire_point(b)),
        _ => None,
    };
    let to_plane = match ents.as_slice() {
        [x, y] => match (planar(y), planar(x)) {
            (Some(plane), None) => Some((x.clone(), plane)),
            (None, Some(plane)) => Some((y.clone(), plane)),
            _ => None,
        },
        _ => None,
    };
    let kind = if let Some((r, plane)) = to_plane.clone() {
        match (sk.wire_point(&r), sk.is_drawn_line(&r)) {
            (Some(q), _) if matches!(ty.as_str(), "auto" | "distance") => {
                let d = sk.wire_point_at(&q).map(|at| at.dot(plane.normal())).unwrap_or(0.0);
                WireConstraintKind::PlaneDistance { p: q, plane, reversed: d < 0.0, value: 0.0 }
            }
            (None, true) if matches!(ty.as_str(), "auto" | "angle") => WireConstraintKind::PlaneAngle { line: r, plane, value: 0.0 },
            _ => return Err(bad(cmd, "to an origin plane: a 3D point (distance) or a drawn 3D line (angle)")),
        }
    } else {
        match (ents.as_slice(), pair, axis) {
            (_, Some((a, b)), Some(axis)) if matches!(ty.as_str(), "distance_x" | "distance_y" | "distance_z") => {
                let d = sk.wire_point_at(&b).zip(sk.wire_point_at(&a)).map(|(q, p)| (q - p).dot(axis.unit())).unwrap_or(0.0);
                WireConstraintKind::AxisDistance { a, b, axis, reversed: d < 0.0, value: 0.0 }
            }
            (_, Some((a, b)), None) if matches!(ty.as_str(), "auto" | "distance" | "length") => WireConstraintKind::Distance { a, b, value: 0.0 },
            ([a, b], None, None) if matches!(ty.as_str(), "auto" | "angle") && sk.is_drawn_line(a) && sk.is_drawn_line(b) => {
                WireConstraintKind::Angle { a: a.clone(), b: b.clone(), value: 0.0 }
            }
            ([arc], None, None) if matches!(ty.as_str(), "auto" | "radius") => WireConstraintKind::Radius { arc: arc.clone(), value: 0.0 },
            _ => {
                return Err(bad(
                    cmd,
                    "3D dimensions: a drawn 3D line (length; distance_x/y/z for its run along an axis), two 3D points (distance, or distance_x/y/z), two drawn 3D lines (angle), a drawn 3D arc (radius), or a 3D point or line and an origin plane xy/yz/xz",
                ));
            }
        }
    };
    let is_angle = kind.is_angle();
    let mut kind = kind;
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
        let (unit, default_expr) = if is_angle { ("deg".to_string(), format!("{} deg", round6(cur.to_degrees()))) } else { doc.design_length(cur) };
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
        let pname = doc.new_model_param(&expression, &unit);
        let (_, errs) = doc.param_values();
        if let Some(err) = errs.get(&pname) {
            return Err(bad(cmd, format!("value: {err}")));
        }
        let cid = sk.add_wire_dimension(kind, Some(pname.clone()), false).map_err(|e| bad(cmd, e.to_string()))?;
        Ok((cid, Some(pname)))
    })?;
    let sk = sketch_store::read(&s.doc, id)?;
    // A dimension the other 3D constraints already determine removes no freedom: refused, or
    // with `driven: true` placed as a reference dimension instead.
    if !driven && sk.clone().solve_wires(None).dof >= dof_before {
        s.doc = start;
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
        _ if ents.len() == 1 && sk.is_drawn_line(&ents[0]) => "length".into(),
        _ if ents.len() == 1 => "radius".into(),
        _ => "distance".into(),
    };
    let expression = param.as_ref().and_then(|n| s.doc.param(n)).map(|x| x.expr.clone());
    let mut out =
        json!({"constraint": cid, "param": param, "type": type_name, "expression": expression, "measured": m, "unit": unit, "sketch": info});
    if driven {
        out["driven"] = json!(true);
    }
    if let Some((name, _)) = defines {
        out["defined"] = json!(name);
    }
    Ok(Some(out))
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

#[cfg(test)]
#[path = "sketch_3d_frame_tests.rs"]
mod frame_tests;
