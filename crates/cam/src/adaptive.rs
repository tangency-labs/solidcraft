//! Adaptive clearing: constant-engagement trochoidal passes.
//!
//! The toolpaths come from the raygeo engine (MIT, crates.io): its clearing
//! planner splits a pocket around narrow passages, orders the areas, plans
//! helical entries and passage crossings, and its adaptive assembler keeps the
//! tool's engagement constant; the resulting motion commands become [`Move`]s
//! in this crate's coordinates and are checked with [`verify`] like every
//! other operation, so a path that breaks a safety rule never leaves this
//! function.
//!
//! Two limits come with the engine's model: a narrow slot between two wide
//! areas is crossed, not cleared (the in-tree [`crate::ops::pocket`] clears
//! those), and the planner works on approximated regions — so every move
//! target is snapped into the exact valid tool-center region (this crate's
//! own offsetting) and an arc that would cross a wall is flattened into
//! cuts. Walls end up safe but may keep up to the region tolerance of
//! material; a finishing contour pass (`cam.op.contour`) cleans them, as
//! adaptive workflows do anyway.

use std::sync::{Arc, Mutex};

use raygeo::cnc::execution::intent::create_intent;
use raygeo::cnc::execution::specs::AggregateOutput;
use raygeo::cnc::plan::clearing::{ClearingWorkplanOptions, plan_clearing};
use raygeo::geo::types::{Point, Polygon};
use raygeo::ops::container::Ops;
use raygeo::ops::part::part::Part;
use raygeo::ops::{MoveCmd, OpCategory};
use raygeo::pipeline::completed::CompletedNode;
use raygeo::pipeline::pipeline::Pipeline;
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Loop2, Region2, Vec2, Vec3};

use crate::check::verify;
use crate::offset::{Shape, offset_regions};
use crate::ops::Cut;
use crate::path::{MAX_MOVES, Move, Toolpath, arc_sweep};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Largest coordinate the engine is asked to cut (mm).
const MAX_COORD: f64 = 1.0e6;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveParams {
    /// Radial engagement per pass (mm), at most the tool diameter.
    pub stepover: f64,
    pub climb: bool,
    /// Material left on the walls (mm).
    pub stock_to_leave: f64,
    /// Chord error of the boundary polygons handed to the engine (mm).
    pub tolerance: f64,
}

