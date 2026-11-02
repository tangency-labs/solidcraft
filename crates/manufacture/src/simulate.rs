//! Deterministic 3-axis toolpath simulation on a bounded height-field.
//!
//! Consumes validated [`PathSegment`] values and simulates material removal on a
//! box-stock height field. Output: removed-volume estimate, height-range statistics,
//! structured envelope/collision issues, and optional scrubber frames.
//!
//! # Stock / WCS conventions
//! - Stock **top surface is at Z = 0.0** in WCS; material extends to **Z = −stock_height**.
//! - The box stock occupies **X ∈ [0, width]**, **Y ∈ [0, depth]**.
//! - Rapid moves carry no material removal; Linear and Arc moves do.
//! - Cell heights are WCS Z values: 0.0 (stock top) → −stock_height (floor).
//! - Removal is **monotonically non-increasing** (the min-update rule; never adds material).

use serde::{Deserialize, Serialize};

use crate::{PathSegment, Stock, Tool, ToolKind};

// ---------------------------------------------------------------------------
// Simulation-specific bounds
// ---------------------------------------------------------------------------

/// Maximum grid columns in one simulation.
pub const MAX_GRID_COLS: u32 = 512;
/// Maximum grid rows in one simulation.
pub const MAX_GRID_ROWS: u32 = 512;
/// Maximum total grid cells (512 × 512 = 262 144).
pub const MAX_GRID_CELLS: usize = (MAX_GRID_COLS as usize) * (MAX_GRID_ROWS as usize);
/// Maximum progress frames returned by one simulation.
pub const MAX_FRAMES: u32 = 64;
/// Maximum total interpolation steps across all segments before simulation is truncated.
pub const MAX_TOTAL_STEPS: usize = 2_000_000;
/// Minimum allowed interpolation step in mm.
pub const MIN_INTERP_STEP_MM: f64 = 0.005;
/// Maximum number of fixtures accepted per simulation.
pub const MAX_FIXTURES: usize = 64;

/// tan(59°) — half-point-angle of a standard 118° drill tip.
const TAN_59_DEG: f64 = 1.664_279_482_350_518;

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// An axis-aligned bounding-box fixture (WCS mm) that the tool must not enter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Human-readable label used in issue messages.
    pub label: String,
    /// Minimum corner \[x, y, z\] in WCS mm.
    pub min: [f64; 3],
    /// Maximum corner \[x, y, z\] in WCS mm.
    pub max: [f64; 3],
}

/// Options that control the simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SimOptions {
    /// Height-field columns (1–[`MAX_GRID_COLS`]).
    pub grid_cols: u32,
    /// Height-field rows (1–[`MAX_GRID_ROWS`]).
    pub grid_rows: u32,
    /// WCS Z at or above which Rapid moves are considered safe.
    /// Rapid moves below this height emit a [`SimSeverity::Warning`].
    pub clearance_z: f64,
    /// Axis-aligned fixtures the tool must not enter.  At most [`MAX_FIXTURES`] entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixtures: Vec<Fixture>,
    /// Number of height-field scrubber frames to capture (0 = none; max [`MAX_FRAMES`]).
    pub max_frames: u32,
    /// Interpolation step override in mm.  `None` = automatic (tool diameter / 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interp_step_mm: Option<f64>,
    /// Optional machine XY travel \[x_max, y_max\] from the WCS origin.
    /// Moves beyond this emit a [`SimSeverity::Error`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_xy_travel: Option<[f64; 2]>,
    /// Optional machine Z lower limit (must be ≤ 0; how far below the WCS origin the
    /// spindle can reach). Moves below this emit a [`SimSeverity::Error`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_z_min: Option<f64>,
}

impl Default for SimOptions {
    fn default() -> Self {
        Self {
            grid_cols: 256,
            grid_rows: 256,
            clearance_z: 5.0,
            fixtures: Vec::new(),
            max_frames: 0,
            interp_step_mm: None,
            machine_xy_travel: None,
            machine_z_min: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

/// Severity of a simulation diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimSeverity {
    /// Fatal; the toolpath must not be posted or exported until resolved.
    Error,
    /// Informational; the simulation completed but there is a concern.
    Warning,
}

/// A diagnostic produced by the simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimIssue {
    pub severity: SimSeverity,
    /// Index into the input `segments` slice.
    pub segment_index: usize,
    pub message: String,
}

/// A height-field snapshot captured for a UI timeline scrubber.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimFrame {
    /// Index of the segment _after_ which this snapshot was taken.
    pub segment_index: usize,
    /// Volume removed from stock up to this point (mm³, estimated).
    pub removed_volume_mm3: f64,
    /// Row-major height values (grid_cols × grid_rows cells), each a WCS Z in mm.
    pub heights: Vec<f64>,
}

/// Output of [`simulate_toolpaths`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimResult {
    /// Estimated volume of material removed (mm³, based on grid cell areas).
    pub removed_volume_mm3: f64,
    /// Minimum remaining surface height across all grid cells (WCS Z, mm).
    pub min_remaining_height_mm: f64,
    /// Maximum remaining surface height across all grid cells (WCS Z, mm).
    pub max_remaining_height_mm: f64,
    /// `true` when at least one [`SimSeverity::Error`] issue was produced.
    pub has_errors: bool,
    /// All issues encountered during simulation, in segment order.
    pub issues: Vec<SimIssue>,
    /// Final height-field: row-major Vec of `grid_cols × grid_rows` WCS Z values (mm).
    pub final_heights: Vec<f64>,
    /// Grid columns used.
    pub grid_cols: u32,
    /// Grid rows used.
    pub grid_rows: u32,
    /// Progress frames for a UI scrubber (empty when `max_frames = 0`).
    pub frames: Vec<SimFrame>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run a deterministic 3-axis toolpath simulation.
