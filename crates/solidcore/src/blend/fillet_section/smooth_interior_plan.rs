//! Frozen stations and conditional enclosures for the option-A G2 correction.
//!
//! Upstream: none; solidcore #2356. [`super::smooth_interior`] defines the kernels.
//!
//! | Rust | Design |
//! |------|--------|
//! | [`SmoothInteriorPlan`] | stations and widths fixed before net refinement |
//! | [`JumpJet`] | jumps of the full section's sided station series |
//! | [`CorrectionBounds`] | uniform enclosure of the correction and its cubic Hermite error |
//!
//! Deviations: this plan borrows the unchanged definition, preventing stale state
//! after transforms. Widths are rounded inward. Rail checks are numerical build
//! checks, not proofs. Enclosures require independently proved factored jump
//! bounds; no production evaluator or net uses them yet. No recursion.

use super::interval::Ivl;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::surface::procedural_blend::ProceduralBlend;
use crate::{Result, SolidError};

const OP: &str = "blend::fillet_section::smooth_interior_plan";

/// Jumps (after minus before) of the complete section, including its thumbweight.
#[derive(Clone, Copy, Debug)]
pub struct JumpJet {
    /// Jump of `S_s`, `S_st` and `S_stt`, respectively.
    pub first: [Vec3; 3],
    /// Jump of `S_ss`, `S_sst` and `S_sstt`, respectively.
    pub second: [Vec3; 3],
}

/// A station and its fixed maximum window radius in definition coordinates.
#[derive(Clone, Debug)]
pub struct CorrectionStation {
    s: f64,
    lambda: f64,
    rail_jumps: [JumpJet; 2],
}

impl CorrectionStation {
    /// Crossing parameter in the definition's original s coordinates.
    #[must_use]
    pub const fn parameter(&self) -> f64 {
        self.s
    }

    /// Inward-rounded half distance to the nearer original neighbouring station.
    #[must_use]
    pub const fn lambda(&self) -> f64 {
        self.lambda
    }

    /// Measured full section jumps at t = 0 and t = 1, in that order.
    #[must_use]
    pub const fn rail_jumps(&self) -> &[JumpJet; 2] {
        &self.rail_jumps
    }
}

/// Immutable correction planning state; it does not change the borrowed surface.
///
/// The caller supplies the **complete initial** station partition, including
/// endpoints and every crossing/leader/law knot. This API cannot certify that
/// discovery is complete. Refinement must retain this plan rather than recreate
/// it from the refined net's knots. The borrow prevents transforming the support
/// definition or attaching the plan to a different surface.
pub struct SmoothInteriorPlan<'a> {
    definition: &'a ProceduralBlend,
    stations: Vec<CorrectionStation>,
}

impl<'a> SmoothInteriorPlan<'a> {
    /// Freezes widths and checks numerical G2 rail compatibility at each station.
    ///
    /// Requires the existing procedural and sided-evaluation opt-ins. This is an
    /// explicit preparation request, not a flag enabling corrected evaluation.
    /// No threshold is supplied by the caller: checks use 512 machine epsilons
    /// times the measured jet scale. Passing them does not prove exact vanishing
    /// of rational coefficients, or the absence of an interior singularity.
    ///
    /// # Errors
    /// Refuses non-G2 definitions, missing opt-ins, invalid partitions,
    /// unrepresentable widths, nonfinite/singular rail jets, or rail jumps that
    /// violate the required vanishing orders. Propagates station solve errors.
    pub fn prepare(k: &Kernel, definition: &'a ProceduralBlend, initial: &[f64]) -> Result<Self> {
        if !k.fixes.fillet_round_as_definition || !k.fixes.fillet_round_on_round_as_definition {
            return Err(SolidError::invalid_input(
                OP,
                "correction preparation requires procedural and sided definition opt-ins",
            ));
        }
        if definition.order() != 2 {
            return Err(SolidError::invalid_input(OP, "this correction plan supports G2 only; G3 needs a separate third-order face-smoothness proof"));
        }
        if initial.len() < 2
            || initial.iter().any(|x| !x.is_finite())
            || initial.windows(2).any(|w| match w {
                [a, b] => a >= b,
                _ => true,
            })
        {
            return Err(SolidError::invalid_input(
                OP,
                "initial stations must be finite and strictly increasing, including both endpoints",
            ));
        }
        let domain = definition.leader().natural_interval();
        if initial.first().is_none_or(|s| *s < domain.min())
            || initial.last().is_none_or(|s| *s > domain.max())
        {
            return Err(SolidError::invalid_input(
                OP,
                "initial station partition lies outside the leader domain",
            ));
        }
        let mut stations = Vec::new();
        for window in initial.windows(3) {
            let [a, s, b] = window else { continue };
            let left = (Ivl::from(*s) - Ivl::from(*a)).lo;
            let right = (Ivl::from(*b) - Ivl::from(*s)).lo;
            let lambda = (Ivl::from(left.min(right)) * Ivl::from(0.5)).lo;
            if !lambda.is_finite() || lambda <= 0.0 {
                return Err(SolidError::invalid_input(
                    OP,
                    "station spacing cannot represent a positive correction width",
                ));
            }
            let rail_jumps = [
                checked_rail(k, definition, *s, 0.0)?,
                checked_rail(k, definition, *s, 1.0)?,
            ];
            stations.push(CorrectionStation {
                s: *s,
                lambda,
                rail_jumps,
            });
        }
        Ok(Self {
            definition,
            stations,
        })
    }

