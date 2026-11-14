//! The end of an `NByOne` corner: the end rib and, at a mixed-convexity corner, the
//! surface extension and the cap topology around the original vertex.
//!
//! Upstream: `source/SMLib/src/SmFilletCorner.cpp` (606–667, 6177–6443).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`find_surface_extension_dist_at_corner`] | `sm_FindSurfaceExtensionDistAtCorner` |
//! | `finish_nx1_end` (crate) | tail of `SmFilletNx1Corner::MakeCornerTopology` |
//!
//! Deviations: the solver is the caller's concrete solver object instead of a
//! `GetSolverType` cast. A failed variable-radius law evaluation yields its status
//! code as the radius, as upstream's `SER` inside the `double`
//! `SmVariableRadiusFS::GetFilletRadius` does (an `Unported` callee and a law refused
//! past its curve, #2706, still propagate). The gap checks of a multi-neighbour end
//! face (vertex and side-edge geometry, extended-surface coincidence), the last statements of
//! `MakeCornerTopology`, are returned as an [`Nx1GapCheck`] for the caller, which
//! holds the corner table and solver store they need, to run before anything else.
//! The end face at a tangent side is extended `CInfinity` and that side's cap edge is
//! `OnExtendEdge` (WR-600, the fork's tangency-labs/smlib@856b54e). At a multi-neighbour
//! end face whose side edges are both cliff edges, the end face is the side face whose
//! surface holds the other side edge; upstream takes the first side's, whatever its
//! surface. There is no recursion.

use std::collections::BTreeMap;

use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_executive::session::{FilletBody, FilletObject, SolverObject};
use crate::blend::fillet_geom::p1::{
    FilletBrep, FilletCornerId, FilletEdgeType, FilletGeomId, FilletVertex, FilletVertexType,
};
use crate::blend::fillet_standard_solver::p1::VariableRadiusFs;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{BrepFaceServices, FaceCaches};
use crate::surface::surface::Surface;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, Vertex, VertexId};

const OP: &str = "blend::fillet_corner::topology::make_nx1_corner_topology";

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn need<T>(value: Option<T>, reason: &'static str) -> Result<T> {
    value.ok_or_else(|| invalid(reason))
}

/// Whether the surface of `side`'s face holds the whole edge of `other` within `tol_3d`
/// (the surface's coincidence check, as the gap check runs it on the end face's extension).
///
/// # Errors
/// [`SolidError::InvalidInput`] for an edge use without face, surface, edge or curve; those
/// of the coincidence check.
fn face_holds_edge(
    k: &Kernel,
    target: &mut Brep,
    side: EdgeuseId,
    other: EdgeuseId,
    tol_3d: f64,
) -> Result<bool> {
    let face = need(eu1::face(target, side)?, "side edge use without face")?;
    let surface = need(
        crate::topology::face::p2::surface(target, face)?,
        "side face without surface",
    )?
    .clone_for_evaluation()?;
    let other_edge = need(eu1::edge(target, other)?, "side edge use without edge")?;
    let curve = need(edge::curve(target, other_edge)?, "side edge without curve")?
        .clone_for_evaluation()?;
    let interval = edge::interval(target, other_edge)?;
    // the face's own surface: its trimmed-surface services come from the B-rep
    let mut services = BrepFaceServices::with_caches(target, FaceCaches::new());
    Ok(surface
        .simple_coincidence_checker_with_context(
            k,
            &surface.natural_uv_domain_with_nurb(k)?,
            &curve,
            &interval,
            tol_3d,
            false,
            [
                Some(CacheObject::Curve(other_edge)),
                Some(CacheObject::Surface(face)),
            ],
            None,
            &mut services,
        )?
        .is_some())
}

