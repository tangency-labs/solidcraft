//! Adaptive Cash–Karp integration for curve lengths and geometric properties.
//!
//! Upstream: `source/SMLib/inc/SmIntegrator.h`, `source/SMLib/src/SmIntegrator.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`integrate`] | `SmIntegrator::IntegrateIt`, `SmODEEval` |
//! | [`OdeIntegrator`] | `SmODEIntegrator` |
//! | [`OdeIntegrator::integrate`] | `RungeKuttaODEIntegrate` |
//! | [`OdeIntegrator::step`] | `RungeKutta5thOrderStep`, `RungeKuttaCashKarpAdvance` |
//!
//! Deviations: closures replace evaluator subclasses; component arrays are zero-based;
//! out-parameters become return values. Like upstream, integration starts at zero
//! regardless of the initial values (except its early return), and scalar integration
//! returns zero on numerical failure. Unported callees still propagate. Algorithm
//! selection is ignored upstream. Disabled Romberg/Simpson and Bulirsch–Stoer paths
//! are not exposed. No recursion; dimensions are bounded at 4096, steps at 1,000,000,
//! and rejected step retries at 1024 to bound malformed caller input. Infinite error
//! scales/accuracy and unused initial values retain their upstream behavior. The step
//! arrays (`ytemp`, `ak2`..`ak6`, `yerr`) are kept across the steps of one integration
//! instead of allocated per step; they are zeroed or overwritten as the fresh ones
//! were, so the values are the same.

use crate::geom::tol::are_same;
use crate::math::{EFF_ZERO, EFF_ZERO_SQ};
use crate::{Result, SolidError};

const OP: &str = "mass::integrator";
const MAX_COMPONENTS: usize = 4096;
const MAX_STEPS: u32 = 1_000_000;
const MAX_RETRIES: u32 = 1024;

/// Historical selector; both values use Cash–Karp in the pinned upstream.
#[derive(Debug, Clone, Copy, Default)]
pub enum IntegrationAlgorithm {
    /// `SM_IA_ROMBERG`.
    #[default]
    Romberg,
    /// `SM_IA_SIMPSONS`.
    Simpsons,
}

/// Controls of `SmODEIntegrator::RungeKuttaODEIntegrate`.
#[derive(Debug, Clone, Copy)]
pub struct OdeOptions<'a> {
    /// Relative error tolerance (`eps`).
    pub accuracy: f64,
    /// Magnitude of the first step; its sign follows the interval.
    pub initial_step: f64,
    /// Smallest accepted next step (`hmin`, upstream compares against its sign).
    pub minimum_step: f64,
    /// Optional lower bounds for each component's error scale.
    pub scale: Option<&'a [f64]>,
}

/// Accepted full-size and reduced steps (`nok`, `nbad`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepCounts {
    /// Accepted without shrinking the requested step.
    pub good: u32,
    /// Accepted after shrinking it.
    pub reduced: u32,
}

/// One accepted adaptive step.
#[derive(Debug, Clone, PartialEq)]
pub struct OdeStep {
    /// New component values.
    pub values: Vec<f64>,
    /// New independent variable.
    pub x: f64,
    /// Step actually taken.
    pub taken: f64,
    /// Proposed next step.
    pub next: f64,
}

/// Local integrator configuration; it contains no shared or global state.
#[derive(Debug, Clone, Copy)]
pub struct OdeIntegrator {
    max_steps: u32,
    tiny: f64,
}

impl Default for OdeIntegrator {
    fn default() -> Self {
        Self {
            max_steps: 10_000,
            tiny: EFF_ZERO_SQ,
        }
    }
}

impl OdeIntegrator {
    /// Constructor parameters of `SmODEIntegrator` (default: 10000, `EFF_ZERO_SQ`).
    pub fn new(max_steps: u32, tiny: f64) -> Result<Self> {
        if max_steps > MAX_STEPS || !tiny.is_finite() {
            return Err(SolidError::invalid_input(
                OP,
                "invalid step budget or tiny value",
            ));
        }
        Ok(Self { max_steps, tiny })
    }

