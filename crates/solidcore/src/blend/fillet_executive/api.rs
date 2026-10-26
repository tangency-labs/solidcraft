//! Public fillet entry points: fillet B-rep edges through one live fillet session.
//!
//! Upstream: `source/SM_API/src/SmApiBrep.cpp` (`SmApiFilletEdges` 2049-2087,
//! `SmApiFilletEdgesPerEdge` 2094-2135, `SmApiVariableRadiusFillet` 2142-2178,
//! `SmApiSetBevelCorners` 2305-2338),
//! `source/SM_API/src/SmApiFillets.cpp` (`SmApiChamferFillet` 40-113,
//! `SmApiCircularFillet` 119-185).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`fillet_edges`] | `SmApiFilletEdges` |
//! | [`fillet_edges_per_edge`] | `SmApiFilletEdgesPerEdge` |
//! | [`variable_radius_fillet`] | `SmApiVariableRadiusFillet` |
//! | [`circular_fillet`] | `SmApiCircularFillet` |
//! | [`chamfer_fillet`] | `SmApiChamferFillet` |
//! | [`set_bevel_corners`] | `SmApiSetBevelCorners` |
//!
//! Deviations: the kernel argument replaces `SmApiGetOrCreateContext()`. Edges are
//! typed ids of `brep`. Upstream destroys its executive (and the executive's
//! fillet-edge attribute) after `StitchAndOrient`; here the session, which holds the
//! only borrow of `brep`, is dropped first. Attributes carry no geometry, so the
//! stitch and orientation see the same topology and geometry. A body whose corners
//! need an unported corner kind returns `Unported` from the session.
//! [`circular_fillet`] and [`chamfer_fillet`] ignore `LoadFilletSolver`'s status
//! as upstream does, and skip an edge whose solver constructor fails (upstream's
//! constructor only signals the error and leaves a solver without surfaces, which
//! `LoadFilletSolver` then refuses); `Unported` still propagates. The stack-held
//! generator and handler become session-retained values. No recursion.

use super::session::FilletSession;
use crate::blend::fillet_geom::p1::FilletSolverId;
use crate::blend::fillet_solver::p1::{
    FilletSurfaceGenerator, FilletSurfaceGeneratorType, MakeSurfaceBlendSih,
};
use crate::blend::fillet_standard_solver::p1::FilletLaw;
use crate::error::{Result, SolidError, SM_ERR_INVALID_INPUT};
use crate::kernel::Kernel;
use crate::math::PI;
use crate::topology::brep::p1;
use crate::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use crate::topology::{Brep, EdgeId, VertexId};

const OP: &str = "blend::fillet_executive::api";

fn invalid_input() -> SolidError {
    SolidError::Upstream {
        operation: OP,
        code: SM_ERR_INVALID_INPUT,
        entity: None,
    }
}

/// `SmApiFilletEdges`: fillet `edges` of `brep` with one constant `radius` and
/// cross-section (`xsect_type` 0 linear, 2 blend curve, any other value circular),
/// then `StitchAndOrient` with its defaults. `continuity` (1, 2 or 3 for G1-G3) and
/// `thumbweight` apply to blend-curve cross-sections.
///
/// # Errors
/// `SM_ERR_INVALID_INPUT` for an empty edge list; the fillet executive's errors
/// (the body keeps the edits made before the failure, as upstream).
pub fn fillet_edges(
    k: &mut Kernel,
    brep: &mut Brep,
    edges: &[EdgeId],
    radius: f64,
    xsect_type: u32,
    continuity: u32,
    thumbweight: f64,
) -> Result<()> {
    if edges.is_empty() {
        return Err(invalid_input());
    }
    {
        let mut session = FilletSession::new(k, brep)?;
        session.prepare_constant_radius_edges(
            k,
            edges,
            radius,
            xsect_type,
            continuity,
            thumbweight,
        )?;
        session.do_fillet(k)?;
    }
    stitch_and_orient(k, brep, &StitchAndOrientOptions::default())
}

/// `SmApiFilletEdgesPerEdge`: fillet `edges` of `brep` with a constant radius and
/// cross-section type per edge, then `StitchAndOrient` with its defaults.
///
/// # Errors
/// `SM_ERR_INVALID_INPUT` for an empty edge list or when `radii` or `xsect_types`
/// does not hold one entry per edge; the fillet executive's errors.
pub fn fillet_edges_per_edge(
    k: &mut Kernel,
    brep: &mut Brep,
    edges: &[EdgeId],
    radii: &[f64],
    xsect_types: &[FilletSurfaceGeneratorType],
    continuity: u32,
    thumbweight: f64,
) -> Result<()> {
    if edges.is_empty() {
        return Err(invalid_input());
    }
    if radii.len() != edges.len() || xsect_types.len() != edges.len() {
        return Err(invalid_input());
    }
    {
        let mut session = FilletSession::new(k, brep)?;
        session.prepare_per_edge_constant_radius(
            k,
            edges,
            radii,
            xsect_types,
            continuity,
            thumbweight,
        )?;
        session.do_fillet(k)?;
    }
    stitch_and_orient(k, brep, &StitchAndOrientOptions::default())
}

