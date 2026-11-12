//! The flat of a sheet made from surfaces (#6748): the pieces its development lays out (each
//! placed panel less its trims, the bend zones, and the tabs cuts leave), and the planar booleans
//! they are cut and joined by. The flat pattern and the folded solid are both made of these
//! pieces, edge for edge.

use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_develop::{Development, crossings};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb2, Ring};
use solidcraft_kernel::BoolOp;

use crate::sheet::FlatWork;
use crate::sheet_surface_cuts::{holds, near_each_other, region_holds};
use crate::{DocError, Result};

/// A piece of a development's flat: a panel's material less its trims, a bend's zone (by its
/// place in the development's zones), or a tab: a piece of a zone that the cuts leave joined to
/// one of its panels only, which rides with that panel, flat, where the rest of the zone bends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Panel(usize),
    Zone(usize),
    /// A piece of zone `.0`, riding with panel `.1`.
    Tab(usize, usize),
}

/// How near (mm) a cut may come to a piece's loops and still be a hole of it as it is: as near
/// as the sew joins points.
const CLEAR: f64 = 1e-6;

/// How far (mm) a piece's edge may lie off a zone's tangent line and still run along it.
const ON_LINE: f64 = 1e-7;

/// A development's pieces before they are joined: each placed panel less its trims, and the
/// zones, all less what the development cuts from the flat (the corners' reliefs and the sheet's
/// cuts), with each piece of a zone sorted by what still holds it (see [`sort_zones`]), and their
/// longest arcs halved (see [`halved`]). The flat pattern and the folded solid are both made of
/// them, edge for edge.
pub(crate) fn pieces(dev: &Development, work: &mut FlatWork) -> Result<Vec<(Piece, Region2)>> {
    #[cfg(test)]
    crate::eval::cost_tests::count_flat();
    let mut pieces = Vec::new();
    for (i, flat) in dev.panels.iter().enumerate() {
        let Some(flat) = flat else { continue };
        let placed = on_grid(&flat.region);
        let trims: Vec<Region2> = dev.trims.iter().filter(|t| t.panel == i).map(|t| on_grid(&t.region)).collect();
        if trims.is_empty() {
            pieces.push((Piece::Panel(i), placed));
        } else {
            // Until the kernel's planar booleans take several tools that share an edge (#6748):
            // trims of two bends can share an edge and a corner with the panel, which the
            // planar booleans take apart only one tool at a time, so join them first.
            let trims = stage(&format!("joining panel {i}'s trims"), planar_union(trims, work))?;
            let left = stage(&format!("trimming panel {i}"), boolean_past_holes(&[placed], &trims, BoolOp::Cut, work))?;
            pieces.extend(left.into_iter().map(|r| (Piece::Panel(i), r)));
        }
    }
    pieces.extend(dev.zones.iter().enumerate().map(|(z, zone)| (Piece::Zone(z), on_grid(&zone.region))));
    if dev.cuts.is_empty() {
        return Ok(halved(pieces));
    }
    // A cut clear inside one piece is a hole of it as it is, its curves kept; the rest are cut
    // from every piece they reach. A cut that came clear of the pieces but meets what the others
    // cut from them is cut the same way, after them.
    let cuts: Vec<Region2> = dev.cuts.iter().map(on_grid).collect();
    let (inside, across): (Vec<&Region2>, Vec<&Region2>) = cuts.iter().partition(|c| pieces.iter().any(|(_, r)| holds_clear(r, c)));
    let mut out = cut_from(pieces, &across, work)?;
    let mut late = Vec::new();
    for c in inside {
        match out.iter_mut().find(|(_, r)| holds_clear(r, c)) {
            Some((_, r)) => r.holes.push(c.outer.ccw().reversed()),
            None => late.push(c),
        }
    }
    Ok(halved(sort_zones(dev, cut_from(out, &late, work)?)?))
}

