//! Thin walls: the regions a wall of constant thickness covers along sketch curves (Thin
//! Extrude). Curves that meet end to end are walked as one chain, open or closed; the wall runs
//! `inside` to the left of each chain and `outside` to its right. A closed chain runs
//! counter-clockwise, so its inside is the area it encloses; an open chain runs so that it
//! bends to the left (its inside is the side it curls toward, as a U's inside), and a straight
//! one runs the way its first listed curve was drawn.
//!
//! The wall is the union of a band along every segment (squared off at its ends) and, where
//! segments meet at a corner, a mitred piece that closes the outer side of the corner. The
//! bands' outlines (offsets and end lines) and the mitres are laid out as sketch curves, the
//! sketch's profile detection splits them where they cross, and the faces inside the wall are
//! merged into regions. Walls that cross or meet (a honeycomb, a ring and a cross) come out as
//! one region where they touch.

use std::f64::consts::{PI, TAU};

use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use crate::model::{CurveKind, Result, Sketch, SketchError};
use crate::profiles::{find_profiles, merge_regions};

/// Most curve segments one wall is built along.
pub const MAX_THIN_SEGMENTS: usize = 2_000;
/// End points closer than this are one vertex of the curves (mm).
const MERGE_TOL: f64 = 1e-6;
/// A mitre longer than this many wall thicknesses is cut off square (a bevel).
const MITRE_LIMIT: f64 = 4.0;
/// Free-form offsets are approximated by cubics within this distance (mm).
const FIT_TOL: f64 = 1e-4;
/// Most cubic pieces the offset of one free-form segment is approximated with.
const MAX_FIT_PIECES: usize = 256;

fn bad(message: impl Into<String>) -> SketchError {
    SketchError::Invalid(message.into())
}

/// The regions of a wall `inside` thick to the left of the curves and `outside` to their
/// right (see the module docs for which way the curves run). `curves` holds each selected
/// curve's exact segments in its drawn direction, in the order the curves were listed.
pub fn thin_wall_regions(curves: &[Vec<Seg2>], inside: f64, outside: f64) -> Result<Vec<Region2>> {
    if !(inside.is_finite() && outside.is_finite()) || inside < 0.0 || outside < 0.0 {
        return Err(bad("the wall thickness must be a positive length"));
    }
    if inside + outside <= 1e-6 {
        return Err(bad("the wall thickness must be positive"));
    }
    if inside.max(outside) > crate::MAX_COORD {
        return Err(bad("the wall thickness is too large"));
    }
    // Every segment with the index of the curve it belongs to.
    let mut segs: Vec<(usize, Seg2)> = Vec::new();
    for (k, curve) in curves.iter().enumerate() {
        for s in curve {
            if !seg_is_finite(s) {
                return Err(bad("a wall curve is not finite"));
            }
            if s.length() > 1e-9 {
                segs.push((k, *s));
            }
            if segs.len() > MAX_THIN_SEGMENTS {
                return Err(SketchError::TooLarge);
            }
        }
    }
    if segs.is_empty() {
        return Err(bad("a thin wall needs at least one curve"));
    }
    let chains = chains(&segs)?;
    let (mut bands, mut boundary) = (Vec::new(), Vec::new());
    // Each vertex's segment ends: (vertex, end point, direction away from it, thickness to the
    // left of that direction, to its right).
    let mut ends: Vec<(usize, Vec2, Vec2, f64, f64)> = Vec::new();
    for chain in &chains {
        for (k, s) in chain.segs.iter().enumerate() {
            let (start_v, end_v) = chain.vertices.get(k).copied().ok_or_else(|| bad("wall chain"))?;
            for side in [inside, -outside] {
                boundary.extend(offset_seg(s, side)?);
            }
            for (p, n) in [(s.start(), s.start_tangent().perp()), (s.end(), s.end_tangent().perp())] {
                boundary.push(Seg2::Line { a: p + n * inside, b: p - n * outside });
            }
            ends.push((start_v, s.start(), s.start_tangent(), inside, outside));
            ends.push((end_v, s.end(), -s.end_tangent(), outside, inside));
            bands.push((*s, inside, outside));
        }
    }
    // Close the outer side of every corner with a mitre (or a bevel where the mitre would run
    // far out): between two neighbouring segment ends around a vertex whose walls leave a gap
    // of more than half a turn.
    let mut fills: Vec<Loop2> = Vec::new();
    ends.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.angle().total_cmp(&b.2.angle())));
    for group in ends.chunk_by(|a, b| a.0 == b.0) {
        if group.len() < 2 {
            continue;
        }
        for (i, a) in group.iter().enumerate() {
            let Some(b) = group.get((i + 1) % group.len()) else { continue };
            let gap = (b.2.angle() - a.2.angle()).rem_euclid(TAU);
            if gap <= PI + 1e-7 {
                continue;
            }
            let (vertex, ui, uj) = (a.1, a.2, b.2);
            let (ti, tj) = (a.3, b.4);
            if ti.max(tj) <= 0.0 {
                continue;
            }
            let pi = vertex + ui.perp() * ti;
            let pj = b.1 - uj.perp() * tj;
            let den = ui.cross(uj);
            let mitre = (den.abs() > 1e-9)
                .then(|| {
                    let s = (pj - pi).cross(uj) / den;
                    let r = (pj - pi).cross(ui) / den;
                    let m = pi + ui * s;
                    (s <= 1e-12 && r <= 1e-12 && m.dist(vertex) <= MITRE_LIMIT * ti.max(tj)).then_some(m)
                })
                .flatten();
            let pts = match mitre {
                Some(m) => vec![vertex, pi, m, pj],
                None => vec![vertex, pi, pj],
            };
            let fill = Loop2::polygon(&pts);
            if fill.signed_area().abs() < 1e-10 {
                continue;
            }
            for w in pts.windows(2).skip(1) {
                boundary.push(Seg2::Line { a: w[0], b: w[1] });
            }
            fills.push(fill);
        }
    }
    let inside_wall = |q: Vec2| bands.iter().any(|(s, l, r)| band_contains(s, *l, *r, q)) || fills.iter().any(|f| f.contains(q));
    // Lay the outlines out as a sketch, read its regions, and keep the ones in the wall.
    let sketch = outline_sketch(&boundary)?;
    let kept: Vec<Region2> = find_profiles(&sketch).into_iter().map(|p| p.region).filter(|r| inside_wall(r.interior_point())).collect();
    if kept.is_empty() {
        return Err(bad("the wall has no area"));
    }
    Ok(merge_regions(&kept))
}

