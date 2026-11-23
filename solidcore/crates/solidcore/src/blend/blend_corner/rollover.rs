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
//! | [`concave_chain_ends`] | the second pass's V: each end of an end arc where the cliff is |
//! | [`has_end_arc`] | whether an edge lies between a cylinder and a plane square to its axis |
//! | [`cliff_rim`] | the cliff's rim at V: the edges a second pass's concave overflow may leave across |
//! | [`chain_off_seams`], [`seams_back`] | the second pass's chain taken off its cylinders' seams where the round has no whole section at a seam's vertex, and the seams put back (#19505) |
//!
//! The event runs in two passes of one blend (the caller's, `solidcore-ops`): B1 and the
//! session's other rounds first, then B2's remainder with B1's end arc on E as one concave
//! chain, handed the first pass's faces. The second pass's session is told it is the
//! event's ([`crate::blend::fillet_executive::session::FilletSession::set_roll_over_pass`]):
//! its concave overflow at the cliff is accepted and trimmed at the creases, and its rail
//! ends are looked for across the cliff's tangent seam at V.
//!
//! A seam is no event of the pass. The session joins two solvers at a seam's vertex on the
//! chain (#2864), which needs a whole section of the round there. Where the round has none
//! (its rail leaves the end face at the seam's section, where an End has its overflow), the
//! caller takes the chain off the seam first ([`chain_off_seams`]: the cylinder's face given
//! its surface turned about its axis, in the pass's copy) and puts the seam back after the
//! pass ([`seams_back`]), so the result's seam is on the input's ruling (#19505).
//!
//! Deviations: none (not a port). No recursion; [`concave_chain`] walks at most the body's
//! edge count of steps.

pub mod crease;
pub mod geometry;

use crate::blend::blend_corner::mixed::{leaving, mixed_ring};
use crate::blend::fillet_corner::topology::check_tangential_edgeuse_at_vertex;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::curve::step_edge::{edge_interval, EdgeVertices};
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::tol::zone_tol3d;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::query::face::p1::{point_3d_classify_at, Point3dClassifyOptions};
use crate::surface::step_seam::seam_into_widest_gap;
use crate::surface::step_window::BoundaryEdge;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::seam_turn::turn_seam;
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

/// The End of the second pass's concave chain at V whose rib runs across the cliff's tangent
/// seam ([`seam_end`]): the chain's corner at V ends on the end face F (the cliff's face the
/// rail on B1 runs onto across B1's tangent seam), and F meets the wall (the cliff's other
/// face, past E's rim) along the seam at W, the far end of F's edge on E. The rib runs from
/// the tangent end on F to the seam at S, then on the wall to the rail's end on the rim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeamEnd {
    /// W: the far end of the end face's edge on E, where the seam meets E.
    pub vertex: VertexId,
    /// W's point.
    pub at: Point3,
    /// The wall: the face across the seam from the end face (a plane parallel to B1's axis).
    pub wall: FaceId,
    /// S: where the torus crosses the seam's line, the point the rib passes from the end face
    /// onto the wall.
    pub split: Point3,
}

