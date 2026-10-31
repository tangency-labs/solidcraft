//! How much of a point cloud lies at each value of one of its properties, over what its filters
//! keep: the histograms the Edit Point Cloud card draws in its range tracks, split into the
//! colours the view draws, and the values where a label changes along a property, which the
//! tracks snap to.
//!
//! Each point weighs the same, the cloud weighing 1. A range filter keeps the points between its
//! ends, as the view does; a filter by values keeps those whose rounded value it holds; the
//! colour sorts the points kept into the colours they are drawn in. The values the points kept
//! take, from the least to the greatest, are the ramp's automatic range: the colours span exactly
//! what is shown.

use crate::point_cloud::{ColorBy, Filter, Keep, PALETTE, PointCloud, Source};

/// Most bins of one histogram.
pub const MAX_BINS: usize = 8192;

/// What the histograms need of a cloud's geometry, worked out once for its points.
#[derive(Clone, Debug, Default)]
pub struct Geometry {
    /// Each point in the design.
    world: Vec<[f64; 3]>,
}

impl Geometry {
    pub fn of(c: &PointCloud) -> Geometry {
        Geometry { world: (0..c.len()).map(|i| c.world_point(i).map_or([0.0; 3], |p| [p.x, p.y, p.z])).collect() }
    }
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
    /// The least and the greatest value the points kept take, within the axis or not.
    pub extent: Option<[f64; 2]>,
}

/// Where values are cut into bands, by increasing values.
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
    fn keeps(&self, v: f64) -> bool {
        let band = self.cut.band(v);
        match self.values {
            None => band == 1,
            Some(values) => integer(band).is_some_and(|v| values.iter().any(|k| i64::from(*k) == v)),
        }
    }
}

/// The value of `s` at point `i`.
fn value(s: Source, i: usize, world: &[[f64; 3]]) -> f64 {
    match s {
        Source::Coordinate(k) => world.get(i).and_then(|p| p.get(k)).copied().unwrap_or(f64::NAN),
        Source::Column(v) => v.get(i).map_or(f64::NAN, |x| f64::from(*x)),
    }
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
    let mut out = vec![vec![0.0; bins]; slots];
    let mut extent = None;
    let Some(p) = c.source(property) else { return Some(Histogram { bins, slots: out, extent }) };
    // A point is not filtered by a property the cloud doesn't have, as in the view.
    let kept: Vec<(Kept, Source)> = filters.iter().filter_map(|f| Some((Kept::of(f), c.source(&f.property)?))).collect();
    let coloured = colour.property().and_then(|n| c.source(n));
    let (colour_cut, plain) = (colour.cut(), slots - 1);
    let m = 1.0 / g.world.len().max(1) as f64;
    let width = (axis[1] - axis[0]) / bins as f64;
    for i in 0..g.world.len() {
        if !kept.iter().all(|(k, s)| k.keeps(value(*s, i, &g.world))) {
            continue;
        }
        let v = value(p, i, &g.world);
        if !v.is_finite() {
            continue;
        }
        extent = Some(extent.map_or([v, v], |[lo, hi]: [f64; 2]| [lo.min(v), hi.max(v)]));
        if !(v >= axis[0] && v <= axis[1]) {
            continue;
        }
        let slot = match (colour_cut, coloured) {
            (Some(cut), Some(s)) => colour.slot(cut.band(value(s, i, &g.world))),
            (Some(_), None) => plain,
            (None, _) => 0,
        };
        // The last bin holds the axis' upper end too.
        let bin = (((v - axis[0]) / width).floor() as usize).min(bins - 1);
        if let Some(b) = out.get_mut(slot).and_then(|s| s.get_mut(bin)) {
            *b += m;
        }
    }
    Some(Histogram { bins, slots: out, extent })
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

/// The values of `property` where the whole-numbered `label` changes, point after point in order
/// of `property`, lowest first.
pub fn label_changes(c: &PointCloud, g: &Geometry, label: &str, property: &str) -> Vec<f64> {
    let (Some(p), Some(l)) = (c.source(property), c.source(label)) else { return Vec::new() };
    let mut at: Vec<(f64, i64)> = (0..g.world.len())
        .filter_map(|i| {
            let (v, l) = (value(p, i, &g.world), value(l, i, &g.world));
            v.is_finite().then(|| (v, if l.is_finite() { l.round() as i64 } else { i64::MIN }))
        })
        .collect();
    at.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<f64> = at.windows(2).filter(|w| w[0].1 != w[1].1).map(|w| w[1].0).collect();
    out.dedup();
    out
}

#[cfg(test)]
#[path = "point_cloud_distribution_tests.rs"]
mod tests;
