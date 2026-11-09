//! Tri-dexel stock: three grids of rays (along X, Y and Z) over a box. Each ray keeps the sorted
//! material intervals left after the cuts so far, and every interval end remembers the move that
//! made it (tag 0 is the original stock face, tag n is move n-1 plus one).
//!
//! Rays are grouped in tiles of 32 by 32, shared copy-on-write, so a checkpoint costs only the
//! tiles a later move changed. A cut that fails (too many segments) leaves the stock untouched.

use std::sync::Arc;

use solidcore_record::Vec3;

use crate::SimError;
use crate::sweep::Sweep;

/// Rays per tile along each of the two perpendicular axes.
pub const TILE: usize = 32;
/// A ray keeps at most this many intervals; more is an error, never a panic.
pub const MAX_SEGS_PER_RAY: usize = 256;
/// Memory per untouched ray, the estimate the budget is checked against (a segment and a start).
pub const BYTES_PER_RAY: u64 = 20;
const MAX_ABS_COORD: f64 = 1e5;
const MAX_CELLS_PER_AXIS: f64 = (1u64 << 20) as f64;

/// One material interval on a ray, `[lo, hi]` along the ray's axis, with the tags of the moves
/// (or the stock face, 0) that made each end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub lo: f32,
    pub hi: f32,
    pub lo_tag: u32,
    pub hi_tag: u32,
}

/// Rays of one tile in a compressed-rows layout: ray `r` owns `segs[start[r]..start[r + 1]]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub start: Vec<u32>,
    pub segs: Vec<Seg>,
}

impl Tile {
    fn uncut(rays: usize, lo: f32, hi: f32) -> Tile {
        Tile { start: (0..=rays).map(|r| r as u32).collect(), segs: vec![Seg { lo, hi, lo_tag: 0, hi_tag: 0 }; rays] }
    }

    fn rays(&self) -> usize {
        self.start.len().saturating_sub(1)
    }

    fn ray(&self, r: usize) -> &[Seg] {
        match (self.start.get(r), self.start.get(r + 1)) {
            (Some(&a), Some(&b)) => self.segs.get(a as usize..b as usize).unwrap_or(&[]),
            _ => &[],
        }
    }
}

#[derive(Clone, Debug)]
struct Grid {
    /// Rays along the two perpendicular axes `u = (axis+1)%3` and `v = (axis+2)%3`, which the
    /// grid is indexed by. The grid's own axis is the index of the grid in the stock.
    nu: usize,
    nv: usize,
    /// Tiles along `u`; tile `(tu, tv)` is `tiles[tv * tu_count + tu]`.
    tu: usize,
    tiles: Vec<Arc<Tile>>,
    /// Sum of the lengths of every ray's material, in stored units. Kept up to date by `apply`, so
    /// the volume is O(1) to read.
    len: f64,
}

/// A box of material with three tri-dexel grids. Cell `i` along an axis spans `min + i * cell`
/// to the next; its ray sits at the middle of that cell (the lattice node).
#[derive(Debug)]
pub struct Stock {
    min: [f64; 3],
    max: [f64; 3],
    cell: [f64; 3],
    n: [usize; 3],
    grids: [Grid; 3],
    /// Bytes the grids take (tiles counted as if none were shared), and the budget they must fit.
    used: u64,
    budget: u64,
    /// Chunks whose surface may have changed since the last [`Stock::clear_dirty`]: each once, sorted.
    dirty: std::collections::BTreeSet<[usize; 3]>,
    /// Every chunk a cut has reached since this stock was built. A chunk not in here is still the
    /// uncut box, so it is not meshed (see `mesh::chunk_mesh`).
    touched: std::collections::BTreeSet<[usize; 3]>,
    /// The changes of a multi-step move so far, while a journal is open (see `begin_journal`).
    journal: Option<Journal>,
}

/// What a multi-step move has changed in a stock, so `undo_journal` can take it back: each tile it
/// replaced (the first time only), the sums before it, the chunks it newly reached, and every chunk it
/// changed.
#[derive(Clone, Debug)]
struct Journal {
    tiles: Vec<(usize, usize, Arc<Tile>)>,
    replaced: std::collections::BTreeSet<(usize, usize)>,
    len: [f64; 3],
    used: u64,
    reached: Vec<[usize; 3]>,
    changed: std::collections::BTreeSet<[usize; 3]>,
}

