//! What is left held against the part: cuts into it (gouges), measured in 3D over the material
//! each cut column runs through, and the material left on it where the program cut.

use super::*;

pub(super) struct Comparison {
    pub(super) worst: f64,
    pub(super) worst_at: Vec3,
    pub(super) gouged: usize,
    pub(super) unrefined: usize,
    pub(super) leftover_max: f64,
    pub(super) leftover_at: Vec3,
    pub(super) leftover_sum: f64,
    pub(super) machined: usize,
}

impl Default for Comparison {
    fn default() -> Comparison {
        Comparison {
            worst: 0.0,
            worst_at: Vec3::ZERO,
            gouged: 0,
            unrefined: 0,
            leftover_max: 0.0,
            leftover_at: Vec3::ZERO,
            leftover_sum: 0.0,
            machined: 0,
        }
    }
}

impl Comparison {
    pub(super) fn add(&mut self, o: &Comparison) {
        if o.worst > self.worst {
            (self.worst, self.worst_at) = (o.worst, o.worst_at);
        }
        self.gouged += o.gouged;
        self.unrefined += o.unrefined;
        if o.leftover_max > self.leftover_max {
            (self.leftover_max, self.leftover_at) = (o.leftover_max, o.leftover_at);
        }
        self.leftover_sum += o.leftover_sum;
        self.machined += o.machined;
    }
}

/// The distance from `p` to `surface`, or `cap` when that is nearer.
pub(crate) fn distance_within(surface: &Surface, p: Vec3, cap: f64) -> f64 {
    let best = Cell::new(cap);
    surface.visit(
        |b: &Aabb3| {
            let q = p.max(b.min).min(b.max);
            p.dist(q) < best.get()
        },
        |_, t| {
            let d = p.dist(nearest_on_triangle(p, t));
            if d < best.get() {
                best.set(d);
            }
        },
    );
    best.get()
}

/// Most crossings of the part's surface followed along one column when measuring a gouge.
const MAX_CROSSINGS: usize = 64;

/// How deep a cut along the straight column from `from` (outside the part) to `to` (where the
/// cut ends) goes into the part, at most `cap`. The part's material along the column is found
/// from where it crosses the part's surface: a triangle facing against the column starts
/// material, one facing along it ends it (a crossing that does neither, as where the column
/// grazes an edge, is passed over). Each stretch of material the column runs through is
/// measured from its middle to the part's surface, and the column's end when it is inside the
/// material. Air between the part's surfaces, as down a hole or a pocket, is no gouge however
/// far it is from them; a stretch of material, however thin, is as deep as its middle.
///
/// Where the crossings do not pair up that way (a part whose triangles face inwards, bodies
/// that overlap, more crossings than are followed), the
/// material cannot be told from the air, and the column is measured as if it were all
/// material: from its end, and from the middle of its stretch past the first crossing, to the
/// part's surface. That never passes a gouge as air. A column that starts inside the part (the
/// surface behind its start faces against it) starts in its material.
pub(crate) fn column_depth(part: &Surface, from: Vec3, to: Vec3, cap: f64) -> f64 {
    let d = to - from;
    let len = d.len();
    if !(len > 1e-12) || !len.is_finite() {
        return 0.0;
    }
    let step = 1e-9 / len;
    let measure = |a: f64, b: f64| distance_within(part, from + d * (0.5 * (a + b)), cap);
    let facing = |i: usize| part.triangle(i).map_or(0.0, |tri| (tri[1] - tri[0]).cross(tri[2] - tri[0]).dot(d));
    let (mut t0, mut inside, mut start, mut worst) = (0.0f64, false, 0.0f64, 0.0f64);
    let (mut first, mut paired, mut ended) = (None, true, false);
    // A column that starts inside the part: the surface behind its start faces against it.
    if let Some((_, i)) = part.raycast(from, -d)
        && facing(i) < 0.0
    {
        (inside, first) = (true, Some(0.0));
    }
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
        let facing = facing(i);
        if facing != 0.0 {
            first.get_or_insert(at);
        }
        if facing < 0.0 && !inside {
            (inside, start) = (true, at);
        } else if facing > 0.0 && inside {
            inside = false;
            worst = worst.max(measure(start, at));
        } else if facing != 0.0 {
            paired = false;
            break;
        }
        t0 = at + step;
    }
    if !(paired && ended) {
        return distance_within(part, to, cap).max(measure(first.unwrap_or(0.0), 1.0)).min(cap);
    }
    if inside {
        worst = worst.max(measure(start, 1.0)).max(distance_within(part, to, cap));
    }
    worst.min(cap)
}

