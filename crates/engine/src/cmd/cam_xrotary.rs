//! The 4th-axis operations of `cam.job` about +X, and the setup check of every rotary
//! operation of a job. Part of `cmd/cam.rs` (its planning types), kept in a file of its own.
//!
//! - `rotary_finish`, `rotary_roughing` and `wrap`: the part turned on the A axis while the
//!   tool, radial, follows it ([`solidcraft_cam::rotary`]).
//! - `wrap_pocket` and `wrap_contour`: a closed profile drawn unrolled (x along the A axis, y the
//!   arc length round a cylinder) cut into that cylinder in depth levels by
//!   [`solidcraft_cam::wrapped`], each level offset at its own radius.
//! - `indexed` and `indexed_pattern`: a 3-axis operation run with A held at an angle (3+1), or
//!   at each of several angles, exactly as that many `indexed` entries would be.
//! - The setup check: a rotary operation's clearance radius, where its rapids go round and where
//!   A turns, must be outside the part (and the stock, for roughing from round stock) along the
//!   stretch of X it works over; an indexed operation's clearance height too, where the post
//!   turns A at it rather than at the machine's top. A job that fails it is refused.

use solidcraft_cam::rotary::{RotaryParams, RotaryPattern, RotaryRoughingParams, rotary_finish, rotary_roughing, wrap_with_step};
use solidcraft_cam::wrapped::{WrapContourParams, WrapCut, WrapPocketParams, wrap_contour, wrap_pocket};
use solidcraft_cam::{DEFAULT_MAX_ANGLE_STEP, Move, Move5, Operation5, Pose};

use super::*;
use crate::schema::cam::{IndexedOp, IndexedPatternOp, Op3Spec, PatternSpec, RotaryFinishOp, RotaryRoughingOp, WrapContourOp, WrapOp, WrapPocketOp};

/// Most angles of one indexed pattern; a job's operations, every angle of its patterns
/// counted, are at most [`MAX_OPERATIONS`] too.
const MAX_PATTERN: usize = MAX_OPERATIONS;
/// Default heights of a wrapped operation above its cylinder (mm).
const WRAP_CLEARANCE: f64 = 5.0;
const WRAP_RETRACT: f64 = 1.0;

/// Refuses more than [`MAX_OPERATIONS`] operations in a job, `have` and `more` together.
fn room(ctx: &Ctx<'_>, have: usize, more: usize) -> Result<()> {
    if have.saturating_add(more) > MAX_OPERATIONS {
        Err(bad(ctx.cmd, format!("a job has at most {MAX_OPERATIONS} operations, every angle of its indexed patterns counted")))
    } else {
        Ok(())
    }
}

/// The operations `spec` plans: itself, or an indexed pattern's at each of its angles; refused
/// before anything is planned when the job, with `have` operations before it, has no room for
/// them.
pub(super) fn plan_op(ctx: &Ctx<'_>, spec: &OperationSpec, have: usize) -> Result<Vec<Planned>> {
    match spec {
        OperationSpec::IndexedPattern(o) => {
            let name = spec_name(spec);
            if name.trim().is_empty() {
                return Err(bad(ctx.cmd, "an `indexed_pattern` operation needs a `name`, on itself or on its inner `operation`"));
            }
            let angles = pattern_angles(ctx, name, o)?;
            room(ctx, have, angles.len())?;
            let mut out = Vec::with_capacity(angles.len());
            for a in angles {
                let mut op = (*o.operation).clone();
                *name3_mut(&mut op) = format!("{name} A{}", num(a));
                out.push(plan(ctx, &OperationSpec::Indexed(IndexedOp { name: None, enabled: None, a_deg: a, operation: Box::new(op) }))?);
            }
            Ok(out)
        }
        _ => {
            room(ctx, have, 1)?;
            Ok(vec![plan(ctx, spec)?])
        }
    }
}

/// `ops` (what [`plan_op`] planned for `spec`) after `planned`, each set-up checked against the
/// operation before it. On an error nothing is added.
pub(super) fn add_checked(ctx: &Ctx<'_>, spec: &OperationSpec, planned: &mut Vec<Planned>, ops: Vec<Planned>) -> Result<()> {
    room(ctx, planned.len(), ops.len())?;
    let start = planned.len();
    planned.extend(ops);
    for k in start..planned.len() {
        let prev = k.checked_sub(1).and_then(|j| planned.get(j));
        if let Some(p) = planned.get(k)
            && let Err(e) = setup_check(ctx, spec, prev, p)
        {
            planned.truncate(start);
            return Err(e);
        }
    }
    Ok(())
}