/// `sm_FindSurfaceExtensionDistAtCorner`: an extension distance for the end surface of
/// an Nx1 or 3x2 mixed-convexity corner, `7.5` times the largest fillet radius of a
/// constant-radius solver, the distance of a constant-distance solver or the radius a
/// variable-radius solver has at the corner end of `fillet_edgeuse` (`0` for any other
/// solver, whose `SE(SM_ERR)` is only reported).
///
/// # Errors
/// [`SolidError::InvalidInput`] for an edge use without vertex use or edge (upstream
/// dereferences them); [`SolidError::Unported`] from the variable-radius law evaluation.
pub fn find_surface_extension_dist_at_corner(
    target: &Brep,
    fillet_edgeuse: EdgeuseId,
    solver: &mut SolverObject,
    corner_vertex: VertexId,
) -> Result<f64> {
    let mut ext_dist = 0.0;
    match solver {
        // return largest fillet radius
        SolverObject::ConstantRadius(cr) => {
            for i in 0..2 {
                let rad = cr.fillet_radius(i)?;
                if rad > ext_dist {
                    ext_dist = rad;
                }
            }
        }
        // return constant distance
        SolverObject::ConstantDistance(cd) => ext_dist = cd.distance(),
        // return radius at vertex
        SolverObject::VariableRadius(vr) => {
            ext_dist = variable_radius_at_vertex(target, fillet_edgeuse, vr, corner_vertex)?;
        }
        // SE(SM_ERR): reported only
        SolverObject::SurfaceSurface(_) => {}
    }

    // In prog_test, Fillet test my_test_suite_0 iter: 2 will create a surface
    // with a slight crease for extension values greater than about 7.5.
    let rad_factor = 7.5;
    Ok(rad_factor * ext_dist)
}

/// `SmVariableRadiusFS::GetFilletRadius` at the corner end of `fillet_edgeuse`: the law's
/// radius at the end of the filleted edge that lies on `corner_vertex`, by
/// [`FilletLaw::fillet_radius`](crate::blend::fillet_executive::p1::FilletLaw): a failed
/// law evaluation yields its status code as the radius.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an edge use without vertex use or edge (upstream
/// dereferences them); those [`FilletLaw::fillet_radius`] propagates: an unported callee,
/// and the refusal of a law continued past its curve to an unusable radius (the corner may
/// have narrowed the law to its setback, so the edge's end can be past the law's curve).
///
/// [`FilletLaw::fillet_radius`]: crate::blend::fillet_executive::p1::FilletLaw::fillet_radius
pub fn variable_radius_at_vertex(
    target: &Brep,
    fillet_edgeuse: EdgeuseId,
    vr: &VariableRadiusFs,
    corner_vertex: VertexId,
) -> Result<f64> {
    let e = need(eu1::edge(target, fillet_edgeuse)?, "edge use without edge")?;
    let ivl = edge::interval(target, e)?;
    let vu = need(
        eu1::vertexuse(target, fillet_edgeuse)?,
        "edge use without vertex use",
    )?;
    let vert = target.vertexuse(vu)?.vertex;
    let orient = eu1::orientation(target, fillet_edgeuse)?;
    let parameter = if (vert == Some(corner_vertex) && orient == OrientType::Opposite)
        || (vert != Some(corner_vertex) && orient == OrientType::Same)
    {
        ivl.max()
    } else {
        ivl.min()
    };
    // a copy: the evaluation may build the law's extension (`m_pExtendedLaw`), the solver's
    // own law keeps building it when it evaluates
    vr.law
        .clone()
        .fillet_radius(parameter, &vr.law_interval, vr.law_orientation)
}

/// Not upstream (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): where a rail
/// rolled over a smooth edge T of its face S onto the face X beyond ends, when the side edge E
/// at the corner's vertex (between S and the end face P) reaches T's vertex and the edge N there
/// between X and P turns back along E (a boss standing taller than the end face, whose foot runs
/// round the boss away from the rail). The end face's surface meets X along N's curve continued
/// back past that vertex, and the rail ends there, inside X: the cap's edge runs from the rail's
/// end along that continuation to the vertex, then along E.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnedBack {
    /// T's vertex at E's far end, where N starts.
    pub far: VertexId,
    /// N, the edge between X and the end face that turns back at `far`.
    pub edge: EdgeId,
    /// X, the face the rail rolled onto.
    pub face: FaceId,
}

