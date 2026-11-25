//! Bounds and placement of written entities, through the kernel's `prism::{bounds, transform}`.
use super::{PathRefusal, PathVertex, MAX_PATH_ITEMS};
use solidcore::prism::bounds as kernel;
use solidcore::prism::{arc_conversion, transform};
use solidcore_convert as convert;
use solidcore_record::Vec2;
type Result<T> = std::result::Result<T, PathRefusal>;
/// Conservative bounds in record coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Minimum coordinates.
    pub min: Vec2,
    /// Maximum coordinates.
    pub max: Vec2,
}
fn record(b: kernel::Bounds) -> Bounds {
    Bounds {
        min: convert::record_vec2(b.min),
        max: convert::record_vec2(b.max),
    }
}
/// Unite two already validated bounds.
pub fn union_bounds(amin: Vec2, amax: Vec2, bmin: Vec2, bmax: Vec2) -> Bounds {
    record(
        kernel::Bounds {
            min: convert::vec2(amin),
            max: convert::vec2(amax),
        }
        .union(kernel::Bounds {
            min: convert::vec2(bmin),
            max: convert::vec2(bmax),
        }),
    )
}
/// Enclose a complete circular support with outward rounding.
pub fn supporting_circle(c: Vec2, r: f64) -> Result<Bounds> {
    kernel::supporting_circle(convert::vec2(c), r).map(record)
}
/// Enclose a complete ellipse with outward axis bounds.
pub fn ellipse_bounds(c: Vec2, major: Vec2, ratio: f64, t0: f64, t1: f64) -> Result<Bounds> {
    kernel::ellipse_bounds(convert::vec2(c), convert::vec2(major), ratio, t0, t1).map(record)
}
/// Enclose stored points after bounded conversion.
pub fn point_bounds(points: &[Vec2]) -> Result<Bounds> {
    if points
        .len()
        .checked_mul(size_of::<Vec2>())
        .is_none_or(|n| n > 64 << 20)
    {
        return Err(PathRefusal {
            piece: None,
            reason: "cumulative geometry payload exceeds byte limit".into(),
        });
    }
    let mut input = Vec::new();
    input
        .try_reserve_exact(points.len())
        .map_err(|_| PathRefusal {
            piece: None,
            reason: "point array allocation failed".into(),
        })?;
    input.extend(points.iter().copied().map(convert::vec2));
    kernel::point_bounds(&input).map(record)
}
/// Positive uniform scale and translation, with the kernel's FMA semantics.
pub fn transformed_point(p: Vec2, scale: f64, translation: Vec2) -> Result<Vec2> {
    transform::transformed_point(convert::vec2(p), scale, convert::vec2(translation))
        .map(convert::record_vec2)
}
/// Scale an axis without translating it.
pub fn scaled_axis(axis: Vec2, scale: f64) -> Result<Vec2> {
    transform::scaled_axis(convert::vec2(axis), scale).map(convert::record_vec2)
}
/// Check for a finite nonzero axis length.
pub fn axis_nonzero(axis: Vec2) -> bool {
    arc_conversion::axis_nonzero(convert::vec2(axis))
}
/// Bound a bulged path. Length/work admission precedes conversion and payload access.
pub fn bulged_bounds(
    vertices: impl IntoIterator<Item = PathVertex>,
    count: usize,
    closed: bool,
    max_vertices: usize,
    max_work: usize,
) -> Result<Bounds> {
    let fail = |s: &str| PathRefusal {
        piece: None,
        reason: format!("bulged polyline bounds: {s}"),
    };
    if !(2..=max_vertices.min(MAX_PATH_ITEMS)).contains(&count) {
        return Err(fail(
            "vertex count exceeds admission or needs at least two vertices",
        ));
    }
    let edges = if closed { count } else { count - 1 };
    let work = edges
        .checked_mul(512)
        .and_then(|n| n.checked_add(count))
        .ok_or_else(|| fail("work count overflow"))?;
    if work > max_work.min(64 << 20) {
        return Err(fail("work budget exceeded before vertex scan"));
    }
    let mut input = Vec::new();
    input
        .try_reserve_exact(count)
        .map_err(|_| fail("vertex allocation failed"))?;
    for v in vertices {
        if input.len() >= count {
            return Err(fail("vertex count differs from declared input"));
        }
        input.push(arc_conversion::Vertex {
            at: convert::vec2(v.at),
            bulge: v.bulge,
        });
    }
    if input.len() != count {
        return Err(fail("vertex count differs from declared input"));
    }
    kernel::bulged_bounds(&input, closed, max_vertices, max_work).map(record)
}

/// Transform a retained source anchor with the kernel's original FMA order.
pub fn transform_anchor(point: Vec2, scale: f64, translation: Vec2) -> Vec2 {
    convert::record_vec2(transform::transform_anchor(
        convert::vec2(point),
        scale,
        convert::vec2(translation),
    ))
}
