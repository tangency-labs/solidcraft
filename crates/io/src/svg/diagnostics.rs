//! Source-addressed checks and conservative, opt-in drawing repair.
use super::{Geom2, IoError, MAX_ENTITIES, Result, Vec2};
use solidcraft_geom::prelude::*;
use std::ops::Range;

/// Geometry and issues found while reading an SVG drawing.
#[derive(Clone, Debug)]
pub struct SvgDrawing {
    pub geometry: Vec<Geom2>,
    pub diagnostics: Vec<SvgDiagnostic>,
}

/// A source location and display overlay for a path diagnostic or explicit approximation.
/// Indices are one-based. Segments count SVG drawing parameter groups (including closure),
/// independent of omitted zero-length lines or arc subdivision; `highlight` is for display and never drives geometry decisions.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SvgDiagnostic {
    pub path: usize,
    pub subpath: usize,
    pub segment: usize,
    pub kind: String,
    pub at: Vec2,
    pub highlight: Vec<Vec2>,
    /// Display-only polylines of retained replacement geometry; empty when unresolved or removed.
    pub replacement_highlight: Vec<Vec<Vec2>>,
    pub repaired: bool,
    pub max_deviation_mm: Option<f64>,
}

fn controls(g: &Geom2) -> Option<Vec<Vec2>> {
    match g {
        Geom2::Line(a, b) => Some(vec![*a, *b]),
        Geom2::Spline { pts, control: true, degree: 3 } if pts.len() == 4 => Some(pts.clone()),
        _ => None,
    }
}

fn cubic(g: &Geom2) -> Option<[Vec2; 4]> {
    if let Geom2::Spline { pts, control: true, degree: 3 } = g { pts.as_slice().try_into().ok() } else { None }
}

// These points are an overlay only. No check or repair reads them back.
fn highlight(g: &Geom2) -> Vec<Vec2> {
    if let Some([a, b, c, d]) = cubic(g) {
        (0..=16)
            .map(|i| {
                let t = f64::from(i) / 16.0;
                let lerp = |p: Vec2, q: Vec2| p * (1.0 - t) + q * t;
                let ab = lerp(a, b);
                let bc = lerp(b, c);
                let cd = lerp(c, d);
                lerp(lerp(ab, bc), lerp(bc, cd))
            })
            .filter(|p| p.is_finite())
            .collect()
    } else {
        controls(g).unwrap_or_default()
    }
}

fn suspect([a, b, c, d]: [Vec2; 4]) -> Option<&'static str> {
    let chord = d - a;
    let length = chord.x.hypot(chord.y);
    if a == d {
        return Some("closed_cubic");
    }
    // The projected derivative is a quadratic Bezier. Check its exact minimum rather than
    // treating a backwards middle control edge as proof that the curve itself runs backwards.
    let projected = [(b - a).dot(chord), (c - b).dot(chord), (d - c).dot(chord)];
    let scale = projected.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
    if scale.is_finite() && scale > 0.0 && projected.iter().all(|q| q.is_finite()) {
        let [q0, q1, q2] = projected.map(|q| q / scale);
        let denominator = q0 - 2.0 * q1 + q2;
        let mut minimum = q0.min(q2);
        if denominator > 0.0 {
            let t = ((q0 - q1) / denominator).clamp(0.0, 1.0);
            minimum = minimum.min((1.0 - t).powi(2) * q0 + 2.0 * t * (1.0 - t) * q1 + t * t * q2);
        }
        if minimum < 0.0 {
            return Some("backtracking_curve");
        }
    }
    // A zero or extremely short endpoint handle can create unbounded curvature at the end.
    // This is a warning about the control polygon, not a claim that the curve is invalid.
    if (b - a).len() < length * 0.01 || (d - c).len() < length * 0.01 {
        return Some("short_tangent");
    }
    None
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let delta = p - a;
    let length2 = d.len2();
    let dot = delta.dot(d);
    if !d.is_finite() || !delta.is_finite() || !length2.is_finite() || !dot.is_finite() {
        return f64::INFINITY;
    }
    let t = if length2 > 0.0 { (dot / length2).clamp(0.0, 1.0) } else { 0.0 };
    let error = p - (a + d * t);
    error.x.hypot(error.y)
}

