//! Native comparison fixtures for actual executive solver loading.

use super::{
    box_access, constant_radius_from_edgeuse, edge, error_output, executive, ffi, flag, harness,
    missing, missing_error, status_of, Access, BoxIn, ConstantRadiusAssistedFs, Deserialize,
    EdgeId, Extent1d, FilletExecutive, FilletSolverId, Handle, HarnessError, Kernel, Op, Output,
    Point3, Serialize, SolidResult,
};
use solidcore::blend::fillet_executive::p3::VertexEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LoaderSolverIn {
    edge: usize,
    mask: usize,
    #[serde(default)]
    radial: bool,
    #[serde(default)]
    geometry_surfaces: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LoaderScript {
    scale: f64,
    #[serde(default)]
    g1: bool,
    solvers: Vec<LoaderSolverIn>,
    loads: Vec<[usize; 2]>,
    #[serde(default)]
    drop_pending_after: Option<usize>,
}

fn loader_record(i: &LoaderScript) -> Vec<f64> {
    let mut out = vec![i.scale, flag(i.g1), i.solvers.len() as f64];
    for solver in &i.solvers {
        out.extend([
            solver.edge as f64,
            solver.mask as f64,
            flag(solver.radial),
            flag(solver.geometry_surfaces),
        ]);
    }
    out.push(i.loads.len() as f64);
    for &[executive, solver] in &i.loads {
        out.extend([executive as f64, solver as f64]);
    }
    out.push(i.drop_pending_after.map_or(0, |index| index + 1) as f64);
    out
}

pub(crate) fn loader_cpp(i: &LoaderScript) -> Result<Vec<f64>, solidcore_ffi::FfiError> {
    ffi::eval(Op::LoadSolver, &loader_record(i))
}

fn loader_edgeuse_label(access: &Access, handle: Handle) -> SolidResult<f64> {
    let Handle::Edgeuse(target) = handle else {
        return missing();
    };
    for (index, (_, edge)) in access
        .breps
        .first()
        .ok_or_else(missing_error)?
        .edges()
        .iter()
        .enumerate()
    {
        if let Some(at) = edge.edgeuses.iter().position(|id| *id == target) {
            return Ok(1.0 + 4.0 * index as f64 + at as f64);
        }
    }
    missing()
}

fn loader_snapshot(
    access: &Access,
    executive: &FilletExecutive<Handle>,
    out: &mut Vec<f64>,
) -> SolidResult<()> {
    out.push(executive.solvers.len() as f64);
    for &solver in &executive.solvers {
        let Handle::Solver(index) = solver else {
            return missing();
        };
        out.push(index as f64 + 1.0);
    }
    out.push(executive.filleted_vertices.len() as f64);
    for vertex in &executive.filleted_vertices {
        out.push(flag(vertex.is_some()));
        if let Some(Handle::Vertex(id)) = vertex {
            let p = access
                .breps
                .first()
                .ok_or_else(missing_error)?
                .vertex(*id)?
                .point;
            out.extend([p.x, p.y, p.z]);
        }
        let pending = match executive.vertex_corner_map.get(vertex) {
            Some(VertexEntry::SolverEdgeuses(pending)) => Some(pending),
            None => None,
            Some(VertexEntry::Corner(_)) => return missing(),
        };
        out.push(flag(pending.is_some()));
        if let Some(pending) = pending {
            out.push(pending.len() as f64);
            for &edgeuse in pending {
                out.push(loader_edgeuse_label(access, edgeuse)?);
            }
        }
    }
    for (_, edge) in access
        .breps
        .first()
        .ok_or_else(missing_error)?
        .edges()
        .iter()
    {
        for edgeuse in &edge.edgeuses {
            out.push(
                match executive.fillet_solver_of_edgeuse(Handle::Edgeuse(*edgeuse)) {
                    Some(Handle::Solver(index)) => index as f64 + 1.0,
                    None => 0.0,
                    _ => return missing(),
                },
            );
        }
    }
    out.push(flag(executive.pseudo_brep.is_some()));
    Ok(())
}

pub(crate) fn loader_rust(i: &LoaderScript) -> Result<Output, HarnessError> {
    ffi::validate(Op::LoadSolver, &loader_record(i)).map_err(|e| harness(e.to_string()))?;
    let bx = BoxIn {
        origin: [0.0; 3],
        size: [i.scale, 2.0 * i.scale, 3.0 * i.scale],
    };
    let mut access = match box_access(&bx)? {
        Ok(access) => access,
        Err(out) => return Ok(out),
    };
    let mut k = Kernel::default();
    if i.g1 {
        use super::super::blend_fillet_standard_solver_p1::{bcurve_of, InlineCrv};
        let b = access.breps.first_mut().ok_or_else(|| harness("no box"))?;
        // The G1 path evaluates owned surfaces generically: supply their actual
        // retained face tolerance rather than treating those surfaces as free.
        let faces = b
            .faces()
            .iter()
            .map(|(id, face)| (id, face.tolerance))
            .collect::<Vec<_>>();
        for (id, tolerance) in faces {
            if let Some(surface) = b
                .face_mut(id)
                .map_err(|e| harness(e.to_string()))?
                .surface
                .as_mut()
            {
                surface.set_owner_zone_tol_3d(Some(tolerance));
            }
        }
        let edges = b.edges().iter().map(|(id, _)| id).collect::<Vec<_>>();
        let mut notifications = solidcore::topology::brep::p2::ArenaNotifyState::new(b);
        for id in edges {
            let edge = b.edge(id).map_err(|e| harness(e.to_string()))?;
            let domain = edge.interval;
            let original = edge
                .curve
                .as_ref()
                .ok_or_else(|| harness("edge without curve"))?;
            let a = original
                .evaluate_point(&k, domain.min())
                .map_err(|e| harness(e.to_string()))?;
            let z = original
                .evaluate_point(&k, domain.max())
                .map_err(|e| harness(e.to_string()))?;
            let middle = a + (z - a) * 0.25;
            let owner = original.owner();
            let c = InlineCrv {
                p: 1,
                cpts: [a, middle, z]
                    .iter()
                    .map(|p| [p.x, p.y, p.z, solidcore::nurbs::math::NOW])
                    .collect(),
                u: vec![
                    domain.min(),
                    domain.min(),
                    (domain.min() + domain.max()) * 0.5,
                    domain.max(),
                    domain.max(),
                ],
            };
            let curve = solidcore::curve::curve::Curve::from(bcurve_of(&c)?);
            let interval = curve.natural_interval();
            // Match Replace3DCurve: notify the change (including deleting old UV
            // trims), replace the actual curve, then restore its edge back pointer.
            edge::set_curve(
                b,
                id,
                Some(curve),
                edge::SetCurveOptions {
                    delete_old_curve: true,
                    ..edge::SetCurveOptions::default()
                },
                &mut notifications.env(None),
                [None, None],
            )
            .map_err(|e| harness(e.to_string()))?;
            let edge = b.edge_mut(id).map_err(|e| harness(e.to_string()))?;
            edge.interval = interval;
            let _ = edge
                .curve
                .as_mut()
                .ok_or_else(|| harness("replacement curve missing"))?
                .set_owner(owner, None);
        }
    }
    let mut closed_edge = None;
    for (index, spec) in i.solvers.iter().enumerate() {
        let id = FilletSolverId(index as u32);
        let result = if spec.edge < 12 {
            let b = access.breps.first_mut().ok_or_else(|| harness("no box"))?;
            let edge = EdgeId::from_index(spec.edge as u32);
            let mut eu = edge::blend_edgeuse(&k, b, edge)
                .map_err(|e| harness(format!("loader blend edgeuse: {e}")))?
                .ok_or_else(|| harness("no blend edgeuse"))?;
            if spec.radial {
                eu = solidcore::topology::edgeuse::p1::radial(b, eu)
                    .map_err(|e| harness(e.to_string()))?
                    .ok_or_else(|| harness("no radial edgeuse"))?;
            }
            constant_radius_from_edgeuse(
                &k,
                b,
                &mut access.pseudo,
                id,
                1e-6,
                0.1,
                0.01,
                0.125 * i.scale,
                eu,
                None,
            )
        } else {
            use solidcore::blend::fillet_custom_solver::SurfaceId;
            use solidcore::math::Vec3;
            use solidcore::surface::plane::Plane;
            use solidcore::surface::surface::Surface;
            let result = ConstantRadiusAssistedFs::from_surfaces(
                &k,
                &mut access.pseudo,
                id,
                1e-6,
                0.1,
                0.01,
                [0.125 * i.scale; 2],
                [
                    (
                        Surface::from(Plane::from_normal(Point3::ZERO, Vec3::Z)),
                        SurfaceId::Free(0),
                    ),
                    (
                        Surface::from(Plane::from_normal(Point3::ZERO, Vec3::Y)),
                        SurfaceId::Free(1),
                    ),
                ],
                [true; 2],
            );
            if spec.edge == 13 {
                let b = access.breps.first_mut().ok_or_else(|| harness("no box"))?;
                let edge = if let Some(edge) = closed_edge {
                    edge
                } else {
                    let domain = Extent1d::from_min_max(0.0, 360.0);
                    let circle = solidcore::curve::ellipse::Ellipse::new_circle(
                        &k,
                        Point3::new(5.0 * i.scale, 0.0, 0.0),
                        Vec3::X,
                        Vec3::Y,
                        domain,
                        0.5 * i.scale,
                        &solidcore::curve::ellipse::EllipseOptions::default(),
                    )
                    .map_err(|e| harness(e.to_string()))?;
                    let circle = solidcore::curve::curve::Curve::from(circle);
                    let domain = circle.natural_interval();
                    let edge = solidcore::topology::brep::p5::create_wire_edge_from_curve(
                        &mut k, b, circle, domain,
                    )
                    .map_err(|e| harness(e.to_string()))?;
                    closed_edge = Some(edge);
                    edge
                };
                let uses = &b.edge(edge).map_err(|e| harness(e.to_string()))?.edgeuses;
                if let Ok(mut solver) = result {
                    solver.solver.edgeuses = [uses.first().copied(), uses.get(1).copied()];
                    Ok(solver)
                } else {
                    result
                }
            } else {
                result
            }
        };
        let mut solver = match result {
            Ok(solver) => solver,
            Err(e) => return Ok(error_output(&e)),
        };
        for rail in 0..2 {
            if spec.mask & (1 << rail) == 0 {
                solver
                    .solver
                    .set_surface(rail, None)
                    .map_err(|e| harness(e.to_string()))?;
            }
        }
        if spec.geometry_surfaces {
            solver
                .solver
                .set_fillet_geom_for_surfaces(solver.solver.fillet_geoms.first().copied());
        }
        access.solvers.push(super::SolverEntry::ConstantRadius(solver));
    }
    let mut executive = executive(Some(Handle::Brep(0)));
    executive.pseudo_brep = Some(Handle::Pseudo);
    let run = (|| -> SolidResult<Vec<f64>> {
        let mut out = vec![0.0];
        let b = access.brep(Handle::Brep(0))?;
        out.push(b.edges().len() as f64);
        for (_, edge) in b.edges().iter() {
            let domain = edge.interval;
            out.extend([domain.min(), domain.max()]);
            let curve = edge.curve.as_ref().ok_or_else(missing_error)?;
            for fraction in [0.0, 0.5, 1.0] {
                let t = domain.min() + (domain.max() - domain.min()) * fraction;
                let point = curve.evaluate_point(&k, t)?;
                out.extend([0.0, point.x, point.y, point.z]);
            }
        }
        for index in 0..access.solvers.len() {
            for rail in 0..2 {
                let surface = access.solver_surface(Handle::Solver(index), rail)?;
                out.push(flag(surface.is_some()));
                if let Some(surface) = surface {
                    let uv = surface
                        .base_surface()
                        .natural_uv_domain_with_nurb(&k)?
                        .evaluate(0.5, 0.5);
                    let point = surface.surface.evaluate_point(&k, uv)?;
                    out.extend([0.0, point.x, point.y, point.z]);
                }
            }
        }
        for (at, &[which, solver]) in i.loads.iter().enumerate() {
            if which != 0 {
                return missing();
            }
            out.push(
                match executive.load_fillet_solver(&k, &mut access, Handle::Solver(solver)) {
                    Ok(()) => 0.0,
                    Err(e) if e.is_unported() => return Err(e),
                    Err(e) => status_of(&e),
                },
            );
            if i.drop_pending_after == Some(at) {
                if let Some(vertex) = executive.filleted_vertices.first().copied() {
                    executive.vertex_corner_map.remove(&vertex);
                }
            }
            loader_snapshot(&access, &executive, &mut out)?;
            for solver in &access.solvers {
                let back_pointer_set = solver.base().fillet_executive().is_some();
                out.push(flag(back_pointer_set));
                let same_pseudo = if back_pointer_set {
                    let pseudo = executive.pseudo_brep.ok_or_else(missing_error)?;
                    std::ptr::eq(access.brep(pseudo)?, access.pseudo.brep())
                } else {
                    false
                };
                out.push(flag(same_pseudo));
                out.push(solver.base().g1_knots.len() as f64);
                out.extend(&solver.base().g1_knots);
            }
        }
        Ok(out)
    })();
    Ok(match run {
        Ok(out) => super::super::blend_fillet_executive_p1::to_output(&out),
        Err(e) => error_output(&e),
    })
}