/// The pieces with each arc of more than three quarters of a turn (a hole's whole circle) halved
/// at the middle of its own sweep: two arcs of its circle that meet where the one did. Until the
/// kernel (solidcore, #1597) takes a circular edge round any sweep, and a whole circle on one
/// vertex, the folded solid's wall along such an arc is two faces, one on each half (see
/// `sheet_surface_fold::cells`, which takes these pieces' arcs as they are). The flat keeps the
/// same two edges: an edge of the flat is an edge of the folded sheet.
fn halved(pieces: Vec<(Piece, Region2)>) -> Vec<(Piece, Region2)> {
    let halves = |l: Loop2| Loop2 {
        segs: l
            .segs
            .into_iter()
            .flat_map(|e| match e {
                Seg2::Arc { center, radius, start, sweep } if sweep.abs() > std::f64::consts::PI * 1.5 => {
                    let half = sweep / 2.0;
                    vec![Seg2::Arc { center, radius, start, sweep: half }, Seg2::Arc { center, radius, start: start + half, sweep: half }]
                }
                e => vec![e],
            })
            .collect(),
    };
    pieces.into_iter().map(|(piece, r)| (piece, Region2 { outer: halves(r.outer), holes: r.holes.into_iter().map(halves).collect() })).collect()
}

/// `pieces` less `cuts`, joined first: what each piece leaves, each a piece of the same kind.
fn cut_from(pieces: Vec<(Piece, Region2)>, cuts: &[&Region2], work: &mut FlatWork) -> Result<Vec<(Piece, Region2)>> {
    if cuts.is_empty() {
        return Ok(pieces);
    }
    let cuts = stage("joining what is cut from the flat", planar_union(cuts.iter().map(|c| (*c).clone()).collect(), work))?;
    let mut out = Vec::with_capacity(pieces.len());
    for (piece, region) in pieces {
        out.extend(stage("cutting the flat", boolean_past_holes(&[region], &cuts, BoolOp::Cut, work))?.into_iter().map(|r| (piece, r)));
    }
    Ok(out)
}

/// Whether `cut` lies clear inside `region`, so that it is a hole of it as it is: within its
/// outline, outside its holes, crossing none of its loops and nowhere nearer them than
/// [`CLEAR`].
pub(crate) fn holds_clear(region: &Region2, cut: &Region2) -> bool {
    let Some(start) = cut.outer.segs.first().map(Seg2::start) else { return false };
    if !cut.holes.is_empty() || !region_holds(region, start) {
        return false;
    }
    // Chords this far inside the curves: two loops whose chords stay farther apart than twice
    // that and `CLEAR` are clear of each other.
    let sag = 1e-4;
    let ring = cut.outer.polyline(sag);
    // Boxes that hold the loops' curves (see [`loop_box`]): a loop whose box stays farther than
    // `CLEAR` from the cut's is clear of it.
    let reach = loop_box(&cut.outer);
    let near = |b: &Aabb2| b.min.x <= reach.max.x && reach.min.x <= b.max.x && b.min.y <= reach.max.y && reach.min.y <= b.max.y;
    let gap = |a: &Ring<Vec2>, b: &Ring<Vec2>| {
        a.iter().map(|p| b.edges().map(|(q0, q1)| segment_distance(*p, *q0, *q1)).fold(f64::INFINITY, f64::min)).fold(f64::INFINITY, f64::min)
    };
    std::iter::once(&region.outer).chain(&region.holes).all(|l| {
        let other = l.polyline(sag);
        !near(&crate::sheet_surface_cuts::loop_box(l))
            || (l.segs.first().is_none_or(|s| !holds(&cut.outer, s.start()))
                && crossings(&cut.outer.segs, &l.segs).is_empty()
                && gap(&ring, &other).min(gap(&other, &ring)) > 2.0 * sag + CLEAR)
    })
}

/// How far `p` lies from the segment from `a` to `b`.
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