    /// Integrates into `initial`. The evaluator reads/writes zero-based components.
    /// Initial values are preserved on failure and on the upstream early return;
    /// a nontrivial successful integration replaces them with the integral from zero.
    pub fn integrate<F>(
        &self,
        initial: &mut [f64],
        x1: f64,
        x2: f64,
        options: OdeOptions<'_>,
        mut evaluate: F,
    ) -> Result<StepCounts>
    where
        F: FnMut(f64, &[f64], &mut [f64]) -> Result<()>,
    {
        // Upstream never reads initial values unless it returns before integrating.
        if initial.len() > MAX_COMPONENTS {
            return Err(SolidError::invalid_input(OP, "too many components"));
        }
        if are_same(x1, x2) || are_same(options.initial_step, EFF_ZERO) {
            return Ok(StepCounts::default());
        }
        if ![x1, x2, options.initial_step, options.minimum_step]
            .iter()
            .all(|v| v.is_finite())
            || options.accuracy.is_nan()
        {
            return Err(SolidError::invalid_input(
                OP,
                "non-finite integration control",
            ));
        }
        if let Some(scale) = options.scale {
            validate_scale(scale)?;
            if scale.len() != initial.len() {
                return Err(SolidError::invalid_input(OP, "scale dimension mismatch"));
            }
        }
        let mut y = vec![0.0; initial.len()];
        let mut dydx = y.clone();
        let mut scale = y.clone();
        let mut x = x1;
        let mut h = if x2 - x1 >= 0.0 {
            options.initial_step.abs()
        } else {
            -options.initial_step.abs()
        };
        let mut counts = StepCounts::default();
        // the step arrays, kept across the steps (sized by the first step)
        let mut scratch = CashKarpScratch::default();
        for _ in 0..self.max_steps {
            evaluate(x, &y, &mut dydx)?;
            finite(&dydx)?;
            for (i, s) in scale.iter_mut().enumerate() {
                *s = at(&y, i)?.abs() + (at(&dydx, i)? * h).abs() + self.tiny;
                if let Some(lower) = options.scale {
                    *s = s.max(at(lower, i)?);
                }
            }
            if (x + h - x2) * (x + h - x1) > 0.0 {
                h = x2 - x;
            }
            let step = Self::step_in(
                &y,
                &dydx,
                x,
                h,
                options.accuracy,
                &mut scale,
                &mut evaluate,
                &mut scratch,
            )
            .map_err(SolidError::nl_ser(OP))?;
            if step.taken == h {
                counts.good += 1;
            } else {
                counts.reduced += 1;
            }
            x = step.x;
            // the step's values (`scratch.temp`, which the next step overwrites)
            core::mem::swap(&mut y, &mut scratch.temp);
            if (x - x2) * (x2 - x1) >= 0.0 {
                initial.copy_from_slice(&y);
                return Ok(counts);
            }
            if step.next.abs() <= options.minimum_step {
                return Err(SolidError::sm_err(OP));
            }
            h = step.next;
        }
        Err(SolidError::sm_err(OP))
    }

    /// `RungeKutta5thOrderStep`; also applies the upstream in-place scale floor.
    pub fn step<F>(
        &self,
        y: &[f64],
        dydx: &[f64],
        x: f64,
        htry: f64,
        accuracy: f64,
        scale: &mut [f64],
        mut evaluate: F,
    ) -> Result<OdeStep>
    where
        F: FnMut(f64, &[f64], &mut [f64]) -> Result<()>,
    {
        let mut scratch = CashKarpScratch::default();
        let step = Self::step_in(
            y,
            dydx,
            x,
            htry,
            accuracy,
            scale,
            &mut evaluate,
            &mut scratch,
        )?;
        Ok(OdeStep {
            values: core::mem::take(&mut scratch.temp),
            x: step.x,
            taken: step.taken,
            next: step.next,
        })
    }