/// For a column at `(x, y)` cut down to `z` that meets no part from above: the height its
/// leftover is measured down to when it is in a hole through the part (the part's bottom, or the
/// stock's `bottom` where that is higher), the part all round it halfway up the stock it keeps.
/// `None` outside the part, or with no stock kept beside the part.
pub(super) fn through_hole_floor(part: &Surface, x: f64, y: f64, z: f64, bottom: f64, tol: f64) -> Option<f64> {
    let b = part.bounds();
    let (lo, hi) = (b.min.z.max(bottom), z.min(b.max.z));
    if hi <= lo + tol {
        return None;
    }
    let mid = Vec3::new(x, y, 0.5 * (lo + hi));
    let reach = (b.max - b.min).len() + 1.0;
    let dirs = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0)];
    dirs.iter().all(|d| part.raycast(mid, *d * reach).is_some()).then_some(lo)
}

/// The stock left in `model` (a band) against the part; cells the program cut are those below
/// `initial` (the stock's top). At most `refine` gouged cells are measured in 3D.
pub(super) fn compare(model: &Model, part: &Surface, tol: f64, initial: f64, refine: usize) -> Comparison {
    let mut cmp = Comparison::default();
    let (mut refined, mut unrefined, mut measured) = (0usize, 0usize, 0usize);
    // The depth into the part of a column cut from `point(outer)` (outside the part) in to `at`,
    // `gap` deep below the part's surface along it: `point(v)` is the column's point at height
    // `v`. Measured in 3D over the material the column runs through ([`column_depth`]).
    let mut depth = |gap: f64, at: f64, outer: f64, point: &dyn Fn(f64) -> Vec3| -> f64 {
        if refined >= refine {
            unrefined += 1;
            return gap;
        }
        refined += 1;
        column_depth(part, point(outer), point(at), gap)
    };
    let gouge = |cmp: &mut Comparison, d: f64, at: Vec3| {
        if d > tol {
            cmp.gouged += 1;
            if d > cmp.worst {
                cmp.worst = d;
                cmp.worst_at = at;
            }
        }
    };
    let leftover = |cmp: &mut Comparison, left: f64, at: Vec3| {
        cmp.machined += 1;
        cmp.leftover_sum += left;
        if left > cmp.leftover_max {
            cmp.leftover_max = left;
            cmp.leftover_at = at;
        }
    };
    // How thick a column's material left outside the part is: `len` along the column, measured
    // in 3D (from its end, `top`, and its middle to the part's surface) when it could be the most
    // so far, so a sliver beside a steep wall that a cell's centre falls in counts as thin as it
    // is there, not the wall's height. Floors keep their height (the end is that far above).
    let thickness = |measured: &mut usize, len: f64, most: f64, top: Vec3, middle: Vec3| -> f64 {
        if len > tol && len > most && *measured < refine {
            *measured += 1;
            distance_within(part, top, len).max(distance_within(part, middle, len))
        } else {
            len
        }
    };
    match model {
        Model::Heights(h) => {
            let span = (h.top - h.bottom).max(0.0) + 2.0;
            // A column the program never cut, above all of the part: nothing to look at there.
            let part_top = part.bounds().max.z;
            for j in 0..h.ny {
                for i in 0..h.nx {
                    let Some(&z) = h.h.get(j * h.nx + i) else { continue };
                    if z >= initial - 1e-9 && part_top <= z + tol {
                        continue;
                    }
                    let (x, y) = h.centre(i, j);
                    let from = Vec3::new(x, y, h.top + 1.0);
                    let Some((t, _)) = part.raycast(from, Vec3::new(0.0, 0.0, -span)) else {
                        // No part under the column: outside the part, or down a hole through it.
                        // Stock the program left in a through hole is leftover, from the part's
                        // bottom up.
                        if z < initial - 1e-9
                            && let Some(base) = through_hole_floor(part, x, y, z, h.bottom, tol)
                        {
                            let left = thickness(
                                &mut measured,
                                (z - base).max(0.0),
                                cmp.leftover_max,
                                Vec3::new(x, y, z),
                                Vec3::new(x, y, 0.5 * (z + base)),
                            );
                            leftover(&mut cmp, left, Vec3::new(x, y, z));
                        }
                        continue;
                    };
                    let surface = h.top + 1.0 - t * span;
                    let gap = z - surface;
                    if gap < -tol {
                        let d = depth(-gap, z, h.top + 1.0, &|v| Vec3::new(x, y, v));
                        gouge(&mut cmp, d, Vec3::new(x, y, z));
                    }
                    if z < initial - 1e-9 {
                        // No stock below the stock's bottom to leave.
                        let base = surface.max(h.bottom);
                        let left =
                            thickness(&mut measured, (z - base).max(0.0), cmp.leftover_max, Vec3::new(x, y, z), Vec3::new(x, y, 0.5 * (z + base)));
                        leftover(&mut cmp, left, Vec3::new(x, y, z));
                    }
                }
            }
        }
    }
    cmp.unrefined = unrefined;
    cmp
}
