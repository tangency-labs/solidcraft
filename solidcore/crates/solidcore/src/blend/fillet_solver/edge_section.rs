//! The rolling ball's section at a point of a fillet solver's edge: the ball touching both
//! of the solver's faces in the plane normal to the edge there, solved from the edge point,
//! so it is the ball that rolls along this edge (the start of a rolling-ball march; Choi and
//! Ju 1989). A rail's end on a side edge is searched from where this section touches it.
//!
//! Not upstream: SMLib starts that search from a fraction of the radius along the edge.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::p1::FilletSolverOps;
use crate::curve::curve::Curve;
use crate::error::Result;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::intersect::surface_intersector::p1::TsectPnt;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::Brep;

/// The rolling ball's section in the plane through the point of `curve` (the solver's
/// filleted edge) at `t`, normal to the edge there: the ball touching both of the solver's
/// faces, solved from the edge point on each face (where a ball of radius 0 touches them), so
/// the solve starts on the edge and finds the ball that rolls along it. `None` where the edge
/// does not move at `t`, the point does not drop onto a face within `domains`, or the solve
/// does not converge. `brep` is the target holding the solver's edge uses.
///
/// # Errors
///
/// Those of the evaluation, the drops and the section solve.
pub(crate) fn edge_section(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    brep: &mut Brep,
    curve: &Curve,
    t: f64,
    domains: &[Extent2d; 2],
) -> Result<Option<TsectPnt>> {
    let pv = curve.evaluate(k, t, 1, true, false)?;
    let (Some(&point), Some(&tangent)) = (pv.first(), pv.get(1)) else {
        return Ok(None);
    };
    let moves = tangent.length() > EFF_ZERO;
    if !moves {
        return Ok(None);
    }
    let normal = tangent.unitize()?;
    // the edge point on each face's surface: the section's guess
    let mut guess = [Point2::UNDEF; 2];
    for ((rail, domain), g) in domains.iter().enumerate().zip(guess.iter_mut()) {
        let Some(slot) = ops.solver().surface(rail)? else {
            return Ok(None);
        };
        let drop = slot.surface.base_surface().drop_point::<NoObject>(
            k,
            point,
            domain,
            &DropPointOptions::default(),
        )?;
        if !drop.success {
            return Ok(None);
        }
        *g = drop.result_uv;
    }
    let [g0, g1] = guess;
    let mut section = TsectPnt::new();
    let found = ops.point_on_plane_solve(
        k,
        Some(brep),
        point,
        normal,
        &domains[0],
        &domains[1],
        g0,
        g1,
        &mut section,
    )?;
    Ok(found.then_some(section))
}
