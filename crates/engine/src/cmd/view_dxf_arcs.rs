//! Circles and arcs for the view drawing. The drawing's curved edges come from a tessellation, so
//! a round hole arrives as a ring of short straight lines. A machining program or a CAD program
//! can't pick that as a circle. This finds the chains of visible lines that are a tessellated
//! circle or arc and writes them as `CIRCLE` and `ARC` entities; every other line stays a line.

use std::collections::{BTreeMap, HashMap};

use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use solidcraft_io::{DxfEntity, DxfShape};

/// A closed ring needs this many segments to count as a circle, an open run this many to count as
/// an arc. A hexagon or an octagon has every corner on a circle too; the chord check below turns
/// them away, and these minimums keep a few lines from being read as a curve by chance.
const MIN_CLOSED: usize = 8;
const MIN_OPEN: usize = 4;

/// The visible lines of `entities` with the tessellated circles and arcs among them replaced by
/// `CIRCLE` and `ARC`. `tolerance` is the chord tolerance the drawing was made with: a chain is a
/// curve only when its chords sag from the circle by no more than about that much.
pub(super) fn recognise(entities: Vec<DxfEntity>, tolerance: f64) -> Vec<DxfEntity> {
    let mut rest = Vec::new();
    let mut segments: Vec<(Vec2, Vec2)> = Vec::new();
    for e in entities {
        match e {
            DxfEntity { layer, shape: DxfShape::Line(a, b) } if layer == "VISIBLE" => segments.push((a, b)),
            other => rest.push(other),
        }
    }
    let extent = segments.iter().flat_map(|(a, b)| [a, b]).fold(1.0_f64, |s, p| s.max(p.x.abs()).max(p.y.abs()));
    // The two faces of a hole project to rings a hair apart; points that close are one point.
    snap(&mut segments, (tolerance * 1e-2).max(extent * 1e-9));
    let eps = extent * 1e-8;
    let key = |p: Vec2| [(p.x / eps).round() as i64, (p.y / eps).round() as i64];

    // The same segment twice (the rim of a hole seen on both of its faces) must not make a
    // junction of every point of the ring.
    let mut seen = std::collections::BTreeSet::new();
    segments.retain(|(a, b)| {
        let (ka, kb) = (key(*a), key(*b));
        ka != kb && seen.insert(if ka <= kb { (ka, kb) } else { (kb, ka) })
    });

    let mut nodes: BTreeMap<[i64; 2], Vec<usize>> = BTreeMap::new();
    for (i, (a, b)) in segments.iter().enumerate() {
        nodes.entry(key(*a)).or_default().push(i);
        nodes.entry(key(*b)).or_default().push(i);
    }

    let mut used = vec![false; segments.len()];
    let mut out = rest;
    for start in 0..segments.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let chain = chain_from(start, &segments, &nodes, &mut used, &key);
        match fit(&chain.points, chain.closed, tolerance) {
            Some(shape) => out.push(DxfEntity { layer: "VISIBLE".into(), shape }),
            None => {
                for w in chain.points.windows(2) {
                    if let [a, b] = w {
                        out.push(DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Line(*a, *b) });
                    }
                }
            }
        }
    }
    out
}

/// Moves every point to the first point seen within `reach` of it, so near-coincident points of the
/// drawing become exactly equal.
fn snap(segments: &mut [(Vec2, Vec2)], reach: f64) {
    let mut reps: Vec<Vec2> = Vec::new();
    let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut at = |p: Vec2| -> Vec2 {
        let (cx, cy) = ((p.x / reach).floor() as i64, (p.y / reach).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for i in cells.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    if let Some(r) = reps.get(*i)
                        && (*r - p).len() <= reach
                    {
                        return *r;
                    }
                }
            }
        }
        reps.push(p);
        cells.entry((cx, cy)).or_default().push(reps.len() - 1);
        p
    };
    for (a, b) in segments.iter_mut() {
        *a = at(*a);
        *b = at(*b);
    }
}

struct Chain {
    points: Vec<Vec2>,
    closed: bool,
}

