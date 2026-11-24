//! A planar region extruded along its frame's normal between two levels: the prism the planar
//! extrusion of `solidcore-ops` joins its pieces from.
//!
//! Not upstream: the placement SolidCraft's extrusion of planar regions made (`crate::extrude`
//! for one region), moved into the kernel (docs/layers.md §1). The solid is the kernel's linear
//! sweep (`SmApiCreateLinearSweep`) of the region's curves.
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::profile::{axes, curve_on_frame, Region2};
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::kernel::Kernel;
use crate::sweep::api_brep::create_linear_sweep;
use crate::topology::Brep;

const OP: &str = "prism::extrude::extrude_region";

/// The solid of `region` (in `frame`'s coordinates) from level `lo` to level `hi` above the
/// frame: its loops' curves placed on the frame at `lo` ([`curve_on_frame`]) and swept along the
/// frame's normal by `hi - lo`, capped at both ends.
///
/// # Errors
///
/// `InvalidInput` for levels not finite or not increasing; those of the curves and the sweep.
pub fn extrude_region(
    k: &mut Kernel,
    frame: &Frame,
    region: &Region2,
    lo: f64,
    hi: f64,
) -> Result<Brep> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SolidError::invalid_input(
            OP,
            "levels not finite and increasing",
        ));
    }
    let curves = region
        .loops()
        .flat_map(|lp| &lp.segs)
        .map(|s| curve_on_frame(k, frame, lo, s))
        .collect::<Result<Vec<_>>>()?;
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let (_, _, _, n) = axes(frame);
    create_linear_sweep(k, &refs, n, hi - lo, true)
}

/// Positive and negative wing `(start, end)` intervals for a symmetric-offset extrude with
/// clearance `gap` on each side of the symmetry plane and `half` distance per wing.
#[must_use]
pub fn symmetric_offset_intervals(gap: f64, half: f64) -> [(f64, f64); 2] {
    let gap = gap.abs();
    [(gap, gap + half), (-gap - half, -gap)]
}

/// Probe points along both wings of a symmetric-offset extrude for auto-operation detection
/// (cut vs join), given the sketch-plane offset `start`, total extent `d`, and `near_limit`.
#[must_use]
pub fn symmetric_offset_probe_points(
    base: [f64; 3],
    n: [f64; 3],
    start: f64,
    d: f64,
    near_limit: f64,
) -> [[f64; 3]; 4] {
    let gap = start.abs();
    let half = d.abs() * 0.5;
    let near = (half * 0.5).min(near_limit);
    let mid = half * 0.5;
    let at = |s: f64| [base[0] + n[0] * s, base[1] + n[1] * s, base[2] + n[2] * s];
    [
        at(-start + gap + near),
        at(-start + gap + mid),
        at(-start - gap - near),
        at(-start - gap - mid),
    ]
}
