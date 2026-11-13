//! Geometry queries for picking a Form in a projected viewport.

use super::vec::VecExt;
use crate::Vec3;

/// Column-major homogeneous projection and its screen rectangle.
#[derive(Clone, Copy)]
pub struct Projection {
    /// World-to-clip transform.
    pub matrix: [[f64; 4]; 4],
    /// Left and top of the screen rectangle, in display points.
    pub origin: [f32; 2],
    /// Width and height of the screen rectangle, in display points.
    pub size: [f32; 2],
}

impl Projection {
    fn clip(self, point: Vec3) -> [f64; 4] {
        self.matrix
            .into_iter()
            .zip([point.x, point.y, point.z, 1.0])
            .fold([0.0; 4], |mut out, (col, v)| {
                for (result, value) in out.iter_mut().zip(col) {
                    *result += value * v;
                }
                out
            })
    }

    fn screen(self, point: Vec3) -> Option<[f32; 2]> {
        self.clip_to_screen(self.clip(point))
    }

    fn clip_to_screen(self, [x, y, _, w]: [f64; 4]) -> Option<[f32; 2]> {
        if !w.is_finite() || w <= 1e-9 {
            return None;
        }
        let [left, top] = self.origin;
        let [width, height] = self.size.map(f64::from);
        let point = [
            left + ((x / w + 1.0) * 0.5 * width) as f32,
            top + ((1.0 - y / w) * 0.5 * height) as f32,
        ];
        point.iter().all(|x| x.is_finite()).then_some(point)
    }

    fn near(self, bounds: Bounds, [x, y]: [f32; 2], radius: f32) -> bool {
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for a in [bounds.min.x, bounds.max.x] {
            for b in [bounds.min.y, bounds.max.y] {
                for c in [bounds.min.z, bounds.max.z] {
                    let Some([u, v]) = self.screen(Vec3::new(a, b, c)) else {
                        // A box crossing the eye plane cannot be pruned safely.
                        return true;
                    };
                    let [left, top] = min;
                    let [right, bottom] = max;
                    min = [left.min(u), top.min(v)];
                    max = [right.max(u), bottom.max(v)];
                }
            }
        }
        let [left, top] = min;
        let [right, bottom] = max;
        left - radius <= x && x <= right + radius && top - radius <= y && y <= bottom + radius
    }
}

/// A curve's world-space bounding box.
#[derive(Clone, Copy)]
pub struct Bounds {
    /// Lower coordinate corner.
    pub min: Vec3,
    /// Upper coordinate corner.
    pub max: Vec3,
}

impl Bounds {
    fn coordinates(point: Vec3) -> [f64; 3] {
        [point.x, point.y, point.z]
    }

    fn valid(self) -> bool {
        self.min.is_finite()
            && self.max.is_finite()
            && Self::coordinates(self.min)
                .into_iter()
                .zip(Self::coordinates(self.max))
                .all(|(a, b)| a <= b)
    }

    fn union(self, other: Self) -> Self {
        let a = self.min;
        let b = self.max;
        let c = other.min;
        let d = other.max;
        Self {
            min: Vec3::new(a.x.min(c.x), a.y.min(c.y), a.z.min(c.z)),
            max: Vec3::new(b.x.max(d.x), b.y.max(d.y), b.z.max(d.z)),
        }
    }

    fn center(self, axis: usize) -> f64 {
        Self::coordinates(self.min)
            .get(axis)
            .zip(Self::coordinates(self.max).get(axis))
            .map_or(0.0, |(a, b)| a * 0.5 + b * 0.5)
    }
}

struct Node {
    bounds: Bounds,
    children: Option<Box<[Self; 2]>>,
    leaves: Vec<(usize, Bounds)>,
}

