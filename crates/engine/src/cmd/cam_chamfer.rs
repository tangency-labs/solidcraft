//! The `chamfer` operation of `cam.job`: a chamfer or deburr along closed profiles' top edges
//! with a chamfer mill, through the CAM crate's `chamfer` (docs/cam-jobs.md). The crate works
//! out the tool's offset and depth from its angle so that the chamfer is as wide as asked, and
//! refuses a chamfer the tool's cone cannot reach. The chamfer need not be in the model: the
//! gouge check allows the band the operation is told to cut ([`solidcraft_cam::band_gouges`]).

use solidcore_record::{Loop2, Region2, Vec2};
use solidcraft_cam::{ChamferParams, DEFAULT_TIP_OFFSET, Frame, Side, Toolpath, chamfer, chamfer_geometry};

use super::{Ctx, Resolved, finite_point};
use crate::Result;
use crate::schema::cam::{ChamferOp, SideSpec};

/// Most holes one chamfer operation takes.
const MAX_HOLES: usize = 10_000;

/// The chamfer's toolpath in `frame`'s coordinates, and what it cuts with.
pub(super) fn plan(ctx: &Ctx<'_>, o: &ChamferOp, frame: &Frame) -> Result<(Toolpath, Resolved)> {
    let c = o.common();
    let r = ctx.resolve(&c)?;
    let (regions, side) = match &o.holes {
        Some(_) if o.sketch.is_some() || o.polygon.is_some() || o.profiles.is_some() => {
            return Err(ctx.bad(c.name, "give a `sketch`, a `polygon` or `holes`, one of them"));
        }
        Some(holes) => {
            if holes.is_empty() || holes.len() > MAX_HOLES {
                return Err(ctx.bad(c.name, format!("give 1 to {MAX_HOLES} `holes`")));
            }
            if o.side.is_some_and(|s| !matches!(s, SideSpec::Inside)) {
                return Err(ctx.bad(c.name, "a hole's rim is chamfered `inside` the hole"));
            }
            let mut regions = Vec::with_capacity(holes.len());
            for h in holes {
                finite_point(&[h.center[0], h.center[1], h.diameter], "a hole's `center` and `diameter`").map_err(|e| ctx.bad(c.name, e))?;
                if !(h.diameter > 0.0) {
                    return Err(ctx.bad(c.name, "a hole's `diameter` must be above 0"));
                }
                regions.push(Region2 { outer: Loop2::circle(Vec2::new(h.center[0], h.center[1]), h.diameter * 0.5), holes: Vec::new() });
            }
            (regions, Side::Inside)
        }
        None => {
            let regions = ctx.regions(c.name, o.sketch.as_ref(), o.profiles.as_deref(), o.polygon.as_deref(), frame)?;
            let side = match o.side.unwrap_or(SideSpec::Outside) {
                SideSpec::Outside => Side::Outside,
                SideSpec::Inside => Side::Inside,
                SideSpec::On => return Err(ctx.bad(c.name, "a chamfer is cut `outside` a part or `inside` an opening, not `on` the profile")),
            };
            (regions, side)
        }
    };
    let h = o.heights;
    let params = ChamferParams {
        width: o.width,
        tip_offset: o.tip_offset.unwrap_or(DEFAULT_TIP_OFFSET),
        side,
        climb: o.climb.unwrap_or(true),
        tolerance: ctx.tolerance(c.name, o.tolerance)?,
        clearance: h.clearance,
        retract: h.retract,
        top: h.top,
        feeds: r.feeds,
    };
    let path = chamfer(&regions, &r.tool, &params).map_err(|e| ctx.bad(c.name, e))?;
    // The chamfer itself is the operation's own: its gouge check allows it.
    let g = chamfer_geometry(&r.tool, params.width, params.tip_offset).map_err(|e| ctx.bad(c.name, e))?;
    ctx.band.set(Some(g.height));
    Ok((path, r))
}
