//! 2.5D operations: contour, pocket, and adaptive clearing. Each one turns
//! planar geometry and cutting data into a [`Toolpath`]: it validates the
//! input, prepares regions, runs the planner (one plan per region and depth
//! level, see [`planner`]), and checks the result with [`verify`] before
//! returning it.

use solidcore_record::{Region2, Vec3};
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::offset::{Shape, offset_regions};
use crate::params::{AdaptiveParams, ContourParams, Cut, PocketParams, check_leave, check_ramp, check_tolerance, mills};
use crate::path::{Move, Toolpath};
use crate::planner;
use crate::planner::translate::Clamp;
use crate::tool::Tool;
use crate::{Result, invalid};

fn finish(moves: Vec<Move>, heights: &crate::params::Heights, empty_why: &str) -> Result<Toolpath> {
    let cuts_any = moves.iter().any(|m| matches!(m, Move::Feed { .. } | Move::Arc { .. }));
    if !cuts_any {
        return invalid(empty_why);
    }
    let path = Toolpath { moves };
    verify(&path, heights)?;
    Ok(path)
}

/// Follows the boundaries of `regions` on the chosen side, one loop after
/// the other, each down through every depth pass before the next. Runs on
/// the planner's exact profile passes: walls come out as exact lines and
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
    let regions = fit_arcs(regions, params.tolerance);
    let mut moves: Vec<Move> = Vec::new();
    let mut pos: Option<Vec3> = None;
    for region in &regions {
        planner::contour_region(region, params, tool, cut, &mut moves, &mut pos)?;
    }
    finish(moves, &cut.heights, "the tool does not fit the profile")
}

/// Clears `regions` (holes are islands): the planner's clearing planner
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
    // The stock to leave comes off the walls before the planner saw the
    // pocket (the crate's own offsetting, as preparer); a region that
    // vanishes was too narrow for the tool and the stock.
    let regions = fit_arcs(regions, params.tolerance);
    let prepared = prepare_stock(&regions, params.stock_to_leave, params.tolerance)?;
    if prepared.is_empty() {
        return invalid("the tool does not fit the pocket with that stock to leave");
    }
    let mut moves: Vec<Move> = Vec::new();
    let mut pos: Option<Vec3> = None;
    for region in &prepared {
        // The oracle the translator clamps against: the prepared region has
        // the stock to leave baked in, so the tool center stays a radius
        // from it.
        let shape = Shape::from_regions(std::slice::from_ref(region), params.tolerance)?;
        planner::clearing_region(
            region,
            tool,
            params.stepover,
            params.ramp_angle,
            true,
            params.climb,
            params.tolerance,
            cut,
            &Clamp::Region { shape, keep_out: r },
            &mut moves,
            &mut pos,
        )?;
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
    // The stock to leave comes off the walls before the planner saw the
    // pocket (the crate's own offsetting, as preparer). A region that
    // vanishes was too narrow for the tool and the stock; an island grows
    // by the same amount.
    let regions = fit_arcs(regions, params.tolerance);
    let prepared = prepare_stock(&regions, params.stock_to_leave, params.tolerance)?;
    if prepared.is_empty() {
        return invalid("the tool does not fit the pocket with that stock to leave");
    }
    let mut moves: Vec<Move> = Vec::new();
    let mut pos: Option<Vec3> = None;
    for region in &prepared {
        let shape = Shape::from_regions(std::slice::from_ref(region), params.tolerance)?;
        planner::clearing_region(
            region,
            tool,
            params.stepover,
            // Roughing keeps the planner's own entries; only the pocket
            // operation exposes a ramp angle today.
            0.0,
            false,
            params.climb,
            params.tolerance,
            cut,
            &Clamp::Region { shape, keep_out: tool.radius() },
            &mut moves,
            &mut pos,
        )?;
    }
    finish(moves, &cut.heights, "the tool does not fit the pocket")
}

/// Sketch profiles arrive as fitted splines; the exact offsetting and the
/// arc output need lines and arcs, so every free-form segment is fitted by
/// them within the operation's tolerance first. A region with a segment
/// that cannot be fitted stays as it is and is followed as a line chain.
fn fit_arcs(regions: &[Region2], tolerance: f64) -> Vec<Region2> {
    regions
        .iter()
        .map(|region| Region2 {
            outer: region.outer.fit_arcs(tolerance).unwrap_or_else(|| region.outer.clone()),
            holes: region.holes.iter().map(|h| h.fit_arcs(tolerance).unwrap_or_else(|| h.clone())).collect(),
        })
        .collect()
}

/// The boundaries of `regions` moved by `stock` into the material (the
/// crate's own offsetting, preparing for the planner): outers shrink by
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
            let inside: Vec<solidcore_record::Loop2> =
                islands.iter().filter(|island| island.polyline(0.05).first().is_some_and(|p| outer.contains(*p))).cloned().collect();
            out.push(Region2 { outer, holes: inside });
        }
    }
    Ok(out)
}