/// The angles of an indexed pattern, degrees.
fn pattern_angles(ctx: &Ctx<'_>, name: &str, o: &IndexedPatternOp) -> Result<Vec<f64>> {
    match (&o.a_deg, o.count) {
        (Some(_), Some(_)) => Err(ctx.bad(name, "give an indexed pattern `a_deg` or `count`, not both")),
        (None, None) => Err(ctx.bad(name, "give an indexed pattern its angles (`a_deg`) or a `count` of them")),
        (Some(angles), None) => {
            if o.start_deg.is_some() {
                return Err(ctx.bad(name, "`start_deg` goes with `count`"));
            }
            if angles.is_empty() || angles.len() > MAX_PATTERN {
                return Err(ctx.bad(name, format!("an indexed pattern has 1 to {MAX_PATTERN} angles")));
            }
            for a in angles {
                ctx.degrees(name, *a, "`a_deg`")?;
            }
            Ok(angles.clone())
        }
        (None, Some(n)) => {
            if n == 0 || n as usize > MAX_PATTERN {
                return Err(ctx.bad(name, format!("an indexed pattern's `count` is 1 to {MAX_PATTERN}")));
            }
            let first = o.start_deg.unwrap_or(0.0);
            ctx.degrees(name, first, "`start_deg`")?;
            Ok((0..n).map(|k| first + 360.0 * f64::from(k) / f64::from(n)).collect())
        }
    }
}

/// The `name` of the 3-axis operation `op`, to change.
pub(super) fn name3_mut(op: &mut Op3Spec) -> &mut String {
    match op {
        Op3Spec::Contour(o) => &mut o.name,
        Op3Spec::Pocket(o) => &mut o.name,
        Op3Spec::Parallel(o) => &mut o.name,
        Op3Spec::Roughing(o) => &mut o.name,
        Op3Spec::Waterline(o) => &mut o.name,
        Op3Spec::Scallop(o) => &mut o.name,
        Op3Spec::SteepShallow(o) => &mut o.name,
        Op3Spec::Pencil(o) => &mut o.name,
        Op3Spec::RestFinish(o) => &mut o.name,
        Op3Spec::Drill(o) => &mut o.name,
        Op3Spec::Chamfer(o) => &mut o.name,
        Op3Spec::Adaptive(o) => &mut o.name,
        Op3Spec::AdaptiveRoughing(o) => &mut o.name,
    }
}

/// The clearance height of the 3-axis operation `op`.
fn op3_clearance(op: &Op3Spec) -> f64 {
    match op {
        Op3Spec::Contour(o) => o.heights.clearance,
        Op3Spec::Pocket(o) => o.heights.clearance,
        Op3Spec::Parallel(o) => o.heights.clearance,
        Op3Spec::Roughing(o) => o.heights.clearance,
        Op3Spec::Waterline(o) => o.heights.clearance,
        Op3Spec::Scallop(o) => o.heights.clearance,
        Op3Spec::SteepShallow(o) => o.heights.clearance,
        Op3Spec::Pencil(o) => o.heights.clearance,
        Op3Spec::RestFinish(o) => o.heights.clearance,
        Op3Spec::Drill(o) => o.heights.clearance,
        Op3Spec::Chamfer(o) => o.heights.clearance,
        Op3Spec::Adaptive(o) => o.heights.clearance,
        Op3Spec::AdaptiveRoughing(o) => o.heights.clearance,
    }
}

fn pattern(ctx: &Ctx<'_>, op: &str, p: PatternSpec) -> Result<RotaryPattern> {
    Ok(match p {
        PatternSpec::Lines { angle_step_deg, zigzag } => {
            RotaryPattern::Lines { angle_step: ctx.degrees(op, angle_step_deg, "`angle_step_deg`")?, zigzag: zigzag.unwrap_or(true) }
        }
        PatternSpec::Helix { pitch } => RotaryPattern::Helix { pitch },
        PatternSpec::Rings { stepover, zigzag } => RotaryPattern::Rings { stepover, zigzag: zigzag.unwrap_or(true) },
    })
}

