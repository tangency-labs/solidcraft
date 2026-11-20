//! Wi-Fi coverage: the multi-wall indoor model behind `inspect.rf_coverage` (#20161).
//!
//! The received signal at a point is
//!
//! `RSSI = EIRP − FSPL(1 m, f) − 10·n·log10(d) − Σ losses`
//!
//! - `FSPL(1 m, f) = 20·log10(f MHz) − 27.55` is the free-space loss over the first metre.
//! - `n` is the path-loss exponent (2 in free space; about 2.2 indoors with furniture).
//! - `d` is the 3D distance in metres from the emitter to the receiver, at least
//!   [`MIN_DISTANCE_M`].
//! - Each wall the straight path crosses adds its material's loss once: a run of one material
//!   along the path is one crossing, however many bodies it is made of
//!   ([`WallMap::path_loss`]).
//!
//! The walls are the regions a horizontal plane cuts out of the model's bodies, read exactly and
//! followed to within [`FLATTEN_MM`]. The path is the straight line between the emitter and the
//! receiver seen from above: the model has one slice, so a wall counts the same at every height.
//! There are no reflections, no antenna patterns, and only the downlink is predicted.

use solidcraft_doc::rf::{Band, RfLoss};

/// How closely the walls' curved outlines are followed, in mm.
pub const FLATTEN_MM: f64 = 1.0;
/// The shortest distance the path loss is worked out for, in metres: closer than this the model
/// would give more than the emitter sends.
pub const MIN_DISTANCE_M: f64 = 0.3;
/// Runs of one material along a path closer than this (mm) are one run: two boxes that meet in
/// a wall make one wall.
pub const MERGE_GAP_MM: f64 = 2.0;
/// The most grid cells one analysis works out.
pub const MAX_CELLS: usize = 4_000_000;
/// The most wall edges one analysis keeps.
pub const MAX_EDGES: usize = 2_000_000;
/// The signal levels the statistics report: −60 dBm (fast) and −67 dBm (video calls).
pub const GOOD_DBM: f64 = -60.0;
pub const FAIR_DBM: f64 = -67.0;

/// The free-space loss over the first metre at `band`'s frequency, in dB.
pub fn fspl_1m(band: Band) -> f64 {
    20.0 * band.mhz().log10() - 27.55
}

/// The received signal in dBm `distance_m` from an emitter of `eirp_dbm`, with `walls_db` of wall
/// losses on the way.
pub fn rssi(eirp_dbm: f64, band: Band, exponent: f64, distance_m: f64, walls_db: f64) -> f64 {
    eirp_dbm - fspl_1m(band) - 10.0 * exponent * distance_m.max(MIN_DISTANCE_M).log10() - walls_db
}

/// One region a slice cuts out of a body: closed rings (an outline and its holes, inside by the
/// even-odd rule) and the material it is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct WallRegion {
    /// Closed rings in world x, y (mm), the last point not repeated.
    pub rings: Vec<Vec<[f64; 2]>>,
    /// Index into [`WallMap::materials`].
    pub material: usize,
    /// The body it was cut from.
    pub body: String,
    /// The box round it: [min x, min y, max x, max y].
    pub bounds: [f64; 4],
}

impl WallRegion {
    pub fn new(rings: Vec<Vec<[f64; 2]>>, material: usize, body: String) -> Option<WallRegion> {
        let rings: Vec<Vec<[f64; 2]>> =
            rings.into_iter().filter(|r| r.len() >= 3 && r.iter().all(|p| p.iter().all(|c| c.is_finite()))).collect();
        let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for p in rings.iter().flatten() {
            bounds = [bounds[0].min(p[0]), bounds[1].min(p[1]), bounds[2].max(p[0]), bounds[3].max(p[1])];
        }
        (!rings.is_empty()).then_some(WallRegion { rings, material, body, bounds })
    }

