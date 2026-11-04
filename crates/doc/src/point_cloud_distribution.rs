//! How much of a point cloud lies at each value of one of its properties, over what its filters
//! keep: the histograms the Edit Point Cloud card draws in its range tracks, split into the
//! colours the view draws, and the values where a label changes along a property, which the
//! tracks snap to.
//!
//! Each element weighs its measure: a dot 1, a segment its length, a triangle its area, each kind
//! divided by its own total so that kinds of different dimension weigh alike. On an element every
//! property is linear (the same everywhere, or its corners' values interpolated), so filters,
//! colours and the histogram all cut it the same way, where a linear function crosses a value. A
//! range filter keeps the part between its ends, as the view does; a filter by values keeps the
//! parts whose rounded value it holds; the colour sorts the parts into the colours they are drawn
//! in. Each part spreads its measure exactly over the values it covers of the histogram's
//! property: a dot's at one value, a segment's evenly, a triangle's as the tent its corners'
//! values make. The values the parts kept take, from the least to the greatest, are the ramp's
//! automatic range: the colours span exactly what is shown.

use crate::point_cloud::{ColorBy, Filter, Keep, On, PALETTE, PointCloud, Source};

/// Most bins of one histogram.
pub const MAX_BINS: usize = 8192;
/// Most bands one part of an element is split into by one cut (a hostile file's values may
/// cross many).
const MAX_BANDS: usize = 4096;
/// Most corners of a part of a triangle: each cut adds at most one to its three.
const MAX_CORNERS: usize = 24;

/// What the histograms need of a cloud's geometry, worked out once for its points and cells.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    /// Each point in the design.
    world: Vec<[f64; 3]>,
    /// The points drawn as dots: those no segment or triangle ends on.
    dots: Vec<u32>,
    /// Each segment's share of all segments' length, and each triangle's of their area.
    segments: Vec<f64>,
    triangles: Vec<f64>,
}

impl Geometry {
    pub fn of(c: &PointCloud) -> Geometry {
        let world: Vec<[f64; 3]> = (0..c.len()).map(|i| c.world_point(i).map_or([0.0; 3], |p| [p.x, p.y, p.z])).collect();
        let mut used = vec![false; world.len()];
        for i in c.complex.segments.0.iter().chain(c.complex.triangles.0.iter()) {
            if let Some(u) = used.get_mut(*i as usize) {
                *u = true;
            }
        }
        let dots = (0..world.len() as u32).filter(|i| !used.get(*i as usize).copied().unwrap_or(true)).collect();
        let at = |i: u32| world.get(i as usize).copied().unwrap_or_default();
        let sub = |a: [f64; 3], b: [f64; 3]| [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let segments = c.complex.segments.0.as_chunks::<2>().0.iter().map(|[a, b]| norm(sub(at(*a), at(*b)))).collect();
        let triangles = c
            .complex
            .triangles
            .0
            .as_chunks::<3>()
            .0
            .iter()
            .map(|[a, b, d]| {
                let (u, v) = (sub(at(*a), at(*b)), sub(at(*a), at(*d)));
                0.5 * norm([u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]])
            })
            .collect();
        Geometry { world, dots, segments: shares(segments), triangles: shares(triangles) }
    }
}

/// Each measure over their total (none for a total that is not a positive number).
fn shares(mut m: Vec<f64>) -> Vec<f64> {
    let total: f64 = m.iter().filter(|v| v.is_finite()).sum();
    for v in &mut m {
        *v = if total > 0.0 && v.is_finite() { *v / total } else { 0.0 };
    }
    m
}

