//! The stock along X and along Y beside the height map (a tri-dexel stock), for a part with
//! faces under its own material along Z, and what those lines cut into the part.

use super::*;

/// Most lines in each set of [`SideDexels`]: they are the grid's cell apart, farther past it.
pub(super) const SIDE_LINES: usize = 250_000;
/// Most stretches of material one line of [`SideDexels`] holds: a cut that would split one
/// further is left out of that line (counted, and said in a warning).
pub(super) const MAX_SIDE_PIECES: usize = 64;
/// Most line visits the side lines of one simulation make: past it they are given up (said in
/// a warning) and the simulation goes on with the height map alone.
pub(super) const SIDE_VISITS: u64 = 1_500_000_000;
/// Most pieces one straight move is split into for [`SideDexels`] as it moves in Z.
pub(super) const MAX_SIDE_SPLITS: usize = 4096;
/// Steps a simulation with side lines cuts at a time ([`SimState::top_round`]).
pub(super) const SIDE_RUN: usize = 256;
/// Vertical rays [`hides_faces`] casts each way over the stock's footprint.
pub(super) const HIDDEN_PROBES: usize = 128;
/// The longest piece of a side line's leftover looked at as one (mm): each is held against the
/// part above and below its middle on its own.
const SIDE_CHUNK: f64 = 2.0;

/// One set of side lines of the stock as a simulation left it ([`SimReport::sides`]): lines
/// along X (`axis` 0) or Y (1) from `lo` to `hi`, line `k·nu + i` at `u0 + (i + ½)·du` across (Y
/// for lines along X, X for lines along Y) and `z0 + (k + ½)·dz` up, each its stretches of
/// material along it in order.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SideLines {
    pub axis: usize,
    pub lo: f64,
    pub hi: f64,
    pub u0: f64,
    pub du: f64,
    pub nu: usize,
    pub z0: f64,
    pub dz: f64,
    pub nz: usize,
    pub lines: Vec<Vec<[f64; 2]>>,
}

impl SideLines {
    pub(super) fn of(set: &DexelSet) -> SideLines {
        let DexelSet { axis, lo, hi, u0, du, nu, z0, dz, nz, .. } = *set;
        SideLines { axis, lo, hi, u0, du, nu, z0, dz, nz, lines: set.lines.clone() }
    }

    /// Why these lines cannot be read (a file's), if they cannot.
    pub fn check(&self) -> std::result::Result<(), String> {
        let finite = [self.lo, self.hi, self.u0, self.du, self.z0, self.dz].iter().all(|v| v.is_finite() && v.abs() <= 1e6);
        if self.axis > 1 || !finite || !(self.du > 0.0 && self.dz > 0.0 && self.hi > self.lo) {
            return Err("the side lines' axis must be X or Y and their ends and spacing finite, the spacing positive".into());
        }
        if self.nu.checked_mul(self.nz) != Some(self.lines.len()) || self.lines.len() > 4 * SIDE_LINES {
            return Err("the side lines must be `nu` times `nz` lines, at most a million".into());
        }
        let ordered = |l: &Vec<[f64; 2]>| {
            l.len() <= MAX_SIDE_PIECES
                && l.iter().all(|p| p[0].is_finite() && p[1].is_finite() && p[0] <= p[1])
                && l.windows(2).all(|w| w[0][1] <= w[1][0])
        };
        if !self.lines.iter().all(ordered) {
            return Err(format!("each side line must be at most {MAX_SIDE_PIECES} finite stretches in order"));
        }
        Ok(())
    }

    /// Whether material lies within `r` of `p` along the lines that pass within `r` of it
    /// (across and up, each line standing for its row); `None` where none does (off the lines).
    pub fn holds_near(&self, p: Vec3, r: f64) -> Option<bool> {
        let (t, u) = if self.axis == 0 { (p.x, p.y) } else { (p.y, p.x) };
        if !(t >= self.lo - r && t <= self.hi + r) {
            return None;
        }
        let iu = line_range(self.u0, self.du, self.nu, u - r - 0.5 * self.du, u + r + 0.5 * self.du)?;
        let ku = line_range(self.z0, self.dz, self.nz, p.z - r - 0.5 * self.dz, p.z + r + 0.5 * self.dz)?;
        let near = |l: &Vec<[f64; 2]>| {
            let s = l.partition_point(|q| q[1] < t - r);
            l.get(s).is_some_and(|q| q[0] <= t + r)
        };
        Some((ku.0..=ku.1).any(|k| (iu.0..=iu.1).any(|i| self.lines.get(k * self.nu + i).is_some_and(near))))
    }
}

/// One set of [`SideDexels`]: lines along X (`axis` 0) or Y (1) through the stock, line
/// `k·nu + i` at `u0 + (i + ½)·du` across (Y for lines along X, X for lines along Y) and
/// `z0 + (k + ½)·dz` up, each its stretches of material along it in order.
#[derive(Clone)]
pub(super) struct DexelSet {
    pub(super) axis: usize,
    pub(super) lo: f64,
    pub(super) hi: f64,
    pub(super) u0: f64,
    pub(super) du: f64,
    pub(super) nu: usize,
    pub(super) z0: f64,
    pub(super) dz: f64,
    pub(super) nz: usize,
    pub(super) lines: Vec<Vec<[f64; 2]>>,
    /// Cuts left out because a line held [`MAX_SIDE_PIECES`] stretches.
    pub(super) unmodelled: u64,
}