impl Node {
    fn build(entries: &mut [(usize, Bounds)]) -> Option<Self> {
        let bounds = entries
            .iter()
            .map(|(_, bounds)| *bounds)
            .reduce(Bounds::union)?;
        if entries.len() <= 8 {
            return Some(Self {
                bounds,
                children: None,
                leaves: entries.to_vec(),
            });
        }
        let sizes = Bounds::coordinates(bounds.max - bounds.min);
        let axis = sizes
            .iter()
            .enumerate()
            .rev()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))?
            .0;
        entries.sort_unstable_by(|(a, a_box), (b, b_box)| {
            a_box
                .center(axis)
                .total_cmp(&b_box.center(axis))
                .then(a.cmp(b))
        });
        let middle = entries.len() / 2;
        let (a, b) = entries.split_at_mut(middle);
        Some(Self {
            bounds,
            children: Some(Box::new([Self::build(a)?, Self::build(b)?])),
            leaves: Vec::new(),
        })
    }

    fn candidates(
        &self,
        projection: Projection,
        pixel: [f32; 2],
        radius: f32,
        limit: usize,
        output: &mut Vec<usize>,
    ) -> bool {
        if !projection.near(self.bounds, pixel, radius) {
            return true;
        }
        if let Some(children) = &self.children {
            return children
                .iter()
                .all(|child| child.candidates(projection, pixel, radius, limit, output));
        }
        for (id, bounds) in &self.leaves {
            if projection.near(*bounds, pixel, radius) {
                if output.len() == limit {
                    return false;
                }
                output.push(*id);
            }
        }
        true
    }
}

/// An index of conservative world-space curve bounds.
pub struct BoundsIndex(Option<Node>);

/// Maximum number of curves in a projected pick index.
pub const MAX_CURVES: usize = 65_536;

impl BoundsIndex {
    /// Build an index from checked curve bounds in caller order.
    pub fn new(bounds: Vec<Bounds>) -> Option<Self> {
        if bounds.len() > MAX_CURVES || bounds.iter().any(|b| !b.valid()) {
            return None;
        }
        let mut entries: Vec<_> = bounds.into_iter().enumerate().collect();
        Some(Self(Node::build(&mut entries)))
    }

    /// All bounds near the pointer, or `None` when the candidate budget is exceeded.
    pub fn candidates(
        &self,
        projection: Projection,
        pixel: [f32; 2],
        radius: f32,
        limit: usize,
    ) -> Option<Vec<usize>> {
        let mut output = Vec::new();
        self.0
            .as_ref()
            .is_none_or(|node| node.candidates(projection, pixel, radius, limit, &mut output))
            .then_some(output)
    }
}

/// A projected candidate evaluated at the requested source fraction.
#[derive(Clone, Copy)]
pub struct CurveHit {
    /// Screen distance squared from the pointer.
    pub distance2: f32,
    /// Fraction along the original curve.
    pub fraction: f64,
    /// Exact source point at that fraction.
    pub point: Vec3,
}

