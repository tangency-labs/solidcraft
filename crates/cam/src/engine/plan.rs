//! Per-depth-level plan builders and runners: the only place that talks to
//! the engine's planning stack. Each call plans one region at one depth and
//! returns the engine's raw motion commands for the translator.

use raygeo::cnc::execution::run::run_plan;
use raygeo::cnc::plan::clearing::{ClearingWorkplanOptions, plan_clearing};
use raygeo::cnc::plan::plan::{Plan, PlanStep};
use raygeo::error::RaygeoError;
use raygeo::geo::geometry::Geometry;
use raygeo::ops::assembly::profile::{ProfileKind, ProfileRampSpec, ProfileSpec};
use raygeo::ops::container::Ops;
use raygeo::ops::part::part::Part;
use raygeo::ops::types::CutDirection;
use solidcraft_geom::Region2;

use crate::Result;
use crate::engine::geometry::region_polygons;
use crate::tool::Tool;
use crate::{CamError, invalid};

fn engine_failed(what: &str, e: impl std::fmt::Display) -> CamError {
    CamError::Invalid(format!("the {what} failed: {e}"))
}

/// One depth level of a wall profile: the tool-center loop derived from
/// `geometry` — the exact line-and-arc offset for curve-true input — walked
/// once at `z`, preceded by a ramp along the loop when `ramp_angle` is
/// above zero. `wall_margin` and `stock` fold into the offset; a profile on
/// the boundary passes `wall_margin: -tool_radius` and `stock: 0`.
pub(crate) fn profile_level(
    geometry: &Geometry,
    kind: ProfileKind,
    tool: &Tool,
    wall_margin: f64,
    stock_to_leave: f64,
    ramp_angle: f64,
    tolerance: f64,
    direction: CutDirection,
    cut: &crate::params::Cut,
    z: f64,
) -> Result<Ops> {
    let part = Part::new(Some(geometry.clone()), (0.0, 0.0));
    let (boundary, islands) = part.extract_boundary();
    let Some(boundary) = boundary else {
        return invalid("the tool does not fit the profile");
    };
    let mut plan = Plan::new(boundary, islands, cut.heights.clearance);
    if ramp_angle > 0.0 {
        plan.steps.push(PlanStep {
            face_id: String::new(),
            spec: std::sync::Arc::new(ProfileRampSpec {
                kind,
                tool_radius: tool.radius(),
                wall_margin,
                stock_to_leave,
                ramp_angle,
                z_start: cut.heights.clearance,
                z_end: z,
                cut_direction: direction,
                tolerance,
            }),
            region_boundary: None,
        });
    }
    plan.steps.push(PlanStep {
        face_id: String::new(),
        spec: std::sync::Arc::new(ProfileSpec {
            kind,
            tool_radius: tool.radius(),
            step_over: tool.radius(),
            step_length: tool.radius() * 0.2,
            target_z: z,
            safe_z: cut.heights.clearance,
            wall_margin,
            stock_to_leave,
            cut_direction: direction,
            start_pos: None,
            tolerance,
            expansion_batch_size: 20,
            cancel_check: None,
            engagement_area_threshold: 1.0,
            engagement_angle_threshold: 30.0,
            feed_reduction_factor: 0.5,
            trace_path: None,
        }),
        region_boundary: None,
    });
    let output = run_plan(&plan, &part).map_err(|e| match e {
        RaygeoError::Cancelled => engine_failed("profiling engine", "it was cancelled"),
        other => engine_failed("profiling engine", other),
    })?;
    Ok(output.ops)
}

/// One depth level of clearing — pocket roughing with a finishing wall pass
/// when `finishing`, adaptive clearing when not: regions split around
/// narrow passages, ordered, entered and linked by the engine's clearing
/// planner. `ramp_angle` (radians) caps how steeply the entries and the
/// finishing pass descend into material.
///
/// Upstream gap: `plan_clearing` hardcodes its cut direction (CCW), so
/// climb/conventional cannot be forwarded yet; every clearing path the
/// engine generates is climb-convention.
pub(crate) fn clearing_level(
    boundary: &[raygeo::geo::types::Point],
    islands: &[Vec<raygeo::geo::types::Point>],
    size: (f64, f64),
    tool: &Tool,
    step_over: f64,
    ramp_angle: f64,
    finishing: bool,
    cut: &crate::params::Cut,
    z: f64,
) -> Result<Ops> {
    let part = Part::from_polygons(&raygeo::geo::types::Polygon::from(boundary.to_vec()), islands, size);
    // The scales follow the engine's own defaults (a 3 mm-radius tool steps
    // 0.6 mm along a pass and a third of a radius down per turn); step-over
    // and depth come from the operation.
    let options = ClearingWorkplanOptions {
        tool_radius: tool.radius(),
        step_over,
        step_length: tool.radius() * 0.2,
        target_z: z,
        safe_z: cut.heights.clearance,
        wall_margin: 0.0,
        safe_margin: tool.radius() * 0.2,
        stock_to_leave: 0.0,
        plunge_pitch: tool.radius() * 0.3,
        angular_step: 0.1,
        area_tolerance: 1.0,
        max_deflection_deg: 30.0,
        max_entry_angle_deg: ramp_angle.to_degrees(),
        finishing,
        // Feeds are chosen per move by the translator, from the operation's
        // own feeds.
        feed_rate: None,
        plunge_feed: None,
    };
    let plan = plan_clearing(&part, "", &options).map_err(|e| engine_failed("clearing planner", e))?;
    if plan.steps.is_empty() {
        return Ok(Ops::new());
    }
    let output = run_plan(&plan, &part).map_err(|e| match e {
        RaygeoError::Cancelled => engine_failed("clearing engine", "it was cancelled"),
        other => engine_failed("clearing engine", other),
    })?;
    Ok(output.ops)
}

/// The flattened boundary and islands of one region, wound for the clearing
/// planners, with the part size they span.
pub(crate) fn clearing_input(region: &Region2, tolerance: f64) -> (Vec<raygeo::geo::types::Point>, Vec<Vec<raygeo::geo::types::Point>>, (f64, f64)) {
    let (boundary, islands) = region_polygons(region, tolerance);
    let size = polygon_size(&boundary);
    (boundary, islands, size)
}

/// The polygon's width and height, for the engine's part record.
fn polygon_size(polygon: &[raygeo::geo::types::Point]) -> (f64, f64) {
    let (mut min, mut max) =
        (raygeo::geo::types::Point::new(f64::INFINITY, f64::INFINITY), raygeo::geo::types::Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in polygon {
        min = raygeo::geo::types::Point::new(min.x.min(p.x), min.y.min(p.y));
        max = raygeo::geo::types::Point::new(max.x.max(p.x), max.y.max(p.y));
    }
    ((max.x - min.x).max(0.0), (max.y - min.y).max(0.0))
}