impl DexelSet {
    /// The point `t` along line `(i, k)`.
    pub(super) fn point(&self, i: usize, k: usize, t: f64) -> Vec3 {
        let (u, z) = (self.u0 + (i as f64 + 0.5) * self.du, self.z0 + (k as f64 + 0.5) * self.dz);
        if self.axis == 0 { Vec3::new(t, u, z) } else { Vec3::new(u, t, z) }
    }

    /// How far from line `(i, k)` the lines next to it up (`way` 1) or down (-1) hold material
    /// at `t`, from the middle of its height, at most `cap`.
    pub(super) fn material_up(&self, i: usize, k: usize, t: f64, cap: f64, way: i64) -> f64 {
        let holds = |k: usize| {
            self.lines.get(k * self.nu + i).is_some_and(|l| {
                let s = l.partition_point(|p| p[1] < t);
                l.get(s).is_some_and(|p| p[0] <= t)
            })
        };
        let mut n = 0usize;
        let mut kk = k;
        while (n as f64 + 0.5) * self.dz < cap {
            let Some(next) = kk.checked_add_signed(way as isize).filter(|&q| q < self.nz) else { break };
            if !holds(next) {
                break;
            }
            (kk, n) = (next, n + 1);
        }
        ((n as f64 + 0.5) * self.dz).min(cap)
    }

    pub(super) fn dir(&self) -> Vec3 {
        if self.axis == 0 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) }
    }
}

/// The indices of the lines (cells) at `o + (i + ½)·d` within `[lo, hi]`, `i < n`.
pub(super) fn line_range(o: f64, d: f64, n: usize, lo: f64, hi: f64) -> Option<(usize, usize)> {
    let a = ((lo - o) / d - 0.5).ceil();
    let b = ((hi - o) / d - 0.5).floor();
    if !(a <= b) || b < 0.0 || a > (n as f64 - 1.0) {
        return None;
    }
    Some((index(a, n), index(b, n)))
}

/// The stock along X and along Y beside the height map along Z (a tri-dexel stock), for a part
/// with faces under its own material along Z (an undercut, a hole across it, a side setup's
/// part): the height map holds one top per column, so it cannot tell air under material from
/// material, nor what is left there from what was cut. Each line keeps its stretches of material;
/// every block of the program cuts them as it cuts the height map (a level move exactly: the line
/// against the capsule the tool's section sweeps at the line's height; a move in Z in pieces at
/// most half the tolerance long in Z or sideways, each cut by the tool where it is lowest and by
/// its section at the piece's top over the whole piece, both inside what the tool sweeps). The
/// whole-part check takes from them what is left under the part's own material both from above
/// and from below (no setup along Z reaches it), and the gouge check what is cut into the part
/// there.
#[derive(Clone)]
pub(super) struct SideDexels {
    pub(super) sets: Vec<DexelSet>,
    /// Lines visited so far, at most [`SIDE_VISITS`].
    pub(super) visits: u64,
    /// The height map's grid and cell they were built for.
    pub(super) grid: Grid,
    pub(super) cell: f64,
}

/// Grows `span` to hold `[a, b]`.
pub(super) fn hull(span: &mut Option<(f64, f64)>, (a, b): (f64, f64)) {
    *span = Some(span.map_or((a, b), |s| (s.0.min(a), s.1.max(b))));
}

/// Where the line at `v` across (running along `t`) is within `rho` of the segment from `p` to
/// `q` (each `[t, across]`): one stretch, the capsule being convex.
pub(super) fn capsule_span(p: [f64; 2], q: [f64; 2], v: f64, rho: f64) -> Option<(f64, f64)> {
    let mut span = None;
    for c in [p, q] {
        let w = rho * rho - (v - c[1]) * (v - c[1]);
        if w >= 0.0 {
            let w = w.sqrt();
            hull(&mut span, (c[0] - w, c[0] + w));
        }
    }
    let (dt, dv) = (q[0] - p[0], q[1] - p[1]);
    let l2 = dt * dt + dv * dv;
    if l2 > 1e-18 {
        let l = l2.sqrt();
        let ev = v - p[1];
        // Within `rho` of the segment's line ...
        let (mut lo, mut hi) = if dv.abs() > 1e-12 {
            let (a, b) = (p[0] + (dt * ev - rho * l) / dv, p[0] + (dt * ev + rho * l) / dv);
            (a.min(b), a.max(b))
        } else if (dt * ev).abs() <= rho * l {
            (f64::NEG_INFINITY, f64::INFINITY)
        } else {
            (1.0, 0.0)
        };
        // ... and beside the segment, not past its ends.
        if dt.abs() > 1e-12 {
            let (a, b) = (p[0] - dv * ev / dt, p[0] + (l2 - dv * ev) / dt);
            (lo, hi) = (lo.max(a.min(b)), hi.min(a.max(b)));
        } else if !(0.0..=l2).contains(&(dv * ev)) {
            (lo, hi) = (1.0, 0.0);
        }
        if lo <= hi && lo.is_finite() && hi.is_finite() {
            hull(&mut span, (lo, hi));
        }
    }
    span
}

