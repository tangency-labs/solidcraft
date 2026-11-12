//! The 3D finishing operations of `cam.job`: `scallop`, `steep_shallow`, `pencil` and
//! `rest_finish`, each read from its contract (`schema/cam_finish3d.rs`) into the CAM crate's
//! parameters and planned on the part's surface in the operation's frame, as the other 3-axis
//! operations are.

use solidcraft_cam::finish3d::{
    OffsetDirection, PencilParams, RestParams, ScallopParams, ShallowPattern, Spacing, SteepShallowParams, pencil, rest_finish, scallop,
    steep_shallow,
};
use solidcraft_cam::{Surface, Tool};

use super::{Ctx, Planned3, heights, or_tool, split_budget};
use crate::Result;
use crate::schema::cam::{DirectionSpec, PencilOp, RestFinishOp, ScallopOp, ShallowSpec, SteepShallowOp};

/// The spacing of an operation: its scallop height, or its stepover (the tool's, else 10% of
/// its diameter), not both.
fn spacing(ctx: &Ctx<'_>, op: &str, scallop_height: Option<f64>, stepover: Option<f64>, tool: &Tool) -> Result<Spacing> {
    match (scallop_height, stepover) {
        (Some(_), Some(_)) => Err(ctx.bad(op, "give `scallop_height` or `stepover`, not both")),
        (Some(h), None) => Ok(Spacing::ScallopHeight(h)),
        (None, s) => Ok(Spacing::Stepover(or_tool(s, tool.stepover, tool.diameter * 0.1))),
    }
}

/// The default grid and sample step: a tenth of the tool's diameter.
fn step(v: Option<f64>, tool: &Tool) -> f64 {
    v.unwrap_or(tool.diameter * 0.1)
}

pub(super) fn plan_scallop(ctx: &Ctx<'_>, o: &ScallopOp, surface: &Surface) -> Result<Planned3> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let (tolerance, merge) = split_budget(ctx.surface_budget(c.name, o.tolerance)?);
    let params = ScallopParams {
        heights: heights(&o.heights),
        feeds: r.feeds,
        descent_feed: r.descent,
        spacing: spacing(ctx, c.name, o.scallop_height, o.stepover, &r.tool)?,
        direction: match o.direction.unwrap_or(DirectionSpec::Inward) {
            DirectionSpec::Inward => OffsetDirection::Inward,
            DirectionSpec::Outward => OffsetDirection::Outward,
        },
        boundary: ctx.boundary(c.name, o.boundary.as_ref())?,
        stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
        step: step(o.step, &r.tool),
        tolerance,
        climb: o.climb.unwrap_or(true),
    };
    let path = scallop(surface, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok((path, r, "scallop", merge))
}

pub(super) fn plan_steep_shallow(ctx: &Ctx<'_>, o: &SteepShallowOp, surface: &Surface) -> Result<Planned3> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let (tolerance, merge) = split_budget(ctx.surface_budget(c.name, o.tolerance)?);
    let shallow = match o.shallow.unwrap_or(ShallowSpec::Scallop) {
        ShallowSpec::Scallop => {
            if o.angle_deg.is_some() {
                return Err(ctx.bad(c.name, "`angle_deg` goes with `shallow: \"parallel\"`"));
            }
            ShallowPattern::Scallop
        }
        ShallowSpec::Parallel => ShallowPattern::Parallel { angle: ctx.degrees(c.name, o.angle_deg.unwrap_or(0.0), "`angle_deg`")? },
    };
    let params = SteepShallowParams {
        heights: heights(&o.heights),
        feeds: r.feeds,
        descent_feed: r.descent,
        spacing: spacing(ctx, c.name, o.scallop_height, o.stepover, &r.tool)?,
        threshold: ctx.degrees(c.name, o.threshold_deg.unwrap_or(40.0), "`threshold_deg`")?,
        overlap: o.overlap.unwrap_or(r.tool.diameter * 0.1),
        stepdown: o.stepdown,
        shallow,
        boundary: ctx.boundary(c.name, o.boundary.as_ref())?,
        stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
        step: step(o.step, &r.tool),
        tolerance,
        climb: o.climb.unwrap_or(true),
    };
    let path = steep_shallow(surface, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok((path, r, "steep_shallow", merge))
}

pub(super) fn plan_pencil(ctx: &Ctx<'_>, o: &PencilOp, surface: &Surface) -> Result<Planned3> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let (tolerance, merge) = split_budget(ctx.surface_budget(c.name, o.tolerance)?);
    let params = PencilParams {
        heights: heights(&o.heights),
        feeds: r.feeds,
        descent_feed: r.descent,
        min_angle: ctx.degrees(c.name, o.min_angle_deg.unwrap_or(30.0), "`min_angle_deg`")?,
        passes: o.passes.unwrap_or(0),
        stepover: or_tool(o.stepover, r.tool.stepover, r.tool.diameter * 0.1),
        min_length: o.min_length.unwrap_or(r.tool.radius()),
        boundary: ctx.boundary(c.name, o.boundary.as_ref())?,
        stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
        step: step(o.step, &r.tool),
        tolerance,
        climb: o.climb.unwrap_or(true),
    };
    let path = pencil(surface, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok((path, r, "pencil", merge))
}

pub(super) fn plan_rest_finish(ctx: &Ctx<'_>, o: &RestFinishOp, surface: &Surface) -> Result<Planned3> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let (tolerance, merge) = split_budget(ctx.surface_budget(c.name, o.tolerance)?);
    let Some(previous) = ctx.tools.iter().find(|t| t.number == o.previous_tool) else {
        return Err(ctx.bad(c.name, format!("no tool number {} (`previous_tool`)", o.previous_tool)));
    };
    let params = RestParams {
        heights: heights(&o.heights),
        feeds: r.feeds,
        descent_feed: r.descent,
        previous: previous.clone(),
        threshold: o.threshold.unwrap_or(2.0 * ctx.tolerance(c.name, o.tolerance)?),
        spacing: spacing(ctx, c.name, o.scallop_height, o.stepover, &r.tool)?,
        overlap: o.overlap.unwrap_or(r.tool.diameter * 0.25),
        boundary: ctx.boundary(c.name, o.boundary.as_ref())?,
        stock_to_leave: o.stock_to_leave.unwrap_or(0.0),
        step: step(o.step, &r.tool),
        tolerance,
        climb: o.climb.unwrap_or(true),
    };
    let path = rest_finish(surface, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    Ok((path, r, "rest_finish", merge))
}