    fn edges(&self) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
        self.rings.iter().flat_map(|r| (0..r.len()).filter_map(move |i| Some((*r.get(i)?, *r.get((i + 1) % r.len())?))))
    }

    /// Whether `p` is inside, by the even-odd rule.
    pub fn contains(&self, p: [f64; 2]) -> bool {
        if p[0] < self.bounds[0] || p[0] > self.bounds[2] || p[1] < self.bounds[1] || p[1] > self.bounds[3] {
            return false;
        }
        let mut inside = false;
        for (a, b) in self.edges() {
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if p[0] < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// The stretches of the segment `a`→`b` inside the region, as parameters 0…1 along it.
    fn inside_along(&self, a: [f64; 2], b: [f64; 2], out: &mut Vec<(f64, f64)>) {
        let d = [b[0] - a[0], b[1] - a[1]];
        let mut hits: Vec<f64> = Vec::new();
        for (p, q) in self.edges() {
            let e = [q[0] - p[0], q[1] - p[1]];
            let den = d[0] * e[1] - d[1] * e[0];
            if den == 0.0 {
                continue;
            }
            // Which side of the path each end of the edge lies on; an end on the path counts as
            // the positive side, so a path through a corner crosses once or not at all.
            let side = |v: [f64; 2]| d[0] * (v[1] - a[1]) - d[1] * (v[0] - a[0]) > 0.0;
            if side(p) == side(q) {
                continue;
            }
            let t = ((p[0] - a[0]) * e[1] - (p[1] - a[1]) * e[0]) / den;
            if (0.0..=1.0).contains(&t) {
                hits.push(t);
            }
        }
        hits.sort_by(f64::total_cmp);
        let mut start = if self.contains(a) { Some(0.0) } else { None };
        for t in hits {
            start = match start {
                Some(s) => {
                    if t > s {
                        out.push((s, t));
                    }
                    None
                }
                None => Some(t),
            };
        }
        if let Some(s) = start
            && s < 1.0
        {
            out.push((s, 1.0));
        }
    }
}

/// The walls one slice of the model makes, with their materials.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WallMap {
    pub regions: Vec<WallRegion>,
    /// The losses each region's `material` indexes, with a name for replies.
    pub materials: Vec<(String, RfLoss)>,
    /// Square buckets of [`BUCKET_MM`] over the regions' boxes, each listing the regions that
    /// overlap it, for finding the regions near a path quickly.
    buckets: Buckets,
}

/// The size of a [`WallMap`]'s buckets, in mm.
const BUCKET_MM: f64 = 500.0;

#[derive(Clone, Debug, Default, PartialEq)]
struct Buckets {
    origin: [f64; 2],
    columns: usize,
    rows: usize,
    cells: Vec<Vec<u32>>,
}

impl WallMap {
    /// The map of `regions` made of `materials`. Regions naming a material that isn't there are
    /// left out.
    pub fn new(regions: Vec<WallRegion>, materials: Vec<(String, RfLoss)>) -> WallMap {
        let regions: Vec<WallRegion> = regions.into_iter().filter(|r| r.material < materials.len()).collect();
        let mut map = WallMap { regions, materials, buckets: Buckets::default() };
        map.index();
        map
    }

    fn index(&mut self) {
        let Some(bounds) = self.bounds() else { return };
        let columns = (((bounds[2] - bounds[0]) / BUCKET_MM).ceil() as usize).clamp(1, 4096);
        let rows = (((bounds[3] - bounds[1]) / BUCKET_MM).ceil() as usize).clamp(1, 4096);
        let mut cells = vec![Vec::new(); columns * rows];
        let at = |v: f64, o: f64, n: usize| (((v - o) / BUCKET_MM).floor().max(0.0) as usize).min(n - 1);
        for (i, r) in self.regions.iter().enumerate() {
            let (c0, c1) = (at(r.bounds[0], bounds[0], columns), at(r.bounds[2], bounds[0], columns));
            let (r0, r1) = (at(r.bounds[1], bounds[1], rows), at(r.bounds[3], bounds[1], rows));
            for row in r0..=r1 {
                for column in c0..=c1 {
                    if let Some(cell) = cells.get_mut(row * columns + column) {
                        cell.push(i as u32);
                    }
                }
            }
        }
        self.buckets = Buckets { origin: [bounds[0], bounds[1]], columns, rows, cells };
    }