/// Takes `[a, b]` out of the stretches `list` (in order); false, taking nothing, when that would
/// leave the line more than [`MAX_SIDE_PIECES`] stretches.
pub(super) fn take_out(list: &mut Vec<[f64; 2]>, a: f64, b: f64) -> bool {
    if !(a < b) {
        return true;
    }
    let s = list.partition_point(|p| p[1] <= a);
    let e = list.partition_point(|p| p[0] < b);
    if s >= e {
        return true;
    }
    let (Some(&first), Some(&last)) = (list.get(s), list.get(e - 1)) else { return true };
    let mut keep: Vec<[f64; 2]> = Vec::with_capacity(2);
    if first[0] < a {
        keep.push([first[0], a]);
    }
    if last[1] > b {
        keep.push([b, last[1]]);
    }
    if keep.len() > e - s && list.len() + keep.len() - (e - s) > MAX_SIDE_PIECES {
        return false;
    }
    list.splice(s..e, keep);
    true
}

/// Puts `[a, b]` into the stretches `list` (in order), joined with those it meets; false,
/// putting nothing, when that would leave the line more than [`MAX_SIDE_PIECES`] stretches.
pub(super) fn put_in(list: &mut Vec<[f64; 2]>, a: f64, b: f64) -> bool {
    if !(a < b) {
        return true;
    }
    // The stretches from `s` up to `e` meet `[a, b]`.
    let s = list.partition_point(|p| p[1] < a);
    let e = list.partition_point(|p| p[0] <= b).max(s);
    let (lo, hi) = match (list.get(s), e.checked_sub(1).and_then(|l| list.get(l))) {
        (Some(f), Some(l)) if s < e => (f[0].min(a), l[1].max(b)),
        _ => (a, b),
    };
    if s == e && list.len() >= MAX_SIDE_PIECES {
        return false;
    }
    list.splice(s..e, [[lo, hi]]);
    true
}

impl SideDexels {
    /// The lines through the stock as `bands` hold it (each cell's material from its `under`, or
    /// the stock's bottom, up to its height); `None` off a height map.
    pub(super) fn new(grid: &Grid, bands: &[Model], cell: f64) -> Option<SideDexels> {
        let Grid::Heights { x0, y0, dx, dy, nx, ny, bottom, top } = *grid else { return None };
        if !(top > bottom && cell > 0.0 && nx > 0 && ny > 0) {
            return None;
        }
        // Every cell's material, row after row.
        let mut cols: Vec<(f64, f64)> = Vec::with_capacity(nx * ny);
        for b in bands {
            let Model::Heights(h) = b else { return None };
            for (c, &z) in h.h.iter().enumerate() {
                cols.push((h.under.get(c).copied().unwrap_or(h.bottom), z));
            }
        }
        if cols.len() != nx * ny {
            return None;
        }
        let whole = cols.iter().all(|&(u, z)| u <= bottom && z >= top);
        let (xs, ys) = (dx * nx as f64, dy * ny as f64);
        let mut sets = Vec::with_capacity(2);
        for axis in [0usize, 1] {
            let (lo, hi, u0, span) = if axis == 0 { (x0, x0 + xs, y0, ys) } else { (y0, y0 + ys, x0, xs) };
            let height = top - bottom;
            let c = if (span / cell) * (height / cell) > SIDE_LINES as f64 { (span * height / SIDE_LINES as f64).sqrt() } else { cell };
            let (nu, nz) = (((span / c).ceil() as usize).max(1), ((height / c).ceil() as usize).max(1));
            let (du, dz) = (span / nu as f64, height / nz as f64);
            let (n, d) = if axis == 0 { (nx, dx) } else { (ny, dy) };
            let mut lines = Vec::with_capacity(nu * nz);
            for k in 0..nz {
                let z = bottom + (k as f64 + 0.5) * dz;
                for i in 0..nu {
                    if whole {
                        lines.push(vec![[lo, hi]]);
                        continue;
                    }
                    let u = u0 + (i as f64 + 0.5) * du;
                    let fixed = if axis == 0 { index((u - y0) / dy, ny) } else { index((u - x0) / dx, nx) };
                    let mut line: Vec<[f64; 2]> = Vec::new();
                    for m in 0..n {
                        let c = if axis == 0 { fixed * nx + m } else { m * nx + fixed };
                        let Some(&(under, h)) = cols.get(c) else { continue };
                        if under <= z && z <= h {
                            let (a, b) = (lo + m as f64 * d, lo + (m + 1) as f64 * d);
                            match line.last_mut() {
                                Some(last) if last[1] >= a - 1e-9 => last[1] = b,
                                _ => line.push([a, b]),
                            }
                        }
                    }
                    lines.push(line);
                }
            }
            sets.push(DexelSet { axis, lo, hi, u0, du, nu, z0: bottom, dz, nz, lines, unmodelled: 0 });
        }
        Some(SideDexels { sets, visits: 0, grid: *grid, cell })
    }

