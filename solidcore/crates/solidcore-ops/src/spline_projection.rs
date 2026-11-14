//! Exact native-curve projection and spline control-budget workflows.
//!
//! Geometry is computed only in the kernel; records cross through solidcore-convert.

use solidcore::curve::spline_projection as kernel;
use solidcore_convert as convert;
use solidcore_record::brep::Curve;
use solidcore_record::{Plane, Vec3};

use crate::{Error, Result};

pub use crate::prism::{PlanarPiece, PlanarSpline};

fn kernel_spline(s: &PlanarSpline) -> solidcore::prism::read::PlanarSpline {
    solidcore::prism::read::PlanarSpline {
        degree: s.degree,
        knots: s.knots.clone(),
        points: s.points.iter().copied().map(convert::vec2).collect(),
        weights: s.weights.clone(),
    }
}

fn record_spline(s: solidcore::prism::read::PlanarSpline) -> PlanarSpline {
    PlanarSpline {
        degree: s.degree,
        knots: s.knots,
        points: s.points.into_iter().map(convert::record_vec2).collect(),
        weights: s.weights,
    }
}

/// Split one clamped spline at existing knots to meet the client's control budget.
///
/// # Errors
/// Refuses malformed data, nonpositive weights and a budget smaller than one span.
pub fn split_spline(s: &PlanarSpline, limit: usize) -> Result<Vec<PlanarSpline>> {
    kernel::split_planar(&kernel_spline(s), limit)
        .map(|pieces| pieces.into_iter().map(record_spline).collect())
        .map_err(crate::error::status)
}

/// Project a native interval in its selected direction, preserving its exact form.
/// `None` means the geometry could not be certified inside the work budget.
pub fn project_curve(
    curve: &Curve,
    interval: [f64; 2],
    forward: bool,
    ends: [Vec3; 2],
    plane: &Plane,
    tol: f64,
    work: &mut usize,
) -> Option<Vec<PlanarPiece>> {
    let curve = convert::curve(curve).ok()?;
    kernel::project_curve(
        &curve,
        interval,
        forward,
        ends.map(convert::vec3),
        &convert::plane(plane),
        tol,
        work,
    )
    .map(|pieces| {
        pieces
            .into_iter()
            .map(|p| match p {
                solidcore::prism::read::PlanarPiece::Seg(s) => {
                    PlanarPiece::Seg(convert::record_seg2(&s))
                }
                solidcore::prism::read::PlanarPiece::Spline(s) => {
                    PlanarPiece::Spline(record_spline(s))
                }
            })
            .collect()
    })
}

/// Project the requested native record edges without building or changing their body.
/// Indices or curves that cannot be read exactly have `None` as their result.
pub fn project_edges(
    body: &solidcore_record::brep::Brep,
    plane: &Plane,
    edges: &[usize],
    tol: f64,
) -> Vec<Option<Vec<PlanarPiece>>> {
    edges
        .iter()
        .map(|i| {
            let edge = body.edges.get(*i)?;
            let at = |i: u32| {
                body.vertices
                    .get(i as usize)
                    .map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))
            };
            let ends = [at(edge.vertices[0])?, at(edge.vertices[1])?];
            project_curve(
                edge.parameters(),
                edge.interval,
                true,
                ends,
                plane,
                tol,
                &mut 262_144,
            )
        })
        .collect()
}

/// Project a circular native edge after its source plane and arc have been certified.
///
/// # Errors
/// The kernel's exact projection refusals.
pub fn circle_edge(
    body: &solidcore_record::brep::Brep,
    plane: &Plane,
    index: usize,
    tol: f64,
    max_coord: f64,
) -> Result<Option<Vec<crate::conic_projection::Projection>>> {
    let Some(edge) = body.edges.get(index) else {
        return Ok(None);
    };
    let at = |i: u32| {
        body.vertices
            .get(i as usize)
            .map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))
    };
    let (Some(a), Some(b)) = (at(edge.vertices[0]), at(edge.vertices[1])) else {
        return Ok(None);
    };
    let curve = convert::curve(edge.parameters()).map_err(|e| Error::Record(e.to_string()))?;
    kernel::circle_projection(
        &curve,
        edge.interval,
        [convert::vec3(a), convert::vec3(b)],
        &convert::plane(plane),
        tol,
        max_coord,
    )
    .map(|projections| {
        projections.map(|ps| {
            ps.into_iter()
                .map(crate::conic_projection::record_projection)
                .collect()
        })
    })
    .map_err(crate::error::status)
}

/// Read and classify one native edge's exact projection for a sketch workflow.
/// Free-form pieces are offered the source-circle certificate before classification.
///
/// # Errors
/// Refuses an unrepresentable projection or a spline beyond the client's degree or control budget.
pub fn projected_edge(
    body: &solidcore_record::brep::Brep,
    plane: &Plane,
    index: usize,
    tol: f64,
    limit: usize,
    max_coord: f64,
) -> Result<Option<Vec<crate::conic_projection::Projection>>> {
    if let Some(edge) = body
        .edges
        .get(index)
        .filter(|e| matches!(e.curve, Curve::Ellipse { .. }))
    {
        let definition = convert::curve(&edge.curve).map_err(|e| Error::Record(e.to_string()))?;
        let form = edge
            .nurbs
            .as_ref()
            .map(convert::curve)
            .transpose()
            .map_err(|e| Error::Record(e.to_string()))?;
        if let Some(projections) = kernel::full_ellipse_projection(
            &definition,
            form.as_ref(),
            edge.interval,
            &convert::plane(plane),
            max_coord,
        )
        .map_err(crate::error::status)?
        {
            return Ok(Some(
                projections
                    .into_iter()
                    .map(crate::conic_projection::record_projection)
                    .collect(),
            ));
        }
    }
    let Some(pieces) = project_edges(body, plane, &[index], tol)
        .into_iter()
        .next()
        .flatten()
    else {
        return Ok(None);
    };
    let free_form = pieces.iter().any(|p| {
        matches!(
            p,
            PlanarPiece::Spline(_)
                | PlanarPiece::Seg(
                    solidcore_record::Seg2::Cubic { .. } | solidcore_record::Seg2::Bezier { .. }
                )
        )
    });
    if free_form {
        if let Some(projections) = circle_edge(body, plane, index, tol, max_coord)? {
            return Ok(Some(projections));
        }
    }
    crate::conic_projection::classify_pieces(plane, &pieces, limit, max_coord).map(Some)
}