fn angle_step(ctx: &Ctx<'_>, op: &str, deg: Option<f64>) -> Result<f64> {
    match deg {
        None => Ok(DEFAULT_MAX_ANGLE_STEP),
        Some(d) => ctx.degrees(op, d, "`max_angle_step_deg`"),
    }
}

fn needs_rotary(ctx: &Ctx<'_>, op: &str) -> Result<()> {
    if ctx.machine.rotaries.is_empty() {
        return Err(ctx.bad(op, "a rotary operation needs a machine with a rotary axis (`machine: \"4axis_a\"`)"));
    }
    Ok(())
}

/// A continuous 4-axis operation planned.
fn continuous(c: Common<'_>, r: Resolved, kind: &str, path: Toolpath5) -> Planned {
    let stats = path.stats();
    let report = OperationReport {
        name: c.name.into(),
        kind: kind.into(),
        tool: r.tool.number,
        moves: path.moves.len(),
        cut_length: stats.cut_length,
        cut_minutes: stats.cut_minutes,
        max_tilt_deg: stats.max_tilt.to_degrees(),
        ..OperationReport::default()
    };
    let op = Operation5 {
        name: c.name.into(),
        tool: r.tool.clone(),
        spindle_rpm: r.rpm,
        coolant: c.coolant,
        path: path.clone(),
        plunge_feed: Some(r.feeds.plunge),
    };
    let sources = (0..path.moves.len()).collect();
    Planned {
        op: MachineOperation::Continuous(op),
        poses: path,
        sources,
        feeds: r.feeds,
        gouge_check: c.gouge_check,
        band: None,
        tool: r.tool,
        report,
        axes: "4-axis".into(),
    }
}

pub(super) fn plan_rotary_finish(ctx: &Ctx<'_>, o: &RotaryFinishOp) -> Result<Planned> {
    let c = o.common();
    needs_rotary(ctx, c.name)?;
    let r = ctx.resolve(&c)?;
    let params = RotaryParams {
        pattern: pattern(ctx, c.name, o.pattern)?,
        clearance: o.clearance,
        retract: o.retract,
        floor: o.floor.unwrap_or(0.0),
        feeds: r.feeds,
        range: o.range,
        step: o.step.unwrap_or(r.tool.diameter * 0.25),
        tolerance: ctx.tolerance(c.name, o.tolerance)?,
        stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
        max_angle_step: angle_step(ctx, c.name, o.max_angle_step_deg)?,
    };
    let part = ctx.surface_in(c.name, &Frame::IDENTITY)?;
    let path = rotary_finish(&part, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok(continuous(c, r, "rotary_finish", path))
}

pub(super) fn plan_rotary_roughing(ctx: &Ctx<'_>, o: &RotaryRoughingOp) -> Result<Planned> {
    let c = o.common();
    needs_rotary(ctx, c.name)?;
    let r = ctx.resolve(&c)?;
    let params = RotaryRoughingParams {
        pattern: pattern(ctx, c.name, o.pattern)?,
        stock_radius: roughing_stock_radius(ctx, o)
            .ok_or_else(|| ctx.bad(c.name, "give `stock_radius`, or the job a `stock` (a `cylinder` about X or a `box`) to take it from"))?,
        clearance: o.clearance,
        retract: o.retract,
        floor: o.floor.unwrap_or(0.0),
        stepdown: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter * 0.5),
        feeds: r.feeds,
        range: o.range,
        step: o.step.unwrap_or(r.tool.diameter * 0.25),
        tolerance: ctx.tolerance(c.name, o.tolerance)?,
        stock_to_leave: o.stock_to_leave.unwrap_or(0.25),
        max_angle_step: angle_step(ctx, c.name, o.max_angle_step_deg)?,
    };
    let part = ctx.surface_in(c.name, &Frame::IDENTITY)?;
    let path = rotary_roughing(&part, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok(continuous(c, r, "rotary_roughing", path))
}

/// The radius a `rotary_roughing` starts from: its own `stock_radius`, else the job's stock's
/// (a cylinder's radius, or how far a box's farthest edge is from the A axis).
fn roughing_stock_radius(ctx: &Ctx<'_>, o: &RotaryRoughingOp) -> Option<f64> {
    use solidcraft_cam::stock::Stock;
    o.stock_radius.or_else(|| match ctx.stock? {
        Stock::Cylinder { radius, .. } => Some(*radius),
        Stock::Box { min, max } => Some(min.y.abs().max(max.y.abs()).hypot(min.z.abs().max(max.z.abs()))),
    })
}

/// The flat path of a wrap operation: across at the clearance height, down to the retract
/// height, plunged to the first point, along the points, and up again.
fn wrap_path(ctx: &Ctx<'_>, op: &str, pts: &[Vec3], clearance: f64, retract: f64, feeds: Feeds) -> Result<Toolpath> {
    if !(clearance.is_finite() && retract.is_finite() && clearance > retract && retract >= 0.0 && clearance <= 1000.0) {
        return Err(ctx.bad(op, "the clearance must be above the retract height, both at or above the cylinder (0) and at most 1000 mm"));
    }
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return Err(ctx.bad(op, "the path needs at least 2 points")) };
    if pts.len() < 2 || pts.len() > MAX_POINTS {
        return Err(ctx.bad(op, format!("the path needs 2 to {MAX_POINTS} points")));
    }
    if pts.iter().any(|p| !p.is_finite() || p.x.abs() > MAX_COORD || p.y.abs() > MAX_COORD || p.z > retract || p.z < -MAX_COORD) {
        return Err(ctx.bad(op, "the path's points must be finite, at or below the retract height"));
    }
    let mut moves = vec![
        Move::Rapid { to: Vec3::new(first.x, first.y, clearance) },
        Move::Rapid { to: Vec3::new(first.x, first.y, retract) },
        Move::Feed { to: *first, feed: feeds.plunge },
    ];
    moves.extend(pts.iter().skip(1).map(|p| Move::Feed { to: *p, feed: feeds.cut }));
    moves.push(Move::Rapid { to: Vec3::new(last.x, last.y, clearance) });
    Ok(Toolpath { moves })
}