/// `SmApiVariableRadiusFillet`: fillet `edges` of `brep` with a radius running
/// linearly from `start_radius` to `end_radius` along each edge
/// (`SmLinearFilletLaw`) and one cross-section (`xsect_type` 0 linear, 2 blend
/// curve, any other value circular), then `StitchAndOrient` with its defaults.
/// `continuity` (1, 2 or 3 for G1-G3) applies to blend-curve cross-sections, whose
/// thumbweight is the `SetFilletParameters` default `1.0`.
///
/// # Errors
/// `SM_ERR_INVALID_INPUT` for an empty edge list; the fillet executive's errors
/// (the body keeps the edits made before the failure, as upstream).
pub fn variable_radius_fillet(
    k: &mut Kernel,
    brep: &mut Brep,
    edges: &[EdgeId],
    start_radius: f64,
    end_radius: f64,
    xsect_type: u32,
    continuity: u32,
) -> Result<()> {
    if edges.is_empty() {
        return Err(invalid_input());
    }
    let law = FilletLaw::linear(start_radius, end_radius, None);
    {
        let mut session = FilletSession::new(k, brep)?;
        session.prepare_variable_radius_edges(
            k,
            edges,
            start_radius,
            law,
            xsect_type,
            continuity,
        )?;
        session.do_fillet(k)?;
    }
    stitch_and_orient(k, brep, &StitchAndOrientOptions::default())
}

/// Keep a value of an upstream call whose status is ignored; `Unported` propagates.
fn ignored<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_unported() => Err(error),
        Err(_) => Ok(None),
    }
}

/// The edge solver kind the all-edge wrappers construct.
#[derive(Clone, Copy)]
enum AllEdgeSolver {
    /// `SmConstantRadiusFS(.., dRadius, pEU)`.
    ConstantRadius,
    /// `SmConstantDistanceFS(.., dDistance, pEU)`.
    ConstantDistance,
}

/// The shared body of `SmApiCircularFillet` and `SmApiChamferFillet`: an
/// `SmMakeSurfaceBlendSIH(1.0)` handler, global merge, one solver per edge with a
/// blend edge use (in `GetEdges` order) with `generator`, then
/// `CreateFilletCorners`, every corner a bevel when `bevel_corners`, and
/// `DoFilleting`.
fn fillet_all_edges(
    k: &mut Kernel,
    session: &mut FilletSession<'_>,
    solver_kind: AllEdgeSolver,
    value: f64,
    generator: FilletSurfaceGenerator,
    bevel_corners: bool,
) -> Result<()> {
    let tol = session.target().tolerance();
    let generator = session.add_surface_generator(generator)?;

    // Define self-intersection handler for edge fillets
    let step_back_factor = 1.0;
    session.set_self_intersection_handler(Some(MakeSurfaceBlendSih::new(step_back_factor)));
    session.set_do_global_merge(true);

    for edge in p1::edges(session.target(), None)? {
        let Some(edgeuse) = session.blend_edgeuse(k, edge)? else {
            continue;
        };
        let ang_tol = 30.0 * PI / 180.0;
        let tan_tol = 2.0 * PI / 180.0;
        let solver: Option<FilletSolverId> = ignored(match solver_kind {
            AllEdgeSolver::ConstantRadius => {
                session.new_constant_radius_solver(k, edgeuse, value, tol, ang_tol, tan_tol)
            }
            AllEdgeSolver::ConstantDistance => {
                session.new_constant_distance_solver(k, edgeuse, value, tol, ang_tol, tan_tol)
            }
        })?;
        let Some(solver) = solver else {
            continue;
        };
        session.set_solver_generator(solver, generator)?;
        ignored(session.load_solver(k, solver))?;
    }

    session.create_corners(k, None)?;
    if bevel_corners {
        for corner in session.fillet_corners()? {
            session.set_corner_bevel(corner, true)?;
        }
    }
    session.do_filleting(k, None)
}