/// How a histogram is split by colour, into slots; the last slot holds what has no value to be
/// coloured by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Colour<'a> {
    /// One colour.
    One,
    /// The ramp over `range` of `property`, in `bands` equal bands; below and above the range
    /// count with the first and the last.
    Ramp { property: &'a str, range: [f64; 2], bands: usize },
    /// The palette of a whole-numbered `property`: a slot per colour of [`PALETTE`], then one
    /// for negative values.
    Palette { property: &'a str },
}

impl Colour<'_> {
    /// How many slots the histogram has.
    pub fn slots(&self) -> usize {
        match self {
            Colour::One => 1,
            Colour::Ramp { bands, .. } => bands + 1,
            Colour::Palette { .. } => PALETTE.len() + 2,
        }
    }
    fn property(&self) -> Option<&str> {
        match self {
            Colour::One => None,
            Colour::Ramp { property, .. } | Colour::Palette { property } => Some(property),
        }
    }
    /// How a property's values are cut into bands, and the slot of each band.
    fn cut(&self) -> Option<Cut> {
        match *self {
            Colour::One => None,
            Colour::Ramp { range: [lo, hi], bands, .. } => Some(Cut::Grid { start: lo, step: (hi - lo) / bands as f64, count: bands + 1 }),
            Colour::Palette { .. } => Some(INTEGERS),
        }
    }
    fn slot(&self, band: usize) -> usize {
        match *self {
            Colour::One => 0,
            Colour::Ramp { bands, .. } => band.saturating_sub(1).min(bands.saturating_sub(1)),
            Colour::Palette { .. } => match integer(band) {
                Some(v) if v >= 0 => v as usize % PALETTE.len(),
                _ => PALETTE.len(),
            },
        }
    }
}

/// How much lies in each bin, by colour slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub bins: usize,
    /// `slots[s][b]`: the measure of slot `s` in bin `b`.
    pub slots: Vec<Vec<f64>>,
    /// The least and the greatest value the parts kept take, within the axis or not.
    pub extent: Option<[f64; 2]>,
}

/// Where a linear function is cut into bands, by increasing values.
#[derive(Clone, Copy, Debug)]
enum Cut {
    /// Below (band 0), within (1, both ends included) and above (2) a range.
    Range([f64; 2]),
    /// `count` values `start + k·step`: band 0 lies below the first, band `k` from the `k`th on.
    Grid { start: f64, step: f64, count: usize },
}

/// The bands of whole numbers: band `k` holds the values rounding to `k - 2³¹`.
const INTEGERS: Cut = Cut::Grid { start: 0.5 - 2_147_483_648.0, step: 1.0, count: u32::MAX as usize };

/// The whole number a band of [`INTEGERS`] holds.
fn integer(band: usize) -> Option<i64> {
    i64::try_from(band).ok().map(|k| k - 2_147_483_648)
}

impl Cut {
    fn band(&self, v: f64) -> usize {
        match *self {
            Cut::Range([a, b]) => usize::from(v >= a) + usize::from(v > b),
            Cut::Grid { start, step, count } => {
                if !(v >= start) {
                    return 0;
                }
                (((v - start) / step).floor() as usize).saturating_add(1).min(count)
            }
        }
    }
    /// The values band `k` holds, from and to (`None`: without end).
    fn bounds(&self, k: usize) -> (Option<f64>, Option<f64>) {
        match *self {
            Cut::Range([a, b]) => match k {
                0 => (None, Some(a)),
                1 => (Some(a), Some(b)),
                _ => (Some(b), None),
            },
            Cut::Grid { start, step, count } => {
                ((k > 0).then_some(start + k.saturating_sub(1) as f64 * step), (k < count).then_some(start + k as f64 * step))
            }
        }
    }
}

/// A part of an element in its own coordinates: a dot, a stretch `[s₀, s₁]` of a segment (0 at
/// its first point), or a convex polygon in a triangle (each corner the weights of the
/// triangle's second and third points).
// Parts live in two buffers reused element after element: boxing a polygon would allocate per
// triangle.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug)]
enum Part {
    Dot,
    Stretch([f64; 2]),
    Polygon(Polygon),
}

#[derive(Clone, Copy, Debug)]
struct Polygon {
    at: [[f64; 2]; MAX_CORNERS],
    len: usize,
}

