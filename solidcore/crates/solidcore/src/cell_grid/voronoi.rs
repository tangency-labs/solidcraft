//! Voronoi cells: irregular convex cells around the hexagonal grid's centres, each centre moved by
//! a pseudo-random offset.
//!
//! # The sites
//! Site `(i, j)` is the hexagonal lattice centre `(i, j)` (see the module docs of the cell grid)
//! moved by an offset worked out from `(seed, i, j)` alone by a small integer hash: a direction and
//! a distance below `irregularity` times half the pitch `p`, the distance between neighbouring
//! centres. Two neighbouring sites therefore never coincide, and a site is the same whatever the
//! field, so a grid split over two parts lines up as the other shapes do. With irregularity 0 the
//! sites are the lattice centres and the cells are the hexagon grid's cells.
//!
//! # A cell
//! The points nearer its site than any other site, less half a web along every side: the site's
//! starting square cut by one half-plane per neighbouring site, the bisector moved half a web
//! towards the site.
//!
//! # Which sites cut a cell
//! A site is at most `d = irregularity * p / 2 <= p / 2` from its lattice centre, and no point of
//! the plane is farther than `p / sqrt 3` from the nearest lattice centre. So every point has a
//! site within `r = p / sqrt 3 + d`, and every point of a cell is within `r` of its own site. A
//! bisector that meets the cell is therefore within `r` of the site: only sites nearer than
//! `2 r <= 2.155 p` give the cell a side. Moving every bisector by the same half web adds no
//! side: a point of the cell half a web or more from all its sides is that far from every other
//! bisector too.
//!
//! The lattice centres of ring `k` (hexagonal distance `k`) are at least `k p sqrt 3 / 2` from
//! the cell's centre, so their sites are at least `k p sqrt 3 / 2 - 2 d` from its site. At
//! irregularity 1 that is `1.598 p` for ring 3, near enough to cut, and `2.464 p` for ring 4, too
//! far: the first [`RINGS`] rings are all of them, at every irregularity.
//!
//! # Clipping
//! A voronoi cell has no lattice lines: `Free` trims the cell to the field as drawn, `Whole` and
//! `Snap` keep only cells entirely inside it.

use std::f64::consts::{PI, TAU};

use crate::math::Vec2;
use crate::prism::profile::{cross2, Loop2};

use super::exact;
use super::free::{self, Cell as FreeCell};
use super::lattice::Lattice;
use super::region::{clip_halfplane, poly_area, poly_centroid, Convex, Prim, Region};
use super::vec::{ring_next, ring_prev, Vec2Ext};
use super::{
    lattice_loops, points_to_prims, push, CellGridError, CellGridPlan, CellGridSpec, CellKind, Clip,
};

/// Rings of lattice neighbours a cell is cut against: enough at irregularity 1, the largest (see
/// the module docs).
pub(crate) const RINGS: i64 = 3;
/// The starting square of a cell reaches this many pitches from its site, beyond every cut.
const START_REACH: f64 = 4.0;
/// Smallest area (mm²) of an opening kept.
const MIN_AREA: f64 = 1e-9;
/// A corner radius is clamped this fraction below the largest the polygon takes, so no side of
/// the rounded cell shrinks to nothing.
const ROUND_SLACK: f64 = 1e-6;
/// The mantissa bits of an `f64`: a hash's top 53 bits as a fraction in `[0, 1)`.
const MANTISSA_BITS: u32 = 53;

/// The splitmix64 finaliser: every bit of the input moves about half the bits of the output.
fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A hash's top bits as a fraction in `[0, 1)`.
fn unit(h: u64) -> f64 {
    (h >> (u64::BITS - MANTISSA_BITS)) as f64 / (1u64 << MANTISSA_BITS) as f64
}

/// Site `(i, j)` in the lattice frame.
fn site(lat: &Lattice, spec: &CellGridSpec, i: i64, j: i64) -> Vec2 {
    let c = lat.center(i, j);
    if spec.irregularity <= 0.0 {
        return c;
    }
    // The indices' bits as they are: a negative index hashes to its own pattern.
    let h = mix(mix(mix(u64::from(spec.seed)) ^ i as u64) ^ j as u64);
    let angle = TAU * unit(h);
    // The square root spreads the sites evenly over the disc they may move in.
    let reach = unit(mix(h)).sqrt() * spec.irregularity * lat.p / 2.0;
    c + Vec2::new(angle.cos(), angle.sin()) * reach
}

