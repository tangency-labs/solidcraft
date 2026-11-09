//! Coordinates and planar geometry for mesh reconstruction.
//!
//! Deviations: no upstream counterpart. No recursion.

use crate::eval::Frame;
use crate::math::{Vec2, Vec3};
use crate::prism::profile::{cross2, Loop2, Region2, Seg2};
use crate::Result;

use super::invalid;

/// The scale-independent normalization used by the reconstruction's normal and chord tests.
const MIN_DIRECTION: f64 = 1e-9;

pub(super) fn normalize(v: Vec3) -> Option<Vec3> {
    let length = v.length();
    (length > MIN_DIRECTION && length.is_finite()).then(|| v / length)
}

/// A planar frame with the original, deterministic choice of horizontal axis.
pub(super) struct MeshPlane {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
}

impl MeshPlane {
    pub(super) fn from_normal(origin: Vec3, normal: Vec3) -> Option<Self> {
        let normal = normalize(normal)?;
        let candidate = if normal.z.abs() > 0.9 {
            Vec3::X
        } else {
            Vec3::Z.cross(normal)
        };
        let x = normalize(candidate - normal * candidate.dot(normal))?;
        let y = normal.cross(x);
        let x = normalize(x)?;
        let normal = normalize(x.cross(y))?;
        let y = normalize(normal.cross(x))?;
        origin
            .to_array()
            .iter()
            .all(|value| value.is_finite())
            .then_some(Self { origin, x, y })
    }

    pub(super) fn normal(&self) -> Vec3 {
        self.x.cross(self.y)
    }

    pub(super) fn to_local(&self, point: Vec3) -> Vec2 {
        let offset = point - self.origin;
        Vec2::new(offset.dot(self.x), offset.dot(self.y))
    }

    pub(super) fn frame(&self) -> Frame {
        Frame {
            origin: self.origin.to_array(),
            x_axis: self.x.to_array(),
            y_axis: self.y.to_array(),
        }
    }
}

/// Planar lengths use hypot to retain the fitter's residual and sagitta comparisons.
pub(super) trait PlanarVector {
    fn len(self) -> f64;
    fn len2(self) -> f64;
    fn dist(self, other: Vec2) -> f64;
    fn cross(self, other: Vec2) -> f64;
    fn normalized(self) -> Option<Vec2>;
    fn divided_by(self, divisor: f64) -> Vec2;
}

impl PlanarVector for Vec2 {
    fn len(self) -> f64 {
        self.x.hypot(self.y)
    }

    fn len2(self) -> f64 {
        self.length_squared()
    }

    fn dist(self, other: Vec2) -> f64 {
        (self - other).len()
    }

    fn cross(self, other: Vec2) -> f64 {
        cross2(self, other)
    }

    fn normalized(self) -> Option<Vec2> {
        let length = self.len();
        (length > MIN_DIRECTION && length.is_finite()).then(|| self.divided_by(length))
    }

    fn divided_by(self, divisor: f64) -> Vec2 {
        Vec2::new(self.x / divisor, self.y / divisor)
    }
}

/// Predecessor, vertex and successor for every vertex, beginning at index zero.
pub(super) fn corners(points: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2, Vec2)> + '_ {
    points
        .iter()
        .cycle()
        .skip(points.len().saturating_sub(1))
        .zip(points)
        .zip(points.iter().cycle().skip(1))
        .map(|((&a, &b), &c)| (a, b, c))
}

/// Start at the sharpest join so a cyclic seam does not interrupt a circular run.
pub(super) fn ring_at_sharpest_corner(points: &[Vec2]) -> Vec<Vec2> {
    let offset = corners(points)
        .enumerate()
        .map(|(i, (a, b, c))| {
            let first = b - a;
            let second = c - b;
            (i, first.cross(second).atan2(first.dot(second)).abs())
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(i, _)| i);
    points
        .iter()
        .cycle()
        .skip(offset)
        .take(points.len() + 1)
        .copied()
        .collect()
}

pub(super) fn polygon(points: &[Vec2]) -> Loop2 {
    let segs = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| Seg2::Line { a, b })
        .collect();
    Loop2 { segs }
}

pub(super) fn circle(center: Vec2, radius: f64) -> Loop2 {
    Loop2 {
        segs: vec![Seg2::Arc {
            center,
            radius,
            start: 0.,
            sweep: std::f64::consts::TAU,
        }],
    }
}

pub(super) fn distance_to_segment(segment: &Seg2, point: Vec2) -> f64 {
    let param = match *segment {
        Seg2::Line { a, b } => {
            let direction = b - a;
            if a == b {
                0.
            } else {
                ((point - a).dot(direction) / direction.len2()).clamp(0., 1.)
            }
        }
        Seg2::Arc {
            center,
            start,
            sweep,
            ..
        } => {
            let offset = point - center;
            let angle = offset.y.atan2(offset.x);
            let along = ((angle - start) * sweep.signum()).rem_euclid(std::f64::consts::TAU);
            if along <= sweep.abs() {
                along / sweep.abs()
            } else if segment.start().dist(point) <= segment.end().dist(point) {
                0.
            } else {
                1.
            }
        }
        _ => return f64::INFINITY,
    };
    segment.point_at(param).dist(point)
}

pub(super) fn segment_length(segment: &Seg2) -> f64 {
    match *segment {
        Seg2::Line { a, b } => a.dist(b),
        Seg2::Arc { radius, sweep, .. } => radius * sweep.abs(),
        _ => f64::INFINITY,
    }
}

pub(super) fn ccw(lp: &Loop2) -> Result<Loop2> {
    Ok(if lp.signed_area()?.value >= 0. {
        lp.clone()
    } else {
        lp.reversed()
    })
}

fn contains(lp: &Loop2, probe: Vec2) -> Result<bool> {
    lp.holds(probe)
        .ok_or_else(|| invalid("boundary loops touch or have uncertain containment"))
}

pub(super) fn contains_loop(outer: &Loop2, inner: &Loop2) -> Result<bool> {
    let probe = inner
        .segs
        .first()
        .ok_or_else(|| invalid("empty mesh boundary loop"))?
        .start();
    contains(outer, probe)
}

/// Use even–odd containment on the input loops and put each hole in its smallest containing outline.
pub(super) fn nest_regions(raw: &[Loop2], fitted: &[Loop2]) -> Result<Option<Vec<Region2>>> {
    let mut regions = Vec::new();
    let mut holes = Vec::new();
    for (i, (input, outline)) in raw.iter().zip(fitted).enumerate() {
        let Some(probe) = input.segs.first().map(Seg2::start) else {
            return Ok(None);
        };
        let mut owners = 0;
        for (j, other) in raw.iter().enumerate() {
            if j != i && contains(other, probe)? {
                owners += 1;
            }
        }
        if owners % 2 == 0 {
            regions.push(Region2 {
                outer: ccw(outline)?,
                holes: Vec::new(),
            });
        } else {
            holes.push((probe, ccw(outline)?.reversed()));
        }
    }
    for (probe, hole) in holes {
        let mut owner = None;
        let mut best = f64::INFINITY;
        for (i, region) in regions.iter().enumerate() {
            if contains(&region.outer, probe)? {
                let area = region.outer.signed_area()?.value.abs();
                if area < best {
                    best = area;
                    owner = Some(i);
                }
            }
        }
        let Some(region) = owner.and_then(|i| regions.get_mut(i)) else {
            return Ok(None);
        };
        region.holes.push(hole);
    }
    Ok(Some(regions))
}
