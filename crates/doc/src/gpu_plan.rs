//! A body's display plan in the form the viewport evaluates on the GPU: solidcore-tess's plan,
//! made once from the kernel's plan for each plan the model caches. The kernel still plans in
//! geom's form; until it plans in solidcore-tess's (#13399) the model keeps both, the kernel's for
//! its CPU evaluation and this one for the GPU, so the viewport's cache, keyed by each plan's
//! allocation, keeps its uploads from frame to frame.

use solidcore_tess::vec3;
use solidcraft_geom::tess::{EvalCurve, EvalSurface, FacePlan, TessPlan};

/// `plan` in solidcore-tess's form: the same surfaces, nodes, triangles and edges, with no pole
/// nodes (geom's plans name none). A NURBS surface keeps the closed directions geom worked out.
pub fn gpu_plan(plan: &TessPlan) -> solidcore_tess::TessPlan {
    solidcore_tess::TessPlan {
        surfaces: plan.surfaces.iter().map(surface).collect(),
        faces: plan.faces.iter().map(face).collect(),
        edges: plan.edges.clone(),
        edge_faces: plan.edge_faces.clone(),
    }
}

fn curve(c: &EvalCurve) -> solidcore_tess::EvalCurve {
    solidcore_tess::EvalCurve::new(c.degree, c.knots.clone(), c.ctrl.clone())
}

fn surface(s: &EvalSurface) -> solidcore_tess::EvalSurface {
    match s {
        EvalSurface::Plane { origin, du, dv } => solidcore_tess::EvalSurface::Plane { origin: vec3(*origin), du: vec3(*du), dv: vec3(*dv) },
        EvalSurface::Nurbs { degree, knots, count, ctrl, closed } => {
            solidcore_tess::EvalSurface::nurbs_closed(*degree, knots.clone(), *count, ctrl.clone(), *closed)
        }
        EvalSurface::Revolved { profile, origin, axis, transform, swap } => solidcore_tess::EvalSurface::Revolved {
            profile: curve(profile),
            origin: vec3(*origin),
            axis: vec3(*axis),
            transform: *transform,
            swap: *swap,
        },
    }
}

fn face(f: &FacePlan) -> solidcore_tess::FacePlan {
    solidcore_tess::FacePlan {
        face: f.face,
        surface: f.surface,
        uv: f.uv.clone(),
        fixed: f.fixed.clone(),
        normals: f.normals.clone(),
        flip: f.flip,
        triangles: f.triangles.clone(),
        poles: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use solidcore_record::Vec3;
    use solidcore_tess::vec3;
    use solidcraft_geom::tess::{EvalCurve, EvalSurface, FacePlan, TessPlan};

    /// Every field of every surface kind, face and edge comes across unchanged, read back through
    /// solidcore's own accessors.
    #[test]
    fn a_plan_converts_field_for_field() {
        let ctrl =
            vec![[1.0, 0.0, 0.0, 1.0], [0.5, 0.5, 0.0, 0.5], [0.0, 1.0, 0.0, 1.0], [1.0, 0.0, 2.0, 1.0], [0.5, 0.5, 1.0, 0.5], [0.0, 1.0, 2.0, 1.0]];
        let knots = [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]];
        let profile = EvalCurve { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[2.0, 0.0, 0.0, 1.0], [3.0, 0.0, 1.5, 1.0]] };
        let transform = [[-1.0, 0.0, 0.0, 4.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.5]];
        let plan = TessPlan {
            surfaces: vec![
                EvalSurface::Plane { origin: Vec3::X, du: Vec3::Y, dv: Vec3::Z },
                EvalSurface::Nurbs { degree: [2, 1], knots: knots.clone(), count: [3, 2], ctrl: ctrl.clone(), closed: [true, false] },
                EvalSurface::Revolved { profile: profile.clone(), origin: Vec3::Z, axis: Vec3::Y, transform, swap: true },
            ],
            faces: vec![FacePlan {
                face: 7,
                surface: Some(2),
                uv: vec![[0.0, 0.5], [1.0, 0.25]],
                fixed: vec![Vec3::X],
                normals: vec![Vec3::Y],
                flip: true,
                triangles: vec![[0, 1, 1]],
            }],
            edges: vec![vec![Vec3::X, Vec3::Y]],
            edge_faces: vec![vec![7]],
        };
        let gpu = super::gpu_plan(&plan);
        let [plane, nurbs, revolved] = gpu.surfaces.as_slice() else { panic!("{:?}", gpu.surfaces) };
        assert_eq!(*plane, solidcore_tess::EvalSurface::Plane { origin: vec3(Vec3::X), du: vec3(Vec3::Y), dv: vec3(Vec3::Z) });
        let solidcore_tess::EvalSurface::Nurbs(n) = nurbs else { panic!("{nurbs:?}") };
        assert_eq!((n.degree(), n.knots(), n.count(), n.ctrl(), n.closed()), ([2, 1], &knots, [3, 2], ctrl.as_slice(), [true, false]));
        let solidcore_tess::EvalSurface::Revolved { profile: p, origin, axis, transform: t, swap } = revolved else { panic!("{revolved:?}") };
        assert_eq!((p.degree(), p.knots(), p.ctrl()), (profile.degree, profile.knots.as_slice(), profile.ctrl.as_slice()));
        assert_eq!((*origin, *axis, *t, *swap), (vec3(Vec3::Z), vec3(Vec3::Y), transform, true));
        let [f] = gpu.faces.as_slice() else { panic!("{:?}", gpu.faces) };
        let g = &plan.faces[0];
        assert_eq!(
            (f.face, f.surface, &f.uv, &f.fixed, &f.normals, f.flip, &f.triangles),
            (g.face, g.surface, &g.uv, &g.fixed, &g.normals, g.flip, &g.triangles)
        );
        assert!(f.poles.is_empty());
        assert_eq!((&gpu.edges, &gpu.edge_faces), (&plan.edges, &plan.edge_faces));
    }

    /// A body's GPU plan is made once per display quality and then read as the same allocation,
    /// by the body and its clones (the viewport keys its uploads by it), equal to its display plan
    /// converted.
    #[test]
    fn a_bodys_gpu_plan_is_one_allocation_across_reads() {
        let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 8.0, 4.0)).unwrap();
        let b = crate::ModelBody::new("Body1".into(), body, 0);
        let q = crate::display_quality::DisplayQuality::default();
        let first = b.gpu_plan_at(q).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &b.gpu_plan_at(q).unwrap()));
        assert!(std::sync::Arc::ptr_eq(&first, &b.clone().gpu_plan_at(q).unwrap()));
        assert!(std::sync::Arc::ptr_eq(&first, &b.refined_display(q, 0).unwrap().0));
        assert_eq!(*first, super::gpu_plan(&b.display_plan_at(q).unwrap()));
    }
}