/// The pieces with each piece of a zone sorted by what still holds it: one that runs along both
/// its panels' tangent lines bends with the zone; one that runs along only one is a tab of that
/// panel. A piece that runs along neither is refused. A zone left with nothing that bends (cuts
/// part the sheet there) leaves its panels as pieces of their own, each where the bend held it.
fn sort_zones(dev: &Development, pieces: Vec<(Piece, Region2)>) -> Result<Vec<(Piece, Region2)>> {
    let mut out = Vec::with_capacity(pieces.len());
    for (piece, region) in pieces {
        let Piece::Zone(z) = piece else {
            out.push((piece, region));
            continue;
        };
        let zone = dev.zones.get(z).ok_or_else(|| DocError::Invalid(format!("the flat has no bend {}", z + 1)))?;
        let sorted = match (along(&region, zone.near), along(&region, zone.far)) {
            (true, true) => Piece::Zone(z),
            (true, false) => Piece::Tab(z, zone.panels[0]),
            (false, true) => Piece::Tab(z, zone.panels[1]),
            (false, false) => {
                return Err(DocError::Invalid(format!("a cut leaves a piece of bend {} joined to neither of its panels: not supported yet", z + 1)));
            }
        };
        out.push((sorted, region));
    }
    Ok(out)
}

/// Whether an edge of `region`'s outline runs along the line from `a` to `b` for more than a
/// rounding.
pub(crate) fn along(region: &Region2, [a, b]: [Vec2; 2]) -> bool {
    let l = a.dist(b);
    let Some(u) = (b - a).normalized() else { return false };
    let on = |p: Vec2| (p - a).cross(u).abs() <= ON_LINE && (-ON_LINE..=l + ON_LINE).contains(&(p - a).dot(u));
    region.outer.segs.iter().any(|s| matches!(s, Seg2::Line { .. }) && s.length() > ON_LINE && on(s.start()) && on(s.end()))
}

/// A development's pieces (see [`pieces`]) joined into its flat.
pub(crate) fn join_pieces(pieces: &[(Piece, Region2)], work: &mut FlatWork) -> Result<Vec<Region2>> {
    let regions: Vec<Region2> = pieces.iter().map(|(_, r)| r.clone()).collect();
    // The holes clear of every other piece, and of their own piece's outline and other holes
    // (see [`holes_apart`]), stay out of the union, and go back after it. A hole that touches its
    // piece's outline (a cut-out with a corner on the sheet's edge, which the planar booleans
    // read so) is the outline there: it goes through the union with it.
    let reach: Vec<Aabb2> = regions.iter().map(|r| loop_box(&r.outer)).collect();
    let mut carried: Vec<Loop2> = Vec::new();
    let mut bare = Vec::with_capacity(regions.len());
    for (i, r) in regions.into_iter().enumerate() {
        let others: Vec<Aabb2> = reach.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, b)| *b).collect();
        let apart = holes_apart(&r);
        let mut near = Vec::new();
        for (h, apart) in r.holes.into_iter().zip(apart) {
            if apart && boxes_clear(&loop_box(&h), &others) {
                carried.push(h);
            } else {
                near.push(h);
            }
        }
        bare.push(Region2 { outer: r.outer, holes: near });
    }
    let joined = stage("joining the sheet's pieces", planar_union(bare, work))?;
    stage("joining the sheet's pieces", put_back(joined, carried))
}

/// A loop's box, out by [`CLEAR`]: the box that holds its segments (see
/// [`crate::sheet_surface_cuts::loop_box`]).
fn loop_box(l: &Loop2) -> Aabb2 {
    crate::sheet_surface_cuts::loop_box(l).inflated(CLEAR)
}

/// Whether box `b` meets none of `of`.
fn boxes_clear(b: &Aabb2, of: &[Aabb2]) -> bool {
    of.iter().all(|o| b.max.x < o.min.x || o.max.x < b.min.x || b.max.y < o.min.y || o.max.y < b.min.y)
}