/// Clears each region (holes are islands) with constant-engagement passes,
/// one depth level after the other, region by region.
pub fn adaptive(regions: &[Region2], tool: &Tool, params: &AdaptiveParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    if tool.kind == ToolKind::Drill {
        return invalid("a drill cannot mill: it cuts only at its point");
    }
    cut.validate()?;
    if regions.is_empty() {
        return invalid("the operation has no regions");
    }
    if !(params.stepover > 0.0) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    if !params.stock_to_leave.is_finite() || !(0.0..=MAX_COORD).contains(&params.stock_to_leave) {
        return invalid("the stock to leave must be 0 or more");
    }
    if !params.tolerance.is_finite() || !(1e-6..=1.0).contains(&params.tolerance) {
        return invalid("the tolerance must be above 0 and at most 1 mm");
    }
    // The engine keeps the tool a radius off whatever boundary it is given, so
    // the stock to leave comes off the walls first, with this crate's own
    // exact offsetting (the engine's own stock option only feeds its optional
    // finishing profile). A region that vanishes was too narrow for the tool
    // and the stock; an island grows by the same amount.
    let prepared: Vec<Region2> = if params.stock_to_leave > 0.0 {
        let mut out = Vec::new();
        for region in regions {
            let outers = offset_regions(&[Region2 { outer: region.outer.clone(), holes: Vec::new() }], params.stock_to_leave, params.tolerance)?;
            let mut islands: Vec<Loop2> = Vec::new();
            for hole in &region.holes {
                islands.extend(offset_regions(&[Region2 { outer: hole.clone(), holes: Vec::new() }], -params.stock_to_leave, params.tolerance)?);
            }
            for outer in outers {
                let inside: Vec<Loop2> =
                    islands.iter().filter(|island| island.polyline(0.05).first().is_some_and(|p| outer.contains(*p))).cloned().collect();
                out.push(Region2 { outer, holes: inside });
            }
        }
        out
    } else {
        regions.to_vec()
    };
    if prepared.is_empty() {
        return invalid("the tool does not fit the pocket with that stock to leave");
    }
    let levels = cut.levels()?;
    let mut moves: Vec<Move> = Vec::new();
    let mut pos: Option<Vec3> = None;
    for region in &prepared {
        // The engine reads the winding to tell pockets from islands, so both
        // come in counter-clockwise whatever the region's own convention is.
        let boundary = polygon(&region.outer.ccw(), params.tolerance);
        if boundary.len() < 3 {
            return invalid("the tool does not fit the pocket");
        }
        let islands: Vec<Polygon> = region.holes.iter().map(|hole| polygon(&hole.ccw(), params.tolerance)).collect();
        let size = bounds(&boundary);
        // The engine plans against approximated regions; this exact shape is
        // what every move target is snapped into.
        let shape = Shape::from_regions(std::slice::from_ref(region), params.tolerance)?;
        let keep_out = tool.radius() + params.stock_to_leave - 1e-6;
        for z in &levels {
            let ops = clear_level(&boundary, &islands, size, tool, params, cut, *z)?;
            append(&ops, cut, &mut moves, &mut pos, &shape, keep_out)?;
            if moves.len() > MAX_MOVES {
                return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
            }
        }
    }
    let cuts_any = moves.iter().any(|m| matches!(m, Move::Feed { .. } | Move::Arc { .. }));
    if !cuts_any {
        return invalid("the tool does not fit the pocket");
    }
    let path = Toolpath { moves };
    verify(&path, &cut.heights)?;
    Ok(path)
}

/// The polygon's width and height, for the engine's part record.
fn bounds(polygon: &[Point]) -> (f64, f64) {
    let mut min = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut max = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in polygon {
        min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
        max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
    }
    ((max.x - min.x).max(0.0), (max.y - min.y).max(0.0))
}

/// The islands inside a plan step's area, for the step's own scoping.
fn islands_inside(outer: &Polygon, islands: &[Polygon]) -> Vec<Polygon> {
    islands.iter().filter(|island| island.first().is_some_and(|p| contains(outer, *p))).cloned().collect()
}

/// Even-odd containment against the ring's segments.
fn contains(ring: &[Point], p: Point) -> bool {
    let mut inside = false;
    for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}
/// A loop as a polygon for the engine, within `tol`.
fn polygon(loop2: &Loop2, tol: f64) -> Polygon {
    loop2.polyline(tol.max(1e-4)).into_iter().map(|p| Point::new(p.x, p.y)).collect()
}

