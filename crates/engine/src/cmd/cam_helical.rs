//! The helical operations of `cam.job`: `bore` and `thread_mill` (docs/cam-jobs.md), planned
//! by `solidcraft_cam::helical` in work coordinates with every rotary at zero and written with
//! their helical arcs kept, like the 3-axis operations. The holes come from a sketch's circles
//! (their exact centres and radii, filtered by diameter) or a list.

use solidcore_record::Vec2;
use solidcraft_cam::Toolpath;
use solidcraft_cam::gcode5::MachineOperation;
use solidcraft_cam::helical::{BoreParams, Descent, Hole, MAX_HOLES, ThreadParams, bore, thread_check_tool, thread_mill};
use solidcraft_cam::indexed::{IndexedOperation, frame_for_index};
use solidcraft_geom::prelude::*;

use super::{Ctx, Planned, Resolved, finite_point, heights, or_tool};
use crate::Result;
use crate::schema::SketchRef;
use crate::schema::cam::{BoreOp, Common, HoleSpec, OperationReport, ThreadMillOp};

/// The helix's slope when a bore gives neither a pitch nor a ramp angle (degrees).
const DEFAULT_RAMP_DEG: f64 = 2.0;
/// The stock a bore's roughing leaves for finishing passes when it does not say (mm).
const DEFAULT_FINISH_STOCK: f64 = 0.1;

/// The circles of sketch `key` (not construction ones) whose diameter is in `range`, in the
/// operation's coordinates (work coordinates, the rotaries at zero): the sketch's plane must
/// face the operation's +Z.
fn sketch_circles(ctx: &Ctx<'_>, op: &str, key: &SketchRef, range: Option<[f64; 2]>) -> Result<Vec<Hole>> {
    let (lo, hi) = match range {
        None => (0.0, f64::INFINITY),
        Some([lo, hi]) if lo.is_finite() && hi.is_finite() && lo >= 0.0 && lo <= hi => (lo, hi),
        Some(_) => return Err(ctx.bad(op, "`diameter_range` must be [smallest, largest], from 0 up, in mm")),
    };
    let id = match ctx.s.doc.find_feature(&key.0) {
        Some(f) if matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. }) => f.id,
        _ => return Err(ctx.bad(op, format!("no sketch `{}`", key.0))),
    };
    let st = ctx.s.model.state();
    let Some(ss) = st.sketch(id) else { return Err(ctx.bad(op, format!("sketch `{}` has not been solved", key.0))) };
    let frame = frame_for_index(ctx.machine, &vec![0.0; ctx.machine.rotaries.len()]).map_err(|e| ctx.bad(op, e))?;
    let plane = &ss.plane;
    // Either way up: only the centres count.
    if !(frame.dir_to_local(plane.x.cross(plane.y)).z.abs() > 1.0 - 1e-6) {
        return Err(ctx.bad(op, format!("sketch `{}` is not on a plane square to the tool: draw the holes on a plane square to it", key.0)));
    }
    let mut holes = Vec::new();
    for curve in &ss.sketch.curves {
        let solidcraft_sketch::CurveKind::Circle { c, r } = curve.kind else { continue };
        if curve.construction || !(2.0 * r >= lo - 1e-9 && 2.0 * r <= hi + 1e-9) {
            continue;
        }
        let Some(q) = ss.sketch.point(c) else { continue };
        let l = frame.to_local(plane.origin + plane.x * q.x + plane.y * q.y - ctx.origin);
        holes.push(Hole { center: Vec2::new(l.x, l.y), diameter: 2.0 * r });
        if holes.len() > MAX_HOLES {
            return Err(ctx.bad(op, format!("sketch `{}` has more than {MAX_HOLES} circles", key.0)));
        }
    }
    if holes.is_empty() {
        let within = if range.is_some() { " in `diameter_range`" } else { "" };
        return Err(ctx.bad(op, format!("sketch `{}` has no circles{within}", key.0)));
    }
    Ok(holes)
}

/// The holes of an operation: a sketch's circles or a list.
fn holes_of(ctx: &Ctx<'_>, op: &str, sketch: Option<&SketchRef>, range: Option<[f64; 2]>, list: Option<Vec<Hole>>, what: &str) -> Result<Vec<Hole>> {
    match (sketch, list) {
        (Some(_), Some(_)) => Err(ctx.bad(op, format!("give `sketch` or `{what}`, not both"))),
        (None, None) => Err(ctx.bad(op, format!("give a `sketch` with circles or `{what}`"))),
        (None, Some(_)) if range.is_some() => Err(ctx.bad(op, "`diameter_range` goes with `sketch`")),
        (Some(k), None) => sketch_circles(ctx, op, k, range),
        (None, Some(l)) if l.is_empty() || l.len() > MAX_HOLES => Err(ctx.bad(op, format!("give 1 to {MAX_HOLES} `{what}`"))),
        (None, Some(l)) => Ok(l),
    }
}