    /// The box round every region: [min x, min y, max x, max y].
    pub fn bounds(&self) -> Option<[f64; 4]> {
        self.regions.iter().map(|r| r.bounds).reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
    }

    /// The regions whose boxes the segment `a`→`b`'s buckets touch, each once.
    fn near(&self, a: [f64; 2], b: [f64; 2], seen: &mut Vec<u32>, stamp: u32, out: &mut Vec<u32>) {
        out.clear();
        let g = &self.buckets;
        if g.columns == 0 || g.rows == 0 {
            return;
        }
        // Walk the buckets along the part of the segment over the grid.
        let to_cell = |p: [f64; 2]| [(p[0] - g.origin[0]) / BUCKET_MM, (p[1] - g.origin[1]) / BUCKET_MM];
        let Some((ca, cb)) = clip(to_cell(a), to_cell(b), [g.columns as f64, g.rows as f64]) else { return };
        let steps = ((cb[0] - ca[0]).abs().max((cb[1] - ca[1]).abs()) * 2.0).ceil().clamp(1.0, 1.0e5) as usize;
        let mut last = usize::MAX;
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let x = ca[0] + (cb[0] - ca[0]) * t;
            let y = ca[1] + (cb[1] - ca[1]) * t;
            // A step of half a bucket can pass a bucket's corner: look at the neighbours too.
            for (dx, dy) in [(0.0, 0.0), (0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)] {
                let (cx, cy) = (x + dx, y + dy);
                if cx < 0.0 || cy < 0.0 || cx >= g.columns as f64 || cy >= g.rows as f64 {
                    continue;
                }
                let cell = cy as usize * g.columns + cx as usize;
                if cell == last && dx == 0.0 && dy == 0.0 {
                    continue;
                }
                if dx == 0.0 && dy == 0.0 {
                    last = cell;
                }
                for &r in g.cells.get(cell).map(Vec::as_slice).unwrap_or(&[]) {
                    if let Some(s) = seen.get_mut(r as usize)
                        && *s != stamp
                    {
                        *s = stamp;
                        out.push(r);
                    }
                }
            }
        }
    }

    /// The wall losses in `band` along the straight path `a`→`b` (world x, y in mm): the loss of
    /// each run of one material the path enters. Where materials overlap, the one with the larger
    /// loss counts. A run the path starts in isn't entered, so it adds nothing.
    pub fn path_loss(&self, a: [f64; 2], b: [f64; 2], band: Band, scratch: &mut Scratch) -> f64 {
        scratch.stamp = scratch.stamp.wrapping_add(1);
        if scratch.stamp == 0 {
            scratch.seen.iter_mut().for_each(|s| *s = 0);
            scratch.stamp = 1;
        }
        scratch.seen.resize(self.regions.len(), 0);
        let mut near = std::mem::take(&mut scratch.near);
        self.near(a, b, &mut scratch.seen, scratch.stamp, &mut near);
        let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let gap = if length > 0.0 { MERGE_GAP_MM / length } else { 0.0 };
        let seg = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
        scratch.events.clear();
        for &i in &near {
            let Some(region) = self.regions.get(i as usize) else { continue };
            let r = region.bounds;
            if r[0] > seg[2] || r[2] < seg[0] || r[1] > seg[3] || r[3] < seg[1] {
                continue;
            }
            scratch.spans.clear();
            region.inside_along(a, b, &mut scratch.spans);
            let loss = self.materials.get(region.material).map(|(_, l)| l.at(band)).unwrap_or(0.0);
            for &(t0, t1) in &scratch.spans {
                scratch.events.push(Span { t0, t1, material: region.material, loss });
            }
        }
        scratch.near = near;
        if scratch.events.is_empty() {
            return 0.0;
        }
        runs_loss(&mut scratch.events, gap)
    }
}