/// Cell `(i, j)` in the lattice frame, counter-clockwise, with its site; `None` when the web
/// leaves nothing of it.
pub(crate) fn cell_polygon(
    lat: &Lattice,
    spec: &CellGridSpec,
    i: i64,
    j: i64,
) -> Option<(Vec2, Vec<Vec2>)> {
    cell_polygon_within(lat, spec, i, j, RINGS)
}

/// [`cell_polygon`] cut against the sites of the first `rings` rings of neighbours.
pub(crate) fn cell_polygon_within(
    lat: &Lattice,
    spec: &CellGridSpec,
    i: i64,
    j: i64,
    rings: i64,
) -> Option<(Vec2, Vec<Vec2>)> {
    let s = site(lat, spec, i, j);
    let reach = START_REACH * lat.p;
    let mut poly = vec![
        s + Vec2::new(-reach, -reach),
        s + Vec2::new(reach, -reach),
        s + Vec2::new(reach, reach),
        s + Vec2::new(-reach, reach),
    ];
    for di in -rings..=rings {
        for dj in -rings..=rings {
            // The hexagonal distance of the neighbour: its ring.
            let ring = di.abs().max(dj.abs()).max((di + dj).abs());
            if ring == 0 || ring > rings {
                continue;
            }
            let t = site(lat, spec, i.checked_add(di)?, j.checked_add(dj)?);
            let Some(n) = (s - t).normalized() else {
                continue;
            };
            // Keep the side of the bisector towards `s`, half a web in from it.
            poly = clip_halfplane(&poly, n, n.dot((s + t) / 2.0) + lat.w / 2.0);
            if poly.len() < 3 {
                return None;
            }
        }
    }
    Some((s, poly))
}

/// `tan` of half the turn at `at` from `prev` to `next`: how far along each side a corner of
/// radius 1 reaches.
fn half_turn_tan(prev: Vec2, at: Vec2, next: Vec2) -> Option<f64> {
    let u = (at - prev).normalized()?;
    let v = (next - at).normalized()?;
    let t = cross2(u, v) / (1.0 + u.dot(v));
    t.is_finite().then_some(t)
}

/// The largest corner radius the convex polygon takes: the two corners of every side meet at most
/// at its middle.
fn max_radius(poly: &[Vec2]) -> f64 {
    let n = poly.len();
    let tan = |k: usize| -> Option<f64> {
        half_turn_tan(
            *poly.get(ring_prev(k, n))?,
            *poly.get(k)?,
            *poly.get(ring_next(k, n))?,
        )
    };
    (0..n)
        .filter_map(|k| {
            let len = poly.get(k)?.distance(*poly.get(ring_next(k, n))?);
            let reach = tan(k)? + tan(ring_next(k, n))?;
            (reach > 0.0).then(|| len / reach)
        })
        .fold(f64::INFINITY, f64::min)
}

/// A convex polygon with its corners rounded: its core (the polygon moved in by `r` on every
/// side), the radius, the outline as lines and arcs, and the area. `None` when the core
/// collapses.
fn rounded(poly: &[Vec2], r: f64) -> Option<(Convex, f64, Vec<Prim>, f64)> {
    let r = r.min(max_radius(poly) * (1.0 - ROUND_SLACK));
    let n = poly.len();
    let mut core = poly.to_vec();
    for k in 0..n {
        let (a, b) = (*poly.get(k)?, *poly.get(ring_next(k, n))?);
        let d = (b - a).normalized()?;
        let inward = Vec2::new(-d.y, d.x);
        core = clip_halfplane(&core, inward, inward.dot(a) + r);
    }
    let core = Convex::new(core)?;
    let m = core.pts.len();
    let mut prims = Vec::with_capacity(2 * m);
    let mut perimeter = 0.0;
    let outward = |k: usize| -> Option<Vec2> {
        let d = (*core.pts.get(ring_next(k, m))? - *core.pts.get(k)?).normalized()?;
        Some(Vec2::new(d.y, -d.x))
    };
    for k in 0..m {
        let (a, b) = (*core.pts.get(k)?, *core.pts.get(ring_next(k, m))?);
        let (n0, n1) = (outward(k)?, outward(ring_next(k, m))?);
        perimeter += a.distance(b);
        prims.push(Prim::line(a + n0 * r, b + n0 * r));
        // The corner at `b`, from this side's normal round to the next side's.
        let sweep = cross2(n0, n1).atan2(n0.dot(n1));
        prims.push(Prim::arc(b, r, n0.y.atan2(n0.x), sweep));
    }
    let area = poly_area(&core.pts) + perimeter * r + PI * r * r;
    Some((core, r, prims, area))
}

