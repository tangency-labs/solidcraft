//! The radial stock of a 4th axis about X: one ray from the axis per cell of a grid over (x, θ),
//! holding the material from the axis (from where a box bar's rectangle starts it) out to its
//! end, less stretches of air inside it, cut by the tool placed at points along every block.

use super::*;

/// Most tool positions on a 4th axis over the whole program.
pub(super) const MAX_STAMPS: usize = 50_000_000;

/// Air inside a ray's material: sorted, disjoint stretches `[from, to]` of radius, all inside
/// the material's end.
pub type Gaps = Vec<[f64; 2]>;

/// Most stretches of air one ray holds: a cut that would make more is left out of the model
/// (and counted in a warning).
pub(super) const MAX_GAPS: usize = 32;
/// Most stretches of air one band of the stock holds over all its rays.
pub(super) const MAX_GAP_ENTRIES: usize = 4_000_000;

/// The material of a ray ending at `end` with air `gaps`, over `[a, b]`: its length, and its
/// measure in ρ² (half the cell's angle times its width times that is its volume).
pub(super) fn material(gaps: Option<&Gaps>, end: f64, a: f64, b: f64) -> (f64, f64) {
    let (a, b) = (a.max(0.0), b.min(end));
    if !(a < b) {
        return (0.0, 0.0);
    }
    let (mut len, mut sq) = (b - a, b * b - a * a);
    for g in gaps.into_iter().flatten() {
        let (lo, hi) = (g[0].max(a), g[1].min(b));
        if lo < hi {
            len -= hi - lo;
            sq -= hi * hi - lo * lo;
        }
    }
    (len.max(0.0), sq.max(0.0))
}

/// A ray's material cut from `a` out: where it ends now (where the air `a` is in starts, when it
/// is in some), the air beyond that gone from `gaps`.
pub(super) fn cut_out(gaps: &mut Gaps, a: f64) -> f64 {
    let end = gaps.iter().find(|g| g[0] < a && a <= g[1]).map_or(a, |g| g[0]);
    gaps.retain(|g| g[0] < end);
    end
}

/// Air `[a, b]` added inside a ray's material, merged with the air it meets.
pub(super) fn add_gap(gaps: &mut Gaps, a: f64, b: f64) {
    let (mut lo, mut hi) = (a, b);
    gaps.retain(|g| {
        if g[1] < lo || g[0] > hi {
            true
        } else {
            lo = lo.min(g[0]);
            hi = hi.max(g[1]);
            false
        }
    });
    let at = gaps.partition_point(|g| g[0] < lo);
    gaps.insert(at, [lo, hi]);
}

/// No air in a ray (a [`GapStore`] slot).
pub(super) const NO_GAPS: u32 = u32::MAX;

/// The air inside the rays' material of a band, by cell: a slot per cell into lists kept for
/// reuse, so that a ray gaining and losing air as a tool passes beside it (as roughing round a
/// bar does at every pass) costs no hashing and no allocation.
#[derive(Clone, Default)]
pub(super) struct GapStore {
    /// How many cells the band has.
    pub(super) cells: usize,
    /// Each cell's list in `lists`, or [`NO_GAPS`]; empty until a ray first holds air.
    pub(super) slot: Vec<u32>,
    pub(super) lists: Vec<Gaps>,
    /// Lists no cell holds, empty, kept with their room.
    pub(super) free: Vec<u32>,
}

impl GapStore {
    pub(super) fn new(cells: usize) -> GapStore {
        GapStore { cells, ..GapStore::default() }
    }

    pub(super) fn get(&self, cell: usize) -> Option<&Gaps> {
        match self.slot.get(cell) {
            Some(&s) if s != NO_GAPS => self.lists.get(s as usize),
            _ => None,
        }
    }

    pub(super) fn get_mut(&mut self, cell: usize) -> Option<&mut Gaps> {
        match self.slot.get(cell) {
            Some(&s) if s != NO_GAPS => self.lists.get_mut(s as usize),
            _ => None,
        }
    }

