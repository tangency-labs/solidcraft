//! Adaptive Cash–Karp integration for curve lengths and geometric properties.
//!
//! Upstream: `source/SMLib/inc/SmIntegrator.h`, `source/SMLib/src/SmIntegrator.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`integrate`], [`integrate_with_kernel`], [`integrate_opt_in`] | `SmIntegrator::IntegrateIt`, `SmODEEval` |
//! | [`OdeIntegrator`] | `SmODEIntegrator` |
//! | [`OdeIntegrator::integrate`] | `RungeKuttaODEIntegrate` |
//! | [`OdeIntegrator::step`] | `RungeKutta5thOrderStep`, `RungeKuttaCashKarpAdvance` |
//!
//! Deviations: closures replace evaluator subclasses; component arrays are zero-based;
//! out-parameters become return values. Like upstream, integration starts at zero
//! regardless of the initial values (except its early return), and scalar integration
//! returns zero on numerical failure by default; with
//! [`crate::kernel::UpstreamFixes::integrator_propagates_failure`] / [`integrate_opt_in`]
//! (WR-94) failures from [`OdeIntegrator::integrate`] propagate instead, while intervals
//! shorter than `EFF_ZERO` still return zero without evaluating. Unported callees always
//! propagate. Algorithm
//! selection is ignored upstream. Disabled Romberg/Simpson and Bulirsch–Stoer paths
//! are not exposed. No recursion; dimensions are bounded at 4096, steps at 1,000,000,
//! and rejected step retries at 1024 to bound malformed caller input. Infinite error
//! scales/accuracy and unused initial values retain their upstream behavior. The step
//! arrays (`ytemp`, `ak2`..`ak6`, `yerr`) are kept across the steps of one integration
//! instead of allocated per step; they are zeroed or overwritten as the fresh ones
//! were, so the values are the same.

use crate::geom::tol::are_same;
use crate::math::{EFF_ZERO, EFF_ZERO_SQ};
use crate::{Kernel, Result, SolidError};

