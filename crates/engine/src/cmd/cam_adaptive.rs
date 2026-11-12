//! Planning of the adaptive clearing operations of `cam.job` (`adaptive`, `adaptive_roughing`):
//! the operation's keys and the tool's data made into the CAM crate's parameters, the path
//! planned, and its engagement measured against the material (and, for `adaptive`, against a
//! plain offset pocket of the same profiles) for the report.

use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_cam::adaptive::{AdaptiveParams, AdaptivePath, AdaptiveRoughingParams, StockModel, adaptive_in, adaptive_roughing_in};
use solidcraft_cam::engagement::{Engagement, Material, engagement};
use solidcraft_cam::stock::Stock;
use solidcraft_cam::{Cut, Frame, PocketParams, Surface, Toolpath, pocket};

use super::{Ctx, Resolved, heights, or_tool};
use crate::Result;
use crate::schema::cam_adaptive::{AdaptiveOp, AdaptiveReport, AdaptiveRoughingOp, ComparedPath};

/// The engagement limits from the optimal load and what the operation gives: `(least, most)`
/// in degrees. The default least stays below the most the operation gives.
pub(super) fn limits(radius: f64, load: f64, min_deg: Option<f64>, max_deg: Option<f64>) -> (f64, f64) {
    let opt = if radius > 0.0 { (1.0 - load / radius).clamp(-1.0, 1.0).acos().to_degrees() } else { 90.0 };
    let most = max_deg.unwrap_or_else(|| (opt * 1.5).max(opt + 15.0).min(180.0));
    (min_deg.unwrap_or_else(|| (opt / 3.0).min(most * 0.5)), most)
}

/// How far above the limit the measured engagement may be before the report warns (degrees):
/// the planner works on a raster a sixteenth of the radius fine.
const OVER_LIMIT: f64 = 3.0;

/// Warnings for a measured path that broke what the operation asked for.
pub(super) fn engagement_warnings(rep: &AdaptiveReport) -> Vec<String> {
    let mut out = Vec::new();
    if !rep.measured {
        return out;
    }
    if rep.max_engagement_deg > rep.limit_deg + OVER_LIMIT {
        out.push(format!(
            "the measured engagement reaches {:.1}° outside the entries, above the limit of {:.1}°: check the path, or use a lighter optimal load",
            rep.max_engagement_deg, rep.limit_deg
        ));
    }
    if rep.full_slot_share > 0.0 {
        out.push(format!("the tool runs in a full slot for {:.1}% of the cutting time outside the entries", rep.full_slot_share * 100.0));
    }
    out
}

/// What both adaptive operations give the planner besides their own keys.
struct Knobs {
    load: Option<f64>,
    min_deg: Option<f64>,
    max_deg: Option<f64>,
    helix: Option<f64>,
    ramp: Option<f64>,
    climb: Option<bool>,
    leave: f64,
    tol: Option<f64>,
}

/// The adaptive parameters of either operation, checked the way every angle of a job is.
fn params(ctx: &Ctx<'_>, name: &str, r: &Resolved, k: Knobs) -> Result<AdaptiveParams> {
    let tool = &r.tool;
    let optimal_load = or_tool(k.load, tool.stepover, tool.diameter * 0.1);
    let (least, most) = limits(tool.radius(), optimal_load, k.min_deg, k.max_deg);
    Ok(AdaptiveParams {
        optimal_load,
        min_engagement: ctx.degrees(name, least, "`min_engagement_deg`")?,
        max_engagement: ctx.degrees(name, most, "`max_engagement_deg`")?,
        stock_to_leave: k.leave,
        tolerance: ctx.tolerance(name, k.tol)?,
        helix_diameter: k.helix.unwrap_or(tool.diameter * 0.8),
        ramp_angle: ctx.degrees(name, k.ramp.unwrap_or(2.0), "`ramp_angle_deg`")?,
        climb: k.climb.unwrap_or(true),
    })
}

/// Measures `path` against `material`; a path too large to measure is reported as not measured.
fn measure(
    path: &Toolpath,
    roles: Option<&[solidcraft_cam::adaptive::MoveRole]>,
    radius: f64,
    material: &Material,
    warnings: &mut Vec<String>,
) -> Option<Engagement> {
    match engagement(path, roles, radius, material, radius / 8.0) {
        Ok(e) => Some(e),
        Err(e) => {
            warnings.push(format!("the engagement was not measured: {e}"));
            None
        }
    }
}

