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
//! A voronoi cell has no lattice lines to snap to. `Whole` keeps only the cells entirely inside
//! the field and `Free` trims a cell to the field as drawn, as with the other shapes. `Snap` fills
//! the field: a cell is trimmed to the field's outline moved in by one web, so the rim along the
//! outline (and around a keep-out) is as wide as the web between two cells.
//!
//! The outline is moved piece by piece ([`inset_loops`]): a line sideways, an arc to a radius one
//! web smaller or larger. At a corner that turns towards the field the moved pieces cross and
//! are cut there; at one that turns away they are joined by an arc of one web about the corner
//! (two lines that turn only a little are lengthened until they meet). A piece the move uses up
//! (an edge or a radius shorter than the web allows) is not handled: the grid then keeps whole
//! cells only and says so. Where the field is narrower than two webs the moved outline crosses
//! itself, so every trimmed cell is measured against the outline as drawn and left solid when it
//! comes nearer than a web.

use std::f64::consts::{PI, TAU};

use crate::math::Vec2;
use crate::prism::profile::{cross2, Loop2};

use super::exact;
use super::free::{self, intersections, Cell as FreeCell};
use super::lattice::Lattice;
use super::region::{
    clip_halfplane, keepout_loops, poly_area, poly_centroid, Convex, Loop, Prim, Region,
};
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
/// A corner radius is at most this fraction of the distance from a cell's centroid to its nearest
/// side, so the cell keeps a core to round about.
const ROUND_MOST: f64 = 0.9;
/// Sides of a rounded cell's core shorter than this (mm), and corners turning less than this
/// (radians), are left out: their arcs and lines would be too short to build.
const ROUND_TOL: f64 = 1e-6;
/// Two lines of an outline that turn away from the field by less than this (radians) are
/// lengthened until they meet when the outline is moved in; a sharper corner gets an arc.
const MITER_MOST: f64 = 0.05;
/// A trimmed cell may come this much (mm) nearer the outline as drawn than one web.
const RIM_TOL: f64 = 1e-6;
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

/// The distance from the convex polygon's centroid to its nearest side.
fn centroid_clearance(poly: &[Vec2]) -> f64 {
    let n = poly.len();
    let c = poly_centroid(poly);
    (0..n)
        .filter_map(|k| {
            let (a, b) = (*poly.get(k)?, *poly.get(ring_next(k, n))?);
            let d = (b - a).normalized()?;
            Some(cross2(d, c - a))
        })
        .fold(f64::INFINITY, f64::min)
}

/// The polygon without the corners that end a side shorter than [`ROUND_TOL`] or turn less than
/// [`ROUND_TOL`].
fn without_slivers(poly: &[Vec2]) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = Vec::with_capacity(poly.len());
    for &p in poly {
        if pts.last().is_none_or(|q| q.distance(p) > ROUND_TOL) {
            pts.push(p);
        }
    }
    while pts.len() > 1
        && matches!((pts.first(), pts.last()), (Some(a), Some(b)) if a.distance(*b) <= ROUND_TOL)
    {
        pts.pop();
    }
    let n = pts.len();
    (0..n)
        .filter_map(|k| {
            let (prev, at, next) = (
                *pts.get(ring_prev(k, n))?,
                *pts.get(k)?,
                *pts.get(ring_next(k, n))?,
            );
            let (u, v) = ((at - prev).normalized()?, (next - at).normalized()?);
            (cross2(u, v).atan2(u.dot(v)).abs() > ROUND_TOL).then_some(at)
        })
        .collect()
}