// Subdivision (at most 24 levels), projection and distance evaluation all round at the
// absolute coordinate scale. Reserve more than their accumulated rounding error; never claim
// a subnormal zero bound when the arithmetic cannot resolve the requested tolerance.
fn rounding_reserve(points: &[Vec2]) -> f64 {
    let magnitude = points.iter().map(|p| p.x.abs().max(p.y.abs())).fold(0.0f64, f64::max);
    (256.0 * f64::EPSILON * magnitude).max(f64::MIN_POSITIVE)
}

fn lerp(a: Vec2, b: Vec2, t: f64) -> Vec2 {
    a * (1.0 - t) + b * t
}

/// The control points of the cubic restricted to `[t0, t1]`.
fn sub_cubic(p: [Vec2; 4], t0: f64, t1: f64) -> [Vec2; 4] {
    fn split(p: [Vec2; 4], t: f64) -> ([Vec2; 4], [Vec2; 4]) {
        let [a, b, c, d] = p;
        let (ab, bc, cd) = (lerp(a, b, t), lerp(b, c, t), lerp(c, d, t));
        let (abc, bcd) = (lerp(ab, bc, t), lerp(bc, cd, t));
        let mid = lerp(abc, bcd, t);
        ([a, ab, abc, mid], [mid, bcd, cd, d])
    }
    let head = if t1 >= 1.0 { p } else { split(p, t1).0 };
    if t0 <= 0.0 || t1 <= 0.0 { head } else { split(head, t0 / t1).1 }
}

fn point_at(p: [Vec2; 4], t: f64) -> Vec2 {
    let [a, b, c, d] = p;
    let (ab, bc, cd) = (lerp(a, b, t), lerp(b, c, t), lerp(c, d, t));
    lerp(lerp(ab, bc, t), lerp(bc, cd, t), t)
}

/// A bound on how far a cubic piece strays from the chord between its ends, or `None` when it
/// cannot be bounded in finite arithmetic. When the control points project inside the chord the
/// curve's offset from the chord line is a cubic Bernstein polynomial vanishing at both ends, so
/// it never exceeds 3/4 of its largest inner control offset; otherwise fall back to the distance
/// of the control hull to the chord.
fn piece_error([a, b, c, d]: [Vec2; 4]) -> Option<f64> {
    let chord = d - a;
    let length2 = chord.len2();
    let hull = distance_to_segment(b, a, d).max(distance_to_segment(c, a, d));
    if !hull.is_finite() {
        return None;
    }
    if length2 > 0.0 && length2.is_finite() {
        let along = |q: Vec2| (q - a).dot(chord) / length2;
        let (sb, sc) = (along(b), along(c));
        if (0.0..=1.0).contains(&sb) && (0.0..=1.0).contains(&sc) {
            let offset = |q: Vec2| {
                let v = q - a;
                (v.x * chord.y - v.y * chord.x).abs() / length2.sqrt()
            };
            let perpendicular = offset(b).max(offset(c));
            if perpendicular.is_finite() {
                return Some((0.75 * perpendicular).min(hull));
            }
        }
    }
    Some(hull)
}

/// Replace the cubic by `n` chords between equally spaced parameters, if every piece stays within
/// `available`. Returns the chords and the largest piece bound.
fn uniform_chords(p: [Vec2; 4], n: usize, available: f64, work_remaining: &mut usize) -> Option<Option<(Vec<Geom2>, f64)>> {
    let mut lines = Vec::with_capacity(n);
    let mut bound = 0.0f64;
    let mut previous = p[0];
    for i in 0..n {
        if *work_remaining == 0 {
            return None;
        }
        *work_remaining -= 1;
        let (t0, t1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
        let end = if i + 1 == n { p[3] } else { point_at(p, t1) };
        let mut piece = if n == 1 { p } else { sub_cubic(p, t0, t1) };
        // The shared vertices are the chord ends; rounding in the split is covered by the reserve.
        piece[0] = previous;
        piece[3] = end;
        let error = piece_error(piece)?;
        if error > available {
            return Some(None);
        }
        bound = bound.max(error);
        if previous != end {
            lines.push(Geom2::Line(previous, end));
        }
        previous = end;
    }
    Some(Some((lines, bound)))
}

/// Each accepted chord has a symmetric Hausdorff bound given by its piece's distance to the
/// segment. The piece lies inside its control hull; its continuous projection covers the chord.
/// The fewest equally spaced chords that meet the bound are used: fewer, longer segments keep
/// later extrusion and region detection cheap.
fn bounded_chords(p: [Vec2; 4], tolerance: f64, work_remaining: &mut usize) -> Option<(Vec<Geom2>, f64)> {
    // A single closed cubic cannot be replaced by an empty chord set. Preserve its topology
    // and leave it unresolved until a closed-curve approximation can be certified separately.
    if p[0] == p[3] {
        return None;
    }
    let reserve = rounding_reserve(&p);
    if tolerance <= reserve {
        return None;
    }
    let available = tolerance - reserve;
    const MAX_CHORDS: usize = 4096;
    // Double until the bound holds, then search the last interval for the smallest count.
    let mut low = 0usize;
    let mut n = 1usize;
    let found = loop {
        match uniform_chords(p, n, available, work_remaining)? {
            Some(found) => break found,
            None if n >= MAX_CHORDS => return None,
            None => {
                low = n;
                n = (n * 2).min(MAX_CHORDS);
            }
        }
    };
    let (mut best, mut high) = (found, n);
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        match uniform_chords(p, mid, available, work_remaining)? {
            Some(found) => {
                best = found;
                high = mid;
            }
            None => low = mid,
        }
    }
    let (lines, bound) = best;
    let bound = bound + reserve;
    (bound <= tolerance && !lines.is_empty()).then_some((lines, bound))
}