/// Bytes of a tile's contents: a segment is 16 bytes, a start 4.
fn tile_bytes(tile: &Tile) -> u64 {
    (tile.segs.len() as u64) * 16 + (tile.start.len() as u64) * 4
}

/// A clone leaves the open journal behind: a clone taken mid-move (a checkpoint, the UI mesher) must not
/// keep the undo tiles of that move alive, and only the original can undo it.
impl Clone for Stock {
    fn clone(&self) -> Stock {
        Stock {
            min: self.min,
            max: self.max,
            cell: self.cell,
            n: self.n,
            grids: self.grids.clone(),
            used: self.used,
            budget: self.budget,
            dirty: self.dirty.clone(),
            touched: self.touched.clone(),
            journal: None,
        }
    }
}

impl Stock {
    /// A solid box `min..max` with rays about `h` apart. Refused (not panicked) when `h` or the box
    /// is bad, or when the memory estimate is over `budget_bytes`.
    pub fn new_box(min: Vec3, max: Vec3, h: f64, budget_bytes: u64) -> Result<Stock, SimError> {
        let layout = BoxLayout::new(min, max, h)?;
        if layout.need > budget_bytes {
            return Err(SimError::Budget { need: layout.need, budget: budget_bytes });
        }
        let (lo, hi, n) = (layout.lo, layout.hi, layout.n);
        let grids = [build_grid(0, &n, &lo, &hi), build_grid(1, &n, &lo, &hi), build_grid(2, &n, &lo, &hi)];
        Ok(Stock {
            min: lo,
            max: hi,
            cell: layout.cell,
            n,
            grids,
            used: layout.need,
            budget: budget_bytes,
            dirty: std::collections::BTreeSet::new(),
            touched: std::collections::BTreeSet::new(),
            journal: None,
        })
    }

    /// Starts a journal: from here on, what `apply` changes is kept, so `undo_journal` can take it back.
    /// A move of several steps opens one, and ends it with `commit_journal` or `undo_journal`.
    pub fn begin_journal(&mut self) {
        let len = [self.grids[0].len, self.grids[1].len, self.grids[2].len];
        self.journal = Some(Journal {
            tiles: Vec::new(),
            replaced: std::collections::BTreeSet::new(),
            len,
            used: self.used,
            reached: Vec::new(),
            changed: std::collections::BTreeSet::new(),
        });
    }

    /// Keeps the changes since `begin_journal` and drops the journal.
    pub fn commit_journal(&mut self) {
        self.journal = None;
    }

    /// Takes back every change since `begin_journal`: the tiles, the sums, and the chunks reached.
    /// The chunks changed are dirty, so the caller redraws them. Does nothing without a journal.
    pub fn undo_journal(&mut self) {
        let Some(j) = self.journal.take() else { return };
        for (a, tidx, tile) in j.tiles {
            if let Some(slot) = self.grids.get_mut(a).and_then(|g| g.tiles.get_mut(tidx)) {
                *slot = tile;
            }
        }
        for (grid, len) in self.grids.iter_mut().zip(j.len) {
            grid.len = len;
        }
        self.used = j.used;
        for c in &j.reached {
            self.touched.remove(c);
        }
        self.dirty.extend(j.changed);
    }

    /// Bytes of the tiles this stock holds that `base` does not share (the same tile, by pointer): what
    /// a checkpoint adds on top of `base`. A stock of another layout shares nothing.
    pub fn unshared_bytes(&self, base: &Stock) -> u64 {
        let mut total = 0u64;
        for (g, grid) in self.grids.iter().enumerate() {
            for (i, tile) in grid.tiles.iter().enumerate() {
                let shared = base.grids.get(g).and_then(|bg| bg.tiles.get(i)).is_some_and(|bt| Arc::ptr_eq(bt, tile));
                if !shared {
                    total = total.saturating_add(tile_bytes(tile));
                }
            }
        }
        total
    }