/// [`TurnedBack`] for the rail rolled onto `rolled` whose corner side edge use is `side` at the
/// corner's vertex `vertex`: `None` unless E's far vertex holds a smooth edge (within
/// `tangency_tol_rad`) between E's other face and `rolled`, and an edge between `rolled` and
/// the face of E it shares whose tangent leaving that vertex points back along E's arriving
/// tangent (negative dot product; at a smooth T the two are parallel).
///
/// # Errors
/// Those of the topology walks, the evaluations and the tangency test.
pub(crate) fn rolled_rail_turns_back(
    k: &Kernel,
    target: &mut Brep,
    side: EdgeuseId,
    vertex: VertexId,
    rolled: FaceId,
    tangency_tol_rad: f64,
) -> Result<Option<TurnedBack>> {
    let Some(e) = eu1::edge(target, side)? else {
        return Ok(None);
    };
    if edge::is_closed(target, e)? {
        return Ok(None);
    }
    let (start, end) = (edge::start_vertex(target, e)?, edge::end_vertex(target, e)?);
    let (far, e_arrives_at_end) = if start == Some(vertex) {
        (end, true)
    } else if end == Some(vertex) {
        (start, false)
    } else {
        return Ok(None);
    };
    let Some(far) = far else {
        return Ok(None);
    };
    let e_faces = edge::faces(target, e, None)?;
    if e_faces.len() != 2 || e_faces.contains(&rolled) {
        return Ok(None);
    }
    let edges = crate::topology::vertex::edges(target, far, None)?;
    // T: between a face of E and `rolled`, smooth
    let mut rolled_off = None;
    for &t in &edges {
        if t == e {
            continue;
        }
        let t_faces = edge::faces(target, t, None)?;
        let Some(&s) = t_faces.iter().find(|f| e_faces.contains(f)) else {
            continue;
        };
        if !t_faces.contains(&rolled) {
            continue;
        }
        if crate::blend::fillet_solver::p1::test_manifold_tangent_rollover(
            k,
            target,
            t,
            tangency_tol_rad,
        )?
        .0
        {
            rolled_off = Some(s);
            break;
        }
    }
    let Some(s) = rolled_off else {
        return Ok(None);
    };
    let Some(&p) = e_faces.iter().find(|&&f| f != s) else {
        return Ok(None);
    };
    let tangent = |edge_id: EdgeId, at_end: bool| -> Result<Vec3> {
        crate::topology::vertex::edge_end_tangent(k, target, edge_id, !at_end)
    };
    // E's tangent arriving at `far`
    let te = if e_arrives_at_end {
        tangent(e, true)?
    } else {
        -tangent(e, false)?
    };
    for &n in &edges {
        if n == e || edge::is_closed(target, n)? {
            continue;
        }
        let n_faces = edge::faces(target, n, None)?;
        if !(n_faces.contains(&rolled) && n_faces.contains(&p)) {
            continue;
        }
        // N's tangent leaving `far`
        let Some(tn) = tangent_leaving(k, target, n, far)? else {
            continue;
        };
        if te.dot(tn) < 0.0 {
            return Ok(Some(TurnedBack {
                far,
                edge: n,
                face: rolled,
            }));
        }
    }
    Ok(None)
}

/// Not upstream (solidcore-native, [`TurnedBack`]): the tangent of the edge `edge` leaving its
/// end `vertex`, along its curve away from it (the curve's tangent at its start, reversed at
/// its end); `None` where `vertex` is neither end. For an open edge: a closed one leaves its
/// vertex both ways.
///
/// # Errors
/// Those of the topology walks and the evaluation.
pub(crate) fn tangent_leaving(
    k: &Kernel,
    target: &Brep,
    edge: EdgeId,
    vertex: VertexId,
) -> Result<Option<Vec3>> {
    if edge::start_vertex(target, edge)? == Some(vertex) {
        Ok(Some(crate::topology::vertex::edge_end_tangent(
            k, target, edge, true,
        )?))
    } else if edge::end_vertex(target, edge)? == Some(vertex) {
        Ok(Some(-crate::topology::vertex::edge_end_tangent(
            k, target, edge, false,
        )?))
    } else {
        Ok(None)
    }
}

