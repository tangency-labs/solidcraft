//! What the simulation reports: the stock the program leaves, every block that removed
//! material, rapids that cut, and what is left held against the part.

use super::*;

/// Most cells measured again in 3D for the gouge depth; beyond, the vertical depth is used.
const MAX_REFINED: usize = 1_000_000;

/// The stock the program leaves.
#[derive(Clone, Debug, PartialEq)]
pub enum StockGrid {
    /// Heights over the box's XY: cell `(i, j)` has its centre at `(x0 + (i + ½)·dx, y0 + (j + ½)·dy)`
    /// and its height at `heights[j·nx + i]`.
    Heights { x0: f64, y0: f64, dx: f64, dy: f64, nx: usize, ny: usize, bottom: f64, top: f64, heights: Vec<f64> },
}

/// One motion block as simulated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockRemoval {
    /// The G-code line, from 1.
    pub line: usize,
    pub rapid: bool,
    /// Material it removed (mm³).
    pub volume: f64,
    /// The deepest it cut into the stock as it was (mm, along Z).
    pub depth: f64,
    /// A cut at the tool's full width (a slot): material on both sides of the tool and under
    /// it ahead, halfway along the move.
    pub slot: bool,
}

/// What the simulation found.
#[derive(Clone, Debug, PartialEq)]
pub struct SimReport {
    /// The cell size used (mm).
    pub cell: f64,
    pub cells: usize,
    /// The stock as the program leaves it.
    pub stock: StockGrid,
    /// Motion blocks simulated (0: the program has none whose position is known, so nothing
    /// was checked).
    pub moves: usize,
    /// Every motion block that removed material, in program order.
    pub removals: Vec<BlockRemoval>,
    /// Rapids that cut more than the tolerance deep: a crash on the machine.
    pub rapids_in_material: Vec<BlockRemoval>,
    pub removed_volume: f64,
    /// The deepest cut into the part beyond the tolerance (mm; 0: none) and the cells cut so.
    pub gouge_worst: f64,
    pub gouge_cells: usize,
    /// Where the deepest cut into the part is (work coordinates), when there is one.
    pub gouge_at: Option<Vec3>,
    /// Material left above the part over the cells the program cut (mm), and where the most is.
    pub leftover_max: f64,
    pub leftover_at: Option<Vec3>,
    pub leftover_mean: f64,
    pub machined_cells: usize,
    /// Cutting moves that removed nothing (cutting air), and their length (mm).
    pub air_moves: usize,
    pub air_length: f64,
    /// Time at feed as the controller times it, dwells included (minutes).
    pub minutes: f64,
    pub feed_length: f64,
    pub rapid_length: f64,
    pub warnings: Vec<String>,
}

