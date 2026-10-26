//! Borrowed offset context for the actual constant-radius rail solver.

use crate::blend::fillet_custom_solver::{
    ConstantRadiusAssistedFs, InitialValues, JacobianLoad, OffsetSurface,
};
use crate::blend::fillet_geom::p1::{FilletGeom, FilletGeomType};
use crate::blend::fillet_standard_solver::p1::constant_radius_load_jacobian;
use crate::blend::fillet_standard_solver::p2::{
    constant_radius_load_initial_values, ConstantRadiusOps,
};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::ExtentNd;
use crate::geom::extent2d::Extent2d;
use crate::geom::matrix::Matrix;
use crate::intersect::surface_intersector::p1::TsectPnt;
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::{Brep, EdgeuseId};

use crate::blend::fillet_standard_solver::p1::VariableRadiusFs;

use super::p1::{FilletSolver, FilletSolverOps, OffsetSurfaceSlot};

struct GeometrySolver<'a> {
    actual: &'a mut ConstantRadiusAssistedFs,
    geometry: &'a mut FilletGeom,
}

impl ConstantRadiusOps for GeometrySolver<'_> {
    fn constant_radius(&self) -> &ConstantRadiusAssistedFs {
        self.actual
    }

    fn constant_radius_mut(&mut self) -> &mut ConstantRadiusAssistedFs {
        self.actual
    }

    fn offset_surface(&self, rail: usize) -> Result<&OffsetSurface> {
        self.geometry
            .offset_surface(rail, Some(&self.actual.solver))?
            .ok_or_else(|| SolidError::null_pointer("blend::fillet_solver::surfaces"))
    }

    fn offset_surface_mut(&mut self, rail: usize) -> Result<&mut OffsetSurface> {
        let own = self.geometry.off_surfs.get_mut(rail).ok_or_else(|| {
            SolidError::invalid_input("blend::fillet_solver::surfaces", "rail index past 1")
        })?;
        if let Some(surface) = own {
            return Ok(surface);
        }
        self.actual.surface_mut(rail)
    }

    fn set_offset_by_radius(&mut self, rail: usize) -> Result<()> {
        let distance = self.actual.fillet_radius(rail)? * self.actual.solver.orientation(rail)?;
        ConstantRadiusOps::offset_surface_mut(self, rail)?.set_offset_distance(distance);
        Ok(())
    }

    fn check_tangent_rollover_guess_context(&self) -> Result<()> {
        if self.geometry.fillet_geom_type() == FilletGeomType::TangentRollover {
            // UNPORTED(blend.fillet_standard_solver_p2): SmConstantRadiusFS::LoadInitialValues tangent-rollover vertex-use context
            return Err(SolidError::unported(
                "SmConstantRadiusFS::LoadInitialValues tangent-rollover vertex-use context",
                "blend.fillet_standard_solver_p2",
            ));
        }
        Ok(())
    }

    fn setup_offset_extension(
        &self,
        k: &Kernel,
        surface: &Surface,
        domain: &mut Extent2d,
    ) -> Result<()> {
        FilletSolverOps::setup_offset_extension(self.actual, k, surface, domain)
    }
}

impl FilletSolverOps for GeometrySolver<'_> {
    fn solver(&self) -> &FilletSolver {
        &self.actual.solver
    }
    fn solver_mut(&mut self) -> &mut FilletSolver {
        &mut self.actual.solver
    }

    fn solver_surface(&self, rail: usize) -> Result<Option<&OffsetSurface>> {
        Ok(Some(ConstantRadiusOps::offset_surface(self, rail)?))
    }

    fn solver_surface_mut(&mut self, rail: usize) -> Result<Option<&mut OffsetSurface>> {
        Ok(Some(ConstantRadiusOps::offset_surface_mut(self, rail)?))
    }

    fn jacobian_size(&self) -> Result<usize> {
        FilletSolverOps::jacobian_size(self.actual)
    }

    fn setup_offset_extension(
        &self,
        k: &Kernel,
        surface: &Surface,
        domain: &mut Extent2d,
    ) -> Result<()> {
        FilletSolverOps::setup_offset_extension(self.actual, k, surface, domain)
    }

    fn load_initial_values(
        &mut self,
        k: &Kernel,
        brep: Option<&Brep>,
        do_surf2_calcs: bool,
        rail_index: usize,
        surface1: &mut OffsetSurface,
        surface1_index: usize,
        intervals: &mut ExtentNd,
        periodicities: &mut Vec<bool>,
        guess_t: &mut Vec<f64>,
    ) -> Result<Option<InitialValues>> {
        constant_radius_load_initial_values(
            self,
            k,
            brep,
            do_surf2_calcs,
            rail_index,
            surface1,
            surface1_index,
            intervals,
            periodicities,
            guess_t,
        )
    }

    fn load_jacobian(
        &mut self,
        k: &Kernel,
        num_equations: &mut usize,
        num_parameters: &mut usize,
        load: &mut JacobianLoad<'_>,
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        let mut offsets = [load.surf1_offset, load.surf2_offset];
        let result = constant_radius_load_jacobian(
            self,
            k,
            num_equations,
            num_parameters,
            load.rail_index,
            [load.surface1, load.surface2],
            &mut offsets,
            load.x,
            f,
            jacobian,
        );
        [load.surf1_offset, load.surf2_offset] = offsets;
        result
    }
}