/// The part of the segment `a`→`b` inside the box from 0 to `size` (Liang–Barsky); `None` when
/// it misses the box.
fn clip(a: [f64; 2], b: [f64; 2], size: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for axis in 0..2 {
        let (p, lo, hi) = (*d.get(axis)?, -*a.get(axis)?, *size.get(axis)? - *a.get(axis)?);
        if p == 0.0 {
            if lo > 0.0 || hi < 0.0 {
                return None;
            }
            continue;
        }
        let (ta, tb) = (lo / p, hi / p);
        let (near, far) = if ta < tb { (ta, tb) } else { (tb, ta) };
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return None;
        }
    }
    let at = |t: f64| [a[0] + d[0] * t, a[1] + d[1] * t];
    Some((at(t0), at(t1)))
}

/// Reused buffers for [`WallMap::path_loss`].
#[derive(Default)]
pub struct Scratch {
    seen: Vec<u32>,
    stamp: u32,
    near: Vec<u32>,
    spans: Vec<(f64, f64)>,
    events: Vec<Span>,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    t0: f64,
    t1: f64,
    material: usize,
    loss: f64,
}

/// The loss of the runs `spans` make along a path: split the path where any span starts or ends,
/// give each piece the material with the largest loss there, join pieces of one material less
/// than `gap` apart, and add each run's loss once. A run starting at the path's start adds nothing.
fn runs_loss(spans: &mut [Span], gap: f64) -> f64 {
    let mut cuts: Vec<f64> = spans.iter().flat_map(|s| [s.t0, s.t1]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut total = 0.0;
    // The material of the last run and where it ended.
    let mut last: Option<(usize, f64)> = None;
    for w in cuts.windows(2) {
        let (Some(&t0), Some(&t1)) = (w.first(), w.get(1)) else { continue };
        let mid = 0.5 * (t0 + t1);
        let top = spans.iter().filter(|s| s.t0 <= mid && mid <= s.t1).max_by(|a, b| a.loss.total_cmp(&b.loss).then(b.material.cmp(&a.material)));
        let Some(top) = top else { continue };
        match last {
            Some((m, end)) if m == top.material && t0 - end <= gap => {}
            _ => {
                if t0 > 0.0 {
                    total += top.loss;
                }
            }
        }
        last = Some((top.material, t1));
    }
    total
}

/// An emitter: where it is and what it sends.
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    pub name: String,
    /// World x, y (mm).
    pub at: [f64; 2],
    /// Its height (mm, world z).
    pub height: f64,
    /// EIRP in dBm at 2.4, 5 and 6 GHz; `None` where it has no radio.
    pub eirp_dbm: [Option<f64>; 3],
}

impl Emitter {
    pub fn eirp(&self, band: Band) -> Option<f64> {
        self.eirp_dbm.get(band.index()).copied().flatten()
    }
}

/// The model's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    pub exponent: f64,
    /// The receiver's height (mm, world z).
    pub client_height: f64,
}

/// The best signal any of `emitters` gives at `p` in `band`, and which emitter gives it; `None`
/// when no emitter has a radio in `band`.
pub fn best_at(walls: &WallMap, emitters: &[Emitter], model: &Model, band: Band, p: [f64; 2], scratch: &mut Scratch) -> Option<(f64, usize)> {
    let mut best: Option<(f64, usize)> = None;
    for (i, e) in emitters.iter().enumerate() {
        let Some(eirp) = e.eirp(band) else { continue };
        let dz = e.height - model.client_height;
        let d = ((p[0] - e.at[0]).powi(2) + (p[1] - e.at[1]).powi(2) + dz * dz).sqrt() / 1000.0;
        let v = rssi(eirp, band, model.exponent, d, walls.path_loss(e.at, p, band, scratch));
        if best.is_none_or(|(b, _)| v > b) {
            best = Some((v, i));
        }
    }
    best
}

/// A grid over the floor: cell centres at `origin + (i + ½)·step`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub origin: [f64; 2],
    pub step: f64,
    pub columns: usize,
    pub rows: usize,
}

