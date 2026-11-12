//! The heightfield stock of a 3-axis program: one top height per cell of a grid over XY, each
//! block lowering every cell to the tool's lower envelope along it.

use super::*;

/// What one sweep removed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Removed {
    /// Material removed (mm³).
    pub volume: f64,
    /// The deepest it cut into the stock as it was (mm, along Z or the radius), and where (the
    /// bottom of that cut).
    pub depth: f64,
    pub at: Vec3,
    /// The most material that stood above the cutting length where it cut (mm: above the tip
    /// plus the flute length; the shank rubbing), and where.
    pub above: f64,
    pub above_at: Vec3,
    /// The deepest it cut below the part's surface along Z (along the ray) where a part floor
    /// is given ([`SimState::set_floor`]), and where; a gouge only once measured in 3D, along
    /// the column from `gouge_from` (just outside the part's surface) to `gouge_at`, as the
    /// report measures a gouge.
    pub gouge: f64,
    pub gouge_at: Vec3,
    pub gouge_from: Vec3,
}

impl Default for Removed {
    fn default() -> Removed {
        Removed {
            volume: 0.0,
            depth: 0.0,
            at: Vec3::ZERO,
            above: 0.0,
            above_at: Vec3::ZERO,
            gouge: 0.0,
            gouge_at: Vec3::ZERO,
            gouge_from: Vec3::ZERO,
        }
    }
}

impl Removed {
    pub(super) fn add(&mut self, o: Removed) {
        self.volume += o.volume;
        if o.depth > self.depth {
            self.depth = o.depth;
            self.at = o.at;
        }
        if o.above > self.above {
            self.above = o.above;
            self.above_at = o.above_at;
        }
        if o.gouge > self.gouge {
            self.gouge = o.gouge;
            self.gouge_at = o.gouge_at;
            self.gouge_from = o.gouge_from;
        }
    }
}

/// Per tile of [`TILE`] × [`TILE`] cells of a band: an upper bound of the values in it (exact
/// unless stale), whether a cell was cut since it was found, and a version that changes with
/// every cut.
#[derive(Clone, Debug)]
pub(super) struct Tiles {
    pub(super) tx: usize,
    pub(super) max: Vec<f64>,
    pub(super) stale: Vec<bool>,
    pub(super) version: Vec<u32>,
}

impl Tiles {
    pub(super) fn new(nu: usize, nv: usize, value: f64) -> Tiles {
        let n = nu.div_ceil(TILE).max(1) * nv.div_ceil(TILE).max(1);
        Tiles { tx: nu.div_ceil(TILE).max(1), max: vec![value; n], stale: vec![false; n], version: vec![0; n] }
    }

    /// Cell `(i, j)` of the band (`j` from the band's first row) was cut.
    pub(super) fn mark(&mut self, i: usize, j: usize) {
        let t = (j / TILE) * self.tx + i / TILE;
        if let Some(s) = self.stale.get_mut(t) {
            *s = true;
        }
        if let Some(v) = self.version.get_mut(t) {
            *v = v.wrapping_add(1);
        }
    }

    /// The highest of `values` (`nu` a row) over the cells `i0..=i1`, `j0..=j1`, by whole tiles:
    /// at least the highest of those cells (stale tiles found again first).
    pub(super) fn max_in(&mut self, values: &[f64], nu: usize, (i0, i1): (usize, usize), (j0, j1): (usize, usize)) -> f64 {
        let mut m = f64::NEG_INFINITY;
        let nv = values.len() / nu.max(1);
        for tj in j0 / TILE..=j1 / TILE {
            for ti in i0 / TILE..=i1 / TILE {
                let t = tj * self.tx + ti;
                if self.stale.get(t) == Some(&true) {
                    let mut best = f64::NEG_INFINITY;
                    for j in tj * TILE..((tj + 1) * TILE).min(nv) {
                        for i in ti * TILE..((ti + 1) * TILE).min(nu) {
                            if let Some(&v) = values.get(j * nu + i) {
                                best = best.max(v);
                            }
                        }
                    }
                    if let (Some(slot), Some(s)) = (self.max.get_mut(t), self.stale.get_mut(t)) {
                        *slot = best;
                        *s = false;
                    }
                }
                m = m.max(self.max.get(t).copied().unwrap_or(f64::NEG_INFINITY));
            }
        }
        m
    }
}