fn listed(ctx: &Ctx<'_>, op: &str, holes: &[HoleSpec]) -> Result<Vec<Hole>> {
    holes
        .iter()
        .map(|h| {
            finite_point(&[h.center[0], h.center[1], h.diameter], "a hole's `center` and `diameter`").map_err(|e| ctx.bad(op, e))?;
            Ok(Hole { center: Vec2::new(h.center[0], h.center[1]), diameter: h.diameter })
        })
        .collect()
}

/// A planned helical operation, written as a 3-axis toolpath with every rotary at zero.
fn planned(ctx: &Ctx<'_>, c: Common<'_>, r: Resolved, kind: &str, path: Toolpath, warnings: Vec<String>) -> Result<Planned> {
    let stats = path.stats();
    let rotary = vec![0.0; ctx.machine.rotaries.len()];
    let io = IndexedOperation { name: c.name.into(), tool: r.tool.clone(), spindle_rpm: r.rpm, coolant: c.coolant, rotary, path };
    let (poses, sources) = io.poses_mapped(ctx.machine, ctx.pose_tol).map_err(|e| ctx.bad(c.name, e))?;
    let report = OperationReport {
        name: c.name.into(),
        kind: kind.into(),
        tool: r.tool.number,
        moves: io.path.moves.len(),
        cut_length: stats.cut_length,
        cut_minutes: stats.cut_minutes,
        max_tilt_deg: poses.stats().max_tilt.to_degrees(),
        warnings,
        ..OperationReport::default()
    };
    Ok(Planned {
        op: MachineOperation::Indexed(io),
        poses,
        sources,
        gouge_check: c.gouge_check,
        band: None,
        tool: r.tool,
        report,
        axes: "3-axis".into(),
    })
}

/// A `bore` operation.
pub(super) fn bore_op(ctx: &Ctx<'_>, o: &BoreOp) -> Result<Planned> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let list = match &o.holes {
        Some(h) => Some(listed(ctx, c.name, h)?),
        None => None,
    };
    let holes = holes_of(ctx, c.name, o.sketch.as_ref(), o.diameter_range, list, "holes")?;
    let descent = match (o.pitch, o.ramp_angle_deg) {
        (Some(_), Some(_)) => return Err(ctx.bad(c.name, "give `pitch` or `ramp_angle_deg`, not both")),
        (Some(_), None) if o.stepdown.is_some() => {
            return Err(ctx.bad(c.name, "`stepdown` caps the pitch of a `ramp_angle_deg`; with `pitch` leave it out"));
        }
        (Some(pitch), None) => Descent::Pitch { pitch },
        (None, ramp) => Descent::Ramp {
            angle: ctx.degrees(c.name, ramp.unwrap_or(DEFAULT_RAMP_DEG), "`ramp_angle_deg`")?,
            max_pitch: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter * 0.1),
        },
    };
    let finish_passes = o.finish_passes.unwrap_or(0);
    let params = BoreParams {
        heights: heights(&o.heights),
        feeds: r.feeds,
        descent,
        climb: o.climb.unwrap_or(true),
        stepover: or_tool(o.stepover, r.tool.stepover, r.tool.diameter * 0.4),
        stock_to_leave: o.stock_to_leave.unwrap_or(if finish_passes > 0 { DEFAULT_FINISH_STOCK } else { 0.0 }),
        finish_passes,
    };
    let out = bore(&holes, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    planned(ctx, c, r, "bore", out.path, out.warnings)
}

/// A `thread_mill` operation. The gouge check takes the thread mill as the core inside its
/// teeth, as deep as the thread ([`thread_check_tool`]).
pub(super) fn thread_mill_op(ctx: &Ctx<'_>, o: &ThreadMillOp) -> Result<Planned> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let list = match &o.centers {
        Some(centers) => Some(
            centers
                .iter()
                .map(|p| {
                    finite_point(p, "`centers`").map_err(|e| ctx.bad(c.name, e))?;
                    Ok(Hole { center: Vec2::new(p[0], p[1]), diameter: 0.0 })
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        None => None,
    };
    let holes = holes_of(ctx, c.name, o.sketch.as_ref(), o.diameter_range, list, "centers")?;
    let centers: Vec<Vec2> = holes.iter().map(|h| h.center).collect();
    let params = ThreadParams {
        heights: heights(&o.heights),
        feeds: r.feeds,
        pitch: o.pitch,
        diameter: o.thread_diameter,
        internal: o.internal.unwrap_or(true),
        right_hand: o.right_hand.unwrap_or(true),
        climb: o.climb.unwrap_or(true),
        depth: o.thread_depth,
        radial_passes: o.radial_passes.unwrap_or(1),
        spring_pass: o.spring_pass.unwrap_or(false),
    };
    let out = thread_mill(&centers, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    let check = thread_check_tool(&r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    // The program keeps the thread mill; only the gouge and holder checks take its core.
    let mut p = planned(ctx, c, r, "thread_mill", out.path, out.warnings)?;
    p.tool = check;
    Ok(p)
}