fn seg_is_finite(s: &Seg2) -> bool {
    let radius_ok = match *s {
        Seg2::Arc { radius, start, sweep, .. } => radius.is_finite() && start.is_finite() && sweep.is_finite(),
        _ => true,
    };
    radius_ok && s.start().is_finite() && s.end().is_finite() && s.mid().is_finite()
}

/// A run of segments end to end, oriented as the wall's sides are measured.
struct Chain {
    segs: Vec<Seg2>,
    /// The vertex at each segment's start and end.
    vertices: Vec<(usize, usize)>,
}

/// The segments walked into chains: maximal runs through vertices where exactly two segments
/// meet, each oriented as the module docs say.
fn chains(segs: &[(usize, Seg2)]) -> Result<Vec<Chain>> {
    let mut verts: Vec<Vec2> = Vec::new();
    let mut vertex = |p: Vec2| match verts.iter().position(|v| v.dist(p) <= MERGE_TOL) {
        Some(i) => i,
        None => {
            verts.push(p);
            verts.len() - 1
        }
    };
    let ends: Vec<(usize, usize)> = segs.iter().map(|(_, s)| (vertex(s.start()), vertex(s.end()))).collect();
    // Each vertex's segment ends: (segment, whether it's the segment's start).
    let mut at: Vec<Vec<(usize, bool)>> = vec![Vec::new(); verts.len()];
    for (e, (a, b)) in ends.iter().enumerate() {
        if let Some(v) = at.get_mut(*a) {
            v.push((e, true));
        }
        if let Some(v) = at.get_mut(*b) {
            v.push((e, false));
        }
    }
    let degree = |v: usize| at.get(v).map_or(0, Vec::len);
    let mut used = vec![false; segs.len()];
    // Walk from segment `e` entered at its start (or its end), through vertices of degree 2.
    let walk = |mut e: usize, mut from_start: bool, used: &mut Vec<bool>| -> Vec<(usize, bool)> {
        let mut run = Vec::new();
        for _ in 0..segs.len() {
            if used.get(e).copied().unwrap_or(true) {
                break;
            }
            if let Some(u) = used.get_mut(e) {
                *u = true;
            }
            run.push((e, !from_start));
            let Some(&(a, b)) = ends.get(e) else { break };
            let far = if from_start { b } else { a };
            if degree(far) != 2 {
                break;
            }
            let next = at.get(far).and_then(|v| v.iter().find(|(e2, s2)| !(*e2 == e && *s2 != from_start)).copied());
            match next {
                Some((e2, s2)) => {
                    e = e2;
                    from_start = s2;
                }
                None => break,
            }
        }
        run
    };
    let mut runs: Vec<(Vec<(usize, bool)>, bool)> = Vec::new();
    for v in 0..verts.len() {
        if degree(v) == 2 {
            continue;
        }
        for &(e, s) in at.get(v).map(Vec::as_slice).unwrap_or_default() {
            if !used.get(e).copied().unwrap_or(true) {
                runs.push((walk(e, s, &mut used), false));
            }
        }
    }
    for e in 0..segs.len() {
        if !used.get(e).copied().unwrap_or(true) {
            runs.push((walk(e, true, &mut used), true));
        }
    }
    let mut out = Vec::with_capacity(runs.len());
    for (mut run, closed) in runs {
        let oriented = |run: &[(usize, bool)]| -> Vec<Seg2> {
            run.iter().filter_map(|(e, rev)| segs.get(*e).map(|(_, s)| if *rev { s.reversed() } else { *s })).collect()
        };
        let path = oriented(&run);
        let (Some(first), Some(last)) = (path.first(), path.last()) else { continue };
        let length: f64 = path.iter().map(Seg2::length).sum();
        let mut area: f64 = path.iter().map(Seg2::area_term).sum();
        if !closed {
            area += 0.5 * last.end().cross(first.start());
        }
        let backwards = if area.abs() > 1e-9 * length * length {
            area < 0.0
        } else {
            // Straight: the way the first listed curve of the chain was drawn.
            run.iter().min_by_key(|(e, _)| segs.get(*e).map_or(usize::MAX, |(k, _)| *k)).is_some_and(|(_, rev)| *rev)
        };
        if backwards {
            run.reverse();
            for item in &mut run {
                item.1 = !item.1;
            }
        }
        let vertices = run
            .iter()
            .map(|(e, rev)| ends.get(*e).map(|&(a, b)| if *rev { (b, a) } else { (a, b) }))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| bad("wall chain"))?;
        out.push(Chain { segs: oriented(&run), vertices });
    }
    Ok(out)
}