/// What a cut of one cell from `old` down to `new` (along Z or the ray) means beyond the
/// material: the shank above the flutes (`flute` above the cut), and the part's floor `floor`
/// (NaN: none) passed by more than `tol`. `point(v)` is the cell's point at height (radius) `v`.
pub(super) fn note_cut(out: &mut Removed, old: f64, new: f64, flute: f64, floor: f64, tol: f64, point: impl Fn(f64) -> Vec3) {
    let cut = old - new;
    if cut > out.depth {
        out.depth = cut;
        out.at = point(new);
    }
    let above = old - (new + flute);
    if flute > 0.0 && above > tol && above > out.above {
        out.above = above;
        out.above_at = point(new + flute);
    }
    let below = floor - new;
    if below > tol && below > out.gouge {
        out.gouge = below;
        out.gouge_at = point(new);
        out.gouge_from = point(floor + tol);
    }
}

/// The heightfield stock (3-axis), or one band of its rows: rows `j0..j0 + ny` of the whole
/// grid, which starts at `y0`.
#[derive(Clone)]
pub(super) struct Heights {
    pub(super) x0: f64,
    pub(super) y0: f64,
    pub(super) dx: f64,
    pub(super) dy: f64,
    pub(super) nx: usize,
    pub(super) j0: usize,
    pub(super) ny: usize,
    pub(super) bottom: f64,
    pub(super) top: f64,
    pub(super) h: Vec<f64>,
    pub(super) visits: Arc<AtomicU64>,
    pub(super) tiles: Tiles,
    /// The tool slot that cut each cell last ([`UNCUT`]: none).
    pub(super) last: Vec<u8>,
    /// The part over every cell of the whole grid, when gouges are watched for as the stock is
    /// cut.
    pub(super) floor: Option<Arc<PartFloor>>,
    /// The lowest Z of each cell's material, where an earlier setup turned upside down cut it
    /// from what is now its underside (empty: every cell's material goes down to `bottom`).
    pub(super) under: Vec<f64>,
    pub(super) tol: f64,
    /// The tool cutting now: its slot and flute length.
    pub(super) slot: u8,
    pub(super) flute: f64,
}

impl Heights {
    /// The centre of the band's cell `(i, j)`.
    pub(super) fn centre(&self, i: usize, j: usize) -> (f64, f64) {
        (self.x0 + (i as f64 + 0.5) * self.dx, self.row_y(j))
    }

    pub(super) fn row_y(&self, j: usize) -> f64 {
        self.y0 + ((self.j0 + j) as f64 + 0.5) * self.dy
    }

    /// Cells whose centres are within `[lo, hi]` along X, as an index range (empty when none).
    pub(super) fn columns(&self, lo: f64, hi: f64) -> Option<(usize, usize)> {
        let a = ((lo - self.x0) / self.dx - 0.5).ceil();
        let b = ((hi - self.x0) / self.dx - 0.5).floor();
        if !(a <= b) || b < 0.0 || a > (self.nx as f64 - 1.0) {
            return None;
        }
        Some((index(a, self.nx), index(b, self.nx)))
    }

    /// The band's rows whose centres are within `[lo, hi]` along Y.
    pub(super) fn rows(&self, lo: f64, hi: f64) -> Option<(usize, usize)> {
        let a = ((lo - self.y0) / self.dy - 0.5).ceil() - self.j0 as f64;
        let b = ((hi - self.y0) / self.dy - 0.5).floor() - self.j0 as f64;
        if !(a <= b) || b < 0.0 || a > (self.ny as f64 - 1.0) {
            return None;
        }
        Some((index(a, self.ny), index(b, self.ny)))
    }