/// Whether loops `h` and `l`, each with its box (see [`loop_box`]), keep farther apart than
/// [`CLEAR`]: their boxes apart, or no segment of one that near a segment of the other, read
/// exactly (see [`near_each_other`]). A pair of curves no exact reading is known for is taken as
/// near: its loop goes through the planar booleans, which read it exactly.
fn loops_apart((h, hb): (&Loop2, Aabb2), (l, lb): (&Loop2, Aabb2)) -> bool {
    boxes_clear(&hb, std::slice::from_ref(&lb)) || h.segs.iter().all(|s| l.segs.iter().all(|e| matches!(near_each_other(s, e, CLEAR), Ok(false))))
}

/// For each hole of `region`, whether it keeps clear of the region's outline and its other holes
/// (see [`loops_apart`]). A hole that touches one of them, at a point or along an edge, bounds the
/// region's material with it: the planar booleans read a cut-out with a corner on the outline as
/// a hole that meets the outline at that corner. Such a hole is not a hole of the region as it
/// is: a boolean of the region takes it with the outline, never past it (see [`put_back`]).
fn holes_apart(region: &Region2) -> Vec<bool> {
    let loops: Vec<(&Loop2, Aabb2)> = std::iter::once(&region.outer).chain(&region.holes).map(|l| (l, loop_box(l))).collect();
    loops.iter().enumerate().skip(1).map(|(k, h)| loops.iter().enumerate().all(|(j, l)| j == k || loops_apart(*h, *l))).collect()
}

/// The holes `carried` back in the regions of `out` that hold them: each kept clear of every
/// other loop (see [`holes_apart`] and [`loop_clear_of`]), so that the region holding a point of
/// it holds it all, read exactly (see [`holds`]).
fn put_back(mut out: Vec<Region2>, carried: Vec<Loop2>) -> Result<Vec<Region2>> {
    for h in carried {
        let Some(at) = h.segs.first().map(Seg2::start) else { continue };
        let r = out
            .iter_mut()
            .find(|r| holds(&r.outer, at) && !r.holes.iter().any(|k| holds(k, at)))
            .ok_or_else(|| DocError::Invalid("a cut-out of the flat lost its piece".into()))?;
        r.holes.push(h);
    }
    Ok(out)
}

/// Whether loop `h` keeps clear of region `t`: apart from it by their boxes, or farther than
/// [`CLEAR`] from each of its loops (see [`loops_apart`]), outside its material and holding none
/// of it. Apart from every loop of `t`, `h` lies all inside or all outside each, and each of them
/// all inside or all outside `h`: one point of each answers, exactly (see [`holds`]).
fn loop_clear_of(h: &Loop2, t: &Region2) -> bool {
    let hb = loop_box(h);
    if boxes_clear(&hb, std::slice::from_ref(&loop_box(&t.outer))) {
        return true;
    }
    let loops: Vec<&Loop2> = std::iter::once(&t.outer).chain(&t.holes).collect();
    if !loops.iter().all(|l| loops_apart((h, hb), (*l, loop_box(l)))) {
        return false;
    }
    let Some(p) = h.segs.first().map(Seg2::start) else { return false };
    let within = holds(&t.outer, p) && !t.holes.iter().any(|k| holds(k, p));
    !within && !loops.iter().any(|l| l.segs.first().is_some_and(|s| holds(h, s.start())))
}