    /// Stations in the original deterministic partition order.
    #[must_use]
    pub fn stations(&self) -> &[CorrectionStation] {
        &self.stations
    }

    /// Takes the full sided section jumps at an interior station and section height.
    ///
    /// # Errors
    /// Invalid station index/height, missing definition opt-ins, or a solve error.
    pub fn jump_at(&self, k: &Kernel, station: usize, t: f64) -> Result<JumpJet> {
        if !k.fixes.fillet_round_as_definition
            || !k.fixes.fillet_round_on_round_as_definition
            || !t.is_finite()
            || !(0.0..=1.0).contains(&t)
        {
            return Err(SolidError::invalid_input(
                OP,
                "procedural/sided definition opt-ins and a height in [0,1] required",
            ));
        }
        let station = self
            .stations
            .get(station)
            .ok_or_else(|| SolidError::invalid_input(OP, "unknown correction station"))?;
        let a = self
            .definition
            .evaluate_definition_sided(k, station.s, t, 2, 2, true)?;
        let b = self
            .definition
            .evaluate_definition_sided(k, station.s, t, 2, 2, false)?;
        jumps(&a, &b)
    }
}

fn entry(d: &[Vec<Vec3>], s: usize, t: usize) -> Result<Vec3> {
    let v = d
        .get(s)
        .and_then(|r| r.get(t))
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP, "missing station partial"))?;
    if [v.x, v.y, v.z].iter().any(|v| !v.is_finite()) {
        return Err(SolidError::invalid_input(OP, "nonfinite station partial"));
    }
    Ok(v)
}

fn jumps(a: &[Vec<Vec3>], b: &[Vec<Vec3>]) -> Result<JumpJet> {
    let j = JumpJet {
        first: [
            entry(b, 1, 0)? - entry(a, 1, 0)?,
            entry(b, 1, 1)? - entry(a, 1, 1)?,
            entry(b, 1, 2)? - entry(a, 1, 2)?,
        ],
        second: [
            entry(b, 2, 0)? - entry(a, 2, 0)?,
            entry(b, 2, 1)? - entry(a, 2, 1)?,
            entry(b, 2, 2)? - entry(a, 2, 2)?,
        ],
    };
    if j.first
        .iter()
        .chain(&j.second)
        .any(|v| [v.x, v.y, v.z].iter().any(|x| !x.is_finite()))
    {
        return Err(SolidError::invalid_input(
            OP,
            "station jump exceeds finite range",
        ));
    }
    Ok(j)
}

fn checked_rail(k: &Kernel, pb: &ProceduralBlend, s: f64, t: f64) -> Result<JumpJet> {
    let a = pb.evaluate_definition_sided(k, s, t, 2, 2, true)?;
    let b = pb.evaluate_definition_sided(k, s, t, 2, 2, false)?;
    check_rail_jets(&a, &b)
}