    /// The list of `cell`, made (empty) when it has none; `None` past the band's cells.
    pub(super) fn entry(&mut self, cell: usize) -> Option<&mut Gaps> {
        if cell >= self.cells {
            return None;
        }
        if self.slot.is_empty() {
            self.slot = vec![NO_GAPS; self.cells];
        }
        let s = *self.slot.get(cell)?;
        let s = if s != NO_GAPS {
            s
        } else {
            let s = match self.free.pop() {
                Some(s) => s,
                None => {
                    self.lists.push(Gaps::new());
                    u32::try_from(self.lists.len() - 1).ok()?
                }
            };
            if let Some(slot) = self.slot.get_mut(cell) {
                *slot = s;
            }
            s
        };
        self.lists.get_mut(s as usize)
    }

    /// `cell` without air, its list kept for another.
    pub(super) fn remove(&mut self, cell: usize) {
        if let Some(slot) = self.slot.get_mut(cell)
            && *slot != NO_GAPS
        {
            if let Some(l) = self.lists.get_mut(*slot as usize) {
                l.clear();
            }
            self.free.push(*slot);
            *slot = NO_GAPS;
        }
    }

    /// Each cell with air, in order, and its air.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &Gaps)> {
        self.slot.iter().enumerate().filter_map(|(c, &s)| if s == NO_GAPS { None } else { self.lists.get(s as usize).map(|g| (c, g)) })
    }
}

/// Whether radius `v` is in the air of `gaps`.
pub(super) fn in_gap(gaps: Option<&Gaps>, v: f64) -> bool {
    gaps.is_some_and(|gs| gs.iter().any(|g| g[0] < v && v < g[1]))
}

/// The radial stock (a 4th axis about X), or one band of its angles: angles `k0..k0 + nt` of
/// the `nt_all` round. Each cell is a ray from the axis holding material from the axis (from
/// where a box bar's rectangle starts it) out to its end, less stretches of air inside it where
/// a tool cut beside the ray without reaching out of the material (an indexed flat, a pocket
/// or a hole in the side of the part seen at a slant).
#[derive(Clone)]
pub(super) struct Radii {
    pub(super) x0: f64,
    pub(super) dx: f64,
    pub(super) nx: usize,
    pub(super) k0: usize,
    pub(super) nt: usize,
    pub(super) nt_all: usize,
    /// The farthest the stock reaches from the axis.
    pub(super) radius: f64,
    pub(super) rho: Vec<f64>,
    /// The air inside the rays' material, by cell of the band.
    pub(super) gaps: GapStore,
    pub(super) gap_entries: usize,
    /// How far the stock reaches along each of the band's angles before it is cut.
    pub(super) start: Vec<f64>,
    /// sin and cos of the band's angles.
    pub(super) sin: Vec<f64>,
    pub(super) cos: Vec<f64>,
    pub(super) visits: Arc<AtomicU64>,
    pub(super) tol: f64,
    /// Cuts beside a ray left out of the model (too many stretches of air).
    pub(super) unmodelled: u64,
    pub(super) tiles: Tiles,
    pub(super) last: Vec<u8>,
    /// The part along every ray of the whole grid.
    pub(super) floor: Option<Arc<PartFloor>>,
    pub(super) slot: u8,
    pub(super) flute: f64,
}

impl Radii {
    pub(super) fn dtheta(&self) -> f64 {
        TAU / self.nt_all as f64
    }