impl Polygon {
    fn triangle() -> Polygon {
        let mut at = [[0.0; 2]; MAX_CORNERS];
        at[1] = [1.0, 0.0];
        at[2] = [0.0, 1.0];
        Polygon { at, len: 3 }
    }
    fn corners(&self) -> &[[f64; 2]] {
        self.at.get(..self.len).unwrap_or(&[])
    }
    /// The part where `g ≥ 0`.
    fn clip(&self, g: impl Fn([f64; 2]) -> f64) -> Polygon {
        let mut out = Polygon { at: [[0.0; 2]; MAX_CORNERS], len: 0 };
        let mut push = |p: [f64; 2]| {
            if let Some(slot) = out.at.get_mut(out.len) {
                *slot = p;
                out.len += 1;
            }
        };
        let c = self.corners();
        for (k, p) in c.iter().enumerate() {
            let q = c.get((k + 1) % c.len()).copied().unwrap_or(*p);
            let (gp, gq) = (g(*p), g(q));
            if gp >= 0.0 {
                push(*p);
            }
            if (gp >= 0.0) != (gq >= 0.0) {
                let f = gp / (gp - gq);
                push([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]);
            }
        }
        out
    }
}

/// A linear function on an element: its values at the corners.
fn at(f: [f64; 3], p: [f64; 2]) -> f64 {
    f[0] + (f[1] - f[0]) * p[0] + (f[2] - f[0]) * p[1]
}

/// The bands of `cut` that `part` meets where `f` takes its values, each with the part in it.
fn split(part: &Part, f: [f64; 3], cut: Cut, mut visit: impl FnMut(usize, Part)) {
    match part {
        Part::Dot => visit(cut.band(f[0]), Part::Dot),
        Part::Stretch([s0, s1]) => {
            let (a, b) = (at(f, [*s0, 0.0]), at(f, [*s1, 0.0]));
            let (lo, hi) = (a.min(b), a.max(b));
            let (k0, k1) = (cut.band(lo), cut.band(hi));
            if k0 == k1 || !(hi > lo) {
                return visit(k0, *part);
            }
            let s = |x: f64| s0 + (x - a) / (b - a) * (s1 - s0);
            for k in k0..=k1.min(k0 + MAX_BANDS) {
                let (from, to) = cut.bounds(k);
                let (x0, x1) = (from.map_or(lo, |v| v.max(lo)), to.map_or(hi, |v| v.min(hi)));
                if x1 > x0 {
                    let (t0, t1) = (s(x0), s(x1));
                    visit(k, Part::Stretch([t0.min(t1), t0.max(t1)]));
                }
            }
        }
        Part::Polygon(p) => {
            let values = p.corners().iter().map(|c| at(f, *c));
            let (lo, hi) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
            let (k0, k1) = (cut.band(lo), cut.band(hi));
            if k0 == k1 || !(hi > lo) {
                return visit(k0, *part);
            }
            for k in k0..=k1.min(k0 + MAX_BANDS) {
                let (from, to) = cut.bounds(k);
                let mut q = *p;
                if let Some(v) = from {
                    q = q.clip(|c| at(f, c) - v);
                }
                if let Some(v) = to {
                    q = q.clip(|c| v - at(f, c));
                }
                if q.len >= 3 {
                    visit(k, Part::Polygon(q));
                }
            }
        }
    }
}

/// A filter as a cut and the bands it keeps.
struct Kept<'a> {
    cut: Cut,
    values: Option<&'a [i32]>,
}

impl Kept<'_> {
    fn of(f: &Filter) -> Kept<'_> {
        match &f.keep {
            Keep::Range { range } => Kept { cut: Cut::Range(*range), values: None },
            Keep::Values { values } => Kept { cut: INTEGERS, values: Some(values) },
        }
    }
    fn keeps(&self, band: usize) -> bool {
        match self.values {
            None => band == 1,
            Some(values) => integer(band).is_some_and(|v| values.iter().any(|k| i64::from(*k) == v)),
        }
    }
}

/// The values of `s` at an element's corners: element `e` of a kind, on points `ids`.
fn corners(s: Source, e: usize, ids: [u32; 3], world: &[[f64; 3]]) -> [f64; 3] {
    match s {
        Source::Coordinate(k) => ids.map(|i| world.get(i as usize).and_then(|p| p.get(k)).copied().unwrap_or(f64::NAN)),
        Source::Corners(v) => ids.map(|i| v.get(i as usize).map_or(f64::NAN, |x| f64::from(*x))),
        Source::Element(v) => [v.get(e).map_or(f64::NAN, |x| f64::from(*x)); 3],
    }
}

