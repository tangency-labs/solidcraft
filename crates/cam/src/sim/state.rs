//! The stock part way through a program: its bands of rows, each cut by every step in program
//! order on a thread of its own, and what each step removed.

use super::*;

/// Most threads one simulation runs on, and the fewest rows (angles) each one takes.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const MAX_THREADS: usize = 16;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const MIN_BAND: usize = 16;
/// Estimated cell visits above which one advance runs its bands on threads.
const THREADED_VISITS: f64 = 250_000.0;

#[derive(Clone)]
pub(super) enum Model {
    Heights(Heights),
    Radii(Radii),
}

impl Model {
    pub(super) fn owns(&self, p: Vec3) -> Option<bool> {
        match self {
            Model::Heights(h) => h.owns(p),
            Model::Radii(r) => r.owns(p),
        }
    }

    pub(super) fn set_visits(&mut self, v: &Arc<AtomicU64>) {
        match self {
            Model::Heights(h) => h.visits = Arc::clone(v),
            Model::Radii(r) => r.visits = Arc::clone(v),
        }
    }

    pub(super) fn set_floor(&mut self, f: &Arc<PartFloor>) {
        match self {
            Model::Heights(h) => h.floor = Some(Arc::clone(f)),
            Model::Radii(r) => r.floor = Some(Arc::clone(f)),
        }
    }

    pub(super) fn set_tool(&mut self, slot: u8, flute: f64) {
        match self {
            Model::Heights(h) => (h.slot, h.flute) = (slot, flute),
            Model::Radii(r) => (r.slot, r.flute) = (slot, flute),
        }
    }

    /// The band's first row (angle) and its rows (angles).
    pub(super) fn span(&self) -> (usize, usize) {
        match self {
            Model::Heights(h) => (h.j0, h.ny),
            Model::Radii(r) => (r.k0, r.nt),
        }
    }

    pub(super) fn values(&self) -> &[f64] {
        match self {
            Model::Heights(h) => &h.h,
            Model::Radii(r) => &r.rho,
        }
    }

    pub(super) fn last(&self) -> &[u8] {
        match self {
            Model::Heights(h) => &h.last,
            Model::Radii(r) => &r.last,
        }
    }

    pub(super) fn tiles(&self) -> &Tiles {
        match self {
            Model::Heights(h) => &h.tiles,
            Model::Radii(r) => &r.tiles,
        }
    }
}

/// Cuts the part of `step` from fraction `f0` to `f1` of the way along through one band.
pub(super) fn cut(model: &mut Model, step: &Step, machine: &Machine, f0: f64, f1: f64) -> Result<Removed> {
    model.set_tool(step.slot, step.flute);
    let whole = f0 <= 0.0 && f1 >= 1.0;
    Ok(match model {
        Model::Heights(h) => match &step.how {
            How::Arc { work_center, sweep, .. } => {
                let (a, b) = (step.start.tip, step.end.tip);
                if whole || chorded(a, b, *work_center, *sweep) {
                    h.sweep_arc(&step.profile, a, b, *work_center, *sweep, (f0, f1))?
                } else {
                    let (p, q) = (arc_point(a, b, *work_center, *sweep, f0), arc_point(a, b, *work_center, *sweep, f1));
                    h.sweep_arc(&step.profile, p, q, *work_center, *sweep * (f1 - f0), (0.0, 1.0))?
                }
            }
            How::Tilted { arc, chords, rotary } => {
                let (k0, k1) = (done(*chords, f0), done(*chords, f1));
                let n = (*chords).max(1) as f64;
                let mut out = Removed::default();
                let mut prev = if k0 == 0 { step.start.tip } else { pose_at(machine, arc.at(k0 as f64 / n), rotary).tip };
                for k in k0 + 1..=k1 {
                    let tip = pose_at(machine, arc.at(k as f64 / n), rotary).tip;
                    out.add(h.sweep_line(&step.profile, prev, tip)?);
                    prev = tip;
                }
                out
            }
            _ => {
                let a = if f0 <= 0.0 { step.start.tip } else { step.start.tip.lerp(step.end.tip, f0) };
                let b = if f1 >= 1.0 { step.end.tip } else { step.start.tip.lerp(step.end.tip, f1) };
                h.sweep_line(&step.profile, a, b)?
            }
        },
        Model::Radii(r) => {
            let (first, last) = step.pose_range();
            let total = last + 1 - first;
            let (d0, d1) = (done(total, f0), done(total, f1));
            let mut out = Removed::default();
            if d1 > d0 {
                each_pose(machine, step, first + d0..=first + d1 - 1, |pose| {
                    out.add(r.stamp(&step.profile, pose)?);
                    Ok(())
                })?;
            }
            out
        }
    })
}

