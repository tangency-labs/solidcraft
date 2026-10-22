//! Surface records ([`brep::Surface`]) in the evaluator's form ([`EvalSurface`]), which the
//! planner and the CPU and GPU evaluators share. Planes, splines and revolutions keep their
//! own parameters ([`SurfaceEval`] holds them in that form already). A cylinder, cone, sphere
//! or torus is the revolution it is, exactly: its generator (a line, or a circle as an exact
//! rational arc) turned about its axis, the angle as the revolution's parameter. The planner
//! finds a boundary's parameters by projecting its nodes, so the generator's own parameter
//! need not be the record's height or latitude.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::brep::{self, Frame};
use solidcraft_geom::tess::{EvalCurve, EvalSurface};

use crate::{KernelError, Result};

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// No transform: the revolution as it is.
const IDENTITY: [[f64; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// `surface` in the evaluator's form. `domain` is the face's parameter box in the record's
/// parameters, `[[u0, u1], [v0, v1]]`: a cylinder's or cone's generator spans the face's
/// heights (an unbounded line has no parameter box to project into).
pub(crate) fn eval_surface(surface: &brep::Surface, domain: [[f64; 2]; 2]) -> Result<EvalSurface> {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let invalid = |what: &str| KernelError::Invalid(format!("tessellation: {what}"));
    let axes = |f: &Frame| {
        let (o, x, y) = (v3(f.origin), v3(f.x_axis), v3(f.y_axis));
        (o, x, x.cross(y))
    };
    let revolved = |profile: EvalCurve, origin: Vec3, axis: Vec3| -> Result<EvalSurface> {
        let axis = axis.normalized().ok_or_else(|| invalid("an analytic surface's frame"))?;
        Ok(EvalSurface::Revolved { profile, origin, axis, transform: IDENTITY, swap: false })
    };
    // The generator of a cylinder or cone over the face's heights, run from the top down so the
    // revolution's (generator, angle) normal is the record's natural normal (outward).
    let generator = |at: &dyn Fn(f64) -> Vec3| -> Result<EvalCurve> {
        let [h0, h1] = domain[1];
        if !(h0.is_finite() && h1.is_finite() && h1 > h0) {
            return Err(invalid("a cylinder or cone face with no height"));
        }
        let (a, b) = (at(h1), at(h0));
        // Parameter −v: the generator at t is the surface at height −t.
        Ok(EvalCurve { degree: 1, knots: vec![-h1, -h1, -h0, -h0], ctrl: vec![[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]] })
    };
    match surface {
        brep::Surface::Cylinder { frame, radius } => {
            let (o, x, z) = axes(frame);
            revolved(generator(&|h| o + x * *radius + z * h)?, o, z)
        }
        brep::Surface::Cone { frame, radius_at_origin, half_angle } => {
            let (o, x, z) = axes(frame);
            let slope = half_angle.tan();
            revolved(generator(&|h| o + x * (radius_at_origin + h * slope) + z * h)?, o, z)
        }
        brep::Surface::Sphere { frame, radius } => {
            let (o, x, z) = axes(frame);
            // The meridian from the north pole to the south pole.
            revolved(arc(o, x, z, *radius, [-FRAC_PI_2, FRAC_PI_2]).reversed(), o, z)
        }
        brep::Surface::Torus { frame, major_radius, minor_radius } => {
            let (o, x, z) = axes(frame);
            revolved(arc(o + x * *major_radius, x, z, *minor_radius, [0.0, TAU]).reversed(), o, z)
        }
        _ => match SurfaceEval::new(surface) {
            Ok(SurfaceEval::Eval(e, _)) => Ok(e),
            Ok(_) => Err(invalid("an analytic surface out of place")),
            Err(e) => Err(KernelError::Failed(format!("not supported yet: tessellation: {e}"))),
        },
    }
}

/// The circle `centre + r (cos t x + sin t y)` for t in `angle`, as an exact rational quadratic
/// spline over [0, 1], one span per quarter turn or less.
fn arc(centre: Vec3, x: Vec3, y: Vec3, r: f64, angle: [f64; 2]) -> EvalCurve {
    let span = angle[1] - angle[0];
    let count = (span / std::f64::consts::FRAC_PI_2).ceil().clamp(1.0, 4.0) as usize;
    let step = span / count as f64;
    let weight = (step / 2.0).cos();
    // A point at angle `t`, its radius divided by `scale` (the middle control point of a span
    // lies 1 / cos(step / 2) out), as homogeneous coordinates of weight `w`.
    let at = |t: f64, scale: f64, w: f64| {
        let p = centre + x * (r * t.cos() / scale) + y * (r * t.sin() / scale);
        [p.x * w, p.y * w, p.z * w, w]
    };
    let mut ctrl = vec![at(angle[0], 1.0, 1.0)];
    let mut knots = vec![0.0; 3];
    for i in 0..count {
        let start = angle[0] + i as f64 * step;
        ctrl.push(at(start + step / 2.0, weight, weight));
        ctrl.push(at(start + step, 1.0, 1.0));
        knots.extend(std::iter::repeat_n((i + 1) as f64 / count as f64, if i + 1 == count { 3 } else { 2 }));
    }
    EvalCurve { degree: 2, knots, ctrl }
}

/// A curve on [0, 1] run the other way: t becomes 1 − t.
trait Reversed {
    fn reversed(self) -> Self;
}

impl Reversed for EvalCurve {
    fn reversed(mut self) -> Self {
        self.ctrl.reverse();
        self.knots = self.knots.iter().rev().map(|k| 1.0 - k).collect();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::Solid;

    /// Every face of `solid` evaluates as truck evaluates it, over its own parameter box.
    fn assert_matches_truck(solid: &Solid) -> [usize; 4] {
        let mut kinds = [0; 4];
        for face in solid.face_iter() {
            let surface = face.surface();
            let kind = match surface {
                mt::Surface::Plane(_) => 0,
                mt::Surface::BSplineSurface(_) => 1,
                mt::Surface::NurbsSurface(_) => 2,
                mt::Surface::RevolutedCurve(_) => 3,
            };
            let Some(eval) = eval_surface(&surface) else { panic!("not evaluable: {surface:?}") };
            // Sample the face's parameter box from its boundary vertices.
            let pts: Vec<(f64, f64)> = face.vertex_iter().filter_map(|v| surface.search_parameter(v.point(), None, 100)).collect();
            if pts.is_empty() {
                continue;
            }
            let (u0, u1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
            let (v0, v1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
            for i in 0..=6 {
                for j in 0..=6 {
                    let (u, v) = (u0 + (u1 - u0) * i as f64 / 6.0, v0 + (v1 - v0) * j as f64 / 6.0);
                    let (p, n) = eval.eval(u, v);
                    let want = from_p3(surface.subs(u, v));
                    assert!(p.dist(want) < 1e-9 * (1.0 + want.len()), "point at ({u}, {v}): {p:?} vs {want:?}");
                    let wn = from_v3(surface.normal(u, v));
                    if wn.len() > 0.5 {
                        assert!(n.dist(wn) < 1e-6, "normal at ({u}, {v}): {n:?} vs {wn:?} ({surface:?})");
                    }
                    kinds[kind] += 1;
                }
            }
        }
        kinds
    }

    fn solid(b: crate::Body) -> Solid {
        (*b.solid).clone()
    }

    #[test]
    fn primitives_evaluate_like_truck() {
        let Ok(bx) = crate::box_solid(Vec3::new(-1.0, -2.0, 0.0), Vec3::new(3.0, 4.0, 5.0)) else { panic!("box") };
        let Ok(cyl) = crate::cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0), 2.5, 7.0) else { panic!("cylinder") };
        let Ok(sph) = crate::sphere(Vec3::new(0.0, 0.0, 1.0), 4.0) else { panic!("sphere") };
        let Ok(tor) = crate::torus(Vec3::ZERO, 6.0, 1.5) else { panic!("torus") };
        let mut kinds = [0; 4];
        for b in [bx, cyl, sph, tor] {
            for (k, n) in kinds.iter_mut().zip(assert_matches_truck(&solid(b))) {
                *k += n;
            }
        }
        // Planes, rational patches (sphere) and revolved profiles (cylinder, torus) all covered.
        assert!(kinds[0] > 0 && kinds[3] > 0 && kinds[1] + kinds[2] > 0, "{kinds:?}");
    }

    #[test]
    fn transformed_and_mirrored_bodies_evaluate_like_truck() {
        let Ok(cyl) = crate::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 5.0) else { panic!("cylinder") };
        // Column-major: x → 3 − x, z → z + 1.
        let mirror = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [3.0, 0.0, 1.0, 1.0]];
        let Ok(m) = crate::transform_matrix(&cyl, mirror) else { panic!("transform") };
        assert!(assert_matches_truck(&solid(m)).iter().sum::<usize>() > 0);
    }
}
