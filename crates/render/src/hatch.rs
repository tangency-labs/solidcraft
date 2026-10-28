//! The section hatch by part, as engineering section drawings tell parts apart: each body a
//! section cuts is hatched at 45 or 135 degrees in one of a few colours of one family, and bodies
//! whose cut faces touch or overlap never share a hatch (a map colouring of the cut, the angle
//! changed first, then the colour). Otherwise a body keeps the hatch its name seeds, so it stays
//! the same from frame to frame and as the plane moves. Spacing and line width are the same for
//! every hatch; only the angle and the colour change.

use std::collections::{HashMap, HashSet};

use solidcraft_geom::{Vec2, Vec3};

/// The colours a cut is hatched in: the section cap's own, then the others of its family.
pub const HATCH_COLOURS: usize = 3;

/// The hatches a body may get: every colour at 45 and at 135 degrees.
pub const HATCHES: usize = 2 * HATCH_COLOURS;

/// One of the [`HATCHES`]: colour `n / 2`, at 135 degrees when `n` is odd. Hatch 0 is the one a
/// section draws every body with when it does not hatch by part: the cap's colour at 45 degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Hatch(pub u8);

impl Hatch {
    /// Which of the [`HATCH_COLOURS`] it is drawn in.
    pub fn colour(self) -> usize {
        usize::from(self.0 / 2) % HATCH_COLOURS
    }

    /// Its lines run at 135 degrees on screen (else 45).
    pub fn turned(self) -> bool {
        self.0 % 2 == 1
    }

    fn of(colour: usize, turned: bool) -> Hatch {
        Hatch(((colour % HATCH_COLOURS) * 2 + usize::from(turned)) as u8)
    }
}

/// A body's seed: its persistent name hashed (FNV-1a, the same on every platform and release).
pub fn seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// A closed body the section cuts: its [`seed`] and where the plane cuts it, as segments in the
/// plane (one per crossed triangle, in world space).
pub struct CutPart<'a> {
    pub seed: u64,
    pub segments: &'a [(Vec3, Vec3)],
}

/// Cut faces closer than this, as a share of the whole cut's size, touch: a pin in its bore,
/// tessellated each its own way, still reads as one contact.
const TOUCH: f64 = 1e-3;

/// The hatch of each part (in order) cut by a plane with normal `normal`: the one its seed picks,
/// unless a part it touches or overlaps on the cut, taken first, has that one already; then the
/// same colour at the other angle, then the next colours. Parts are taken in the order of their
/// seeds, so the hatches depend only on which parts are cut and which of them touch.
pub fn hatches(parts: &[CutPart], normal: Vec3) -> Vec<Hatch> {
    let neighbours = touching(parts, normal);
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by_key(|&i| (parts.get(i).map_or(0, |p| p.seed), i));
    let mut out: Vec<Option<Hatch>> = vec![None; parts.len()];
    for i in order {
        let Some(part) = parts.get(i) else { continue };
        let taken: Vec<Hatch> = neighbours.get(&i).into_iter().flatten().filter_map(|j| out.get(*j).copied().flatten()).collect();
        let own = Hatch((mix(part.seed) % HATCHES as u64) as u8);
        let candidates = (0..HATCH_COLOURS).flat_map(|k| {
            let colour = own.colour() + k;
            [Hatch::of(colour, own.turned()), Hatch::of(colour, !own.turned())]
        });
        // The first free one; with every one taken, the one the fewest neighbours have.
        let pick = candidates.clone().find(|h| !taken.contains(h)).or_else(|| candidates.min_by_key(|h| taken.iter().filter(|t| *t == h).count()));
        if let Some(slot) = out.get_mut(i) {
            *slot = pick;
        }
    }
    out.into_iter().map(Option::unwrap_or_default).collect()
}