impl Grid {
    /// The grid of `step` covering `bounds` ([min x, min y, max x, max y]); `None` when it would
    /// have more than [`MAX_CELLS`] cells or none.
    pub fn covering(bounds: [f64; 4], step: f64) -> Option<Grid> {
        if !(step.is_finite() && step > 0.0) || bounds.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let columns = ((bounds[2] - bounds[0]) / step).ceil().max(1.0);
        let rows = ((bounds[3] - bounds[1]) / step).ceil().max(1.0);
        if columns * rows > MAX_CELLS as f64 {
            return None;
        }
        Some(Grid { origin: [bounds[0], bounds[1]], step, columns: columns as usize, rows: rows as usize })
    }

    pub fn len(&self) -> usize {
        self.columns * self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn centre(&self, column: usize, row: usize) -> [f64; 2] {
        [self.origin[0] + (column as f64 + 0.5) * self.step, self.origin[1] + (row as f64 + 0.5) * self.step]
    }
}

/// What a cell of the grid is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Outside the floor region: not counted.
    Outside,
    /// Inside a wall: not counted.
    Wall,
    /// On the floor: counted.
    Floor,
}

/// Which cells are walls: a cell whose centre lies inside a region.
pub fn wall_cells(walls: &WallMap, grid: &Grid) -> Vec<bool> {
    let mut out = vec![false; grid.len()];
    for r in &walls.regions {
        let c0 = (((r.bounds[0] - grid.origin[0]) / grid.step).floor().max(0.0) as usize).min(grid.columns);
        let c1 = (((r.bounds[2] - grid.origin[0]) / grid.step).ceil().max(0.0) as usize).min(grid.columns);
        let r0 = (((r.bounds[1] - grid.origin[1]) / grid.step).floor().max(0.0) as usize).min(grid.rows);
        let r1 = (((r.bounds[3] - grid.origin[1]) / grid.step).ceil().max(0.0) as usize).min(grid.rows);
        for row in r0..r1 {
            for column in c0..c1 {
                if let Some(cell) = out.get_mut(row * grid.columns + column)
                    && !*cell
                    && r.contains(grid.centre(column, row))
                {
                    *cell = true;
                }
            }
        }
    }
    out
}

/// The cells a wall edge passes through or touches, so a thin pane that misses every cell centre
/// still closes a room.
fn edge_cells(walls: &WallMap, grid: &Grid) -> Vec<bool> {
    let mut out = vec![false; grid.len()];
    for r in &walls.regions {
        for (a, b) in r.edges() {
            let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let n = ((length / (grid.step * 0.25)).ceil() as usize).clamp(1, 1_000_000);
            for k in 0..=n {
                let t = k as f64 / n as f64;
                let x = (a[0] + (b[0] - a[0]) * t - grid.origin[0]) / grid.step;
                let y = (a[1] + (b[1] - a[1]) * t - grid.origin[1]) / grid.step;
                if x >= 0.0 && y >= 0.0 && (x as usize) < grid.columns && (y as usize) < grid.rows {
                    if let Some(c) = out.get_mut(y as usize * grid.columns + x as usize) {
                        *c = true;
                    }
                }
            }
        }
    }
    out
}

/// The floor the walls enclose: the cells a flood from the grid's edge can't reach without
/// crossing a wall. Empty when the walls enclose nothing (an open plan, a door left out).
pub fn enclosed(walls: &WallMap, grid: &Grid, wall: &[bool]) -> Vec<bool> {
    let blocked: Vec<bool> = edge_cells(walls, grid).iter().zip(wall).map(|(e, w)| *e || *w).collect();
    let mut reached = vec![false; grid.len()];
    let mut stack: Vec<usize> = Vec::new();
    let (w, h) = (grid.columns, grid.rows);
    for column in 0..w {
        stack.push(column);
        stack.push((h - 1) * w + column);
    }
    for row in 0..h {
        stack.push(row * w);
        stack.push(row * w + w - 1);
    }
    while let Some(i) = stack.pop() {
        if blocked.get(i).copied().unwrap_or(true) || reached.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(r) = reached.get_mut(i) {
            *r = true;
        }
        let (column, row) = (i % w, i / w);
        if column > 0 {
            stack.push(i - 1);
        }
        if column + 1 < w {
            stack.push(i + 1);
        }
        if row > 0 {
            stack.push(i - w);
        }
        if row + 1 < h {
            stack.push(i + w);
        }
    }
    (0..grid.len()).map(|i| !blocked.get(i).copied().unwrap_or(true) && !reached.get(i).copied().unwrap_or(true)).collect()
}