/// One depth level of one region through the raygeo clearing planner.
///
/// The planner splits the pocket around narrow passages, orders the areas and
/// plans their entries and passage crossings; the pipeline runs the plan and
/// links the steps' cleared state. The planner scopes each area without its
/// islands, so they are attached to the plan's steps again before it runs —
/// without them the area's clearing would cut into the island.
fn clear_level(
    boundary: &Polygon,
    islands: &[Polygon],
    size: (f64, f64),
    tool: &Tool,
    params: &AdaptiveParams,
    cut: &Cut,
    target_z: f64,
) -> Result<Ops> {
    let part = Part::from_polygons(boundary, islands, size);
    // The scales follow the engine's own defaults (a 3 mm-radius tool steps
    // 0.6 mm along a pass and a third of a radius down per turn); step-over
    // and depth come from the operation. The stock to leave came off the
    // walls before the engine saw the pocket.
    let options = ClearingWorkplanOptions {
        tool_radius: tool.radius(),
        step_over: params.stepover,
        step_length: tool.radius() * 0.2,
        target_z,
        safe_z: cut.heights.clearance,
        wall_margin: 0.0,
        safe_margin: tool.radius() * 0.2,
        stock_to_leave: 0.0,
        plunge_pitch: tool.radius() * 0.3,
        angular_step: 0.1,
        area_tolerance: 1.0,
        max_deflection_deg: 30.0,
        finishing: false,
    };
    let mut plan = plan_clearing(&part, "", &options).map_err(|e| CamError::Invalid(format!("the clearing planner failed: {e}")))?;
    for step in &mut plan.steps {
        if let Some((polygon, scoped)) = &mut step.region_boundary
            && scoped.is_empty()
        {
            *scoped = islands_inside(polygon, islands);
        }
    }
    let nodes = create_intent(&plan, &part, 0);
    if nodes.is_empty() {
        return Ok(Ops::new());
    }
    let result: Arc<Mutex<Option<std::result::Result<Ops, String>>>> = Arc::new(Mutex::new(None));
    let sink = result.clone();
    let captured = move |node: CompletedNode| {
        let mut slot = match sink.lock() {
            Ok(slot) => slot,
            Err(_) => return,
        };
        if slot.is_some() {
            return;
        }
        if let Some(error) = &node.error {
            *slot = Some(Err(format!("the clearing engine failed: {error:?}")));
            return;
        }
        if let Some(output) = &node.output
            && let Some(aggregate) = output.downcast_ref::<AggregateOutput>()
        {
            *slot = Some(Ok(aggregate.ops.clone()));
        }
    };
    Pipeline::default().execute(nodes, captured, None).map_err(|_| CamError::Invalid("the clearing engine was cancelled".into()))?;
    match result.lock().map_err(|_| CamError::Invalid("the clearing engine panicked".into()))?.take() {
        Some(Ok(ops)) => Ok(ops),
        Some(Err(message)) => Err(CamError::Invalid(message)),
        None => Ok(Ops::new()),
    }
}

/// The farthest point toward `to` (from a valid `from`) that stays `keep_out`
/// inside the region: the engine plans against approximated regions, so a
/// target may sit past the true wall; the tool never does.
fn clamp_to(shape: &Shape, keep_out: f64, from: Vec2, to: Vec2) -> Vec2 {
    if shape.clear(to, keep_out) || !shape.clear(from, keep_out) {
        return to;
    }
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if shape.clear(from.lerp(to, mid), keep_out) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    from.lerp(to, lo)
}