/// The points of a wrap operation.
fn wrap_points(ctx: &Ctx<'_>, o: &WrapOp) -> Result<Vec<Vec3>> {
    let op = o.name.as_str();
    match (&o.points, &o.sketch) {
        (Some(_), Some(_)) => Err(ctx.bad(op, "give `points` or `sketch` and `curve`, not both")),
        (None, None) => Err(ctx.bad(op, "give `points` or `sketch` and `curve`")),
        (Some(p), None) => {
            if o.curve.is_some() || o.depth.is_some() {
                return Err(ctx.bad(op, "`curve` and `depth` go with `sketch`"));
            }
            Ok(p.iter().map(|q| v3(*q)).collect())
        }
        (None, Some(k)) => {
            let Some(curve) = &o.curve else { return Err(ctx.bad(op, "with `sketch`, give the `curve` id")) };
            let depth = o.depth.unwrap_or(0.0);
            let id = match ctx.s.doc.find_feature(&k.0) {
                Some(f) if matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. }) => f.id,
                _ => return Err(ctx.bad(op, format!("no sketch `{}`", k.0))),
            };
            let st = ctx.s.model.state();
            let Some(ss) = st.sketch(id) else { return Err(ctx.bad(op, format!("sketch `{}` has not been solved", k.0))) };
            let Some(ci) = ss.sketch.curve_index(curve) else { return Err(ctx.bad(op, format!("sketch `{}` has no curve `{curve}`", k.0))) };
            Ok(ss.sketch.polyline(ci).into_iter().map(|q| Vec3::new(q.x, q.y, depth)).collect())
        }
    }
}

pub(super) fn plan_wrap(ctx: &Ctx<'_>, o: &WrapOp) -> Result<Planned> {
    let c = o.common();
    needs_rotary(ctx, c.name)?;
    let r = ctx.resolve(&c)?;
    let pts = wrap_points(ctx, o)?;
    let flat = wrap_path(ctx, c.name, &pts, o.clearance.unwrap_or(WRAP_CLEARANCE), o.retract.unwrap_or(WRAP_RETRACT), r.feeds)?;
    let tol = ctx.tolerance(c.name, None)?;
    let step = angle_step(ctx, c.name, o.max_angle_step_deg)?;
    let path = wrap_with_step(&flat, o.radius, tol, step).map_err(|e| ctx.bad(c.name, e))?;
    Ok(continuous(c, r, "wrap", path))
}