/// Run with one actual solver and its selected geometry; no offset object moves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rail_edgeuse_intersect(
    actual: &mut ConstantRadiusAssistedFs,
    k: &Kernel,
    geometry: &mut FilletGeom,
    target: &mut Brep,
    rail_index: usize,
    edgeuse: EdgeuseId,
    guess: f64,
    tsect: &mut TsectPnt,
) -> Result<Option<f64>> {
    FilletSolver::rail_edgeuse_intersect(
        &mut GeometrySolver { actual, geometry },
        k,
        target,
        rail_index,
        edgeuse,
        guess,
        tsect,
    )
}

struct BaseGeometrySolver<'a> {
    solver: &'a mut FilletSolver,
    geometry: &'a mut FilletGeom,
}

impl FilletSolverOps for BaseGeometrySolver<'_> {
    fn solver(&self) -> &FilletSolver {
        self.solver
    }

    fn solver_mut(&mut self) -> &mut FilletSolver {
        self.solver
    }

    fn solver_surface(&self, rail: usize) -> Result<Option<&OffsetSurface>> {
        let own = self.geometry.off_surfs.get(rail).ok_or_else(|| {
            SolidError::invalid_input("blend::fillet_solver::surfaces", "rail index past 1")
        })?;
        if let Some(surface) = own {
            return Ok(Some(surface));
        }
        if self.solver.fillet_geom_for_surfaces().is_some()
            && self.geometry.fillet_solver().is_some()
        {
            return Err(SolidError::invalid_input(
                "blend::fillet_solver::surfaces",
                "geometry set for surfaces has no offset surface for rail (UB-204)",
            ));
        }
        Ok(self.solver.surface(rail)?.map(|slot| &slot.surface))
    }

    fn solver_surface_mut(&mut self, rail: usize) -> Result<Option<&mut OffsetSurface>> {
        let has_solver = self.geometry.fillet_solver().is_some();
        let own = self.geometry.off_surfs.get_mut(rail).ok_or_else(|| {
            SolidError::invalid_input("blend::fillet_solver::surfaces", "rail index past 1")
        })?;
        if let Some(surface) = own {
            return Ok(Some(surface));
        }
        if self.solver.fillet_geom_for_surfaces().is_some() && has_solver {
            return Err(SolidError::invalid_input(
                "blend::fillet_solver::surfaces",
                "geometry set for surfaces has no offset surface for rail (UB-204)",
            ));
        }
        Ok(self.solver.surface_mut(rail)?.map(|slot| &mut slot.surface))
    }
}

/// Run with a base [`FilletSolver`] and its selected geometry; no offset object moves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn base_rail_edgeuse_intersect(
    solver: &mut FilletSolver,
    k: &Kernel,
    geometry: &mut FilletGeom,
    target: &mut Brep,
    rail_index: usize,
    edgeuse: EdgeuseId,
    guess: f64,
    tsect: &mut TsectPnt,
) -> Result<Option<f64>> {
    FilletSolver::rail_edgeuse_intersect(
        &mut BaseGeometrySolver { solver, geometry },
        k,
        target,
        rail_index,
        edgeuse,
        guess,
        tsect,
    )
}

/// The transient keys of the geometry's offset surfaces while a variable-radius
/// solver holds them; they never leave [`variable_radius_rail_edgeuse_intersect`].
const SELECTED_KEYS: [u64; 2] = [u64::MAX - 1, u64::MAX];