/// Nearest projected point of a visible-to-the-camera segment, with its world witness.
pub fn segment_hit(
    projection: Projection,
    pixel: [f32; 2],
    [start, end]: [Vec3; 2],
) -> Option<CurveHit> {
    let start_clip = projection.clip(start);
    let end_clip = projection.clip(end);
    let [x, y] = projection.clip_to_screen(start_clip)?;
    let [end_x, end_y] = projection.clip_to_screen(end_clip)?;
    let [dx, dy] = [end_x - x, end_y - y];
    let size2 = dx * dx + dy * dy;
    let t = if size2 > 1e-6 {
        (((pixel[0] - x) * dx + (pixel[1] - y) * dy) / size2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let [ox, oy] = [x + dx * t - pixel[0], y + dy * t - pixel[1]];
    let distance2 = ox * ox + oy * oy;
    let t = f64::from(t);
    // Interpolating in screen space mixes clip points weighted by inverse w.
    let end_weight = t * start_clip[3];
    let fraction = end_weight / ((1.0 - t) * end_clip[3] + end_weight);
    let point = start * (1.0 - fraction) + end * fraction;
    (distance2.is_finite() && point.is_finite()).then_some(CurveHit {
        distance2,
        fraction,
        point,
    })
}

/// Find projected local minima of the supplied curve, sorted by distance.
pub fn curve_hits(
    projection: Projection,
    [x, y]: [f32; 2],
    radius: f32,
    point: impl Fn(f64) -> Option<Vec3>,
) -> Vec<CurveHit> {
    const SAMPLES: usize = 64;
    let evaluate = |fraction| {
        let point = point(fraction)?;
        let [u, v] = projection.screen(point)?;
        let (du, dv) = (u - x, v - y);
        let distance2 = du * du + dv * dv;
        distance2.is_finite().then_some(CurveHit {
            distance2,
            fraction,
            point,
        })
    };
    let sampled: Vec<_> = (0..=SAMPLES)
        .map(|i| evaluate(i as f64 / SAMPLES as f64))
        .collect();
    let mut hits = Vec::new();
    for (i, value) in sampled.iter().copied().enumerate() {
        let Some(value) = value else { continue };
        if i == 0 || i == SAMPLES {
            hits.push(value);
        }
        let distance = value.distance2;
        // Only adjacent sample distances are needed to qualify this search window.
        let before = i
            .checked_sub(1)
            .and_then(|j| sampled.get(j))
            .and_then(Option::as_ref);
        let after = sampled.get(i + 1).and_then(Option::as_ref);
        if before.is_some_and(|other| other.distance2 <= distance)
            || after.is_some_and(|other| other.distance2 < distance)
        {
            continue;
        }
        hits.push(value);
        let mut low = i.saturating_sub(1) as f64 / SAMPLES as f64;
        let mut high = (i + 1).min(SAMPLES) as f64 / SAMPLES as f64;
        for _ in 0..24 {
            let a = low + (high - low) / 3.0;
            let b = high - (high - low) / 3.0;
            let (Some(va), Some(vb)) = (evaluate(a), evaluate(b)) else {
                break;
            };
            if va.distance2 <= vb.distance2 {
                high = b;
            } else {
                low = a;
            }
        }
        hits.extend(evaluate((low + high) * 0.5));
    }
    hits.retain(|value| value.distance2 <= radius * radius);
    hits.sort_by(|a, b| {
        a.distance2
            .total_cmp(&b.distance2)
            .then(a.fraction.total_cmp(&b.fraction))
    });
    hits
}

/// World rays and tolerances for visibility of one logical point.
pub struct Visibility {
    /// Point depth in its screen ray.
    pub depth: f64,
    /// Bias for its own display mesh, including the fit tolerance.
    pub tolerance: f64,
    /// Origin of the ray through the exact world point for context occlusion.
    pub origin: Vec3,
    /// Direction of that exact-point ray.
    pub direction: Vec3,
    /// Point depth along that exact-point ray.
    pub context_depth: f64,
}

/// Keep a separate exact-point context ray so projection rounding cannot hide a touching rim.
pub fn visibility(
    point: Vec3,
    origin: Vec3,
    direction: Vec3,
    nearby: Vec3,
    fit: f64,
    perspective: bool,
) -> Option<Visibility> {
    let delta = point - origin;
    let depth = delta.dot(direction);
    let tolerance = (nearby - origin).len().max(3.0 * fit.max(0.05));
    let (origin, direction, context_depth) = if perspective {
        (origin, delta.normalized()?, delta.len())
    } else {
        (point - direction * depth, direction, depth)
    };
    Some(Visibility {
        depth,
        tolerance,
        origin,
        direction,
        context_depth,
    })
}

/// Whether a hit is strictly before a point, apart from arithmetic roundoff at coincidence.
pub fn occludes(origin: Vec3, depth: f64, hit: f64) -> bool {
    let scale = origin
        .x
        .abs()
        .max(origin.y.abs())
        .max(origin.z.abs())
        .max(depth.abs())
        .max(hit.abs())
        .max(1.0);
    hit < depth - 64.0 * f64::EPSILON * scale
}

/// Whether the point remains visible with the bias allowed for its own preview.
pub fn own_surface_visible(depth: f64, tolerance: f64, hit: Option<f64>) -> bool {
    hit.is_none_or(|hit| hit >= depth - tolerance)
}

/// Squared distance used to break ties between projected source points.
pub fn distance2(point: Vec3, eye: Vec3) -> f64 {
    (point - eye).len2()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORTHO: Projection = Projection {
        matrix: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        origin: [10.0, 20.0],
        size: [800.0, 600.0],
    };

    #[test]
    fn projected_curve_reports_its_source_fraction() {
        let hits = curve_hits(ORTHO, [315.0, 320.0], 9.0, |t| {
            Some(Vec3::new(t - 0.5, 0.0, 0.0))
        });
        let hit = hits.first().unwrap();
        assert!((hit.fraction - 0.2625).abs() < 1e-6);
        assert!(hit.point.dist(Vec3::new(-0.2375, 0.0, 0.0)) < 1e-6);
        assert!(curve_hits(ORTHO, [315.0, 340.0], 9.0, |t| Some(Vec3::new(
            t - 0.5,
            0.0,
            0.0
        )))
        .is_empty());
    }

    #[test]
    fn projected_segment_witness_uses_clip_weights() {
        let mut perspective = ORTHO;
        perspective.matrix[2][3] = 1.0;
        perspective.matrix[3][3] = 0.0;
        let [start, end] = [Vec3::new(-1.0, 0.0, 2.0), Vec3::new(5.0, 0.0, 10.0)];
        let screen_midpoint = [410.0, 320.0];
        let hit = segment_hit(perspective, screen_midpoint, [start, end]).unwrap();
        assert_eq!(hit.distance2, 0.0);
        assert!((hit.fraction - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(perspective.screen(hit.point), Some(screen_midpoint));
        let reverse = segment_hit(perspective, screen_midpoint, [end, start]).unwrap();
        assert!(reverse.point.dist(hit.point) < 1e-12);
        let orthographic = segment_hit(ORTHO, [1_210.0, 320.0], [start, end]).unwrap();
        assert_eq!(orthographic.fraction, 0.5);
        assert_eq!(orthographic.point, Vec3::new(2.0, 0.0, 6.0));
    }

    #[test]
    fn projected_bounds_refuse_a_truncated_candidate_list() {
        let left = Bounds {
            min: Vec3::new(-0.8, -0.2, 0.0),
            max: Vec3::new(-0.4, 0.2, 0.0),
        };
        let right = Bounds {
            min: Vec3::new(0.4, -0.2, 0.0),
            max: Vec3::new(0.8, 0.2, 0.0),
        };
        let mut boxes = vec![right; 9];
        boxes.extend([left, left]);
        let index = BoundsIndex::new(boxes).unwrap();
        assert_eq!(index.candidates(ORTHO, [170.0, 320.0], 9.0, 1), None);
        assert_eq!(
            index.candidates(ORTHO, [170.0, 320.0], 9.0, 2),
            Some(vec![9, 10])
        );
        assert_eq!(
            index.candidates(ORTHO, [410.0, 320.0], 9.0, 1),
            Some(Vec::new())
        );
    }

    #[test]
    fn exact_context_ray_preserves_a_touching_support() {
        let point = Vec3::new(1.0, 0.0, 10.0);
        let view = visibility(point, Vec3::ZERO, Vec3::Z, Vec3::X * 0.01, 0.05, true).unwrap();
        assert!((view.origin + view.direction * view.context_depth).dist(point) < 1e-12);
        assert!(!occludes(
            view.origin,
            view.context_depth,
            view.context_depth
        ));
        assert!(occludes(
            view.origin,
            view.context_depth,
            view.context_depth - 1e-6
        ));
        assert!(own_surface_visible(view.depth, view.tolerance, Some(9.95)));
    }
}
