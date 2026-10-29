//! 3D sketch drawing: with the sketch's 3D Sketch option on (Sketch Palette), lines, arcs,
//! splines and points may leave the sketch plane. They are kept as drawn 3D curves (wires with
//! fit points, exact arcs keeping their analytic form): drawn and snapped to, movable point by
//! point, and usable as sweep and pipe paths.

use serde_json::{Value, json};
use solidcraft_geom::{Vec2, Vec3};
use solidcraft_sketch::{CurveKind, WireSpace};

use super::sketch::edit;
use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, req_vec3, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.line3d", "3D Line", line3d).icon("line").enabled(in_sketch).params(concat!(
        "points: [[x,y,z]…] world points (2 or more): one 3D line per segment, joined end to end (needs 3D Sketch on)",
        created_doc!()
    )),
    CommandSpec::new("sketch.spline3d", "3D Spline", spline3d)
        .icon("spline")
        .enabled(in_sketch)
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
        .enabled(in_sketch)
        .params(concat!("point: [x,y,z] world point (needs 3D Sketch on)", created_doc!())),
    CommandSpec::new("sketch.move3d", "Move 3D Point", move3d).icon("sketch_move").enabled(in_sketch).params(
        "wire: drawn 3D curve or point id, index?: fit point (default 0) | point: a sketch point on the plane (`l1.end`, `p3`: a line's end, a fit point of a spline, a lone point), to: [x,y,z] | by: [dx,dy,dz] (world); points joined to it move too. In a 3D sketch a `point` on the plane lifts the lines and fit-point splines through it into drawn 3D curves first (arcs, circles and control-point splines stay on the plane)"
    ),
];

/// The 2D curves (indices) [`lift`] turns into drawn 3D curves, with their points and fit points
/// in component space: a line's two ends, a fit-point spline's points.
#[allow(clippy::type_complexity)]
fn liftable(sk: &solidcraft_sketch::Sketch, curves: &[usize], comp: &dyn Fn(Vec2) -> Vec3, cmd: &str) -> Result<Vec<(Vec<usize>, Vec<Vec3>)>> {
    let mut out = Vec::new();
    for &ci in curves {
        let c = sk.curves.get(ci).ok_or_else(|| bad(cmd, "no such curve"))?;
        if c.link.is_some() {
            return Err(bad(cmd, format!("`{}` is projected geometry; it stays where it was projected", c.id)));
        }
        let ids = match &c.kind {
            CurveKind::Line { a, b } => vec![*a, *b],
            CurveKind::Spline { pts, control: false, periodic: false, .. } => pts.clone(),
            other => {
                let what = match other {
                    CurveKind::Spline { .. } => "a control-point or closed spline",
                    CurveKind::Circle { .. } => "a circle",
                    CurveKind::Arc { .. } => "an arc",
                    CurveKind::Ellipse { .. } => "an ellipse",
                    _ => "a conic",
                };
                return Err(bad(
                    cmd,
                    format!("not supported yet: moving {what} (`{}`) off the sketch plane; lines, fit-point splines and points can", c.id),
                ));
            }
        };
        let fit = ids
            .iter()
            .map(|i| sk.points.get(*i).map(|p| comp(p.pos)).ok_or_else(|| bad(cmd, "a curve point is missing")))
            .collect::<Result<Vec<_>>>()?;
        out.push((ids, fit));
    }
    Ok(out)
}