/// `SmApiCircularFillet`: fillet every edge of `brep` that bounds a blendable
/// sector with a circular cross-section of `radius` (exact arcs,
/// `SmCircularCrossSectionFSG(FALSE, 1.0e-3)`), then `StitchAndOrient` with its
/// defaults.
///
/// # Errors
/// The fillet executive's errors (the body keeps the edits made before the
/// failure, as upstream).
pub fn circular_fillet(k: &mut Kernel, brep: &mut Brep, radius: f64) -> Result<()> {
    {
        let mut session = FilletSession::new(k, brep)?;
        fillet_all_edges(
            k,
            &mut session,
            AllEdgeSolver::ConstantRadius,
            radius,
            FilletSurfaceGenerator::circular(false, 1.0e-3, false, false),
            false,
        )?;
    }
    // This operation requires stitching for some reason
    stitch_and_orient(k, brep, &StitchAndOrientOptions::default())
}

/// `SmApiChamferFillet`: chamfer every edge of `brep` that bounds a blendable
/// sector with a linear cross-section of chord length `distance`
/// (`SmConstantDistanceFS`), every corner a bevel. Unlike the other entry points
/// there is no `StitchAndOrient`.
///
/// # Errors
/// The fillet executive's errors (the body keeps the edits made before the
/// failure, as upstream).
pub fn chamfer_fillet(k: &mut Kernel, brep: &mut Brep, distance: f64) -> Result<()> {
    let mut session = FilletSession::new(k, brep)?;
    fillet_all_edges(
        k,
        &mut session,
        AllEdgeSolver::ConstantDistance,
        distance,
        FilletSurfaceGenerator::Linear,
        true,
    )
}