    /// Cuts the tool sweeping straight from tip `p` to tip `q` (its axis +Z) out of every line.
    pub(super) fn cut_piece(&mut self, prof: &Profile, p: Vec3, q: Vec3, top: f64) -> Result<()> {
        let low = if p.z <= q.z { p } else { q };
        let z_hi = p.z.max(q.z);
        let r = prof.radius;
        for set in &mut self.sets {
            let (ax, ac) = (set.axis, 1 - set.axis);
            let (pp, qq, ll) = ([comp(p, ax), comp(p, ac)], [comp(q, ax), comp(q, ac)], [comp(low, ax), comp(low, ac)]);
            let Some((i0, i1)) = line_range(set.u0, set.du, set.nu, pp[1].min(qq[1]) - r, pp[1].max(qq[1]) + r) else { continue };
            let Some((k0, k1)) = line_range(set.z0, set.dz, set.nz, low.z, top) else { continue };
            self.visits = self.visits.saturating_add(((i1 - i0 + 1) * (k1 - k0 + 1)) as u64);
            if self.visits > SIDE_VISITS {
                return Err(CamError::TooLarge("the side lines of the stock take too long to cut".into()));
            }
            for k in k0..=k1 {
                let z = set.z0 + (k as f64 + 0.5) * set.dz;
                let (cap, disc) = (prof.section(z - z_hi), prof.section(z - low.z));
                for i in i0..=i1 {
                    let Some(line) = set.lines.get_mut(k * set.nu + i) else { continue };
                    if line.is_empty() {
                        continue;
                    }
                    let u = set.u0 + (i as f64 + 0.5) * set.du;
                    let mut span = None;
                    if let Some(s) = cap.and_then(|rho| capsule_span(pp, qq, u, rho)) {
                        hull(&mut span, s);
                    }
                    if let Some(s) = disc.and_then(|rho| capsule_span(ll, ll, u, rho)) {
                        hull(&mut span, s);
                    }
                    if let Some((a, b)) = span
                        && !take_out(line, a, b)
                    {
                        set.unmodelled += 1;
                    }
                }
            }
        }
        Ok(())
    }

    /// Cuts the tool moving straight from tip `a` to tip `b`, in pieces as it moves in Z.
    pub(super) fn cut_move(&mut self, prof: &Profile, a: Vec3, b: Vec3, tol: f64, top: f64) -> Result<()> {
        let (lxy, dz) = (hyp(b.x - a.x, b.y - a.y), (b.z - a.z).abs());
        let n = ((lxy.min(dz) / (0.5 * tol)).ceil().min(MAX_SIDE_SPLITS as f64) as usize).max(1);
        let mut prev = a;
        for m in 1..=n {
            let next = if m == n { b } else { a.lerp(b, m as f64 / n as f64) };
            self.cut_piece(prof, prev, next, top)?;
            prev = next;
        }
        Ok(())
    }

    /// Cuts the part of `step` from fraction `f0` to `f1` of the way along (arcs as chords at most
    /// a quarter of the tolerance from them).
    pub(super) fn cut_step(&mut self, step: &Step, machine: &Machine, (f0, f1): (f64, f64), tol: f64, top: f64) -> Result<()> {
        if !(f1 > f0) {
            return Ok(());
        }
        let n = match &step.how {
            How::Straight => 1,
            How::Arc { work_center, sweep, .. } => {
                let r = hyp(step.start.tip.x - work_center.x, step.start.tip.y - work_center.y);
                let c = (1.0 - 0.25 * tol / r.max(1e-9)).clamp(-1.0, 1.0);
                let chord = (2.0 * c.acos()).clamp(1e-6, 0.5 * PI);
                ((sweep.abs() * (f1 - f0) / chord).ceil().min(MAX_BLOCK_POINTS as f64) as usize).max(1)
            }
            How::Tilted { chords, .. } => (*chords).max(1),
            How::Joint { .. } => step.points.max(1),
        };
        let at = |f: f64| -> Vec3 {
            if f <= 0.0 {
                step.start.tip
            } else if f >= 1.0 {
                step.end.tip
            } else if matches!(step.how, How::Straight) {
                step.start.tip.lerp(step.end.tip, f)
            } else {
                step.pose_frac(machine, f).tip
            }
        };
        let mut prev = at(f0);
        for m in 1..=n {
            let next = at(f0 + (f1 - f0) * m as f64 / n as f64);
            self.cut_move(&step.profile, prev, next, tol, top)?;
            prev = next;
        }
        Ok(())
    }

    pub(super) fn unmodelled(&self) -> u64 {
        self.sets.iter().map(|s| s.unmodelled).sum()
    }