/// The depths and motion of a wrapped operation.
struct WrapSpec<'a> {
    radius: f64,
    top: Option<f64>,
    bottom: f64,
    stepdown: Option<f64>,
    clearance: Option<f64>,
    retract: Option<f64>,
    tolerance: Option<f64>,
    max_angle_step_deg: Option<f64>,
    sketch: Option<&'a crate::schema::SketchRef>,
    profiles: Option<&'a [usize]>,
    polygon: Option<&'a [[f64; 2]]>,
}

/// What both wrapped operations share: the tool resolved, the profile and the cut.
fn wrap_setup(ctx: &Ctx<'_>, c: &Common<'_>, w: &WrapSpec<'_>) -> Result<(Resolved, Vec<Region2>, WrapCut)> {
    needs_rotary(ctx, c.name)?;
    if ctx.machine.rotaries.len() != 1 {
        return Err(ctx.bad(c.name, "a wrapped operation needs a machine with one rotary axis (`machine: \"4axis_a\"`)"));
    }
    let r = ctx.resolve(c)?;
    let regions = ctx.regions(c.name, w.sketch, w.profiles, w.polygon, &Frame::IDENTITY)?;
    let cut = WrapCut {
        radius: w.radius,
        top: w.top.unwrap_or(0.0),
        bottom: w.bottom,
        stepdown: or_tool(w.stepdown, r.tool.stepdown, r.tool.diameter * 0.5),
        clearance: w.clearance.unwrap_or(WRAP_CLEARANCE),
        retract: w.retract.unwrap_or(WRAP_RETRACT),
        feeds: r.feeds,
        tolerance: ctx.tolerance(c.name, w.tolerance)?,
        max_angle_step: angle_step(ctx, c.name, w.max_angle_step_deg)?,
    };
    Ok((r, regions, cut))
}