/// `op` of `a` with `b` ([`planar_boolean`]), the holes of `a` clear of every region of `b`
/// (see [`loop_clear_of`]) and of their own region's other loops (see [`holes_apart`]) left out
/// of it and put back in the region of the result that holds them.
pub(crate) fn boolean_past_holes(a: &[Region2], b: &[Region2], op: BoolOp, work: &mut FlatWork) -> Result<Vec<Region2>> {
    let clear_of_tools = |h: &Loop2| b.iter().all(|t| loop_clear_of(h, t));
    let mut carried: Vec<Loop2> = Vec::new();
    let bare: Vec<Region2> = a
        .iter()
        .map(|r| {
            let mut near = Vec::new();
            for (h, apart) in r.holes.iter().zip(holes_apart(r)) {
                if apart && clear_of_tools(h) {
                    carried.push(h.clone());
                } else {
                    near.push(h.clone());
                }
            }
            Region2 { outer: r.outer.clone(), holes: near }
        })
        .collect();
    if carried.is_empty() {
        return planar_boolean(a, b, op, work);
    }
    put_back(planar_boolean(&bare, b, op, work)?, carried)
}

/// How far apart (mm) the planar booleans let a loop's segments end and still close it: the
/// kernel's own reading of a loop, which moves nothing.
const CLOSE: f64 = 1e-7;

/// `op` of `a` with `b` by the kernel's planar booleans, as the regions are: every edge of the
/// result is a piece of an edge of `a` or `b`, however short, and no corner moves. The work is
/// charged to `work`, which bounds the flat's.
pub(crate) fn planar_boolean(a: &[Region2], b: &[Region2], op: BoolOp, work: &mut FlatWork) -> Result<Vec<Region2>> {
    let edges =
        |rs: &[Region2]| rs.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).fold(0usize, |n, l| n.saturating_add(l.segs.len()));
    let n = edges(a).saturating_add(edges(b));
    work.charge(n.saturating_mul(n).saturating_mul(16))?;
    Ok(solidcraft_kernel::planar_regions_boolean(a, b, op, CLOSE)?)
}

/// The union of `regions` ([`planar_boolean`]), in balanced rounds: many pieces side by side do
/// not each meet a growing union one at a time.
pub(crate) fn planar_union(regions: Vec<Region2>, work: &mut FlatWork) -> Result<Vec<Region2>> {
    let mut groups: Vec<Vec<Region2>> = regions.into_iter().map(|r| vec![r]).collect();
    while groups.len() > 1 {
        let mut next = Vec::with_capacity(groups.len().div_ceil(2));
        let mut iter = groups.into_iter();
        while let Some(a) = iter.next() {
            match iter.next() {
                Some(b) => next.push(planar_boolean(&a, &b, BoolOp::Union, work)?),
                None => next.push(a),
            }
        }
        groups = next;
    }
    Ok(groups.pop().unwrap_or_default())
}

/// A region with its points on a nanometer grid. Until the kernel's planar booleans weld edges
/// that nearly coincide (#6748): the development is exact, but two pieces can reach one corner by
/// different sums and land a rounding apart, and the planar booleans take two edges that nearly
/// coincide for two, where on the grid they coincide.
fn on_grid(r: &Region2) -> Region2 {
    let g = |v: Vec2| Vec2::new((v.x * 1e9).round() / 1e9, (v.y * 1e9).round() / 1e9);
    let seg = |s: &Seg2| match *s {
        Seg2::Line { a, b } => Seg2::Line { a: g(a), b: g(b) },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: g(center), radius, start, sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: g(p0), p1: g(p1), p2: g(p2), p3: g(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: g(a), apex: g(apex), b: g(b), w },
        Seg2::Bezier { n, mut p } => {
            for q in p.iter_mut().take(usize::from(n) + 1) {
                *q = g(*q);
            }
            Seg2::Bezier { n, p }
        }
    };
    let lp = |l: &Loop2| Loop2 { segs: l.segs.iter().map(seg).collect() };
    Region2 { outer: lp(&r.outer), holes: r.holes.iter().map(lp).collect() }
}

/// Names the step of making the flat an error came from.
fn stage<T>(what: &str, r: Result<T>) -> Result<T> {
    r.map_err(|e| DocError::Invalid(format!("{what}: {e}")))
}
