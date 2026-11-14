//! A blend refused on its edges or corners, as data ([`BlendRefusal`], carried by
//! [`crate::SolidError::BlendRefused`]), and the queries the refusals rest on: the tangent
//! chain of edges ([`tangent_chain`], [`chains_among`]), the edge a blended edge runs on into
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
    /// A setback asked at `vertex` too short for the corner: `least` is the least setback the
    /// refusing test takes, and `why` says which test refused it and whether a setback of
    /// `least` builds the corner by it.
    SetbackTooShort {
        /// The corner's vertex.
        vertex: VertexId,
        /// The least setback the refusing test takes ([`SetbackTooShortWhy`] says which and
        /// how).
        least: f64,
        /// Which test refused it.
        why: SetbackTooShortWhy,
    },
    /// A setback asked at `vertex` that does not fit on the corner's edges, or on its rounds
    /// where a far corner cuts them: one below `largest` fits. `why` says which test
    /// refused it.
    SetbackTooLong {
        /// The corner's vertex.
        vertex: VertexId,
        /// The size the refusing test names ([`SetbackTooLongWhy`] says which and how).
        largest: f64,
        /// Which test refused it.
        why: SetbackTooLongWhy,
    },
    /// A chamfer's angle to its first face that runs out past the other face instead of
    /// meeting it: the chamfer's line and the other face meet only at an angle less than
    /// pi less the wedge between the faces. Radians, at the wedge the edge's chamfer is
    /// measured at ([`crate::surface::procedural_blend::chamfer`]).
    ChamferAngleTooLarge {
        /// The edge.
        edge: EdgeId,
        /// The angle asked.
        angle: f64,
        /// The bound below which an angle meets the other face: pi less the wedge.
        largest: f64,
    },
}

/// Which test refused a setback as too short ([`BlendRefusal::SetbackTooShort`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetbackTooShortWhy {
    /// It stops short of where the rails of the rounds beside one of the corner's edges cross
    /// the edge ([`crate::blend::blend_setback::distances`]). `least` is the farthest they
    /// cross it from the vertex. A setback of `least` passes this test only: the corner's
    /// later tests may still refuse it.
    RailsCross,
    /// At a Setback corner, its G1 fill is not certified convex at it. `least` is the setback
    /// at the least scale of it, on a grid of thousandths, at which the fill is certified
    /// convex; with its ease it lies within the corner's rooms.
    ConvexFill,
    /// At a Setback corner beside a curved face, its fill is not certified no more concave
    /// than its faces at it. `least` is the setback at the least scale of it, on a grid of
    /// thousandths, at which it is; with its ease it lies within the corner's rooms.
    ConcaveFill,
}

/// Which test refused a setback as too long ([`BlendRefusal::SetbackTooLong`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetbackTooLongWhy {
    /// With its ease it leaves an edge at its corner no length past the far end's claim (that
    /// end's fill's setback and ease, or the setback set on its corner; 0 where it has none):
    /// an open bound. Every set-back corner, before the blends are traced; `largest` is the
    /// least of `length - ease - far` less the fit margin, a thousand of the edge's
    /// tolerances, and a setback above it is refused.
    Overlap,
    /// At a Setback corner, with its ease it does not lie below an edge's room by the
    /// tolerance: `setback + ease >= length - claim - tol`, the far end's claim its fill's
    /// setback and ease (0 where the far end has no fill: a far N x N corner's set setback is
    /// claimed by [`SetbackTooLongWhy::Overlap`] only). `largest` is the least of
    /// `room - ease`.
    Room,
    /// At a Setback corner, with its ease it reaches along an edge as far as a far corner
    /// cuts one of the edge's round's rails, less the fit margin. `largest` is that reach
    /// less the ease.
    PastRails,
    /// At a Setback corner, it takes a spring curve off its face: the curve comes within the
    /// tolerances of another edge of the face, or is not shown not to (solidcore#2343 (d)).
    /// `largest` is the largest scale of the asked setbacks that keeps every spring curve on
    /// its face, times the setback.
    SpringLeaves,
    /// At a Setback corner beside a curved face, the piece it takes out of a face (bounded by
    /// the face's spring curve, the end sections' traces on the face and its two edges from
    /// the vertex to their setbacks) holds another edge or loop of the face, or is not shown
    /// not to (solidcore#2343 (d), condition 2). `largest` is the largest scale of the asked
    /// setbacks at which every spring curve stays on its face and no piece holds another edge
    /// of its face, times the setback.
    PieceHoldsEdge,
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
            | Self::RoundTooSmall { edge, .. }
            | Self::ChamferAngleTooLarge { edge, .. } => EntityRef::Edge(edge),
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

/// Which tangent chain among `edges` each of them is on: one number per edge, the same for
/// two edges exactly when a run of edges of `edges` joins them, each running on into the next
/// at a vertex as in [`tangent_chain`] (an edge not in `edges` joins nothing). A closed
/// rim's arcs are one chain.
///
/// # Errors
/// Those of [`edge::blend_edgeuse`] and [`continuing_edges`].
pub fn chains_among(
    k: &Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    tangency: f64,
) -> Result<Vec<usize>> {
    let mut chains: Vec<usize> = (0..edges.len()).collect();
    let mut ends = Vec::new();
    for &e in edges {
        ends.push([edge::start_vertex(b, e)?, edge::end_vertex(b, e)?]);
    }
    for (i, &e) in edges.iter().enumerate() {
        // only an edge sharing a vertex with another of them can run on into it
        let meets = |(j, other): (usize, &[Option<VertexId>; 2])| {
            let mine = ends.get(i).into_iter().flatten().flatten();
            j != i && mine.into_iter().any(|v| other.contains(&Some(*v)))
        };
        if !ends.iter().enumerate().any(meets) {
            continue;
        }
        for (_, f) in runs_on_into(k, b, e, tangency, false)? {
            let Some(j) = edges.iter().position(|&x| x == f) else {
                continue;
            };
            if let (Some(&keep), Some(&gone)) = (chains.get(i), chains.get(j)) {
                for c in &mut chains {
                    if *c == gone {
                        *c = keep;
                    }
                }
            }
        }
    }
    Ok(chains)
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
/// Its queries build the UV trim curves `e`'s edge uses lack and refit the edge's tolerance
/// to them, so a caller that must leave a body as it is passes a copy (the session does).
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