    /// The tool at `pose` cuts every ray it enters.
    pub(super) fn stamp(&mut self, p: &Profile, pose: &Pose) -> Result<Removed> {
        let mut out = Removed::default();
        let r = p.radius;
        // The frame turned so that the tool axis is +Z: work angle θ is at θ - α in it.
        let alpha = pose.axis.y.atan2(pose.axis.z);
        let (sa, ca) = alpha.sin_cos();
        let t = pose.tip;
        let (x, y, z) = (t.x, t.y * ca - t.z * sa, t.y * sa + t.z * ca);
        if z >= self.radius {
            return Ok(out);
        }
        let i0 = ((x - r - self.x0) / self.dx - 0.5).ceil();
        let i1 = ((x + r - self.x0) / self.dx - 0.5).floor();
        if !(i0 <= i1) || i1 < 0.0 || i0 > self.nx as f64 - 1.0 {
            return Ok(out);
        }
        let (i0, i1) = (index(i0, self.nx), index(i1, self.nx));
        // The angles the tool's box (sideways ±r, from its tip up to the stock's radius) is
        // seen at from the axis.
        let dt = self.dtheta();
        let (k0, k1) = if y - r <= 0.0 && y + r >= 0.0 && z <= 0.0 {
            (0i64, self.nt_all as i64 - 1)
        } else {
            let corners = [(y - r, z), (y + r, z), (y - r, self.radius), (y + r, self.radius)];
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for (cy, cz) in corners {
                let a = cy.atan2(cz);
                lo = lo.min(a);
                hi = hi.max(a);
            }
            let k0 = ((alpha + lo) / dt - 0.5).floor();
            let k1 = ((alpha + hi) / dt - 0.5).ceil();
            if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > 4.0 * self.nt_all as f64 {
                return Ok(out);
            }
            (k0 as i64, (k1 as i64).min(k0 as i64 + self.nt_all as i64 - 1))
        };
        let nt = self.nt_all as i64;
        let area = self.dx * dt * 0.5;
        let band = self.k0..self.k0 + self.nt;
        let mine = (k0..=k1).filter(|k| band.contains(&(k.rem_euclid(nt) as usize))).count();
        if mine == 0 {
            // None of this band's rays: another band's work.
            return Ok(out);
        }
        count(&self.visits, (mine as u64) * ((i1 - i0 + 1) as u64))?;
        for k in k0..=k1 {
            let Some(kk) = (k.rem_euclid(nt) as usize).checked_sub(self.k0).filter(|kk| *kk < self.nt) else { continue };
            let (Some(&st), Some(&ct)) = (self.sin.get(kk), self.cos.get(kk)) else { continue };
            // sin and cos of θ - α.
            let (s, c) = (st * ca - ct * sa, ct * ca + st * sa);
            if c <= 0.0 && z > 0.0 {
                continue;
            }
            for i in i0..=i1 {
                let cell = kk * self.nx + i;
                let Some(&have) = self.rho.get(cell) else { continue };
                if c > 0.0 && have * c <= z {
                    continue;
                }
                let cx = self.x0 + (i as f64 + 0.5) * self.dx;
                let Some((a, b)) = p.ray_span(cx - x, y, z, s, c, have) else { continue };
                let a = a.max(0.0);
                if !(a < have) {
                    continue;
                }
                let point = |v: f64| Vec3::new(cx, v * st, v * ct);
                // The tool reaches out of the material along the ray (to within half the
                // tolerance of its end): the ray ends where the tool enters it. Else it cut
                // beside the ray, inside the material: a stretch of air.
                let outward = b >= have - 0.5 * self.tol;
                let top = if outward { have } else { b };
                let (len, sq) = material(self.gaps.get(cell), have, a, top);
                if !(len > 0.0) {
                    continue;
                }
                if outward {
                    let end = match self.gaps.get_mut(cell) {
                        Some(g) => {
                            let before = g.len();
                            let end = cut_out(g, a);
                            self.gap_entries = self.gap_entries.saturating_sub(before - g.len());
                            let empty = g.is_empty();
                            if empty {
                                self.gaps.remove(cell);
                            }
                            end
                        }
                        None => a,
                    };
                    if let Some(slot) = self.rho.get_mut(cell) {
                        *slot = end;
                    }
                } else {
                    let full = self.gap_entries >= MAX_GAP_ENTRIES;
                    let Some(g) = self.gaps.entry(cell) else { continue };
                    if full || g.len() >= MAX_GAPS {
                        let empty = g.is_empty();
                        if empty {
                            self.gaps.remove(cell);
                        }
                        self.unmodelled += 1;
                        continue;
                    }
                    let before = g.len();
                    add_gap(g, a, b);
                    self.gap_entries = (self.gap_entries + g.len()).saturating_sub(before);
                }
                out.volume += sq * area;
                self.tiles.mark(i, kk);
                if let Some(l) = self.last.get_mut(cell) {
                    *l = self.slot;
                }
                // How deep it cut into the stock, and the material it cut standing above the
                // flutes (along the tool axis).
                if len > out.depth {
                    out.depth = len;
                    out.at = point(a);
                }
                let above = top * c - z - self.flute;
                if self.flute > 0.0 && above > self.tol && above > out.above {
                    out.above = above;
                    out.above_at = point(top);
                }
                // The deepest along the ray is measured again in 3D; of cuts as deep, the one
                // farthest from the stock's ends along X (where the part's end face is nearer
                // than the surface the tool cut into).
                let (x_lo, x_hi) = (self.x0, self.x0 + self.nx as f64 * self.dx);
                let inward = |px: f64| (px - x_lo).min(x_hi - px);
                if let Some(f) = &self.floor
                    && let Some((depth, at, from)) = f.part_cut(self.k0 * self.nx + cell, a, top, self.tol)
                    && (depth > out.gouge + 1e-9 || (depth > out.gouge - 1e-9 && inward(cx) > inward(out.gouge_at.x)))
                {
                    out.gouge = depth;
                    out.gouge_at = point(at);
                    out.gouge_from = point(from);
                }
            }
        }
        Ok(out)
    }