    /// What the lines leave under the part's own material both from above and from below along
    /// Z (no setup along Z reaches it), as [`PartLeft`] pieces (each set's half of the volume),
    /// and what they cut into the part there, as a gouge (the worst, where, and how many pieces).
    pub(super) fn compare(&self, part: &Surface, tol: f64) -> (PartLeft, f64, Vec3, usize) {
        let mut left = PartLeft::default();
        let (mut worst, mut worst_at, mut gouged) = (0.0f64, Vec3::ZERO, 0usize);
        let b = part.bounds();
        let reach = (b.max - b.min).len() + 1.0;
        let up = Vec3::new(0.0, 0.0, reach);
        // How far the part is above and below `p` along Z, when it is both.
        let over_under = |p: Vec3| Some((part.raycast(p, up)?.0 * reach, part.raycast(p, up * -1.0)?.0 * reach));
        let enclosed = |p: Vec3| over_under(p).is_some();
        for set in &self.sets {
            let stride = (((set.nu * set.nz) as f64 / (0.5 * WHOLE_SAMPLES as f64)).sqrt().ceil() as usize).max(1);
            let allowance = tol + set.du.max(set.dz);
            let area = set.du * set.dz * (stride * stride) as f64 * 0.5;
            let (ax, ac) = (set.axis, 1 - set.axis);
            let (pa, pb) = (comp(b.min, ax).max(set.lo), comp(b.max, ax).min(set.hi));
            if !(pa < pb) {
                continue;
            }
            let (from, len) = (pa - 1.0, pb - pa + 2.0);
            for k in (0..set.nz).step_by(stride) {
                for i in (0..set.nu).step_by(stride) {
                    let Some(line) = set.lines.get(k * set.nu + i) else { continue };
                    let mid = set.point(i, k, 0.0);
                    let across = comp(mid, ac);
                    if mid.z <= b.min.z || mid.z >= b.max.z || across <= comp(b.min, ac) || across >= comp(b.max, ac) {
                        continue;
                    }
                    // A line meeting no part may still run under it (along a slot through it).
                    let Some(list) = segment_stretches(part, set.point(i, k, from), set.dir() * len) else { continue };
                    let stretches: Vec<[f64; 2]> = list.iter().map(|s| [from + s[0] * len, from + s[1] * len]).collect();
                    let point = |t: f64| set.point(i, k, t);
                    // Material outside the part, within the part's extent along the line, in
                    // pieces each held against the part above and below its middle.
                    let material: Vec<[f64; 2]> = line.iter().filter(|m| m[1].min(pb) > m[0].max(pa)).map(|m| [m[0].max(pa), m[1].min(pb)]).collect();
                    for (o, _) in outside_part(&material, &stretches) {
                        if o[1] - o[0] <= allowance {
                            continue;
                        }
                        let pieces = ((o[1] - o[0]) / SIDE_CHUNK).ceil().max(1.0) as usize;
                        let piece = (o[1] - o[0]) / pieces as f64;
                        for c in 0..pieces {
                            let (a, e) = (o[0] + c as f64 * piece, o[0] + (c + 1) as f64 * piece);
                            let t = 0.5 * (a + e);
                            let m = point(t);
                            let Some((over, under)) = over_under(m) else { continue };
                            // As thick as it is along the line, or up the lines over and under
                            // it that hold material there, to the part.
                            let ends = distance_within(part, point(a), piece).max(distance_within(part, point(e), piece));
                            let along = ends.max(2.0 * distance_within(part, m, piece)).min(piece);
                            let thick = along.max(set.material_up(i, k, t, over, 1) + set.material_up(i, k, t, under, -1));
                            if thick > allowance {
                                let volume = piece * area;
                                left.volume += volume;
                                left.hidden_volume += volume;
                                left.spot(thick, m);
                                left.pieces.push(LeftPiece { from: point(a), to: point(e), volume, thick, hidden: true, enclosed: true });
                            }
                        }
                    }
                    // Air cut into the part where the height map cannot see.
                    let mut at = set.lo;
                    let mut air: Vec<[f64; 2]> = Vec::new();
                    for m in line {
                        if m[0] > at {
                            air.push([at, m[0]]);
                        }
                        at = at.max(m[1]);
                    }
                    if set.hi > at {
                        air.push([at, set.hi]);
                    }
                    for g in &air {
                        for s in &stretches {
                            let (a, e) = (g[0].max(s[0]), g[1].min(s[1]));
                            if e - a <= tol {
                                continue;
                            }
                            let m = point(0.5 * (a + e));
                            if !enclosed(m) {
                                continue;
                            }
                            let span = e - a;
                            let d =
                                distance_within(part, m, span).max(distance_within(part, point(a), span)).max(distance_within(part, point(e), span));
                            if d > tol {
                                gouged += 1;
                                if d > worst {
                                    (worst, worst_at) = (d, m);
                                }
                            }
                        }
                    }
                }
            }
        }
        (left, worst, worst_at, gouged)
    }
}

/// Whether `part` has faces under its own material along Z over `grid`'s footprint: a vertical
/// ray every so often (about [`HIDDEN_PROBES`] each way) that meets more than one stretch of it.
pub(super) fn hides_faces(part: &Surface, grid: &Grid) -> bool {
    let Grid::Heights { x0, y0, dx, dy, nx, ny, bottom, top } = *grid else { return false };
    if part.is_empty() {
        return false;
    }
    let b = part.bounds();
    let (lx, hx) = (b.min.x.max(x0), b.max.x.min(x0 + dx * nx as f64));
    let (ly, hy) = (b.min.y.max(y0), b.max.y.min(y0 + dy * ny as f64));
    if !(lx < hx && ly < hy) {
        return false;
    }
    let from = top.max(b.max.z) + 1.0;
    let span = from - bottom.min(b.min.z) + 1.0;
    let n = HIDDEN_PROBES as f64;
    (0..HIDDEN_PROBES).any(|a| {
        (0..HIDDEN_PROBES).any(|c| {
            let (x, y) = (lx + (a as f64 + 0.5) / n * (hx - lx), ly + (c as f64 + 0.5) / n * (hy - ly));
            segment_stretches(part, Vec3::new(x, y, from), Vec3::new(0.0, 0.0, -span)).is_some_and(|l| l.len() > 1)
        })
    })
}

/// The part's stretches along the segment from `from` to `from + d`, as fractions of it in order
/// from `from`: `[enter, leave]` (a stretch the segment ends inside leaves at 1). `None` where the
/// crossings do not pair up (triangles facing inwards, bodies that overlap, too many crossings).
pub(super) fn segment_stretches(part: &Surface, from: Vec3, d: Vec3) -> Option<Vec<[f64; 2]>> {
    let facing = |i: usize| part.triangle(i).map_or(0.0, |tri| (tri[1] - tri[0]).cross(tri[2] - tri[0]).dot(d));
    let step = 1e-9 / d.len().max(1e-9);
    let (mut t0, mut inside, mut start) = (0.0f64, false, 0.0f64);
    let mut list: Vec<[f64; 2]> = Vec::new();
    for _ in 0..MAX_CROSSINGS {
        let Some((t, i)) = part.raycast(from + d * t0, d) else {
            if inside {
                list.push([start, 1.0]);
            }
            return Some(list);
        };
        let at = t0 + t;
        if !(at <= 1.0) {
            if inside {
                list.push([start, 1.0]);
            }
            return Some(list);
        }
        let f = facing(i);
        if f < 0.0 && !inside {
            (inside, start) = (true, at);
        } else if f > 0.0 && inside {
            inside = false;
            list.push([start, at]);
        } else if f != 0.0 {
            return None;
        }
        t0 = at + step;
    }
    None
}