fn check_rail_jets(a: &[Vec<Vec3>], b: &[Vec<Vec3>]) -> Result<JumpJet> {
    let j = jumps(a, b)?;
    let mut scale = 1.0_f64;
    for d in [a, b] {
        for row in d {
            for v in row {
                scale = scale.max(v.x.abs()).max(v.y.abs()).max(v.z.abs());
            }
        }
    }
    let tol = 512.0 * f64::EPSILON * scale;
    for d in [a, b] {
        let normal = entry(d, 1, 0)?.cross(entry(d, 0, 1)?);
        let length = normal.length();
        if !length.is_finite() || length <= 0.0 {
            return Err(SolidError::invalid_input(
                OP,
                "singular rail station; cannot establish G2 contact",
            ));
        }
        let n = normal / length;
        // Delta = O(t), Delta.N = O(t^2), Delta2.N = O(t).
        // Also require the original section's position and t-jets to agree.
        for q in 0..=2 {
            if (entry(b, 0, q)? - entry(a, 0, q)?).length() > tol {
                return Err(SolidError::invalid_input(
                    OP,
                    "section rail jets are discontinuous; G2 correction cannot preserve contact",
                ));
            }
        }
        if j.first.first().is_none_or(|v| v.length() > tol)
            || j.first.get(1).is_none_or(|v| n.dot(*v).abs() > tol)
            || j.second.first().is_none_or(|v| n.dot(*v).abs() > tol)
        {
            return Err(SolidError::invalid_input(OP, "rail jump Taylor orders do not preserve G2 contact; smoother support faces are required"));
        }
    }
    Ok(j)
}

/// Conditional componentwise enclosures, requiring proved bounds on the jump rows.
#[derive(Clone, Copy, Debug)]
pub struct CorrectionBounds {
    /// Enclosure of Delta G + Delta2 G2 (the correction subtracted from S).
    pub value: [Ivl; 3],
    /// Enclosure of its first s derivative.
    pub first: [Ivl; 3],
    /// Enclosure of its second s derivative on either side of the station.
    pub second: [Ivl; 3],
    /// Enclosure of Herm[correction] - correction on a span not crossing the station.
    pub hermite_error: [Ivl; 3],
}

