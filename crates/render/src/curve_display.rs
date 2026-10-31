//! Pixel-bounded display paths. CAD segments remain untouched; positive homogeneous
//! control hulls bound perspective projection, including looping and rational curves.
use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};

use crate::Mat4;

const MAX_VERTICES: usize = 4096;
const MAX_DEPTH: u8 = 24;
const PIXEL_ERROR: f64 = 0.5;
/// The vertices a new part has room for before it grows.
const PART_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelCurveView {
    pub vp: Mat4,
    pub width_px: f64,
    pub height_px: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplingQuality {
    Complete,
    BudgetLimited,
    Invalid,
}

/// Shared per-frame allocation and traversal limits. Each segment also has a local cap.
#[derive(Clone, Debug)]
pub struct SamplingBudget {
    pub remaining_vertices: usize,
    pub remaining_nodes: usize,
}

impl SamplingBudget {
    pub fn new(max_vertices: usize, max_nodes: usize) -> Self {
        Self { remaining_vertices: max_vertices.min(1_000_000), remaining_nodes: max_nodes.min(2_000_000) }
    }
}

impl Default for SamplingBudget {
    fn default() -> Self {
        Self::new(200_000, 400_000)
    }
}

#[derive(Clone, Debug)]
pub struct SampledPath {
    pub parts: Vec<Vec<Vec3>>,
    pub quality: SamplingQuality,
    pub vertex_count: usize,
}

type H = [f64; 4];

fn mix(a: H, b: H, t: f64) -> H {
    std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t)
}

/// The most control points of one span: a Bézier of the highest degree.
const MAX_CONTROLS: usize = solidcraft_geom::MAX_BEZIER_DEGREE + 1;

/// One span's homogeneous control points, kept inline: the sampler splits and projects
/// thousands of them per curve and view, and a `Vec` each was most of its allocations.
#[derive(Clone, Copy)]
struct Hull {
    len: usize,
    p: [H; MAX_CONTROLS],
}

impl Hull {
    fn new(points: &[H]) -> Option<Hull> {
        let mut p = [[0.0; 4]; MAX_CONTROLS];
        p.get_mut(..points.len())?.copy_from_slice(points);
        Some(Hull { len: points.len(), p })
    }

    fn points(&self) -> &[H] {
        &self.p[..self.len]
    }

    /// The two halves at t = 0.5 (de Casteljau), each level's midpoints worked out in place
    /// with the same `mix` as one level at a time.
    fn halves(&self) -> (Hull, Hull) {
        let n = self.len;
        let mut work = self.p;
        let (mut l, mut r) = (Hull { len: n, p: [[0.0; 4]; MAX_CONTROLS] }, Hull { len: n, p: [[0.0; 4]; MAX_CONTROLS] });
        for level in 0..n {
            let last = n - 1 - level;
            l.p[level] = work[0];
            r.p[last] = work[last];
            for i in 0..last {
                work[i] = mix(work[i], work[i + 1], 0.5);
            }
        }
        (l, r)
    }
}

fn world(h: H) -> Option<Vec3> {
    let p = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
    (h[3] > 0.0 && [p.x, p.y, p.z].iter().all(|x| x.is_finite())).then_some(p)
}

fn project_h(m: Mat4, h: H) -> H {
    std::array::from_fn(|r| (0..4).map(|c| m.0[c][r] * h[c]).sum())
}

// Wgpu clip volume (depth 0..w), plus positive w to avoid bridging the eye plane.
fn distances(p: H, view: &PixelCurveView, clip_xy: bool) -> [f64; 7] {
    // A one-pixel guard band retains the approximating chord of every visible
    // curve point, even when that chord lies just outside the viewport.
    let x = if clip_xy { p[3] * (1.0 + 2.0 / view.width_px) } else { f64::INFINITY };
    let y = if clip_xy { p[3] * (1.0 + 2.0 / view.height_px) } else { f64::INFINITY };
    [x + p[0], x - p[0], y + p[1], y - p[1], p[2], p[3] - p[2], p[3] - 1e-12]
}

fn screen(p: H, view: &PixelCurveView) -> Vec2 {
    Vec2::new(p[0] / p[3] * view.width_px * 0.5, p[1] / p[3] * view.height_px * 0.5)
}

fn distance_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.dot(d).max(1e-300)).clamp(0.0, 1.0);
    p.dist(a + d * t)
}