    /// Bytes a box of `min..max` at ray spacing `h` takes, before any cut: what [`Stock::new_box`]
    /// checks against its budget, so a caller can pick `h` without building the stock.
    pub fn box_bytes(min: Vec3, max: Vec3, h: f64) -> Result<u64, SimError> {
        BoxLayout::new(min, max, h).map(|l| l.need)
    }

    /// Cuts the stock with a sweep. Only rays the sweep can reach are visited. `tag` is recorded on
    /// the new ends (use move index plus one). On error the stock is unchanged.
    pub fn apply(&mut self, sweep: &dyn Sweep, tag: u32) -> Result<(), SimError> {
        let b = sweep.bounds();
        if b.is_empty() {
            return Ok(());
        }
        let bmin: [f64; 3] = b.min.into();
        let bmax: [f64; 3] = b.max.into();
        if !(bmin.iter().chain(bmax.iter()).all(|x| x.is_finite())) {
            return Err(SimError::BadInput("sweep bounds are not finite".into()));
        }
        let mut staged: Vec<(usize, usize, Tile)> = Vec::new();
        let mut out: Vec<(f64, f64)> = Vec::new();
        for a in 0..3 {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let Some((iu0, iu1)) = self.index_range(u, bmin[u], bmax[u]) else { continue };
            let Some((iv0, iv1)) = self.index_range(v, bmin[v], bmax[v]) else { continue };
            for tv in (iv0 / TILE)..=(iv1 / TILE) {
                for tu in (iu0 / TILE)..=(iu1 / TILE) {
                    let grid = &self.grids[a];
                    let tidx = tv * grid.tu + tu;
                    let Some(tile) = grid.tiles.get(tidx) else { continue };
                    let w = TILE.min(grid.nu - tu * TILE);
                    let h = TILE.min(grid.nv - tv * TILE);
                    let mut cuts: Vec<(usize, Vec<(f64, f64)>)> = Vec::new();
                    for lv in 0..h {
                        for lu in 0..w {
                            let (iu, iv) = (tu * TILE + lu, tv * TILE + lv);
                            if iu < iu0 || iu > iu1 || iv < iv0 || iv > iv1 {
                                continue;
                            }
                            let mut o = [0.0f64; 3];
                            let mut d = [0.0f64; 3];
                            if let Some(x) = o.get_mut(u) {
                                *x = self.pos(u, iu as i64);
                            }
                            if let Some(x) = o.get_mut(v) {
                                *x = self.pos(v, iv as i64);
                            }
                            if let Some(x) = d.get_mut(a) {
                                *x = 1.0;
                            }
                            out.clear();
                            sweep.ray(Vec3::from(o), Vec3::from(d), (self.min[a], self.max[a]), &mut out);
                            // Stored coordinates are relative to the stock's minimum on each axis.
                            let base = self.min[a];
                            // A cut that is empty once stored as f32 is skipped: it would split nothing.
                            let kept: Vec<(f64, f64)> = out
                                .iter()
                                .copied()
                                .filter(|(x, y)| x.is_finite() && y.is_finite())
                                .map(|(x, y)| (x - base, y - base))
                                .filter(|(x, y)| (*y as f32) > (*x as f32))
                                .collect();
                            if !kept.is_empty() {
                                cuts.push((lv * w + lu, kept));
                            }
                        }
                    }
                    if cuts.is_empty() {
                        continue;
                    }
                    staged.push((a, tidx, rebuild(tile, &cuts, tag)?));
                }
            }
        }
        // The byte total must still fit the budget; checked before anything is committed.
        let mut added = 0u64;
        let mut removed = 0u64;
        let mut len_change = [0.0f64; 3];
        for (a, tidx, tile) in &staged {
            added = added.saturating_add(tile_bytes(tile));
            len_change[*a] += tile_len(tile);
            if let Some(old) = self.grids[*a].tiles.get(*tidx) {
                removed = removed.saturating_add(tile_bytes(old));
                len_change[*a] -= tile_len(old);
            }
        }
        let used = self.used.saturating_add(added).saturating_sub(removed);
        if used > self.budget {
            return Err(SimError::Budget { need: used, budget: self.budget });
        }
        for (a, tidx, tile) in staged {
            if let Some(slot) = self.grids[a].tiles.get_mut(tidx) {
                if let Some(j) = self.journal.as_mut()
                    && j.replaced.insert((a, tidx))
                {
                    j.tiles.push((a, tidx, Arc::clone(slot)));
                }
                *slot = Arc::new(tile);
            }
        }
        for (grid, change) in self.grids.iter_mut().zip(len_change) {
            grid.len += change;
        }
        self.used = used;
        // The chunks that hold the nodes this sweep's box covers are dirty (see mesh::chunks_for_nodes).
        let counts = crate::mesh::chunk_counts(&*self);
        if let Some((lo, hi)) = self.node_box(bmin, bmax) {
            let chunks = crate::mesh::chunks_for_nodes(lo, hi, counts);
            for c in chunks {
                let fresh = self.touched.insert(c);
                self.dirty.insert(c);
                if let Some(j) = self.journal.as_mut() {
                    if fresh {
                        j.reached.push(c);
                    }
                    j.changed.insert(c);
                }
            }
        }
        Ok(())
    }

