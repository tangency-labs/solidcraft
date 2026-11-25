//! Where a face's surface has a pole, as the kernel's classifier says, asked once per iso-line.
//!
//! Whether a surface is collapsed along `d` at `q` is decided by the kernel on the iso-line
//! through `q` along `d` alone ([`solidcore::eval::collapse`]): it depends on `q` only through
//! the other parameter (wrapped into the surface's period) and on the reach. One question builds
//! that iso-line's whole control row, a de Boor evaluation per column of the net, while the
//! mesher asks at every node, and the nodes of a face share iso-lines (an edge on a patch side,
//! a grid's rows and columns, a cut grid's lines). [`Face`] keeps each answer under its iso-line
//! and asks the kernel once. A node past the surface's parameter range by no more than the face's
//! own tolerance (the largest of its edges' and vertices', as the record carries them) is asked
//! at its nearest point of the range: it lies on the face to within that tolerance. Where the
//! kernel cannot tell (a weight not certified non-zero, a node farther off than the face's
//! tolerance), nothing is taken for an answer: the face is refused, named with the point and the
//! reason ([`Face::refusal`]).

use crate::hash::BitsMap;
use std::sync::Mutex;

use solidcore::eval::plan::EvalSurface;

/// A face's surface for the mesher: the plan evaluator's form, the kernel's form of the same
/// surface (its classifier's), and the poles found on it.
pub(crate) struct Face {
    eval: EvalSurface,
    kernel: solidcore::eval::EvalSurface,
    /// How far (mm) a node may lie off the surface's parameter range and still be the face's.
    tolerance: f64,
    /// `(d, other parameter, reach)`, the two numbers by their bits, to the kernel's answer.
    collapsed: Mutex<BitsMap<(usize, u64, u64), bool>>,
    /// The first point where the kernel could not tell, with its reason.
    refused: Mutex<Option<String>>,
    /// Calls of the kernel's classifier made, for the tests.
    #[cfg(test)]
    asked: std::sync::atomic::AtomicUsize,
}

