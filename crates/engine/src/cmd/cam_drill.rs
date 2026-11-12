//! The `drill` operation of `cam.job` (docs/cam-jobs.md, "Drilling"): the holes from a sketch's
//! circle centres and points or from explicit points, in the operation's frame (work
//! coordinates, or the frame an `indexed` operation's angle brings under the tool), drilled by
//! `solidcraft_cam::drill`. A chamfer mill spots the holes: the cone it cuts at each is the
//! operation's own band, which its gouge check allows.

use std::collections::BTreeSet;

use solidcore_record::Vec2;
use solidcraft_cam::drill::{Cycle, DrillParams, check_approach, drill};
use solidcraft_cam::{Frame, ToolKind, Toolpath};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::CurveKind;

use super::{Ctx, Resolved, finite_point, heights, or_tool};
use crate::Result;
use crate::schema::cam::{DrillCycleSpec, DrillOp};

/// Default distance (mm) a peck returns to above the depth reached, and a chip break backs off.
const DEFAULT_PECK_GAP: f64 = 0.5;
/// Default dwell at depth (seconds).
const DEFAULT_DWELL: f64 = 0.5;
/// Most explicit points read (the CAM crate drills at most `MAX_HOLES` of them).
const MAX_POINTS: usize = 100_000;

/// The drilling toolpath of `o` in `frame`.
pub(super) fn plan(ctx: &Ctx<'_>, o: &DrillOp, frame: &Frame) -> Result<(Toolpath, Resolved)> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let points = holes(ctx, o, frame)?;
    let cycle_kind = o.cycle.unwrap_or(DrillCycleSpec::Simple);
    let pecks = matches!(cycle_kind, DrillCycleSpec::Peck | DrillCycleSpec::ChipBreak);
    let refuse = |key: &str, cycles: &str| Err(ctx.bad(c.name, format!("`{key}` goes with `cycle` {cycles}")));
    if o.peck.is_some() && !pecks {
        return refuse("peck", "`peck` or `chip_break`");
    }
    if o.peck_clearance.is_some() && cycle_kind != DrillCycleSpec::Peck {
        return refuse("peck_clearance", "`peck`");
    }
    if o.back_off.is_some() && cycle_kind != DrillCycleSpec::ChipBreak {
        return refuse("back_off", "`chip_break`");
    }
    if o.dwell_seconds.is_some() && cycle_kind != DrillCycleSpec::Dwell {
        return refuse("dwell_seconds", "`dwell`");
    }
    let peck = or_tool(o.peck, r.tool.stepdown, r.tool.diameter);
    let cycle = match cycle_kind {
        DrillCycleSpec::Simple => Cycle::Simple,
        DrillCycleSpec::Dwell => Cycle::Dwell { seconds: o.dwell_seconds.unwrap_or(DEFAULT_DWELL) },
        DrillCycleSpec::Peck => Cycle::Peck { peck, clearance: o.peck_clearance.unwrap_or(DEFAULT_PECK_GAP) },
        DrillCycleSpec::ChipBreak => Cycle::ChipBreak { peck, back_off: o.back_off.unwrap_or(DEFAULT_PECK_GAP) },
    };
    let params = DrillParams {
        cycle,
        heights: heights(&o.heights),
        feed: r.feeds.plunge,
        to_full_diameter: o.to_full_diameter.unwrap_or(false),
        stay_at_retract: false,
        part_top: None,
    };
    let path = drill(&points, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    if r.tool.kind == ToolKind::Chamfer {
        // Spotting: the cone at each hole is the operation's own (the hole's chamfer).
        ctx.band.set(Some(params.heights.top - params.heights.bottom));
    }
    // The tool comes down to R at rapid over every hole: nothing of the part or the fixtures may
    // be above R round one (a hole in a boss drilled with the plate's heights).
    let all = ctx.all_in(c.name, frame)?;
    check_approach(&all, &r.tool, &points, params.heights.retract, ctx.tol).map_err(|e| ctx.bad(c.name, e))?;
    Ok((path, r))
}

/// The hole positions of `o`, in `frame`'s XY.
fn holes(ctx: &Ctx<'_>, o: &DrillOp, frame: &Frame) -> Result<Vec<Vec2>> {
    let op = o.name.as_str();
    match (&o.sketch, &o.points) {
        (Some(_), Some(_)) => Err(ctx.bad(op, "give `sketch` or `points`, not both")),
        (None, None) => Err(ctx.bad(op, "give a `sketch` (its circle centres and points are the holes) or `points`")),
        (None, Some(pts)) => {
            if o.circles.is_some() || o.diameter_range.is_some() || o.sketch_points.is_some() {
                return Err(ctx.bad(op, "`circles`, `diameter_range` and `sketch_points` go with `sketch`"));
            }
            if pts.len() > MAX_POINTS {
                return Err(ctx.bad(op, format!("more than {MAX_POINTS} points")));
            }
            pts.iter()
                .map(|p| {
                    finite_point(p, "`points`").map_err(|e| ctx.bad(op, e))?;
                    Ok(Vec2::new(p[0], p[1]))
                })
                .collect()
        }
        (Some(key), None) => sketch_holes(ctx, o, &key.0, frame),
    }
}

/// The centres of the circles and the lone points of sketch `key`, in `frame`'s XY.
fn sketch_holes(ctx: &Ctx<'_>, o: &DrillOp, key: &str, frame: &Frame) -> Result<Vec<Vec2>> {
    let op = o.name.as_str();
    let (circles, points) = (o.circles.unwrap_or(true), o.sketch_points.unwrap_or(true));
    if !circles && !points {
        return Err(ctx.bad(op, "with `circles: false` and `sketch_points: false` the sketch gives no holes"));
    }
    let range = match o.diameter_range {
        None => None,
        Some(_) if !circles => return Err(ctx.bad(op, "`diameter_range` goes with `circles`")),
        Some([lo, hi]) => {
            finite_point(&[lo, hi], "`diameter_range`").map_err(|e| ctx.bad(op, e))?;
            if !(0.0..=hi).contains(&lo) {
                return Err(ctx.bad(op, "`diameter_range` must be [smallest, largest], 0 or more, the smallest first"));
            }
            Some((lo, hi))
        }
    };
    let id = match ctx.s.doc.find_feature(key) {
        Some(f) if matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. }) => f.id,
        _ => return Err(ctx.bad(op, format!("no sketch `{key}`"))),
    };
    let st = ctx.s.model.state();
    let Some(ss) = st.sketch(id) else { return Err(ctx.bad(op, format!("sketch `{key}` has not been solved"))) };
    let plane = &ss.plane;
    let normal = frame.dir_to_local(ctx.place.turn_dir(plane.x.cross(plane.y)));
    if !(normal.z.abs() > 1.0 - 1e-6) {
        return Err(ctx.bad(op, format!("sketch `{key}` is not on a plane square to the operation's tool axis")));
    }
    let map = |q: Vec2| {
        let l = frame.to_local(ctx.place.in_work(plane.origin + plane.x * q.x + plane.y * q.y));
        Vec2::new(l.x, l.y)
    };
    let sketch = &ss.sketch;
    let mut out = Vec::new();
    if circles {
        for curve in sketch.curves.iter().filter(|c| !c.construction && !c.centerline) {
            if let CurveKind::Circle { c, r } = curve.kind
                && range.is_none_or(|(lo, hi)| 2.0 * r >= lo - 1e-6 && 2.0 * r <= hi + 1e-6)
                && let Some(centre) = sketch.point(c)
            {
                out.push(map(centre));
            }
        }
    }
    if points {
        let used: BTreeSet<usize> = sketch.curves.iter().flat_map(|c| c.kind.point_ids()).collect();
        for (i, p) in sketch.points.iter().enumerate() {
            if i != 0 && p.id != "origin" && !used.contains(&i) {
                out.push(map(p.pos));
            }
        }
    }
    if out.is_empty() {
        let what = match (circles, points, range) {
            (true, true, None) => "no circles or points",
            (true, true, Some(_)) => "no circles in `diameter_range` and no points",
            (true, false, None) => "no circles",
            (true, false, Some(_)) => "no circles in `diameter_range`",
            (false, _, _) => "no points that are not part of a curve",
        };
        return Err(ctx.bad(op, format!("sketch `{key}` has {what} to drill")));
    }
    Ok(out)
}