    /// Whether `p` is in the stock, when its angle is in this band (`None` when it is not).
    pub(super) fn owns(&self, p: Vec3) -> Option<bool> {
        let th = p.y.atan2(p.z).rem_euclid(TAU);
        let k = index(th / self.dtheta(), self.nt_all).checked_sub(self.k0).filter(|k| *k < self.nt)?;
        let fx = (p.x - self.x0) / self.dx;
        if !(fx >= 0.0 && fx < self.nx as f64) {
            return Some(false);
        }
        let cell = k * self.nx + index(fx, self.nx);
        let v = p.y.hypot(p.z);
        Some(self.rho.get(cell).is_some_and(|&r| v < r) && !in_gap(self.gaps.get(cell), v))
    }
}

/// How far the ray from the axis along `(0, s, c)` reaches inside the rectangle `[y0, y1, z0,
/// z1]` round the axis.
pub(super) fn section_reach(sec: [f64; 4], s: f64, c: f64) -> f64 {
    let [y0, y1, z0, z1] = sec;
    let along = |d: f64, lo: f64, hi: f64| {
        if d > 1e-15 {
            hi / d
        } else if d < -1e-15 {
            lo / d
        } else {
            f64::INFINITY
        }
    };
    along(s, y0, y1).min(along(c, z0, z1))
}

impl Profile {
    /// The least radius along the ray `(x, ρ·s, ρ·c)` (ρ from 0 to `cap`) inside the tool, its
    /// tip at `(x - dx, y, z)` and its axis +Z; `None` when the ray does not enter it below `cap`.
    #[cfg(test)]
    pub(crate) fn ray_entry(&self, dx: f64, y: f64, z: f64, s: f64, c: f64, cap: f64) -> Option<f64> {
        self.ray_span(dx, y, z, s, c, cap).map(|(a, _)| a)
    }