/// A piece of work: step `k` from fraction `f0` to `f1` of the way along.
pub(super) type Work = (usize, f64, f64);

/// Runs `work` through one band: what each piece removed, and which probes were in the stock
/// before its step started.
pub(super) fn band_advance(model: &mut Model, plan: &SimPlan, work: &[Work]) -> Result<Vec<(Removed, u8)>> {
    let mut out = Vec::with_capacity(work.len());
    for &(k, f0, f1) in work {
        let Some(step) = plan.steps.get(k) else { continue };
        let mut bits = 0u8;
        if f0 <= 0.0
            && let Some(ps) = &step.probes
        {
            for (b, p) in ps.iter().enumerate() {
                if model.owns(*p) == Some(true) {
                    bits |= 1 << b;
                }
            }
        }
        out.push((cut(model, step, &plan.machine, f0, f1)?, bits));
    }
    Ok(out)
}

/// Bands to split `lines` rows (angles) into: one per core up to [`MAX_THREADS`], each at least
/// [`MIN_BAND`] rows; one on the web, which has no threads.
pub(super) fn band_count(lines: usize) -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = lines;
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        cores.min(MAX_THREADS).min(lines / MIN_BAND).max(1)
    }
}

/// `f` run for every item, each on a thread of its own when `threaded` and one can be had (else
/// here), the results in order.
pub(super) fn each_threaded<T: Send, R: Send>(items: Vec<T>, threaded: bool, f: impl Fn(T) -> Result<R> + Sync) -> Result<Vec<R>> {
    if !threaded || items.len() <= 1 {
        return items.into_iter().map(&f).collect();
    }
    let f = &f;
    let lost = || CamError::Unsupported("a simulation thread lost its work".into());
    std::thread::scope(|scope| {
        let mut pending = Vec::with_capacity(items.len());
        for (b, item) in items.into_iter().enumerate() {
            // The item goes through a slot the thread takes it from, so that it is still here
            // when no thread can be had.
            let slot = Arc::new(Mutex::new(Some(item)));
            let mine = Arc::clone(&slot);
            let job = move || match mine.lock().ok().and_then(|mut s| s.take()) {
                Some(item) => f(item),
                None => Err(lost()),
            };
            match std::thread::Builder::new().name(format!("cam-sim-{b}")).spawn_scoped(scope, job) {
                Ok(h) => pending.push(Err(h)),
                Err(_) => pending.push(Ok(match slot.lock().ok().and_then(|mut s| s.take()) {
                    Some(item) => f(item),
                    None => Err(lost()),
                })),
            }
        }
        pending
            .into_iter()
            .map(|p| match p {
                Ok(done) => done,
                Err(h) => h.join().unwrap_or_else(|_| Err(CamError::Unsupported("a simulation thread stopped".into()))),
            })
            .collect()
    })
}