const OP: &str = "mass::integrator";
/// The operation an ODE integration's `SM_ERR` names when its step control fails: the
/// step underflows, or the steps or retries run out (not upstream: upstream's status is
/// `SM_ERR` for every failure; the code is the same, the name says which).
pub(crate) const OP_STEP_CONTROL: &str = "mass::integrator::step_control";
/// The operation an ODE integration's `SM_ERR` names when the integrand is not finite.
pub(crate) const OP_INTEGRAND: &str = "mass::integrator::integrand";
/// A step's refusal of an accuracy below its error's rounding: its step control cannot
/// meet it.
const UNACHIEVABLE: &str = "unachievable accuracy";
const MAX_COMPONENTS: usize = 4096;
const MAX_STEPS: u32 = 1_000_000;
const MAX_RETRIES: u32 = 1024;

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
    /// The derivative reads only `x`, never `y`: the integration is a quadrature. Each
    /// step's error is then also estimated from the fifth divided difference of its six
    /// samples ([`quadrature_estimate`]), not only from the embedded fourth-order solution;
    /// the step is accepted on the larger.
    pub quadrature: bool,
    /// Optional bounds, one per component, on the absolute rounding error of each
    /// derivative sample (the evaluator's, from its data). With them a step whose
    /// estimate is above the accuracy but within its own rounding is accepted, its error
    /// owed to a final check against the integral: within the accuracy, or the
    /// integration is refused as unachievable ([`estimate_rounding`]). `None` (the
    /// rounding is not known): upstream's step control.
    pub rounding: Option<&'a [f64]>,
    /// The integral's size per unit of `x` over the whole range it is part of, when
    /// known (`r >= 0`). Each step of `h` is then held to the accuracy relative to its
    /// share of the range, `|h| r`, plus its own increment `|y_(k+1) - y_k|` (error per
    /// unit step), instead of upstream's scale `|y| + |h y'|`, the integral so far, and
    /// the owed errors to the accuracy relative to `r |x2 - x1| + |y|`. Against the
    /// integral so far, each of `N` steps may err by the accuracy relative to nearly all
    /// of it, and together by up to `N` times the accuracy; against their shares, the
    /// steps' errors add up to at most the accuracy times `r |x2 - x1| + ∫ |y'|`, over
    /// the whole range `r` was measured on at most twice the integral when `r` is a lower
    /// bound on its mean and the integrand is non-negative, as a curve's speed is
    /// ([`integrate_rounded_with_kernel`]). A lower bound in `scale` still floors each
    /// step's scale.
    pub rate: Option<f64>,
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
            || options.rate.is_some_and(|r| !(r.is_finite() && r >= 0.0))
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
        if let Some(rounding) = options.rounding {
            validate_rounding(rounding, initial.len())?;
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
        // per component, the errors of the steps accepted through the rounding floor
        let mut debt = vec![0.0; initial.len()];
        for _ in 0..self.max_steps {
            evaluate(x, &y, &mut dydx)?;
            finite(&dydx)?;
            for (i, s) in scale.iter_mut().enumerate() {
                // (with `rate`, the step's share of the range and its own increment are
                // added to this floor in `step_in`, for the step it takes)
                *s = if options.rate.is_some() {
                    self.tiny
                } else {
                    at(&y, i)?.abs() + (at(&dydx, i)? * h).abs() + self.tiny
                };
                if let Some(lower) = options.scale {
                    *s = s.max(at(lower, i)?);
                }
            }
            // the step's end: `x2` itself for the final step, so the last node is the
            // interval's end exactly (`x + (x2 - x)` can round past it)
            let mut end = x + h;
            if (end - x2) * (end - x1) > 0.0 {
                h = x2 - x;
                end = x2;
            }
            let step = Self::step_in(
                &y,
                &dydx,
                x,
                h,
                end,
                options.accuracy,
                &mut scale,
                &mut evaluate,
                &mut scratch,
                StepControl {
                    quadrature: options.quadrature,
                    rounding: options.rounding,
                    rate: options.rate,
                },
            )
            .map_err(|e| match e {
                SolidError::Upstream {
                    operation: OP_STEP_CONTROL | OP_INTEGRAND,
                    ..
                } => e,
                // the step control cannot meet the accuracy (upstream: SM_ERR, as below)
                SolidError::InvalidInput {
                    reason: UNACHIEVABLE,
                    ..
                } => SolidError::sm_err(OP_STEP_CONTROL),
                e => SolidError::nl_ser(OP)(e),
            })?;
            if step.taken == h {
                counts.good += 1;
            } else {
                counts.reduced += 1;
            }
            x = step.x;
            for (total, owed) in debt.iter_mut().zip(&scratch.debt) {
                *total += owed;
            }
            // the step's values (`scratch.temp`, which the next step overwrites)
            core::mem::swap(&mut y, &mut scratch.temp);
            if (x - x2) * (x2 - x1) >= 0.0 {
                // The steps accepted through the rounding floor: their errors, together,
                // within the accuracy relative to the integral (with `rate`, plus the
                // range's share; or its scale's lower bound), or the accuracy is below what
                // the integrand resolves.
                for (i, owed) in debt.iter().enumerate() {
                    let mut size = at(&y, i)?.abs();
                    if let Some(rate) = options.rate {
                        size += rate * (x2 - x1).abs();
                    }
                    if let Some(lower) = options.scale {
                        size = size.max(at(lower, i)?);
                    }
                    if *owed > options.accuracy * size {
                        return Err(SolidError::sm_err(OP_STEP_CONTROL));
                    }
                }
                initial.copy_from_slice(&y);
                return Ok(counts);
            }
            if step.next.abs() <= options.minimum_step {
                return Err(SolidError::sm_err(OP_STEP_CONTROL));
            }
            h = step.next;
        }
        Err(SolidError::sm_err(OP_STEP_CONTROL))
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
            x + htry,
            accuracy,
            scale,
            &mut evaluate,
            &mut scratch,
            StepControl {
                quadrature: false,
                rounding: None,
                rate: None,
            },
        )?;
        Ok(OdeStep {
            values: core::mem::take(&mut scratch.temp),
            x: step.x,
            taken: step.taken,
            next: step.next,
        })
    }

    /// [`OdeIntegrator::step`] into `scratch`: the new values are left in
    /// `scratch.temp`, so the steps of one integration allocate nothing. `end` is the
    /// end of a step of `htry` (the step's last node, exactly; a smaller retry ends at
    /// `x + h`). `control` as [`OdeOptions::quadrature`], [`OdeOptions::rounding`] and
    /// [`OdeOptions::rate`]: a step whose estimate is above the accuracy but within its
    /// own rounding is accepted, its error owed (`scratch.debt`, [`estimate_rounding`]).
    #[allow(clippy::too_many_arguments)] // `step`'s arguments plus the scratch arrays
    fn step_in<F>(
        y: &[f64],
        dydx: &[f64],
        x: f64,
        htry: f64,
        end: f64,
        accuracy: f64,
        scale: &mut [f64],
        evaluate: &mut F,
        scratch: &mut CashKarpScratch,
        control: StepControl<'_>,
    ) -> Result<StepEnd>
    where
        F: FnMut(f64, &[f64], &mut [f64]) -> Result<()>,
    {
        validate(y)?;
        validate(dydx)?;
        validate_scale(scale)?;
        if y.len() != dydx.len()
            || y.len() != scale.len()
            || ![x, htry, end].iter().all(|v| v.is_finite())
            || accuracy.is_nan()
        {
            return Err(SolidError::invalid_input(
                OP,
                "invalid step controls or dimensions",
            ));
        }
        let StepControl {
            quadrature,
            rounding,
            rate,
        } = control;
        // (`y.len()` is bounded by `validate` above)
        scratch.resize(y.len());
        let mut h = htry;
        for _ in 0..MAX_RETRIES {
            let step_end = if h == htry { end } else { x + h };
            cash_karp(y, dydx, x, h, step_end, evaluate, scratch)?;
            // Per component: the larger estimate relative to its error scale, and relative
            // to the scale floored at the estimate's rounding `E` (`estimate_rounding`);
            // the step is controlled on the floored ratio. A component accepted only
            // through the floor owes its step's error, at most `2 E` (the estimate is
            // within `E` of the exact samples' and at most `E` itself), to the
            // integration's final check ([`OdeIntegrator::integrate`]).
            let mut errmax: f64 = 0.0;
            let mut embedded_max: f64 = 0.0;
            for i in 0..y.len() {
                let s = scale
                    .get_mut(i)
                    .ok_or_else(|| SolidError::invalid_input(OP, "component missing"))?;
                if s.abs() < EFF_ZERO {
                    *s = EFF_ZERO;
                }
                // this try's share of the range and its own increment
                let s = if let Some(rate) = rate {
                    s.max(h.abs() * rate + (at(&scratch.temp, i)? - at(y, i)?).abs())
                } else {
                    *s
                };
                let embedded = at(&scratch.errors, i)?;
                let samples = step_samples(dydx, scratch, i)?;
                let delta = sample_rounding(accuracy, rounding, i)?;
                let rounding_of = |weights| {
                    delta.map_or(0.0, |delta| estimate_rounding(weights, h, delta, samples))
                };
                let embedded_rounding = rounding_of(EMBEDDED_ERROR_WEIGHTS);
                let mut raw = (embedded / s).abs();
                let mut ratio = (embedded / floored(s, embedded_rounding / accuracy)).abs();
                embedded_max = embedded_max.max(ratio);
                let mut bound = embedded_rounding;
                if quadrature {
                    let estimate = quadrature_estimate(h, samples)?;
                    let rounding = rounding_of(QUADRATURE_ERROR_WEIGHTS);
                    raw = raw.max((estimate / s).abs());
                    ratio = ratio.max((estimate / floored(s, rounding / accuracy)).abs());
                    bound = bound.max(rounding);
                }
                errmax = errmax.max(ratio);
                if let Some(debt) = scratch.debt.get_mut(i) {
                    *debt = if raw > accuracy { 2.0 * bound } else { 0.0 };
                }
            }
            if accuracy <= embedded_max * EFF_ZERO {
                return Err(SolidError::invalid_input(OP, UNACHIEVABLE));
            }
            // (the guard above reads the embedded estimate alone: a coarse step's larger
            // fifth-order estimate asks for a smaller step, not for a lower accuracy)
            errmax /= accuracy;
            if errmax <= 1.0 {
                let next = if errmax > 1.89e-4 {
                    0.9 * h * errmax.powf(-0.2)
                } else {
                    5.0 * h
                };
                return Ok(StepEnd {
                    x: step_end,
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
                return Err(SolidError::sm_err(OP_STEP_CONTROL));
            }
        }
        Err(SolidError::sm_err(OP_STEP_CONTROL))
    }
}

/// How [`OdeIntegrator::step_in`] controls a step: [`OdeOptions::quadrature`],
/// [`OdeOptions::rounding`] and [`OdeOptions::rate`].
#[derive(Clone, Copy)]
struct StepControl<'a> {
    quadrature: bool,
    rounding: Option<&'a [f64]>,
    rate: Option<f64>,
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
        Err(SolidError::sm_err(OP_INTEGRAND))
    }
}
fn validate_scale(scale: &[f64]) -> Result<()> {
    if scale.len() > MAX_COMPONENTS || scale.iter().any(|v| v.is_nan()) {
        return Err(SolidError::invalid_input(OP, "invalid error scale"));
    }
    Ok(())
}