/// A fillet vertex on the original vertex `tb.far` (`SM_FV_ON_VERTEX`), for a [`TurnedBack`]
/// cap edge.
fn turned_back_vertex(
    k: &Kernel,
    pseudo: &mut FilletBrep,
    corner: FilletCornerId,
    tb: TurnedBack,
) -> Result<VertexId> {
    let fv = pseudo.brep_mut().add_unlinked_vertex_record(Vertex {
        point: Point3::ZERO,
        tolerance: 0.0,
        ..Vertex::default()
    })?;
    pseudo.insert_fillet_vertex(fv, FilletVertex::new(k, Some(corner)));
    let record = pseudo.fillet_vertex_mut(fv)?;
    record.set_point_class(PointClassType::Vertex, Some(EntityRef::Vertex(tb.far)));
    record.set_fillet_vertex_type(FilletVertexType::OnVertex);
    Ok(fv)
}

/// The live objects of the end of `SmFilletNx1Corner::MakeCornerTopology`.
pub(crate) struct Nx1End<'a> {
    pub k: &'a Kernel,
    pub target: &'a mut Brep,
    pub pseudo: &'a mut FilletBrep,
    pub corner: FilletCornerId,
    pub solver: &'a mut SolverObject,
    pub extended_surfaces: &'a mut BTreeMap<FaceId, Surface>,
    pub topo_edges: &'a mut Vec<Option<FilletObject>>,
    pub geometry: FilletGeomId,
    pub filleted: EdgeuseId,
    pub vertex: VertexId,
    pub total: usize,
    pub mixed: bool,
    pub same: [bool; 2],
    /// `bTangentSide` (the fork's tangency-labs/smlib@856b54e): each side edge that is a
    /// tangent seam the rail on its side crosses; `false` without WR-600's fix.
    pub tangent_side: [bool; 2],
    pub sides: [EdgeuseId; 2],
    pub end_face: Option<FaceId>,
    pub extend_edge: Option<EdgeId>,
    /// Not upstream: each rail rolled onto a face whose foot edge turns back at the rolled-over
    /// edge's vertex ([`TurnedBack`]); `None` for every other rail.
    pub turned_back: [Option<TurnedBack>; 2],
}

/// The gap checks `SmFilletNx1Corner::MakeCornerTopology` ends with when the end face
/// of a mixed-convexity corner connects to more than the fillet surface and one other
/// face: `CalcCornerVertGeom`, `CalcCornerEdgeGeom` of the two new side edges (statuses
/// ignored) and, when the end face's extended surface was kept, its
/// `SimpleCoincidenceChecker` against the other side edge, clearing
/// `m_bExtendedSurfacePatch` when they are not coincident. The caller runs them right
/// after the topology returns, before anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nx1GapCheck {
    /// `pNewFilletEdge1` (rail-1 vertex side, along `pEU[0]`).
    pub edge1: EdgeId,
    /// `pNewFilletEdge2` (rail-2 vertex side, along `pEU[1]`).
    pub edge2: EdgeId,
    /// The end face whose new extended surface stayed in the extended-surface map
    /// (`pNewExtendSurface` not NULL), if any.
    pub extended_face: Option<FaceId>,
    /// `pOtherSideEdgeuse`.
    pub other_side_edgeuse: Option<EdgeuseId>,
}

fn pseudo_edge(edge: EdgeId) -> FilletObject {
    FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(edge))
}