/// `SmApiSetBevelCorners`: fillet `edges` of `brep` with a constant circular
/// `radius` (`SetFilletParameters` with its default continuity 1 and thumbweight
/// 1.0), make the corners at `bevel_vertices` bevels, then `DoFilleting` and
/// `StitchAndOrient` with its defaults.
///
/// # Errors
/// `SM_ERR_INVALID_INPUT` for an empty edge list; `SM_ERR_NULL_POINTER` for a bevel
/// vertex without a corner entry; the fillet executive's errors.
pub fn set_bevel_corners(
    k: &mut Kernel,
    brep: &mut Brep,
    edges: &[EdgeId],
    radius: f64,
    bevel_vertices: &[VertexId],
) -> Result<()> {
    if edges.is_empty() {
        return Err(invalid_input());
    }
    {
        let mut session = FilletSession::new(k, brep)?;
        // SetFilletParameters(.., SM_FSG_CIRCULAR, SM_FS_CONST_RADIUS, dRadius, NULL,
        // -1, -1, -1, -1) with lContinuity = 1, dThumbweight = 1.0 defaulted
        session.prepare_constant_radius_edges(k, edges, radius, 1, 1, 1.0)?;
        session.create_corners(k, None)?;
        for vertex in bevel_vertices {
            session.set_bevel_corner(*vertex)?;
        }
        session.do_filleting(k, None)?;
    }
    stitch_and_orient(k, brep, &StitchAndOrientOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::topology::brep::p1;

    fn boxed(k: &mut Kernel) -> Brep {
        crate::primitive::api_primitives::create_box(k, Vec3::ZERO, 10.0, 20.0, 30.0).unwrap()
    }

    #[test]
    fn all_edge_setup_loads_one_solver_per_edge_with_the_blending_handler() {
        for solver_kind in [
            AllEdgeSolver::ConstantRadius,
            AllEdgeSolver::ConstantDistance,
        ] {
            let mut k = Kernel::default();
            let mut body = boxed(&mut k);
            let mut session = FilletSession::new(&mut k, &mut body).unwrap();
            let generator = session
                .add_surface_generator(FilletSurfaceGenerator::Linear)
                .unwrap();
            session.set_self_intersection_handler(Some(MakeSurfaceBlendSih::new(1.0)));
            let edges = p1::edges(session.target(), None).unwrap();
            for (i, edge) in edges.iter().enumerate() {
                let edgeuse = session.blend_edgeuse(&k, *edge).unwrap().unwrap();
                let solver = match solver_kind {
                    AllEdgeSolver::ConstantRadius => session
                        .new_constant_radius_solver(&k, edgeuse, 0.5, 1e-6, 0.5, 0.03)
                        .unwrap(),
                    AllEdgeSolver::ConstantDistance => session
                        .new_constant_distance_solver(&k, edgeuse, 0.5, 1e-6, 0.5, 0.03)
                        .unwrap(),
                };
                assert_eq!(solver.0 as usize, i);
                session.set_solver_generator(solver, generator).unwrap();
                session.load_solver(&k, solver).unwrap();
                let base = session.solver(solver).unwrap().base();
                assert_eq!(
                    base.self_intersection_handler().unwrap(),
                    Some(crate::blend::fillet_intersector::HandlingAlgorithmType::Blending)
                );
            }
            // clearing the executive's handler is seen by every loaded solver
            session.set_self_intersection_handler(None);
            for i in 0..edges.len() {
                let id = FilletSolverId(u32::try_from(i).unwrap());
                let base = session.solver(id).unwrap().base();
                assert_eq!(base.self_intersection_handler().unwrap(), None);
            }
            session.create_corners(&k, None).unwrap();
            let corners = session.fillet_corners().unwrap();
            assert_eq!(corners.len(), 8);
            for corner in &corners {
                session.set_corner_bevel(*corner, true).unwrap();
                assert!(session.corner(*corner).unwrap().is_bevel_corner());
            }
        }
    }

    #[test]
    fn bevel_corner_of_a_vertex_and_of_a_vertex_without_entry() {
        let mut k = Kernel::default();
        let mut body = boxed(&mut k);
        let edges = p1::edges(&body, None).unwrap();
        let vertices = p1::vertices(&body, None).unwrap();
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges[..1], 0.5, 1, 1, 1.0)
            .unwrap();
        session.create_corners(&k, None).unwrap();
        let mut beveled = 0;
        for vertex in &vertices {
            match session.set_bevel_corner(*vertex) {
                Ok(()) => beveled += 1,
                Err(error) => assert!(
                    matches!(error, SolidError::Upstream { code, .. } if code == crate::error::SM_ERR_NULL_POINTER)
                ),
            }
        }
        // one edge: corners at its two end vertices only
        assert_eq!(beveled, 2);
        for corner in session.fillet_corners().unwrap() {
            assert!(session.corner(corner).unwrap().is_bevel_corner());
        }
    }

    #[test]
    fn bevel_corners_without_edges_is_invalid_input() {
        let mut k = Kernel::default();
        let mut body = boxed(&mut k);
        let r = set_bevel_corners(&mut k, &mut body, &[], 1.0, &[]);
        assert!(
            matches!(r, Err(SolidError::Upstream { code, .. }) if code == SM_ERR_INVALID_INPUT)
        );
        assert_eq!(p1::faces(&body, None).unwrap().len(), 6);
    }

    #[test]
    fn single_box_edge_fillet_is_a_closed_manifold_solid() {
        let mut k = Kernel::default();
        let mut body = boxed(&mut k);
        let edge = p1::edges(&body, None).unwrap()[0];
        fillet_edges(&mut k, &mut body, &[edge], 0.5, 1, 1, 1.0).unwrap();
        assert_eq!(p1::faces(&body, None).unwrap().len(), 7);
        for edge in p1::edges(&body, None).unwrap() {
            assert!(crate::topology::edge::is_manifold(&body, edge).unwrap());
        }
        assert!(!k.context.doing_boolean());
    }

    #[test]
    fn per_edge_fillet_matches_the_shared_parameter_fillet() {
        let mut k = Kernel::default();
        let mut shared = boxed(&mut k);
        let edge = p1::edges(&shared, None).unwrap()[0];
        fillet_edges(&mut k, &mut shared, &[edge], 0.5, 1, 1, 1.0).unwrap();
        let mut k = Kernel::default();
        let mut per_edge = boxed(&mut k);
        fillet_edges_per_edge(
            &mut k,
            &mut per_edge,
            &[edge],
            &[0.5],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        assert_eq!(
            p1::edges(&per_edge, None).unwrap().len(),
            p1::edges(&shared, None).unwrap().len()
        );
        assert_eq!(
            p1::vertices(&per_edge, None).unwrap().len(),
            p1::vertices(&shared, None).unwrap().len()
        );
    }

    #[test]
    fn empty_or_mismatched_lists_are_invalid_input_without_edits() {
        let mut k = Kernel::default();
        let mut body = boxed(&mut k);
        let edges = p1::edges(&body, None).unwrap();
        let invalid = |r: Result<()>| matches!(r, Err(SolidError::Upstream { code, .. }) if code == SM_ERR_INVALID_INPUT);
        assert!(invalid(fillet_edges(
            &mut k,
            &mut body,
            &[],
            0.5,
            1,
            1,
            1.0
        )));
        assert!(invalid(fillet_edges_per_edge(
            &mut k,
            &mut body,
            &edges[..2],
            &[0.5],
            &[FilletSurfaceGeneratorType::Circular; 2],
            1,
            1.0,
        )));
        assert_eq!(p1::faces(&body, None).unwrap().len(), 6);
    }
}