/// Measure spread over bins: what falls in a bin directly, and evenly spread measure as ramps
/// (`k·max(0, x − c)` summed into its distribution function at each bin edge).
struct Bins {
    lo: f64,
    hi: f64,
    width: f64,
    n: usize,
    direct: Vec<f64>,
    slope: Vec<f64>,
    offset: Vec<f64>,
}

impl Bins {
    fn new([lo, hi]: [f64; 2], n: usize, slots: usize) -> Bins {
        Bins {
            lo,
            hi,
            width: (hi - lo) / n as f64,
            n,
            direct: vec![0.0; slots * n],
            slope: vec![0.0; slots * (n + 2)],
            offset: vec![0.0; slots * (n + 2)],
        }
    }
    fn edge(&self, j: usize) -> f64 {
        if j >= self.n { self.hi } else { self.lo + j as f64 * self.width }
    }
    /// The bin holding `x`, the last bin holding its upper end too.
    fn bin(&self, x: f64) -> Option<usize> {
        if !(x >= self.lo && x <= self.hi) {
            return None;
        }
        Some((((x - self.lo) / self.width).floor() as usize).min(self.n - 1))
    }
    fn add(&mut self, slot: usize, j: usize, m: f64) {
        if let Some(d) = self.direct.get_mut(slot * self.n + j) {
            *d += m;
        }
    }
    fn point(&mut self, slot: usize, x: f64, m: f64) {
        if let Some(j) = self.bin(x) {
            self.add(slot, j, m);
        }
    }
    /// A ramp of slope `k` from `c`: it reaches the first edge at or after `c`.
    fn ramp(&mut self, slot: usize, c: f64, k: f64) {
        let j = ((c - self.lo) / self.width).ceil().clamp(0.0, (self.n + 1) as f64) as usize;
        let at = slot * (self.n + 2) + j;
        if let (Some(s), Some(o)) = (self.slope.get_mut(at), self.offset.get_mut(at)) {
            *s += k;
            *o += k * c;
        }
    }
    /// `m` spread evenly over `[a, b]`.
    fn even(&mut self, slot: usize, a: f64, b: f64, m: f64) {
        if !(b > a) {
            return self.point(slot, a, m);
        }
        let k = m / (b - a);
        self.ramp(slot, a, k);
        self.ramp(slot, b, -k);
    }
    /// `m` spread as a triangle's measure over its corners' values `q₀ ≤ q₁ ≤ q₂`: rising to
    /// `q₁` and falling to `q₂`.
    fn tent(&mut self, slot: usize, [q0, q1, q2]: [f64; 3], m: f64) {
        if !(q2 > q0) {
            return self.point(slot, q0, m);
        }
        let below = |x: f64| {
            if x <= q0 {
                0.0
            } else if x >= q2 {
                m
            } else if x <= q1 {
                m * (x - q0) * (x - q0) / ((q2 - q0) * (q1 - q0))
            } else {
                m * (1.0 - (q2 - x) * (q2 - x) / ((q2 - q0) * (q2 - q1)))
            }
        };
        let (Some(j0), Some(j1)) = (self.bin(q0.max(self.lo)), self.bin(q2.min(self.hi))) else { return };
        for j in j0..=j1 {
            let (a, b) = (self.edge(j).max(q0), self.edge(j + 1).min(q2));
            self.add(slot, j, below(b) - below(a));
        }
    }
    fn finish(self, slots: usize) -> Vec<Vec<f64>> {
        (0..slots)
            .map(|s| {
                let (mut slope, mut offset, mut before) = (0.0, 0.0, 0.0);
                (0..=self.n)
                    .filter_map(|j| {
                        let at = s * (self.n + 2) + j;
                        slope += self.slope.get(at).copied().unwrap_or(0.0);
                        offset += self.offset.get(at).copied().unwrap_or(0.0);
                        let f = slope * self.edge(j) - offset;
                        let spread = (j > 0).then_some(f - before);
                        before = f;
                        spread.map(|d| d + self.direct.get(s * self.n + j - 1).copied().unwrap_or(0.0))
                    })
                    .collect()
            })
            .collect()
    }
}

