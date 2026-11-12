//! The stock, facing and rapid safety of `cam.job` (a child of `cmd/cam.rs`): the job's stock
//! resolved and checked, the `face` operation planned on it, and every rapid and every position
//! of the written program checked against the part, the fixtures, the stock and the machine's
//! travel (docs/cam-jobs.md, "Stock, facing and rapid safety").

use solidcore_record::{Vec2, Vec3};
use solidcraft_cam::gcode5::{MachineOperation, MachineProgram, Post5};
use solidcraft_cam::indexed::{IndexedOperation, frame_for_index};
use solidcraft_cam::kinematics::Machine;
use solidcraft_cam::rapids::{RAPID_STEP, check_rapids_staged, check_travel};
use solidcraft_cam::stock::{FaceParams, Stock, default_face_overhang, default_face_stepover, face};
use solidcraft_cam::{Frame, Heights, Move5, Toolpath};

use super::{Ctx, Placement, Planned, Surfaces, cam_err, finite_point, num, v3};
use crate::Result;
use crate::params::bad;
use crate::schema::cam::{CamJob, OperationReport, OperationSpec};
use crate::schema::cam_stock::{AroundPartSpec, FaceOp, RapidHitReport, RapidsReport, StockReport, StockSpec, TravelReport};

/// Hits the reply lists in full.
const LISTED_HITS: usize = 20;
/// How far above the top (or the stock's top) a face operation's retract height is by default,
/// and its clearance height above that (mm).
const FACE_RETRACT: f64 = 2.0;
const FACE_CLEARANCE: f64 = 10.0;

/// The retract and clearance heights a face operation runs with: those given, else its retract
/// [`FACE_RETRACT`] above its `top` (or the stock's top, if that is higher) and its clearance
/// [`FACE_CLEARANCE`] above the retract.
fn face_heights(top: f64, stock_top: Option<f64>, retract: Option<f64>, clearance: Option<f64>) -> (f64, f64) {
    let retract = retract.unwrap_or(top.max(stock_top.unwrap_or(top)) + FACE_RETRACT);
    (retract, clearance.unwrap_or(retract + FACE_CLEARANCE))
}

/// The exact box round the bodies `names` in the work coordinates of `place`: each body's exact
/// box turned with the part (exact for the flips and quarter turns a second setup uses; round
/// another turn it is the box round the turned box). `None` without a solid body.
pub(super) fn work_bounds(s: &crate::Session, names: &[String], place: &Placement) -> Result<Option<solidcraft_geom::Aabb3>> {
    let st = s.world_state()?;
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh() && names.contains(&b.name)) {
        let bb = b.body.exact_bounds()?;
        for k in 0..8u8 {
            let c = Vec3::new(
                if k & 1 == 0 { bb.min.x } else { bb.max.x },
                if k & 2 == 0 { bb.min.y } else { bb.max.y },
                if k & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            let w = place.in_work(c);
            lo = Vec3::new(lo.x.min(w.x), lo.y.min(w.y), lo.z.min(w.z));
            hi = Vec3::new(hi.x.max(w.x), hi.y.max(w.y), hi.z.max(w.z));
        }
    }
    let finite = [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z].iter().all(|v| v.is_finite());
    Ok(finite.then_some(solidcraft_geom::Aabb3 { min: lo, max: hi }))
}

/// The job's stock in its work coordinates (`place`): a box, a bar, or the box round the exact
/// part (`part`, its bodies) grown by `around_part`'s margins. `None` without a stock.
pub(super) fn stock_of(cmd: &str, s: &crate::Session, job: &CamJob, part: &[String], place: &Placement) -> Result<Option<Stock>> {
    let Some(spec) = &job.stock else { return Ok(None) };
    let stock = match *spec {
        StockSpec::Box(b) => {
            finite_point(&[b.min, b.max].concat(), "`stock.box`").map_err(|e| bad(cmd, e))?;
            Stock::Box { min: v3(b.min), max: v3(b.max) }
        }
        StockSpec::Cylinder(c) => {
            finite_point(&[c.radius, c.x_min, c.x_max], "`stock.cylinder`").map_err(|e| bad(cmd, e))?;
            Stock::Cylinder { radius: c.radius, x_min: c.x_min, x_max: c.x_max }
        }
        StockSpec::AroundPart(a) => {
            let bounds = work_bounds(s, part, place)?.ok_or_else(|| bad(cmd, "the part has no extent: the stock round it has none"))?;
            around_part(cmd, &bounds, &a)?
        }
    };
    stock.validate().map_err(|e| cam_err(cmd, "`stock`", e))?;
    Ok(Some(stock))
}