fn validate_rounding(rounding: &[f64], n: usize) -> Result<()> {
    if rounding.len() != n || rounding.iter().any(|v| !(v.is_finite() && *v >= 0.0)) {
        return Err(SolidError::invalid_input(OP, "invalid rounding bounds"));
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
    /// Per component, the error a step accepted through the rounding floor may carry
    /// beyond its scale (`2 E`, [`OdeIntegrator::step_in`]); zero otherwise.
    debt: Vec<f64>,
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
            &mut self.debt,
        ] {
            v.resize(n, 0.0);
        }
    }
}

/// One Cash–Karp step into `s`: the fifth-order values in `s.temp`, the error
/// estimates in `s.errors`. `s` is sized to `y.len()`. The step's last node (`c = 1`)
/// is `end`, exactly, and every node lies between `x` and `end`: `x + c h` rounded can
/// land an ulp past the end (`-0.1 + 0.4` is `0.30000000000000004`), where an integrand
/// whose end is a knot would read the span beyond it.
fn cash_karp<F>(
    y: &[f64],
    dydx: &[f64],
    x: f64,
    h: f64,
    end: f64,
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
        errors: _,
        debt: _,
    } = s;
    // (`max` then `min`: no panic; `x` and `end` are ordered as the step's direction)
    let node = |c: f64| {
        let t = x + c * h;
        if h >= 0.0 {
            t.max(x).min(end)
        } else {
            t.min(x).max(end)
        }
    };
    // (zeroed for each evaluation, as the fresh arrays upstream's step allocates; an
    // evaluator may leave components unwritten)
    for ak in [&mut *ak2, &mut *ak3, &mut *ak4, &mut *ak5, &mut *ak6] {
        ak.fill(0.0);
    }
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)? + 0.2 * h * at(dydx, i)?;
    }
    evaluate(node(0.2), temp, ak2)?;
    finite(ak2)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)? + h * ((3.0 / 40.0) * at(dydx, i)? + (9.0 / 40.0) * at(ak2, i)?);
    }
    evaluate(node(0.3), temp, ak3)?;
    finite(ak3)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)? + h * (0.3 * at(dydx, i)? - 0.9 * at(ak2, i)? + 1.2 * at(ak3, i)?);
    }
    evaluate(node(0.6), temp, ak4)?;
    finite(ak4)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)?
            + h * ((-11.0 / 54.0) * at(dydx, i)?
                + 2.5 * at(ak2, i)?
                + (-70.0 / 27.0) * at(ak3, i)?
                + (35.0 / 27.0) * at(ak4, i)?);
    }
    evaluate(end, temp, ak5)?;
    finite(ak5)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)?
            + h * ((1631.0 / 55296.0) * at(dydx, i)?
                + (175.0 / 512.0) * at(ak2, i)?
                + (575.0 / 13824.0) * at(ak3, i)?
                + (44275.0 / 110592.0) * at(ak4, i)?
                + (253.0 / 4096.0) * at(ak5, i)?);
    }
    evaluate(node(0.875), temp, ak6)?;
    finite(ak6)?;
    for (i, t) in temp.iter_mut().enumerate() {
        *t = at(y, i)?
            + h * ((37.0 / 378.0) * at(dydx, i)?
                + (250.0 / 621.0) * at(ak3, i)?
                + (125.0 / 594.0) * at(ak4, i)?
                + (512.0 / 1771.0) * at(ak6, i)?);
    }
    finite(temp)?;
    for i in 0..y.len() {
        let estimate = weighted(EMBEDDED_ERROR_WEIGHTS, h, step_samples(dydx, s, i)?);
        let e = s
            .errors
            .get_mut(i)
            .ok_or_else(|| SolidError::invalid_input(OP, "component missing"))?;
        *e = estimate;
    }
    finite(&s.errors)?;
    Ok(())
}

