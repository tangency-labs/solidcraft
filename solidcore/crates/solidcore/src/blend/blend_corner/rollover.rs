//! The blend corner's Roll-over event: a convex round B1 and a concave round B2 that share
//! a face T at a vertex V, with no crossing to mitre on, the other edges at V not blended.
//! B1 ends as it would alone, on its end face E (B2's other face). B2 rolls over B1's
//! surface and B1's end face: its ball, touching B1's round and E, sweeps a torus about
//! B1's axis A1 (major radius `r1 + r2`, minor radius `r2`: the `Circle` exact class).
//! Where the torus's rail on E crosses E's rim, the material it adds is trimmed flush with
//! the cliff faces continued, at a crease whose dihedral angle fades to zero where the
//! cliff face runs on tangentially into B1 ([`crease`]).
//!
//! Upstream: none (SMLib refuses this corner; the configuration is the generic blend's
//! Roll-over row, `docs/generic-blend.md` §3.2).
//!
//! | Rust       | What                                                         |
//! |------------|--------------------------------------------------------------|
//! | [`crease`] | the creases with the cliff faces: explicit graphs, certified |
//! | [`geometry`] | the torus's frame, the cliff's profile, the creases as curves |
//! | [`RollOver`], [`classify_roll_over`] | the event's corner: which vertex, which round goes first |
//! | [`concave_chain`] | the second pass's edges: B2's remainder and B1's end arc on E |
//! | [`is_concave_chain`] | whether edges asked of one round are such a chain (the event's second pass asked as a feature of its own) |
//! | [`concave_chain_ends`] | the second pass's V: where each end arc of B1 in the chain meets the cliff |
//! | [`cliff_rim`] | the cliff's rim at V: the edges a second pass's concave overflow may leave across |
//!
//! The event runs in two passes of one blend (the caller's, `solidcore-ops`): B1 and the
//! session's other rounds first, then B2's remainder with B1's end arc on E as one concave
//! chain, handed the first pass's faces. The second pass's session is told it is the
//! event's ([`crate::blend::fillet_executive::session::FilletSession::set_roll_over_pass`]):
//! its concave overflow at the cliff is accepted and trimmed at the creases, and its rail
//! ends are looked for across the cliff's tangent seam at V.
//!
//! Deviations: none (not a port). No recursion; [`concave_chain`] walks at most the body's
//! edge count of steps.

pub mod crease;
pub mod geometry;

use crate::blend::blend_corner::mixed::{leaving, mixed_ring};
use crate::blend::fillet_corner::topology::check_tangential_edgeuse_at_vertex;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP_CLASSIFY: &str = "blend::blend_corner::rollover::classify_roll_over";

/// A Roll-over corner: at `vertex`, the convex round's edge `convex` (B1) goes first and the
/// concave round's edge `concave` (B2) then rolls over B1's surface and end face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollOver {
    /// V.
    pub vertex: VertexId,
    /// B1's edge (convex).
    pub convex: EdgeId,
    /// B2's edge (concave).
    pub concave: EdgeId,
}