/// The cells of a voronoi grid over a field given as closed loops (see the module docs).
pub(crate) fn plan(field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan, CellGridError> {
    let lat = spec.lattice()?;
    let mut warnings = Vec::new();
    let (loops, edges, flattened) = lattice_loops(field, spec, &mut warnings)?;
    // Without lattice lines there is nothing to snap to.
    let usable = |c: Clip| if c == Clip::Snap { Clip::Whole } else { c };
    let field_clip = usable(spec.clip);
    let keepout_clip = usable(spec.keepout_clip.unwrap_or(spec.clip));
    let region = Region::from_loops(loops, lat.p);
    // Curves other than lines and arcs and a margin are decided on the exact edges.
    let exact = (flattened || spec.margin > 0.0).then(|| exact::ExactField::new(edges, lat.p));
    let mut out = CellGridPlan {
        warnings,
        ..CellGridPlan::default()
    };
    let Some(bounds) = region.bounds() else {
        return Ok(out);
    };
    // A cell reaches up to a pitch past its lattice cell.
    for (i, j) in lat.cells_in(&bounds.grown(lat.p))? {
        let Some((c, poly)) = cell_polygon(&lat, spec, i, j) else {
            continue;
        };
        let Some(convex) = Convex::new(poly) else {
            continue;
        };
        let round = if spec.corner_radius > 0.0 {
            rounded(&convex.pts, spec.corner_radius)
        } else {
            None
        };
        // The straight polygon holds the rounded one: what misses it misses the rounded cell.
        let mut hits = region.hits(&convex);
        if let Some(exact) = &exact {
            if !exact.meets(&exact::CellOutline::Polygon(&convex), spec.margin) {
                hits.clear();
            } else if hits.is_empty() {
                hits.push(usize::MAX);
            }
        }
        let whole = || match &round {
            Some((_, _, prims, area)) => (vec![prims.clone()], *area),
            None => (vec![points_to_prims(&convex.pts)], poly_area(&convex.pts)),
        };
        if hits.is_empty() {
            if region.contains(poly_centroid(&convex.pts)) {
                let (loops, area) = whole();
                push(&mut out, spec, (i, j, c), CellKind::Whole, loops, area);
            }
            continue;
        }
        // A cell is clipped by the mode of the loops it meets; one that meets both kinds gets
        // the stricter.
        let on_keepout = |h: &usize| region.keepout.get(*h).copied().unwrap_or(false);
        let clip = match (
            hits.iter().any(|h| !on_keepout(h)),
            hits.iter().any(on_keepout),
        ) {
            (false, true) => keepout_clip,
            (_, false) => field_clip,
            (true, true) if field_clip == Clip::Free && keepout_clip == Clip::Free => Clip::Free,
            (true, true) => Clip::Whole,
        };
        if clip != Clip::Free {
            continue;
        }
        let cell = match &round {
            Some((core, r, prims, _)) => FreeCell::Rounded {
                core,
                r: *r,
                prims: prims.clone(),
            },
            None => FreeCell::Polygon(&convex),
        };
        match free::clip_cell(&region, &cell) {
            Some(pieces) => {
                let area: f64 = pieces.iter().map(|p| p.1).sum();
                if area > MIN_AREA {
                    push(
                        &mut out,
                        spec,
                        (i, j, c),
                        CellKind::Free,
                        pieces.into_iter().map(|p| p.0).collect(),
                        area,
                    );
                }
            }
            None => out.warnings.push(format!(
                "cell ({i}, {j}) could not be trimmed to the field and was left solid"
            )),
        }
    }
    Ok(out)
}
