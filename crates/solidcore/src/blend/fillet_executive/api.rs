//! Public fillet entry points: fillet B-rep edges through one live fillet session.
//!
//! Upstream: `source/SM_API/src/SmApiBrep.cpp` (`SmApiFilletEdges` 2049-2087,
//! `SmApiFilletEdgesPerEdge` 2094-2135).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`fillet_edges`] | `SmApiFilletEdges` |
//! | [`fillet_edges_per_edge`] | `SmApiFilletEdgesPerEdge` |
//!
//! Deviations: the kernel argument replaces `SmApiGetOrCreateContext()`. Edges are
//! typed ids of `brep`. Upstream destroys its executive (and the executive's
//! fillet-edge attribute) after `StitchAndOrient`; here the session, which holds the
//! only borrow of `brep`, is dropped first. Attributes carry no geometry, so the
//! stitch and orientation see the same topology and geometry. A body whose corners
//! need an unported corner kind returns `Unported` from the session. No recursion.

use super::session::FilletSession;
use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use crate::error::{Result, SolidError, SM_ERR_INVALID_INPUT};
use crate::kernel::Kernel;
use crate::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use crate::topology::{Brep, EdgeId};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::topology::brep::p1;

    fn boxed(k: &mut Kernel) -> Brep {
        crate::primitive::api_primitives::create_box(k, Vec3::ZERO, 10.0, 20.0, 30.0).unwrap()
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
