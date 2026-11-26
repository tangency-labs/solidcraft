//! Exact native-curve projection and spline control-budget workflows.
//!
//! Geometry is computed only in the kernel; records cross through solidcore-convert.

use solidcore::curve::spline_projection as kernel;
use solidcore_convert as convert;
use solidcore_record::brep::{Curve, Surface};
use solidcore_record::{Plane, Vec3};

use crate::{Error, Result};

pub use crate::prism::{PlanarPiece, PlanarSpline};

/// The certificate work an edge's exact projection may take before it is not read.
const EDGE_WORK: usize = 262_144;

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

fn record_pieces(pieces: Vec<solidcore::prism::read::PlanarPiece>) -> Vec<PlanarPiece> {
    pieces
        .into_iter()
        .map(|p| match p {
            solidcore::prism::read::PlanarPiece::Seg(s) => {
                PlanarPiece::Seg(convert::record_seg2(&s))
            }
            solidcore::prism::read::PlanarPiece::Spline(s) => PlanarPiece::Spline(record_spline(s)),
        })
        .collect()
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
    .map(record_pieces)
}

/// A boundary of an analytic cylinder, projected along its axis. The kernel
/// certifies the complete native interval against that support's exact circle.
/// Conflicting supporting circles are ambiguous and do not select an image.
pub fn cylinder_edge_projection(
    body: &solidcore_record::brep::Brep,
    plane: &Plane,
    index: usize,
    tol: f64,
) -> Option<Vec<PlanarPiece>> {
    let edge = body.edges.get(index)?;
    // An analytic curve keeps the general path's own reading.
    if matches!(
        &edge.curve,
        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } | Curve::Polyline { .. }
    ) {
        return None;
    }
    if !edge.uses.iter().any(|usage| {
        body.faces
            .get(usage.face as usize)
            .is_some_and(|face| matches!(face.surface, Surface::Cylinder { .. }))
    }) {
        return None;
    }
    let at = |i: u32| {
        body.vertices
            .get(i as usize)
            .map(|v| convert::vec3(Vec3::from(v.point)))
    };
    let ends = [at(edge.vertices[0])?, at(edge.vertices[1])?];
    let curve = convert::curve(edge.parameters()).ok()?;
    let target = convert::plane(plane);
    let mut found = None;
    for usage in &edge.uses {
        let Some(Surface::Cylinder { frame, radius }) =
            body.faces.get(usage.face as usize).map(|f| &f.surface)
        else {
            continue;
        };
        let Some(pieces) = kernel::cylinder_projection(
            &curve,
            edge.interval,
            ends,
            (&convert::frame(frame), *radius),
            &target,
            tol,
        ) else {
            continue;
        };
        if found.as_ref().is_some_and(|previous| previous != &pieces) {
            return None;
        }
        found = Some(pieces);
    }
    found.map(record_pieces)
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
            let mut work = EDGE_WORK;
            project_curve(
                edge.parameters(),
                edge.interval,
                true,
                ends,
                plane,
                tol,
                &mut work,
            )
        })
        .collect()
}

/// The part of `curve` over the record's parameter `interval`, whose end points are `ends`,
/// projected on `plane` exactly ([`project_curve`] within an edge's work budget) as sketch
/// geometry ([`crate::conic_projection::classify_pieces`] within `limit` and `max_coord`).
/// `None` when the projection is not certified within the budget.
///
/// # Errors
/// Those of [`crate::conic_projection::classify_pieces`].
pub fn projected_part(
    curve: &Curve,
    interval: [f64; 2],
    ends: [Vec3; 2],
    plane: &Plane,
    tol: f64,
    limit: usize,
    max_coord: f64,
) -> Result<Option<Vec<crate::conic_projection::Projection>>> {
    let mut work = EDGE_WORK;
    let Some(pieces) = project_curve(curve, interval, true, ends, plane, tol, &mut work) else {
        return Ok(None);
    };
    crate::conic_projection::classify_pieces(plane, &pieces, limit, max_coord).map(Some)
}

/// Whether an edge can be read in a plane parallel to the target, before cylinder recognition.
/// Invalid indices or an exhausted certificate budget are not readable.
pub fn has_parallel_projection(
    body: &solidcore_record::brep::Brep,
    plane: &Plane,
    index: usize,
    tol: f64,
) -> bool {
    let Some(edge) = body.edges.get(index) else {
        return false;
    };
    let at = |i: u32| {
        body.vertices
            .get(i as usize)
            .map(|v| solidcore::Vec3::new(v.point[0], v.point[1], v.point[2]))
    };
    let (Some(a), Some(b)) = (at(edge.vertices[0]), at(edge.vertices[1])) else {
        return false;
    };
    let Ok(curve) = convert::curve(edge.parameters()) else {
        return false;
    };
    let mut work = EDGE_WORK;
    kernel::has_parallel_projection(
        &curve,
        edge.interval,
        [a, b],
        &convert::plane(plane),
        tol,
        &mut work,
    )
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
