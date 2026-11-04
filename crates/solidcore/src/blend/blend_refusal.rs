//! A blend refused on its edges or corners, as data ([`BlendRefusal`], carried by
//! [`crate::SolidError::BlendRefused`]), and the queries the refusals rest on: the tangent
//! chain of edges ([`tangent_chain`]), the edge a blended edge runs on into
//! ([`continuation`]) and a round too small to change the part ([`round_too_small`]).
//!
//! Not upstream (solidcore's). The chain follows the blend corner's own test of an edge
//! running on tangentially at a vertex ([`continuing_edges`], the fork's
//! `EndAtSmoothVertex`), and an edge has a blend where it has a blend edge use
//! ([`edge::blend_edgeuse_or_why`]). No recursion.

use crate::blend::blend_corner::end::continuing_edges;
use crate::math::{rad_to_deg, PI};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, vertex, Brep, EdgeId, EdgeuseId, VertexId};
use crate::{EntityRef, Kernel, Result, SolidError};

pub use crate::topology::edge::NotBlendable;

const OP: &str = "blend::blend_refusal";

/// The stations along an edge (fractions of its interval) where [`round_too_small`] takes
/// the faces' turn: [`crate::topology::edgeuse::p2::is_tangent_sector`]'s, the kernel's
/// tangency test of an edge.
const STATIONS: [f64; 5] = [0.34, 0.78, 0.55, 0.12, 0.93];

/// Why a blend is refused on one of its edges or corners. Each carries the entities and
/// sizes an application words it with.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BlendRefusal {
    /// The edge has no blend: it has no blend edge use
    /// ([`edge::blend_edgeuse_or_why`]).
    EdgeNotBlendable {
        /// The edge.
        edge: EdgeId,
        /// Why.
        reason: NotBlendable,
    },
    /// A blend that cannot end at `vertex`, where its edge runs on tangentially into
    /// `continues_into`, an edge of its convexity that is not blended
    /// ([`crate::blend::fillet_executive::NoRoomReason::TangentContinuationEndUnsupported`]):
    /// blending `continues_into` with it ([`tangent_chain`]) runs the blend on.
    ContinuesInto {
        /// The blended edge.
        edge: EdgeId,
        /// Where the blend would end.
        vertex: VertexId,
        /// The edge it runs on into.
        continues_into: EdgeId,
    },
    /// A round too small to change the part ([`round_too_small`]): its faces turn by `turn`
    /// across the edge (radians, the least at the kernel's stations), so a round of the
    /// radius asked lies within `tolerance` of the edge. `smallest_radius` is the least
    /// radius that rounds it, `tolerance / (sec(turn / 2) - 1)`.
    RoundTooSmall {
        /// The edge.
        edge: EdgeId,
        /// The least turn of the faces' normals across it, radians.
        turn: f64,
        /// The edge's tolerance (at least the body's).
        tolerance: f64,
        /// The least radius that rounds it.
        smallest_radius: f64,
    },
    /// A setback asked at `vertex` that stops short of where the rails of the rounds beside
    /// one of its edges cross it: `least` from the vertex
    /// ([`crate::blend::blend_setback::distances`]).
    SetbackTooShort {
        /// The corner's vertex.
        vertex: VertexId,
        /// The least setback: the farthest the rails beside an edge cross it.
        least: f64,
    },
    /// A setback asked at `vertex` that, with its ease, does not fit on the corner's edges
    /// (each edge's length less the far end's claim on it): one below `largest` fits.
    SetbackTooLong {
        /// The corner's vertex.
        vertex: VertexId,
        /// The bound below which a setback fits.
        largest: f64,
    },
}

impl BlendRefusal {
    /// [`crate::SolidError::BlendRefused`] with this refusal, the status `code`.
    #[must_use]
    pub const fn error(self, operation: &'static str, code: i32) -> SolidError {
        SolidError::BlendRefused {
            operation,
            code,
            refusal: self,
        }
    }

    /// The entity refused: the edge, or the corner's vertex.
    #[must_use]
    pub const fn entity(&self) -> EntityRef {
        match *self {
            Self::EdgeNotBlendable { edge, .. }
            | Self::ContinuesInto { edge, .. }
            | Self::RoundTooSmall { edge, .. } => EntityRef::Edge(edge),
            Self::SetbackTooShort { vertex, .. } | Self::SetbackTooLong { vertex, .. } => {
                EntityRef::Vertex(vertex)
            }
        }
    }
}

/// The edge uses round `vertex`, one per manifold edge there (its primary edge use).
fn edgeuses_round(b: &Brep, vertex: VertexId) -> Result<Vec<EdgeuseId>> {
    let mut out = Vec::new();
    for e in vertex::edges(b, vertex, None)? {
        if !edge::is_manifold(b, e)? {
            continue;
        }
        if let Some(eu) = edge::primary_edgeuse(b, e)? {
            out.push(eu);
        }
    }
    Ok(out)
}

/// The edges `e` runs on into, tangentially and of its convexity, at each of its vertices
/// (each with the vertex), within `tangency` radians; none for an edge without a blend
/// edge use. `run_out`: leave out an edge a blend of `e` run on would run out of the part
/// along, as the End does ([`continuing_edges`]); a tangent chain keeps it.
fn runs_on_into(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    tangency: f64,
    run_out: bool,
) -> Result<Vec<(VertexId, EdgeId)>> {
    let mut out = Vec::new();
    let Some(fil_eu) = edge::blend_edgeuse(k, b, e)? else {
        return Ok(out);
    };
    let start = edge::start_vertex(b, e)?;
    let end = edge::end_vertex(b, e)?.filter(|&v| Some(v) != start); // a closed edge's one
    for v in [start, end].into_iter().flatten() {
        let round = edgeuses_round(b, v)?;
        let found = continuing_edges(
            k,
            b,
            v,
            fil_eu,
            &round,
            rad_to_deg(tangency),
            &[],
            false,
            run_out,
        )?;
        out.extend(found.into_iter().map(|f| (v, f)));
    }
    Ok(out)
}

