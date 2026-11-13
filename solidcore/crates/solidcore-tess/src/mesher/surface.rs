//! Surface records ([`brep::Surface`]) in the evaluator's form ([`EvalSurface`]), which the
//! planner and the CPU and GPU evaluators share, and the map from the record's (u, v) to the
//! evaluator's, so a face's boundary takes its parameters from its edges' exact pcurves.
//! Planes, splines and revolutions keep their own parameters ([`solidcore::eval::SurfaceEval`] holds them in
//! that form already). A cylinder, cone, sphere or torus is the revolution it is, exactly: its
//! generator (a line, or a circle as an exact rational arc) turned about its axis by the
//! record's angle u; the generator's parameter is −v on a line and, on an arc, the rational
//! parameter of the record's angle v (closed form, [`Arc::param`]).

use solidcore::Vec3;
use solidcore_record::brep::{self, Frame};
use crate::vec::VecExt as _;
use solidcore::eval::plan::{EvalCurve, EvalSurface};

use crate::{Result, TessError};

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// No transform: the revolution as it is.
const IDENTITY: [[f64; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// How a face's record parameters become its evaluator's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Params {
    /// The same parameters (planes, splines, revolutions).
    Same,
    /// A cylinder or cone: (angle, height) to (−height, angle).
    Height,
    /// A sphere or torus: (angle, meridian angle) to (the reversed arc's parameter, angle).
    Meridian(Arc),
}

impl Params {
    /// The evaluator's parameters of the record's `uv`.
    pub(crate) fn to_eval(self, uv: [f64; 2]) -> [f64; 2] {
        let [u, v] = uv;
        match self {
            Params::Same => uv,
            Params::Height => [-v, u],
            Params::Meridian(a) => [1.0 - a.param(v), u],
        }
    }
}

/// The spans of a circular arc made by [`arc`]: from angle `start`, `count` spans of `step`
/// each, over the parameter [0, 1]; `periodic` for a whole turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Arc {
    start: f64,
    step: f64,
    count: usize,
    periodic: bool,
}

impl Arc {
    /// The arc's parameter at `angle`. On a rational quadratic span of half angle α with middle
    /// weight cos α, tan(θ / 2) runs linearly from −tan(α / 2) to tan(α / 2) with the span's
    /// own parameter (θ from the span's middle). A whole turn continues by whole parameters.
    pub(crate) fn param(&self, angle: f64) -> f64 {
        let total = self.step * self.count as f64;
        let turns = if self.periodic { ((angle - self.start) / total).floor() } else { 0.0 };
        let a = angle - turns * total;
        let i = (((a - self.start) / self.step).floor().max(0.0) as usize).min(self.count.saturating_sub(1));
        let half = 0.5 * self.step;
        let local = a - (self.start + (i as f64 + 0.5) * self.step);
        let s = 0.5 * ((0.5 * local).tan() / (0.5 * half).tan() + 1.0);
        (i as f64 + s) / self.count as f64 + turns
    }
}

/// `surface` in the evaluator's form, and how the record's parameters map onto it. `domain` is
/// the face's parameter box in the record's parameters, `[[u0, u1], [v0, v1]]`: a cylinder's
/// or cone's generator spans the face's heights.
pub(crate) fn eval_surface(surface: &brep::Surface, domain: [[f64; 2]; 2]) -> Result<(EvalSurface, Params)> {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let invalid = |what: &str| TessError::Invalid(format!("tessellation: {what}"));
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
    // A meridian from the record's least angle to its greatest, reversed (so the normal is the
    // record's natural one, outward), its angle measured from `x` toward `y` in the plane of `x`
    // and the axis `z`.
    let meridian = |centre: Vec3, [x, y]: [Vec3; 2], z: Vec3, r: f64, angle: [f64; 2], o: Vec3| -> Result<(EvalSurface, Params)> {
        let (curve, spans) = arc(centre, x, y, r, angle);
        Ok((revolved(curve.reversed(), o, z)?, Params::Meridian(spans)))
    };
    match surface {
        brep::Surface::Cylinder { frame, radius } => {
            let (o, x, z) = axes(frame);
            Ok((revolved(generator(&|h| o + x * *radius + z * h)?, o, z)?, Params::Height))
        }
        brep::Surface::Cone { frame, radius_at_origin, half_angle } => {
            let (o, x, z) = axes(frame);
            let slope = half_angle.tan();
            Ok((revolved(generator(&|h| o + x * (radius_at_origin + h * slope) + z * h)?, o, z)?, Params::Height))
        }
        brep::Surface::Sphere { frame, radius } => {
            let (o, x, z) = axes(frame);
            meridian(o, [x, z], z, *radius, [-FRAC_PI_2, FRAC_PI_2], o)
        }
        brep::Surface::Torus { frame, major_radius, minor_radius, tube_start } => {
            let (o, x, z) = axes(frame);
            // The tube angle counts from its start.
            let (s, c) = tube_start.sin_cos();
            meridian(o + x * *major_radius, [x * c + z * s, z * c - x * s], z, *minor_radius, [0.0, TAU], o)
        }
        _ => {
            let value = solidcore_convert::surface(surface).map_err(|e| TessError::Invalid(format!("tessellation: {e}")))?;
            match solidcore::eval::SurfaceEval::new(&value) {
                Ok(solidcore::eval::SurfaceEval::Eval(e, _)) => Ok((evaluator_form(&e), Params::Same)),
                Ok(_) => Err(invalid("an analytic surface out of place")),
                Err(e) => Err(TessError::Failed(format!("not supported yet: tessellation: {}", crate::eval_message(e)))),
            }
        }
    }
}