/// Whether `q` lies in the band `left` thick to the left of `s` and `right` thick to its right,
/// between the lines square to `s` at its ends.
fn band_contains(s: &Seg2, left: f64, right: f64, q: Vec2) -> bool {
    let within = |d: f64| d >= -right - 1e-9 && d <= left + 1e-9;
    match *s {
        Seg2::Line { a, b } => {
            let d = b - a;
            let (t, n) = ((q - a).dot(d) / d.len2(), d.len());
            t > 1e-12 && t < 1.0 - 1e-12 && within((q - a).dot(d.perp()) / n)
        }
        Seg2::Arc { center, radius, start, sweep } => {
            let angle = (q - center).angle();
            let along = if sweep > 0.0 { (angle - start).rem_euclid(TAU) } else { (start - angle).rem_euclid(TAU) };
            let rho = q.dist(center);
            along > 1e-12 && along < sweep.abs() - 1e-12 && within(if sweep > 0.0 { radius - rho } else { rho - radius })
        }
        _ => {
            let t = s.closest_param(q);
            if t <= 1e-9 || t >= 1.0 - 1e-9 {
                return false;
            }
            s.derivative(t).normalized().is_some_and(|tangent| within((q - s.point_at(t)).dot(tangent.perp())))
        }
    }
}

/// `s` moved `d` to its left (to its right when `d` is negative): exact for lines and arcs,
/// cubics within [`FIT_TOL`] for free-form segments.
fn offset_seg(s: &Seg2, d: f64) -> Result<Vec<Seg2>> {
    match *s {
        Seg2::Line { a, b } => {
            let n = (b - a).normalized().ok_or_else(|| bad("a wall curve has no length"))?.perp();
            Ok(vec![Seg2::Line { a: a + n * d, b: b + n * d }])
        }
        Seg2::Arc { center, radius, start, sweep } => {
            // The centre is on the left of a counter-clockwise arc.
            let r = if sweep > 0.0 { radius - d } else { radius + d };
            if r <= 1e-6 {
                return Err(bad(format!("the wall is thicker than the radius of an arc it follows ({radius:.3} mm)")));
            }
            Ok(vec![Seg2::Arc { center, radius: r, start, sweep }])
        }
        _ => offset_freeform(s, d),
    }
}