/// Component `i`'s six samples of the step in `s`, in node order: the derivative at the
/// step's start, then `ak2` to `ak6`.
fn step_samples(dydx: &[f64], s: &CashKarpScratch, i: usize) -> Result<[f64; 6]> {
    Ok([
        at(dydx, i)?,
        at(&s.ak2, i)?,
        at(&s.ak3, i)?,
        at(&s.ak4, i)?,
        at(&s.ak5, i)?,
        at(&s.ak6, i)?,
    ])
}

/// `h sum_j w_j g_j`: a step's estimate of weights `weights` on its samples `samples`
/// ([`step_samples`]), summed in node order. The embedded estimate, the quadrature
/// estimate and their rounding bound ([`estimate_rounding`]) are each one.
fn weighted(weights: [f64; 6], h: f64, samples: [f64; 6]) -> f64 {
    let sum: f64 = weights.iter().zip(samples).map(|(w, g)| w * g).sum();
    h * sum
}

/// The weights of the embedded error estimate (fifth- minus fourth-order Cash–Karp
/// weights) on the samples in order: the derivative at the step's start, then `ak2` to
/// `ak6`.
const EMBEDDED_ERROR_WEIGHTS: [f64; 6] = [
    (37.0 / 378.0) - (2825.0 / 27648.0),
    0.0,
    (250.0 / 621.0) - (18575.0 / 48384.0),
    (125.0 / 594.0) - (13525.0 / 55296.0),
    -277.0 / 14336.0,
    (512.0 / 1771.0) - 0.25,
];
/// The weights of the quadrature estimate on the same samples: `(-1/960)` times the
/// fifth divided difference ([`quadrature_estimate`]).
const QUADRATURE_ERROR_WEIGHTS: [f64; 6] = [
    FIFTH_ORDER_ERROR_ON_S5 * DIVIDED_DIFFERENCE_5[0],
    FIFTH_ORDER_ERROR_ON_S5 * DIVIDED_DIFFERENCE_5[1],
    FIFTH_ORDER_ERROR_ON_S5 * DIVIDED_DIFFERENCE_5[2],
    FIFTH_ORDER_ERROR_ON_S5 * DIVIDED_DIFFERENCE_5[3],
    FIFTH_ORDER_ERROR_ON_S5 * DIVIDED_DIFFERENCE_5[4],
    FIFTH_ORDER_ERROR_ON_S5 * DIVIDED_DIFFERENCE_5[5],
];

/// `gamma_n = n u / (1 - n u)`, `u = 2^-53` the unit roundoff: the bound on the relative
/// error of `n` successive roundings (Higham, "Accuracy and Stability of Numerical
/// Algorithms", 2nd ed., Lemma 3.1).
#[must_use]
pub fn gamma(n: u32) -> f64 {
    let nu = f64::from(n) * (f64::EPSILON / 2.0);
    nu / (1.0 - nu)
}

/// Not upstream: the bound on the rounding error of a step's error estimate
/// `h sum_j w_j g_j` (weights `weights`, [`EMBEDDED_ERROR_WEIGHTS`] or
/// [`QUADRATURE_ERROR_WEIGHTS`]) on the samples `samples` ([`step_samples`]), from the
/// bound `delta` on the absolute rounding error of each sample ([`OdeOptions::rounding`]):
///
/// `E = (1 + gamma_16) |h| sum_j |w_j| (delta + gamma_11 |g_j|)`, `g_j` the computed samples.
///
/// Each computed sample is `g_j` with `|g_j - g*_j| <= delta + gamma_3 |g*_j|` against
/// the exact `g*_j` (the caller's absolute part, plus up to three roundings relative to
/// the sample, as a Euclidean norm takes), so `gamma_3 |g*_j| <= gamma_4 (|g_j| + delta)`.
/// The estimate ([`weighted`]: six products, five additions and the product with `h`)
/// rounds by at most `gamma_7 |h| sum_j |w_j g_j|` (Higham, eq. 3.5), and
/// `gamma_4 + gamma_7 <= gamma_11`. That bound, `|h| sum_j |w_j| ((1 + gamma_4) delta +
/// gamma_11 |g_j|)`, is itself evaluated in at most ten roundings of non-negative terms:
/// the factor `1 + gamma_16` covers them and the `gamma_4` on `delta`.
/// The computed estimate is then within `E` of the estimate of the exact samples:
/// an estimate at or below `E` cannot be told from zero.
///
/// A step's error scale `s` is floored at `E / accuracy`: a step whose estimate is
/// above `accuracy * s` but within `E` cannot be told from one of zero estimate, and no
/// smaller step resolves it where `s` vanishes with the step (the integral's start at a
/// zero of the integrand: `s = |h g(x)|`). Such a step is accepted, and its error, at
/// most `2 E` (the exact samples' estimate is within `E` of the computed one, at most
/// `E`), is owed: the integration returns its value only when the owed errors of its
/// steps sum within `accuracy` relative to the integral, and is refused as unachievable
/// (`SM_ERR`, [`OP_STEP_CONTROL`]) otherwise. So the floor never loosens the accuracy
/// asked: where the integrand's rounding is below it, as for a speed evaluated in
/// difference form ([`crate::nurbs::b_spline_curve::speed::SpanSpeed`]), it only
/// accepts steps at a zero of the integrand; where its rounding is above it, the
/// integration is refused.
fn estimate_rounding(weights: [f64; 6], h: f64, delta: f64, samples: [f64; 6]) -> f64 {
    let relative = gamma(11);
    (1.0 + gamma(16))
        * weighted(
            weights.map(f64::abs),
            h.abs(),
            samples.map(|g| delta + relative * g.abs()),
        )
}