/// The pieces of `material` (`[low, high]` heights or radii, in order) outside the part's
/// `stretches` (the same measure), each with whether part material stands over it (`true`: under
/// the part, where no tool of the setup reaches).
pub(super) fn outside_part(material: &[[f64; 2]], stretches: &[[f64; 2]]) -> Vec<([f64; 2], bool)> {
    let top = stretches.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let mut out = Vec::new();
    for piece in material {
        let mut a = piece[0];
        let mut within: Vec<&[f64; 2]> = stretches.iter().filter(|p| p[1] > piece[0] && p[0] < piece[1]).collect();
        within.sort_by(|x, y| x[0].total_cmp(&y[0]));
        for p in within {
            if p[0] > a {
                let o = [a, p[0].min(piece[1])];
                out.push((o, o[1] <= top));
            }
            a = a.max(p[1]);
        }
        if piece[1] > a {
            out.push(([a, piece[1]], a < top));
        }
    }
    out
}

/// `main` here and `side` on `s`, when there is one, on a thread of its own when `threaded` and
/// one can be had (else after `main`): what `main` gives, and whether `side` went through (false
/// too when its thread could not be had or was lost).
pub(super) fn beside<R>(
    s: Option<&mut SideDexels>,
    threaded: bool,
    side: impl Fn(&mut SideDexels) -> bool + Sync,
    main: impl FnOnce() -> R,
) -> (R, bool) {
    let Some(s) = s else { return (main(), true) };
    #[cfg(not(target_arch = "wasm32"))]
    if threaded {
        let side = &side;
        return std::thread::scope(|scope| match std::thread::Builder::new().name("cam-sim-sides".into()).spawn_scoped(scope, move || side(s)) {
            Ok(h) => {
                let r = main();
                (r, h.join().unwrap_or(false))
            }
            Err(_) => (main(), false),
        });
    }
    #[cfg(target_arch = "wasm32")]
    let _ = threaded;
    let r = main();
    (r, side(s))
}