/// The run of segments through `start`, continued while a point joins exactly two segments.
fn chain_from(
    start: usize,
    segments: &[(Vec2, Vec2)],
    nodes: &BTreeMap<[i64; 2], Vec<usize>>,
    used: &mut [bool],
    key: &impl Fn(Vec2) -> [i64; 2],
) -> Chain {
    if let Some(u) = used.get_mut(start) {
        *u = true;
    }
    let Some((a, b)) = segments.get(start).copied() else { return Chain { points: Vec::new(), closed: false } };
    let mut points = vec![a, b];
    let mut closed = false;
    for pass in 0..2 {
        while let Some(end) = points.last().copied() {
            let Some(joined) = nodes.get(&key(end)).filter(|n| n.len() == 2) else { break };
            let Some(next) = joined.iter().copied().find(|i| used.get(*i) == Some(&false)) else {
                // Both segments are used: the chain came back to where it began.
                closed = pass == 0 && points.len() > 2 && points.first().is_some_and(|f| key(*f) == key(end));
                break;
            };
            let Some((p, q)) = segments.get(next).copied() else { break };
            if let Some(u) = used.get_mut(next) {
                *u = true;
            }
            points.push(if key(p) == key(end) { q } else { p });
        }
        if closed {
            break;
        }
        points.reverse();
    }
    if !closed && points.len() > 2 && matches!((points.first(), points.last()), (Some(f), Some(l)) if key(*f) == key(*l)) {
        closed = true;
    }
    Chain { points, closed }
}

/// The circle or arc a chain of points is a tessellation of, or `None`.
fn fit(points: &[Vec2], closed: bool, tolerance: f64) -> Option<DxfShape> {
    let segments = points.len().saturating_sub(1);
    if segments < if closed { MIN_CLOSED } else { MIN_OPEN } {
        return None;
    }
    // A closed ring repeats its first point at the end; the fit takes each point once.
    let pts: &[Vec2] = if closed { points.get(..segments)? } else { points };
    let n = pts.len() as f64;
    let (mx, my) = (pts.iter().map(|p| p.x).sum::<f64>() / n, pts.iter().map(|p| p.y).sum::<f64>() / n);
    let (mut suu, mut suv, mut svv, mut suuu, mut svvv, mut suvv, mut svuu) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for p in pts {
        let (u, v) = (p.x - mx, p.y - my);
        suu += u * u;
        suv += u * v;
        svv += v * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-12 * (suu * svv).abs().max(1e-300) {
        return None; // a straight run
    }
    let (b1, b2) = (0.5 * (suuu + suvv), 0.5 * (svvv + svuu));
    let (uc, vc) = ((b1 * svv - b2 * suv) / det, (suu * b2 - suv * b1) / det);
    let r = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    let c = Vec2::new(mx + uc, my + vc);
    if !r.is_finite() || r <= 0.0 {
        return None;
    }
    // Every point sits on the circle.
    let slack = 1e-4 * r + 1e-6;
    if pts.iter().any(|p| ((*p - c).len() - r).abs() > slack) {
        return None;
    }
    // Consecutive points go the same way round, and each chord sags from the circle by no more than
    // the drawing's own chord tolerance (twice it, for rounding): a polygon's sides sag far more.
    let angle = |p: Vec2| (p.y - c.y).atan2(p.x - c.x);
    let mut sweep = 0.0;
    let mut direction = 0.0_f64;
    for w in points.windows(2) {
        let (a, b) = (*w.first()?, *w.get(1)?);
        let mut d = angle(b) - angle(a);
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        if d == 0.0 || (direction != 0.0 && d.signum() != direction) {
            return None;
        }
        direction = d.signum();
        let half_chord = (a.dist(b) / 2.0).min(r);
        if r - (r * r - half_chord * half_chord).sqrt() > 2.0 * tolerance {
            return None;
        }
        sweep += d;
    }
    if closed {
        return ((sweep.abs() - std::f64::consts::TAU).abs() < 1e-6).then_some(DxfShape::Circle { c, r });
    }
    if sweep.abs() >= std::f64::consts::TAU - 1e-6 {
        return None;
    }
    let (first, last) = (angle(*points.first()?), angle(*points.last()?));
    // A DXF arc runs counter-clockwise from its first angle to its second.
    let (from, to) = if sweep > 0.0 { (first, last) } else { (last, first) };
    // A start a rounding error below 0 is 0, not 360.
    let degrees = |a: f64| Some(a.to_degrees().rem_euclid(360.0)).map(|d| if 360.0 - d < 1e-9 { 0.0 } else { d }).unwrap_or(0.0);
    Some(DxfShape::Arc { c, r, a0: degrees(from), a1: degrees(to) })
}