/// Turn the curves `curves` of the target 3D sketch into drawn 3D curves through the same
/// places, moved by `offset` (world); the points `points` move by `offset` too, each on its own:
/// a lone point becomes a drawn 3D point, and the lines and fit-point splines through one that
/// isn't on a moved curve become drawn 3D curves with just that point moved. With `grab` (a
/// sketch point and the world place it goes to), turn every line and fit-point spline through that
/// point (or the point alone) into drawn 3D curves and move just that point there, its neighbours
/// staying joined. Returns the new drawn curves and, for a grab, where the grabbed point is now
/// (curve id, fit index).
pub(super) fn lift(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    curves: &[String],
    points: &[String],
    offset: Vec3,
    grab: Option<(String, Vec3)>,
) -> Result<(Vec<String>, Option<(String, usize)>, Value)> {
    let id = target_sketch(s, p, cmd)?;
    let st = s.world_state();
    let plane = st.sketch(id).map(|ss| ss.plane).ok_or_else(|| bad(cmd, "the sketch has no plane"))?;
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let comp = move |q: Vec2| solidcraft_doc::apply_point(&inverse, plane.to_world(q));
    let shift = solidcraft_doc::apply_vector(&inverse, offset);
    let ((wires, at), info) = edit(s, p, cmd, false, |sk, _| {
        needs_3d(sk, cmd)?;
        let mut which = Vec::new();
        for c in curves {
            which.push(sk.curve_index(c).ok_or_else(|| bad(cmd, format!("no curve `{c}`")))?);
        }
        let mut lone = None;
        let mut grabbed = None;
        if let Some((r, to)) = &grab {
            let pi = sk.resolve_point(r).filter(|i| *i != 0).ok_or_else(|| bad(cmd, format!("no point `{r}`")))?;
            let through: Vec<usize> = sk.curves.iter().enumerate().filter(|(_, c)| c.kind.uses(pi)).map(|(i, _)| i).collect();
            if through.is_empty() {
                lone = Some((pi, sk.points.get(pi).map(|q| comp(q.pos)).ok_or_else(|| bad(cmd, "the point is missing"))?));
            }
            which.extend(through);
            grabbed = Some((pi, *to));
        }
        which.sort_unstable();
        which.dedup();
        // Points moved on their own: lone ones, and ones on curves that stay where they are
        // (those curves are lifted in place, then just the point moves).
        let mut alone: Vec<(usize, Vec3)> = Vec::new();
        let mut on_curves: Vec<usize> = Vec::new();
        let mut in_place: Vec<usize> = Vec::new();
        for r in points {
            let pi = sk.resolve_point(r).filter(|i| *i != 0).ok_or_else(|| bad(cmd, format!("no point `{r}`")))?;
            let through: Vec<usize> = sk.curves.iter().enumerate().filter(|(_, c)| c.kind.uses(pi)).map(|(i, _)| i).collect();
            if through.is_empty() {
                if !alone.iter().any(|(q, _)| *q == pi) {
                    alone.push((pi, sk.points.get(pi).map(|q| comp(q.pos)).ok_or_else(|| bad(cmd, "the point is missing"))?));
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
        let lifted = liftable(sk, &all, &comp, cmd)?;
        sk.remove_curves(&all);
        let mut gone: Vec<usize> = alone.iter().map(|(q, _)| *q).collect();
        if let Some((pi, _)) = lone {
            gone.push(pi);
        }
        sk.remove_points(&gone);
        let mut made = Vec::new();
        let mut at = None;
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
            if let Some((pi, _)) = grabbed
                && at.is_none()
                && let Some(k) = ids.iter().position(|i| *i == pi)
            {
                at = Some((w.clone(), k));
            }
            made.push(w);
        }
        if let Some((_, q)) = lone {
            let w = sk.add_drawn_wire_in(&[q + shift], WireSpace::Component)?;
            at = Some((w.clone(), 0));
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
        if let (Some((_, to)), Some((w, k))) = (grabbed, &at) {
            sk.move_drawn_point(w, *k, solidcraft_doc::apply_point(&inverse, to))?;
        }
        Ok((made, at))
    })?;
    Ok((wires, at, info))
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

/// The 3D Arc button starts an interactive tool, so it is only offered while a 3D sketch is
/// being edited; otherwise the tool could only fail on its third click.
fn in_sketch_3d(s: &Session) -> std::result::Result<(), String> {
    let id = sketch_store::active(s).ok_or("no sketch is being edited (Create Sketch or Edit Sketch first)")?;
    match sketch_store::read(&s.doc, id) {
        Ok(sk) if sk.view.three_d => Ok(()),
        Ok(_) => Err("turn on 3D Sketch (sketch.options sketch_3d: true) to draw 3D arcs".into()),
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
    Ok(json!({"curves": ids, "sketch": info}))
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
    if let Some(point) = p.get("point").and_then(Value::as_str) {
        let parse = |name| p.get(name).map(|v| vec3(v).ok_or_else(|| bad(cmd, format!("`{name}` must be [x,y,z]")))).transpose();
        let (to, by) = (parse("to")?, parse("by")?);
        let id = target_sketch(s, p, cmd)?;
        let st = s.world_state();
        let ss = st.sketch(id).ok_or_else(|| bad(cmd, "the sketch has no plane"))?;
        let at = ss
            .sketch
            .resolve_point(point)
            .and_then(|i| ss.sketch.points.get(i))
            .map(|q| ss.plane.to_world(q.pos))
            .ok_or_else(|| bad(cmd, format!("no point `{point}`")))?;
        let to = match (to, by) {
            (Some(t), _) => t,
            (None, Some(d)) => at + d,
            _ => return Err(bad(cmd, "give `to` or `by`")),
        };
        let (wires, grabbed, info) = lift(s, p, cmd, &[], &[], Vec3::ZERO, Some((point.to_string(), to)))?;
        let (wire, index) = grabbed.ok_or_else(|| bad(cmd, "the point did not lift"))?;
        return Ok(json!({"moved": 1, "lifted": wires, "wire": wire, "index": index, "sketch": info}));
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
    Ok(json!({"moved": n, "sketch": info}))
}

/// Move/Copy on drawn 3D geometry: each picked drawn point (`(wire, Some(index))`) or whole
/// drawn curve (`(wire, None)`) moves as the picked plane geometry does: `f` moves its place
/// along the plane (sketch coordinates, turn included) and `z` adds that much along the normal;
/// its height off the plane is kept. Returns how many points moved, and the sketch's state.
pub(super) fn move_drawn(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    picks: &[(String, Option<usize>)],
    f: &dyn Fn(Vec2) -> Vec2,
    z: f64,
) -> Result<(usize, Value)> {
    let id = target_sketch(s, p, cmd)?;
    let world = s.world_state().sketch(id).map(|ss| ss.plane).ok_or_else(|| bad(cmd, "the sketch has no plane"))?;
    let inverse = inverse_owner_frame(s, p, cmd)?;
    // The plane as component-space drawn points see it.
    let component = solidcraft_geom::Plane {
        origin: solidcraft_doc::apply_point(&inverse, world.origin),
        x: solidcraft_doc::apply_vector(&inverse, world.x),
        y: solidcraft_doc::apply_vector(&inverse, world.y),
    };
    edit(s, p, cmd, false, |sk, _| {
        let mut n = 0;
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
            let normal = plane.normal();
            let moved: Vec<(usize, Vec3)> = which
                .into_iter()
                .filter_map(|i| drawn.fit.get(i).map(|q| (i, *q)))
                .map(|(i, q)| (i, plane.to_world(f(plane.to_local(q))) + normal * ((q - plane.origin).dot(normal) + z)))
                .collect();
            for (i, to) in moved {
                sk.move_drawn_point(wire, i, to)?;
                n += 1;
            }
        }
        Ok(n)
    })
}

#[cfg(test)]
#[path = "sketch_3d_frame_tests.rs"]
mod frame_tests;