/// The seed spread over all its bits (SplitMix64's finaliser), so names that differ in one
/// letter pick unrelated hatches.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Which parts touch or overlap on the cut: per part, the others.
fn touching(parts: &[CutPart], normal: Vec3) -> HashMap<usize, Vec<usize>> {
    let u = normal.any_perp();
    let v = normal.cross(u).normalized().unwrap_or(Vec3::Y);
    let flat = |p: Vec3| Vec2::new(p.dot(u), p.dot(v));
    let cuts: Vec<Vec<(Vec2, Vec2)>> = parts
        .iter()
        .map(|p| p.segments.iter().map(|(a, b)| (flat(*a), flat(*b))).filter(|(a, b)| a.is_finite() && b.is_finite()).collect())
        .collect();
    let boxes: Vec<Option<(Vec2, Vec2)>> = cuts.iter().map(|c| bounds(c)).collect();
    let Some((lo, hi)) = boxes.iter().flatten().copied().reduce(|(a, b), (c, d)| (min2(a, c), max2(b, d))) else {
        return HashMap::new();
    };
    let extent = (hi - lo).len();
    if !(extent.is_finite() && extent > 0.0) {
        return HashMap::new();
    }
    let eps = extent * TOUCH;
    let mut pairs: HashSet<(usize, usize)> = HashSet::new();
    // Outlines closer than `eps`: through a grid of cells at least `eps` wide, each holding the
    // pieces of segments (no longer than a cell) whose box, grown by half of `eps`, reaches it.
    let cell = eps.max(extent / 1024.0);
    let mut grid: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
    let at = |x: f64, o: f64| ((x - o) / cell).floor() as i64;
    for (pi, cut) in cuts.iter().enumerate() {
        for (si, (a, b)) in cut.iter().enumerate() {
            let pieces = ((*b - *a).len() / cell).ceil().clamp(1.0, 4096.0) as usize;
            let mut last = None;
            for k in 0..pieces {
                let (p, q) = (a.lerp(*b, k as f64 / pieces as f64), a.lerp(*b, (k + 1) as f64 / pieces as f64));
                let (x0, x1) = (at(p.x.min(q.x) - eps / 2.0, lo.x), at(p.x.max(q.x) + eps / 2.0, lo.x));
                let (y0, y1) = (at(p.y.min(q.y) - eps / 2.0, lo.y), at(p.y.max(q.y) + eps / 2.0, lo.y));
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        // Consecutive pieces share cells: the segment goes in each cell once.
                        if last.is_some_and(|(lx0, lx1, ly0, ly1)| (lx0..=lx1).contains(&x) && (ly0..=ly1).contains(&y)) {
                            continue;
                        }
                        grid.entry((x, y)).or_default().push((pi, si));
                    }
                }
                last = Some((x0, x1, y0, y1));
            }
        }
    }
    for list in grid.values() {
        for (k, &(pa, sa)) in list.iter().enumerate() {
            for &(pb, sb) in list.iter().skip(k + 1) {
                let pair = (pa.min(pb), pa.max(pb));
                if pa == pb || pairs.contains(&pair) {
                    continue;
                }
                let seg = |p: usize, s: usize| cuts.get(p).and_then(|c| c.get(s)).copied();
                if let (Some(x), Some(y)) = (seg(pa, sa), seg(pb, sb))
                    && segment_distance(x, y) <= eps
                {
                    pairs.insert(pair);
                }
            }
        }
    }
    // Overlap without outlines that meet: one part's cut lies inside the other's material.
    let mut rows: HashMap<usize, Rows> = HashMap::new();
    for a in 0..cuts.len() {
        for b in a + 1..cuts.len() {
            if pairs.contains(&(a, b)) {
                continue;
            }
            let (Some(Some(ba)), Some(Some(bb))) = (boxes.get(a), boxes.get(b)) else { continue };
            if ba.1.x < bb.0.x || bb.1.x < ba.0.x || ba.1.y < bb.0.y || bb.1.y < ba.0.y {
                continue;
            }
            let inside = |outer: usize, inner: usize, rows: &mut HashMap<usize, Rows>| {
                let (Some(o), Some(Some(first))) = (cuts.get(outer), cuts.get(inner).map(|c| c.first())) else { return false };
                let r = rows.entry(outer).or_insert_with(|| Rows::new(o));
                r.contains(o, first.0.lerp(first.1, 0.5))
            };
            if inside(a, b, &mut rows) || inside(b, a, &mut rows) {
                pairs.insert((a, b));
            }
        }
    }
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for (a, b) in pairs {
        out.entry(a).or_default().push(b);
        out.entry(b).or_default().push(a);
    }
    out
}

/// A part's cut segments sorted into rows across the plane's second axis, so a point-in-material
/// test looks at the segments of one row.
struct Rows {
    lo: f64,
    step: f64,
    rows: Vec<Vec<usize>>,
}

/// Rows a [`Rows`] makes, at most.
const ROWS: usize = 256;

impl Rows {
    fn new(cut: &[(Vec2, Vec2)]) -> Rows {
        let (lo, hi) = cut.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), (a, b)| (l.min(a.y).min(b.y), h.max(a.y).max(b.y)));
        let count = (cut.len() / 16).clamp(1, ROWS);
        let step = if hi > lo { (hi - lo) / count as f64 } else { 1.0 };
        let mut rows = vec![Vec::new(); count];
        let row = |y: f64| (((y - lo) / step).floor().max(0.0) as usize).min(count - 1);
        for (i, (a, b)) in cut.iter().enumerate() {
            for r in rows.iter_mut().take(row(a.y.max(b.y)) + 1).skip(row(a.y.min(b.y))) {
                r.push(i);
            }
        }
        Rows { lo, step, rows }
    }

    /// Whether `p` lies inside the material `cut` bounds: a ray along the first axis crosses its
    /// outline an odd number of times.
    fn contains(&self, cut: &[(Vec2, Vec2)], p: Vec2) -> bool {
        let r = ((p.y - self.lo) / self.step).floor();
        let Some(row) = (r >= 0.0).then_some(r as usize).and_then(|r| self.rows.get(r)) else { return false };
        let crossings = row
            .iter()
            .filter_map(|i| cut.get(*i))
            .filter(|(a, b)| (a.y > p.y) != (b.y > p.y) && a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) > p.x)
            .count();
        crossings % 2 == 1
    }
}

fn min2(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x.min(b.x), a.y.min(b.y))
}

fn max2(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x.max(b.x), a.y.max(b.y))
}

fn bounds(cut: &[(Vec2, Vec2)]) -> Option<(Vec2, Vec2)> {
    cut.iter().map(|(a, b)| (min2(*a, *b), max2(*a, *b))).reduce(|(a, b), (c, d)| (min2(a, c), max2(b, d)))
}

/// The distance between two segments: zero where they cross, else the nearest end to the other.
fn segment_distance((a, b): (Vec2, Vec2), (c, d): (Vec2, Vec2)) -> f64 {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return 0.0;
    }
    let to = |p: Vec2, s: Vec2, e: Vec2| {
        let se = e - s;
        let l2 = se.dot(se);
        let t = if l2 > 0.0 { ((p - s).dot(se) / l2).clamp(0.0, 1.0) } else { 0.0 };
        (p - (s + se * t)).len()
    };
    to(a, c, d).min(to(b, c, d)).min(to(c, a, b)).min(to(d, a, b))
}