/// Whether the filleted `vertex`, which no other corner kind takes (not the Mixed corner),
/// is the Roll-over event's, and its rounds. Its ring ([`mixed_ring`]) has four edges: B1's
/// edge E1 (convex) between the common face T and F1, B2's E2 (concave) between T and the
/// end face E, the unblended seam between F1 and the face F3 beyond it, and the unblended
/// edge beyond (E3, between E and F3). The event builds where the ball of B2 sweeps a torus
/// about B1's axis, so it is narrowed to that geometry: T and E planes, E1 a line square to
/// E (E's normal along E1, within `tangency_tol_deg`), F1 a plane or a cylinder whose axis
/// runs along E1 (B1 is then a cylinder about a line parallel to E1, and E is square to it);
/// the seam tangent at V (the cliff F1 + F3 runs on smoothly), and E3 sharp and convex (the
/// cliff's rim). `None` for every other vertex.
///
/// # Errors
/// Those of the ring's convexity and face lookups, and of the tangent tests.
pub fn classify_roll_over(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    tangency_tol_deg: f64,
) -> Result<Option<RollOver>> {
    let any = |_: EdgeuseId| Some(());
    let Some((ring, _)) = mixed_ring(k, b, solver_eus, all_eus, &any)? else {
        return Ok(None);
    };
    let Some(beyond) = ring.beyond else {
        return Ok(None);
    };
    let nul = || SolidError::invalid_input(OP_CLASSIFY, "an edge use of the ring has no edge");
    let convex = eu1::edge(b, ring.convex)?.ok_or_else(nul)?;
    let concave = eu1::edge(b, ring.concave)?.ok_or_else(nul)?;
    let beyond_e = eu1::edge(b, beyond)?.ok_or_else(nul)?;
    // the cliff F1 + F3 runs on smoothly across the seam; its rim E3 is sharp and convex
    if !check_tangential_edgeuse_at_vertex(k, b, ring.seam, vertex, tangency_tol_deg)?
        || check_tangential_edgeuse_at_vertex(k, b, beyond, vertex, tangency_tol_deg)?
        || is_concave_edge(k, b, beyond_e)?
    {
        return Ok(None);
    }
    let along = leaving(k, b, convex, vertex, OP_CLASSIFY)?;
    let straight = match edge::curve(b, convex)? {
        Some(c) => matches!(c.kind(), crate::curve::curve::CurveKind::Line(_)),
        None => false,
    };
    let t_plane = plane_normal(b, ring.t)?;
    let e_plane = plane_normal(b, ring.f2)?;
    let (Some(_), Some(e_normal)) = (t_plane, e_plane) else {
        return Ok(None);
    };
    if !straight || !e_normal.is_parallel_to(along, tangency_tol_deg) {
        return Ok(None);
    }
    let ruled_along = match b.face(ring.f1)?.surface.as_ref().map(Surface::kind) {
        Some(SurfaceKind::Plane(_)) => true,
        Some(SurfaceKind::Cone(c)) => {
            c.is_cylinder()
                && c.rev()
                    .position()
                    .z_axis()
                    .is_parallel_to(along, tangency_tol_deg)
        }
        _ => false,
    };
    if !ruled_along {
        return Ok(None);
    }
    Ok(Some(RollOver {
        vertex,
        convex,
        concave,
    }))
}

/// The unit normal of face `f`'s surface where it is a plane; `None` otherwise.
fn plane_normal(b: &Brep, f: FaceId) -> Result<Option<Vec3>> {
    Ok(match b.face(f)?.surface.as_ref().map(Surface::kind) {
        Some(SurfaceKind::Plane(p)) => Some(p.is_planar().1),
        _ => None,
    })
}

/// The Roll-over's second pass's edges once the first pass has rounded B1 (`first`: the faces
/// it made): `remnants` (what is left of B2's edges) and, walked on from each end of the
/// chain they form, every concave edge of a face in `first` that continues the chain G1 at
/// its end (within the kernel's angular tolerance): B1's end arc on E, along which B2 rolls
/// over B1. The walk takes each edge once, so it ends within the body's edge count.
///
/// # Errors
/// Those of the topology walks, the convexity and the tangency tests.
pub fn concave_chain(
    k: &Kernel,
    b: &mut Brep,
    remnants: &[EdgeId],
    first: &[FaceId],
) -> Result<Vec<EdgeId>> {
    let tol_deg = k.tolerances.ang_tol_deg;
    let mut chain: Vec<EdgeId> = remnants.to_vec();
    let mut grew = true;
    while grew {
        grew = false;
        for at in chain.clone() {
            for end in [edge::start_vertex(b, at)?, edge::end_vertex(b, at)?] {
                let Some(end) = end else {
                    continue;
                };
                for next in crate::topology::vertex::edges(b, end, None)? {
                    if chain.contains(&next) {
                        continue;
                    }
                    let on_first = edge::faces(b, next, None)?
                        .iter()
                        .any(|f| first.contains(f));
                    if !on_first || !is_concave_edge(k, b, next)? {
                        continue;
                    }
                    let (g1, _) = crate::blend::fillet_corner::p1::test_g1_continuity(
                        k,
                        b,
                        Some(end),
                        Some(at),
                        Some(next),
                        tol_deg,
                    )?;
                    if g1 {
                        chain.push(next);
                        grew = true;
                    }
                }
            }
        }
    }
    Ok(chain)
}