/// The tangent chain of `edges`: `edges` (each once, in order), then every edge with a
/// blend that one of them runs on into tangentially at a vertex, and so on until no more
/// do. An edge runs on into another where the blend corner would end a blend of it
/// there at a smooth vertex ([`continuing_edges`]): the other edge runs on along one of its
/// faces, leaves the vertex opposite to it within `tangency` radians, has the same
/// convexity (a round cannot pass from one side of the faces to the other), and its faces
/// do not meet tangentially at the vertex. Whether a blend run on past the vertex would run
/// out of the part (#2656) is the End's question, not the chain's. An edge without a blend
/// edge use (a smooth edge, a lamina edge) ends the chain.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a tangency not finite or not in `(0, pi / 2)`; those of
/// [`edge::blend_edgeuse`] and [`continuing_edges`].
pub fn tangent_chain(
    k: &Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    tangency: f64,
) -> Result<Vec<EdgeId>> {
    if !(tangency > 0.0 && tangency < 0.5 * PI) {
        return Err(SolidError::invalid_input(
            OP,
            "a tangency angle between 0 and a right angle",
        ));
    }
    let mut chain: Vec<EdgeId> = Vec::new();
    for &e in edges {
        b.edge(e)?;
        if !chain.contains(&e) {
            chain.push(e);
        }
    }
    let mut next = 0;
    while let Some(&e) = chain.get(next) {
        next += 1;
        for (_, f) in runs_on_into(k, b, e, tangency, false)? {
            if !chain.contains(&f) && edge::blend_edgeuse(k, b, f)?.is_some() {
                chain.push(f);
            }
        }
    }
    Ok(chain)
}

/// The first edge not in `edges` that one of them runs on into, as [`tangent_chain`] takes
/// the next edge: the blended edge, the vertex and the edge it runs on into.
///
/// # Errors
/// Those of [`tangent_chain`].
pub fn continuation(
    k: &Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    tangency: f64,
) -> Result<Option<(EdgeId, VertexId, EdgeId)>> {
    for &e in edges {
        for (v, f) in runs_on_into(k, b, e, tangency, true)? {
            if !edges.contains(&f) && edge::blend_edgeuse(k, b, f)?.is_some() {
                return Ok(Some((e, v, f)));
            }
        }
    }
    Ok(None)
}

/// The first edge not in `blended` that `e` runs on into at its vertex `v`, as
/// [`tangent_chain`] takes the next edge.
///
/// # Errors
/// Those of [`tangent_chain`].
pub fn continuation_at(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    v: VertexId,
    tangency: f64,
    blended: &[EdgeId],
) -> Result<Option<EdgeId>> {
    for (w, f) in runs_on_into(k, b, e, tangency, true)? {
        if w == v && !blended.contains(&f) && edge::blend_edgeuse(k, b, f)?.is_some() {
            return Ok(Some(f));
        }
    }
    Ok(None)
}

/// [`BlendRefusal::RoundTooSmall`] for a circular round of `radius` on `e`, or `None` where
/// it changes the part. Across an edge whose faces' normals turn by `turn`, a round of
/// radius r lies `r (sec(turn / 2) - 1)` inside the sharp edge at its middle, the farthest
/// it moves the part; below the edge's tolerance (at least the body's) the round is the edge
/// itself to the model's accuracy. `turn` is the least turn at the kernel's stations along
/// the edge (those of its tangency test): at least the least turn along it, so a round
/// refused is within the tolerance where the faces turn least. An edge whose faces turn by
/// no more than the blend's tangency angle there ([`edge::blend_edgeuse`]'s) is left to the
/// blend, as is an edge without a blend edge use.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a radius not finite and positive, or an edge use without
/// a radial partner; those of [`edge::blend_edgeuse`] and the binormal evaluations.
pub fn round_too_small(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    radius: f64,
) -> Result<Option<BlendRefusal>> {
    if !(radius.is_finite() && radius > 0.0) {
        return Err(SolidError::invalid_input(OP, "a finite positive radius"));
    }
    let Some(eu) = edge::blend_edgeuse(k, b, e)? else {
        return Ok(None);
    };
    let other = eu1::radial(b, eu)?
        .ok_or_else(|| SolidError::invalid_input(OP, "a blend edge use without its radial"))?;
    let interval = b.edge(e)?.interval;
    let options = BinormalOptions::default();
    let mut turn = PI;
    for s in STATIONS {
        let t = interval.evaluate(s);
        let n0 = evaluate_binormal(k, b, eu, t, false, &options)?.face_normal;
        let n1 = evaluate_binormal(k, b, other, t, false, &options)?.face_normal;
        let here = n0.angle_between(n1)?;
        if here < turn {
            turn = here;
        }
    }
    // the blend's tangency angle: faces within it are tangent ([`edge::blend_edgeuse`])
    let tangent = PI / 180.0 * (90.0 - edge::crease_angle(k));
    let reach = 1.0 / (0.5 * turn).cos() - 1.0;
    if turn <= tangent || !(reach.is_finite() && reach > 0.0) {
        return Ok(None);
    }
    let tolerance = b.edge(e)?.tolerance.max(b.tolerance());
    if radius * reach >= tolerance {
        return Ok(None);
    }
    Ok(Some(BlendRefusal::RoundTooSmall {
        edge: e,
        turn,
        tolerance,
        smallest_radius: tolerance / reach,
    }))
}