    /// Cuts several pieces of one move under one tag, all or nothing: on an error the stock is as it
    /// was before the call. One piece is atomic already; more are applied to a copy, which replaces
    /// the stock only once every piece has gone through.
    pub fn apply_all(&mut self, pieces: &[&dyn Sweep], tag: u32) -> Result<(), SimError> {
        if pieces.len() <= 1 {
            for p in pieces {
                self.apply(*p, tag)?;
            }
            return Ok(());
        }
        // The trial replaces self, so it takes the open journal (a clone drops it). On an error the
        // journal goes back: the trial's extra entries hold the values self already has.
        let mut trial = self.clone();
        trial.journal = self.journal.take();
        for p in pieces {
            if let Err(e) = trial.apply(*p, tag) {
                self.journal = trial.journal.take();
                return Err(e);
            }
        }
        *self = trial;
        Ok(())
    }

    /// The chunks marked dirty since the last [`Stock::clear_dirty`], each once.
    pub fn dirty_chunks(&self) -> Vec<[usize; 3]> {
        self.dirty.iter().copied().collect()
    }

    /// Forgets the dirty chunks (after the caller has remeshed what they touch).
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// The inclusive node range, per axis, of the world box `lo..=hi`, clamped to the lattice's
    /// `-1..=n`; `None` when the box holds no node.
    fn node_box(&self, lo: [f64; 3], hi: [f64; 3]) -> Option<([i64; 3], [i64; 3])> {
        let spans: [Option<(i64, i64)>; 3] = std::array::from_fn(|k| node_span(self.min[k], self.cell[k], self.n[k] as i64, lo[k], hi[k]));
        if spans.iter().any(Option::is_none) {
            return None;
        }
        let a = spans.map(|s| s.map_or(0, |x| x.0));
        let b = spans.map(|s| s.map_or(0, |x| x.1));
        Some((a, b))
    }

    /// Bytes the grids take now, against the budget they were created with.
    pub fn memory(&self) -> (u64, u64) {
        (self.used, self.budget)
    }

    /// Material volume in mm^3, the cheap midpoint estimate: the average of the three grids (each is
    /// the sum of its ray lengths times its cell area). A wall that lies on a ray row loses half a row
    /// per wall, so the error is bounded by about one cell size times the wall area, and it falls as
    /// the cell size falls. [`crate::mesh::mesh_volume`] is the same quantity from the surface, which
    /// rounds sharp edges by up to a cell instead.
    pub fn volume(&self) -> f64 {
        let mut total = 0.0;
        for (a, grid) in self.grids.iter().enumerate() {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            total += grid.len * self.cell[u] * self.cell[v];
        }
        total / 3.0
    }

    /// Marks these chunks dirty: the caller redraws them. A rebuilt stock marks the chunks the old one
    /// had touched, since their surface has changed back to the box.
    pub fn mark_dirty(&mut self, chunks: impl IntoIterator<Item = [usize; 3]>) {
        self.dirty.extend(chunks);
    }

    /// The chunks a cut has reached since this stock was built, each once, sorted.
    pub fn touched_chunks(&self) -> Vec<[usize; 3]> {
        self.touched.iter().copied().collect()
    }