    /// [`OdeIntegrator::step`] into `scratch`: the new values are left in
    /// `scratch.temp`, so the steps of one integration allocate nothing.
    #[allow(clippy::too_many_arguments)] // `step`'s arguments plus the scratch arrays
    fn step_in<F>(
        y: &[f64],
        dydx: &[f64],
        x: f64,
        htry: f64,
        accuracy: f64,
        scale: &mut [f64],
        evaluate: &mut F,
        scratch: &mut CashKarpScratch,
    ) -> Result<StepEnd>
    where
        F: FnMut(f64, &[f64], &mut [f64]) -> Result<()>,
    {
        validate(y)?;
        validate(dydx)?;
        validate_scale(scale)?;
        if y.len() != dydx.len()
            || y.len() != scale.len()
            || ![x, htry].iter().all(|v| v.is_finite())
            || accuracy.is_nan()
        {
            return Err(SolidError::invalid_input(
                OP,
                "invalid step controls or dimensions",
            ));
        }
        // (`y.len()` is bounded by `validate` above)
        scratch.resize(y.len());
        let mut h = htry;
        for _ in 0..MAX_RETRIES {
            cash_karp(y, dydx, x, h, evaluate, scratch)?;
            let mut errmax: f64 = 0.0;
            for (error, s) in scratch.errors.iter().zip(scale.iter_mut()) {
                if s.abs() < EFF_ZERO {
                    *s = EFF_ZERO;
                }
                errmax = errmax.max((error / *s).abs());
            }
            if accuracy <= errmax * EFF_ZERO {
                return Err(SolidError::invalid_input(OP, "unachievable accuracy"));
            }
            errmax /= accuracy;
            if errmax <= 1.0 {
                let next = if errmax > 1.89e-4 {
                    0.9 * h * errmax.powf(-0.2)
                } else {
                    5.0 * h
                };
                return Ok(StepEnd {
                    x: x + h,
                    taken: h,
                    next,
                });
            }
            let htemp = 0.9 * h * errmax.powf(-0.25);
            h = if h >= 0.0 {
                htemp.max(0.1 * h)
            } else {
                htemp.min(0.1 * h)
            };
            if !h.is_finite() || are_same(x + h, x) {
                return Err(SolidError::sm_err(OP));
            }
        }
        Err(SolidError::sm_err(OP))
    }
}

fn at(values: &[f64], i: usize) -> Result<f64> {
    values
        .get(i)
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP, "component missing"))
}
fn finite(values: &[f64]) -> Result<()> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(SolidError::sm_err(OP))
    }
}
fn validate_scale(scale: &[f64]) -> Result<()> {
    if scale.len() > MAX_COMPONENTS || scale.iter().any(|v| v.is_nan()) {
        return Err(SolidError::invalid_input(OP, "invalid error scale"));
    }
    Ok(())
}

fn validate(values: &[f64]) -> Result<()> {
    if values.len() > MAX_COMPONENTS {
        return Err(SolidError::invalid_input(OP, "too many components"));
    }
    finite(values)
}

/// The accepted step of [`OdeIntegrator::step_in`] (its values are in the scratch).
struct StepEnd {
    x: f64,
    taken: f64,
    next: f64,
}

/// The arrays of one Cash–Karp step (`RungeKuttaCashKarpAdvance`'s `ytemp`, `ak2` to
/// `ak6` and `yerr`), kept across the steps of one integration. Each step overwrites
/// `temp` and `errors` entirely and zeroes `ak2` to `ak6` before their evaluation, as
/// the fresh zeroed arrays it used to allocate per step.
#[derive(Default)]
struct CashKarpScratch {
    temp: Vec<f64>,
    ak2: Vec<f64>,
    ak3: Vec<f64>,
    ak4: Vec<f64>,
    ak5: Vec<f64>,
    ak6: Vec<f64>,
    errors: Vec<f64>,
}

