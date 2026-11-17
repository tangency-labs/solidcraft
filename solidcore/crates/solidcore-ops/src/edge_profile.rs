//! Router edge profiles: exact section sweeps, stock containment, then subtraction.

use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::edge_section::{corner_section, edge_section_tool};
use solidcore::topology::edge;

use crate::body::{finish, Expect};
use crate::boolean::boolean;
use crate::measure::outer_faceuse;
use crate::session::with_body;
use crate::{Body, BooleanOp, Error, Order, Result};

pub use solidcore::sweep::edge_section::{CornerProfile, CornerSizes};

/// Tangent continuation of record edges, mapped through their import order, with tolerance in radians.
///
/// # Errors
/// Invalid indices or an undecided native tangent continuation.
pub fn tangent_edges(
    body: &Body,
    order: &Order,
    edges: &[usize],
    angle: f64,
) -> Result<Vec<usize>> {
    with_body("router tangent edges", body, |k, b| {
        let seeds = edges
            .iter()
            .map(|&i| crate::order::edge(order, i))
            .collect::<Result<Vec<_>>>();
        let seeds = match seeds {
            Ok(seeds) => seeds,
            Err(e) => return Ok(Err(e)),
        };
        let chain = solidcore::blend::blend_refusal::tangent_chain(k, b, &seeds, angle)?;
        Ok(chain
            .into_iter()
            .map(|e| {
                order
                    .edges
                    .iter()
                    .position(|&id| id == e)
                    .ok_or(Error::Exception)
            })
            .collect())
    })?
}

/// Route the given record edges of one body. The returned lengths are in edge order.
/// Every tool must be wholly inside the original stock; an oversized cut is refused.
///
/// # Errors
/// Invalid edge indices, sizes, support faces or paths; a tool exceeding the stock;
/// the kernel's sweep, boolean or solid-validation refusal.
pub fn edge_profile(
    body: &Body,
    order: &Order,
    edges: &[usize],
    profile: CornerProfile,
    sizes: CornerSizes,
) -> Result<(Body, Vec<f64>)> {
    if edges.is_empty() || edges.len() > 256 {
        return Err(Error::Argument);
    }
    let mut tools = Vec::new();
    let mut lengths = Vec::new();
    for &index in edges {
        let e = crate::order::edge(order, index)?;
        let tool = with_body("router edge section", body, |k, b| {
            let faces = edge::faces(b, e, None)?;
            let (Some(&a), Some(&c)) = (faces.first(), faces.get(1)) else {
                return Ok(Err(Error::Argument));
            };
            if faces.len() != 2 || a == c {
                return Ok(Err(Error::Argument));
            }
            let (fa, fc) = (outer_faceuse(b, a)?, outer_faceuse(b, c)?);
            let (fa, fc) = match (fa, fc) {
                (Ok(a), Ok(c)) => (a, c),
                (Err(e), _) | (_, Err(e)) => return Ok(Err(e)),
            };
            let na = solidcore::query::api_queries::face_normal_at_edge(k, b, e, a, fa)?
                .normal
                .value;
            let nc = solidcore::query::api_queries::face_normal_at_edge(k, b, e, c, fc)?
                .normal
                .value;
            let planar = |f| {
                solidcore::topology::face::surface(b, f)
                    .map(|s| matches!(s.map(Surface::kind), Some(SurfaceKind::Plane(_))))
            };
            let (pa, pc) = (planar(a)?, planar(c)?);
            let uses = if (pa && !pc) || (pa == pc && na.z.abs() >= nc.z.abs()) {
                [fa, fc]
            } else {
                [fc, fa]
            };
            let section = corner_section(profile, sizes)?;
            let tool = edge_section_tool(k, b, e, uses, &section)?;
            Ok(finish(k, tool, Expect::SOLID)?.map(Body::new))
        })??;
        if boolean(tool.copy()?, body.copy()?, BooleanOp::Subtract)?.is_some() {
            return Err(Error::Unsupported(
                "router profile exceeds one of its support faces",
            ));
        }
        lengths.push(body.edge_length(order, index, 1e-10)?);
        tools.push(tool);
    }
    let mut result = body.copy()?;
    for tool in tools {
        result = boolean(result, tool, BooleanOp::Subtract)?.ok_or(Error::Empty)?;
    }
    Ok((result, lengths))
}