    /// Whether a cut has reached chunk `idx`. A chunk no cut reached is the uncut box.
    pub fn is_touched(&self, idx: [usize; 3]) -> bool {
        self.touched.contains(&idx)
    }

    /// FNV-1a over every segment and every start, in grid order: equal programs give equal hashes.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |x: u64| {
            for byte in x.to_le_bytes() {
                h ^= u64::from(byte);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        };
        for grid in &self.grids {
            for tile in &grid.tiles {
                eat(tile.start.len() as u64);
                for s in &tile.start {
                    eat(u64::from(*s));
                }
                for seg in &tile.segs {
                    eat(u64::from(seg.lo.to_bits()));
                    eat(u64::from(seg.hi.to_bits()));
                    eat(u64::from(seg.lo_tag));
                    eat(u64::from(seg.hi_tag));
                }
            }
        }
        h
    }

    /// Total number of material intervals over all rays.
    pub fn segments(&self) -> usize {
        self.grids.iter().flat_map(|g| g.tiles.iter()).map(|t| t.segs.len()).sum()
    }

    /// The box's minimum and maximum corners, in the stored frame.
    pub(crate) fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        (self.min, self.max)
    }

    /// Cells per axis; the lattice nodes run from `-1` to `n` on each axis.
    pub(crate) fn cell_counts(&self) -> [usize; 3] {
        self.n
    }

    /// Position of lattice node `i` along axis `k` (`-1` and `n` are the air just outside).
    pub(crate) fn pos(&self, k: usize, i: i64) -> f64 {
        let cell = self.cell.get(k).copied().unwrap_or(0.0);
        self.min.get(k).copied().unwrap_or(0.0) + (i as f64 + 0.5) * cell
    }

    /// Whether lattice node `n` is material: the majority of the three rays through it. A ray
    /// that does not exist (the node is outside the box) counts as air.
    pub(crate) fn node_inside(&self, node: [i64; 3]) -> bool {
        let mut votes = 0;
        for a in 0..3 {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let (Some(&nu), Some(&nv), Some(&na)) = (node.get(u), node.get(v), node.get(a)) else { continue };
            if real(nu, self.n_of(u)) && real(nv, self.n_of(v)) && self.contains(a, nu as usize, nv as usize, self.pos(a, na)) {
                votes += 1;
            }
        }
        votes >= 2
    }

    /// The material boundary the edge from `lower` along `axis` crosses: the first ray end inside
    /// the edge, or the middle of the edge when the ray has none (the surface is then approximate).
    pub(crate) fn edge_crossing(&self, axis: usize, lower: [i64; 3]) -> Option<Vec3> {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let (nu, nv, na) = (*lower.get(u)?, *lower.get(v)?, *lower.get(axis)?);
        if !(real(nu, self.n_of(u)) && real(nv, self.n_of(v))) {
            return None;
        }
        let (t0, t1) = (self.rel(axis, self.pos(axis, na)), self.rel(axis, self.pos(axis, na + 1)));
        let mut best: Option<f64> = None;
        for s in self.ray_segs(axis, nu as usize, nv as usize) {
            for b in [f64::from(s.lo), f64::from(s.hi)] {
                if b >= t0 && b <= t1 && best.is_none_or(|x| b < x) {
                    best = Some(b);
                }
            }
        }
        let t = self.min[axis] + best.unwrap_or(0.5 * (t0 + t1));
        let mut p = [0.0f64; 3];
        if let Some(x) = p.get_mut(axis) {
            *x = t;
        }
        if let Some(x) = p.get_mut(u) {
            *x = self.pos(u, nu);
        }
        if let Some(x) = p.get_mut(v) {
            *x = self.pos(v, nv);
        }
        Some(Vec3::from(p))
    }

    fn n_of(&self, k: usize) -> i64 {
        self.n.get(k).copied().unwrap_or(0) as i64
    }

    /// A coordinate along `axis` in the stored frame (relative to the stock minimum).
    fn rel(&self, axis: usize, t: f64) -> f64 {
        t - self.min.get(axis).copied().unwrap_or(0.0)
    }

    fn contains(&self, axis: usize, iu: usize, iv: usize, t: f64) -> bool {
        let t = self.rel(axis, t);
        self.ray_segs(axis, iu, iv).iter().any(|s| f64::from(s.lo) <= t && t <= f64::from(s.hi))
    }

    fn ray_segs(&self, axis: usize, iu: usize, iv: usize) -> &[Seg] {
        let Some(grid) = self.grids.get(axis) else { return &[] };
        if iu >= grid.nu || iv >= grid.nv {
            return &[];
        }
        let (tu, tv) = (iu / TILE, iv / TILE);
        let Some(tile) = grid.tiles.get(tv * grid.tu + tu) else { return &[] };
        let w = TILE.min(grid.nu - tu * TILE);
        tile.ray((iv % TILE) * w + iu % TILE)
    }

    /// Inclusive range of ray indices along `k` whose rays reach `lo..=hi` (with half a cell of
    /// slack), or `None` when there are none.
    fn index_range(&self, k: usize, lo: f64, hi: f64) -> Option<(usize, usize)> {
        let cell = self.cell[k];
        let half = 0.5 * cell;
        let base = self.min[k] + half;
        let a = ((lo - half - base) / cell).ceil().max(0.0);
        let b = ((hi + half - base) / cell).floor().min((self.n[k] as f64) - 1.0);
        if !(a.is_finite() && b.is_finite()) || a > b {
            return None;
        }
        Some((a as usize, b as usize))
    }
}

