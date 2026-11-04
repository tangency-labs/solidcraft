//! A face's surface in the evaluator's form ([`EvalSurface`]), which the planner and the CPU
//! and GPU evaluators share, on the face's own (u, v): its pcurves and domain are in those
//! parameters, so its boundary takes them from its edges' exact pcurves as they are. A face's
//! parameters are a plane or a spline ([`brep::Face::parameters`]): the record holds every
//! cylinder, cone, sphere, ball patch, torus and revolution as a label with its exact NURBS form,
//! made by the kernel ([`solidcore::eval::form`]), and refuses one without it. Tess converts
//! nothing of its own: [`SurfaceEval`] makes the evaluator's form, and a label in place of its
//! form is the record's error.

use solidcore::eval::{EvalSurface, SurfaceEval};
use solidcore_convert as kernel;
use solidcore_record::brep;

use crate::{Result, TessError};

/// `surface` (a face's parameters, [`brep::Face::parameters`]) in the evaluator's form, on its
/// own parameters.
pub(crate) fn eval_surface(surface: &brep::Surface) -> Result<EvalSurface> {
    match SurfaceEval::new(&kernel::surface(surface)) {
        Ok(SurfaceEval::Eval(e, _)) => Ok(e),
        // The record requires the form ([`brep::Brep::validate`]): a label the kernel's
        // evaluator keeps in its closed form (a cylinder, cone, sphere or torus) stands here
        // without it.
        Ok(_) => Err(TessError::Invalid(format!(
            "tessellation: a {} face without its NURBS form",
            super::kind(surface)
        ))),
        Err(e) => Err(TessError::Failed(format!(
            "not supported yet: tessellation: {}",
            crate::eval_message(e)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::tests::fixture;

    /// Every face of the primitives evaluates on its own parameters (a plane, or the NURBS form
    /// the record holds), and a cylinder, sphere or torus label in place of its form is refused
    /// as the record's error: tess makes no form of its own.
    #[test]
    fn a_face_evaluates_on_its_form_and_a_label_alone_is_refused() {
        // A box from (-1, -2, 0) to (3, 4, 5); a cylinder of radius 2.5 and length 7 from
        // (1, 2, 3) along (0, 1, 1); a sphere of radius 4 about (0, 0, 1); a torus of radii 6
        // and 1.5 about the origin; a cylinder of radius 2 and length 5 mirrored and moved.
        let bodies = [
            "box_offset",
            "cylinder_tilted",
            "sphere_r4",
            "torus_6_1_5",
            "cylinder_r2_h5_mirrored",
            // A drilled hole's cylinder and cone (its drill point), and SMLib's bodies: a half
            // sphere, a torus and the threads' cylinders and spline walls.
            "box_with_drill_hole_r3_depth_10",
            "smlib_half_sphere",
            "smlib_torus_22_4",
            "smlib_thread_in_bore",
            "smlib_thread_run_out",
        ];
        let (mut labels, mut cones) = (0, 0);
        for name in bodies {
            for face in &fixture(name).faces {
                assert!(
                    eval_surface(face.parameters()).is_ok(),
                    "{name}: {:?}",
                    face.surface
                );
                if matches!(
                    face.surface,
                    brep::Surface::Cylinder { .. }
                        | brep::Surface::Cone { .. }
                        | brep::Surface::Sphere { .. }
                        | brep::Surface::Torus { .. }
                ) {
                    labels += 1;
                    cones += usize::from(matches!(face.surface, brep::Surface::Cone { .. }));
                    match eval_surface(&face.surface) {
                        Err(TessError::Invalid(why)) => {
                            assert!(why.contains("without its NURBS form"), "{name}: {why}");
                        }
                        other => panic!("{name}: a label alone: {other:?}"),
                    }
                }
            }
        }
        assert!(labels > 0 && cones > 0, "{labels} labels, {cones} cones");
    }
}