/// Whether the second pass's corner at V (`corner_vertex`) ends across the cliff's tangent
/// seam, and where: [`SeamEnd`]. `end_face` is F, the face its tangent end's rib lies on
/// (across B1's tangent seam `tangent_seam` from B1's face), `side_edge` the corner's other
/// side edge (F's edge on E, from V to W), `r2` the concave round's radius.
///
/// The torus is the Roll-over's: B1 a cylinder (axis A1, radius `r1`), E a plane square to
/// A1, the torus's centre on A1 at `r2` from E on E's outer side (the ball's), major radius
/// `R = r1 + r2`, minor `r2`. F must be a cylinder along A1 and the wall a plane parallel to A1, meeting F
/// tangentially at W along the seam (their profiles square to A1 meet G1 at W's profile
/// point), the seam running on from W away from B1, so that S, on B1's side of W, lies on
/// the wall's continuation over the seam's line. The torus's rail on E crosses E's rim, not F's edge, exactly where W's distance
/// `rho` from A1 is less than `R` (the seam inside the rail's circle on E, as
/// [`geometry::Profile::pieces`] takes it); then S is on the seam's line at the torus's
/// height `h = sqrt((rho - r1) (R + r2 - rho))` over its centre plane, on E's side. The
/// decision needs `r1 + tol < rho < R - tol`: within `tol` of the rail through W, or of
/// B1's rail, `None` (the corner is as before).
///
/// `None` wherever the geometry is not this one.
///
/// # Errors
/// Those of the topology walks and the tangency test.
#[allow(clippy::too_many_arguments)] // the corner's faces, edges, the radius and the tolerance
pub fn seam_end(
    k: &Kernel,
    b: &mut Brep,
    corner_vertex: VertexId,
    end_face: FaceId,
    side_edge: EdgeId,
    tangent_seam: EdgeId,
    r2: f64,
    tol: f64,
) -> Result<Option<SeamEnd>> {
    let tol_deg = k.tolerances.ang_tol_deg;
    let sized = r2 > 0.0 && r2.is_finite() && tol > 0.0 && tol.is_finite();
    if !sized {
        return Ok(None);
    }
    // B1: the face across the tangent seam from F, a cylinder
    let seam_faces = edge::faces(b, tangent_seam, None)?;
    let [f, g] = seam_faces.as_slice() else {
        return Ok(None);
    };
    let b1 = match (*f == end_face, *g == end_face) {
        (true, false) => *g,
        (false, true) => *f,
        _ => return Ok(None),
    };
    let cylinder = |f: FaceId| -> Result<Option<(Point3, Vec3, f64)>> {
        Ok(match b.face(f)?.surface.as_ref().map(Surface::kind) {
            Some(SurfaceKind::Cone(c)) if c.is_cylinder() => {
                let place = c.rev().position();
                Some((place.origin(), place.z_axis(), c.radius(0.0)))
            }
            _ => None,
        })
    };
    let (Some((origin, axis, r1)), Some((_, f_axis, _))) = (cylinder(b1)?, cylinder(end_face)?)
    else {
        return Ok(None);
    };
    let Ok(axis) = axis.unitize() else {
        return Ok(None);
    };
    if !(r1 > tol && r1.is_finite() && f_axis.is_parallel_to(axis, tol_deg)) {
        return Ok(None);
    }
    // E: F's other face along the side edge, a plane square to A1
    let side_faces = edge::faces(b, side_edge, None)?;
    let Some(&e_face) = side_faces.iter().find(|&&f| f != end_face) else {
        return Ok(None);
    };
    match plane_normal(b, e_face)? {
        Some(n) if n.is_parallel_to(axis, tol_deg) => {}
        _ => return Ok(None),
    }
    // W: the side edge's far end
    let far = if edge::start_vertex(b, side_edge)? == Some(corner_vertex) {
        edge::end_vertex(b, side_edge)?
    } else {
        edge::start_vertex(b, side_edge)?
    };
    let Some(w) = far.filter(|&w| w != corner_vertex) else {
        return Ok(None);
    };
    // the seam at W: F against a plane parallel to A1, not E, tangential at W
    let sin_tol = (tol_deg * (crate::math::PI / 180.0)).sin();
    let mut wall = None;
    for x in crate::topology::vertex::edges(b, w, None)? {
        if x == side_edge || edge::is_closed(b, x)? {
            continue;
        }
        let xf = edge::faces(b, x, None)?;
        let [f, g] = xf.as_slice() else {
            continue;
        };
        let other = match (*f == end_face, *g == end_face) {
            (true, false) => *g,
            (false, true) => *f,
            _ => continue,
        };
        if other == e_face {
            continue;
        }
        let parallel = match plane_normal(b, other)? {
            Some(n) => n.dot(axis).abs() <= sin_tol * n.length(),
            None => false,
        };
        if parallel && tangential_at(k, b, x, w, tol_deg)? {
            wall = Some((other, x));
            break;
        }
    }
    let Some((wall, seam)) = wall else {
        return Ok(None);
    };
    // the torus lies on E's outer side, where the ball rolls on E: towards E is against E's
    // outward normal at the rim. At a ledge B1's tangent seam leaves V on that side; a
    // cylinder that runs on past E meets the cliff on the other side (#19476), so the seam's
    // direction does not tell.
    let Some(rim_use) = edge::edgeuse_of_face(b, side_edge, Some(e_face))? else {
        return Ok(None);
    };
    let rim_middle = b.edge(side_edge)?.interval.evaluate(0.5);
    let (_, out) = crate::blend::chamfer_side::outward(k, b, rim_use, rim_middle)?;
    let towards_e = if out.dot(axis) > 0.0 { -axis } else { axis };
    // the seam runs on from W past E, away from B1: S, on B1's side of W, is then on the
    // wall's continuation over the seam's line (the extension the End's rib lies on)
    if leaving(k, b, seam, w, OP_SEAM)?.dot(towards_e) <= 0.0 {
        return Ok(None);
    }
    let at = b.vertex(w)?.point;
    let on_e = origin + axis * (at - origin).dot(axis);
    let centre = on_e - towards_e * r2;
    let radii = crease::Radii { r1, r2 };
    let Ok(frame) = geometry::TorusFrame::new(centre, towards_e, at - centre, radii) else {
        return Ok(None);
    };
    // W is at height r2 over the centre plane by construction (the centre is W's foot on A1
    // moved r2 off E, which holds W)
    let q = frame.local(at);
    let rho = (q[0] * q[0] + q[1] * q[1]).sqrt();
    let big_r = r1 + r2;
    let inside = rho > r1 + tol && rho < big_r - tol;
    if !inside {
        return Ok(None);
    }
    let h = ((rho - r1) * (big_r + r2 - rho)).sqrt();
    // on the seam's line through W (parallel to A1), at the torus's height
    let split = at - towards_e * (r2 - h);
    let finite = split.x.is_finite() && split.y.is_finite() && split.z.is_finite();
    if !finite {
        return Ok(None);
    }
    Ok(Some(SeamEnd {
        vertex: w,
        at,
        wall,
        split,
    }))
}