fn report(a: &AdaptivePath, e: Option<Engagement>, limit_deg: f64) -> AdaptiveReport {
    let measured = e.is_some();
    let e = e.unwrap_or_default();
    AdaptiveReport {
        measured,
        max_engagement_deg: e.max_deg,
        max_entry_engagement_deg: e.max_entry_deg,
        mean_engagement_deg: e.mean_deg,
        full_slot_share: e.full_slot_share,
        entry_slot_share: e.entry_slot_share,
        limit_deg,
        levels: a.levels,
        helices: a.helices,
        lifts: a.lifts,
        left_area: a.left_area,
        corner_area: a.corner_area,
        pocket: None,
    }
}

/// The box round the job's stock (a bar: round its circle), seen in `frame`: its low and high
/// corners in the frame's coordinates.
fn box_in(stock: &Stock, frame: &Frame) -> (Vec3, Vec3) {
    let (min, max) = match *stock {
        Stock::Box { min, max } => (min, max),
        Stock::Cylinder { radius, x_min, x_max } => (Vec3::new(x_min, -radius, -radius), Vec3::new(x_max, radius, radius)),
    };
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for k in 0..8 {
        let c = Vec3::new(if k & 1 == 0 { min.x } else { max.x }, if k & 2 == 0 { min.y } else { max.y }, if k & 4 == 0 { min.z } else { max.z });
        let l = frame.to_local(c);
        lo = Vec3::new(lo.x.min(l.x), lo.y.min(l.y), lo.z.min(l.z));
        hi = Vec3::new(hi.x.max(l.x), hi.y.max(l.y), hi.z.max(l.z));
    }
    (lo, hi)
}

/// The job's stock seen in the operation's frame `frame`: looking straight down on a box (its Z
/// along work Z), the box round its corners there and its top. A bar, or a box seen down a tilted
/// frame, says nothing here (the regions are then taken as full of material), nor does no stock.
fn stock_in(stock: Option<&Stock>, frame: &Frame) -> StockModel {
    match stock {
        Some(st @ Stock::Box { .. }) if frame.z.z > 1.0 - 1e-9 => {
            let (lo, hi) = box_in(st, frame);
            StockModel { rect: Some([Vec2::new(lo.x, lo.y), Vec2::new(hi.x, hi.y)]), top: Some(hi.z), ..Default::default() }
        }
        _ => StockModel::default(),
    }
}

fn rect_region(lo: Vec2, hi: Vec2) -> Region2 {
    let p = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    let segs = (0..4).filter_map(|i| Some(Seg2::Line { a: *p.get(i)?, b: *p.get((i + 1) % 4)? })).collect();
    Region2 { outer: Loop2 { segs }, holes: vec![] }
}

/// `adaptive`: 2D adaptive clearing of profiles, on the job's stock box (with `from_stock`).
pub(super) fn plan(ctx: &Ctx<'_>, o: &AdaptiveOp, frame: &Frame) -> Result<(Toolpath, Resolved, &'static str)> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let _span = solidcraft_profile::span("cam", "adaptive.op");
    let regions = ctx.regions(c.name, o.sketch.as_ref(), o.profiles.as_deref(), o.polygon.as_deref(), frame)?;
    let cut = Cut { heights: heights(&o.heights), feeds: r.feeds, stepdown: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter) };
    let knobs = Knobs {
        load: o.optimal_load,
        min_deg: o.min_engagement_deg,
        max_deg: o.max_engagement_deg,
        helix: o.helix_diameter,
        ramp: o.ramp_angle_deg,
        climb: o.climb,
        leave: o.stock_to_leave.unwrap_or(0.0),
        tol: o.tolerance,
    };
    let p = params(ctx, c.name, &r, knobs)?;
    let stock = if o.from_stock.unwrap_or(true) { stock_in(ctx.stock, frame) } else { StockModel::default() };
    let a = solidcraft_profile::scope("cam", "adaptive", || adaptive_in(&regions, &r.tool, &p, &cut, &stock)).map_err(|e| ctx.bad(c.name, e))?;
    let mut warnings = a.warnings.clone();
    let material = Material { regions: regions.clone(), top: stock.top.map_or(cut.heights.top, |t| t.min(cut.heights.top)), within: stock.rect };
    let radius = r.tool.radius();
    let e = measure(&a.path, Some(&a.roles), radius, &material, &mut warnings);
    let mut rep = report(&a, e, p.max_engagement.to_degrees());
    warnings.extend(engagement_warnings(&rep));
    if o.compare.unwrap_or(true) {
        let pp = PocketParams {
            stepover: r.tool.diameter * 0.4,
            climb: p.climb,
            stock_to_leave: p.stock_to_leave,
            tolerance: p.tolerance,
            ramp_angle: 0.0,
        };
        match pocket(&regions, &r.tool, &pp, &cut) {
            Ok(plain) => {
                let stats = plain.stats();
                if let Some(pe) = measure(&plain, None, radius, &material, &mut warnings) {
                    rep.pocket = Some(ComparedPath {
                        max_engagement_deg: pe.max_deg,
                        full_slot_share: pe.full_slot_share,
                        cut_length: stats.cut_length,
                        cut_minutes: stats.cut_minutes,
                    });
                }
            }
            Err(e) => warnings.push(format!("the plain pocket to compare with could not be planned: {e}")),
        }
    }
    *ctx.adaptive.borrow_mut() = Some((rep, warnings));
    Ok((a.path, r, "adaptive"))
}