/// The stock part way through a program: every band of it, and what each step removed so far.
/// Cloning it (a keyframe) copies the stock.
pub struct SimState {
    pub(super) bands: Vec<Model>,
    /// What each step removed, every band's share added in band order.
    pub(super) removed: Vec<Removed>,
    pub(super) probes: Vec<u8>,
    /// Steps before `next` are done, and `frac` of step `next`.
    pub(super) next: usize,
    pub(super) frac: f64,
    pub(super) visits: Arc<AtomicU64>,
    pub(super) failed: bool,
    /// The stock along X and Y beside the height map, for a part with faces under its own
    /// material along Z ([`SideDexels`]); `None` for every other part (no cost on them).
    pub(super) side: Option<Box<SideDexels>>,
    /// The side lines were given up part way: cutting them would have taken too long.
    pub(super) side_dropped: bool,
    /// The side lines as they stood when they were given up: what they hold is no less than the
    /// material left (a side line only ever loses material), so a next setup still reads from
    /// them what an earlier one cut under other material ([`SimReport::sides`]).
    pub(super) given_up: Option<Box<SideDexels>>,
}

impl Clone for SimState {
    fn clone(&self) -> SimState {
        let visits = Arc::new(AtomicU64::new(0));
        let mut bands = self.bands.clone();
        for b in &mut bands {
            b.set_visits(&visits);
        }
        SimState {
            bands,
            removed: self.removed.clone(),
            probes: self.probes.clone(),
            next: self.next,
            frac: self.frac,
            visits,
            failed: self.failed,
            side: self.side.clone(),
            side_dropped: self.side_dropped,
            given_up: self.given_up.clone(),
        }
    }
}

impl SimState {
    /// The stock before the program.
    pub fn new(plan: &SimPlan) -> SimState {
        let visits = Arc::new(AtomicU64::new(0));
        let bands = plan.bands.iter().map(|&(from, to)| plan.grid.band(from, to, &visits, plan.tol)).collect();
        SimState {
            bands,
            removed: Vec::new(),
            probes: Vec::new(),
            next: 0,
            frac: 0.0,
            visits,
            failed: false,
            side: None,
            side_dropped: false,
            given_up: None,
        }
    }

    /// The stock as an earlier setup left it: every cell of a height grid no higher than the
    /// highest `start` has anywhere over the cell (a cell off `start` keeps the stock's top), so a
    /// map written on other cells never claims air where the earlier setup left material (the
    /// wall of its cut across a cell is not taken for a cut into the part). A bar's radii are kept
    /// as they are.
    pub fn lower_to(&mut self, start: &crate::adaptive::HeightMap) {
        for b in &mut self.bands {
            let Model::Heights(h) = b else { continue };
            for j in 0..h.ny {
                for i in 0..h.nx {
                    let (x, y) = h.centre(i, j);
                    let Some(z) = over_cell(start, x, y, h.dx, h.dy, f64::max) else { continue };
                    if let Some(v) = h.h.get_mut(j * h.nx + i)
                        && z < *v
                    {
                        *v = z.max(h.bottom);
                        h.tiles.mark(i, j);
                    }
                }
            }
        }
        self.retrack();
    }

    /// The stock an earlier setup left ([`SimState::lower_to`]) held up to the part: every cell
    /// of a height grid at least as high as the part's top over its centre (`floor`, from
    /// [`SimPlan::part_floor`]). That setup's stock is read from cells of its own, turned with the
    /// part, so where it cut down to the part its surface can fall inside the part by a share of
    /// a cell; the part's material is still there (that setup's own check measures what it cut
    /// into the part), so what this program cuts into the part is measured against the part,
    /// not against a carried surface inside it.
    pub fn hold_part(&mut self, floor: &PartFloor) {
        let mut raised = false;
        for b in &mut self.bands {
            let Model::Heights(h) = b else { continue };
            for j in 0..h.ny {
                for i in 0..h.nx {
                    let Some(&f) = floor.outer.get((h.j0 + j) * h.nx + i) else { continue };
                    if let Some(v) = h.h.get_mut(j * h.nx + i)
                        && f.is_finite()
                        && *v < f
                    {
                        *v = f.min(h.top);
                        h.tiles.mark(i, j);
                        raised = true;
                    }
                }
            }
        }
        if raised {
            self.retrack();
        }
    }

