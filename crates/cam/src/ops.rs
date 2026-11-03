//! 2.5D operations: contour, pocket, and adaptive clearing. Each one turns
//! planar geometry and cutting data into a [`Toolpath`]: it validates the
//! input, prepares regions, runs the engine (one plan per region and depth
//! level), translates the engine's commands into checked moves, and checks
//! the result with [`verify`] before returning it.

use raygeo::ops::types::CutDirection;
use solidcraft_geom::{Region2, Vec3};

use crate::check::verify;
use crate::engine::translate::Clamp;
use crate::engine::{geometry, plan, translate};
use crate::offset::{Shape, offset_regions};
use crate::params::{AdaptiveParams, ContourParams, Cut, PocketParams, Side, check_leave, check_ramp, check_tolerance, mills};
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Most moves in one operation.
const MOVES_LIMIT: usize = MAX_MOVES;

fn push_checked(ops: &raygeo::ops::container::Ops, cut: &Cut, moves: &mut Vec<Move>, pos: &mut Option<Vec3>, clamp: &Clamp) -> Result<()> {
    translate::append(ops, cut, moves, pos, clamp)?;
    if moves.len() > MOVES_LIMIT {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MOVES_LIMIT} moves")));
    }
    Ok(())
}

fn finish(moves: Vec<Move>, heights: &crate::params::Heights, empty_why: &str) -> Result<Toolpath> {
    let cuts_any = moves.iter().any(|m| matches!(m, Move::Feed { .. } | Move::Arc { .. }));
    if !cuts_any {
        return invalid(empty_why);
    }
    let path = Toolpath { moves };
    verify(&path, heights)?;
    Ok(path)
}

/// The walking direction that reproduces the operation's climb convention
/// for `side`: the engine's CCW walks a loop as stored (the wall on the
/// left of an inner loop, on the right of an outer one), which is the
/// convention the region offsetting built.
fn direction(side: Side, climb: bool) -> CutDirection {
    let ccw_is_climb = side != Side::Outside;
    match (climb, ccw_is_climb) {
        (true, true) | (false, false) => CutDirection::Ccw,
        (true, false) | (false, true) => CutDirection::Cw,
    }
}

/// Follows the boundaries of `regions` on the chosen side, one loop after
/// the other, each down through every depth pass before the next. Runs on
/// the engine's exact profile passes: walls come out as exact lines and
/// arcs for line-and-arc boundaries, with a ramp along the wall when a ramp
/// angle is set.
pub fn contour(regions: &[Region2], tool: &Tool, params: &ContourParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    check_tolerance(params.tolerance)?;
    if regions.is_empty() {
        return invalid("the operation has no regions");
    }
    // The profile engine walks the exact offset itself: the translator
    // clamps nothing, so the roll-corner arcs survive as arcs.
    let clamp = Clamp::Exact;

    let mut moves: Vec<Move> = Vec::new();
    let mut pos: Option<Vec3> = None;
    let direction = direction(params.side, params.climb);
    let run_group = |geometry: raygeo::geo::geometry::Geometry,
                     kind: raygeo::ops::assembly::profile::ProfileKind,
                     wall_margin: f64,
                     stock: f64,
                     moves: &mut Vec<Move>,
                     pos: &mut Option<Vec3>|
     -> Result<()> {
        for z in cut.levels()? {
            let ops = plan::profile_level(&geometry, kind, tool, wall_margin, stock, params.ramp_angle, params.tolerance, direction, cut, z)?;
            push_checked(&ops, cut, moves, pos, &clamp)?;
        }
        Ok(())
    };

    for region in regions {
        match params.side {
            Side::Inside | Side::On => {
                // The tool inside the opening: the inset boundary and the
                // grown islands in one pass group. On the boundary, the
                // offset is zero: the engine folds tool radius and stock
                // into its offset, so they cancel out here.
                let on = params.side == Side::On;
                let geometry = geometry::loop_group(region, params.tolerance);
                let _ = &geometry;
                run_group(
                    geometry,
                    raygeo::ops::assembly::profile::ProfileKind::Inner,
                    if on { -tool.radius() } else { 0.0 },
                    if on { 0.0 } else { params.stock_to_leave },
                    &mut moves,
                    &mut pos,
                )?;
            }
            Side::Outside => {
                // The regions are the part: profile around the outside, then
                // around each hole's wall (the tool inside the opening).
                run_group(
                    geometry::loop_geometry(&region.outer, params.tolerance),
                    raygeo::ops::assembly::profile::ProfileKind::Outer,
                    0.0,
                    params.stock_to_leave,
                    &mut moves,
                    &mut pos,
                )?;
                for hole in &region.holes {
                    run_group(
                        geometry::loop_geometry(hole, params.tolerance),
                        raygeo::ops::assembly::profile::ProfileKind::Inner,
                        0.0,
                        params.stock_to_leave,
                        &mut moves,
                        &mut pos,
                    )?;
                }
            }
        }
    }
    finish(moves, &cut.heights, "the tool does not fit the profile")
}