pub(super) fn plan_wrap_pocket(ctx: &Ctx<'_>, o: &WrapPocketOp) -> Result<Planned> {
    let c = o.common();
    let w = WrapSpec {
        radius: o.radius,
        top: o.top,
        bottom: o.bottom,
        stepdown: o.stepdown,
        clearance: o.clearance,
        retract: o.retract,
        tolerance: o.tolerance,
        max_angle_step_deg: o.max_angle_step_deg,
        sketch: o.sketch.as_ref(),
        profiles: o.profiles.as_deref(),
        polygon: o.polygon.as_deref(),
    };
    let (r, regions, cut) = wrap_setup(ctx, &c, &w)?;
    let params = WrapPocketParams {
        cut,
        stepover: or_tool(o.stepover, r.tool.stepover, r.tool.diameter * 0.4),
        climb: o.climb.unwrap_or(true),
        stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
        ramp_angle: ctx.degrees(c.name, o.ramp_angle_deg.unwrap_or(0.0), "`ramp_angle_deg`")?,
    };
    let path = wrap_pocket(&regions, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok(continuous(c, r, "wrap_pocket", path))
}

pub(super) fn plan_wrap_contour(ctx: &Ctx<'_>, o: &WrapContourOp) -> Result<Planned> {
    let c = o.common();
    let w = WrapSpec {
        radius: o.radius,
        top: o.top,
        bottom: o.bottom,
        stepdown: o.stepdown,
        clearance: o.clearance,
        retract: o.retract,
        tolerance: o.tolerance,
        max_angle_step_deg: o.max_angle_step_deg,
        sketch: o.sketch.as_ref(),
        profiles: o.profiles.as_deref(),
        polygon: o.polygon.as_deref(),
    };
    let (r, regions, cut) = wrap_setup(ctx, &c, &w)?;
    let params = WrapContourParams {
        cut,
        side: match o.side.unwrap_or(SideSpec::Outside) {
            SideSpec::Outside => Side::Outside,
            SideSpec::Inside => Side::Inside,
            SideSpec::On => Side::On,
        },
        climb: o.climb.unwrap_or(true),
        stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
        ramp_angle: ctx.degrees(c.name, o.ramp_angle_deg.unwrap_or(0.0), "`ramp_angle_deg`")?,
    };
    let path = wrap_contour(&regions, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok(continuous(c, r, "wrap_contour", path))
}

/// Where the tool is while A turns or rapids go round for the operation `spec` (a radius from
/// the A axis), and the radius of the stock it must also clear; `None` when nothing turns at a
/// height the operation sets.
fn turning_radius(ctx: &Ctx<'_>, spec: &OperationSpec) -> Option<(f64, f64)> {
    if ctx.machine.rotaries.is_empty() {
        return None;
    }
    let post = ctx.post;
    // An indexed operation turns A where the post puts the tool: at the machine's top after its
    // `safe_rotate` lines, or at the post's clearance, or else at the operation's clearance.
    let indexed = |op: &Op3Spec| {
        if !post.safe_rotate.is_empty() {
            return None;
        }
        Some((post.clearance.filter(|_| post.clearance_retract).unwrap_or_else(|| op3_clearance(op)), f64::NEG_INFINITY))
    };
    match spec {
        OperationSpec::RotaryFinish(o) => Some((o.clearance, f64::NEG_INFINITY)),
        OperationSpec::RotaryRoughing(o) => Some((o.clearance, roughing_stock_radius(ctx, o).unwrap_or(f64::NEG_INFINITY))),
        OperationSpec::Wrap(o) => Some((o.radius + o.clearance.unwrap_or(WRAP_CLEARANCE), f64::NEG_INFINITY)),
        OperationSpec::WrapPocket(o) => Some((o.radius + o.clearance.unwrap_or(WRAP_CLEARANCE), f64::NEG_INFINITY)),
        OperationSpec::WrapContour(o) => Some((o.radius + o.clearance.unwrap_or(WRAP_CLEARANCE), f64::NEG_INFINITY)),
        OperationSpec::Indexed(o) => indexed(&o.operation),
        OperationSpec::IndexedPattern(o) => indexed(&o.operation),
        _ => None,
    }
}

/// Refuses the planned operation `p` of `spec` when A turns into it, or within it, closer to the
/// axis than the part (or the stock) reaches there.
///
/// A turn into an operation happens where the operation before it (`prev`) left the tool, so
/// with the same tool and no `safe_rotate` lines it is checked there ([`turn_check`]). A
/// continuous operation also turns A at its clearance radius along the X it works over, the
/// tool's radius either side. An indexed operation turns only on entry: where the tool's place
/// is not known (the first operation, or after a tool change) its own clearance is held against
/// the X it works over instead.
fn setup_check(ctx: &Ctx<'_>, spec: &OperationSpec, prev: Option<&Planned>, p: &Planned) -> Result<()> {
    if ctx.machine.rotaries.is_empty() {
        return Ok(());
    }
    let entry = prev.filter(|q| ctx.post.safe_rotate.is_empty() && q.tool.number == p.tool.number);
    if let Some(q) = entry {
        turn_check(ctx, q, p)?;
        if matches!(spec, OperationSpec::Indexed(_) | OperationSpec::IndexedPattern(_)) {
            return Ok(());
        }
    }
    let Some((clearance, stock)) = turning_radius(ctx, spec) else { return Ok(()) };
    let (lo, hi) = p.poses.poses().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.tip.x), hi.max(q.tip.x)));
    if !(lo <= hi) {
        return Ok(());
    }
    let r = p.tool.radius();
    let (lo, hi) = (lo - r, hi + r);
    let part = ctx.part.and_then(|part| part_reach(part, lo, hi));
    let (reach, what) = match part {
        Some(m) if m >= stock => (m, "part"),
        _ if stock.is_finite() => (stock, "stock"),
        _ => return Ok(()),
    };
    if clearance > reach + ctx.tol {
        return Ok(());
    }
    Err(ctx.bad(
        &p.report.name,
        format!(
            "the clearance, {} mm from the A axis, is not outside the {what}, which reaches {} mm from it between X {} and {}: the tool \
             would meet it where rapids go round or A turns. Give a clearance above {} mm",
            num(clearance),
            num(reach),
            num(lo),
            num(hi),
            num(reach + ctx.tol)
        ),
    ))
}