/// `SmFilletGeom::RailEdgeuseIntersect`'s call into `SmFilletSolver::RailEdgeuseIntersect`
/// for a variable-radius solver while `geometry` is its geometry for surfaces: every
/// `GetSurface(rail)` then returns the geometry's offset surface. The variable-radius
/// solver reads its rails through its own slots, so the geometry's offset surfaces are
/// moved into those slots for the call and moved back afterwards (also on error),
/// with the solver's own surfaces and geometry for surfaces restored. A rail the
/// geometry has no offset surface for has no surface during the call (upstream's
/// `GetOffsetSurface` falls back to the solver's `GetSurface`, which asks the same
/// geometry again without end: UB-204).
#[allow(clippy::too_many_arguments)]
pub(crate) fn variable_radius_rail_edgeuse_intersect(
    actual: &mut VariableRadiusFs,
    k: &Kernel,
    geometry: &mut FilletGeom,
    target: &mut Brep,
    rail_index: usize,
    edgeuse: EdgeuseId,
    guess: f64,
    tsect: &mut TsectPnt,
) -> Result<Option<f64>> {
    let saved_geom = actual.curve_based.solver.fillet_geom_for_surfaces();
    let mut own: [Option<OffsetSurfaceSlot>; 2] = [None, None];
    for ((slot, kept), (selected, key)) in actual
        .curve_based
        .solver
        .surfaces
        .iter_mut()
        .zip(own.iter_mut())
        .zip(geometry.off_surfs.iter_mut().zip(SELECTED_KEYS))
    {
        let selected = selected
            .take()
            .map(|surface| OffsetSurfaceSlot { key, surface });
        *kept = std::mem::replace(slot, selected);
    }
    actual.curve_based.solver.set_fillet_geom_for_surfaces(None);
    let result =
        FilletSolver::rail_edgeuse_intersect(actual, k, target, rail_index, edgeuse, guess, tsect);
    actual
        .curve_based
        .solver
        .set_fillet_geom_for_surfaces(saved_geom);
    for ((slot, kept), selected) in actual
        .curve_based
        .solver
        .surfaces
        .iter_mut()
        .zip(own.iter_mut())
        .zip(geometry.off_surfs.iter_mut())
    {
        *selected = std::mem::replace(slot, kept.take()).map(|slot| slot.surface);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_custom_solver::SurfaceId;
    use crate::blend::fillet_geom::p1::{FilletBrep, FilletGeomId, FilletSolverId};
    use crate::math::Point3;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::offset_surface::SS_UNKNOWN;

    fn offset(k: &Kernel, distance: f64) -> OffsetSurface {
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 1.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(1.0, 0.0, 0.0, NOW),
                CPoint::new(1.0, 1.0, 0.0, NOW),
            ],
        ];
        let nurb = crate::nurbs::srf_basic::Surface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        OffsetSurface::new(
            k,
            distance,
            Surface::from(BSplineSurface::from_nurb(nurb, false)),
            SurfaceId::Free(8),
            SS_UNKNOWN,
        )
        .unwrap()
    }

    fn actual(k: &Kernel) -> ConstantRadiusAssistedFs {
        ConstantRadiusAssistedFs::from_parts(
            1e-5,
            1e-3,
            0.01,
            [0.5, 0.5],
            [Some(offset(k, 99.0)), Some(offset(k, 101.0))],
            [-1.0, -1.0],
            [None, None],
        )
    }

    fn geometry(k: &Kernel) -> FilletGeom {
        let mut geometry = FilletGeom::new(k, None, FilletGeomType::Default).unwrap();
        geometry.solver = Some(FilletSolverId(37));
        geometry.off_surfs = [Some(offset(k, 7.0)), Some(offset(k, 8.0))];
        geometry
    }

    #[test]
    fn selected_objects_and_nested_jacobian_writes_stay_in_actual_geometry() {
        let k = Kernel::default();
        let mut actual = actual(&k);
        actual
            .solver
            .set_fillet_geom_for_surfaces(Some(FilletGeomId(57)));
        let mut geometry = geometry(&k);
        let selected = std::ptr::from_ref(geometry.off_surfs[0].as_ref().unwrap());
        let mut view = GeometrySolver {
            actual: &mut actual,
            geometry: &mut geometry,
        };
        assert!(std::ptr::eq(
            view.solver_surface(0).unwrap().unwrap(),
            selected
        ));
        assert_eq!(
            view.solver_surface(0).unwrap().unwrap().offset_distance(),
            7.0
        );
        let mut load = JacobianLoad {
            rail_index: 0,
            surface1: None,
            surface2: None,
            surf1_offset: 0,
            surf2_offset: 0,
            x: &[0.25, 0.75, 0.25, 0.75],
        };
        let mut equations = 0;
        let mut parameters = 0;
        assert!(view
            .load_jacobian(
                &k,
                &mut equations,
                &mut parameters,
                &mut load,
                &mut [0.0; 3],
                None
            )
            .unwrap());
        assert_eq!(
            view.solver_surface(0).unwrap().unwrap().offset_distance(),
            -0.5
        );
        assert_eq!(
            view.solver_surface(1).unwrap().unwrap().offset_distance(),
            -0.5
        );
        assert_eq!(
            view.actual.solver.surfaces[0]
                .as_ref()
                .unwrap()
                .surface
                .offset_distance(),
            99.0
        );
        assert_eq!(
            view.actual.solver.surfaces[1]
                .as_ref()
                .unwrap()
                .surface
                .offset_distance(),
            101.0
        );
        assert_eq!(
            view.actual.solver.fillet_geom_for_surfaces(),
            Some(FilletGeomId(57))
        );
        assert_eq!(equations, 3);
        assert_eq!(parameters, 4);
    }

    #[test]
    fn actual_geometry_context_is_restored_on_target_lookup_failure() {
        let k = Kernel::default();
        let mut actual = actual(&k);
        let saved = Some(FilletGeomId(57));
        actual.solver.set_fillet_geom_for_surfaces(saved);
        let mut pseudo = FilletBrep::new();
        let g = pseudo.add_geom(geometry(&k));
        let error = pseudo
            .rail_edgeuse_intersect_with_geometry(
                &k,
                g,
                &mut actual,
                &mut Brep::new(),
                0,
                EdgeuseId::from_index(0),
                0.5,
                &mut TsectPnt::default(),
            )
            .unwrap_err();
        assert!(!error.is_unported(), "{error:?}");
        assert_eq!(actual.solver.fillet_geom_for_surfaces(), saved);
        assert_eq!(
            actual.solver.surfaces[0]
                .as_ref()
                .unwrap()
                .surface
                .offset_distance(),
            99.0
        );
        assert_eq!(
            pseudo.geom(g).unwrap().off_surfs[0]
                .as_ref()
                .unwrap()
                .offset_distance(),
            7.0
        );
        assert_eq!(
            pseudo.geom_mut(g).unwrap().off_surfs[0]
                .as_mut()
                .unwrap()
                .surface
                .evaluate_point_extending(&k, crate::geom::vector2d::Point2::new(0.25, 0.75))
                .unwrap(),
            Point3::new(0.25, 0.75, 7.0)
        );
    }

    #[test]
    fn base_solver_geometry_context_resolves_offsets_and_guards_ub_204() {
        let k = Kernel::default();
        let mut solver = FilletSolver::new(1e-5, 1e-3, 0.01);
        let saved = Some(FilletGeomId(91));
        solver.set_fillet_geom_for_surfaces(saved);

        let mut pseudo = FilletBrep::new();
        let g = pseudo.add_geom(geometry(&k));
        let err = pseudo
            .rail_edgeuse_intersect_with_geometry(
                &k,
                g,
                &mut solver,
                &mut Brep::new(),
                0,
                EdgeuseId::from_index(0),
                0.5,
                &mut TsectPnt::default(),
            )
            .unwrap_err();
        assert!(!err.is_unported(), "{err:?}");
        assert_eq!(solver.fillet_geom_for_surfaces(), saved);

        pseudo.geom_mut(g).unwrap().off_surfs[1] = None;
        let ub = pseudo
            .rail_edgeuse_intersect_with_geometry(
                &k,
                g,
                &mut solver,
                &mut Brep::new(),
                0,
                EdgeuseId::from_index(0),
                0.5,
                &mut TsectPnt::default(),
            )
            .unwrap_err();
        assert!(matches!(ub, SolidError::InvalidInput { .. }), "{ub:?}");
        assert_eq!(solver.fillet_geom_for_surfaces(), saved);
    }
}