const OP_SEAM: &str = "blend::blend_corner::rollover::seam_end";

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

/// `edges` and the edges they run on into across seams' vertices (the pieces of their own
/// curves that only a seam splits off, which the session blends with them, #2864): the chain's
/// ends are those of the whole run.
///
/// # Errors
/// Those of the topology walks and the seam and tangency tests.
fn with_run_ons(k: &Kernel, b: &mut Brep, edges: &[EdgeId]) -> Result<Vec<EdgeId>> {
    let mut all = edges.to_vec();
    let tol_deg = k.tolerances.ang_tol_deg;
    for run_on in
        crate::blend::fillet_executive::session::runs_on_across_seams(k, b, edges, tol_deg)?
    {
        all.push(run_on.edge);
    }
    Ok(all)
}

/// The vertices V of the Roll-over's second pass in `edges` and the edges they run on into
/// across seams' vertices (`with_run_ons`): each end of each end arc A where the cliff is. A is
/// concave, between a cylinder C and a plane E square to C's axis (within the kernel's angular
/// tolerance): B1's end arc on its end face once B1 is rounded (the ledge, #10876), or the foot
/// of a cylinder that stands through E (a tube above a fin's top, #19476). At V, C meets a face
/// along an edge whose faces meet tangentially (C runs onto the cliff smoothly), and E meets a
/// face along an edge that is sharp and convex (the cliff's rim). An arc may have a V at each
/// end. A pocket floor or a boss foot with rounded vertical corners has no such V: at each end
/// of a corner's arc the floor's edge runs on concave (G1 into the next wall's foot), never
/// sharp and convex. A ledge's arc has none where it joins B2's remainder: E's other edge there
/// is the remainder, concave. Empty for every other set of edges.
///
/// # Errors
/// Those of the topology walks, the convexity and the tangency tests.
pub fn concave_chain_ends(k: &Kernel, b: &mut Brep, edges: &[EdgeId]) -> Result<Vec<VertexId>> {
    let edges = &with_run_ons(k, b, edges)?;
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
        for far in [v0, v1] {
            if meets_cliff(k, b, arc, cylinder, end_face, far, tol_deg)? && !ends.contains(&far) {
                ends.push(far);
            }
        }
    }
    Ok(ends)
}