/// A convex polygon with its corners rounded: its core (the polygon moved in by `r` on every
/// side, so a side too short for the radius goes), the radius, the outline as lines and arcs (the
/// points within `r` of the core), and the area. The radius is at most [`ROUND_MOST`] of the
/// centroid's clearance. `None` when no core is left.
fn rounded(poly: &[Vec2], r: f64) -> Option<(Convex, f64, Vec<Prim>, f64)> {
    let r = r.min(centroid_clearance(poly) * ROUND_MOST);
    if !(r > ROUND_TOL) {
        return None;
    }
    let n = poly.len();
    let mut core = poly.to_vec();
    for k in 0..n {
        let (a, b) = (*poly.get(k)?, *poly.get(ring_next(k, n))?);
        let d = (b - a).normalized()?;
        let inward = Vec2::new(-d.y, d.x);
        core = clip_halfplane(&core, inward, inward.dot(a) + r);
    }
    let core = Convex::new(without_slivers(&core))?;
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

/// The piece moved `w` to its left (`None` for an arc whose radius the move uses up).
fn moved_left(p: &Prim, w: f64) -> Option<Prim> {
    match *p {
        Prim::Line { a, b, .. } => {
            let d = (b - a).normalized()?;
            let n = Vec2::new(-d.y, d.x) * w;
            Some(Prim::line(a + n, b + n))
        }
        Prim::Arc {
            c, r, a0, sweep, ..
        } => {
            // A counter-clockwise arc has its centre on the left.
            let r = if sweep >= 0.0 { r - w } else { r + w };
            (r > ROUND_TOL).then(|| Prim::arc(c, r, a0, sweep))
        }
    }
}

/// The moved piece `p` between the points `from` and `to` on it; `None` when nothing of it is
/// left that runs the way it did.
fn between(p: &Prim, from: Vec2, to: Vec2) -> Option<Prim> {
    match *p {
        Prim::Line { a, b, .. } => {
            let d = (b - a).normalized()?;
            ((to - from).dot(d) > ROUND_TOL).then(|| Prim::line(from, to))
        }
        Prim::Arc {
            c, r, a0, sweep, ..
        } => {
            let turn = sweep.signum();
            // How far round the arc a point is, from just before its start to just past its end.
            let along = |x: Vec2| {
                let t = (((x.y - c.y).atan2(x.x - c.x) - a0) * turn).rem_euclid(TAU);
                if t > (sweep.abs() + TAU) / 2.0 {
                    t - TAU
                } else {
                    t
                }
            };
            // A whole circle starts and ends at one point.
            let (t0, t1) = if from.distance(to) <= ROUND_TOL && sweep.abs() >= TAU - ROUND_TOL {
                (0.0, sweep.abs())
            } else {
                (along(from), along(to))
            };
            ((t1 - t0) * r > ROUND_TOL)
                .then(|| Prim::arc(c, r, a0 + t0 * turn, (t1 - t0) * turn).with_start(from))
        }
    }
}

/// The loop (the field on its left) moved in by `w`: see the module docs. `None` when a piece is
/// used up or two moved pieces that should cross do not.
fn inset_loop(lp: &Loop, w: f64) -> Option<Loop> {
    // A rounded corner of the field no wider than the move leaves a sharp corner: its
    // neighbours meet.
    let kept: Vec<&Prim> = lp
        .prims
        .iter()
        .filter(|p| !matches!(**p, Prim::Arc { r, sweep, .. } if sweep > 0.0 && r <= w + ROUND_TOL))
        .collect();
    let n = kept.len();
    if n < 2 && n < lp.prims.len() {
        return None;
    }
    let moved: Vec<Prim> = kept
        .iter()
        .map(|p| moved_left(p, w))
        .collect::<Option<_>>()?;
    // For the corner after each piece: where the piece now ends, a joining arc if there is one,
    // and where the next piece now starts.
    let mut joints: Vec<(Vec2, Option<Prim>, Vec2)> = Vec::with_capacity(n);
    for k in 0..n {
        let (p, q) = (*kept.get(k)?, *kept.get(ring_next(k, n))?);
        let (mp, mq) = (moved.get(k)?, moved.get(ring_next(k, n))?);
        let corner = p.end();
        let joined = corner.distance(q.start()) <= ROUND_TOL;
        let (u, v) = (p.direction_at(corner), q.direction_at(q.start()));
        let turn = cross2(u, v).atan2(u.dot(v));
        let (e, s) = (mp.end(), mq.start());
        let lines = matches!((p, q), (Prim::Line { .. }, Prim::Line { .. }));
        let joint = if e.distance(s) <= ROUND_TOL {
            (e, None, e)
        } else if lines && joined && turn > -MITER_MOST {
            // Both lines moved by `w`: they meet on the corner's bisector.
            let (nu, nv) = (Vec2::new(-u.y, u.x), Vec2::new(-v.y, v.x));
            // A corner that turns back on itself has no meeting point.
            if 1.0 + nu.dot(nv) <= 1e-9 {
                return None;
            }
            let x = corner + (nu + nv) * (w / (1.0 + nu.dot(nv)));
            (x, None, x)
        } else if turn > 0.0 || !joined {
            let x = intersections(mp, mq)
                .into_iter()
                .min_by(|a, b| a.distance(corner).total_cmp(&b.distance(corner)))?;
            (x, None, x)
        } else {
            let from = e - corner;
            (
                e,
                Some(Prim::arc(corner, w, from.y.atan2(from.x), turn).with_start(e)),
                s,
            )
        };
        joints.push(joint);
    }
    let mut prims = Vec::with_capacity(2 * n);
    for k in 0..n {
        let (before, after) = (joints.get(ring_prev(k, n))?, joints.get(k)?);
        prims.push(between(moved.get(k)?, before.2, after.0)?);
        prims.extend(after.1);
    }
    Loop::new(prims, Some(lp.flattened))
}

/// The loops with the fields' outlines (`fields`) and the keep-outs' (`keepouts`) moved by `w`
/// into the field, each as asked; the others as they are. `None` when one cannot be moved.
fn inset_loops(loops: &[Loop], (fields, keepouts): (bool, bool), w: f64) -> Option<Vec<Loop>> {
    let roles = keepout_loops(loops);
    let moved: Vec<Loop> = loops
        .iter()
        .zip(&roles)
        .map(|(lp, &keepout)| {
            if !(if keepout { keepouts } else { fields }) {
                return Some(lp.clone());
            }
            // The field is on the left of a counter-clockwise outline and of a clockwise keep-out.
            let moved = inset_loop(&lp.clone().oriented(!keepout), w)?;
            // An outline that turns inside out has no field left.
            ((moved.signed_area() > 0.0) != keepout).then_some(moved)
        })
        .collect::<Option<_>>()?;
    // A moved loop that crosses another, or nests differently, would turn a keep-out into field.
    let crossed = moved
        .iter()
        .enumerate()
        .any(|(i, a)| moved.iter().skip(i + 1).any(|b| loops_cross(a, b)));
    (!crossed && keepout_loops(&moved) == roles).then_some(moved)
}

/// Whether two loops cross or touch.
fn loops_cross(a: &Loop, b: &Loop) -> bool {
    a.bounds().overlaps(&b.bounds(), 0.0)
        && a.prims.iter().any(|p| {
            b.prims
                .iter()
                .any(|q| p.bounds().overlaps(&q.bounds(), 0.0) && !intersections(p, q).is_empty())
        })
}

/// A trimmed cell's piece as a convex polygon, when it is one of straight sides.
fn convex_piece(prims: &[Prim]) -> Option<Vec<Vec2>> {
    if !prims.iter().all(|p| matches!(p, Prim::Line { .. })) {
        return None;
    }
    let pts = without_slivers(&prims.iter().map(Prim::start).collect::<Vec<_>>());
    let n = pts.len();
    let convex = n >= 3
        && (0..n).all(|k| {
            matches!(
                (pts.get(ring_prev(k, n)), pts.get(k), pts.get(ring_next(k, n))),
                (Some(&a), Some(&b), Some(&c)) if cross2(b - a, c - b) > 0.0
            )
        });
    convex.then_some(pts)
}

/// Whether every piece keeps `w` clear of the outline as drawn (measured at each of its pieces'
/// ends and middles and at the outline's corners, and never crossing it), for the fields' outlines (`fields`) and the keep-outs' (`keepouts`) as asked.
fn keeps_rim(
    outline: &Region,
    (fields, keepouts): (bool, bool),
    pieces: &[(Vec<Prim>, f64)],
    w: f64,
) -> bool {
    pieces.iter().flat_map(|p| p.0.iter()).all(|p| {
        let near = outline.near(&p.bounds().grown(w));
        near.iter()
            .filter(|&&i| match outline.keepout.get(i) {
                Some(true) => keepouts,
                _ => fields,
            })
            .filter_map(|&i| outline.prims.get(i))
            .all(|o| {
                // The piece's ends and middle against the outline, the outline piece's start
                // against the piece (the nearest point of a piece passing a corner lies between
                // its samples), and no crossing.
                [p.start(), p.mid()]
                    .iter()
                    .all(|&x| o.distance(x) >= w - RIM_TOL)
                    && p.distance(o.start()) >= w - RIM_TOL
                    && intersections(p, o).is_empty()
            })
    })
}

/// The cells of a voronoi grid over a field given as closed loops (see the module docs).
pub(crate) fn plan(field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan, CellGridError> {
    let lat = spec.lattice()?;
    let mut warnings = Vec::new();
    let (loops, edges, flattened) = lattice_loops(field, spec, &mut warnings)?;
    let mut field_clip = spec.clip;
    let mut keepout_clip = spec.keepout_clip.unwrap_or(spec.clip);
    // `Snap` fills the field up to a rim of one web: the cells are trimmed to the outline moved
    // in by the web (see the module docs).
    let fills = (field_clip == Clip::Snap, keepout_clip == Clip::Snap);
    let mut rim = None;
    let mut loops = loops;
    if (fills.0 || fills.1) && lat.w > 0.0 {
        if let Some(moved) = inset_loops(&loops, fills, lat.w) {
            rim = Some(Region::from_loops(loops, lat.p));
            loops = moved;
            if spec.margin > 0.0 {
                warnings.push(
                    "the margin is not used where the field is filled up to a rim of one web"
                        .into(),
                );
            }
        } else {
            warnings.push(
                "the field's outline could not be moved in by the web, so only whole cells were \
                 kept"
                    .into(),
            );
            let whole = |c: Clip| if c == Clip::Snap { Clip::Whole } else { c };
            (field_clip, keepout_clip) = (whole(field_clip), whole(keepout_clip));
        }
    }
    let region = Region::from_loops(loops, lat.p);
    // Curves other than lines and arcs and a margin are decided on the exact edges. A filled
    // field is decided on its moved outline, which has no exact edges.
    let exact = ((flattened || spec.margin > 0.0) && rim.is_none())
        .then(|| exact::ExactField::new(edges, lat.p));
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
            (true, true) if field_clip != Clip::Whole && keepout_clip != Clip::Whole => {
                // Trimmed by both; the rim is checked when either has one.
                if field_clip == Clip::Snap {
                    field_clip
                } else {
                    keepout_clip
                }
            }
            (true, true) => Clip::Whole,
        };
        if clip == Clip::Whole {
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
        let mut pieces = free::clip_cell(&region, &cell);
        if let (Clip::Snap, Some(outline)) = (clip, &rim) {
            // A corner the rim cut is rounded like the others when the piece allows it.
            if round.is_some() {
                let straight = free::clip_cell(&region, &FreeCell::Polygon(&convex));
                if let Some([(prims, _)]) = straight.as_deref() {
                    if let Some((_, _, prims, area)) =
                        convex_piece(prims).and_then(|pts| rounded(&pts, spec.corner_radius))
                    {
                        pieces = Some(vec![(prims, area)]);
                    }
                }
            }
            if pieces
                .as_ref()
                .is_some_and(|p| !keeps_rim(outline, fills, p, lat.w))
            {
                out.warnings.push(format!(
                    "cell ({i}, {j}) would leave a rim thinner than the web and was left solid"
                ));
                continue;
            }
        }
        match pieces {
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