/// The vertices V of the Roll-over's second pass in `edges` (the concave chain [`concave_chain`]
/// finds once B1 is rounded), one for each end arc A of B1 in `edges`, in `edges`' order: A is
/// concave, between a cylinder C (B1's round) and a plane E square to C's axis (within the
/// kernel's angular tolerance: B1's end face). At one end of A another edge of `edges`, concave
/// between E and a plane T (B2's remainder), meets A G1, and C meets T there along an edge
/// whose faces meet tangentially (B1 runs off T smoothly). At A's other end, V, C meets a face
/// along an edge whose faces meet tangentially (B1 runs onto the cliff smoothly), and E meets a
/// face along an edge that is sharp and convex (the cliff's rim E3). These are the ledge
/// corner's four edges ([`classify_roll_over`]: T and E planes, E square to B1's axis, the
/// cliff tangent across the seam and its rim sharp and convex) once B1 has been rounded. A
/// pocket floor or a boss foot with rounded vertical corners has no such V: at each end of a
/// corner's arc the floor's edge runs on concave (G1 into the next wall's foot), never sharp and
/// convex, so it is never taken for the Roll-over. Empty for every other set of edges.
///
/// # Errors
/// Those of the topology walks, the convexity and the tangency tests.
pub fn concave_chain_ends(k: &Kernel, b: &mut Brep, edges: &[EdgeId]) -> Result<Vec<VertexId>> {
    let tol_deg = k.tolerances.ang_tol_deg;
    let mut ends = Vec::new();
    for &arc in edges {
        let Some((cylinder, end_face)) = end_arc_faces(b, arc, tol_deg)? else {
            continue;
        };
        if !is_concave_edge(k, b, arc)? {
            continue;
        }
        let (Some(v0), Some(v1)) = (edge::start_vertex(b, arc)?, edge::end_vertex(b, arc)?) else {
            continue;
        };
        if v0 == v1 {
            continue;
        }
        for (near, far) in [(v0, v1), (v1, v0)] {
            if meets_remainder(k, b, edges, arc, cylinder, end_face, near, tol_deg)?
                && meets_cliff(k, b, arc, cylinder, end_face, far, tol_deg)?
                && !ends.contains(&far)
            {
                ends.push(far);
            }
        }
    }
    Ok(ends)
}

/// Where `arc` lies between a cylinder C and a plane E whose normal is along C's axis (within
/// `tol_deg`), `(C, E)`.
fn end_arc_faces(b: &Brep, arc: EdgeId, tol_deg: f64) -> Result<Option<(FaceId, FaceId)>> {
    let faces = edge::faces(b, arc, None)?;
    let [f, g] = faces.as_slice() else {
        return Ok(None);
    };
    for (c, e) in [(*f, *g), (*g, *f)] {
        let axis = match b.face(c)?.surface.as_ref().map(Surface::kind) {
            Some(SurfaceKind::Cone(cone)) if cone.is_cylinder() => {
                Some(cone.rev().position().z_axis())
            }
            _ => None,
        };
        if let (Some(axis), Some(normal)) = (axis, plane_normal(b, e)?) {
            if normal.is_parallel_to(axis, tol_deg) {
                return Ok(Some((c, e)));
            }
        }
    }
    Ok(None)
}

/// Whether the faces of `e` meet tangentially at `v` (through one of its uses; the test is
/// the two faces' normals there).
fn tangential_at(k: &Kernel, b: &mut Brep, e: EdgeId, v: VertexId, tol_deg: f64) -> Result<bool> {
    match edge::edgeuses(b, e, None, None)?.first() {
        Some(&eu) => check_tangential_edgeuse_at_vertex(k, b, eu, v, tol_deg),
        None => Ok(false),
    }
}