/// The kinds of element, each with its elements' points and shares of measure.
fn kinds<'a>(c: &'a PointCloud, g: &'a Geometry) -> [(On, Box<dyn Iterator<Item = (usize, [u32; 3], f64)> + 'a>); 3] {
    let dot = if g.dots.is_empty() { 0.0 } else { 1.0 / g.dots.len() as f64 };
    [
        (On::Point, Box::new(g.dots.iter().map(move |i| (*i as usize, [*i; 3], dot)))),
        (
            On::Segment,
            Box::new(c.complex.segments.0.as_chunks::<2>().0.iter().zip(&g.segments).enumerate().map(|(e, ([a, b], m))| (e, [*a, *b, *b], *m))),
        ),
        (On::Triangle, Box::new(c.complex.triangles.0.as_chunks::<3>().0.iter().zip(&g.triangles).enumerate().map(|(e, (ids, m))| (e, *ids, *m)))),
    ]
}

/// How much of the cloud lies in each of `bins` bins over `axis` of `property`, by `colour`,
/// within every one of `filters`; `None` for an axis or a number of bins it can't have.
pub fn histogram(
    c: &PointCloud,
    g: &Geometry,
    property: &str,
    axis: [f64; 2],
    bins: usize,
    colour: &Colour,
    filters: &[Filter],
) -> Option<Histogram> {
    if !(axis[0].is_finite() && axis[1].is_finite() && axis[1] > axis[0]) || bins == 0 || bins > MAX_BINS {
        return None;
    }
    if let Colour::Ramp { range: [lo, hi], bands, .. } = *colour
        && !(hi > lo && bands > 0 && bands <= MAX_BINS)
    {
        return None;
    }
    let slots = colour.slots();
    let mut out = Bins::new(axis, bins, slots);
    let kept: Vec<Kept> = filters.iter().map(Kept::of).collect();
    let (colour_cut, plain) = (colour.cut(), slots - 1);
    let (mut parts, mut next) = (Vec::new(), Vec::new());
    let mut extent = None;
    for (on, elements) in kinds(c, g) {
        let Some(p) = c.source(property, on) else { continue };
        let filtered: Vec<Option<Source>> = filters.iter().map(|f| c.source(&f.property, on)).collect();
        let coloured = colour.property().and_then(|n| c.source(n, on));
        let start = match on {
            On::Point => Part::Dot,
            On::Segment => Part::Stretch([0.0, 1.0]),
            On::Triangle => Part::Polygon(Polygon::triangle()),
        };
        for (e, ids, m) in elements {
            if m <= 0.0 {
                continue;
            }
            parts.clear();
            parts.push(start);
            for (k, s) in kept.iter().zip(&filtered) {
                // An element without the filtered property is not filtered by it, as in the view.
                let Some(s) = s else { continue };
                let f = corners(*s, e, ids, &g.world);
                next.clear();
                for part in &parts {
                    split(part, f, k.cut, |band, q| {
                        if k.keeps(band) {
                            next.push(q);
                        }
                    });
                }
                std::mem::swap(&mut parts, &mut next);
            }
            let pv = corners(p, e, ids, &g.world);
            let colour_values = coloured.map(|s| corners(s, e, ids, &g.world));
            for part in &parts {
                // A linear function is least and greatest at a part's corners.
                for v in part_values(part, pv) {
                    if v.is_finite() {
                        extent = Some(extent.map_or([v, v], |[lo, hi]: [f64; 2]| [lo.min(v), hi.max(v)]));
                    }
                }
                match (colour_cut, colour_values) {
                    (Some(cut), Some(f)) => split(part, f, cut, |band, q| spread(&mut out, colour.slot(band), &q, pv, m)),
                    (Some(_), None) => spread(&mut out, plain, part, pv, m),
                    (None, _) => spread(&mut out, 0, part, pv, m),
                }
            }
        }
    }
    Some(Histogram { bins, slots: out.finish(slots), extent })
}

