//! Native face parameters mapped into the common kernel evaluator.
use crate::{Result, TessError};
use solidcore::eval::plan::EvalSurface;
use solidcore_record::brep;

pub(crate) use solidcore::eval::surface_plan::Params;

pub(crate) fn eval_surface(
    surface: &brep::Surface,
    domain: [[f64; 2]; 2],
) -> Result<(EvalSurface, Params)> {
    let surface = solidcore_convert::surface(surface)
        .map_err(|error| TessError::Invalid(format!("tessellation: {error}")))?;
    solidcore::eval::surface_plan::eval_surface(&surface, domain)
        .map_err(|error| TessError::Failed(format!("tessellation: {}", crate::eval_message(error))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every face of `b` evaluates, through [`eval_surface`] and its [`Params`], as the kernel's
    /// evaluator ([`solidcore::eval::SurfaceEval`]) evaluates the record, over the face's parameter box: the same
    /// point and the same natural normal. Counts of planes, analytic surfaces and others.
    fn assert_matches_the_record(b: &brep::Brep) -> [usize; 3] {
        let mut kinds = [0; 3];
        for face in &b.faces {
            let kind = match face.surface {
                brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. } => 0,
                brep::Surface::Cylinder { .. }
                | brep::Surface::Cone { .. }
                | brep::Surface::Sphere { .. }
                | brep::Surface::Torus { .. } => 1,
                _ => 2,
            };
            let (eval, map) = match eval_surface(face.parameters(), face.domain) {
                Ok(e) => e,
                Err(e) => panic!("not evaluable: {e} ({:?})", face.surface),
            };
            let Ok(reference) = solidcore_convert::surface(face.parameters())
                .map(|s| solidcore::eval::SurfaceEval::new(&s))
            else {
                panic!("the record: {:?}", face.surface)
            };
            let Ok(reference) = reference else {
                panic!("the kernel's evaluator: {:?}", face.surface)
            };
            let [[u0, u1], [v0, v1]] = face.domain;
            for i in 0..=6 {
                for j in 0..=6 {
                    let uv = [
                        u0 + (u1 - u0) * i as f64 / 6.0,
                        v0 + (v1 - v0) * j as f64 / 6.0,
                    ];
                    let q = map.to_eval(uv);
                    let (p, n) = eval.eval(q[0], q[1]);
                    let Some((want, su, sv)) = reference.derivs(uv[0], uv[1]) else {
                        panic!("reference at {uv:?}")
                    };
                    assert!(
                        p.dist(want) < 1e-9 * (1.0 + want.len()),
                        "point at {uv:?}: {p:?} vs {want:?}"
                    );
                    if let Some(wn) = su
                        .cross(sv)
                        .normalized()
                        .filter(|_| su.cross(sv).len() > 1e-9 * (su.len() * sv.len()).max(1e-12))
                    {
                        assert!(
                            n.dist(wn) < 1e-6,
                            "normal at {uv:?}: {n:?} vs {wn:?} ({:?})",
                            face.surface
                        );
                    }
                    kinds[kind] += 1;
                }
            }
        }
        kinds
    }

    #[test]
    fn primitives_evaluate_like_the_record() {
        // A box from (-1, -2, 0) to (3, 4, 5); a cylinder of radius 2.5 and length 7 from (1, 2, 3)
        // along (0, 1, 1); a sphere of radius 4 about (0, 0, 1); a torus of radii 6 and 1.5.
        let bodies = ["box_offset", "cylinder_tilted", "sphere_r4", "torus_6_1_5"]
            .map(crate::fixtures::fixture);
        let mut kinds = [0; 3];
        for b in bodies {
            for (k, n) in kinds.iter_mut().zip(assert_matches_the_record(&b)) {
                *k += n;
            }
        }
        // Planes and the analytic surfaces (cylinder, sphere, torus) all covered.
        assert!(kinds[0] > 0 && kinds[1] > 0, "{kinds:?}");
    }

    #[test]
    fn transformed_and_mirrored_bodies_evaluate_like_the_record() {
        // A cylinder of radius 2 and length 5 mirrored by x → 3 − x and moved by z → z + 1.
        assert!(
            assert_matches_the_record(&crate::fixtures::fixture("cylinder_r2_h5_mirrored"))
                .iter()
                .sum::<usize>()
                > 0
        );
    }
}