impl CashKarpScratch {
    /// Sizes every array to `n` components (the caller bounds `n`).
    fn resize(&mut self, n: usize) {
        for v in [
            &mut self.temp,
            &mut self.ak2,
            &mut self.ak3,
            &mut self.ak4,
            &mut self.ak5,
            &mut self.ak6,
            &mut self.errors,
        ] {
            v.resize(n, 0.0);
        }
    }
}

/// One Cash–Karp step into `s`: the fifth-order values in `s.temp`, the error
/// estimates in `s.errors`. `s` is sized to `y.len()`.
fn cash_karp<F>(
    y: &[f64],
    dydx: &[f64],
    x: f64,
    h: f64,
    evaluate: &mut F,
    s: &mut CashKarpScratch,
) -> Result<()>
where
    F: FnMut(f64, &[f64], &mut [f64]) -> Result<()>,
{
    let CashKarpScratch {
        temp,
        ak2,
        ak3,
        ak4,
        ak5,
        ak6,
        errors,
    } = s;
    // (zeroed for each evaluation, as the fresh arrays upstream's step allocates; an
    // evaluator may leave components unwritten)
    for ak in [&mut *ak2, &mut *ak3, &mut *ak4, &mut *ak5, &mut *ak6] {
        ak.fill(0.0);
    }
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)? + 0.2 * h * at(dydx, i)?;
    }
    evaluate(x + 0.2 * h, temp, ak2)?;
    finite(ak2)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)? + h * ((3.0 / 40.0) * at(dydx, i)? + (9.0 / 40.0) * at(ak2, i)?);
    }
    evaluate(x + 0.3 * h, temp, ak3)?;
    finite(ak3)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)? + h * (0.3 * at(dydx, i)? - 0.9 * at(ak2, i)? + 1.2 * at(ak3, i)?);
    }
    evaluate(x + 0.6 * h, temp, ak4)?;
    finite(ak4)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)?
            + h * ((-11.0 / 54.0) * at(dydx, i)?
                + 2.5 * at(ak2, i)?
                + (-70.0 / 27.0) * at(ak3, i)?
                + (35.0 / 27.0) * at(ak4, i)?);
    }
    evaluate(x + 1.0 * h, temp, ak5)?;
    finite(ak5)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)?
            + h * ((1631.0 / 55296.0) * at(dydx, i)?
                + (175.0 / 512.0) * at(ak2, i)?
                + (575.0 / 13824.0) * at(ak3, i)?
                + (44275.0 / 110592.0) * at(ak4, i)?
                + (253.0 / 4096.0) * at(ak5, i)?);
    }
    evaluate(x + 0.875 * h, temp, ak6)?;
    finite(ak6)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)?
            + h * ((37.0 / 378.0) * at(dydx, i)?
                + (250.0 / 621.0) * at(ak3, i)?
                + (125.0 / 594.0) * at(ak4, i)?
                + (512.0 / 1771.0) * at(ak6, i)?);
    }
    for (i, e) in errors.iter_mut().enumerate() {
        *e = h
            * (((37.0 / 378.0) - (2825.0 / 27648.0)) * at(dydx, i)?
                + ((250.0 / 621.0) - (18575.0 / 48384.0)) * at(ak3, i)?
                + ((125.0 / 594.0) - (13525.0 / 55296.0)) * at(ak4, i)?
                + (-277.0 / 14336.0) * at(ak5, i)?
                + ((512.0 / 1771.0) - 0.25) * at(ak6, i)?);
    }
    finite(temp)?;
    finite(errors)?;
    Ok(())
}