/// The error scale `s` floored at `floor` when there is one (a zero floor leaves `s`
/// as upstream's, also a negative one a caller's lower bound gave).
fn floored(s: f64, floor: f64) -> f64 {
    if floor > 0.0 {
        s.max(floor)
    } else {
        s
    }
}

/// The bound `delta` on the rounding of component `i`'s samples
/// ([`OdeOptions::rounding`]); `None` without sample rounding bounds (the integrand's
/// rounding is then not known, and the step control is upstream's: no floor) and at a
/// non-positive accuracy, which the step refuses as unachievable.
fn sample_rounding(accuracy: f64, rounding: Option<&[f64]>, i: usize) -> Result<Option<f64>> {
    match rounding {
        Some(rounding) if accuracy > 0.0 => at(rounding, i).map(Some),
        _ => Ok(None),
    }
}

/// The weights of the fifth divided difference on the Cash–Karp nodes `0, 1/5, 3/10, 3/5,
/// 1, 7/8` (in that order: the derivative at the step's start, then `ak2` to `ak6`):
/// `1 / prod_{j != i} (c_i - c_j)`.
const DIVIDED_DIFFERENCE_5: [f64; 6] = [
    -2000.0 / 63.0,
    6250.0 / 27.0,
    -400_000.0 / 1449.0,
    12500.0 / 99.0,
    250.0 / 7.0,
    -4_096_000.0 / 47817.0,
];
/// The error of the fifth-order Cash–Karp weights on `s^5` over `[0, 1]`:
/// `37/378 (3/10)^5 + 250/621 (3/5)^5 + 125/594 + 512/1771 (7/8)^5 - 1/6`.
const FIFTH_ORDER_ERROR_ON_S5: f64 = -1.0 / 960.0;

/// Not upstream: the error of a quadrature step's fifth-order value from its samples
/// `samples` ([`step_samples`]).
///
/// For a derivative of `x` alone, a Cash–Karp step is a quadrature rule on six samples
/// of `g(s) = f(x + s h)`: the fifth-order value integrates polynomials of degree 4
/// exactly, and its error is `h (-1/960) g^(5)(xi) / 120 = h (-1/960) g[c_0..c_5]` to
/// leading order, the fifth divided difference of the six samples. The embedded
/// estimate is the difference of the fourth- and fifth-order values, a rule that is
/// zero on cubics and so measures `g^(4)` only: where `g^(4)` changes sign within the
/// step it can vanish while the step's error does not. On a step of half a quarter
/// circle's trim it read 3.1e-5 where the step was off by 3.5e-3. The divided
/// difference measures `g^(5)`; the step is accepted on the larger of the two, which
/// misses only where both vanish together. (A pair of null rules of consecutive
/// degrees: Berntsen and Espelid, "Error estimation in automatic quadrature routines",
/// ACM TOMS 17 (1991).)
fn quadrature_estimate(h: f64, samples: [f64; 6]) -> Result<f64> {
    let error = weighted(QUADRATURE_ERROR_WEIGHTS, h, samples);
    if error.is_finite() {
        Ok(error)
    } else {
        Err(SolidError::sm_err(OP_INTEGRAND))
    }
}

/// `SmIntegrator::IntegrateIt`. Numerical/evaluator failure yields `Ok(0.0)` as
/// upstream does; an unported evaluator is always propagated for differential coverage.
/// Intervals shorter than `EFF_ZERO` return zero without evaluating the function.
///
/// Use [`integrate_with_kernel`] or [`integrate_opt_in`] to honour
/// [`crate::kernel::UpstreamFixes::integrator_propagates_failure`] (WR-94).
pub fn integrate<F>(a: f64, b: f64, accuracy: f64, evaluate: F) -> Result<f64>
where
    F: FnMut(f64) -> Result<f64>,
{
    integrate_opt_in(a, b, accuracy, false, evaluate)
}

/// [`integrate`] reading `k.fixes.integrator_propagates_failure` (WR-94).
pub fn integrate_with_kernel<F>(
    k: &Kernel,
    a: f64,
    b: f64,
    accuracy: f64,
    evaluate: F,
) -> Result<f64>
where
    F: FnMut(f64) -> Result<f64>,
{
    integrate_rounded_with_kernel(k, a, b, accuracy, None, None, evaluate)
}

/// [`integrate_with_kernel`] for an integrand whose samples are each within `rounding`
/// of its exact value, when that is known ([`OdeOptions::rounding`]): its value is within
/// the accuracy by the step control's estimates, or, where the accuracy asks for less
/// than the integrand resolves, the integration is refused as unachievable (`SM_ERR`,
/// [`OP_STEP_CONTROL`]; a zero without failure propagation, as every failure). With
/// `rate`, the integral's size per unit of `x` over the whole range `[a, b]` is part of
/// ([`OdeOptions::rate`]), each step is held to its share of that range plus its own
/// increment, so the steps of every piece of the range err together by at most the
/// accuracy times `rate` × the range plus the integral; `None`: upstream's error scale.
pub fn integrate_rounded_with_kernel<F>(
    k: &Kernel,
    a: f64,
    b: f64,
    accuracy: f64,
    rounding: Option<f64>,
    rate: Option<f64>,
    mut evaluate: F,
) -> Result<f64>
where
    F: FnMut(f64) -> Result<f64>,
{
    k.check_cancelled(OP)?;
    let mut eval_calls = 0u64;
    integrate_scalar(
        a,
        b,
        accuracy,
        rounding,
        rate,
        k.fixes.integrator_propagates_failure,
        |t| {
            eval_calls += 1;
            if (eval_calls & 63) == 1 {
                k.check_cancelled(OP)?;
            }
            evaluate(t)
        },
    )
}