impl Face {
    pub(crate) fn new(eval: EvalSurface, tolerance: f64) -> Face {
        let kernel = kernel_form(&eval);
        Face {
            eval,
            kernel,
            tolerance: if tolerance.is_finite() { tolerance.max(0.0) } else { 0.0 },
            collapsed: Mutex::new(BitsMap::default()),
            refused: Mutex::new(None),
            #[cfg(test)]
            asked: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// The plan evaluator's form, for the plan.
    pub(crate) fn into_eval(self) -> EvalSurface {
        self.eval
    }

    /// The kernel's form of the surface (the pole pass's).
    pub(crate) fn kernel(&self) -> &solidcore::eval::EvalSurface {
        &self.kernel
    }

    /// Whether the surface is collapsed along `d` at `q`, or within `reach` mm of it, as the
    /// kernel's classifier says. Where it cannot tell, the face is refused ([`Face::refusal`]) and
    /// the answer is no.
    pub(crate) fn pole(&self, q: [f64; 2], d: usize, reach: f64) -> bool {
        match self.classified(q, d, reach) {
            Ok(answer) => answer,
            Err(why) => {
                let mut slot = self.refused.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if slot.is_none() {
                    *slot = Some(format!("the surface's poles cannot be told at {q:?}: {why}"));
                }
                false
            }
        }
    }

    /// Why the kernel could not classify a point of this face, if it could not.
    pub(crate) fn refusal(&self) -> Option<String> {
        self.refused.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// `q` (wrapped into the surface's period) on the surface's parameter range: moved onto it
    /// where it lies past it by no more than the face's tolerance; the reason where farther.
    fn on_range(&self, q: [f64; 2]) -> Result<[f64; 2], String> {
        let w = self.kernel.wrap(q);
        let mut on = w;
        for (k, x) in on.iter_mut().enumerate() {
            if let Some((lo, hi)) = self.kernel.interval(k) {
                // Max then min: `f64::clamp` panics on a NaN bound.
                *x = x.max(lo.min(hi)).min(lo.max(hi));
            }
        }
        if on == w {
            return Ok(w);
        }
        // How far the node lies off the range, at the surface's speed along each parameter there.
        let Some((_, su, sv)) = self.eval.derivs(on[0], on[1]) else {
            return Err(format!("the surface does not evaluate at {on:?}"));
        };
        let off = (w[0] - on[0]).abs() * su.length() + (w[1] - on[1]).abs() * sv.length();
        if off <= self.tolerance {
            Ok(on)
        } else {
            Err(format!("the node lies about {off:.3e} mm off the surface's parameter range, beyond the face's tolerance of {:.3e} mm", self.tolerance))
        }
    }

    /// The kernel's answer, asked once per iso-line.
    fn classified(&self, q: [f64; 2], d: usize, reach: f64) -> Result<bool, String> {
        if !(q[0].is_finite() && q[1].is_finite()) || d > 1 {
            return solidcore::eval::uv::degenerate_within(&self.kernel, q, d, reach).map_err(crate::eval_message);
        }
        let q = self.on_range(q)?;
        self.asked_on_range(q, d, reach).map_err(crate::eval_message)
    }

    /// [`Self::classified`] at a point on the range.
    fn asked_on_range(&self, q: [f64; 2], d: usize, reach: f64) -> Result<bool, solidcore::eval::EvalError> {
        let ask = || {
            #[cfg(test)]
            self.asked.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            solidcore::eval::uv::degenerate_within(&self.kernel, q, d, reach)
        };
        if matches!(self.eval, EvalSurface::Plane { .. }) {
            return ask();
        }
        let Some(&other) = q.get(1 - d) else { return ask() };
        let key = (d, other.to_bits(), reach.to_bits());
        if let Some(&known) = self.lock().get(&key) {
            return Ok(known);
        }
        let answer = ask()?;
        self.lock().insert(key, answer);
        Ok(answer)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BitsMap<(usize, u64, u64), bool>> {
        // The map holds only finished answers, so one left by a panicking thread is sound.
        self.collapsed.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Calls of the kernel's classifier made so far.
    #[cfg(test)]
    pub(crate) fn asked(&self) -> usize {
        self.asked.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl std::ops::Deref for Face {
    type Target = EvalSurface;

    fn deref(&self) -> &EvalSurface {
        &self.eval
    }
}

/// `s` in the kernel's evaluator form: the same plane, net or revolution, value for value.
pub(crate) fn kernel_form(s: &EvalSurface) -> solidcore::eval::EvalSurface {
    use solidcore::eval::{EvalCurve, EvalSurface as K};
    match s {
        EvalSurface::Plane { origin, du, dv } => K::Plane { origin: *origin, du: *du, dv: *dv },
        EvalSurface::Nurbs { degree, knots, count, ctrl, .. } => K::nurbs(*degree, knots.clone(), *count, ctrl.clone()),
        EvalSurface::Revolved { profile, origin, axis, transform, swap } => K::Revolved {
            profile: EvalCurve::new(profile.degree, profile.knots.clone(), profile.ctrl.clone()),
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

    /// The memo gives the kernel's own answer at every node, and asks the kernel once per
    /// iso-line: on a 9 x 9 grid of each curved face of the coil and of a sphere (whose poles
    /// collapse a whole row), 2 directions x 9 iso-lines x 2 reaches, not 324 questions.
    #[test]
    fn the_pole_classifier_is_asked_once_per_iso_line() {
        for name in ["smlib_coil_lower_half", "sphere_r4"] {
            for face in &crate::fixtures::fixture(name).faces {
                let Ok((s, _)) = super::super::surface::eval_surface(face.parameters(), face.domain) else { panic!("{name}: evaluates") };
                if matches!(s, EvalSurface::Plane { .. }) {
                    continue;
                }
                let f = Face::new(s, 0.0);
                let dom = super::super::uv::domain(&f);
                let [(u0, u1), (v0, v1)] = dom.range.map(|r| r.unwrap_or((0.0, 1.0)));
                let at = |a: f64, b: f64, i: usize| a + (b - a) * i as f64 / 8.0;
                for reach in [0.0, 1e-3] {
                    for i in 0..9 {
                        for j in 0..9 {
                            let q = [at(u0, u1, i), at(v0, v1, j)];
                            for d in 0..2 {
                                let Ok(want) = solidcore::eval::uv::degenerate_within(f.kernel(), q, d, reach) else { panic!("{name}: classifies {q:?}") };
                                assert_eq!(f.pole(q, d, reach), want, "{name} {q:?}");
                            }
                        }
                    }
                }
                assert_eq!(f.asked(), 2 * 9 * 2, "{name}");
                assert!(f.refusal().is_none(), "{name}");
            }
        }
    }

    /// A node a little past a spline's parameter range is asked at its nearest point of the
    /// range when it lies within the face's tolerance, and refuses the face, naming how far off it
    /// is, when it does not: nothing is taken for the answer past the range.
    #[test]
    fn a_node_past_the_range_is_clamped_within_the_tolerance_and_refused_beyond() {
        let face = &crate::fixtures::fixture("smlib_coil_lower_half").faces[0];
        let Ok((s, _)) = super::super::surface::eval_surface(face.parameters(), face.domain) else { panic!("evaluates") };
        let probe = Face::new(s.clone(), 0.0);
        // A direction the surface does not close along, so the node stays past its range.
        let (Some(lo), Some(hi)) = (probe.kernel().interval(0), probe.kernel().interval(1)) else { panic!("a spline's ranges") };
        let ranges = [lo, hi];
        let Some(k) = (0..2).find(|&k| {
            let mut q = [0.5 * (lo.0 + lo.1), 0.5 * (hi.0 + hi.1)];
            q[k] = ranges[k].1 + 1e-6 * (1.0 + ranges[k].1.abs());
            probe.kernel().wrap(q)[k] > ranges[k].1
        }) else {
            panic!("an open direction")
        };
        let mut on = [0.5 * (lo.0 + lo.1), 0.5 * (hi.0 + hi.1)];
        on[k] = ranges[k].1;
        let mut off = on;
        off[k] = ranges[k].1 + 1e-6 * (1.0 + ranges[k].1.abs());
        // Within a tolerance of 1 mm: the answer on the range, no refusal.
        let within = Face::new(s.clone(), 1.0);
        for d in 0..2 {
            assert_eq!(within.pole(off, d, 0.0), probe.pole(on, d, 0.0));
        }
        assert!(within.refusal().is_none(), "{:?}", within.refusal());
        // With no tolerance: refused, with the distance named.
        let beyond = Face::new(s, 0.0);
        assert!(!beyond.pole(off, 0, 0.0));
        let Some(why) = beyond.refusal() else { panic!("refused") };
        assert!(why.contains("off the surface's parameter range"), "{why}");
    }
}