impl SimState {
    /// What is left held against `part`, and the report [`simulate`] gives.
    pub fn finish(self, plan: &SimPlan, part: &Surface) -> SimReport {
        let (tol, initial) = (plan.tol, plan.initial);
        let refine = (MAX_REFINED / self.bands.len().max(1)).max(1);
        let threaded = self.bands.len() > 1 && plan.cells() > 100_000;
        let models: Vec<&Model> = self.bands.iter().collect();
        let cmps = each_threaded(models, threaded, |m| Ok(compare(m, part, tol, initial, refine))).unwrap_or_default();
        let mut removals = Vec::new();
        for (k, step) in plan.steps.iter().enumerate() {
            let r = self.removed.get(k).copied().unwrap_or_default();
            let slot = self.probes.get(k).copied().unwrap_or(0) == 0b111;
            if r.volume > 0.0 || slot {
                removals.push(BlockRemoval { line: step.line, rapid: step.rapid, volume: r.volume, depth: r.depth, slot });
            }
        }
        let rapids_in_material: Vec<BlockRemoval> = removals.iter().filter(|r| r.rapid && r.depth > tol).copied().collect();
        let removed_volume = removals.iter().map(|r| r.volume).sum();
        let mut warnings = Vec::new();
        if plan.steps.is_empty() {
            warnings
                .push("no move of the program was simulated: none comes after every axis of the machine is written, so nothing was checked".into());
        }
        let slots: Vec<usize> = removals.iter().filter(|r| r.slot).map(|r| r.line).collect();
        if !slots.is_empty() {
            let shown: Vec<String> = slots.iter().take(10).map(usize::to_string).collect();
            warnings.push(format!(
                "{} cutting moves take the tool's full width (a slot), lines {}{}: on a light machine keep their depth and feed low",
                slots.len(),
                shown.join(", "),
                if slots.len() > shown.len() { ", …" } else { "" }
            ));
        }
        let mut cmp = Comparison::default();
        for c in &cmps {
            cmp.add(c);
        }
        if cmp.unrefined > 0 {
            warnings.push(format!("{} gouged cells were measured along Z only: too many to measure in 3D", cmp.unrefined));
        }
        let (mut air_moves, mut air_length) = (0usize, 0.0);
        for (k, step) in plan.steps.iter().enumerate() {
            if !step.rapid && step.from.is_some() && self.removed.get(k).is_none_or(|r| r.volume <= 0.0) {
                air_moves += 1;
                air_length += step.travel;
            }
        }
        let mut values = Vec::with_capacity(plan.cells());
        for b in self.bands {
            match b {
                Model::Heights(h) => {
                    // A column cut down past where its material ends below (an earlier setup,
                    // upside down from this one, cut it from what is now its underside) is empty:
                    // its top is the stock's bottom.
                    let bottom = h.bottom;
                    let under = h.under;
                    values.extend(h.h.into_iter().enumerate().map(|(c, z)| match under.get(c) {
                        Some(&u) if u > bottom && z <= u => bottom,
                        _ => z,
                    }));
                }
            }
        }
        let stock = stock_grid(&plan.grid, values);
        SimReport {
            cell: plan.cell,
            cells: match &stock {
                StockGrid::Heights { heights, .. } => heights.len(),
            },
            stock,
            moves: plan.steps.len(),
            removals,
            rapids_in_material,
            removed_volume,
            gouge_worst: cmp.worst,
            gouge_cells: cmp.gouged,
            gouge_at: (cmp.gouged > 0).then_some(cmp.worst_at),
            leftover_max: cmp.leftover_max,
            leftover_at: (cmp.leftover_max > 0.0).then_some(cmp.leftover_at),
            leftover_mean: if cmp.machined > 0 { cmp.leftover_sum / cmp.machined as f64 } else { 0.0 },
            machined_cells: cmp.machined,
            air_moves,
            air_length,
            minutes: plan.minutes,
            feed_length: plan.feed_length,
            rapid_length: plan.rapid_length,
            warnings,
        }
    }
}

pub(super) fn stock_grid(grid: &Grid, values: Vec<f64>) -> StockGrid {
    match *grid {
        Grid::Heights { x0, y0, dx, dy, nx, ny, bottom, top } => StockGrid::Heights { x0, y0, dx, dy, nx, ny, bottom, top, heights: values },
    }
}

impl StockGrid {
    /// The highest the stock is left over the cells whose centres are within `radius` of
    /// `center` (work XY), with that cell's centre: what stands in a round hole cut down Z. `None`
    /// with no cell centre in the circle.
    pub fn highest_in_circle(&self, center: Vec2, radius: f64) -> Option<(f64, Vec2)> {
        let StockGrid::Heights { x0, y0, dx, dy, nx, ny, heights, .. } = self;
        if !(radius > 0.0 && radius.is_finite() && center.x.is_finite() && center.y.is_finite() && *dx > 0.0 && *dy > 0.0) {
            return None;
        }
        let span = |lo: f64, d: f64, n: usize, c: f64| -> Option<(usize, usize)> {
            let a = ((c - radius - lo) / d - 0.5).ceil().max(0.0);
            let b = ((c + radius - lo) / d - 0.5).floor().min(n as f64 - 1.0);
            (a <= b).then(|| (index(a, n), index(b, n)))
        };
        let ((i0, i1), (j0, j1)) = (span(*x0, *dx, *nx, center.x)?, span(*y0, *dy, *ny, center.y)?);
        let mut best: Option<(f64, Vec2)> = None;
        for j in j0..=j1 {
            let y = y0 + (j as f64 + 0.5) * dy;
            for i in i0..=i1 {
                let x = x0 + (i as f64 + 0.5) * dx;
                if (x - center.x).hypot(y - center.y) > radius {
                    continue;
                }
                if let Some(&h) = heights.get(j * nx + i)
                    && best.is_none_or(|(b, _)| h > b)
                {
                    best = Some((h, Vec2::new(x, y)));
                }
            }
        }
        best
    }
}