/// The circle `centre + r (cos t x + sin t y)` for t in `angle`, as an exact rational quadratic
/// spline over [0, 1], one span per quarter turn or less, and its spans.
fn arc(centre: Vec3, x: Vec3, y: Vec3, r: f64, angle: [f64; 2]) -> (EvalCurve, Arc) {
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
    let periodic = (span - std::f64::consts::TAU).abs() < 1e-12;
    (EvalCurve { degree: 2, knots, ctrl }, Arc { start: angle[0], step, count, periodic })
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

/// The kernel's form of a plane, spline or revolution ([`solidcore::eval::EvalSurface`]) in
/// the plan evaluator's: the same values, field for field.
fn evaluator_form(s: &solidcore::eval::EvalSurface) -> EvalSurface {
    match s {
        solidcore::eval::EvalSurface::Plane { origin, du, dv } => EvalSurface::Plane { origin: *origin, du: *du, dv: *dv },
        solidcore::eval::EvalSurface::Nurbs(n) => EvalSurface::nurbs(n.degree(), n.knots().clone(), n.count(), n.ctrl().to_vec()),
        solidcore::eval::EvalSurface::Revolved { profile, origin, axis, transform, swap } => EvalSurface::Revolved {
            profile: EvalCurve { degree: profile.degree(), knots: profile.knots().to_vec(), ctrl: profile.ctrl().to_vec() },
            origin: *origin,
            axis: *axis,
            transform: *transform,
            swap: *swap,
        },
    }
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
                brep::Surface::Cylinder { .. } | brep::Surface::Cone { .. } | brep::Surface::Sphere { .. } | brep::Surface::Torus { .. } => 1,
                _ => 2,
            };
            let (eval, map) = match eval_surface(face.parameters(), face.domain) {
                Ok(e) => e,
                Err(e) => panic!("not evaluable: {e} ({:?})", face.surface),
            };
            let Ok(reference) = solidcore_convert::surface(face.parameters()).map(|s| solidcore::eval::SurfaceEval::new(&s)) else { panic!("the record: {:?}", face.surface) };
            let Ok(reference) = reference else { panic!("the kernel's evaluator: {:?}", face.surface) };
            let [[u0, u1], [v0, v1]] = face.domain;
            for i in 0..=6 {
                for j in 0..=6 {
                    let uv = [u0 + (u1 - u0) * i as f64 / 6.0, v0 + (v1 - v0) * j as f64 / 6.0];
                    let q = map.to_eval(uv);
                    let (p, n) = eval.eval(q[0], q[1]);
                    let Some((want, su, sv)) = reference.derivs(uv[0], uv[1]) else { panic!("reference at {uv:?}") };
                    assert!(p.dist(want) < 1e-9 * (1.0 + want.len()), "point at {uv:?}: {p:?} vs {want:?}");
                    if let Some(wn) = su.cross(sv).normalized().filter(|_| su.cross(sv).len() > 1e-9 * (su.len() * sv.len()).max(1e-12)) {
                        assert!(n.dist(wn) < 1e-6, "normal at {uv:?}: {n:?} vs {wn:?} ({:?})", face.surface);
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
        let bodies = ["box_offset", "cylinder_tilted", "sphere_r4", "torus_6_1_5"].map(crate::fixtures::fixture);
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
        assert!(assert_matches_the_record(&crate::fixtures::fixture("cylinder_r2_h5_mirrored")).iter().sum::<usize>() > 0);
    }
}