/// The box round `part` grown by an `around_part` stock's margins: `margin` on every side, its
/// `top` and `bottom`, when given, above and below instead of `margin`'s Z.
fn around_part(cmd: &str, part: &solidcraft_geom::Aabb3, a: &AroundPartSpec) -> Result<Stock> {
    for (v, what) in [(a.top, "`stock.around_part.top`"), (a.bottom, "`stock.around_part.bottom`")] {
        if v.is_some_and(|v| !(v.is_finite() && (0.0..=1.0e4).contains(&v))) {
            return Err(bad(cmd, format!("{what} must be from 0 to 10 m")));
        }
    }
    let stock = Stock::around(part, v3(a.margin)).map_err(|e| cam_err(cmd, "`stock.around_part`", e))?;
    let Stock::Box { mut min, mut max } = stock else { return Ok(stock) };
    if let Some(top) = a.top {
        max.z = part.max.z + top;
    }
    if let Some(bottom) = a.bottom {
        min.z = part.min.z - bottom;
    }
    if !(min.z < max.z) {
        return Err(bad(cmd, "`stock.around_part`: the part is flat along Z: give the stock some `top` or `bottom`"));
    }
    let stock = Stock::Box { min, max };
    stock.validate().map_err(|e| cam_err(cmd, "`stock.around_part`", e))?;
    Ok(stock)
}

