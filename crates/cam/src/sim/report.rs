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
    /// Radii over (x, θ): cell `(i, k)` has its centre at `x = x0 + (i + ½)·dx`, `θ = (k + ½)·dθ`
    /// (`dθ = 2π / nt`) and its radius at `radii[k·nx + i]`; `gaps`, by cell `k·nx + i` in order,
    /// the stretches of air inside the material of the rays that have some.
    Radii { x0: f64, dx: f64, nx: usize, nt: usize, radius: f64, radii: Vec<f64>, gaps: Vec<(usize, Gaps)> },
}

/// One motion block as simulated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockRemoval {
    /// The G-code line, from 1.
    pub line: usize,
    pub rapid: bool,
    /// Material it removed (mm³).
    pub volume: f64,
    /// The deepest it cut into the stock as it was (mm, along Z or the radius).
    pub depth: f64,
    /// A cut at the tool's full width (a slot): material on both sides of the tool and under
    /// it ahead, halfway along the move.
    pub slot: bool,
}

/// What the simulation found.
#[derive(Clone, Debug, PartialEq)]
pub struct SimReport {
    /// The cell size used (mm; on a 4th axis, along X and round the stock's surface).
    pub cell: f64,
    pub cells: usize,
    /// The stock as the program leaves it.
    pub stock: StockGrid,
    /// Motion blocks simulated (0: the program has none whose position is known, as when a 4th
    /// axis is never written, so nothing was checked).
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
    /// Material left on the part anywhere, not only where the program cut ([`PartLeft`]).
    pub part_left: PartLeft,
    /// Cutting moves that removed nothing (cutting air), and their length (mm).
    pub air_moves: usize,
    pub air_length: f64,
    /// Time at feed as the controller times it, dwells included (minutes).
    pub minutes: f64,
    pub feed_length: f64,
    pub rapid_length: f64,
    pub warnings: Vec<String>,
    /// The stock along X and Y as the program leaves it, when the simulation kept it beside the
    /// height map ([`SimState::track_sides`]), or as it stood when it was given up (holding no
    /// less than the material left); empty otherwise. A next setup turned onto its side
    /// reads the material that runs under other material from them.
    pub sides: Vec<SideLines>,
}

impl SimState {
    /// What is left held against `part`, and the report [`simulate`] gives.
    pub fn finish(self, plan: &SimPlan, part: &Surface) -> SimReport {
        let (tol, initial) = (plan.tol, plan.initial);
        let refine = (MAX_REFINED / self.bands.len().max(1)).max(1);
        let threaded = self.bands.len() > 1 && plan.cells() > 100_000;
        let models: Vec<&Model> = self.bands.iter().collect();
        // How far the part reaches from the A axis along X, once for every band.
        let reach = match plan.grid {
            Grid::Radii { x0, dx, nx, .. } => column_reach(part, x0, dx, nx),
            Grid::Heights { .. } => Vec::new(),
        };
        // Every so many columns each way for the whole part's leftover, about WHOLE_SAMPLES of them.
        let stride = ((plan.cells() as f64 / WHOLE_SAMPLES as f64).sqrt().ceil() as usize).max(1);
        let sides = self.side.is_some();
        let cmps = each_threaded(models, threaded, |m| Ok(compare(m, part, tol, initial, refine, &reach, stride, sides))).unwrap_or_default();
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
            warnings.push(
                "no move of the program was simulated: none comes after every axis of the machine (its rotary included) is written, so \
                 nothing was checked"
                    .into(),
            );
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
        if self.side_dropped {
            warnings.push(
                "the stock along X and Y (kept for a part with faces under its own material along Z) took too long to cut and was given \
                 up: what is left under the part's own material is not checked as cut, and is measured as on a height map alone"
                    .into(),
            );
        }
        if let Some(side) = &self.side {
            let (left, worst, worst_at, gouged) = side.compare(part, tol);
            cmp.add(&Comparison { worst, worst_at, gouged, part_left: left, ..Comparison::default() });
            let unmodelled = side.unmodelled();
            if unmodelled > 0 {
                warnings.push(format!(
                    "{unmodelled} times the tool cut a line of the stock along X or Y that already holds {MAX_SIDE_PIECES} stretches of \
                     material: the stock model leaves those cuts out of it there, so under the part's own material they are not checked \
                     here (the job's gouge check covers them)"
                ));
            }
        }
        if cmp.unrefined > 0 {
            warnings.push(format!("{} gouged cells were measured along Z (or the radius) only: too many to measure in 3D", cmp.unrefined));
        }
        let unmodelled: u64 = self.bands.iter().map(|b| if let Model::Radii(m) = b { m.unmodelled } else { 0 }).sum();
        if unmodelled > 0 {
            warnings.push(format!(
                "{unmodelled} times the tool cut beside a ray from the A axis where the ray already holds {MAX_GAPS} stretches of air (or the \
                 stock holds too many): the stock model leaves those cuts out, so they are not checked here (the job's gouge check covers \
                 them)"
            ));
        }
        let (mut air_moves, mut air_length) = (0usize, 0.0);
        for (k, step) in plan.steps.iter().enumerate() {
            if !step.rapid && step.from.is_some() && self.removed.get(k).is_none_or(|r| r.volume <= 0.0) {
                air_moves += 1;
                air_length += step.travel;
            }
        }
        let mut values = Vec::with_capacity(plan.cells());
        let mut gaps = Vec::new();
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
                Model::Radii(r) => {
                    let base = r.k0 * r.nx;
                    gaps.extend(r.gaps.iter().map(|(c, g)| (base + c, g.clone())));
                    values.extend(r.rho);
                }
            }
        }
        let stock = stock_grid(&plan.grid, values, gaps);
        let kept = self.side.as_ref().or(self.given_up.as_ref());
        let sides = kept.map(|s| s.sets.iter().map(SideLines::of).collect()).unwrap_or_default();
        SimReport {
            cell: plan.cell,
            cells: match &stock {
                StockGrid::Heights { heights, .. } => heights.len(),
                StockGrid::Radii { radii, .. } => radii.len(),
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
            part_left: cmp.part_left,
            air_moves,
            air_length,
            minutes: plan.minutes,
            feed_length: plan.feed_length,
            rapid_length: plan.rapid_length,
            warnings,
            sides,
        }
    }
}