    /// The stock's material ending below at `under` (work Z, under each cell's centre): what an
    /// earlier setup, upside down from this one, cut from what is now the stock's underside (the
    /// lowest `under` has anywhere over the cell). A cell off `under` keeps its material down to
    /// the stock's bottom. The part left is measured over each cell's material only.
    pub fn raise_under(&mut self, under: &crate::adaptive::HeightMap) {
        for b in &mut self.bands {
            let Model::Heights(h) = b else { continue };
            let mut lows = Vec::with_capacity(h.nx * h.ny);
            for j in 0..h.ny {
                for i in 0..h.nx {
                    let (x, y) = h.centre(i, j);
                    lows.push(over_cell(under, x, y, h.dx, h.dy, f64::min).map_or(h.bottom, |z| z.max(h.bottom)));
                }
            }
            h.under = lows;
        }
        self.retrack();
    }

    /// Watches for cuts below `floor` (from [`SimPlan::part_floor`]) as the stock is cut: each
    /// step's [`Removed::gouge`].
    pub fn set_floor(&mut self, floor: &Arc<PartFloor>) {
        for b in &mut self.bands {
            b.set_floor(floor);
        }
    }

    /// How far it is: every step before the first number is cut, and the second's fraction of
    /// that step.
    pub fn at(&self) -> (usize, f64) {
        (self.next, self.frac)
    }

    /// What each step cut so far, in step order (the step under way included).
    pub fn removed(&self) -> &[Removed] {
        &self.removed
    }