/// `SmIntegrator::IntegrateIt`. Numerical/evaluator failure yields `Ok(0.0)` as
/// upstream does; an unported evaluator is always propagated for differential coverage.
/// Intervals shorter than `EFF_ZERO` return zero without evaluating the function.
pub fn integrate<F>(
    a: f64,
    b: f64,
    _algorithm: IntegrationAlgorithm,
    accuracy: f64,
    mut evaluate: F,
) -> Result<f64>
where
    F: FnMut(f64) -> Result<f64>,
{
    if (a - b).abs() < EFF_ZERO {
        return Ok(0.0);
    }
    let mut y = [0.0];
    let options = OdeOptions {
        accuracy,
        initial_step: (b - a) / 10.0,
        minimum_step: (b - a) / 100000000.0,
        scale: None,
    };
    match OdeIntegrator::default().integrate(&mut y, a, b, options, |t, _, dy| {
        let value = evaluate(t)?;
        if let Some(d) = dy.first_mut() {
            *d = value;
        }
        Ok(())
    }) {
        Ok(_) => Ok(y.into_iter().next().unwrap_or(0.0)),
        Err(e) if e.is_unported() => Err(e),
        Err(_) => Ok(0.0),
    }
}

// Precise geometric and mass properties built on the integrator (documented by its
// inner docs, whose links resolve in the module itself).
pub mod properties;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unused_initial_and_infinite_controls_follow_upstream() {
        let integrator = OdeIntegrator::default();
        for (accuracy, scale) in [(1e-6, f64::INFINITY), (f64::INFINITY, 1.0)] {
            let mut y = [f64::NAN];
            integrator
                .integrate(
                    &mut y,
                    0.0,
                    1.0,
                    OdeOptions {
                        accuracy,
                        initial_step: 0.1,
                        minimum_step: 0.0,
                        scale: Some(&[scale]),
                    },
                    |_, _, dy| {
                        dy[0] = 1.0;
                        Ok(())
                    },
                )
                .unwrap();
            assert!((y[0] - 1.0).abs() < 1e-14);
        }
        let mut y = [f64::INFINITY];
        assert_eq!(
            integrator
                .integrate(
                    &mut y,
                    2.0,
                    2.0,
                    OdeOptions {
                        accuracy: f64::NAN,
                        initial_step: f64::NAN,
                        minimum_step: f64::NAN,
                        scale: Some(&[f64::NAN]),
                    },
                    |_, _, _| panic!("early return must not evaluate")
                )
                .unwrap(),
            StepCounts::default()
        );
        assert_eq!(y, [f64::INFINITY]);
    }
    #[test]
    fn safety_and_upstream_quirks() {
        let mut y = [7.0];
        let opt = OdeOptions {
            accuracy: 1e-6,
            initial_step: 0.1,
            minimum_step: 0.0,
            scale: None,
        };
        OdeIntegrator::default()
            .integrate(&mut y, 0.0, 1.0, opt, |_, _, dy| {
                dy[0] = 1.0;
                Ok(())
            })
            .unwrap();
        assert!((y[0] - 1.0).abs() < 1e-14); // initial value is deliberately ignored
        let saved = y;
        assert!(OdeIntegrator::default()
            .integrate(&mut y, 0.0, 1.0, opt, |_, _, dy| {
                dy[0] = f64::NAN;
                Ok(())
            })
            .is_err());
        assert_eq!(y, saved);
        assert!(OdeIntegrator::new(u32::MAX, 0.0).is_err());
        assert!(OdeIntegrator::default()
            .integrate(
                &mut vec![0.0; MAX_COMPONENTS + 1],
                0.0,
                1.0,
                opt,
                |_, _, _| Ok(())
            )
            .is_err());
        assert_eq!(
            integrate(0.0, 1.0, IntegrationAlgorithm::Romberg, 0.0, |_| Ok(1.0)).unwrap(),
            0.0
        );
        assert!(
            integrate(0.0, 1.0, IntegrationAlgorithm::Romberg, 1e-6, |_| Err(
                SolidError::unported("test", "test")
            ))
            .unwrap_err()
            .is_unported()
        );
    }
}