impl CorrectionStation {
    /// Encloses the correction and cubic Hermite residual uniformly over section t.
    ///
    /// `first_over_r` must enclose Delta(t) / r, r=t(1-t), **by proof**, not
    /// samples, over `t`. `second` must enclose Delta2(t). No normal projection or
    /// regularity/contact guarantee follows from these componentwise bounds.
    ///
    /// On either side of the station, `|G_dd| <= 3/(4 eps)`, and
    /// `|G2_dd| <= 7/8`. Since eps=4 lambda r, the apparent singularity cancels:
    /// `|(Delta G)_ss| <= 3 |first_over_r|/(16 lambda)`.
    /// A C1 function with Lipschitz derivative bounded by M has cubic Hermite
    /// error <= M h^2/4 (linear interpolation error plus endpoint slope errors).
    /// This remains valid across the C2 window edges and through eps -> 0.
    /// Hermite endpoint slopes at the station must be taken from the span's side.
    /// It is conservative O(h^2), not the future sixth-derivative certificate.
    ///
    /// # Errors
    /// Invalid/nonfinite intervals, a span crossing the station, or overflow.
    pub fn enclose(
        &self,
        span: Ivl,
        t: Ivl,
        first_over_r: [Ivl; 3],
        second: [Ivl; 3],
    ) -> Result<CorrectionBounds> {
        for q in [span, t].into_iter().chain(first_over_r).chain(second) {
            if !q.lo.is_finite() || !q.hi.is_finite() || q.lo > q.hi {
                return Err(SolidError::invalid_input(
                    OP,
                    "finite ordered enclosure intervals required",
                ));
            }
        }
        if t.lo < 0.0 || t.hi > 1.0 || (span.lo < self.s && span.hi > self.s) {
            return Err(SolidError::invalid_input(
                OP,
                "height must lie in [0,1] and span must not cross a station",
            ));
        }
        let zero = [Ivl::from(0.0); 3];
        let mut out = CorrectionBounds {
            value: zero,
            first: zero,
            second: zero,
            hermite_error: zero,
        };
        let r = t * (Ivl::from(1.0) - t);
        let r = Ivl::new(0.0, r.hi.min(0.25));
        let eps = Ivl::from(4.0) * Ivl::from(self.lambda) * r;
        let d = span - Ivl::from(self.s);
        if (t.lo == t.hi && (t.lo == 0.0 || t.lo == 1.0)) || d.lo > eps.hi || d.hi < -eps.hi {
            return Ok(out);
        }
        let g = Ivl::new(-3.0 / 16.0, 0.0) * eps;
        let gd = Ivl::new(-0.5, 0.5);
        let gdd_r = Ivl::new(-3.0 / 16.0, 0.0) / Ivl::from(self.lambda);
        let g2 = Ivl::new(-3.0 / 32.0, 3.0 / 32.0) * eps * eps;
        let g2d = Ivl::new(-11.0 / 32.0, 11.0 / 32.0) * eps;
        let g2dd = Ivl::new(-7.0 / 8.0, 7.0 / 8.0);
        let h = Ivl::from(span.hi) - Ivl::from(span.lo);
        for (i, (a, b)) in first_over_r.into_iter().zip(second).enumerate() {
            let v = a * r * g + b * g2;
            let ds = a * r * gd + b * g2d;
            let dss = a * gdd_r + b * g2dd;
            let m = Ivl::from(a.mag()) * Ivl::from(3.0 / 16.0) / Ivl::from(self.lambda)
                + Ivl::from(b.mag()) * Ivl::from(7.0 / 8.0);
            let error = (m * h * h * Ivl::from(0.25)).hi;
            for q in [v, ds, dss] {
                if !q.lo.is_finite() || !q.hi.is_finite() || !error.is_finite() {
                    return Err(SolidError::invalid_input(
                        OP,
                        "correction enclosure exceeds finite range",
                    ));
                }
            }
            if let Some(x) = out.value.get_mut(i) {
                *x = v;
            }
            if let Some(x) = out.first.get_mut(i) {
                *x = ds;
            }
            if let Some(x) = out.second.get_mut(i) {
                *x = dss;
            }
            if let Some(x) = out.hermite_error.get_mut(i) {
                *x = Ivl::new(-error, error);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_section::smooth_interior::correction_kernels;

    fn station() -> CorrectionStation {
        let j = JumpJet {
            first: [Vec3::ZERO; 3],
            second: [Vec3::ZERO; 3],
        };
        CorrectionStation {
            s: 0.5,
            lambda: 0.125,
            rail_jumps: [j; 2],
        }
    }

    fn contains(i: Ivl, v: f64) {
        assert!(i.lo <= v && v <= i.hi, "{v:e} outside {i:?}");
    }

    #[test]
    fn uniform_bounds_enclose_kernels_and_hermite_residual_through_shrinking_windows() {
        let st = station();
        let av = [1.0, -2.0, 0.5];
        let bv = [0.2, -0.5, 1.0];
        let a = av.map(Ivl::from);
        let b = bv.map(Ivl::from);
        // Both sides, adjacent and non-adjacent refined spans, including spans
        // straddling a moving window edge. Coefficients are exact polynomials,
        // so the input enclosures here hold by construction, not measurement.
        for (lo, hi) in [(0.5, 0.51), (0.49, 0.5), (0.51, 0.54), (0.4, 0.48)] {
            let span = Ivl::new(lo, hi);
            let enclosure = st.enclose(span, Ivl::new(0.0, 1.0), a, b).unwrap();
            for t in [1e-15, 1e-10, 1e-5, 0.01, 0.2, 0.5, 0.99, 1.0 - 1e-10] {
                let r = t * (1.0 - t);
                let eps = 4.0 * st.lambda * r;
                let jet = |s: f64, i: usize| {
                    let (g, g2) = correction_kernels(s - st.s, eps, hi <= st.s).unwrap();
                    [
                        av[i] * r * g.value + bv[i] * g2.value,
                        av[i] * r * g.first + bv[i] * g2.first,
                        av[i] * r * g.second + bv[i] * g2.second,
                    ]
                };
                let h = hi - lo;
                for q in 0..=32 {
                    let u = f64::from(q) / 32.0;
                    let s = lo + h * u;
                    for (i, _) in av.iter().enumerate() {
                        let p = jet(s, i);
                        contains(enclosure.value[i], p[0]);
                        contains(enclosure.first[i], p[1]);
                        contains(enclosure.second[i], p[2]);
                        let l = jet(lo, i);
                        let rr = jet(hi, i);
                        let herm = (2.0 * u.powi(3) - 3.0 * u * u + 1.0) * l[0]
                            + (u.powi(3) - 2.0 * u * u + u) * h * l[1]
                            + (-2.0 * u.powi(3) + 3.0 * u * u) * rr[0]
                            + (u.powi(3) - u * u) * h * rr[1];
                        contains(enclosure.hermite_error[i], herm - p[0]);
                    }
                }
            }
        }
    }

    #[test]
    fn hermite_error_bound_converges_quadratically_without_inverse_rail_width() {
        let st = station();
        let a = [Ivl::new(-2.0, 3.0); 3];
        let b = [Ivl::new(-1.0, 1.0); 3];
        let full = st
            .enclose(Ivl::new(0.5, 0.625), Ivl::new(0.0, 1.0), a, b)
            .unwrap();
        let half = st
            .enclose(Ivl::new(0.5, 0.5625), Ivl::new(0.0, 1.0), a, b)
            .unwrap();
        assert!((full.hermite_error[0].hi / half.hermite_error[0].hi - 4.0).abs() < 1e-12);
        let tiny = st
            .enclose(Ivl::new(0.5, 0.625), Ivl::new(0.0, 1e-20), a, b)
            .unwrap();
        assert!(tiny.value[0].mag() < 1e-38);
        assert!(tiny.first[0].mag() < 1e-19);
        assert!(tiny.second[0].mag().is_finite());
    }

    #[test]
    fn exact_rails_and_disjoint_windows_have_zero_correction() {
        let st = station();
        for (span, t) in [
            (Ivl::new(0.5, 0.6), Ivl::from(0.0)),
            (Ivl::new(0.4, 0.5), Ivl::from(1.0)),
            (Ivl::new(0.8, 0.9), Ivl::new(0.0, 1.0)),
        ] {
            let e = st
                .enclose(span, t, [Ivl::from(1.0); 3], [Ivl::from(1.0); 3])
                .unwrap();
            assert_eq!(e.value, [Ivl::from(0.0); 3]);
            assert_eq!(e.first, [Ivl::from(0.0); 3]);
            assert_eq!(e.second, [Ivl::from(0.0); 3]);
            assert_eq!(e.hermite_error, [Ivl::from(0.0); 3]);
        }
    }

    #[test]
    fn invalid_enclosures_and_spans_crossing_the_station_are_refused() {
        let st = station();
        let z = [Ivl::from(0.0); 3];
        assert!(st
            .enclose(Ivl::new(0.4, 0.6), Ivl::new(0.0, 1.0), z, z)
            .is_err());
        assert!(st
            .enclose(Ivl::new(0.5, 0.6), Ivl::new(-0.1, 1.0), z, z)
            .is_err());
        assert!(st
            .enclose(Ivl::new(0.6, 0.5), Ivl::new(0.0, 1.0), z, z)
            .is_err());
        assert!(st
            .enclose(Ivl::new(0.5, 0.6), Ivl::new(0.0, 1.0), [Ivl::whole(); 3], z)
            .is_err());
    }

    #[test]
    fn rail_check_refuses_each_missing_vanishing_order() {
        let mut a = vec![vec![Vec3::ZERO; 3]; 3];
        a[1][0] = Vec3::X;
        a[0][1] = Vec3::Y;
        assert!(check_rail_jets(&a, &a).is_ok());
        for (s, t, v) in [
            (1, 0, Vec3::Y),
            (1, 1, Vec3::Z),
            (2, 0, Vec3::Z),
            (0, 2, Vec3::Z),
        ] {
            let mut b = a.clone();
            b[s][t] += v * 1e-6;
            assert!(check_rail_jets(&a, &b).is_err(), "{s},{t}");
        }
        let mut b = a.clone();
        b[1][1] = Vec3::X;
        b[2][0] = Vec3::Y;
        // Permitted tangential jumps must remain available to the correction.
        assert!(check_rail_jets(&a, &b).is_ok());
        b[1][0] = Vec3::ZERO;
        assert!(check_rail_jets(&b, &b).is_err());
        b[1][0] = Vec3::new(f64::NAN, 0.0, 0.0);
        assert!(check_rail_jets(&b, &b).is_err());
    }
}