/// The values a linear function `f` takes at a part's corners.
fn part_values(part: &Part, f: [f64; 3]) -> impl Iterator<Item = f64> + '_ {
    let (ends, corners): (Option<[f64; 2]>, &[[f64; 2]]) = match part {
        Part::Dot => (Some([f[0], f[0]]), &[]),
        Part::Stretch([s0, s1]) => (Some([at(f, [*s0, 0.0]), at(f, [*s1, 0.0])]), &[]),
        Part::Polygon(p) => (None, p.corners()),
    };
    ends.into_iter().flatten().chain(corners.iter().map(move |c| at(f, *c)))
}

/// A range for a ramp from the values it spans: a single value gets one unit around it.
pub fn ramp_range([lo, hi]: [f64; 2]) -> [f64; 2] {
    if hi > lo { [lo, hi] } else { [lo - 0.5, lo + 0.5] }
}

/// The ramp's range: the one set; else, coloured by a property, exactly the values it takes over
/// what `filters` keep (the cloud's own bound, [`PointCloud::color_range`], when nothing is
/// kept); coloured by height, the cloud's own (its points but the highest and lowest few).
pub fn color_range(c: &PointCloud, g: &Geometry, filters: &[Filter]) -> Option<[f64; 2]> {
    color_range_by(c, |property, axis| histogram(c, g, property, axis, 1, &Colour::One, filters).and_then(|h| h.extent))
}

/// [`color_range`], with the values the coloured property takes over what is kept given by
/// `extent` (of the property and an axis over its span): worked out elsewhere, or not yet.
pub fn color_range_by(c: &PointCloud, extent: impl FnOnce(&str, [f64; 2]) -> Option<[f64; 2]>) -> Option<[f64; 2]> {
    let property = c.colored_property()?;
    if c.height_range.is_some() || c.color_by != ColorBy::Property {
        return c.color_range();
    }
    let axis = ramp_range(c.span(property)?);
    extent(property, axis).map(ramp_range).or_else(|| c.color_range())
}

/// Spread a part's measure (`m`, the whole element's share) over the values `f` takes on it.
fn spread(out: &mut Bins, slot: usize, part: &Part, f: [f64; 3], m: f64) {
    match part {
        Part::Dot => out.point(slot, f[0], m),
        Part::Stretch([s0, s1]) => {
            let (a, b) = (at(f, [*s0, 0.0]), at(f, [*s1, 0.0]));
            out.even(slot, a.min(b), a.max(b), m * (s1 - s0));
        }
        Part::Polygon(p) => {
            let c = p.corners();
            let Some(first) = c.first() else { return };
            for w in c.get(1..).unwrap_or(&[]).windows(2) {
                let [b, d] = [w[0], w[1]];
                // The share of the triangle's area: twice the area in its own coordinates.
                let area = ((b[0] - first[0]) * (d[1] - first[1]) - (b[1] - first[1]) * (d[0] - first[0])).abs();
                let mut q = [at(f, *first), at(f, b), at(f, d)];
                q.sort_by(f64::total_cmp);
                out.tent(slot, q, m * area);
            }
        }
    }
}

/// The values of `property` where the whole-numbered `label` changes, element after element in
/// order of `property` (each element at its smallest value, with its label there), lowest first.
pub fn label_changes(c: &PointCloud, g: &Geometry, label: &str, property: &str) -> Vec<f64> {
    let mut at: Vec<(f64, i64)> = Vec::new();
    for (on, elements) in kinds(c, g) {
        let (Some(p), Some(l)) = (c.source(property, on), c.source(label, on)) else { continue };
        let n = match on {
            On::Point => 1,
            On::Segment => 2,
            On::Triangle => 3,
        };
        for (e, ids, _) in elements {
            let (pv, lv) = (corners(p, e, ids, &g.world), corners(l, e, ids, &g.world));
            let lowest = (0..n).min_by(|i, j| pv[*i].total_cmp(&pv[*j])).unwrap_or(0);
            let v = pv[lowest];
            if v.is_finite() {
                let l = lv[lowest];
                at.push((v, if l.is_finite() { l.round() as i64 } else { i64::MIN }));
            }
        }
    }
    at.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<f64> = at.windows(2).filter(|w| w[0].1 != w[1].1).map(|w| w[1].0).collect();
    out.dedup();
    out
}

#[cfg(test)]
#[path = "point_cloud_distribution_tests.rs"]
mod tests;