/// Refuses `p` when A turns into it from where `prev` (with the same tool) left the tool and
/// the part, turning, would meet the tool there. The post rises first, never down, to its
/// `clearance` or else the highest rapid of `p` (with `clearance_retract`; without it the tool
/// stays where it is), then turns A at that X and moves across: the tool's end must stay farther
/// from the A axis than the part reaches within the tool's radius of that X.
fn turn_check(ctx: &Ctx<'_>, prev: &Planned, p: &Planned) -> Result<()> {
    let post = ctx.post;
    let (Some(from), Some(to)) = (prev.poses.poses().last(), p.poses.poses().next()) else { return Ok(()) };
    let turns = match (&prev.op, &p.op) {
        (MachineOperation::Indexed(a), MachineOperation::Indexed(b)) => {
            a.rotary.len() != b.rotary.len() || a.rotary.iter().zip(&b.rotary).any(|(x, y)| (x - y).abs() > 1e-12)
        }
        _ => from.axis.dist(to.axis) > 1e-9,
    };
    if !turns {
        return Ok(());
    }
    // The tool's height in program coordinates: along its axis from the A axis.
    let height = |q: &Pose| q.tip.y * q.axis.y + q.tip.z * q.axis.z;
    let z = height(&from);
    // How far the tip is off to the side of the axis, square to the tool.
    let side = (from.tip.y.hypot(from.tip.z).powi(2) - z * z).max(0.0).sqrt();
    let rise = if post.clearance_retract {
        post.clearance.or_else(|| {
            p.poses
                .moves
                .iter()
                .filter_map(|m| match m {
                    Move5::Rapid { to } => Some(height(to)),
                    _ => None,
                })
                .reduce(f64::max)
        })
    } else {
        None
    };
    let up = rise.map_or(z, |c| c.max(z));
    let r = p.tool.radius();
    // The nearest the tool's end comes to the axis as it turns.
    let near = (side - r).max(0.0).hypot(up);
    let (lo, hi) = (from.tip.x - r, from.tip.x + r);
    let Some(reach) = ctx.part.and_then(|part| part_reach(part, lo, hi)) else { return Ok(()) };
    if near > reach + ctx.tol {
        return Ok(());
    }
    Err(ctx.bad(
        &p.report.name,
        format!(
            "A turns into it where operation `{}` leaves the tool, {} mm from the A axis at X {}, but the part reaches {} mm from it \
             between X {} and {}: it would swing into the tool. Raise the clearance of `{}` (or the post's) so the tool turns more \
             than {} mm from the axis",
            prev.report.name,
            num(near),
            num(from.tip.x),
            num(reach),
            num(lo),
            num(hi),
            prev.report.name,
            num(reach + ctx.tol)
        ),
    ))
}

/// The farthest the part reaches from the X axis between `lo` and `hi` along it: each triangle
/// cut to that slab, the farthest of its corners (distance from a line is convex, so a flat
/// piece is farthest at a corner). Read on the tessellated part, whose facets lie within the
/// chord tolerance of the exact surfaces.
fn part_reach(part: &Surface, lo: f64, hi: f64) -> Option<f64> {
    let mut top = f64::NEG_INFINITY;
    let mut poly: Vec<Vec3> = Vec::with_capacity(8);
    let mut next: Vec<Vec3> = Vec::with_capacity(8);
    for t in part.triangles() {
        let (tlo, thi) = t.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.x), b.max(p.x)));
        if thi < lo || tlo > hi {
            continue;
        }
        poly.clear();
        poly.extend_from_slice(t);
        for side in [1.0, -1.0] {
            // Keep where side·(x − bound) ≥ 0.
            let bound = if side > 0.0 { lo } else { hi };
            let keep = |p: &Vec3| side * (p.x - bound);
            next.clear();
            for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
                let (da, db) = (keep(a), keep(b));
                if da >= 0.0 {
                    next.push(*a);
                }
                if (da >= 0.0) != (db >= 0.0) {
                    next.push(a.lerp(*b, da / (da - db)));
                }
            }
            std::mem::swap(&mut poly, &mut next);
        }
        for p in &poly {
            top = top.max(p.y.hypot(p.z));
        }
    }
    top.is_finite().then_some(top)
}