/// Closed source boundaries sharing an exactly identical line or cubic can trace other portions of
/// that boundary with different segmentation. Normalize only these interacting source loops:
/// analytic chords let the arrangement consistently split their near-coincident portions.
/// Eligibility uses exact source controls; all approximation uses the certified hull bound.
fn shared_boundaries(geometry: &[Geom2], spans: &[(usize, usize, Range<usize>)]) -> Vec<bool> {
    let mut shared = vec![false; spans.len()];
    let mut seen = std::collections::HashMap::<Vec<u64>, usize>::new();
    for (span, (_, _, range)) in spans.iter().enumerate() {
        let Some(source) = geometry.get(range.clone()) else { continue };
        let first = source.first().and_then(controls).and_then(|p| p.first().copied());
        let last = source.last().and_then(controls).and_then(|p| p.last().copied());
        if first.is_none() || first != last {
            continue;
        }
        for g in source {
            let Some(p) = controls(g) else { continue };
            if p.windows(2).all(|w| w.first() == w.last()) {
                continue;
            }
            // Normalize signed zero so the key follows exact geometric equality.
            let key = |points: &[Vec2]| points.iter().flat_map(|p| [p.x, p.y]).map(|v| if v == 0.0 { 0 } else { v.to_bits() }).collect::<Vec<_>>();
            let forward = key(&p);
            let reversed = key(&p.iter().rev().copied().collect::<Vec<_>>());
            let key = forward.min(reversed);
            if let Some(&other) = seen.get(&key) {
                if other != span {
                    if let Some(value) = shared.get_mut(other) {
                        *value = true;
                    }
                    if let Some(value) = shared.get_mut(span) {
                        *value = true;
                    }
                }
            } else {
                seen.insert(key, span);
            }
        }
    }
    let mut affected = vec![false; geometry.len()];
    for (i, (_, _, range)) in spans.iter().enumerate() {
        if shared.get(i) == Some(&true)
            && let Some(entities) = affected.get_mut(range.clone())
        {
            entities.fill(true);
        }
    }
    affected
}