/// Fails each operation whose lowest cut goes below a box stock's bottom by more than `tol` on
/// a machine without rotaries: below it are the parallels, the vise's floor or the table that
/// hold the stock, or below the lowest fixture that reaches under it (`fixture_low`, read on
/// the fixtures' exact boxes). The deepest point of every cutting move with the tool upright
/// counts (a drill's point included). A job's `breakthrough` lets a cut go that far below the
/// bottom (onto a spoil board, or between parallels clear of it). Returns the operations (by
/// name, with how far below the bottom they go) that only pass by the breakthrough, with no
/// fixture under the stock to check them against.
pub(super) fn check_below_stock(
    planned: &[Planned],
    stock: &Stock,
    fixture_low: Option<f64>,
    breakthrough: f64,
    machine: &Machine,
    tol: f64,
    failures: &mut Vec<String>,
) -> Vec<(String, f64)> {
    let mut through = Vec::new();
    let Stock::Box { min, .. } = *stock else { return through };
    if !machine.rotaries.is_empty() {
        return through;
    }
    // Fixtures that reach below the stock (parallels it stands on, a vise's jaws) are what holds
    // it there, and every cut is checked against them: a cut through the stock may go on down
    // to where the lowest of them reaches.
    let held = fixture_low.filter(|z| z.is_finite() && *z < min.z);
    let modelled = held.unwrap_or(min.z);
    let floor = held.unwrap_or(min.z).min(min.z - breakthrough.max(0.0));
    let held = held.filter(|z| *z <= floor);
    for p in planned {
        let lowest = p
            .poses
            .moves
            .iter()
            .filter_map(|m| match m {
                Move5::Feed { to, .. } if to.axis.z >= 1.0 - 1e-9 => Some(to.tip.z),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min);
        if !(lowest.is_finite() && lowest < floor - tol) {
            if lowest.is_finite() && lowest < modelled - tol {
                through.push((p.report.name.clone(), modelled - lowest));
            }
            continue;
        }
        failures.push(match held {
            Some(z) => format!(
                "below the stock: operation `{}` cuts down to Z{}, {} mm below the lowest fixture (Z{}) under the stock's bottom (Z{}), into \
                 what holds them (the vise's floor or the table): raise its bottom to Z{} or above, or model what the stock stands on as \
                 fixtures reaching as deep as the cut",
                p.report.name,
                num(lowest),
                num(z - lowest),
                num(z),
                num(min.z),
                num(z)
            ),
            None => format!(
                "below the stock: operation `{}` cuts down to Z{}, {} mm below the stock's bottom (Z{}){}, into what holds the stock \
                 (parallels, the vise's floor or the table): raise its bottom to Z{} or above, or, when a spoil board or parallels clear \
                 of the cut leave room under the stock, give the job `breakthrough: {}`",
                p.report.name,
                num(lowest),
                num(min.z - lowest),
                num(min.z),
                if breakthrough > 0.0 { format!(" and past the job's `breakthrough` of {} mm", num(breakthrough)) } else { String::new() },
                num(floor),
                num(((min.z - lowest) * 10.0).ceil() / 10.0)
            ),
        });
    }
    through
}

/// The stock as the reply shows it.
pub(super) fn stock_report(stock: &Stock) -> StockReport {
    match *stock {
        Stock::Box { min, max } => {
            StockReport { kind: "box".into(), min: Some([min.x, min.y, min.z]), max: Some([max.x, max.y, max.z]), ..StockReport::default() }
        }
        Stock::Cylinder { radius, x_min, x_max } => {
            StockReport { kind: "cylinder".into(), radius: Some(radius), x_min: Some(x_min), x_max: Some(x_max), ..StockReport::default() }
        }
    }
}

fn finite_in_range(v: f64) -> bool {
    v.is_finite() && v.abs() <= 1.0e6
}

/// A `face` operation planned with every rotary at zero.
pub(super) fn plan_face(ctx: &Ctx<'_>, o: &FaceOp) -> Result<Planned> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let stock = ctx.stock;
    let mut warnings = Vec::new();
    let area = match (o.rect, stock) {
        (Some([lo, hi]), _) => {
            finite_point(&[lo[0], lo[1], hi[0], hi[1]], "`rect`").map_err(|e| ctx.bad(c.name, e))?;
            if !(lo[0] < hi[0] && lo[1] < hi[1]) {
                return Err(ctx.bad(c.name, "`rect` must be [[low x, low y], [high x, high y]]"));
            }
            [Vec2::new(lo[0], lo[1]), Vec2::new(hi[0], hi[1])]
        }
        (None, Some(s)) => s.footprint(),
        (None, None) => {
            let Some(part) = ctx.part else { return Err(ctx.bad(c.name, "give `rect`: the job has no stock and no part to face")) };
            let b = part.bounds();
            [Vec2::new(b.min.x, b.min.y), Vec2::new(b.max.x, b.max.y)]
        }
    };
    let top = match (o.top, stock) {
        (Some(t), _) => t,
        (None, Some(s)) => s.top(),
        (None, None) => return Err(ctx.bad(c.name, "give `top` (the job has no `stock` to face the top of)")),
    };
    for (v, what) in [(Some(o.bottom), "`bottom`"), (Some(top), "`top`"), (o.retract, "`retract`"), (o.clearance, "`clearance`")] {
        if v.is_some_and(|v| !finite_in_range(v)) {
            return Err(ctx.bad(c.name, format!("{what} must be a finite height within 1000 m")));
        }
    }
    let stock_top = stock.map(Stock::top);
    if let Some(st) = stock_top
        && top < st - ctx.tol
    {
        warnings.push(format!("the top ({}) is below the stock's top ({}): the first level takes more than a stepdown", num(top), num(st)));
    }
    let (retract, clearance) = face_heights(top, stock_top, o.retract, o.clearance);
    let tool = &r.tool;
    let params = FaceParams {
        area,
        heights: Heights { clearance, retract, top, bottom: o.bottom },
        feeds: r.feeds,
        stepdown: o.stepdown.or(tool.stepdown).unwrap_or(1.0),
        stepover: o.stepover.or(tool.stepover).unwrap_or_else(|| default_face_stepover(tool)),
        angle: ctx.degrees(c.name, o.angle_deg.unwrap_or(0.0), "`angle_deg`")?,
        overhang: o.overhang.unwrap_or_else(|| default_face_overhang(tool)),
        zigzag: o.zigzag.unwrap_or(true),
        stock: stock.copied(),
        lead_radius: o.lead_radius,
    };
    let path: Toolpath = face(tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    let rotary = vec![0.0; ctx.machine.rotaries.len()];
    // The passes are in work coordinates: the machine must hold the work square with every
    // rotary at zero (it does on `3axis` and `4axis_a`).
    if frame_for_index(ctx.machine, &rotary).map_err(|e| ctx.bad(c.name, e))? != Frame::IDENTITY {
        return Err(ctx.bad(c.name, "facing needs a machine whose rotaries at zero leave the work coordinates as they are"));
    }
    let stats = path.stats();
    let io = IndexedOperation { name: c.name.into(), tool: tool.clone(), spindle_rpm: r.rpm, coolant: c.coolant, rotary, path };
    let (poses, sources) = io.poses_mapped(ctx.machine, ctx.pose_tol).map_err(|e| ctx.bad(c.name, e))?;
    let report = OperationReport {
        name: c.name.into(),
        kind: "face".into(),
        tool: tool.number,
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
        feeds: r.feeds,
        gouge_check: c.gouge_check,
        band: None,
        tool: r.tool,
        report,
        axes: "3-axis".into(),
    })
}

/// What is left of the job's stock as each of its operations starts: a `face` over the whole
/// top of a box stock lowers it to the face's bottom for the operations after it. Only with the
/// default overhang, which puts the outermost passes' centres outside the edges, so the flat of
/// the tool's end reaches them (a smaller one may leave a bull nose's corner along an edge).
fn stock_stages(job: &CamJob, stock: Option<&Stock>) -> Vec<Option<Stock>> {
    let mut left = stock.copied();
    job.operations
        .iter()
        .filter(|spec| super::op_enabled(spec))
        .map(|spec| {
            let now = left;
            if let (OperationSpec::Face(o), Some(s)) = (spec, left)
                && o.overhang.is_none()
            {
                let area = o.rect.map_or_else(|| s.footprint(), |[lo, hi]| [Vec2::new(lo[0], lo[1]), Vec2::new(hi[0], hi[1])]);
                left = Some(s.faced(area, o.bottom));
            }
            now
        })
        .collect()
}

fn at(p: Vec3) -> String {
    format!("[{}, {}, {}]", num(p.x), num(p.y), num(p.z))
}

/// What the rapid and travel checks need of the job as written.
pub(super) struct Written<'a> {
    pub text: &'a str,
    pub program: &'a MachineProgram,
    pub post: &'a Post5,
    pub machine: &'a Machine,
}