/// The inclusive node indices along one axis whose positions lie in `lo..=hi`, clamped to `-1..=n`.
fn node_span(min: f64, cell: f64, n: i64, lo: f64, hi: f64) -> Option<(i64, i64)> {
    let a = ((lo - min) / cell - 0.5).ceil();
    let b = ((hi - min) / cell - 0.5).floor();
    if !(a.is_finite() && b.is_finite()) {
        return None;
    }
    let (a, b) = ((a as i64).max(-1), (b as i64).min(n));
    (a <= b).then_some((a, b))
}

fn real(i: i64, n: i64) -> bool {
    i >= 0 && i < n
}

/// The cell counts and sizes of a box at ray spacing `h`, and the bytes its grids take.
struct BoxLayout {
    lo: [f64; 3],
    hi: [f64; 3],
    n: [usize; 3],
    cell: [f64; 3],
    need: u64,
}

impl BoxLayout {
    fn new(min: Vec3, max: Vec3, h: f64) -> Result<BoxLayout, SimError> {
        if !h.is_finite() || h <= 0.0 {
            return Err(SimError::BadInput("cell size must be positive and finite".into()));
        }
        let lo: [f64; 3] = min.into();
        let hi: [f64; 3] = max.into();
        let mut n = [0usize; 3];
        let mut cell = [0.0f64; 3];
        for (k, n_k) in n.iter_mut().enumerate() {
            let (a, b) = (lo[k], hi[k]);
            if !(a.is_finite() && b.is_finite()) || a >= b || a.abs() > MAX_ABS_COORD || b.abs() > MAX_ABS_COORD {
                return Err(SimError::BadInput("stock box must be finite, non-empty and within 100 m".into()));
            }
            let cells = ((b - a) / h).ceil();
            if !(1.0..=MAX_CELLS_PER_AXIS).contains(&cells) {
                return Err(SimError::BadInput("stock has too many cells along an axis".into()));
            }
            *n_k = cells as usize;
            if let Some(c) = cell.get_mut(k) {
                *c = (b - a) / cells;
            }
        }
        let mut need = 0u64;
        for axis in 0..3 {
            let (nu, nv) = (n_at(&n, (axis + 1) % 3), n_at(&n, (axis + 2) % 3));
            let rays = (nu as u64).saturating_mul(nv as u64);
            // The per-ray estimate, plus the start sentinel of each tile (4 bytes).
            let tiles = (nu.div_ceil(TILE) as u64).saturating_mul(nv.div_ceil(TILE) as u64);
            need = need.saturating_add(rays.saturating_mul(BYTES_PER_RAY)).saturating_add(tiles.saturating_mul(4));
        }
        Ok(BoxLayout { lo, hi, n, cell, need })
    }
}

fn n_at(n: &[usize; 3], k: usize) -> usize {
    n.get(k).copied().unwrap_or(0)
}