/// `SmIntegrator::IntegrateIt` with explicit failure-propagation control (WR-94).
///
/// Intervals shorter than `EFF_ZERO` return `Ok(0.0)` without evaluating `evaluate`.
/// When `propagate_failure` is `true`, any error returned by
/// [`OdeIntegrator::integrate`] (an evaluator error, non-finite derivative or control,
/// unachievable accuracy, step underflow below `minimum_step`, or exceeding `max_steps`)
/// is returned instead of being hidden as `Ok(0.0)`. [`SolidError::Unported`] always
/// propagates regardless of `propagate_failure`.
///
/// Not upstream: each step is accepted on the larger of the embedded estimate and the
/// fifth divided difference of its samples ([`OdeOptions::quadrature`]); upstream reads
/// the embedded estimate alone, which vanishes where `g''''` changes sign.
pub fn integrate_opt_in<F>(
    a: f64,
    b: f64,
    accuracy: f64,
    propagate_failure: bool,
    evaluate: F,
) -> Result<f64>
where
    F: FnMut(f64) -> Result<f64>,
{
    integrate_scalar(a, b, accuracy, None, None, propagate_failure, evaluate)
}

/// [`integrate_opt_in`] with the samples' rounding bound `rounding` and the integral's
/// size per unit of `x` `rate` ([`OdeOptions::rate`]), when known.
fn integrate_scalar<F>(
    a: f64,
    b: f64,
    accuracy: f64,
    rounding: Option<f64>,
    rate: Option<f64>,
    propagate_failure: bool,
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
        // `evaluate` reads `t` alone: the steps are accepted on the quadrature estimate
        quadrature: true,
        rounding: rounding.as_ref().map(core::slice::from_ref),
        rate,
    };
    match OdeIntegrator::default().integrate(&mut y, a, b, options, |t, _, dy| {
        let value = evaluate(t)?;
        if let Some(d) = dy.first_mut() {
            *d = value;
        }
        Ok(())
    }) {
        Ok(_) => Ok(y.into_iter().next().unwrap_or(0.0)),
        Err(e) if e.is_unported_or_cancelled() || propagate_failure => Err(e),
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
                        quadrature: false,
                        rounding: None,
                        rate: None,
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
                        quadrature: false,
                        rounding: None,
                        rate: None,
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
            quadrature: false,
            rounding: None,
            rate: None,
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
        assert_eq!(integrate(0.0, 1.0, 0.0, |_| Ok(1.0)).unwrap(), 0.0);
        assert!(integrate(0.0, 1.0, 1e-6, |_| Err(SolidError::unported(
            "test", "test"
        )))
        .unwrap_err()
        .is_unported());
    }

    #[test]
    fn wr94_zero_accuracy_and_evaluator_error_propagate_under_opt_in() {
        let k_default = Kernel::default();
        let mut k_fix = Kernel::default();
        k_fix.fixes.integrator_propagates_failure = true;

        // Zero, negative, and NaN accuracy fail inside OdeIntegrator; default hides as 0.0.
        for bad_accuracy in [0.0, -1e-6, f64::NAN] {
            assert_eq!(
                integrate(0.0, 1.0, bad_accuracy, |_| Ok(1.0),).unwrap(),
                0.0
            );
            assert_eq!(
                integrate_opt_in(0.0, 1.0, bad_accuracy, false, |_| Ok(1.0),).unwrap(),
                0.0
            );
            assert_eq!(
                integrate_with_kernel(&k_default, 0.0, 1.0, bad_accuracy, |_| Ok(1.0),).unwrap(),
                0.0
            );
            assert!(integrate_opt_in(0.0, 1.0, bad_accuracy, true, |_| Ok(1.0),).is_err());
            assert!(integrate_with_kernel(&k_fix, 0.0, 1.0, bad_accuracy, |_| Ok(1.0),).is_err());
        }

        // Evaluator failing partway through [0.0, 1.0] and evaluator returning NaN/Inf.
        let partial_err = |t: f64| {
            if t > 0.4 {
                Err(SolidError::sm_err("test"))
            } else {
                Ok(1.0)
            }
        };
        assert_eq!(
            integrate_with_kernel(&k_default, 0.0, 1.0, 1e-6, partial_err,).unwrap(),
            0.0
        );
        assert_eq!(
            integrate_with_kernel(&k_fix, 0.0, 1.0, 1e-6, partial_err,)
                .unwrap_err()
                .sm_status(),
            Some(crate::error::SM_ERR)
        );
        for non_finite in [f64::NAN, f64::INFINITY] {
            assert_eq!(
                integrate_with_kernel(&k_default, 0.0, 1.0, 1e-6, |_| Ok(non_finite),).unwrap(),
                0.0
            );
            assert_eq!(
                integrate_with_kernel(&k_fix, 0.0, 1.0, 1e-6, |_| Ok(non_finite),)
                    .unwrap_err()
                    .sm_status(),
                Some(crate::error::SM_ERR)
            );
        }

        // Unported evaluators always propagate regardless of the flag.
        for k in [&k_default, &k_fix] {
            assert!(
                integrate_with_kernel(k, 0.0, 1.0, 1e-6, |_| Err(SolidError::unported(
                    "test", "test"
                )),)
                .unwrap_err()
                .is_unported()
            );
        }

        // Degenerate intervals shorter than EFF_ZERO return Ok(0.0) without calling evaluate.
        for k in [&k_default, &k_fix] {
            for (a, b) in [(2.0, 2.0), (2.0, 2.0 + 0.5 * EFF_ZERO)] {
                assert_eq!(
                    integrate_with_kernel(k, a, b, 0.0, |_| {
                        panic!("degenerate interval must not evaluate")
                    })
                    .unwrap(),
                    0.0
                );
            }
        }

        // Valid integrals agree bit-for-bit in both modes (including reversed bounds).
        for (a, b, expected) in [(0.0, 2.0, 8.0), (2.0, 0.0, -8.0)] {
            let v_def = integrate_with_kernel(&k_default, a, b, 1e-8, |t| Ok(3.0 * t * t)).unwrap();
            let v_fix = integrate_with_kernel(&k_fix, a, b, 1e-8, |t| Ok(3.0 * t * t)).unwrap();
            assert_eq!(v_def, v_fix);
            assert!((v_def - expected).abs() < 1e-12, "{v_def} vs {expected}");
        }
    }

    #[test]
    fn wr94_curve_length_and_surface_area_propagate_failure() {
        use crate::curve::curve::Curve;
        use crate::curve::ellipse::Ellipse;
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::geom::extent2d::Extent2d;
        use crate::math::{Point3, Vec3, PI, SQRT3};
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::surface::sphere::{Sphere, SphereOptions};
        use crate::surface::surface::p1::AreaIntegrand;
        use crate::surface::surface::Surface;

        let k_default = Kernel::default();
        let mut k_fix = Kernel::default();
        k_fix.fixes.integrator_propagates_failure = true;

        // Curved B-spline-derived Curve::length (radius-2 circle, circumference 4 * PI).
        let circle = Curve::from(
            Ellipse::create_canonical_circle(
                &k_default,
                &Axis2Placement::IDENTITY,
                2.0,
                None,
                None,
            )
            .unwrap(),
        );
        let ivl = circle.natural_interval();
        assert_eq!(circle.length(&k_default, &ivl, 0.0).unwrap(), 0.0);
        assert!(circle.length(&k_fix, &ivl, 0.0).is_err());

        let len_def = circle.length(&k_default, &ivl, 1e-8).unwrap();
        let len_fix = circle.length(&k_fix, &ivl, 1e-8).unwrap();
        assert_eq!(len_def, len_fix);
        assert!(
            (len_def - 4.0 * PI).abs() < 1e-7,
            "circle length {len_def} vs {}",
            4.0 * PI
        );

        // Curved Surface patch via AreaIntegrand::evaluate and outer integrate_with_kernel:
        // a 90 deg x 60 deg patch on a sphere of radius 2 has area PI * sqrt(3).
        let mut sphere = Sphere::new(
            &k_default,
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, 0.0, 90.0, 60.0),
            2.0,
            SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(&k_default).unwrap();
        let surface = Surface::from(sphere);
        let dom = surface.natural_uv_domain_if_built();
        let mid_u = 0.5 * (dom.min().x + dom.max().x);

        let bad_inner = AreaIntegrand {
            surface: &surface,
            uv_domain: dom,
            iso_curve_accuracy: 0.0,
        };
        assert_eq!(bad_inner.evaluate(&k_default, mid_u).unwrap(), 0.0);
        assert!(bad_inner.evaluate(&k_fix, mid_u).is_err());
        assert_eq!(
            integrate_with_kernel(&k_default, dom.min().x, dom.max().x, 1e-6, |u| bad_inner
                .evaluate(&k_default, u),)
            .unwrap(),
            0.0
        );
        assert!(
            integrate_with_kernel(&k_fix, dom.min().x, dom.max().x, 1e-6, |u| bad_inner
                .evaluate(&k_fix, u),)
            .is_err()
        );

        // Surface evaluator failure (an uninitialised BSplineSurface without a NURB).
        let missing_nurb = Surface::from(BSplineSurface::new());
        let bad_surface = AreaIntegrand {
            surface: &missing_nurb,
            uv_domain: Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            iso_curve_accuracy: 1e-6,
        };
        assert_eq!(bad_surface.evaluate(&k_default, 0.5).unwrap(), 0.0);
        assert!(bad_surface.evaluate(&k_fix, 0.5).is_err());

        // Valid accuracies yield identical positive strip and patch areas.
        let ok_area = AreaIntegrand {
            surface: &surface,
            uv_domain: dom,
            iso_curve_accuracy: 1e-8,
        };
        let strip_def = ok_area.evaluate(&k_default, mid_u).unwrap();
        let strip_fix = ok_area.evaluate(&k_fix, mid_u).unwrap();
        assert_eq!(strip_def, strip_fix);
        assert!(strip_def > 0.0);

        let area_def = integrate_with_kernel(&k_default, dom.min().x, dom.max().x, 1e-6, |u| {
            ok_area.evaluate(&k_default, u)
        })
        .unwrap();
        let area_fix = integrate_with_kernel(&k_fix, dom.min().x, dom.max().x, 1e-6, |u| {
            ok_area.evaluate(&k_fix, u)
        })
        .unwrap();
        assert_eq!(area_def, area_fix);
        let expected_area = PI * SQRT3;
        assert!(
            (area_def - expected_area).abs() < 1e-5,
            "patch area {area_def} vs {expected_area}"
        );
    }

    #[test]
    fn wr94_flag_lookup() {
        let mut fixes = crate::kernel::UpstreamFixes::default();
        assert!(!fixes.integrator_propagates_failure);
        *fixes.flag_mut("integrator_propagates_failure").unwrap() = true;
        assert!(fixes.integrator_propagates_failure);
        assert_eq!(
            fixes.flag_mut("integrator_propagates_failure").copied(),
            Some(true)
        );
    }

    /// An integration's failures name their cause: a non-finite integrand, and a step
    /// control that cannot meet the accuracy (here: below the rounding of the error), each
    /// `SM_ERR` as upstream's.
    #[test]
    fn a_failure_names_its_cause() {
        let options = |accuracy| OdeOptions {
            accuracy,
            initial_step: 0.1,
            minimum_step: 0.0,
            scale: None,
            quadrature: false,
            rounding: None,
            rate: None,
        };
        let mut y = [0.0];
        let nan = OdeIntegrator::default().integrate(&mut y, 0.0, 1.0, options(1e-6), |x, _, d| {
            if let Some(d) = d.first_mut() {
                *d = if x > 0.5 { f64::NAN } else { 1.0 };
            }
            Ok(())
        });
        assert!(
            matches!(
                nan,
                Err(SolidError::Upstream {
                    operation: OP_INTEGRAND,
                    code: crate::error::SM_ERR,
                    ..
                })
            ),
            "{nan:?}"
        );
        let mut y = [0.0];
        let tight =
            OdeIntegrator::default().integrate(&mut y, 0.0, 1.0, options(1e-300), |x, _, d| {
                if let Some(d) = d.first_mut() {
                    *d = (10.0 * x).sin();
                }
                Ok(())
            });
        assert!(
            matches!(
                tight,
                Err(SolidError::Upstream {
                    operation: OP_STEP_CONTROL,
                    code: crate::error::SM_ERR,
                    ..
                })
            ),
            "{tight:?}"
        );
    }

    /// The fifth divided difference's weights are zero on polynomials of degree 4 and one
    /// on `s^5` (so the quadrature estimate's are `-1/960` there), and the fifth-order
    /// weights miss `s^5` by `-1/960`.
    #[test]
    fn quadrature_error_constants() {
        let nodes = [0.0, 0.2, 0.3, 0.6, 1.0, 0.875];
        let fifth = [
            37.0 / 378.0,
            0.0,
            250.0 / 621.0,
            125.0 / 594.0,
            0.0,
            512.0 / 1771.0,
        ];
        for power in 0..=5 {
            let d: f64 = DIVIDED_DIFFERENCE_5
                .iter()
                .zip(nodes)
                .map(|(w, c): (&f64, f64)| w * c.powi(power))
                .sum();
            let want = if power == 5 { 1.0 } else { 0.0 };
            assert!((d - want).abs() < 1e-9, "s^{power}: {d}");
            let q = weighted(QUADRATURE_ERROR_WEIGHTS, 1.0, nodes.map(|c| c.powi(power)));
            assert!(
                (q - want * FIFTH_ORDER_ERROR_ON_S5).abs() < 1e-12,
                "s^{power}: {q}"
            );
        }
        let miss: f64 = fifth
            .iter()
            .zip(nodes)
            .map(|(w, c): (&f64, f64)| w * c.powi(5))
            .sum::<f64>()
            - 1.0 / 6.0;
        assert!((miss - FIFTH_ORDER_ERROR_ON_S5).abs() < 1e-15, "{miss}");
    }

    /// `s^5 + a s^4`, with `a` making the embedded estimate exactly zero on `[0, 1]`: one
    /// step is accepted with an error of 1/960 unless the step's divided difference is
    /// read too.
    #[test]
    fn a_quadrature_step_is_not_accepted_on_a_blind_embedded_estimate() {
        let nodes = [0.0, 0.2, 0.3, 0.6, 1.0, 0.875];
        let on = |power: i32| -> f64 {
            EMBEDDED_ERROR_WEIGHTS
                .iter()
                .zip(nodes)
                .map(|(w, c): (&f64, f64)| w * c.powi(power))
                .sum()
        };
        let a = -on(5) / on(4);
        let exact = 1.0 / 6.0 + a / 5.0;
        let run = |quadrature: bool| {
            let mut y = [0.0];
            OdeIntegrator::default()
                .integrate(
                    &mut y,
                    0.0,
                    1.0,
                    OdeOptions {
                        accuracy: 1e-6,
                        initial_step: 1.0,
                        minimum_step: 0.0,
                        scale: Some(&[1.0]),
                        quadrature,
                        rounding: None,
                        rate: None,
                    },
                    |t, _, dy| {
                        dy[0] = t.powi(5) + a * t.powi(4);
                        Ok(())
                    },
                )
                .unwrap();
            y[0] - exact
        };
        let blind = run(false);
        assert!((blind + 1.0 / 960.0).abs() < 1e-12, "{blind}");
        let error = run(true);
        assert!(error.abs() <= 1e-6, "{error}");
    }

    /// The final step's last node is the interval's end itself: `-0.1 + (0.3 - -0.1)`
    /// rounds to `0.30000000000000004`, past the end, where an integrand whose end is a
    /// knot (a curve's span, a face's iso-u piece) would read the span beyond it. Every
    /// node lies within `[x1, x2]`, and the step that ends the integration ends at `x2`.
    #[test]
    fn the_final_step_ends_at_the_interval_end_exactly() {
        let (x1, x2) = (-0.1_f64, 0.3_f64);
        assert!(x1 + (x2 - x1) > x2);
        for (a, b) in [(x1, x2), (x2, x1)] {
            let mut y = [0.0];
            let mut outside = Vec::new();
            let mut at_end = false;
            OdeIntegrator::default()
                .integrate(
                    &mut y,
                    a,
                    b,
                    OdeOptions {
                        accuracy: 1e-9,
                        // one step over the whole interval: its h is clamped to b - a
                        initial_step: 1.0,
                        minimum_step: 0.0,
                        scale: None,
                        quadrature: true,
                        rounding: None,
                        rate: None,
                    },
                    |t, _, dy| {
                        if !(x1 <= t && t <= x2) {
                            outside.push(t);
                        }
                        at_end |= t == b;
                        dy[0] = 1.0;
                        Ok(())
                    },
                )
                .unwrap();
            assert!(
                outside.is_empty(),
                "nodes outside [{x1}, {x2}]: {outside:?}"
            );
            assert!(at_end, "no node at the end {b}");
            assert!((y[0] - (b - a)).abs() <= 1e-15, "{}", y[0]);
        }
    }
}