/// Whether an edge of `edges` lies between a cylinder and a plane square to its axis (an end
/// arc, [`concave_chain_ends`]): what a caller reads, without touching `b`, to know that the
/// edges may be the Roll-over's concave chain.
///
/// # Errors
/// Those of the topology walks.
pub fn has_end_arc(k: &Kernel, b: &Brep, edges: &[EdgeId]) -> Result<bool> {
    let tol_deg = k.tolerances.ang_tol_deg;
    for &e in edges {
        if end_arc_faces(b, e, tol_deg)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
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

/// Whether `edges` (asked of one round), with the edges they run on into across seams'
/// vertices, are the Roll-over's concave chain once B1 is rounded: every edge concave and on
/// one end face E, with at least one V ([`concave_chain_ends`]).
/// Such a round asked on its own (the event's second pass as a feature of its own, after B1
/// was rounded by an earlier one) is built as the event's second pass.
///
/// # Errors
/// Those of [`concave_chain_ends`] and the convexity tests.
pub fn is_concave_chain(k: &Kernel, b: &mut Brep, edges: &[EdgeId]) -> Result<bool> {
    let edges = &with_run_ons(k, b, edges)?;
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

/// A cylinder of the chain whose seam [`chain_off_seams`] turned off the chain: the surface it
/// had and the one it was given.
#[derive(Debug, Clone)]
pub struct TurnedSeam {
    /// Its surface before.
    pub was: Surface,
    /// Its surface turned about its axis.
    pub turned: Surface,
}

/// The second pass's chain with its cylinders' seams turned off it ([`chain_off_seams`]).
#[derive(Debug, Clone)]
pub struct OffSeams {
    /// The chain's edges now: the asked edges and the edges they ran on into that are left
    /// (two pieces a seam split are one edge).
    pub edges: Vec<EdgeId>,
    /// Their sizes, one an edge.
    pub sizes: Vec<f64>,
    /// The seams turned, for [`seams_back`].
    pub turned: Vec<TurnedSeam>,
}

/// The Roll-over's second pass's chain `edges` (rounds of `sizes`, one an edge) taken off the
/// seams of its cylinders, in the pass's own copy `b` (#19505). A seam is the cylinder's
/// parameterisation, not a feature (#2864): the session blends a chain across a seam's vertex
/// with two solvers joined there, and the Join needs a whole section at the vertex. Where the
/// round has none there (its contact on the end face E, `size` from the vertex along the
/// cylinder's outward normal, is on E's boundary or off E: the rail leaves E at the seam's
/// section, where an End of the pass has its overflow), the cylinder's face is given its
/// surface turned about its axis so that the seam lies in the middle of the widest gap the
/// chain's arcs on it leave ([`seam_into_widest_gap`], the arcs as its boundary), by
/// [`turn_seam`]. The chain's pieces the seam split are then one edge and the pass meets no
/// vertex there. [`seams_back`] puts the seams back after the pass.
///
/// `None`, and `b` untouched, where no seam is turned: no seam's vertex inside the chain, the
/// round whole at each one (the Join builds as before), the pieces at a seam asked at
/// different sizes, or a chain that leaves its cylinder no gap.
///
/// # Errors
/// Those of the topology walks, the classification, [`seam_into_widest_gap`] and
/// [`turn_seam`]. After an error `b` may have a seam turned: the caller gives the copy up.
pub fn chain_off_seams(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    sizes: &[f64],
) -> Result<Option<OffSeams>> {
    let tol_deg = k.tolerances.ang_tol_deg;
    let tol = b.tolerance();
    let mut all: Vec<(EdgeId, f64)> = edges.iter().copied().zip(sizes.iter().copied()).collect();
    for run_on in
        crate::blend::fillet_executive::session::runs_on_across_seams(k, b, edges, tol_deg)?
    {
        let Some(&size) = sizes.get(run_on.source) else {
            return Ok(None);
        };
        all.push((run_on.edge, size));
    }
    let chain: Vec<EdgeId> = all.iter().map(|&(e, _)| e).collect();
    // the cylinders with a seam's vertex inside the chain where the round has no whole section
    let mut cylinders: Vec<FaceId> = Vec::new();
    for &(arc, size) in &all {
        let Some((c, e)) = end_arc_faces(b, arc, tol_deg)? else {
            continue;
        };
        let (s, z) = edge::start_end_vertices(b, arc)?;
        for v in [s, z].into_iter().flatten() {
            if cylinders.contains(&c) || !seam_vertex_in_chain(k, b, arc, c, v, &chain)? {
                continue;
            }
            if !section_is_whole(k, b, arc, c, e, v, size, tol)? {
                cylinders.push(c);
            }
        }
    }
    let mut turned = Vec::new();
    for c in cylinders {
        let mut arcs: Vec<(EdgeId, f64)> = Vec::new();
        for &(arc, size) in &all {
            if matches!(end_arc_faces(b, arc, tol_deg)?, Some((f, _)) if f == c) {
                arcs.push((arc, size));
            }
        }
        // two pieces joined are one edge, which takes one size
        let one_size = arcs
            .windows(2)
            .all(|w| matches!(w, [a, z] if a.1.to_bits() == z.1.to_bits()));
        let Some(surface) = b.face(c)?.surface.clone().filter(|_| one_size) else {
            continue;
        };
        let mut curves = Vec::new();
        for &(arc, _) in &arcs {
            curves.push(step_arc(k, b, arc, tol)?);
        }
        // each arc there and back: a closed boundary whose window is the arc's own
        let loops: Vec<Vec<BoundaryEdge<'_>>> = curves
            .iter()
            .map(|(curve, interval)| {
                [true, false]
                    .into_iter()
                    .map(|forward| BoundaryEdge {
                        curve,
                        interval: *interval,
                        forward,
                    })
                    .collect()
            })
            .collect();
        let placed = seam_into_widest_gap(k, &surface, &loops, &[], None, tol)?;
        if placed.u.angle.is_none() {
            continue;
        }
        turn_seam(k, b, c, placed.surface.clone())?;
        turned.push(TurnedSeam {
            was: surface,
            turned: placed.surface,
        });
    }
    if turned.is_empty() {
        return Ok(None);
    }
    let now = b.edge_list().to_vec();
    let (edges, sizes) = all.into_iter().filter(|(e, _)| now.contains(e)).unzip();
    Ok(Some(OffSeams {
        edges,
        sizes,
        turned,
    }))
}

/// The seams [`chain_off_seams`] turned, put back once the pass has built: every face of `b`
/// on a turned surface (the same cylinder, placed as the turn placed it) is given the surface
/// it had ([`turn_seam`]), so the seam's ruling is the input's and the edges the turned seam
/// split are whole again.
///
/// # Errors
/// Those of [`turn_seam`].
pub fn seams_back(k: &mut Kernel, b: &mut Brep, turned: &[TurnedSeam]) -> Result<()> {
    for t in turned {
        let Some(placed) = cylinder_placed(&t.turned) else {
            continue;
        };
        let mut on = Vec::new();
        for &f in b.face_list() {
            if b.face(f)?.surface.as_ref().and_then(cylinder_placed) == Some(placed) {
                on.push(f);
            }
        }
        for f in on {
            turn_seam(k, b, f, t.was.clone())?;
        }
    }
    Ok(())
}

/// A cylinder's position and its radius's bits; `None` for another surface.
fn cylinder_placed(s: &Surface) -> Option<(Axis2Placement, u64)> {
    match s.kind() {
        SurfaceKind::Cone(c) if c.is_cylinder() => {
            Some((*c.rev().position(), c.radius(0.0).to_bits()))
        }
        _ => None,
    }
}

/// Whether `v`, an end of the chain's edge `arc` on the cylinder `c`, is a seam's vertex
/// inside the chain: another edge of the chain ends there, and a seam of `c` (an edge with
/// both sides on `c`).
fn seam_vertex_in_chain(
    k: &Kernel,
    b: &mut Brep,
    arc: EdgeId,
    c: FaceId,
    v: VertexId,
    chain: &[EdgeId],
) -> Result<bool> {
    let (mut runs_on, mut seam) = (false, false);
    for x in crate::topology::vertex::edges(b, v, None)? {
        if x == arc {
            continue;
        }
        if chain.contains(&x) {
            runs_on = true;
        } else if edge::faces(b, x, None)?.as_slice() == [c] && edge::is_seam(k, b, x)? {
            seam = true;
        }
    }
    Ok(runs_on && seam)
}

/// Whether the round `size` of `arc` (between the cylinder `c` and the plane `e`) has a whole
/// section at `arc`'s end `v`: its contact on `e` there, `size` from `v` along `c`'s outward
/// normal (the ball's centre is `size` from both faces on their outer sides, so its foot on
/// `e` is that point), is classified inside `e`'s face, not on its boundary and not off it
/// ([`point_3d_classify_at`] at the body's tolerance `tol`).
#[allow(clippy::too_many_arguments)] // the arc, its faces, its end, the size and the tolerance
fn section_is_whole(
    k: &Kernel,
    b: &mut Brep,
    arc: EdgeId,
    c: FaceId,
    e: FaceId,
    v: VertexId,
    size: f64,
    tol: f64,
) -> Result<bool> {
    let Some(eu) = edge::edgeuse_of_face(b, arc, Some(c))? else {
        return Ok(false);
    };
    let interval = b.edge(arc)?.interval;
    let t = if edge::start_vertex(b, arc)? == Some(v) {
        interval.min()
    } else {
        interval.max()
    };
    let (_, out) = crate::blend::chamfer_side::outward(k, b, eu, t)?;
    let contact = b.vertex(v)?.point + out * size;
    let mut pc = PointClassification::new(zone_tol3d(k, tol));
    point_3d_classify_at(
        k,
        b,
        e,
        contact,
        tol,
        true,
        &mut pc,
        Point3dClassifyOptions::default(),
    )?;
    Ok(pc.point_class() == PointClassType::Face)
}

/// `arc`'s curve and its STEP interval on it, as [`BoundaryEdge`] takes them: an edge runs
/// along its curve's parameter from its start vertex to its end vertex.
fn step_arc(k: &Kernel, b: &Brep, arc: EdgeId, tol: f64) -> Result<(Curve, Extent1d)> {
    let nul = || SolidError::invalid_input(OP_OFF_SEAMS, "a chain edge without curve or ends");
    let curve = edge::curve(b, arc)?.ok_or_else(nul)?.clone();
    let (Some(s), Some(z)) = edge::start_end_vertices(b, arc)? else {
        return Err(nul());
    };
    let vertices = EdgeVertices::Pair {
        start: b.vertex(s)?.point,
        end: b.vertex(z)?.point,
    };
    let at = edge_interval(k, &curve, vertices, true, tol)?;
    Ok((curve, at.interval()))
}

const OP_OFF_SEAMS: &str = "blend::blend_corner::rollover::chain_off_seams";