fn controls(segment: &Seg2, plane: &Plane) -> Option<(Vec<Hull>, H)> {
    let mut terms: H = [0.0; 4];
    let mut h = |p: Vec2, w: f64| {
        for (r, (o, x, y)) in [(plane.origin.x, plane.x.x, plane.y.x), (plane.origin.y, plane.x.y, plane.y.y), (plane.origin.z, plane.x.z, plane.y.z)]
            .into_iter()
            .enumerate()
        {
            terms[r] = terms[r].max((o.abs() + x.abs() * p.x.abs() + y.abs() * p.y.abs()) * w.abs());
        }
        terms[3] = terms[3].max(w.abs());
        let p = plane.to_world(p);
        [p.x * w, p.y * w, p.z * w, w]
    };
    let spans = match *segment {
        Seg2::Line { a, b } => vec![Hull::new(&[h(a, 1.0), h(b, 1.0)])?],
        Seg2::Cubic { p0, p1, p2, p3 } => vec![Hull::new(&[h(p0, 1.0), h(p1, 1.0), h(p2, 1.0), h(p3, 1.0)])?],
        Seg2::Conic { a, apex, b, w } if w > 0.0 => vec![Hull::new(&[h(a, 1.0), h(apex, w), h(b, 1.0)])?],
        Seg2::Bezier { n, p } if (1..=solidcraft_geom::MAX_BEZIER_DEGREE as u8).contains(&n) => {
            let mut hull = Hull { len: n as usize + 1, p: [[0.0; 4]; MAX_CONTROLS] };
            for (out, p) in hull.p.iter_mut().zip(&p[..=n as usize]) {
                *out = h(*p, 1.0);
            }
            vec![hull]
        }
        Seg2::Arc { center, radius, start, sweep }
            if radius > 0.0 && start.is_finite() && sweep.is_finite() && sweep.abs() <= std::f64::consts::TAU + 1e-9 =>
        {
            let count = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            let mut spans = Vec::with_capacity(count);
            for i in 0..count {
                let a = start + sweep * i as f64 / count as f64;
                let b = start + sweep * (i + 1) as f64 / count as f64;
                let mid = (a + b) * 0.5;
                let w = ((b - a) * 0.5).cos();
                let p = |t: f64, r: f64| center + Vec2::new(t.cos(), t.sin()) * r;
                spans.push(Hull::new(&[h(p(a, radius), 1.0), h(p(mid, radius / w), w), h(p(b, radius), 1.0)])?);
            }
            spans
        }
        _ => return None,
    };
    spans.iter().flat_map(Hull::points).all(|p| p.iter().all(|x| x.is_finite()) && p[3] > 0.0).then_some((spans, terms))
}

/// Samples one exact segment to a conservative 0.5 physical-pixel control-hull
/// bound. Clipped gaps remain separate parts. Budget-limited paths may omit unresolved
/// portions; callers must not treat their quality as an accuracy certificate.
pub fn sample_segment(segment: &Seg2, plane: &Plane, view: &PixelCurveView, budget: &mut SamplingBudget) -> SampledPath {
    sample(segment, plane, view, budget, true)
}

/// Retains offscreen XY boundaries for display-only fill triangulation. Eye/near/far
/// clipping and budgets still apply; only a complete closed path can form a fill.
pub fn sample_segment_unclipped(segment: &Seg2, plane: &Plane, view: &PixelCurveView, budget: &mut SamplingBudget) -> SampledPath {
    sample(segment, plane, view, budget, false)
}