pub(super) fn stock_grid(grid: &Grid, values: Vec<f64>, mut gaps: Vec<(usize, Gaps)>) -> StockGrid {
    match *grid {
        Grid::Heights { x0, y0, dx, dy, nx, ny, bottom, top } => StockGrid::Heights { x0, y0, dx, dy, nx, ny, bottom, top, heights: values },
        Grid::Radii { x0, dx, nx, nt, radius, .. } => {
            gaps.sort_by_key(|(c, _)| *c);
            StockGrid::Radii { x0, dx, nx, nt, radius, radii: values, gaps }
        }
    }
}

impl StockGrid {
    /// The stock's surface as triangles (top heights, or the radial surface), every `step`-th
    /// cell so that a side has at most `max_side` points; for a preview.
    pub fn mesh(&self, max_side: usize) -> Mesh {
        let max_side = max_side.max(2);
        let mut m = Mesh::default();
        let (nu, nv, point): (usize, usize, Box<dyn Fn(usize, usize) -> Option<Vec3> + '_>) = match self {
            StockGrid::Heights { x0, y0, dx, dy, nx, ny, heights, .. } => (
                *nx,
                *ny,
                Box::new(move |i, j| heights.get(j * nx + i).map(|&z| Vec3::new(x0 + (i as f64 + 0.5) * dx, y0 + (j as f64 + 0.5) * dy, z))),
            ),
            StockGrid::Radii { x0, dx, nx, nt, radii, .. } => (
                *nx,
                *nt,
                Box::new(move |i, k| {
                    let th = (k as f64 + 0.5) * TAU / *nt as f64;
                    radii.get(k * nx + i).map(|&r| Vec3::new(x0 + (i as f64 + 0.5) * dx, r * th.sin(), r * th.cos()))
                }),
            ),
        };
        let su = nu.div_ceil(max_side).max(1);
        let sv = nv.div_ceil(max_side).max(1);
        let us: Vec<usize> = (0..nu).step_by(su).chain((nu > 0 && (nu - 1) % su != 0).then(|| nu - 1)).collect();
        let vs: Vec<usize> = (0..nv).step_by(sv).chain((nv > 0 && (nv - 1) % sv != 0).then(|| nv - 1)).collect();
        for &v in &vs {
            for &u in &us {
                m.positions.push(point(u, v).unwrap_or(Vec3::ZERO));
            }
        }
        let wrap = matches!(self, StockGrid::Radii { .. });
        let (cu, cv) = (us.len(), vs.len());
        let rows = if wrap { cv } else { cv.saturating_sub(1) };
        for b in 0..rows {
            let b1 = (b + 1) % cv.max(1);
            for a in 0..cu.saturating_sub(1) {
                let at = |a: usize, b: usize| u32::try_from(b * cu + a).ok();
                if let (Some(p), Some(q), Some(r), Some(s)) = (at(a, b), at(a + 1, b), at(a + 1, b1), at(a, b1)) {
                    m.triangles.push([p, q, r]);
                    m.triangles.push([p, r, s]);
                }
            }
        }
        // Air inside the rays' material (a pocket or a flat in the side seen at a slant): a
        // patch where each stretch starts (facing out, a floor) and ends (facing in).
        if let StockGrid::Radii { x0, dx, nx, nt, gaps, .. } = self {
            let (dth, half_x) = (TAU / (*nt).max(1) as f64, 0.5 * su as f64 * dx);
            for &v in &vs {
                for &u in &us {
                    let Ok(at) = gaps.binary_search_by_key(&(v * nx + u), |(c, _)| *c) else { continue };
                    let Some((_, list)) = gaps.get(at) else { continue };
                    let x = x0 + (u as f64 + 0.5) * dx;
                    let th = (v as f64 + 0.5) * dth;
                    let (t0, t1) = (th - 0.5 * sv as f64 * dth, th + 0.5 * sv as f64 * dth);
                    for g in list {
                        for (r, out) in [(g[0], true), (g[1], false)] {
                            let p = |xx: f64, t: f64| Vec3::new(xx, r * t.sin(), r * t.cos());
                            let Ok(base) = u32::try_from(m.positions.len()) else { return m };
                            m.positions.extend([p(x - half_x, t0), p(x + half_x, t0), p(x + half_x, t1), p(x - half_x, t1)]);
                            if out {
                                m.triangles.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
                            } else {
                                m.triangles.extend([[base, base + 3, base + 2], [base, base + 2, base + 1]]);
                            }
                        }
                    }
                }
            }
        }
        m
    }
}

impl StockGrid {
    /// The highest the stock is left over the cells whose centres are within `radius` of
    /// `center` (work XY), with that cell's centre: what stands in a round hole cut down Z. `None`
    /// on a 4th axis's stock, or with no cell centre in the circle.
    pub fn highest_in_circle(&self, center: Vec2, radius: f64) -> Option<(f64, Vec2)> {
        let StockGrid::Heights { x0, y0, dx, dy, nx, ny, heights, .. } = self else { return None };
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