/// At `near`, an edge of `edges` other than `arc`, concave between `end_face` and a plane T,
/// meets `arc` G1, and `cylinder` meets T there along an edge whose faces meet tangentially.
#[allow(clippy::too_many_arguments)] // the arc's two faces, the vertex and the tolerance
fn meets_remainder(
    k: &Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    arc: EdgeId,
    cylinder: FaceId,
    end_face: FaceId,
    near: VertexId,
    tol_deg: f64,
) -> Result<bool> {
    for &other in edges {
        if other == arc {
            continue;
        }
        let ends = [edge::start_vertex(b, other)?, edge::end_vertex(b, other)?];
        if !ends.contains(&Some(near)) {
            continue;
        }
        let faces = edge::faces(b, other, None)?;
        let [f, g] = faces.as_slice() else {
            continue;
        };
        let t = match (*f == end_face, *g == end_face) {
            (true, false) => *g,
            (false, true) => *f,
            _ => continue,
        };
        if plane_normal(b, t)?.is_none() || !is_concave_edge(k, b, other)? {
            continue;
        }
        let g1 = crate::blend::fillet_corner::p1::test_g1_continuity(
            k,
            b,
            Some(near),
            Some(arc),
            Some(other),
            tol_deg,
        )?
        .0;
        if !g1 {
            continue;
        }
        for x in crate::topology::vertex::edges(b, near, None)? {
            let xf = edge::faces(b, x, None)?;
            if xf.contains(&cylinder) && xf.contains(&t) && tangential_at(k, b, x, near, tol_deg)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// At `far`, `cylinder` meets a face along an edge whose faces meet tangentially, and
/// `end_face` meets a face along an edge, not `arc`, that is sharp and convex.
fn meets_cliff(
    k: &Kernel,
    b: &mut Brep,
    arc: EdgeId,
    cylinder: FaceId,
    end_face: FaceId,
    far: VertexId,
    tol_deg: f64,
) -> Result<bool> {
    let (mut smooth, mut rim) = (false, false);
    for x in crate::topology::vertex::edges(b, far, None)? {
        if x == arc || edge::is_closed(b, x)? {
            continue;
        }
        let xf = edge::faces(b, x, None)?;
        let tangential = tangential_at(k, b, x, far, tol_deg)?;
        if xf.contains(&cylinder) && !xf.contains(&end_face) && tangential {
            smooth = true;
        }
        if xf.contains(&end_face) && !xf.contains(&cylinder) && !tangential {
            rim |= !is_concave_edge(k, b, x)?;
        }
    }
    Ok(smooth && rim)
}

/// The cliff's rim at the second pass's vertex V (`arc` B1's end arc there): the edges at V,
/// other than `arc`, that are sharp (their faces not tangential at V) and convex, and the edges
/// each continues into G1 across its far vertex while they stay sharp and convex and share a
/// face with it (the rim E3 run on across a seam's vertex). A concave overflow of the
/// second pass is the Roll-over's only where its rails leave their faces across these
/// edges. The walk takes each edge once, so it ends within the body's edge count.
///
/// # Errors
/// Those of the topology walks, the convexity and the tangency tests.
pub fn cliff_rim(k: &Kernel, b: &mut Brep, v: VertexId, arc: EdgeId) -> Result<Vec<EdgeId>> {
    let tol_deg = k.tolerances.ang_tol_deg;
    let mut rim: Vec<(EdgeId, VertexId)> = Vec::new();
    for x in crate::topology::vertex::edges(b, v, None)? {
        if x == arc || edge::is_closed(b, x)? {
            continue;
        }
        if !tangential_at(k, b, x, v, tol_deg)? && !is_concave_edge(k, b, x)? {
            rim.push((x, v));
        }
    }
    let mut next = 0;
    let bound = b.edge_list().len();
    while let Some(&(at, from)) = rim.get(next) {
        next += 1;
        if rim.len() > bound {
            break;
        }
        let far = if edge::start_vertex(b, at)? == Some(from) {
            edge::end_vertex(b, at)?
        } else {
            edge::start_vertex(b, at)?
        };
        let Some(far) = far else {
            continue;
        };
        let faces = edge::faces(b, at, None)?;
        for y in crate::topology::vertex::edges(b, far, None)? {
            if y == arc || rim.iter().any(|&(e, _)| e == y) || edge::is_closed(b, y)? {
                continue;
            }
            let shares = edge::faces(b, y, None)?.iter().any(|f| faces.contains(f));
            if !shares || tangential_at(k, b, y, far, tol_deg)? || is_concave_edge(k, b, y)? {
                continue;
            }
            let g1 = crate::blend::fillet_corner::p1::test_g1_continuity(
                k,
                b,
                Some(far),
                Some(at),
                Some(y),
                tol_deg,
            )?
            .0;
            if g1 {
                rim.push((y, far));
            }
        }
    }
    Ok(rim.into_iter().map(|(e, _)| e).collect())
}

/// Whether `edges` (asked of one round) are the Roll-over's concave chain once B1 is rounded:
/// every edge concave and on one end face E, with at least one V ([`concave_chain_ends`]).
/// Such a round asked on its own (the event's second pass as a feature of its own, after B1
/// was rounded by an earlier one) is built as the event's second pass.
///
/// # Errors
/// Those of [`concave_chain_ends`] and the convexity tests.
pub fn is_concave_chain(k: &Kernel, b: &mut Brep, edges: &[EdgeId]) -> Result<bool> {
    let tol_deg = k.tolerances.ang_tol_deg;
    let mut end_face = None;
    for &e in edges {
        if let Some((_, face)) = end_arc_faces(b, e, tol_deg)? {
            end_face = Some(face);
            break;
        }
    }
    let Some(end_face) = end_face else {
        return Ok(false);
    };
    for &e in edges {
        if !edge::faces(b, e, None)?.contains(&end_face) || !is_concave_edge(k, b, e)? {
            return Ok(false);
        }
    }
    Ok(!concave_chain_ends(k, b, edges)?.is_empty())
}