/// The cells of a 3-axis grid a cutting step may reach (round its tips' sweep in XY by the
/// tool's radius and a cell); `None` off a height map, for a step wholly above the stock or off
/// the grid, or one it cannot bound.
pub(super) fn step_cells(grid: &Grid, step: &Step) -> Option<((usize, usize), (usize, usize))> {
    let Grid::Heights { x0, y0, dx, dy, nx, ny, top, .. } = *grid else { return None };
    let (a, b) = (step.start.tip, step.end.tip);
    if a.z.min(b.z) > top {
        return None;
    }
    let (mut lo, mut hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
    match &step.how {
        How::Straight => {}
        How::Arc { work_center, .. } => {
            let r = hyp(a.x - work_center.x, a.y - work_center.y);
            lo = Vec2::new(lo.x.min(work_center.x - r), lo.y.min(work_center.y - r));
            hi = Vec2::new(hi.x.max(work_center.x + r), hi.y.max(work_center.y + r));
        }
        _ => return None,
    }
    let w = step.profile.radius + dx.max(dy);
    let (fx0, fx1) = ((lo.x - w - x0) / dx, (hi.x + w - x0) / dx);
    let (fy0, fy1) = ((lo.y - w - y0) / dy, (hi.y + w - y0) / dy);
    if ![fx0, fx1, fy0, fy1].iter().all(|v| v.is_finite()) || fx1 < 0.0 || fy1 < 0.0 || fx0 > nx as f64 || fy0 > ny as f64 {
        return None;
    }
    Some(((index(fx0, nx), index(fx1, nx)), (index(fy0, ny), index(fy1, ny))))
}

impl SimState {
    /// Keeps the stock along X and Y as well as along Z from here on (a tri-dexel stock), for a part with faces under its own material along Z: what is left there and what
    /// is cut into the part there is then held against the part in [`SimState::finish`]. Built
    /// from the stock as it is now (best before any step is cut: a cut made before is in the
    /// height map only). False on a 4th axis's stock, which keeps its air already.
    pub fn track_sides(&mut self, plan: &SimPlan) -> bool {
        self.side = SideDexels::new(&plan.grid, &self.bands, plan.cell).map(Box::new);
        self.side.is_some()
    }

    /// The material the side lines hold along Z over `(x, y)` (work coordinates), bottom up:
    /// what is at a point is what every set of lines holds there (each line nearest it), read
    /// at the middle of each layer of the finest set. `None` without side lines (they keep the
    /// stock only for a part with faces under its own material along Z); with them given up, as
    /// they stood then (no less than the material left). For a view: material under a cut the
    /// height map cannot show, and air under material it shows as solid.
    pub fn side_column(&self, x: f64, y: f64) -> Option<Vec<[f64; 2]>> {
        let side = self.side.as_deref().or(self.given_up.as_deref())?;
        let base = side.sets.iter().min_by(|a, b| a.dz.total_cmp(&b.dz))?;
        let holds = |set: &DexelSet, z: f64| {
            let (u, t) = if set.axis == 0 { (y, x) } else { (x, y) };
            let (i, k) = (index((u - set.u0) / set.du, set.nu), index((z - set.z0) / set.dz, set.nz));
            set.lines.get(k * set.nu + i).is_some_and(|l| {
                let s = l.partition_point(|p| p[1] < t);
                l.get(s).is_some_and(|p| p[0] <= t)
            })
        };
        let mut out: Vec<[f64; 2]> = Vec::new();
        for k in 0..base.nz {
            let (lo, hi) = (base.z0 + k as f64 * base.dz, base.z0 + (k + 1) as f64 * base.dz);
            let z = 0.5 * (lo + hi);
            if !side.sets.iter().all(|s| holds(s, z)) {
                continue;
            }
            match out.last_mut() {
                Some(last) if last[1] >= lo - 1e-9 => last[1] = hi,
                _ => out.push([lo, hi]),
            }
        }
        Some(out)
    }

    /// The finest layer of the side lines along Z (mm), when they are kept ([`SimState::side_column`]).
    pub fn side_layer(&self) -> Option<f64> {
        let side = self.side.as_deref().or(self.given_up.as_deref())?;
        side.sets.iter().map(|s| s.dz).min_by(f64::total_cmp)
    }

    /// The stretches of material of the side line along X (`axis` 0) or Y (1) nearest `p`.
    #[cfg(test)]
    pub(crate) fn side_line(&self, axis: usize, p: Vec3) -> Option<Vec<[f64; 2]>> {
        let set = self.side.as_ref()?.sets.iter().find(|s| s.axis == axis)?;
        let u = if axis == 0 { p.y } else { p.x };
        let (i, k) = (index((u - set.u0) / set.du, set.nu), index((p.z - set.z0) / set.dz, set.nz));
        set.lines.get(k * set.nu + i).cloned()
    }

    /// The highest the height map holds material round where `step` takes the tool (its tips'
    /// box, an arc's whole circle, widened by the tool's radius and a cell), with a margin for
    /// what a cell's centre misses; infinite when that cannot be told.
    pub(super) fn top_round(&mut self, plan: &SimPlan, step: &Step) -> f64 {
        let Grid::Heights { x0, y0, dx, dy, nx, ny, .. } = plan.grid else { return f64::INFINITY };
        let (a, b) = (step.start.tip, step.end.tip);
        let (mut lo, mut hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
        match &step.how {
            How::Straight => {}
            How::Arc { work_center, .. } => {
                let r = hyp(a.x - work_center.x, a.y - work_center.y);
                lo = Vec2::new(lo.x.min(work_center.x - r), lo.y.min(work_center.y - r));
                hi = Vec2::new(hi.x.max(work_center.x + r), hi.y.max(work_center.y + r));
            }
            _ => return f64::INFINITY,
        }
        let w = step.profile.radius + dx.max(dy);
        let (fx0, fx1) = ((lo.x - w - x0) / dx, (hi.x + w - x0) / dx);
        let (fy0, fy1) = ((lo.y - w - y0) / dy, (hi.y + w - y0) / dy);
        if ![fx0, fx1, fy0, fy1].iter().all(|v| v.is_finite()) {
            return f64::INFINITY;
        }
        if fx1 < 0.0 || fy1 < 0.0 || fx0 > nx as f64 || fy0 > ny as f64 {
            return f64::NEG_INFINITY;
        }
        let m = self.max_in(plan, (index(fx0, nx), index(fx1, nx)), (index(fy0, ny), index(fy1, ny)));
        m + (2.0 * plan.cell).max(0.5) + plan.tol
    }

    /// Whether the stock is kept along X and Y as well ([`SimState::track_sides`]).
    pub fn tracks_sides(&self) -> bool {
        self.side.is_some()
    }

    /// Keeps the stock along X and Y from here on ([`SimState::track_sides`]) and takes out of
    /// every side line what `carried` (the material an earlier setup left, at a point in work
    /// coordinates) says is air, sampled along each line at the lines' spacing: what that setup
    /// cut under other material along this setup's Z (a slot through the part when it is turned
    /// onto its side) stays open in the side lines, where the height map holds a column solid
    /// from its top. The part's own material along a line is never taken out (as with
    /// [`SimState::hold_part`]: the samples see that setup's cut a sample at a time). A stretch
    /// a line cannot split further is left as material. False when no side lines are kept (off
    /// a height map).
    pub fn carve_sides(&mut self, plan: &SimPlan, carried: &Carried, part: &Surface) -> bool {
        if self.side.is_none() && !self.track_sides(plan) {
            return false;
        }
        let Some(side) = self.side.as_mut() else { return false };
        for set in &mut side.sets {
            let step = set.du.min(set.dz).max(plan.cell);
            let n = (((set.hi - set.lo) / step).ceil().max(1.0) as usize).min(1 << 20);
            let step = (set.hi - set.lo) / n as f64;
            let (nu, lo) = (set.nu.max(1), set.lo);
            let (axis, u0, du, z0, dz, hi) = (set.axis, set.u0, set.du, set.z0, set.dz, set.hi);
            let carve = |k0: usize, lines: &mut [Vec<[f64; 2]>]| -> u64 {
                let mut lost = 0;
                for (o, line) in lines.iter_mut().enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    let (i, k) = ((k0 + o) % nu, (k0 + o) / nu);
                    let (u, z) = (u0 + (i as f64 + 0.5) * du, z0 + (k as f64 + 0.5) * dz);
                    let at = |t: f64| if axis == 0 { Vec3::new(t, u, z) } else { Vec3::new(u, t, z) };
                    // The part along the line, found once a stretch of it is to be taken out
                    // (`None` inside: its crossings do not pair up, so the part is not kept).
                    let mut solid: Option<Option<Vec<[f64; 2]>>> = None;
                    let mut air: Option<f64> = None;
                    for m in 0..=n {
                        let open = m < n && {
                            let t = lo + (m as f64 + 0.5) * step;
                            !carried(at(t))
                        };
                        match (open, air) {
                            (true, None) => air = Some(lo + m as f64 * step),
                            (false, Some(a)) => {
                                let b = lo + m as f64 * step;
                                let solid = solid.get_or_insert_with(|| {
                                    let len = hi - lo;
                                    segment_stretches(part, at(lo), at(hi) - at(lo))
                                        .map(|l| l.iter().map(|s| [lo + s[0] * len, lo + s[1] * len]).collect())
                                });
                                let open: Vec<[f64; 2]> = match solid {
                                    Some(s) => outside_part(&[[a, b]], s).into_iter().map(|(g, _)| g).collect(),
                                    None => vec![[a, b]],
                                };
                                for g in open {
                                    if !take_out(line, g[0], g[1]) {
                                        lost += 1;
                                    }
                                }
                                air = None;
                            }
                            _ => {}
                        }
                    }
                }
                lost
            };
            let chunk = (set.lines.len() / 16).max(4096);
            // On threads where they can be had (else here, as on the web).
            let chunks: Vec<_> = set.lines.chunks_mut(chunk).enumerate().collect();
            let lost: u64 = each_threaded(chunks, true, |(c, part)| Ok(carve(c * chunk, part))).map_or(0, |n| n.into_iter().sum());
            set.unmodelled = set.unmodelled.saturating_add(lost);
        }
        true
    }

    /// The side lines held up to the part, as [`SimState::hold_part`] holds the height map: each
    /// line holding material holds the part's own material along it too. A stock an earlier
    /// setup left is read on cells of its own, so a line built from it ends at the last cell
    /// whose centre is in the part, up to half a cell short of a wall the part stands to there;
    /// the part's material is still there, so what this program cuts into the part is measured
    /// against the part, not against a carried line inside it.
    pub fn hold_sides(&mut self, part: &Surface) {
        let Some(side) = self.side.as_mut() else { return };
        if part.is_empty() {
            return;
        }
        for set in &mut side.sets {
            let (nu, lo, hi) = (set.nu.max(1), set.lo, set.hi);
            let (axis, u0, du, z0, dz) = (set.axis, set.u0, set.du, set.z0, set.dz);
            let hold = |k0: usize, lines: &mut [Vec<[f64; 2]>]| -> u64 {
                let mut lost = 0;
                for (o, line) in lines.iter_mut().enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    let (i, k) = ((k0 + o) % nu, (k0 + o) / nu);
                    let (u, z) = (u0 + (i as f64 + 0.5) * du, z0 + (k as f64 + 0.5) * dz);
                    let at = |t: f64| if axis == 0 { Vec3::new(t, u, z) } else { Vec3::new(u, t, z) };
                    // `None` where the part's crossings along the line do not pair up: nothing
                    // is put in.
                    let Some(solid) = segment_stretches(part, at(lo), at(hi) - at(lo)) else { continue };
                    let len = hi - lo;
                    for s in solid {
                        if !put_in(line, lo + s[0] * len, lo + s[1] * len) {
                            lost += 1;
                        }
                    }
                }
                lost
            };
            let chunk = (set.lines.len() / 16).max(4096);
            // On threads where they can be had (else here, as on the web).
            let chunks: Vec<_> = set.lines.chunks_mut(chunk).enumerate().collect();
            let lost: u64 = each_threaded(chunks, true, |(c, part)| Ok(hold(c * chunk, part))).map_or(0, |n| n.into_iter().sum());
            set.unmodelled = set.unmodelled.saturating_add(lost);
        }
    }

    /// Builds the side lines again from the height map, when they are kept.
    pub(super) fn retrack(&mut self) {
        if let Some((grid, cell)) = self.side.as_ref().map(|s| (s.grid, s.cell)) {
            self.side = SideDexels::new(&grid, &self.bands, cell).map(Box::new);
        }
    }

    /// Changes the version of every tile over the cells `(i0, i1)`, `(j0, j1)` of the grid.
    pub(super) fn touch_tiles(&mut self, ((i0, i1), (j0, j1)): ((usize, usize), (usize, usize))) {
        for b in &mut self.bands {
            let Model::Heights(h) = b else { continue };
            let (lo, hi) = (j0.max(h.j0), j1.min((h.j0 + h.ny).saturating_sub(1)));
            if lo > hi || h.ny == 0 {
                continue;
            }
            for tj in (lo - h.j0) / TILE..=(hi - h.j0) / TILE {
                for ti in i0 / TILE..=i1 / TILE {
                    if ti < h.tiles.tx
                        && let Some(v) = h.tiles.version.get_mut(tj * h.tiles.tx + ti)
                    {
                        *v = v.wrapping_add(1);
                    }
                }
            }
        }
    }
}