    /// Cuts on to step `to`, and a fraction `fraction` of it (steps past the program's end are
    /// its end). A position before the state's own is refused: start again from an earlier state.
    pub fn advance(&mut self, plan: &SimPlan, to: usize, fraction: f64) -> Result<()> {
        if self.failed {
            return invalid("this simulation state stopped part way through a step: start again from an earlier one");
        }
        let n = plan.steps.len();
        let (mut to, mut fraction) = (to, if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 });
        if fraction >= 1.0 {
            to = to.saturating_add(1);
            fraction = 0.0;
        }
        if to >= n {
            to = n;
            fraction = 0.0;
        }
        if to < self.next || (to == self.next && fraction < self.frac) {
            return invalid("the stock cannot be cut back to an earlier move: start again from an earlier state");
        }
        let mut work: Vec<Work> = Vec::new();
        let (mut k, mut f0) = (self.next, self.frac);
        while k < to {
            work.push((k, f0, 1.0));
            k += 1;
            f0 = 0.0;
        }
        if k < n && fraction > f0 {
            work.push((k, f0, fraction));
        }
        if work.is_empty() {
            return Ok(());
        }
        // The side lines beside the bands, each step no higher than the height map holds material
        // round it before this advance (a line holds material only where the column over it does).
        let tops: Vec<f64> = if self.side.is_some() {
            work.iter().map(|&(k, _, _)| plan.steps.get(k).map_or(f64::INFINITY, |st| self.top_round(plan, st))).collect()
        } else {
            Vec::new()
        };
        let cut_sides = |side: &mut SideDexels| {
            work.iter()
                .zip(&tops)
                .all(|(&(k, f0, f1), &top)| plan.steps.get(k).is_none_or(|st| side.cut_step(st, &plan.machine, (f0, f1), plan.tol, top).is_ok()))
        };
        self.visits.store(0, Ordering::Relaxed);
        let threaded = plan.cost(&work) > THREADED_VISITS;
        let bands: Vec<&mut Model> = self.bands.iter_mut().collect();
        let (results, sides_cut) =
            beside(self.side.as_deref_mut(), threaded, cut_sides, || each_threaded(bands, threaded, |m| band_advance(m, plan, &work)));
        if !sides_cut {
            self.given_up = self.side.take();
            self.side_dropped = true;
        }
        if self.side.is_some() {
            // A view draws what the side lines hold over each tile: a tile they may have cut is
            // drawn again, whether its heights changed or not.
            for &(k, _, _) in &work {
                if let Some(cells) = plan.steps.get(k).and_then(|st| step_cells(&plan.grid, st)) {
                    self.touch_tiles(cells);
                }
            }
        }
        let results = match results {
            Ok(r) => r,
            Err(e) => {
                self.failed = true;
                return Err(e);
            }
        };
        for (w, &(k, _, _)) in work.iter().enumerate() {
            while self.removed.len() <= k {
                self.removed.push(Removed::default());
                self.probes.push(0);
            }
            for band in &results {
                if let Some(&(r, bits)) = band.get(w) {
                    if let Some(slot) = self.removed.get_mut(k) {
                        slot.add(r);
                    }
                    if let Some(p) = self.probes.get_mut(k) {
                        *p |= bits;
                    }
                }
            }
        }
        (self.next, self.frac) = (to, fraction);
        Ok(())
    }

    /// The band holding row (angle) `j`, and the row within it.
    pub(super) fn band_of(&self, j: usize) -> Option<(&Model, usize)> {
        let b = self.bands.partition_point(|m| m.span().0 <= j).checked_sub(1)?;
        let m = self.bands.get(b)?;
        let (j0, n) = m.span();
        (j < j0 + n).then_some((m, j - j0))
    }

    /// The height (radius) of cell `(i, j)` of the whole grid.
    pub fn value(&self, plan: &SimPlan, i: usize, j: usize) -> Option<f64> {
        let nx = plan.grid.columns();
        if i >= nx {
            return None;
        }
        let (m, jj) = self.band_of(j)?;
        m.values().get(jj * nx + i).copied()
    }

    /// The slot of the tool that cut cell `(i, j)` last ([`UNCUT`]: none).
    pub fn last_tool(&self, plan: &SimPlan, i: usize, j: usize) -> u8 {
        let nx = plan.grid.columns();
        if i >= nx {
            return UNCUT;
        }
        self.band_of(j).and_then(|(m, jj)| m.last().get(jj * nx + i).copied()).unwrap_or(UNCUT)
    }

    /// The version of tile `(ti, tj)`: it changes whenever a cell of the tile is cut.
    pub fn tile_version(&self, ti: usize, tj: usize) -> u32 {
        let Some((m, jj)) = self.band_of(tj * TILE) else { return 0 };
        let t = m.tiles();
        if ti >= t.tx {
            return 0;
        }
        t.version.get((jj / TILE) * t.tx + ti).copied().unwrap_or(0)
    }

    /// The highest of the cells `i0..=i1` by `j0..=j1` by whole tiles (at least their highest).
    pub fn max_in(&mut self, plan: &SimPlan, (i0, i1): (usize, usize), (j0, j1): (usize, usize)) -> f64 {
        let nx = plan.grid.columns();
        let mut m = f64::NEG_INFINITY;
        for b in &mut self.bands {
            let (b0, n) = b.span();
            let (lo, hi) = (j0.max(b0), j1.min(b0 + n.saturating_sub(1)));
            if n == 0 || lo > hi || i0 > i1 || i0 >= nx {
                continue;
            }
            let (values, tiles) = match b {
                Model::Heights(h) => (&h.h, &mut h.tiles),
                Model::Radii(r) => (&r.rho, &mut r.tiles),
            };
            m = m.max(tiles.max_in(values, nx, (i0, i1.min(nx - 1)), (lo - b0, hi - b0)));
        }
        m
    }

    /// Whether `p` (work coordinates) is inside the stock.
    pub fn contains(&self, p: Vec3) -> bool {
        self.bands.iter().find_map(|b| b.owns(p)).unwrap_or(false)
    }
}