fn sample(segment: &Seg2, plane: &Plane, view: &PixelCurveView, budget: &mut SamplingBudget, clip_xy: bool) -> SampledPath {
    let mut out = SampledPath { parts: Vec::new(), quality: SamplingQuality::Complete, vertex_count: 0 };
    if !view.width_px.is_finite()
        || !view.height_px.is_finite()
        || view.width_px <= 0.0
        || view.height_px <= 0.0
        || view.width_px > 1e7
        || view.height_px > 1e7
        || view.vp.0.iter().flatten().any(|x| !x.is_finite())
    {
        out.quality = SamplingQuality::Invalid;
        return out;
    }
    let Some((spans, source_terms)) = controls(segment, plane) else {
        out.quality = SamplingQuality::Invalid;
        return out;
    };
    // Depth first: the spans not yet started, and one right half per level below.
    let mut stack = Vec::with_capacity(spans.len() + usize::from(MAX_DEPTH) + 1);
    for hull in spans.into_iter().rev() {
        let c = hull.points();
        let magnitude: H = std::array::from_fn(|i| c.iter().map(|p| p[i].abs()).fold(source_terms[i], f64::max));
        let terms: H = std::array::from_fn(|r| (0..4).map(|i| view.vp.0[i][r].abs() * magnitude[i]).sum());
        stack.push((hull, 0u8, terms));
    }
    let mut connected = false;
    let mut nodes = 0usize;
    let mut q: [H; MAX_CONTROLS] = [[0.0; 4]; MAX_CONTROLS];
    while let Some((hull, depth, terms)) = stack.pop() {
        if budget.remaining_nodes == 0 || nodes == 32_768 {
            out.quality = SamplingQuality::BudgetLimited;
            break;
        }
        budget.remaining_nodes -= 1;
        nodes += 1;
        let c = hull.points();
        let n = c.len();
        for (q, h) in q.iter_mut().zip(c) {
            *q = project_h(view.vp, *h);
        }
        let q = &q[..n];
        if q.iter().flatten().any(|x| !x.is_finite()) {
            out.quality = SamplingQuality::Invalid;
            break;
        }
        let e: H = terms.map(|t| t * f64::EPSILON * (f64::from(depth) + 4.0) * 64.0);
        let clip_error = [
            e[0] + e[3] * (1.0 + 2.0 / view.width_px),
            e[0] + e[3] * (1.0 + 2.0 / view.width_px),
            e[1] + e[3] * (1.0 + 2.0 / view.height_px),
            e[1] + e[3] * (1.0 + 2.0 / view.height_px),
            e[2],
            e[2] + e[3],
            e[3],
        ];
        if (0..7).any(|i| q.iter().all(|p| distances(*p, view, clip_xy)[i] < -clip_error[i])) {
            connected = false;
            continue;
        }
        let positive = q.iter().all(|p| p[3] > 1e-12);
        let depth_safe = n == 2 || (4..7).all(|i| q.iter().all(|p| distances(*p, view, clip_xy)[i] >= 0.0));
        let flat = positive
            && q.first().zip(q.last()).is_some_and(|(a, b)| {
                let (a, b) = (screen(*a, view), screen(*b, view));
                let magnitude = q.iter().map(|p| screen(*p, view).len()).fold(1.0, f64::max);
                // Keep floating-point roundoff inside the advertised bound.
                // Absolute summands also cover cancellation in VP multiplication.
                // Subdivision propagates roundoff from the original homogeneous hull.
                let projection_error = q
                    .iter()
                    .map(|p| {
                        let safe_w = p[3] - e[3];
                        if safe_w <= 0.0 {
                            return f64::INFINITY;
                        }
                        let x = (e[0] + (p[0] / p[3]).abs() * e[3]) / safe_w * view.width_px * 0.5;
                        let y = (e[1] + (p[1] / p[3]).abs() * e[3]) / safe_w * view.height_px * 0.5;
                        x.hypot(y)
                    })
                    .fold(0.0, f64::max);
                let allowance = projection_error + magnitude * f64::EPSILON * 128.0;
                allowance < PIXEL_ERROR && q.iter().all(|p| distance_segment(screen(*p, view), a, b) + allowance <= PIXEL_ERROR)
            });
        if !flat || !depth_safe {
            if depth == MAX_DEPTH || out.vertex_count >= MAX_VERTICES {
                out.quality = SamplingQuality::BudgetLimited;
                connected = false;
                continue;
            }
            let (l, r) = hull.halves();
            stack.push((r, depth + 1, terms));
            stack.push((l, depth + 1, terms));
            continue;
        }
        let (Some(a), Some(b), Some(qa), Some(qb)) = (c.first(), c.last(), q.first(), q.last()) else { continue };
        let mut lo: f64 = 0.0;
        let mut hi: f64 = 1.0;
        for (a, b) in distances(*qa, view, clip_xy).into_iter().zip(distances(*qb, view, clip_xy)) {
            if a < 0.0 && b < 0.0 {
                hi = -1.0;
                break;
            }
            if a < 0.0 {
                lo = lo.max(a / (a - b));
            } else if b < 0.0 {
                hi = hi.min(a / (a - b));
            }
        }
        if lo > hi {
            connected = false;
            continue;
        }
        let (Some(a), Some(b)) = (world(mix(*a, *b, lo)), world(mix(*a, *b, hi))) else {
            out.quality = SamplingQuality::Invalid;
            break;
        };
        // Exact consecutive subdivision endpoints share their homogeneous value.
        let join = connected && lo == 0.0 && out.parts.last().and_then(|p| p.last()).is_some_and(|p| *p == a);
        let need = if join { 1 } else { 2 };
        if need > budget.remaining_vertices || out.vertex_count + need > MAX_VERTICES {
            out.quality = SamplingQuality::BudgetLimited;
            break;
        }
        budget.remaining_vertices -= need;
        out.vertex_count += need;
        if join {
            if let Some(part) = out.parts.last_mut() {
                part.push(b);
            }
        } else {
            // Room for a few chords: most visible pieces take several, and growing from two
            // reallocated each part twice.
            let mut part = Vec::with_capacity(PART_CAPACITY);
            part.extend([a, b]);
            out.parts.push(part);
        }
        connected = hi == 1.0;
    }
    out
}

#[cfg(test)]
#[path = "curve_display_tests.rs"]
mod tests;