impl FilletCorner {
    /// From the extension distance on: the end rib, its end face, the extended end
    /// surface and, at a mixed-convexity corner, the vertex on the original vertex and
    /// the two side edges joining it to the rail vertices; the gap checks of a
    /// multi-neighbour end face are returned for the caller.
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub(crate) fn finish_nx1_end(&mut self, env: Nx1End<'_>) -> Result<Option<Nx1GapCheck>> {
        let Nx1End {
            k,
            target,
            pseudo,
            corner,
            solver,
            extended_surfaces,
            topo_edges,
            geometry,
            filleted,
            vertex,
            total,
            mixed,
            same,
            tangent_side,
            sides,
            mut end_face,
            mut extend_edge,
            turned_back,
        } = env;

        // compute how much to extend the endSurface to make sure it covers the 'hole'
        let mut ext_dist = if mixed {
            find_surface_extension_dist_at_corner(target, filleted, solver, vertex)?
        } else {
            0.0
        };

        let start = self.vertices.first().copied();
        let end = self.vertices.get(1).copied();
        let rib = pseudo.make_fillet_edge(start, end, Some(corner))?;
        pseudo
            .fillet_edge_mut(rib)?
            .set_fillet_edge_type(FilletEdgeType::FilletXSideFace);
        let primary = need(
            eu1::primary_edgeuse(pseudo.brep(), rib)?,
            "end rib without primary edge use",
        )?;
        pseudo.set_edgeuse_fillet_geom(primary, Some(geometry))?;
        self.edges.push(rib);

        // In the mixed-cvxty case, pEndFace will be NULL when we arrive here
        // if both side edges have opposite convexity to the filleted edge.
        let mut multi_neighbor_end_face = false;
        let mut other_side_edgeuse = None;
        if end_face.is_none() {
            let [first, second] = sides;
            end_face = face_by_two_edgeuses(target, Some(first), Some(second), None)?;
            if end_face.is_none() {
                multi_neighbor_end_face = true;
                if total > 3 {
                    // Use the side edge that is a cliff edge.
                    let (pick, other) = if same[0] {
                        (first, second)
                    } else if same[1] {
                        (second, first)
                    } else if !face_holds_edge(k, target, first, second, self.approx_tol_3d)?
                        && face_holds_edge(k, target, second, first, self.approx_tol_3d)?
                    {
                        // Both are cliff edges. The cap lies on the end face's surface
                        // between the end rib and both side edges, so the end face is the
                        // one whose surface holds the other side edge (the gap check asks
                        // that of its extension). Upstream picks the first, which made an
                        // End beside two faces meeting tangentially (a bore's flat wall
                        // and its round bottom) depend on which of them came first round
                        // the vertex: the mirror image built and the part did not (#15768).
                        (second, first)
                    } else {
                        (first, second)
                    };
                    end_face = eu1::face(target, pick)?;
                    extend_edge = eu1::edge(target, pick)?;
                    other_side_edgeuse = Some(other);
                } else {
                    // Invalid brep topology
                    return Err(SolidError::blend_failed(
                        OP,
                        "the brep's topology at the corner is not valid",
                    ));
                }
            }
        }
        pseudo.fillet_edge_mut(rib)?.set_original_face(end_face);

        // compute how much to extend the sideSurface to make sure it covers the 'hole'
        let mut do_extend = extend_edge.is_some();
        if ext_dist > EFF_ZERO && !do_extend && matches!(solver, SolverObject::ConstantRadius(_)) {
            ext_dist *= 0.1; // use only a small extension
            do_extend = true; // for constant radius only
        }

        // When a surface extension is needed to define the endFace shape,
        // build and save it.
        let mut new_extend_face = None;
        if do_extend {
            let face = need(end_face, "extended end face is null")?;
            let surface = need(
                target.face(face)?.surface.as_ref(),
                "end face without surface",
            )?;
            // create an extended surface large enough to fill in the endCap requirements
            // At a tangent end the end face is continued as itself: the end rib's shape
            // there depends on how its curvature differs from the rail's face across the
            // seam, which a linear (G1) extension would not keep (tangency-labs/smlib@856b54e).
            let extension = if mixed && (tangent_side[0] || tangent_side[1]) {
                crate::surface::extend::Continuity::Natural
            } else {
                crate::surface::extend::Continuity::Tangent
            };
            // Keep a successful continuation's local chart. If its circular span has
            // insufficient natural reach, a certified cylinder supplies the same support
            // without continuing that span through its finite parameter limit.
            let new_surface = match surface.extended_sides(
                k,
                &crate::surface::side::BoxSide::ALL,
                extension,
                ext_dist,
            ) {
                Ok(extended) => extended.surface,
                Err(error)
                    if extension == crate::surface::extend::Continuity::Natural
                        && matches!(
                            error.root_cause(),
                            SolidError::ExtendRefused {
                                refusal: crate::surface::extend::ExtendRefusal::NotExact,
                                ..
                            }
                        ) =>
                {
                    let carrier = match surface.kind() {
                        crate::surface::surface::SurfaceKind::ProceduralBlend(round) => {
                            match crate::surface::procedural_blend::exact::cylinder_support(
                                k,
                                round,
                                self.approx_tol_3d,
                            ) {
                                Ok(carrier) => carrier,
                                Err(error) if error.is_unported_or_cancelled() => {
                                    return Err(error)
                                }
                                // A failed alternate certificate retains the original
                                // exact-extension refusal and its geometric limit.
                                Err(_) => None,
                            }
                        }
                        _ => None,
                    };
                    match carrier {
                        Some(carrier) => carrier,
                        None => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            };
            // Copy pNewSurface, when possible as an analytic surface
            let new_extend_surface = new_surface.copy_and_add_analytics(k)?;
            // If AddExtendedOriginalSurface() deleted pNewExtendSurface, then
            // pEndFace already had an extended surface, and we don't have to
            // deal with it anymore.
            if Self::add_extended_original_surface_with_map(
                extended_surfaces,
                face,
                new_extend_surface,
            ) {
                new_extend_face = Some(face);
            }
        }

        // When adding an endSurface to fill the 'hole',
        // set up the topology for the end cap.
        if mixed {
            // add a filletVertex at the origVertex location
            let fv = pseudo.brep_mut().add_unlinked_vertex_record(Vertex {
                point: Point3::ZERO,
                tolerance: 0.0,
                ..Vertex::default()
            })?;
            pseudo.insert_fillet_vertex(fv, FilletVertex::new(k, Some(corner)));
            pseudo
                .fillet_vertex_mut(fv)?
                .set_point_class(PointClassType::Vertex, Some(EntityRef::Vertex(vertex)));
            pseudo
                .fillet_vertex_mut(fv)?
                .set_fillet_vertex_type(FilletVertexType::OnVertex);
            self.vertices.push(fv);

            let v = |i: usize| -> Result<VertexId> {
                need(self.vertices.get(i).copied(), "corner vertex index")
            };
            let (v0, v1, v2) = (v(0)?, v(1)?, v(2)?);

            let rib_index = self.edges.len().saturating_sub(1);
            // Not upstream: a rail rolled onto a face whose foot edge turns back at the rolled-over
            // edge's vertex ends inside that face; the cap's edge from it runs along the foot
            // edge's continuation to that vertex, then along the side edge (`TurnedBack`)
            self.turned_back_arcs = [None, None];
            let edge2 = if let Some(tb) = turned_back[1] {
                let f = turned_back_vertex(k, pseudo, corner, tb)?;
                self.vertices.push(f);
                let arc = pseudo.make_fillet_edge(Some(v1), Some(f), Some(corner))?;
                let fe = pseudo.fillet_edge_mut(arc)?;
                fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                fe.set_original_edge(Some(tb.edge));
                fe.set_original_face(Some(tb.face));
                self.edges.push(arc);
                *need(self.turned_back_arcs.get_mut(1), "rail index")? = Some(arc);
                let line = pseudo.make_fillet_edge(Some(f), Some(v2), Some(corner))?;
                pseudo
                    .fillet_edge_mut(line)?
                    .set_fillet_edge_type(FilletEdgeType::OnEdge);
                if total <= 3 {
                    topo_edges.push(Some(pseudo_edge(line)));
                }
                let original = eu1::edge(target, sides[1])?;
                pseudo.fillet_edge_mut(line)?.set_original_edge(original);
                let face1 = eu1::face(target, sides[1])?;
                pseudo.fillet_edge_mut(line)?.set_original_face(face1);
                self.edges.push(line);
                line
            } else {
                // Add 1st fillet edge from expected rail2/sideEdge2 intersection to origVertex
                let edge2 = pseudo.make_fillet_edge(Some(v1), Some(v2), Some(corner))?;
                if pseudo.fillet_vertex(v1)?.fillet_vertex_type()
                    == FilletVertexType::RailXExtendedEdgeuse
                    || tangent_side[1]
                {
                    let fe = pseudo.fillet_edge_mut(edge2)?;
                    fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                    fe.set_original_edge(extend_edge);
                } else {
                    pseudo
                        .fillet_edge_mut(edge2)?
                        .set_fillet_edge_type(FilletEdgeType::OnEdge);
                    if total <= 3 {
                        // Register this edge and process it later
                        // when removing topological vertices
                        topo_edges.push(Some(pseudo_edge(edge2)));
                    }
                    let original = eu1::edge(target, sides[1])?;
                    pseudo.fillet_edge_mut(edge2)?.set_original_edge(original);
                }
                let face1 = eu1::face(target, sides[1])?;
                pseudo.fillet_edge_mut(edge2)?.set_original_face(face1);
                self.edges.push(edge2);
                edge2
            };

            let edge1 = if let Some(tb) = turned_back[0] {
                let f = turned_back_vertex(k, pseudo, corner, tb)?;
                self.vertices.push(f);
                let line = pseudo.make_fillet_edge(Some(v2), Some(f), Some(corner))?;
                pseudo
                    .fillet_edge_mut(line)?
                    .set_fillet_edge_type(FilletEdgeType::OnEdge);
                if total <= 3 {
                    topo_edges.push(Some(pseudo_edge(line)));
                }
                let original = eu1::edge(target, sides[0])?;
                pseudo.fillet_edge_mut(line)?.set_original_edge(original);
                let face0 = eu1::face(target, sides[0])?;
                pseudo.fillet_edge_mut(line)?.set_original_face(face0);
                self.edges.push(line);
                let arc = pseudo.make_fillet_edge(Some(f), Some(v0), Some(corner))?;
                let fe = pseudo.fillet_edge_mut(arc)?;
                fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                fe.set_original_edge(Some(tb.edge));
                fe.set_original_face(Some(tb.face));
                self.edges.push(arc);
                *need(self.turned_back_arcs.get_mut(0), "rail index")? = Some(arc);
                line
            } else {
                // Add 2nd fillet edge from origVertex to other expected rail1/sideEdge intersection
                let edge1 = pseudo.make_fillet_edge(Some(v2), Some(v0), Some(corner))?;
                if pseudo.fillet_vertex(v0)?.fillet_vertex_type()
                    == FilletVertexType::RailXExtendedEdgeuse
                    || tangent_side[0]
                {
                    let fe = pseudo.fillet_edge_mut(edge1)?;
                    fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                    fe.set_original_edge(extend_edge);
                } else {
                    pseudo
                        .fillet_edge_mut(edge1)?
                        .set_fillet_edge_type(FilletEdgeType::OnEdge);
                    if total <= 3 {
                        topo_edges.push(Some(pseudo_edge(edge1)));
                    }
                    let original = eu1::edge(target, sides[0])?;
                    pseudo.fillet_edge_mut(edge1)?.set_original_edge(original);
                }
                let face0 = eu1::face(target, sides[0])?;
                pseudo.fillet_edge_mut(edge1)?.set_original_face(face0);
                self.edges.push(edge1);
                edge1
            };
            // The cap's face is oriented at the corner's vertex from the edges before and after
            // it (`MakeFaceBrep` reads the second and third edges): with a turned-back rail 2,
            // the loop starts after the end rib.
            if turned_back[1].is_some() {
                // the edges from the end rib on, the rib moved to their end
                let rotated: Vec<EdgeId> = self
                    .edges
                    .iter()
                    .take(rib_index)
                    .chain(self.edges.iter().skip(rib_index.saturating_add(1)))
                    .chain(self.edges.get(rib_index))
                    .copied()
                    .collect();
                self.edges = rotated;
            }

            // When endFace connects to more than the filletSurface and 1 other Face,
            // check for gaps
            if multi_neighbor_end_face {
                return Ok(Some(Nx1GapCheck {
                    edge1,
                    edge2,
                    extended_face: new_extend_face,
                    other_side_edgeuse,
                }));
            }
        }
        Ok(None)
    }
}