    /// The stretch of the ray `(x, ρ·s, ρ·c)` (ρ from 0 to `cap`) inside the tool, its tip at
    /// `(x - dx, y, z)` and its axis +Z: where the ray enters it and where it leaves it (the
    /// cutting end with the endless cylinder above it is convex, so it is one stretch); `None`
    /// when the ray does not enter it below `cap`.
    pub(crate) fn ray_span(&self, dx: f64, y: f64, z: f64, s: f64, c: f64, cap: f64) -> Option<(f64, f64)> {
        let r = self.radius;
        if dx.abs() > r {
            return None;
        }
        let w = (r * r - dx * dx).max(0.0).sqrt();
        // Where the ray is within the tool's radius, seen along the axis.
        let (lo, hi) = if s.abs() < 1e-15 {
            if y.abs() > w {
                return None;
            }
            (0.0, cap)
        } else {
            let (a, b) = ((y - w) / s, (y + w) / s);
            (a.min(b).max(0.0), a.max(b).min(cap))
        };
        if !(lo <= hi) {
            return None;
        }
        // Where the ray is above a plane at height `h` above the tip, within the radius.
        let above = |h: f64| -> Option<(f64, f64)> {
            let at = z + h;
            if c > 1e-15 {
                let p = at / c;
                (p <= hi).then(|| (p.max(lo), hi))
            } else if c < -1e-15 {
                let p = at / c;
                (p >= lo).then(|| (lo, p.min(hi)))
            } else {
                (at <= 0.0).then_some((lo, hi))
            }
        };
        match self.shape {
            Shape::Flat => above(0.0),
            Shape::Ball => {
                // The sphere about (dx, y, z + r) seen from the ray's start at the axis, and the
                // cylinder above its middle.
                let b = s * y + c * (z + r);
                let q = dx * dx + y * y + (z + r) * (z + r) - r * r;
                let disc = b * b - q;
                let sphere = (disc >= 0.0)
                    .then(|| {
                        let root = disc.sqrt();
                        let (a, e) = ((b - root).max(lo), (b + root).min(hi));
                        (a <= e).then_some((a, e))
                    })
                    .flatten();
                match (sphere, above(r)) {
                    (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
                    (a, b) => a.or(b),
                }
            }
            Shape::Bull | Shape::Cone(_) | Shape::Taper { .. } => {
                // Inside where g ≥ 0, g concave along the ray.
                let g = |p: f64| p * c - z - self.height(hyp(dx, p * s - y));
                let dg = |p: f64| {
                    let e = p * s - y;
                    let d = hyp(dx, e);
                    if d <= 1e-15 { c } else { c - self.slope(d) * s * e / d }
                };
                let top = if dg(lo) <= 0.0 {
                    lo
                } else if dg(hi) >= 0.0 {
                    hi
                } else {
                    let (mut a, mut b) = (lo, hi);
                    for _ in 0..BISECTIONS {
                        let mid = 0.5 * (a + b);
                        if dg(mid) > 0.0 {
                            a = mid;
                        } else {
                            b = mid;
                        }
                    }
                    0.5 * (a + b)
                };
                if g(top) < 0.0 {
                    return if g(lo) >= 0.0 { Some((lo, lo)) } else { None };
                }
                let entry = if g(lo) >= 0.0 {
                    lo
                } else {
                    let (mut a, mut b) = (lo, top);
                    for _ in 0..BISECTIONS {
                        let mid = 0.5 * (a + b);
                        if g(mid) < 0.0 {
                            a = mid;
                        } else {
                            b = mid;
                        }
                    }
                    b
                };
                let exit = if g(hi) >= 0.0 {
                    hi
                } else {
                    let (mut a, mut b) = (top, hi);
                    for _ in 0..BISECTIONS {
                        let mid = 0.5 * (a + b);
                        if g(mid) >= 0.0 {
                            a = mid;
                        } else {
                            b = mid;
                        }
                    }
                    a
                };
                Some((entry, exit.max(entry)))
            }
        }
    }
}

/// The part along the ray towards the axis from radius `out` at `x` along `(0, s, c)`: the
/// radius where it first meets it (NaN: never) and, unless that is all of it (one stretch from
/// the axis out to there), its stretches from the axis out (a stretch from `-∞` runs through the
/// axis). Where the crossings do not pair up (a part whose triangles face inwards, bodies that
/// overlap, more crossings than are followed) only the first is given.
pub(super) fn ray_stretches(part: &Surface, x: f64, s: f64, c: f64, out: f64) -> (f64, Option<Vec<[f64; 2]>>) {
    let dir = Vec3::new(0.0, s, c);
    let from = Vec3::new(x, 0.0, 0.0) + dir * out;
    let d = dir * -out;
    let facing = |i: usize| part.triangle(i).map_or(0.0, |tri| (tri[1] - tri[0]).cross(tri[2] - tri[0]).dot(d));
    let step = 1e-9 / out.max(1e-9);
    let (mut t0, mut inside, mut start, mut first) = (0.0f64, false, 0.0f64, f64::NAN);
    let mut list: Vec<[f64; 2]> = Vec::new();
    let mut ended = false;
    for _ in 0..MAX_CROSSINGS {
        let Some((t, i)) = part.raycast(from + d * t0, d) else {
            ended = true;
            break;
        };
        let at = t0 + t;
        if !(at <= 1.0) {
            ended = true;
            break;
        }
        let rho = out * (1.0 - at);
        if first.is_nan() {
            first = rho;
        }
        let f = facing(i);
        if f < 0.0 && !inside {
            (inside, start) = (true, rho);
        } else if f > 0.0 && inside {
            inside = false;
            list.push([rho, start]);
        } else if f != 0.0 {
            return (first, None);
        }
        t0 = at + step;
    }
    if !ended {
        return (first, None);
    }
    if inside {
        list.push([f64::NEG_INFINITY, start]);
    }
    list.reverse();
    match list.as_slice() {
        [] => (first, None),
        [one] if one[0] == f64::NEG_INFINITY && one[1] == first => (first, None),
        _ => (first, Some(list)),
    }
}

/// The most the part reaches from the A axis at each column's middle of the radial grid from
/// `x0`, `nx` columns `dx` wide: no more than its triangles there reach (the farthest point of a
/// triangle from the axis is a corner). A ray of a column the program never cut that already
/// ends this far out cannot be cut into the part, so it is not looked at.
pub(crate) fn column_reach(part: &Surface, x0: f64, dx: f64, nx: usize) -> Vec<f64> {
    let mut reach = vec![0.0f64; nx];
    if !(dx > 0.0) || nx == 0 {
        return reach;
    }
    for t in part.triangles() {
        let (lo, hi) = (t[0].x.min(t[1].x).min(t[2].x), t[0].x.max(t[1].x).max(t[2].x));
        let r = t.iter().map(|p| p.y.hypot(p.z)).fold(0.0f64, f64::max);
        let (a, b) = (((lo - x0) / dx - 0.5).ceil().max(0.0), ((hi - x0) / dx - 0.5).floor());
        if !(a <= b) || !a.is_finite() || !b.is_finite() || b < 0.0 {
            continue;
        }
        let (a, b) = (a as usize, (b as usize).min(nx - 1));
        for v in reach.iter_mut().take(b + 1).skip(a) {
            *v = v.max(r);
        }
    }
    reach
}

/// The tool positions of `step` on a 4th axis with indices `ks` (at `k / n`), one at a time (a
/// block may have up to [`MAX_BLOCK_POINTS`], and every band walks them).
pub(super) fn each_pose(machine: &Machine, step: &Step, ks: std::ops::RangeInclusive<usize>, mut f: impl FnMut(&Pose) -> Result<()>) -> Result<()> {
    let n = step.points.max(1);
    for k in ks {
        f(&step.pose_frac(machine, k as f64 / n as f64))?;
    }
    Ok(())
}

/// Tool positions for a travel of `steps` sample steps.
pub(super) fn points(steps: f64) -> Result<usize> {
    let n = steps.ceil().max(1.0);
    if !(n <= MAX_BLOCK_POINTS as f64) {
        return Err(CamError::TooLarge("a block needs too many tool positions to simulate: give a larger tolerance".into()));
    }
    Ok(n as usize)
}

impl PartFloor {
    /// How far the cut `[a, b]` along ray `cell` goes into the part along the ray, the deepest
    /// point of it (a radius) and where a column measuring it in 3D starts (outside the part
    /// when the cut came from outside it, else at the cut's end): `None` when not more than
    /// `tol`.
    pub(super) fn part_cut(&self, cell: usize, a: f64, b: f64, tol: f64) -> Option<(f64, f64, f64)> {
        let mut best: Option<(f64, f64, f64)> = None;
        let mut take = |p0: f64, p1: f64| {
            let (o0, o1) = (a.max(p0), b.min(p1));
            if !(o0 < o1) {
                return;
            }
            let (depth, at) = if p0 == f64::NEG_INFINITY {
                (p1 - o0, o0)
            } else {
                let m = 0.5 * (p0 + p1);
                let q = m.clamp(o0, o1);
                ((q - p0).min(p1 - q), q)
            };
            let from = if b >= p1 { p1 + tol } else { b };
            if depth > tol && best.is_none_or(|(d, _, _)| depth > d) {
                best = Some((depth, at, from));
            }
        };
        match self.stretches.get(&cell) {
            Some(list) => list.iter().for_each(|s| take(s[0], s[1])),
            None => {
                let outer = self.outer.get(cell).copied().unwrap_or(f64::NAN);
                if outer.is_finite() {
                    take(f64::NEG_INFINITY, outer);
                }
            }
        }
        best
    }
}

impl SimState {
    /// The stretches of air inside the material of ray `(i, k)` (a 4th axis's stock; none on a
    /// heightfield).
    pub fn gaps(&self, plan: &SimPlan, i: usize, k: usize) -> &[[f64; 2]] {
        let nx = plan.grid.columns();
        if i >= nx {
            return &[];
        }
        match self.band_of(k) {
            Some((Model::Radii(r), kk)) => r.gaps.get(kk * nx + i).map_or(&[], Vec::as_slice),
            _ => &[],
        }
    }
}
