//! Exact projection workflows for spline controls and certified circular arcs.
//!
//! Every geometric computation is a kernel primitive. This layer converts the
//! record data and chooses the sketch's representation and control budget.

use crate::error::status;
use crate::prism::{PlanarPiece, PlanarSpline};
use crate::Result;
use solidcore::curve::conic_projection as k;
use solidcore_convert::{plane, record_vec2, vec2};
use solidcore_record::{Plane, Vec2};

/// A planar projected image, ready for an application's link state.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    /// An already exact native segment.
    Segment(solidcore_record::Seg2),
    /// A point image.
    Point(Vec2),
    /// A complete linear image.
    Line(Vec2, Vec2),
    /// A control spline.
    Spline {
        /// Its exact spline definition.
        spline: PlanarSpline,
        /// The requested source degree.
        degree: usize,
        /// Whether the source omitted uniform knots.
        implicit: bool,
    },
    /// A rational quadratic conic.
    Conic {
        /// Its first point.
        a: Vec2,
        /// Its middle control point.
        apex: Vec2,
        /// Its last point.
        b: Vec2,
        /// Its conic weight ratio.
        rho: f64,
    },
    /// A full ellipse.
    Ellipse {
        /// Its centre.
        center: Vec2,
        /// A point on its major axis.
        major: Vec2,
        /// Its minor radius.
        minor: f64,
    },
}

pub(crate) fn record_projection(p: k::Projection) -> Projection {
    match p {
        k::Projection::Segment(s) => Projection::Segment(solidcore_convert::record_seg2(&s)),
        k::Projection::Point(p) => Projection::Point(record_vec2(p)),
        k::Projection::Line(a, b) => Projection::Line(record_vec2(a), record_vec2(b)),
        k::Projection::Spline {
            spline,
            degree,
            implicit,
        } => Projection::Spline {
            spline: PlanarSpline {
                degree: spline.degree,
                knots: spline.knots,
                points: spline.points.into_iter().map(record_vec2).collect(),
                weights: spline.weights,
            },
            degree,
            implicit,
        },
        k::Projection::Conic { a, apex, b, rho } => Projection::Conic {
            a: record_vec2(a),
            apex: record_vec2(apex),
            b: record_vec2(b),
            rho,
        },
        k::Projection::Ellipse {
            center,
            major,
            minor,
        } => Projection::Ellipse {
            center: record_vec2(center),
            major: record_vec2(major),
            minor,
        },
    }
}

fn kernel_projection(p: Projection) -> k::Projection {
    match p {
        Projection::Segment(s) => k::Projection::Segment(solidcore_convert::seg2(&s)),
        Projection::Point(p) => k::Projection::Point(vec2(p)),
        Projection::Line(a, b) => k::Projection::Line(vec2(a), vec2(b)),
        Projection::Spline {
            spline,
            degree,
            implicit,
        } => k::Projection::Spline {
            spline: solidcore::prism::read::PlanarSpline {
                degree: spline.degree,
                knots: spline.knots,
                points: spline.points.into_iter().map(vec2).collect(),
                weights: spline.weights,
            },
            degree,
            implicit,
        },
        Projection::Conic { a, apex, b, rho } => k::Projection::Conic {
            a: vec2(a),
            apex: vec2(apex),
            b: vec2(b),
            rho,
        },
        Projection::Ellipse {
            center,
            major,
            minor,
        } => k::Projection::Ellipse {
            center: vec2(center),
            major: vec2(major),
            minor,
        },
    }
}

/// The relative orientation of two sketch planes.
#[derive(Clone, Copy, Debug)]
pub struct PlaneRelation {
    /// Their normals are parallel.
    pub parallel: bool,
    /// Their projected plane has only roundoff width.
    pub edge_on: bool,
    /// Their projection reverses orientation.
    pub flip: bool,
}

/// Ask the kernel for the relationship between two planes.
pub fn plane_relation(source: &Plane, target: &Plane) -> PlaneRelation {
    let r = k::plane_relation(&plane(source), &plane(target));
    PlaneRelation {
        parallel: r.parallel,
        edge_on: r.edge_on,
        flip: r.flip,
    }
}

/// Project points between two plane frames.
pub fn project_points(source: &Plane, target: &Plane, points: &[Vec2]) -> Vec<Vec2> {
    k::project_points(
        &plane(source),
        &plane(target),
        &points.iter().copied().map(vec2).collect::<Vec<_>>(),
    )
    .into_iter()
    .map(record_vec2)
    .collect()
}

/// Project a control spline, retaining the source's uniform or explicit representation.
pub fn project_control(
    source: &Plane,
    target: &Plane,
    points: &[Vec2],
    degree: usize,
    metadata: Option<(&[f64], &[f64])>,
    collapse: bool,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    k::project_control_between(
        &plane(source),
        &plane(target),
        &points.iter().copied().map(vec2).collect::<Vec<_>>(),
        degree,
        metadata,
        collapse,
        max_coord,
    )
    .map(|g| g.into_iter().map(record_projection).collect())
    .map_err(status)
}

/// Keep the complete image of one linear source once.
fn coalesce_linear(geometry: Vec<Projection>) -> Vec<Projection> {
    k::coalesce_linear(geometry.into_iter().map(kernel_projection).collect())
        .into_iter()
        .map(record_projection)
        .collect()
}

/// Fit the source spline exactly, project its controls, and retain its complete image.
pub fn project_fit(
    source: &Plane,
    target: &Plane,
    points: &[Vec2],
    periodic: bool,
    limit: usize,
    collapse: bool,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let source_frame = plane(source);
    let target_frame = plane(target);
    let points = points.iter().copied().map(vec2).collect::<Vec<_>>();
    let scale = k::projection_scale(&source_frame, &target_frame, &points);
    let mut out = Vec::new();
    for s in
        solidcore::curve::fit_projection::fit_splines(&points, periodic, limit).map_err(status)?
    {
        let points = k::project_points(&source_frame, &target_frame, &s.points);
        out.extend(
            k::classify_control(
                points,
                s.degree,
                Some((&s.knots, &s.weights)),
                scale,
                collapse,
                max_coord,
            )
            .map_err(status)?
            .into_iter()
            .map(record_projection),
        );
    }
    Ok(coalesce_linear(out))
}

/// Classify already projected native pieces, using the kernel's exact spline splitting.
pub fn classify_pieces(
    target: &Plane,
    pieces: &[PlanarPiece],
    limit: usize,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let pieces: Vec<_> = pieces
        .iter()
        .map(|p| match p {
            PlanarPiece::Seg(s) => {
                solidcore::prism::read::PlanarPiece::Seg(solidcore_convert::seg2(s))
            }
            PlanarPiece::Spline(s) => {
                solidcore::prism::read::PlanarPiece::Spline(solidcore::prism::read::PlanarSpline {
                    degree: s.degree,
                    knots: s.knots.clone(),
                    points: s.points.iter().copied().map(vec2).collect(),
                    weights: s.weights.clone(),
                })
            }
        })
        .collect();
    k::classify_pieces(&plane(target), &pieces, limit, max_coord)
        .map(|g| g.into_iter().map(record_projection).collect())
        .map_err(status)
}