/// The coverage figures for one band.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub cells: usize,
    /// The share of the floor at [`GOOD_DBM`] or better, and at [`FAIR_DBM`] or better (0–1).
    pub good: f64,
    pub fair: f64,
    /// The 5th percentile, the median, the weakest and the strongest signal (dBm).
    pub p5: f64,
    pub median: f64,
    pub min: f64,
    pub max: f64,
}

/// The figures for `values` over the cells `cells` marks [`Cell::Floor`].
pub fn stats(values: &[f32], cells: &[Cell]) -> Option<Stats> {
    let mut v: Vec<f64> =
        values.iter().zip(cells).filter(|(v, c)| **c == Cell::Floor && v.is_finite()).map(|(v, _)| f64::from(*v)).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len() as f64;
    let at = |q: f64| v.get(((q * (n - 1.0)).round() as usize).min(v.len() - 1)).copied().unwrap_or(f64::NAN);
    Some(Stats {
        cells: v.len(),
        good: v.iter().filter(|x| **x >= GOOD_DBM).count() as f64 / n,
        fair: v.iter().filter(|x| **x >= FAIR_DBM).count() as f64 / n,
        p5: at(0.05),
        median: at(0.5),
        min: v.first().copied().unwrap_or(f64::NAN),
        max: v.last().copied().unwrap_or(f64::NAN),
    })
}

/// The signal over `grid` in `band`: the best emitter's RSSI at each cell's centre that `cells`
/// marks floor; NaN elsewhere.
pub fn coverage(walls: &WallMap, emitters: &[Emitter], model: &Model, band: Band, grid: &Grid, cells: &[Cell]) -> Vec<f32> {
    let mut scratch = Scratch::default();
    let mut out = vec![f32::NAN; grid.len()];
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            let i = row * grid.columns + column;
            if cells.get(i) != Some(&Cell::Floor) {
                continue;
            }
            if let (Some(slot), Some((v, _))) = (out.get_mut(i), best_at(walls, emitters, model, band, grid.centre(column, row), &mut scratch)) {
                *slot = v as f32;
            }
        }
    }
    out
}

/// The colour scale of the heatmap and its legend: from each threshold (dBm) up, a colour.
pub const SCALE: [(f64, [u8; 3]); 8] = [
    (-50.0, [22, 120, 86]),
    (-55.0, [42, 157, 96]),
    (-60.0, [124, 191, 82]),
    (-67.0, [214, 200, 55]),
    (-70.0, [236, 160, 50]),
    (-75.0, [226, 104, 52]),
    (-80.0, [190, 58, 58]),
    (f64::NEG_INFINITY, [120, 120, 130]),
];

/// The colour of a signal of `dbm`.
pub fn colour(dbm: f64) -> [u8; 3] {
    SCALE.iter().find(|(t, _)| dbm >= *t).map(|(_, c)| *c).unwrap_or([120, 120, 130])
}

/// The heatmap as straight RGBA8, one pixel a cell, the first row the grid's lowest (smallest y):
/// floor cells in their signal's colour, walls dark, the rest clear.
pub fn rgba(values: &[f32], cells: &[Cell], grid: &Grid) -> Vec<u8> {
    let mut out = vec![0u8; grid.len() * 4];
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let (cell, v) = (cells.get(i).copied().unwrap_or(Cell::Outside), values.get(i).copied().unwrap_or(f32::NAN));
        let rgba = match cell {
            Cell::Wall => [40, 46, 52, 235],
            Cell::Floor if v.is_finite() => {
                let [r, g, b] = colour(f64::from(v));
                [r, g, b, 210]
            }
            _ => [0, 0, 0, 0],
        };
        px.copy_from_slice(&rgba);
    }
    out
}

#[cfg(test)]
#[path = "rf_coverage_tests.rs"]
mod tests;