/// Clears `regions` (holes are islands): the engine's clearing planner
/// splits each pocket around narrow passages and orders the areas, keeps
/// the engagement constant, and a finishing wall pass cleans the walls to
/// the exact offset.
pub fn pocket(regions: &[Region2], tool: &Tool, params: &PocketParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    check_tolerance(params.tolerance)?;
    let r = tool.radius();
    // A pass closer than a micron to the previous one explodes the
    // offsetting stack's numerics for nothing machinable.
    if !(params.stepover >= 1.0e-3) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be between 1 micron and the tool diameter");
    }
    if regions.is_empty() {
        return invalid("the operation has no regions");
    }
    // The stock to leave comes off the walls before the engine saw the
    // pocket (the crate's own offsetting, as preparer); a region that
    // vanishes was too narrow for the tool and the stock.
    let prepared = prepare_stock(regions, params.stock_to_leave, params.tolerance)?;
    if prepared.is_empty() {
        return invalid("the tool does not fit the pocket with that stock to leave");
    }
    // Climb/conventional cannot be forwarded yet: plan_clearing hardcodes
    // its cut direction upstream.
    let mut moves: Vec<Move> = Vec::new();
    let mut pos: Option<Vec3> = None;
    for region in &prepared {
        // The oracle the translator clamps against: the prepared region has
        // the stock to leave baked in, so the tool center stays a radius
        // from it.
        let shape = Shape::from_regions(std::slice::from_ref(region), params.tolerance)?;
        let clamp = Clamp::Region { shape, keep_out: r };
        let (boundary, islands, size) = plan::clearing_input(region, params.tolerance);
        if boundary.len() < 3 {
            return invalid("the tool does not fit the pocket");
        }
        for z in cut.levels()? {
            let ops = plan::clearing_level(&boundary, &islands, size, tool, params.stepover, params.ramp_angle, true, cut, z)?;
            push_checked(&ops, cut, &mut moves, &mut pos, &clamp)?;
        }
    }
    finish(moves, &cut.heights, "the tool does not fit the pocket")
}

/// Clears each region (holes are islands) with constant-engagement passes,
/// one depth level after the other, region by region.
pub fn adaptive(regions: &[Region2], tool: &Tool, params: &AdaptiveParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    if regions.is_empty() {
        return invalid("the operation has no regions");
    }
    // A pass closer than a micron to the previous one explodes the
    // offsetting stack's numerics for nothing machinable.
    if !(params.stepover >= 1.0e-3) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be between 1 micron and the tool diameter");
    }
    if !params.stock_to_leave.is_finite() || !(0.0..=crate::params::MAX_COORD).contains(&params.stock_to_leave) {
        return invalid("the stock to leave must be 0 or more");
    }
    if !params.tolerance.is_finite() || !(1e-6..=1.0).contains(&params.tolerance) {
        return invalid("the tolerance must be above 0 and at most 1 mm");
    }
    // The stock to leave comes off the walls before the engine saw the
    // pocket (the crate's own offsetting, as preparer). A region that
    // vanishes was too narrow for the tool and the stock; an island grows
    // by the same amount.
    let prepared = prepare_stock(regions, params.stock_to_leave, params.tolerance)?;
    if prepared.is_empty() {
        return invalid("the tool does not fit the pocket with that stock to leave");
    }
    // Climb/conventional cannot be forwarded yet: plan_clearing hardcodes
    // its cut direction upstream.
    let mut moves: Vec<Move> = Vec::new();
    let mut pos: Option<Vec3> = None;
    for region in &prepared {
        let shape = Shape::from_regions(std::slice::from_ref(region), params.tolerance)?;
        let clamp = Clamp::Region { shape, keep_out: tool.radius() };
        let (boundary, islands, size) = plan::clearing_input(region, params.tolerance);
        if boundary.len() < 3 {
            return invalid("the tool does not fit the pocket");
        }
        for z in cut.levels()? {
            let ops = plan::clearing_level(
                &boundary,
                &islands,
                size,
                tool,
                params.stepover,
                // Roughing keeps the engine's own entries; only the pocket
                // operation exposes a ramp angle today.
                0.0,
                false,
                cut,
                z,
            )?;
            push_checked(&ops, cut, &mut moves, &mut pos, &clamp)?;
        }
    }
    finish(moves, &cut.heights, "the tool does not fit the pocket")
}

/// The boundaries of `regions` moved by `stock` into the material (the
/// crate's own offsetting, preparing for the engine): outers shrink by
/// `stock`, islands grow, and each island stays with the outer it sits in.
/// A region whose outer vanished is dropped.
fn prepare_stock(regions: &[Region2], stock: f64, tolerance: f64) -> Result<Vec<Region2>> {
    if stock <= 0.0 {
        return Ok(regions.to_vec());
    }
    let mut out = Vec::new();
    for region in regions {
        let outers = offset_regions(&[Region2 { outer: region.outer.clone(), holes: Vec::new() }], stock, tolerance)?;
        let mut islands = Vec::new();
        for hole in &region.holes {
            islands.extend(offset_regions(&[Region2 { outer: hole.clone(), holes: Vec::new() }], -stock, tolerance)?);
        }
        for outer in outers {
            let inside: Vec<solidcraft_geom::Loop2> =
                islands.iter().filter(|island| island.polyline(0.05).first().is_some_and(|p| outer.contains(*p))).cloned().collect();
            out.push(Region2 { outer, holes: inside });
        }
    }
    Ok(out)
}
