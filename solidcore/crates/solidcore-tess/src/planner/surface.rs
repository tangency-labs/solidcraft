//! A face's surface in the evaluator's form ([`EvalSurface`]), which the planner and the CPU
//! and GPU evaluators share, on the face's own (u, v): its pcurves and domain are in those
//! parameters, so its boundary takes them from its edges' exact pcurves as they are. A face's
//! parameters are a plane or a spline ([`brep::Face::parameters`]): the record holds every
//! cylinder, cone, sphere, ball patch, torus and revolution as a label with its exact NURBS form,
//! made by the kernel ([`solidcore::eval::form`]), and refuses one without it. Tess converts
//! nothing of its own: [`SurfaceEval`] makes the evaluator's form, and a label in place of its
//! form is the record's error.

use std::collections::HashMap;
use std::sync::Mutex;

use solidcore::eval::{EvalSurface, SurfaceEval};
use solidcore_convert as kernel;
use solidcore_record::brep;

use crate::{Result, TessError};

/// `surface` (a face's parameters, [`brep::Face::parameters`]) in the evaluator's form, on its
/// own parameters.
pub(crate) fn eval_surface(surface: &brep::Surface) -> Result<EvalSurface> {
    let surface_value = kernel::surface(surface).map_err(|e| TessError::Invalid(e.to_string()))?;
    match SurfaceEval::new(&surface_value) {
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

/// A face's surface with the kernel's pole classifier's answers kept per iso-line.
///
/// Whether a surface is collapsed along `d` at `q` is decided on the iso-line through `q` along
/// `d` alone ([`solidcore::eval::collapse`]): it depends on `q` only through the other
/// parameter (wrapped into the surface's period, [`EvalSurface::wrap`]) and on the reach. One
/// classification builds that iso-line's whole control row (a de Boor evaluation per column of
/// the net), so on a spline of many poles it costs far more than a point evaluation, while the
/// planner asks it at every node, and the nodes of a face share iso-lines: every
/// node of an edge on a patch side, a structured grid's rows and columns, a cut grid's lines.
/// Each answer is kept here under its iso-line and asked of the kernel once. A refusal is not
/// kept (its message names the point), so it is asked again and comes out the same.
pub(crate) struct FaceSurface {
    eval: EvalSurface,
    /// `(d, other parameter, reach)`, the two numbers by their bits, to the kernel's answer.
    collapsed: Mutex<HashMap<(usize, u64, u64), bool>>,
    /// Calls of the kernel's classifier made, for the tests.
    #[cfg(test)]
    asked: std::sync::atomic::AtomicUsize,
}

impl FaceSurface {
    pub(crate) fn new(eval: EvalSurface) -> FaceSurface {
        FaceSurface {
            eval,
            collapsed: Mutex::new(HashMap::new()),
            #[cfg(test)]
            asked: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// The surface, for the plan.
    pub(crate) fn into_eval(self) -> EvalSurface {
        self.eval
    }

    /// Whether the surface is collapsed along `d` at `q` to within `reach`, as the kernel's
    /// classifier says ([`solidcore::eval::uv::degenerate_within`]), asked once per iso-line.
    pub(crate) fn collapsed(
        &self,
        q: [f64; 2],
        d: usize,
        reach: f64,
    ) -> std::result::Result<bool, solidcore::eval::EvalError> {
        let ask = || {
            #[cfg(test)]
            self.asked
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            solidcore::eval::uv::degenerate_within(&self.eval, q, d, reach)
        };
        // A plane's answer is its axis alone, cheaper than the lookup; a parameter that is not
        // finite, or a direction past v, is the kernel's refusal.
        if matches!(self.eval, EvalSurface::Plane { .. })
            || d > 1
            || !(q[0].is_finite() && q[1].is_finite())
        {
            return ask();
        }
        let Some(&other) = self.eval.wrap(q).get(1 - d) else {
            return ask();
        };
        let key = (d, other.to_bits(), reach.to_bits());
        if let Some(&known) = self.lock().get(&key) {
            return Ok(known);
        }
        let answer = ask()?;
        self.lock().insert(key, answer);
        Ok(answer)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(usize, u64, u64), bool>> {
        // The map holds only finished answers, so one left by a panicking thread is sound.
        self.collapsed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Calls of the kernel's classifier made so far.
    #[cfg(test)]
    pub(crate) fn asked(&self) -> usize {
        self.asked.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl std::ops::Deref for FaceSurface {
    type Target = EvalSurface;

    fn deref(&self) -> &EvalSurface {
        &self.eval
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::tests::fixture;

    /// The memo gives the kernel's own answer at every node, and asks the kernel once per
    /// iso-line: on a 9 x 9 grid of each curved face of the coil and of a sphere (whose poles
    /// collapse a whole row), 18 questions per reach, not 162.
    #[test]
    fn the_pole_classifier_is_asked_once_per_iso_line() {
        for name in ["smlib_coil_lower_half", "sphere_r4"] {
            for face in &fixture(name).faces {
                let s = FaceSurface::new(eval_surface(face.parameters()).unwrap());
                if matches!(*s, EvalSurface::Plane { .. }) {
                    continue;
                }
                let dom = solidcore::eval::uv::domain(&s);
                let [(u0, u1), (v0, v1)] = dom.range.map(|r| r.unwrap_or((0.0, 1.0)));
                let at = |a: f64, b: f64, i: usize| a + (b - a) * i as f64 / 8.0;
                for reach in [0.0, 1e-3] {
                    for i in 0..9 {
                        for j in 0..9 {
                            let q = [at(u0, u1, i), at(v0, v1, j)];
                            for d in 0..2 {
                                let want = solidcore::eval::uv::degenerate_within(&s, q, d, reach)
                                    .unwrap();
                                assert_eq!(s.collapsed(q, d, reach).unwrap(), want, "{name} {q:?}");
                            }
                        }
                    }
                }
                assert_eq!(s.asked(), 2 * 2 * 9, "{name}");
            }
        }
    }

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