    pub(super) fn count(&self, n: usize) -> Result<()> {
        count(&self.visits, n as u64)
    }

    /// Lowers cell `c` to `z` (not below the stock's bottom).
    pub(super) fn lower(&mut self, c: usize, z: f64, out: &mut Removed) {
        let area = self.dx * self.dy;
        let bottom = self.bottom;
        let Some(h) = self.h.get_mut(c) else { return };
        let z = z.max(bottom);
        if !(z < *h) {
            return;
        }
        let old = *h;
        out.volume += (old - z) * area;
        *h = z;
        let nx = self.nx.max(1);
        let (i, j) = (c % nx, c / nx);
        self.tiles.mark(i, j);
        if let Some(l) = self.last.get_mut(c) {
            *l = self.slot;
        }
        let floor = self.floor.as_ref().and_then(|f| f.outer.get(self.j0 * self.nx + c).copied()).unwrap_or(f64::NAN);
        let (x, y) = self.centre(i, j);
        note_cut(out, old, z, self.flute, floor, self.tol, |v| Vec3::new(x, y, v));
    }

    /// The highest material over the cells whose centres are within `[x0, x1] × [y0, y1]` and
    /// satisfy `inside`, above `z`: the first such cell's top, or `None`.
    pub(super) fn above(&mut self, (x0, x1): (f64, f64), (y0, y1): (f64, f64), z: f64, inside: &dyn Fn(f64, f64) -> bool) -> Option<Vec3> {
        if z >= self.top {
            return None;
        }
        let ((i0, i1), (j0, j1)) = (self.columns(x0, x1)?, self.rows(y0, y1)?);
        if self.tiles.max_in(&self.h, self.nx, (i0, i1), (j0, j1)) <= z {
            return None;
        }
        for j in j0..=j1 {
            for i in i0..=i1 {
                let Some(&h) = self.h.get(j * self.nx + i) else { continue };
                let (x, y) = self.centre(i, j);
                if h > z && h > self.bottom && inside(x, y) {
                    return Some(Vec3::new(x, y, h));
                }
            }
        }
        None
    }