fn build_grid(axis: usize, n: &[usize; 3], lo: &[f64; 3], hi: &[f64; 3]) -> Grid {
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let (nu, nv) = (n_at(n, u), n_at(n, v));
    let (tu, tv) = (nu.div_ceil(TILE), nv.div_ceil(TILE));
    // The axis runs from 0 to its length in stored coordinates, so f32 keeps the offsets exact.
    let length = hi.get(axis).copied().unwrap_or(0.0) - lo.get(axis).copied().unwrap_or(0.0);
    let (a_lo, a_hi) = (0.0f32, length as f32);
    let mut cache: Vec<((usize, usize), Arc<Tile>)> = Vec::new();
    let mut tiles = Vec::with_capacity(tu * tv);
    for tvi in 0..tv {
        for tui in 0..tu {
            let w = TILE.min(nu - tui * TILE);
            let h = TILE.min(nv - tvi * TILE);
            let tile = match cache.iter().find(|(k, _)| *k == (w, h)) {
                Some((_, t)) => Arc::clone(t),
                None => {
                    let t = Arc::new(Tile::uncut(w * h, a_lo, a_hi));
                    cache.push(((w, h), Arc::clone(&t)));
                    t
                }
            };
            tiles.push(tile);
        }
    }
    let len = tiles.iter().map(|t| tile_len(t)).sum();
    Grid { nu, nv, tu, tiles, len }
}

/// Sum of the ray lengths of one tile, in stored units.
fn tile_len(tile: &Tile) -> f64 {
    tile.segs.iter().map(|s| f64::from(s.hi) - f64::from(s.lo)).sum()
}

/// The tile's rays after the cuts: each cut ray is rebuilt, the others are copied.
fn rebuild(old: &Tile, cuts: &[(usize, Vec<(f64, f64)>)], tag: u32) -> Result<Tile, SimError> {
    let rays = old.rays();
    let mut start = Vec::with_capacity(rays + 1);
    let mut segs = Vec::with_capacity(old.segs.len());
    let mut next = 0usize;
    for r in 0..rays {
        let base = segs.len();
        start.push(index_u32(base)?);
        match cuts.get(next).filter(|(lr, _)| *lr == r) {
            Some((_, c)) => {
                let mut c = c.clone();
                subtract(old.ray(r), &mut c, tag, &mut segs);
                next += 1;
            }
            None => segs.extend_from_slice(old.ray(r)),
        }
        if segs.len() - base > MAX_SEGS_PER_RAY {
            return Err(SimError::SegmentCap { max: MAX_SEGS_PER_RAY });
        }
    }
    start.push(index_u32(segs.len())?);
    // The old tile's buffers may be larger than the cut tile needs: give back the slack.
    start.shrink_to_fit();
    segs.shrink_to_fit();
    Ok(Tile { start, segs })
}

fn index_u32(x: usize) -> Result<u32, SimError> {
    u32::try_from(x).map_err(|_| SimError::BadInput("stock has too many segments".into()))
}

/// Removes the cut intervals from one ray's material, tagging the new ends with `tag`.
fn subtract(old: &[Seg], cuts: &mut [(f64, f64)], tag: u32, out: &mut Vec<Seg>) {
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(cuts.len());
    for &(a, b) in cuts.iter() {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    for seg in old {
        let shi = f64::from(seg.hi);
        let mut cur_lo = seg.lo;
        let mut cur_tag = seg.lo_tag;
        let mut covered = false;
        for &(a, b) in &merged {
            if b <= f64::from(seg.lo) {
                continue;
            }
            if a >= shi {
                break;
            }
            if a > f64::from(cur_lo) {
                push_seg(out, cur_lo, a as f32, cur_tag, tag);
            }
            if b >= shi {
                covered = true;
                break;
            }
            cur_lo = b as f32;
            cur_tag = tag;
        }
        if !covered {
            push_seg(out, cur_lo, seg.hi, cur_tag, seg.hi_tag);
        }
    }
}

fn push_seg(out: &mut Vec<Seg>, lo: f32, hi: f32, lo_tag: u32, hi_tag: u32) {
    if hi > lo {
        out.push(Seg { lo, hi, lo_tag, hi_tag });
    }
}

#[cfg(test)]
#[path = "dexel_tests.rs"]
mod tests;