/// Every rapid of the written program against the part, fixtures and stock, and every position
/// against the machine's travel; each failed check adds to `failures`. `unchecked` are the
/// operations (in program order) whose cuts are not held to the part (no gouge check, or a
/// chamfer mill's own band), so a rapid of theirs that leaves what they cut is not either.
pub(super) fn check_motion(
    cmd: &str,
    job: &CamJob,
    w: &Written<'_>,
    surfaces: &Surfaces,
    stock: Option<&Stock>,
    unchecked: &[bool],
    tol: f64,
    failures: &mut Vec<String>,
) -> Result<(RapidsReport, Option<TravelReport>)> {
    let program = w.program;
    let stages = stock_stages(job, stock);
    let stages = if stages.len() == program.operations.len() { stages } else { Vec::new() };
    let unchecked = if unchecked.len() == program.operations.len() { unchecked } else { &[] };
    let check = check_rapids_staged(w.text, program, w.post, w.machine, &surfaces.all, &surfaces.part, stock, &stages, unchecked, tol, RAPID_STEP)
        .map_err(|e| cam_err(cmd, "checking the rapids", e))?;
    let op_name = |k: Option<usize>| {
        k.and_then(|k| program.operations.get(k)).map(|o| match o {
            MachineOperation::Continuous(c) => c.name.clone(),
            MachineOperation::Indexed(i) => i.name.clone(),
        })
    };
    let first: Vec<RapidHitReport> = check
        .hits
        .iter()
        .take(LISTED_HITS)
        .map(|h| RapidHitReport {
            line: h.line,
            operation: op_name(h.operation),
            kind: serde_json::to_value(h.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
            depth: h.depth,
            at: [h.at.x, h.at.y, h.at.z],
        })
        .collect();
    if let Some(h) = check.hits.first() {
        let op = op_name(h.operation).map_or(String::new(), |n| format!(" (operation `{n}`)"));
        failures.push(format!(
            "rapids: {} rapid blocks run into something; the first, line {}{op}, {} by {} mm at {}",
            check.hit_count,
            h.line,
            h.kind.label(),
            num(h.depth),
            at(h.at)
        ));
    }
    let rapids = RapidsReport {
        checked: check.checked,
        hits: check.hit_count,
        worst: check.worst,
        pass: check.pass(),
        from_unknown: check.from_unknown,
        first,
    };
    let travel = check_travel(w.text, w.machine).map_err(|e| cam_err(cmd, "checking the travel", e))?.map(|t| {
        let violation =
            t.violation.map(|v| format!("line {}: {} at {} is beyond its travel {} to {}", v.line, v.axis, num(v.value), num(v.low), num(v.high)));
        if let Some(v) = &violation {
            failures.push(format!("travel: {v}"));
        }
        TravelReport { checked: t.checked, pass: violation.is_none(), violation }
    });
    Ok((rapids, travel))
}