    /// The tool swept straight from tip `a` to tip `b`.
    pub(super) fn sweep_line(&mut self, p: &Profile, a: Vec3, b: Vec3) -> Result<Removed> {
        let mut out = Removed::default();
        let r = p.radius;
        let d = Vec2::new(b.x - a.x, b.y - a.y);
        let len = d.len();
        let dir = if len > 1e-12 { d * (1.0 / len) } else { Vec2::new(1.0, 0.0) };
        let nrm = dir.perp();
        let (z0, dz) = (a.z, b.z - a.z);
        if a.z.min(b.z) >= self.top {
            return Ok(out);
        }
        let Some((j0, j1)) = self.rows(a.y.min(b.y) - r, a.y.max(b.y) + r) else { return Ok(out) };
        for j in j0..=j1 {
            let y = self.row_y(j);
            // The capsule round the move on this row.
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for e in [a, b] {
                let ey = y - e.y;
                if ey.abs() <= r {
                    let w = (r * r - ey * ey).sqrt();
                    lo = lo.min(e.x - w);
                    hi = hi.max(e.x + w);
                }
            }
            if len > 1e-12 && dir.y.abs() > 1e-12 {
                for side in [-r, r] {
                    let s = (y - a.y - side * nrm.y) / dir.y;
                    if (0.0..=len).contains(&s) {
                        let x = a.x + side * nrm.x + s * dir.x;
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
            }
            let Some((i0, i1)) = self.columns(lo, hi) else { continue };
            self.count(i1 - i0 + 1)?;
            for i in i0..=i1 {
                let c = j * self.nx + i;
                let Some(&h) = self.h.get(c) else { continue };
                let x = self.x0 + (i as f64 + 0.5) * self.dx;
                let rel = Vec2::new(x - a.x, y - a.y);
                let (u, v) = if len > 1e-12 { (rel.dot(dir), rel.cross(dir).abs()) } else { (rel.len(), 0.0) };
                // Nothing to do where the stock is already below anything the tool reaches.
                let outside = (-u).max(u - len).max(0.0);
                if h <= z0.min(z0 + dz) + p.height(hyp(outside, v)) {
                    continue;
                }
                if let Some(z) = p.along_line(u, v, len, z0, dz) {
                    self.lower(c, z, &mut out);
                }
            }
        }
        Ok(out)
    }

    /// The tool swept along an arc in XY about `center` from tip `a` to tip `b`, turning `sweep`.
    /// A helix is followed as chords; `part` (fractions from and to) cuts only the chords of that
    /// part of it, so that cutting it in parts leaves what cutting it whole does.
    pub(super) fn sweep_arc(&mut self, p: &Profile, a: Vec3, b: Vec3, center: Vec2, sweep: f64, part: (f64, f64)) -> Result<Removed> {
        let tol = self.tol;
        let rho = Vec2::new(a.x, a.y).dist(center);
        let r = p.radius;
        if chorded(a, b, center, sweep) {
            // A helix (or a degenerate arc): chords at most a quarter of the tolerance inside it.
            let e = (tol * 0.25).min(rho);
            let step = if rho > 1e-9 { 2.0 * (1.0 - e / rho).clamp(-1.0, 1.0).acos() } else { TAU };
            let n = (sweep.abs() / step.max(1e-9)).ceil().max(1.0);
            if !(n <= MAX_BLOCK_POINTS as f64) {
                return Err(CamError::TooLarge("an arc needs too many chords to simulate".into()));
            }
            let n = n as usize;
            let start = Vec2::new(a.x - center.x, a.y - center.y).angle();
            let at = |k: usize| {
                let t = k as f64 / n as f64;
                if k == n { b } else { (center + Vec2::from_angle(start + sweep * t) * rho).extend(a.z + (b.z - a.z) * t) }
            };
            let (k0, k1) = (done(n, part.0), done(n, part.1));
            let mut out = Removed::default();
            let mut prev = if k0 == 0 { a } else { at(k0) };
            for k in k0 + 1..=k1 {
                let q = at(k);
                out.add(self.sweep_line(p, prev, q)?);
                prev = q;
            }
            return Ok(out);
        }
        let mut out = Removed::default();
        if a.z >= self.top {
            return Ok(out);
        }
        let start = Vec2::new(a.x - center.x, a.y - center.y).angle();
        // The arc's box: its ends and the quarter points it passes.
        let (mut lo, mut hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
        for q in 0..4 {
            let ang = q as f64 * PI * 0.5;
            if within_sweep(ang, start, sweep) {
                let pt = center + Vec2::from_angle(ang) * rho;
                lo = Vec2::new(lo.x.min(pt.x), lo.y.min(pt.y));
                hi = Vec2::new(hi.x.max(pt.x), hi.y.max(pt.y));
            }
        }
        let (Some((i0, i1)), Some((j0, j1))) = (self.columns(lo.x - r, hi.x + r), self.rows(lo.y - r, hi.y + r)) else { return Ok(out) };
        let (pa, pb) = (Vec2::new(a.x, a.y), Vec2::new(b.x, b.y));
        for j in j0..=j1 {
            self.count(i1 - i0 + 1)?;
            for i in i0..=i1 {
                let c = j * self.nx + i;
                let Some(&h) = self.h.get(c) else { continue };
                if h <= a.z {
                    continue;
                }
                let (x, y) = self.centre(i, j);
                let q = Vec2::new(x, y);
                let rel = q - center;
                let rq = rel.len();
                if (rq - rho).abs() > r {
                    continue;
                }
                let d = if rq > 1e-12 && within_sweep(rel.angle(), start, sweep) { (rq - rho).abs() } else { q.dist(pa).min(q.dist(pb)) };
                if d <= r {
                    self.lower(c, a.z + p.height(d), &mut out);
                }
            }
        }
        Ok(out)
    }

    /// Whether `p` is in the stock, when it is over this band (`None` when it is not).
    pub(super) fn owns(&self, p: Vec3) -> Option<bool> {
        let (fx, fy) = ((p.x - self.x0) / self.dx, (p.y - self.y0) / self.dy - self.j0 as f64);
        if !(fy >= 0.0 && fy < self.ny as f64) {
            return None;
        }
        if !(fx >= 0.0 && fx < self.nx as f64) {
            return Some(false);
        }
        let c = index(fy, self.ny) * self.nx + index(fx, self.nx);
        Some(self.h.get(c).is_some_and(|&h| p.z < h && p.z > self.bottom))
    }
}

/// Whether an arc from `a` to `b` about `center` turning `sweep` is followed as chords (a helix,
/// or a degenerate arc).
pub(super) fn chorded(a: Vec3, b: Vec3, center: Vec2, sweep: f64) -> bool {
    (b.z - a.z).abs() > 1e-12 || Vec2::new(a.x, a.y).dist(center) <= 1e-9 || sweep.abs() <= 1e-12
}

/// The turn of an arc from `from` to `to` about `center` as the controller runs it (signed,
/// radians): an arc whose end is written at its start is a whole turn, not none.
pub(super) fn arc_turn(from: Vec3, to: Vec3, center: Vec2, ccw: bool) -> f64 {
    let (a, b) = (Vec2::new(from.x, from.y), Vec2::new(to.x, to.y));
    if a.dist(b) <= 1e-9 && a.dist(center) > 1e-9 {
        return if ccw { TAU } else { -TAU };
    }
    arc_sweep(from, to, center, ccw)
}

/// Whether the angle `ang` is on an arc starting at `start` and turning `sweep` (signed).
pub(super) fn within_sweep(ang: f64, start: f64, sweep: f64) -> bool {
    if sweep >= 0.0 { (ang - start).rem_euclid(TAU) <= sweep + 1e-12 } else { (start - ang).rem_euclid(TAU) <= -sweep + 1e-12 }
}

/// The highest (`pick` max) or lowest (min) `map` holds over the cell centred on `(x, y)`, `dx`
/// by `dy` (at most nine of the map's cells on a side); `None` where the cell is off the map.
pub(super) fn over_cell(map: &crate::adaptive::HeightMap, x: f64, y: f64, dx: f64, dy: f64, pick: fn(f64, f64) -> f64) -> Option<f64> {
    // The map's cells under the cell, its edges drawn in a hair so a cell that only touches one
    // does not take it.
    let range = |o: f64, d: f64, n: usize, c: f64, h: f64| {
        let (a, b) = (((c - 0.5 * h - o) / d + 1e-6).floor(), ((c + 0.5 * h - o) / d - 1e-6).floor());
        if !(a.is_finite() && b.is_finite()) || b < 0.0 || a > n as f64 - 1.0 {
            return None;
        }
        let a = a.max(0.0) as usize;
        let b = (b.min(n as f64 - 1.0).max(0.0) as usize).max(a);
        Some((a, b.min(a + 8)))
    };
    let (i, j) = (range(map.x0, map.dx, map.nx, x, dx)?, range(map.y0, map.dy, map.ny, y, dy)?);
    let mut out: Option<f64> = None;
    for jj in j.0..=j.1 {
        for ii in i.0..=i.1 {
            if let Some(&z) = map.heights.get(jj * map.nx + ii).filter(|z| z.is_finite()) {
                out = Some(out.map_or(z, |o| pick(o, z)));
            }
        }
    }
    out
}