/// `adaptive_roughing`: 3D adaptive roughing of a stock box.
pub(super) fn plan_roughing(ctx: &Ctx<'_>, o: &AdaptiveRoughingOp, frame: &Frame) -> Result<(Toolpath, Resolved, &'static str)> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let surface: std::borrow::Cow<'_, Surface> = ctx.surface_in(c.name, frame)?;
    let knobs = Knobs {
        load: o.optimal_load,
        min_deg: o.min_engagement_deg,
        max_deg: o.max_engagement_deg,
        helix: o.helix_diameter,
        ramp: o.ramp_angle_deg,
        climb: o.climb,
        leave: o.stock_to_leave.unwrap_or(0.25),
        tol: o.tolerance,
    };
    let p = params(ctx, c.name, &r, knobs)?;
    let mut boundary = ctx.boundary(c.name, o.boundary.as_ref())?;
    // There is no material outside the job's stock: the boundary's rectangle (the stock as the
    // roughing takes it) is held to the stock's outline seen down the frame. The tool still goes
    // past it into the open air round it, by its radius and the room to enter and turn.
    if let (Some([lo, hi]), Some(st)) = (boundary.rect, ctx.stock) {
        let (slo, shi) = box_in(st, frame);
        let (lo, hi) = (Vec2::new(lo.x.max(slo.x), lo.y.max(slo.y)), Vec2::new(hi.x.min(shi.x), hi.y.min(shi.y)));
        if lo.x < hi.x && lo.y < hi.y {
            boundary.rect = Some([lo, hi]);
        }
    }
    let rp = AdaptiveRoughingParams {
        heights: heights(&o.heights),
        feeds: r.feeds,
        stepdown: or_tool(o.stepdown, r.tool.stepdown, r.tool.diameter),
        boundary: boundary.clone(),
        flats: o.flats.unwrap_or(true),
        adaptive: p,
        rest: None,
        step_up: o.step_up,
    };
    let a = adaptive_roughing_in(&surface, &r.tool, &rp, None).map_err(|e| ctx.bad(c.name, e))?;
    let mut warnings = a.warnings.clone();
    // The stock as the operation took it: the boundary's rectangle, else the part's extent, and
    // inside the boundary's polygon too when there is one.
    let stock = match boundary.rect {
        Some([lo, hi]) => Some((lo, hi)),
        None => {
            let b = surface.bounds();
            (!b.is_empty()).then(|| (Vec2::new(b.min.x, b.min.y), Vec2::new(b.max.x, b.max.y)))
        }
    };
    let e = match stock {
        Some((lo, hi)) => {
            let regions = vec![boundary.region.clone().unwrap_or_else(|| rect_region(lo, hi))];
            let material = Material { regions, top: rp.heights.top, within: Some([lo, hi]) };
            measure(&a.path, Some(&a.roles), r.tool.radius(), &material, &mut warnings)
        }
        None => None,
    };
    let rep = report(&a, e, p.max_engagement.to_degrees());
    warnings.extend(engagement_warnings(&rep));
    *ctx.adaptive.borrow_mut() = Some((rep, warnings));
    Ok((a.path, r, "adaptive_roughing"))
}
