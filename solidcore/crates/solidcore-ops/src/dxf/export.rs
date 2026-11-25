//! Writing DXF: a path's pieces converted for the file, their bounds, the drawing's scale and
//! translation, and the checks a written entity must pass, each through the kernel's
//! `prism::{arc_conversion, bounds, transform}`, with record conversion and budgets only.
//!
//! The kernel keeps the original piece indices, admission rules and precision checks.
//! Arc fitting remains the caller's existing workflow; this module computes no geometry.

use solidcore::prism::arc_conversion as kernel;
use solidcore::prism::profile::Loop2 as KernelLoop;
use solidcore_convert as convert;
use solidcore_record::{Seg2, Vec2};

mod bounds;
pub use bounds::{
    axis_nonzero, bulged_bounds, ellipse_bounds, point_bounds, scaled_axis, supporting_circle,
    transform_anchor, transformed_point, union_bounds, Bounds,
};
pub use kernel::{
    arc_fields_finite, distinct_degrees, finite_degrees, finite_span, PathRefusal, PathShape,
    PathStyle, ScalarEncoding, Vertex, MAX_PATH_ITEMS,
};
pub use solidcore::prism::transform::scaled_positive;
/// Export geometry expressed in application record coordinates.
pub type Shape = PathShape<Vec2>;
/// One record-space vertex and its departing bulge.
pub type PathVertex = Vertex<Vec2>;
type Result<T> = std::result::Result<T, PathRefusal>;

fn checked_style(segs: &[Seg2], style: PathStyle) -> Result<()> {
    if let PathStyle::Arcs { tolerance_mm } = style {
        if !(tolerance_mm.is_finite() && tolerance_mm >= 1e-9) {
            return Err(PathRefusal {
                piece: None,
                reason: "arc tolerance must be finite and at least 1e-9 mm".into(),
            });
        }
    }
    if segs.len() > MAX_PATH_ITEMS {
        return Err(PathRefusal {
            piece: None,
            reason: "input piece budget exceeded".into(),
        });
    }
    Ok(())
}

/// Check source geometry and budgets before allocating the kernel input records.
pub fn admit_path(segs: &[Seg2], style: PathStyle) -> Result<()> {
    checked_style(segs, style)?;
    for (index, seg) in segs.iter().enumerate() {
        admit_piece(seg, index, segs.len())?;
    }
    Ok(())
}

/// Admit a single piece while keeping its original index in any refusal.
pub fn admit_piece(seg: &Seg2, index: usize, total: usize) -> Result<()> {
    kernel::admit_piece(&convert::seg2(seg), index, total)
}

fn input(segs: &[Seg2], style: PathStyle) -> Result<KernelLoop> {
    admit_path(segs, style)?;
    let mut result = KernelLoop::default();
    for seg in segs {
        result.segs.try_reserve(1).map_err(|_| PathRefusal {
            piece: None,
            reason: "cannot allocate kernel input pieces".into(),
        })?;
        result.segs.push(convert::seg2(seg));
    }
    Ok(result)
}

/// Recognize a whole circle or ellipse without converting separate pieces or fitting.
pub fn recognized_path(
    segs: &[Seg2],
    closed: bool,
    style: PathStyle,
    encoding: ScalarEncoding,
) -> Result<Option<Shape>> {
    if !closed {
        return Ok(None);
    }
    let lp = input(segs, style)?;
    kernel::recognized_path(kernel::PlanarPath::Closed(&lp), style, encoding)
        .map(|v| v.map(record_shape))
}

/// Convert admitted exact or polyline geometry; the caller handles free-form arc fitting.
pub fn path_shapes(
    segs: &[Seg2],
    closed: bool,
    style: PathStyle,
    encoding: ScalarEncoding,
) -> Result<Vec<Shape>> {
    let lp = input(segs, style)?;
    let path = if closed {
        kernel::PlanarPath::Closed(&lp)
    } else {
        kernel::PlanarPath::Open(&lp.segs)
    };
    kernel::path_shapes(path, style, encoding).map(|v| v.into_iter().map(record_shape).collect())
}

/// Convert one source or fitted piece with the original source index.
pub fn exact_piece(
    seg: &Seg2,
    index: usize,
    total: usize,
    encoding: ScalarEncoding,
) -> Result<Shape> {
    kernel::exact_piece(&convert::seg2(seg), index, total, encoding).map(record_shape)
}

/// Reconstruct a bulged edge for format validation without changing its stored values.
pub fn bulge_arc(from: PathVertex, to: Vec2) -> Option<Seg2> {
    kernel::bulge_arc(
        Vertex {
            at: convert::vec2(from.at),
            bulge: from.bulge,
        },
        convert::vec2(to),
    )
    .as_ref()
    .map(convert::record_seg2)
}

/// Endpoints and directed cardinal extrema, at most six record points.
pub fn bulged_edge_points(from: PathVertex, to: Vec2) -> Result<Vec<Vec2>> {
    kernel::bulged_edge_points(
        Vertex {
            at: convert::vec2(from.at),
            bulge: from.bulge,
        },
        convert::vec2(to),
    )
    .map(|v| v.into_iter().map(convert::record_vec2).collect())
}

fn record_shape(shape: PathShape<solidcore::math::Vec2>) -> Shape {
    use kernel::PathShape as S;
    let p = convert::record_vec2;
    match shape {
        S::Line(a, b) => S::Line(p(a), p(b)),
        S::Circle { c, r } => S::Circle { c: p(c), r },
        S::Arc { c, r, a0, a1 } => S::Arc { c: p(c), r, a0, a1 },
        S::Ellipse {
            c,
            major,
            ratio,
            t0,
            t1,
        } => S::Ellipse {
            c: p(c),
            major: p(major),
            ratio,
            t0,
            t1,
        },
        S::Spline {
            degree,
            knots,
            ctrl,
            weights,
        } => S::Spline {
            degree,
            knots,
            ctrl: ctrl.into_iter().map(p).collect(),
            weights,
        },
        S::BulgedPolyline { vertices, closed } => S::BulgedPolyline {
            vertices: vertices
                .into_iter()
                .map(|v| Vertex {
                    at: p(v.at),
                    bulge: v.bulge,
                })
                .collect(),
            closed,
        },
    }
}