/// A free-form segment's offset as Hermite cubics through exact offset points and tangents,
/// halved until each is within [`FIT_TOL`] of the exact offset.
fn offset_freeform(s: &Seg2, d: f64) -> Result<Vec<Seg2>> {
    if d == 0.0 && matches!(s, Seg2::Cubic { .. }) {
        return Ok(vec![*s]);
    }
    // The exact offset and its derivative at `t`. Along a curve of signed curvature κ the
    // offset's derivative is the curve's scaled by 1 − dκ; it turns back where that reaches 0.
    let at = |t: f64| -> Result<(Vec2, Vec2)> {
        let d1 = s.derivative(t);
        let speed = d1.len();
        let tangent = d1.normalized().ok_or_else(|| bad("a wall curve has a point without a direction"))?;
        let h = 1e-6;
        let d2 = (s.derivative((t + h).min(1.0)) - s.derivative((t - h).max(0.0))) / ((t + h).min(1.0) - (t - h).max(0.0));
        let kappa = d1.cross(d2) / (speed * speed * speed);
        let scale = 1.0 - d * kappa;
        if !(scale > 0.05) || !kappa.is_finite() {
            return Err(bad("the wall is thicker than the radius of a curve it follows"));
        }
        let p = s.point_at(t) + tangent.perp() * d;
        if !p.is_finite() {
            return Err(bad("a wall offset is not finite"));
        }
        Ok((p, d1 * scale))
    };
    for k in 0..=64 {
        at(k as f64 / 64.0)?;
    }
    let mut out = Vec::new();
    let mut stack = vec![(0.0f64, 1.0f64)];
    while let Some((t0, t1)) = stack.pop() {
        let ((q0, v0), (q1, v1)) = (at(t0)?, at(t1)?);
        let h = (t1 - t0) / 3.0;
        let cubic = Seg2::Cubic { p0: q0, p1: q0 + v0 * h, p2: q1 - v1 * h, p3: q1 };
        let mut error = 0.0f64;
        for k in 1..8 {
            let u = k as f64 / 8.0;
            error = error.max(cubic.point_at(u).dist(at(t0 + (t1 - t0) * u)?.0));
        }
        if error <= FIT_TOL || out.len() + stack.len() >= MAX_FIT_PIECES {
            if error > FIT_TOL * 10.0 {
                return Err(bad("could not follow a free-form wall curve closely enough"));
            }
            out.push((t0, cubic));
        } else {
            let mid = 0.5 * (t0 + t1);
            stack.push((mid, t1));
            stack.push((t0, mid));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out.into_iter().map(|(_, c)| c).collect())
}

/// The outline segments as the curves of a sketch (duplicate lines once).
fn outline_sketch(boundary: &[Seg2]) -> Result<Sketch> {
    let mut sk = Sketch::new();
    let mut lines: Vec<(Vec2, Vec2)> = Vec::new();
    for s in boundary {
        match *s {
            Seg2::Line { a, b } => {
                if a.dist(b) <= 1e-9 {
                    continue;
                }
                let same = |(p, q): &(Vec2, Vec2)| (p.dist(a) <= 1e-9 && q.dist(b) <= 1e-9) || (p.dist(b) <= 1e-9 && q.dist(a) <= 1e-9);
                if lines.iter().any(same) {
                    continue;
                }
                lines.push((a, b));
                sk.add_line(a, b, None, None, None)?;
            }
            Seg2::Arc { center, sweep, .. } => {
                let (p0, p1) = if sweep > 0.0 { (s.start(), s.end()) } else { (s.end(), s.start()) };
                sk.add_arc(center, p0, p1, [None; 3], None)?;
            }
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let pts = [p0, p1, p2, p3].iter().map(|p| sk.add_point(*p, None)).collect::<Result<Vec<_>>>()?;
                sk.add_curve(CurveKind::Spline { pts, control: true, degree: 3, periodic: false, handles: Vec::new() }, None)?;
            }
            _ => return Err(bad("not supported yet: this kind of wall curve")),
        }
    }
    Ok(sk)
}