/// Appends the engine's motion commands as moves.
///
/// The engine links its passes with high-speed skims that rise just above the
/// cut and knows from its own cleared-area state that they cross cleared
/// material. This crate judges a toolpath from the path alone, so a travel
/// below the retract height is rewritten the way the pocket operation builds
/// its own: up to the retract plane, across, and down at the plunge feed.
/// Feed moves straight down also take the plunge feed.
fn append(ops: &Ops, cut: &Cut, moves: &mut Vec<Move>, pos: &mut Option<Vec3>, shape: &Shape, keep_out: f64) -> Result<()> {
    let h = &cut.heights;
    let feeds = &cut.feeds;
    for node in ops.commands.iter() {
        let OpCategory::Moving { end, cmd } = &node.category else { continue };
        let mut to = Vec3::new(end.x, end.y, end.z);
        if !to.is_finite() {
            return Err(CamError::Unsafe("the clearing engine produced a move that is not a finite number".into()));
        }
        let from = match *pos {
            Some(from) if from.dist(to) <= 1e-9 => continue,
            Some(from) => from,
            None if moves.is_empty() => {
                // The engine's first command positions at its own working
                // height; the path still begins at the clearance plane.
                let at = Vec3::new(to.x, to.y, h.clearance.max(to.z));
                moves.push(Move::Rapid { to: at });
                *pos = Some(at);
                continue;
            }
            None => continue,
        };
        let from_xy = Vec2::new(from.x, from.y);
        let to_xy = Vec2::new(to.x, to.y);
        match cmd {
            MoveCmd::MoveTo => {
                let clamped = clamp_to(shape, keep_out, from_xy, to_xy);
                to = clamped.extend(to.z);
                let same_xy = (from.x - to.x).abs() <= 1e-9 && (from.y - to.y).abs() <= 1e-9;
                if to.z >= h.retract - 1e-9 {
                    // Rising out of a cut: straight up before any sideways
                    // motion, the same rule the verifier holds the pocket to.
                    if !same_xy && from.z < h.retract - 1e-9 {
                        moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, h.retract) });
                    }
                    moves.push(Move::Rapid { to });
                } else if same_xy {
                    if from.z > h.retract {
                        moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, h.retract) });
                    }
                    moves.push(Move::Feed { to, feed: feeds.plunge });
                } else {
                    moves.push(Move::Rapid { to: Vec3::new(from.x, from.y, h.retract) });
                    moves.push(Move::Rapid { to: Vec3::new(to.x, to.y, h.retract) });
                    moves.push(Move::Feed { to, feed: feeds.plunge });
                }
            }
            MoveCmd::LineTo => {
                let clamped = clamp_to(shape, keep_out, from_xy, to_xy);
                to = clamped.extend(to.z);
                let plunges = from.x == to.x && from.y == to.y && to.z < from.z - 1e-9;
                moves.push(Move::Feed { to, feed: if plunges { feeds.plunge } else { feeds.cut } });
            }
            MoveCmd::ArcTo(arc) => {
                let center = Vec2::new(arc.center.x, arc.center.y);
                let start = from_xy - center;
                let sweep = arc_sweep(from, to, center, !arc.cw);
                // An arc that would cross a wall is flattened into clamped cuts.
                let on_arc = |t: f64| center + Vec2::from_angle(start.angle() + sweep * t) * start.len();
                let crossed = [0.25, 0.5, 0.75].iter().any(|t| !shape.clear(on_arc(*t), keep_out)) || !shape.clear(to_xy, keep_out);
                if crossed {
                    let steps = ((sweep.abs() / 0.2).ceil() as usize).clamp(2, 64);
                    for k in 1..=steps {
                        let t = k as f64 / steps as f64;
                        let q = clamp_to(shape, keep_out, from_xy, on_arc(t));
                        let z = from.z + (to.z - from.z) * t;
                        moves.push(Move::Feed { to: q.extend(z), feed: feeds.cut });
                        *pos = Some(q.extend(z));
                    }
                    continue;
                }
                let end = to_xy - center;
                if start.dist(end) <= 1e-9 || start.len() <= 1e-9 {
                    // A full turn as two halves, like the pocket operation splits its arcs.
                    for half in [0.0, std::f64::consts::PI] {
                        let at = center + Vec2::from_angle(start.angle() + half) * start.len();
                        let z = from.z + (to.z - from.z) * half / std::f64::consts::TAU;
                        moves.push(Move::Arc { to: at.extend(z), center, ccw: !arc.cw, feed: feeds.cut });
                    }
                    continue;
                }
                let sweep = arc_sweep(from, to, center, !arc.cw);
                if sweep.abs() > std::f64::consts::PI {
                    let mid = center + Vec2::from_angle(start.angle() + sweep * 0.5) * start.len();
                    let z_mid = from.z + (to.z - from.z) * 0.5;
                    moves.push(Move::Arc { to: mid.extend(z_mid), center, ccw: !arc.cw, feed: feeds.cut });
                }
                moves.push(Move::Arc { to, center, ccw: !arc.cw, feed: feeds.cut });
            }
            // The clearing engine cuts lines and arcs; anything else would
            // have to be linearized first and is refused rather than guessed.
            MoveCmd::BezierTo(_) | MoveCmd::QuadraticBezierTo { .. } | MoveCmd::ScanLine { .. } => {
                return Err(CamError::Unsupported("the clearing engine produced a curve this operation does not handle".into()));
            }
        }
        *pos = Some(to);
    }
    Ok(())
}