pub(super) fn process(
    geometry: Vec<Geom2>,
    spans: Vec<(usize, usize, Range<usize>)>,
    source_segments: Vec<usize>,
    unpainted: Vec<Range<usize>>,
    closed: Vec<Range<usize>>,
    tolerance: Option<f64>,
) -> Result<SvgDrawing> {
    let shared = shared_boundaries(&geometry, &spans);
    let mut closed_boundary = vec![false; geometry.len()];
    for range in closed {
        if let Some(entities) = closed_boundary.get_mut(range) {
            entities.fill(true);
        }
    }
    let mut unpainted_boundary = vec![false; geometry.len()];
    for range in unpainted {
        let Some(source) = geometry.get(range.clone()) else { continue };
        let first = source.first().and_then(controls).and_then(|p| p.first().copied());
        let last = source.last().and_then(controls).and_then(|p| p.last().copied());
        if first.is_some()
            && first == last
            && let Some(entities) = unpainted_boundary.get_mut(range)
        {
            entities.fill(true);
        }
    }
    let mut replacements: Vec<Option<Vec<Geom2>>> = vec![None; geometry.len()];
    let mut diagnostics = Vec::new();
    let mut work_remaining = 1_000_000usize;
    let mut final_count = geometry.len();
    for (path, subpath, range) in spans {
        let Some(source) = geometry.get(range.clone()) else { continue };
        // A path which returns to its start and then continues to a second closure has an
        // attached loop. Deleting it is permitted only if its entire control hull is bounded.
        let first = source.first().and_then(controls).and_then(|p| p.first().copied());
        let last = source.last().and_then(controls).and_then(|p| p.last().copied());
        let mut removed_tail = None;
        if let Some(start) = first.filter(|p| Some(*p) == last) {
            let closure = source
                .iter()
                .enumerate()
                .take(source.len().saturating_sub(1))
                .find_map(|(i, g)| controls(g).and_then(|p| p.last().copied()).filter(|p| *p == start).map(|_| i + 1));
            if let Some(index) = closure {
                let tail = source.get(index..).unwrap_or_default();
                let hull: Option<Vec<Vec<Vec2>>> = tail.iter().map(controls).collect();
                if let Some(hull) = hull {
                    let points: Vec<Vec2> = hull.into_iter().flatten().collect();
                    let bound = points
                        .iter()
                        .map(|p| {
                            let d = *p - start;
                            d.x.hypot(d.y)
                        })
                        .fold(0.0f64, f64::max);
                    let reserve = rounding_reserve(&points).max(rounding_reserve(&[start]));
                    let bound = bound + reserve;
                    let repaired = tolerance.is_some_and(|t| t > reserve && bound.is_finite() && bound <= t);
                    diagnostics.push(SvgDiagnostic {
                        path,
                        subpath,
                        segment: source_segments.get(range.start + index).copied().unwrap_or(index + 1),
                        kind: "attached_loop".into(),
                        at: start,
                        highlight: tail.iter().flat_map(highlight).take(256).collect(),
                        replacement_highlight: Vec::new(),
                        repaired,
                        max_deviation_mm: repaired.then_some(bound),
                    });
                    if repaired {
                        for i in range.start + index..range.end {
                            if let Some(replacement) = replacements.get_mut(i) {
                                *replacement = Some(Vec::new());
                                final_count = final_count.saturating_sub(1);
                            }
                        }
                        removed_tail = Some(index);
                    }
                }
            }
        }
        for (index, g) in source.iter().enumerate() {
            if removed_tail.is_some_and(|i| index >= i) {
                continue;
            }
            let Some(p) = cubic(g) else { continue };
            let Some(kind) = suspect(p)
                .or_else(|| (shared.get(range.start + index) == Some(&true)).then_some("shared_boundary"))
                .or_else(|| (unpainted_boundary.get(range.start + index) == Some(&true)).then_some("unpainted_boundary"))
                .or_else(|| (tolerance.is_some() && closed_boundary.get(range.start + index) == Some(&true)).then_some("outline_approximation"))
            else {
                continue;
            };
            let repair = tolerance.and_then(|t| bounded_chords(p, t, &mut work_remaining));
            let replacement_highlight = repair.as_ref().map(|(geometry, _)| geometry.iter().map(highlight).collect()).unwrap_or_default();
            let bound = repair.as_ref().map(|(_, bound)| *bound);
            let repaired = repair.is_some();
            if let Some((lines, _)) = repair {
                final_count = final_count.saturating_sub(1).saturating_add(lines.len());
                if final_count > MAX_ENTITIES {
                    return Err(IoError::Invalid("SVG repair would create too many drawing elements; use a larger tolerance".into()));
                }
                if let Some(replacement) = replacements.get_mut(range.start + index) {
                    *replacement = Some(lines);
                }
            }
            diagnostics.push(SvgDiagnostic {
                path,
                subpath,
                segment: source_segments.get(range.start + index).copied().unwrap_or(index + 1),
                kind: kind.into(),
                at: p[0],
                highlight: highlight(g),
                replacement_highlight,
                repaired,
                max_deviation_mm: bound,
            });
        }
    }
    let mut result = Vec::with_capacity(final_count);
    for (index, g) in geometry.into_iter().enumerate() {
        match replacements.get_mut(index).and_then(Option::take) {
            Some(replacement) => result.extend(replacement),
            None => result.push(g),
        }
    }
    Ok(SvgDrawing { geometry: result, diagnostics })
}