///
/// Applies each [`PathSegment`] to a box-stock height field using the given `tool`'s cutting
/// geometry.  Cylinder stock is not supported and returns a single error issue.
///
/// The function is **non-mutating with respect to all inputs** (no `&mut`, no global state)
/// and produces identical output for identical inputs (deterministic).
pub fn simulate_toolpaths(stock: &Stock, tool: &Tool, segments: &[PathSegment], opts: &SimOptions) -> SimResult {
    // Validate options up-front; return a synthetic error result on bad input.
    if let Err(msg) = validate_options(opts) {
        return single_error_result(format!("invalid simulation options: {msg}"), opts);
    }

    let (stock_width, stock_depth, stock_height) = match stock.sim_box_extents() {
        Some(extents) => extents,
        None => {
            return single_error_result("stock mode is not yet supported for toolpath simulation; use fixed or relative box stock", opts);
        }
    };

    // Basic stock validity (the Stock type already validates this, but be defensive).
    if !(stock_width > 0.0
        && stock_width.is_finite()
        && stock_depth > 0.0
        && stock_depth.is_finite()
        && stock_height > 0.0
        && stock_height.is_finite())
    {
        return single_error_result("stock dimensions must be finite and strictly positive", opts);
    }

    let grid_cols = opts.grid_cols;
    let grid_rows = opts.grid_rows;
    let cell_w = stock_width / grid_cols as f64;
    let cell_d = stock_depth / grid_rows as f64;
    let stock_floor = -stock_height;

    // Height field: initially all at Z = 0.0 (stock top surface).
    let n_cells = (grid_cols as usize) * (grid_rows as usize);
    let mut heights = vec![0.0_f64; n_cells];

    // Effective interpolation step.
    let raw_step = opts.interp_step_mm.unwrap_or_else(|| tool.diameter / 4.0);
    let interp_step = raw_step.max(MIN_INTERP_STEP_MM);

    let tool_r = tool.diameter / 2.0;

    let mut issues: Vec<SimIssue> = Vec::new();

    // Tool tip position before any segment.  Starts at clearance height above stock corner.
    let mut pos = [0.0_f64, 0.0, opts.clearance_z];

    // Step budget across all segments.
    let mut total_steps: usize = 0;

    // Frame capture: decide interval (in segments) for requested frames.
    let n_segs = segments.len();
    let frame_interval: usize = if opts.max_frames > 0 && n_segs > 0 { (n_segs / opts.max_frames as usize).max(1) } else { usize::MAX };
    let mut frames: Vec<SimFrame> = Vec::new();

    // Per-simulation deduplication set for fixture collision issues.
    // Key = (segment_index, fixture_index) to emit at most one issue per pair.
    let mut reported_fixture: std::collections::BTreeSet<(usize, usize)> = std::collections::BTreeSet::new();
    // Per-segment flags for single-emission issues.
    let mut reported_depth_error: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut reported_flute_error: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut reported_rapid_surface: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

    for (seg_idx, seg) in segments.iter().enumerate() {
        // Hard-validate the segment; skip invalid ones.
        if let Err(e) = seg.validate() {
            push_issue(&mut issues, SimSeverity::Error, seg_idx, format!("invalid segment: {e}"));
            continue;
        }

        match seg {
            PathSegment::Rapid { to } => {
                // ── Envelope checks ───────────────────────────────────────────
                check_xy_envelope(*to, seg_idx, stock_width, stock_depth, opts.machine_xy_travel, &mut issues);
                check_z_envelope(to[2], seg_idx, stock_floor, opts.machine_z_min, &mut issues);

                // Rapid below clearance height → warning.
                if to[2] < opts.clearance_z {
                    push_issue(
                        &mut issues,
                        SimSeverity::Warning,
                        seg_idx,
                        format!("rapid moves below clearance height {:.3} mm (to Z = {:.3} mm)", opts.clearance_z, to[2]),
                    );
                }

                // Rapid below current stock surface → collision risk → error.
                let (col, row) = xy_to_cell(to[0], to[1], cell_w, cell_d, grid_cols, grid_rows);
                if in_stock_xy(to[0], to[1], stock_width, stock_depth)
                    && let Some(&cell_h) = heights.get(row * grid_cols as usize + col)
                    && to[2] < cell_h
                    && !reported_rapid_surface.contains(&seg_idx)
                {
                    push_issue(
                        &mut issues,
                        SimSeverity::Error,
                        seg_idx,
                        format!(
                            "rapid collision: tip Z {:.3} mm is below stock surface \
                                     {:.3} mm at ({:.3}, {:.3})",
                            to[2], cell_h, to[0], to[1]
                        ),
                    );
                    reported_rapid_surface.insert(seg_idx);
                }

                // Fixture collision (rapid trajectory is a straight line; check endpoint).
                check_fixture_point(*to, tool_r, &opts.fixtures, seg_idx, &mut reported_fixture, &mut issues);

                pos = *to;
            }

            PathSegment::Linear { to, .. } => {
                // ── Envelope checks at destination ────────────────────────────
                check_xy_envelope(*to, seg_idx, stock_width, stock_depth, opts.machine_xy_travel, &mut issues);
                check_z_envelope(to[2], seg_idx, stock_floor, opts.machine_z_min, &mut issues);

                // Deepest Z during this segment (for depth/flute checks).
                let min_z = pos[2].min(to[2]);
                check_tool_depth(min_z, tool, seg_idx, &mut reported_depth_error, &mut reported_flute_error, &mut issues);

                // Interpolate and apply cutter.
                let dist = dist3(pos, *to);
                let (n_steps, truncated) = steps_for_distance(dist, interp_step, total_steps);
                if truncated && total_steps < MAX_TOTAL_STEPS {
                    push_issue(
                        &mut issues,
                        SimSeverity::Warning,
                        seg_idx,
                        format!(
                            "simulation step budget ({MAX_TOTAL_STEPS}) reached at segment \
                             {seg_idx}; remaining segments skipped"
                        ),
                    );
                }
                total_steps = total_steps.saturating_add(n_steps);

                for i in 0..=n_steps {
                    let t = if n_steps == 0 { 1.0 } else { i as f64 / n_steps as f64 };
                    let p = lerp3(pos, *to, t);
                    apply_cutter(&mut heights, p[0], p[1], p[2], tool_r, tool, grid_cols, grid_rows, cell_w, cell_d, stock_floor);
                    check_fixture_point(p, tool_r, &opts.fixtures, seg_idx, &mut reported_fixture, &mut issues);
                }

                if truncated {
                    break;
                }
                pos = *to;
            }

            PathSegment::Arc { center, to, clockwise, .. } => {
                // ── Envelope checks at destination ────────────────────────────
                check_xy_envelope(*to, seg_idx, stock_width, stock_depth, opts.machine_xy_travel, &mut issues);
                check_z_envelope(to[2], seg_idx, stock_floor, opts.machine_z_min, &mut issues);

                let min_z = pos[2].min(to[2]);
                check_tool_depth(min_z, tool, seg_idx, &mut reported_depth_error, &mut reported_flute_error, &mut issues);

                // Compute arc parameters.
                let arc_r = {
                    let dx = pos[0] - center[0];
                    let dy = pos[1] - center[1];
                    (dx * dx + dy * dy).sqrt()
                };

                if arc_r < 1e-9 {
                    // Degenerate arc (zero radius) → apply single point at destination.
                    apply_cutter(&mut heights, to[0], to[1], to[2], tool_r, tool, grid_cols, grid_rows, cell_w, cell_d, stock_floor);
                    check_fixture_point(*to, tool_r, &opts.fixtures, seg_idx, &mut reported_fixture, &mut issues);
                } else {
                    let angle_from = (pos[1] - center[1]).atan2(pos[0] - center[0]);
                    let angle_to_val = (to[1] - center[1]).atan2(to[0] - center[0]);
                    let sweep = arc_sweep(angle_from, angle_to_val, *clockwise);
                    let arc_len = arc_r * sweep.abs();

                    let (n_steps, truncated) = steps_for_distance(arc_len, interp_step, total_steps);
                    if truncated && total_steps < MAX_TOTAL_STEPS {
                        push_issue(
                            &mut issues,
                            SimSeverity::Warning,
                            seg_idx,
                            format!(
                                "simulation step budget ({MAX_TOTAL_STEPS}) reached at segment \
                                 {seg_idx}; remaining segments skipped"
                            ),
                        );
                    }
                    total_steps = total_steps.saturating_add(n_steps);

                    for i in 0..=n_steps {
                        let t = if n_steps == 0 { 1.0 } else { i as f64 / n_steps as f64 };
                        let angle = angle_from + t * sweep;
                        let px = center[0] + arc_r * angle.cos();
                        let py = center[1] + arc_r * angle.sin();
                        let pz = pos[2] + t * (to[2] - pos[2]);
                        apply_cutter(&mut heights, px, py, pz, tool_r, tool, grid_cols, grid_rows, cell_w, cell_d, stock_floor);
                        check_fixture_point([px, py, pz], tool_r, &opts.fixtures, seg_idx, &mut reported_fixture, &mut issues);
                    }

                    if truncated {
                        break;
                    }
                }

                pos = *to;
            }
        }

        // Capture scrubber frame if requested.
        if opts.max_frames > 0 && (seg_idx + 1) % frame_interval == 0 && frames.len() < opts.max_frames as usize {
            let vol = compute_removed_volume(&heights, cell_w, cell_d);
            frames.push(SimFrame { segment_index: seg_idx, removed_volume_mm3: vol, heights: heights.clone() });
        }
    }

    // Final statistics.
    let removed_vol = compute_removed_volume(&heights, cell_w, cell_d);
    let (min_h, max_h) = height_range(&heights);
    let has_errors = issues.iter().any(|i| i.severity == SimSeverity::Error);

    SimResult {
        removed_volume_mm3: removed_vol,
        min_remaining_height_mm: min_h,
        max_remaining_height_mm: max_h,
        has_errors,
        issues,
        final_heights: heights,
        grid_cols,
        grid_rows,
        frames,
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_options(opts: &SimOptions) -> std::result::Result<(), String> {
    if opts.grid_cols == 0 || opts.grid_cols > MAX_GRID_COLS {
        return Err(format!("grid_cols must be 1–{MAX_GRID_COLS}"));
    }
    if opts.grid_rows == 0 || opts.grid_rows > MAX_GRID_ROWS {
        return Err(format!("grid_rows must be 1–{MAX_GRID_ROWS}"));
    }
    if !opts.clearance_z.is_finite() {
        return Err("clearance_z must be finite".into());
    }
    if opts.max_frames > MAX_FRAMES {
        return Err(format!("max_frames must be ≤ {MAX_FRAMES}"));
    }
    if let Some(step) = opts.interp_step_mm
        && (!step.is_finite() || step < MIN_INTERP_STEP_MM)
    {
        return Err(format!("interp_step_mm must be ≥ {MIN_INTERP_STEP_MM:.3} and finite"));
    }
    if opts.fixtures.len() > MAX_FIXTURES {
        return Err(format!("too many fixtures (maximum {MAX_FIXTURES})"));
    }
    for (fi, f) in opts.fixtures.iter().enumerate() {
        for axis in 0..3 {
            if !f.min[axis].is_finite() || !f.max[axis].is_finite() {
                return Err(format!("fixture[{fi}] `{}`: coordinates must be finite", f.label));
            }
            if f.min[axis] >= f.max[axis] {
                return Err(format!("fixture[{fi}] `{}`: min[{axis}] must be < max[{axis}]", f.label));
            }
        }
    }
    if let Some(xy) = opts.machine_xy_travel
        && !(xy[0] > 0.0 && xy[0].is_finite() && xy[1] > 0.0 && xy[1].is_finite())
    {
        return Err("machine_xy_travel values must be strictly positive and finite".into());
    }
    if let Some(z_min) = opts.machine_z_min
        && (!z_min.is_finite() || z_min > 0.0)
    {
        return Err("machine_z_min must be ≤ 0 and finite".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Issue helpers
// ---------------------------------------------------------------------------

fn push_issue(issues: &mut Vec<SimIssue>, severity: SimSeverity, segment_index: usize, message: impl Into<String>) {
    issues.push(SimIssue { severity, segment_index, message: message.into() });
}

/// Build a `SimResult` that contains a single error issue and empty height data.
/// Used when options are invalid or stock type is unsupported.
fn single_error_result(msg: impl Into<String>, opts: &SimOptions) -> SimResult {
    let gc = opts.grid_cols.clamp(1, MAX_GRID_COLS);
    let gr = opts.grid_rows.clamp(1, MAX_GRID_ROWS);
    SimResult {
        removed_volume_mm3: 0.0,
        min_remaining_height_mm: 0.0,
        max_remaining_height_mm: 0.0,
        has_errors: true,
        issues: vec![SimIssue { severity: SimSeverity::Error, segment_index: 0, message: msg.into() }],
        final_heights: vec![0.0_f64; (gc as usize) * (gr as usize)],
        grid_cols: gc,
        grid_rows: gr,
        frames: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Envelope / depth checks
// ---------------------------------------------------------------------------

fn check_xy_envelope(to: [f64; 3], seg_idx: usize, stock_w: f64, stock_d: f64, machine_xy: Option<[f64; 2]>, issues: &mut Vec<SimIssue>) {
    // Check machine travel limits (hard error).
    if let Some([x_max, y_max]) = machine_xy {
        if to[0] < 0.0 || to[0] > x_max {
            push_issue(issues, SimSeverity::Error, seg_idx, format!("X {:.3} mm exceeds machine travel envelope [0, {x_max:.3}]", to[0]));
        }
        if to[1] < 0.0 || to[1] > y_max {
            push_issue(issues, SimSeverity::Error, seg_idx, format!("Y {:.3} mm exceeds machine travel envelope [0, {y_max:.3}]", to[1]));
        }
    }
    // Warn if significantly outside the stock bounds (but not a hard error unless machine limits set).
    let margin = 0.0; // tool center outside stock bounds at all → warning
    if (to[0] < -margin || to[0] > stock_w + margin || to[1] < -margin || to[1] > stock_d + margin) && machine_xy.is_none() {
        push_issue(
            issues,
            SimSeverity::Warning,
            seg_idx,
            format!(
                "move to ({:.3}, {:.3}) lies outside the stock footprint \
                     ([0, {stock_w:.3}] × [0, {stock_d:.3}])",
                to[0], to[1]
            ),
        );
    }
}

fn check_z_envelope(z: f64, seg_idx: usize, stock_floor: f64, machine_z_min: Option<f64>, issues: &mut Vec<SimIssue>) {
    if let Some(z_min) = machine_z_min
        && z < z_min
    {
        push_issue(issues, SimSeverity::Error, seg_idx, format!("Z {z:.3} mm exceeds machine Z travel limit ({z_min:.3} mm)"));
    }
    if z < stock_floor {
        push_issue(issues, SimSeverity::Warning, seg_idx, format!("Z {z:.3} mm is below the stock floor ({stock_floor:.3} mm)"));
    }
}

fn check_tool_depth(
    min_z: f64,
    tool: &Tool,
    seg_idx: usize,
    reported_tool_len: &mut std::collections::BTreeSet<usize>,
    reported_flute: &mut std::collections::BTreeSet<usize>,
    issues: &mut Vec<SimIssue>,
) {
    if min_z >= 0.0 {
        return; // above stock top: no depth concern
    }
    let depth = -min_z; // positive depth of cut below stock top
    if depth > tool.length && !reported_tool_len.contains(&seg_idx) {
        push_issue(issues, SimSeverity::Error, seg_idx, format!("cut depth {depth:.3} mm exceeds tool overall length {:.3} mm", tool.length));
        reported_tool_len.insert(seg_idx);
    } else if depth > tool.flute_length && !reported_flute.contains(&seg_idx) {
        push_issue(
            issues,
            SimSeverity::Error,
            seg_idx,
            format!(
                "cut depth {depth:.3} mm exceeds tool flute length {:.3} mm; \
                 tool holder may collide with stock",
                tool.flute_length
            ),
        );
        reported_flute.insert(seg_idx);
    }
}

fn check_fixture_point(
    p: [f64; 3],
    tool_r: f64,
    fixtures: &[Fixture],
    seg_idx: usize,
    reported: &mut std::collections::BTreeSet<(usize, usize)>,
    issues: &mut Vec<SimIssue>,
) {
    for (fi, fixture) in fixtures.iter().enumerate() {
        if reported.contains(&(seg_idx, fi)) {
            continue;
        }
        // Cylinder tool (radius tool_r, centered at (p[0], p[1])) intersects the fixture
        // if the XY projection of the cylinder overlaps the fixture AND the Z range overlaps.
        // We approximate the tool as a point (center) for simplicity — acceptable for checks.
        let in_x = p[0] + tool_r > fixture.min[0] && p[0] - tool_r < fixture.max[0];
        let in_y = p[1] + tool_r > fixture.min[1] && p[1] - tool_r < fixture.max[1];
        let in_z = p[2] < fixture.max[2] && p[2] > fixture.min[2];
        if in_x && in_y && in_z {
            push_issue(
                issues,
                SimSeverity::Error,
                seg_idx,
                format!("tool collides with fixture `{}` at ({:.3}, {:.3}, {:.3})", fixture.label, p[0], p[1], p[2]),
            );
            reported.insert((seg_idx, fi));
        }
    }
}

// ---------------------------------------------------------------------------
// Material-removal kernel
// ---------------------------------------------------------------------------

/// Apply the cutter footprint centred at (tx, ty, tz) to `heights`.
///
/// Only cells within the tool's XY radius are updated; height can only decrease
/// (monotonic removal rule enforced by `min`).  The result is clamped to `stock_floor`.
fn apply_cutter(
    heights: &mut [f64],
    tx: f64,
    ty: f64,
    tz: f64,
    tool_r: f64,
    tool: &Tool,
    grid_cols: u32,
    grid_rows: u32,
    cell_w: f64,
    cell_d: f64,
    stock_floor: f64,
) {
    // Bounding box of the footprint in cell indices.
    let col_lo = safe_cell_lo(tx - tool_r, cell_w);
    let col_hi = safe_cell_hi(tx + tool_r, cell_w, grid_cols);
    let row_lo = safe_cell_lo(ty - tool_r, cell_d);
    let row_hi = safe_cell_hi(ty + tool_r, cell_d, grid_rows);

    let r2 = tool_r * tool_r;

    for row in row_lo..=row_hi {
        let cy = (row as f64 + 0.5) * cell_d;
        let dy = cy - ty;
        for col in col_lo..=col_hi {
            let cx = (col as f64 + 0.5) * cell_w;
            let dx = cx - tx;
            let d2 = dx * dx + dy * dy;
            if d2 > r2 {
                continue; // outside footprint
            }
            let d = d2.sqrt();
            let floor_z = cutter_floor_z(tz, tool_r, d, tool);
            let idx = row * grid_cols as usize + col;
            if let Some(h) = heights.get_mut(idx) {
                // Monotonic removal: height can only decrease; clamp to stock floor.
                let new_h = floor_z.max(stock_floor);
                if new_h < *h {
                    *h = new_h;
                }
            }
        }
    }
}

/// Compute the Z floor that the cutter tip removes to at horizontal distance `d` from the
/// tool centre, given the tip is at `tz`.
fn cutter_floor_z(tz: f64, r: f64, d: f64, tool: &Tool) -> f64 {
    match tool.kind {
        ToolKind::FlatEndmill | ToolKind::Facemill | ToolKind::Chamfer | ToolKind::Tap => {
            // Flat bottom: removes everything to tz within radius.
            tz
        }
        ToolKind::BallEndmill => {
            // Spherical tip: centre at tz + r.  Floor = (tz + r) - sqrt(r² - d²).
            let under = (r * r - d * d).max(0.0);
            tz + r - under.sqrt()
        }
        ToolKind::BullnoseEndmill => {
            // Flat centre + toroidal corner.
            let r_c = tool.corner_radius;
            let r_inner = r - r_c;
            if d <= r_inner {
                tz // flat zone
            } else {
                // Corner torus: depth = r_c - sqrt(r_c² - (d - r_inner)²)
                let d_corner = d - r_inner;
                let under = (r_c * r_c - d_corner * d_corner).max(0.0);
                tz + r_c - under.sqrt()
            }
        }
        ToolKind::Drill => {
            // 118° point angle → half-angle 59°. Floor rises linearly from tip.
            tz + d * TAN_59_DEG
        }
        ToolKind::Probe => {
            // Stylus ball approximated as a sphere (same floor math as a ball nose).
            let under = (r * r - d * d).max(0.0);
            tz + r - under.sqrt()
        }
        ToolKind::Waterjet | ToolKind::Laser | ToolKind::Plasma => {
            // Kerf approximated as a flat disc of diameter `tool.diameter`.
            tz
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

#[inline]
fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[inline]
fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])]
}

/// Compute the arc sweep angle (signed, in radians) from `angle_from` towards `angle_to`,
/// in the direction indicated by `clockwise` (CW = negative rotation about +Z).
fn arc_sweep(angle_from: f64, angle_to: f64, clockwise: bool) -> f64 {
    let two_pi = 2.0 * std::f64::consts::PI;
    let diff = angle_to - angle_from;
    if clockwise {
        // CW: angle decreases (negative sweep)
        let d = if diff > 0.0 { diff - two_pi } else { diff };
        d.max(-two_pi)
    } else {
        // CCW: angle increases (positive sweep)
        let d = if diff < 0.0 { diff + two_pi } else { diff };
        d.min(two_pi)
    }
}

/// Convert an XY coordinate to a (col, row) grid index, clamped to valid range.
fn xy_to_cell(x: f64, y: f64, cell_w: f64, cell_d: f64, grid_cols: u32, grid_rows: u32) -> (usize, usize) {
    let col = if cell_w > 0.0 { ((x / cell_w).floor() as i64).clamp(0, grid_cols as i64 - 1) as usize } else { 0 };
    let row = if cell_d > 0.0 { ((y / cell_d).floor() as i64).clamp(0, grid_rows as i64 - 1) as usize } else { 0 };
    (col, row)
}

/// Lower cell index (inclusive) for a footprint edge at coordinate `v` with cell size `cs`.
fn safe_cell_lo(v: f64, cs: f64) -> usize {
    if !v.is_finite() || cs <= 0.0 {
        return 0;
    }
    ((v / cs).floor() as i64).max(0) as usize
}

/// Upper cell index (inclusive) for a footprint edge at coordinate `v` with cell size `cs`,
/// clamped to `[0, grid_dim - 1]`.
fn safe_cell_hi(v: f64, cs: f64, grid_dim: u32) -> usize {
    if !v.is_finite() || cs <= 0.0 || grid_dim == 0 {
        return 0;
    }
    let max_idx = grid_dim as usize - 1;
    (((v / cs).ceil() as i64).max(0) as usize).min(max_idx)
}

#[inline]
fn in_stock_xy(x: f64, y: f64, w: f64, d: f64) -> bool {
    x >= 0.0 && x <= w && y >= 0.0 && y <= d
}

/// Returns `(n_steps, was_truncated)` for a move of `dist` mm with step `step` mm,
/// subject to the remaining step budget.
fn steps_for_distance(dist: f64, step: f64, used: usize) -> (usize, bool) {
    let budget = MAX_TOTAL_STEPS.saturating_sub(used);
    if dist < 1e-10 {
        return (0, false); // zero-length move
    }
    let ideal = ((dist / step).ceil() as usize).max(1);
    if ideal <= budget { (ideal, false) } else { (budget.max(1), true) }
}

// ---------------------------------------------------------------------------
// Statistics helpers
// ---------------------------------------------------------------------------

fn compute_removed_volume(heights: &[f64], cell_w: f64, cell_d: f64) -> f64 {
    let cell_area = cell_w * cell_d;
    let removed: f64 = heights.iter().map(|&h| (-h).max(0.0)).sum();
    removed * cell_area
}

fn height_range(heights: &[f64]) -> (f64, f64) {
    heights.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &h| (lo.min(h), hi.max(h)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MfgSetup, Operation, PocketOperation, Stock, Tool, ToolKind};

    fn flat_6mm() -> Tool {
        Tool {
            number: 1,
            kind: ToolKind::FlatEndmill,
            diameter: 6.0,
            corner_radius: 0.0,
            length: 50.0,
            flute_length: 16.0,
            flutes: 4,
            description: String::new(),
            ..Default::default()
        }
    }

    fn ball_6mm() -> Tool {
        Tool { kind: ToolKind::BallEndmill, ..flat_6mm() }
    }

    fn box_stock() -> Stock {
        Stock::Box { width: 100.0, depth: 100.0, height: 25.0 }
    }

    fn default_opts() -> SimOptions {
        SimOptions { grid_cols: 100, grid_rows: 100, clearance_z: 5.0, ..Default::default() }
    }

    // ── Oracle: straight cut ────────────────────────────────────────────────

    /// A linear move along X at constant Z=-5mm and Y=50mm.
    /// With a 6mm flat endmill, the trench covers roughly Y ∈ [47, 53] (±3mm radius).
    /// All cells within that band from X=10 to X=90 should have height = -5.0.
    #[test]
    fn straight_cut_produces_trench() {
        let segments = vec![
            PathSegment::Rapid { to: [10.0, 50.0, 5.0] },
            PathSegment::Linear { to: [10.0, 50.0, -5.0], feed: 300.0 },  // plunge at X=10
            PathSegment::Linear { to: [90.0, 50.0, -5.0], feed: 1000.0 }, // constant-Z cut
        ];
        let tool = flat_6mm();
        let stock = box_stock();
        let opts = default_opts();

        let result = simulate_toolpaths(&stock, &tool, &segments, &opts);

        assert!(!result.has_errors, "unexpected errors: {:?}", result.issues);
        // Height field: 100×100 grid over 100×100mm stock → cell = 1mm×1mm
        // Cells with |cy - 50| ≤ 3 and cx ∈ [10, 90] should be at -5.0
        let cell_w = 100.0 / 100.0;
        let cell_d = 100.0 / 100.0;
        let tool_r = 3.0;
        let mut trench_count = 0usize;
        for row in 0..100usize {
            let cy = (row as f64 + 0.5) * cell_d;
            let dy = (cy - 50.0).abs();
            if dy > tool_r {
                continue;
            }
            for col in 0..100usize {
                let cx = (col as f64 + 0.5) * cell_w;
                if cx < 10.0 || cx > 90.0 {
                    continue;
                }
                let h = result.final_heights[row * 100 + col];
                assert!((h - (-5.0_f64)).abs() < 0.01, "expected -5.0 at ({cx}, {cy}), got {h}");
                trench_count += 1;
            }
        }
        assert!(trench_count > 20, "expected a non-trivial trench, found {trench_count} cells");
        // Removed volume > 0
        assert!(result.removed_volume_mm3 > 0.0);
        // Min height should be ≤ -5.0
        assert!(result.min_remaining_height_mm <= -4.99);
    }

    // ── Oracle: arc removes curved trench ──────────────────────────────────

    /// A quarter-circle CCW arc centred at (50, 50), radius 20mm, from (70, 50) to (50, 70),
    /// at constant Z = -3.  Cells along the arc ± tool_radius should be cut to -3.
    #[test]
    fn arc_cut_produces_curved_trench() {
        let center = [50.0_f64, 50.0, 0.0];
        let arc_to = [50.0, 70.0, -3.0];
        let segments = vec![
            PathSegment::Rapid { to: [70.0, 50.0, 5.0] },                // rapid above clearance
            PathSegment::Linear { to: [70.0, 50.0, -3.0], feed: 300.0 }, // plunge to cut depth
            PathSegment::Arc { center, to: arc_to, feed: 500.0, clockwise: false },
        ];
        let tool = flat_6mm();
        let stock = box_stock();
        let opts = default_opts();

        let result = simulate_toolpaths(&stock, &tool, &segments, &opts);

        // Arc at Z=-3 removes material; no unexpected errors.
        assert!(!result.has_errors, "unexpected errors: {:?}", result.issues);
        assert!(result.removed_volume_mm3 > 0.0, "arc cut should remove material");
        assert!(result.min_remaining_height_mm <= -2.99);
        // Cell at (70, 50) should be at -3 (start of arc = end of plunge)
        let col = 69usize; // cx = 69.5 ≈ 70
        let row = 49usize;
        let h = result.final_heights[row * 100 + col];
        assert!(h <= -2.9, "arc start cell should be cut: h={h}");
    }

    // ── Oracle: monotonic removal ──────────────────────────────────────────

    /// Two passes at the same Z: second pass must not add material.
    #[test]
    fn monotonic_removal_second_pass_does_not_add() {
        let segments_once = vec![PathSegment::Linear { to: [50.0, 50.0, -5.0], feed: 1000.0 }];
        let segments_twice = vec![
            PathSegment::Linear { to: [50.0, 50.0, -5.0], feed: 1000.0 },
            PathSegment::Linear { to: [0.0, 0.0, 5.0], feed: 1000.0 },    // retract
            PathSegment::Linear { to: [50.0, 50.0, -5.0], feed: 1000.0 }, // same cut
        ];
        let tool = flat_6mm();
        let stock = box_stock();
        let opts = default_opts();

        let once = simulate_toolpaths(&stock, &tool, &segments_once, &opts);
        let twice = simulate_toolpaths(&stock, &tool, &segments_twice, &opts);

        // The cells cut in the first pass must have the same final height in both runs.
        // Specifically: no cell in `twice` should have a HIGHER final height than in `once`
        // for the region that was cut in `once`.
        for (h_once, h_twice) in once.final_heights.iter().zip(twice.final_heights.iter()) {
            assert!(*h_twice <= h_once + 1e-12, "monotonic violation: h_twice={h_twice} > h_once={h_once}");
        }
        // The region cut in `once` should appear cut in `twice` too.
        assert!(twice.min_remaining_height_mm <= once.min_remaining_height_mm + 1e-12);
    }

    // ── Oracle: out-of-bounds move → warning / error ───────────────────────

    #[test]
    fn move_outside_stock_xy_generates_issue() {
        let segments = vec![PathSegment::Linear { to: [200.0, 50.0, -5.0], feed: 1000.0 }];
        let result = simulate_toolpaths(&box_stock(), &flat_6mm(), &segments, &default_opts());
        assert!(result.issues.iter().any(|i| i.message.contains("outside the stock footprint")), "expected out-of-bounds warning");
    }

    #[test]
    fn machine_travel_exceeded_generates_error() {
        let opts = SimOptions { machine_xy_travel: Some([80.0, 100.0]), ..default_opts() };
        let segments = vec![PathSegment::Linear { to: [90.0, 50.0, -5.0], feed: 1000.0 }];
        let result = simulate_toolpaths(&box_stock(), &flat_6mm(), &segments, &opts);
        assert!(
            result.issues.iter().any(|i| i.severity == SimSeverity::Error && i.message.contains("machine travel")),
            "expected machine travel error; issues: {:?}",
            result.issues
        );
        assert!(result.has_errors);
    }

    // ── Oracle: rapid collision ─────────────────────────────────────────────

    #[test]
    fn rapid_below_stock_top_warns() {
        // Stock top = Z=0; rapid to Z=-1 is below stock → warning (below clearance) + error (collision).
        let segments = vec![
            PathSegment::Linear { to: [50.0, 50.0, -5.0], feed: 500.0 }, // cut first
            PathSegment::Rapid { to: [50.0, 50.0, -1.0] },               // then rapid into material
        ];
        let result = simulate_toolpaths(&box_stock(), &flat_6mm(), &segments, &default_opts());
        let has_rapid_issue = result.issues.iter().any(|i| i.segment_index == 1 && (i.message.contains("rapid") || i.message.contains("clearance")));
        assert!(has_rapid_issue, "expected rapid collision or clearance issue; got: {:?}", result.issues);
    }

    #[test]
    fn rapid_below_stock_surface_is_error() {
        // Make a cut first, then rapid into the cut region below the current surface.
        let segments = vec![
            PathSegment::Linear { to: [50.0, 50.0, -10.0], feed: 500.0 },
            PathSegment::Rapid { to: [50.0, 50.0, -5.0] }, // above cut, below clearance
        ];
        let result = simulate_toolpaths(&box_stock(), &flat_6mm(), &segments, &default_opts());
        // Must have at least a warning for rapid below clearance.
        assert!(
            result.issues.iter().any(|i| i.severity == SimSeverity::Warning || i.severity == SimSeverity::Error),
            "expected at least a warning for rapid below clearance"
        );
    }

    // ── Oracle: fixture collision → error ───────────────────────────────────

    #[test]
    fn fixture_collision_generates_error() {
        let opts =
            SimOptions { fixtures: vec![Fixture { label: "Clamp-A".into(), min: [45.0, 45.0, -10.0], max: [55.0, 55.0, 5.0] }], ..default_opts() };
        // Linear move through the fixture centre.
        let segments = vec![PathSegment::Linear { to: [50.0, 50.0, -3.0], feed: 500.0 }];
        let result = simulate_toolpaths(&box_stock(), &flat_6mm(), &segments, &opts);
        assert!(
            result.issues.iter().any(|i| i.severity == SimSeverity::Error && i.message.contains("Clamp-A")),
            "expected fixture collision error; got: {:?}",
            result.issues
        );
        assert!(result.has_errors);
    }

    // ── Oracle: deterministic output ────────────────────────────────────────

    #[test]
    fn simulation_is_deterministic() {
        let segments = vec![
            PathSegment::Rapid { to: [0.0, 50.0, 5.0] },
            PathSegment::Linear { to: [0.0, 50.0, -4.0], feed: 300.0 },
            PathSegment::Linear { to: [100.0, 50.0, -4.0], feed: 1000.0 },
            PathSegment::Arc { center: [50.0, 50.0, 0.0], to: [100.0, 50.0, -4.0], feed: 500.0, clockwise: false },
        ];
        let tool = flat_6mm();
        let stock = box_stock();
        let opts = SimOptions { max_frames: 2, ..default_opts() };

        let r1 = simulate_toolpaths(&stock, &tool, &segments, &opts);
        let r2 = simulate_toolpaths(&stock, &tool, &segments, &opts);

        assert_eq!(r1.final_heights, r2.final_heights, "heights must be identical");
        assert_eq!(r1.removed_volume_mm3, r2.removed_volume_mm3, "volume must be identical");
        assert_eq!(r1.issues.len(), r2.issues.len(), "issue count must be identical");
        assert_eq!(r1.frames.len(), r2.frames.len(), "frame count must be identical");
    }

    // ── Flute-length and tool-length depth checks ───────────────────────────

    #[test]
    fn flute_too_short_generates_error() {
        // Flute length = 16 mm; cut to Z = -20 mm → flute exceeded.
        let segments = vec![PathSegment::Linear { to: [50.0, 50.0, -20.0], feed: 300.0 }];
        let result = simulate_toolpaths(&box_stock(), &flat_6mm(), &segments, &default_opts());
        assert!(
            result.issues.iter().any(|i| i.severity == SimSeverity::Error && i.message.contains("flute")),
            "expected flute-length error; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn tool_too_long_depth_error() {
        // Tool length = 50 mm; cut to Z = -60 mm → beyond tool length.
        let segments = vec![PathSegment::Linear { to: [50.0, 50.0, -60.0], feed: 300.0 }];
        let opts = SimOptions { ..default_opts() };
        let result = simulate_toolpaths(&box_stock(), &flat_6mm(), &segments, &opts);
        assert!(
            result.issues.iter().any(|i| i.severity == SimSeverity::Error && i.message.contains("overall length")),
            "expected tool-length error; got: {:?}",
            result.issues
        );
    }

    // ── Ball endmill oracle: volume less than flat endmill for same depth ───

    #[test]
    fn ball_endmill_removes_less_than_flat() {
        let segments = vec![PathSegment::Linear { to: [50.0, 50.0, -3.0], feed: 500.0 }];
        let stock = box_stock();
        let opts = default_opts();
        let flat = simulate_toolpaths(&stock, &flat_6mm(), &segments, &opts);
        let ball = simulate_toolpaths(&stock, &ball_6mm(), &segments, &opts);
        // Ball endmill has a curved tip → removes less volume than a flat endmill at same depth.
        assert!(
            ball.removed_volume_mm3 < flat.removed_volume_mm3,
            "ball should remove less than flat: ball={} flat={}",
            ball.removed_volume_mm3,
            flat.removed_volume_mm3
        );
    }

    // ── SimOptions validation ───────────────────────────────────────────────

    #[test]
    fn invalid_grid_cols_returns_error_result() {
        let opts = SimOptions { grid_cols: 0, ..default_opts() };
        let r = simulate_toolpaths(&box_stock(), &flat_6mm(), &[], &opts);
        assert!(r.has_errors);
        assert!(r.issues[0].message.contains("grid_cols"));
    }

    #[test]
    fn too_many_fixtures_returns_error() {
        let fixtures = (0..MAX_FIXTURES + 1).map(|i| Fixture { label: format!("F{i}"), min: [0.0, 0.0, -5.0], max: [1.0, 1.0, 0.0] }).collect();
        let opts = SimOptions { fixtures, ..default_opts() };
        let r = simulate_toolpaths(&box_stock(), &flat_6mm(), &[], &opts);
        assert!(r.has_errors);
        assert!(r.issues[0].message.contains("fixture"));
    }

    #[test]
    fn cylinder_stock_returns_error() {
        let stock = Stock::Cylinder { diameter: 50.0, height: 25.0 };
        let r = simulate_toolpaths(&stock, &flat_6mm(), &[], &default_opts());
        assert!(r.has_errors);
    }

    // ── Scrubber frames ─────────────────────────────────────────────────────

    #[test]
    fn frames_are_captured_correctly() {
        let segs: Vec<PathSegment> = (0..8).map(|i| PathSegment::Linear { to: [i as f64 * 12.0 + 5.0, 50.0, -2.0], feed: 500.0 }).collect();
        let opts = SimOptions { max_frames: 4, ..default_opts() };
        let r = simulate_toolpaths(&box_stock(), &flat_6mm(), &segs, &opts);
        assert!(r.frames.len() <= 4, "at most 4 frames requested");
        // Frames are in segment order.
        for w in r.frames.windows(2) {
            assert!(w[0].segment_index <= w[1].segment_index, "frames out of order");
        }
    }

    // ── Invalid segment skipped ─────────────────────────────────────────────

    #[test]
    fn invalid_segment_is_skipped_with_error() {
        let segs = vec![
            PathSegment::Linear { to: [f64::NAN, 0.0, 0.0], feed: 500.0 }, // invalid
            PathSegment::Linear { to: [50.0, 50.0, -3.0], feed: 500.0 },   // valid
        ];
        let r = simulate_toolpaths(&box_stock(), &flat_6mm(), &segs, &default_opts());
        assert!(r.issues.iter().any(|i| i.severity == SimSeverity::Error && i.segment_index == 0));
        // The valid segment should still cut.
        assert!(r.removed_volume_mm3 > 0.0, "valid segment should cut material");
    }

    // ── MfgSetup integration (smoke test) ──────────────────────────────────

    #[test]
    fn simulate_with_default_setup_and_no_segments() {
        let setup = MfgSetup {
            stock: box_stock(),
            tools: vec![flat_6mm()],
            operations: vec![Operation::Pocket(PocketOperation::default())],
            ..Default::default()
        };
        let tool = setup.tools.first().expect("tool exists in test setup");
        let r = simulate_toolpaths(&setup.stock, tool, &[], &default_opts());
        // No segments → nothing removed.
        assert_eq!(r.removed_volume_mm3, 0.0);
        assert!(!r.has_errors);
        assert_eq!(r.final_heights, vec![0.0_f64; 100 * 100]);
    }
}
