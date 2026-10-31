//! Time stepping: classical Runge–Kutta 4 with a fixed step, or Dormand–Prince 5(4) with an
//! embedded error estimate, each followed by projection onto the joints (positions, then
//! velocities), landing on every output sample.

use crate::model::{Integrator, MAX_MAGNITUDE, MAX_STEPS, Model, Settings};
use crate::system::System;
use crate::{DynamicsError, Result, Sample, Stats, Trajectory};

/// Dormand–Prince 5(4) tableau (the system is autonomous, so the nodes are not needed): stage
/// weights, fifth-order weights and the difference between the fifth- and fourth-order weights
/// (the error estimate).
const DP_A: [[f64; 6]; 7] = [
    [0.0; 6],
    [1.0 / 5.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0, 0.0],
    [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0, 0.0],
    [19372.0 / 6561.0, -25360.0 / 2187.0, 64448.0 / 6561.0, -212.0 / 729.0, 0.0, 0.0],
    [9017.0 / 3168.0, -355.0 / 33.0, 46732.0 / 5247.0, 49.0 / 176.0, -5103.0 / 18656.0, 0.0],
    [35.0 / 384.0, 0.0, 500.0 / 1113.0, 125.0 / 192.0, -2187.0 / 6784.0, 11.0 / 84.0],
];
const DP_B: [f64; 7] = [35.0 / 384.0, 0.0, 500.0 / 1113.0, 125.0 / 192.0, -2187.0 / 6784.0, 11.0 / 84.0, 0.0];
const DP_E: [f64; 7] = [
    35.0 / 384.0 - 5179.0 / 57600.0,
    0.0,
    500.0 / 1113.0 - 7571.0 / 16695.0,
    125.0 / 192.0 - 393.0 / 640.0,
    -2187.0 / 6784.0 + 92097.0 / 339200.0,
    11.0 / 84.0 - 187.0 / 2100.0,
    -1.0 / 40.0,
];

/// `y + Σ k_i · w_i · h`.
fn combine(y: &[f64], ks: &[Vec<f64>], weights: &[f64], h: f64) -> Vec<f64> {
    let mut out = y.to_vec();
    for (k, w) in ks.iter().zip(weights) {
        if *w == 0.0 {
            continue;
        }
        for (o, d) in out.iter_mut().zip(k) {
            *o += h * w * d;
        }
    }
    out
}

fn at_time(e: DynamicsError, t: f64) -> DynamicsError {
    match e {
        DynamicsError::Singular { .. } => DynamicsError::Singular { time: t },
        DynamicsError::Diverged { .. } => DynamicsError::Diverged { time: t },
        other => other,
    }
}

struct Run<'a> {
    sys: &'a System,
    settings: &'a Settings,
    stats: Stats,
}

impl Run<'_> {
    fn deriv(&self, y: &[f64]) -> Result<Vec<f64>> {
        self.sys.accelerations(y).map(|a| a.ydot)
    }

    /// Normalise, project and check a new state.
    fn finish_step(&mut self, y: &mut [f64], t: f64) -> Result<()> {
        self.sys.normalize(y).map_err(|e| at_time(e, t))?;
        if self.settings.projection {
            self.sys.project_positions(y, self.settings.projection_tol).map_err(|e| at_time(e, t))?;
            self.sys.project_velocities(y).map_err(|e| at_time(e, t))?;
        }
        if !y.iter().all(|x| x.is_finite() && x.abs() < MAX_MAGNITUDE) {
            return Err(DynamicsError::Diverged { time: t });
        }
        Ok(())
    }

    fn count_step(&mut self, t: f64) -> Result<()> {
        if self.stats.steps + self.stats.rejected >= MAX_STEPS {
            return Err(DynamicsError::StepLimit { time: t });
        }
        Ok(())
    }

    fn rk4(&mut self, y: &[f64], h: f64) -> Result<Vec<f64>> {
        let k1 = self.deriv(y)?;
        let k2 = self.deriv(&combine(y, std::slice::from_ref(&k1), &[0.5], h))?;
        let k3 = self.deriv(&combine(y, std::slice::from_ref(&k2), &[0.5], h))?;
        let k4 = self.deriv(&combine(y, std::slice::from_ref(&k3), &[1.0], h))?;
        Ok(combine(y, &[k1, k2, k3, k4], &[1.0 / 6.0, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0], h))
    }

    /// One Dormand–Prince step: the fifth-order state and the scaled error norm.
    fn dopri(&mut self, y: &[f64], h: f64, rel_tol: f64, abs_tol: f64) -> Result<(Vec<f64>, f64)> {
        let mut ks: Vec<Vec<f64>> = Vec::with_capacity(7);
        for a in DP_A.iter().take(6) {
            let stage = combine(y, &ks, a, h);
            ks.push(self.deriv(&stage)?);
        }
        let y5 = combine(y, &ks, &DP_B, h);
        ks.push(self.deriv(&y5)?);
        let err = combine(&vec![0.0; y.len()], &ks, &DP_E, h);
        let n = y.len().max(1) as f64;
        let sum: f64 = err
            .iter()
            .zip(y.iter().zip(&y5))
            .map(|(e, (a, b))| {
                let scale = abs_tol + rel_tol * a.abs().max(b.abs());
                (e / scale).powi(2)
            })
            .sum();
        Ok((y5, (sum / n).sqrt()))
    }

    /// Advance `y` from `t` to `t_end` exactly.
    fn advance(&mut self, y: &mut Vec<f64>, t: &mut f64, t_end: f64, h_next: &mut f64) -> Result<()> {
        let span = t_end - *t;
        let eps = 1e-12 * t_end.abs().max(1.0);
        match self.settings.integrator {
            Integrator::Rk4 { step } => {
                // Equal steps no longer than `step` that land on t_end.
                let n = (span / step).ceil().max(1.0);
                let h = span / n;
                for _ in 0..n as usize {
                    self.count_step(*t)?;
                    let mut next = self.rk4(y, h).map_err(|e| at_time(e, *t))?;
                    *t += h;
                    self.finish_step(&mut next, *t)?;
                    *y = next;
                    self.stats.steps += 1;
                }
                *t = t_end;
            }
            Integrator::Adaptive { rel_tol, abs_tol, min_step, max_step } => {
                while t_end - *t > eps {
                    self.count_step(*t)?;
                    let remaining = t_end - *t;
                    let h = h_next.clamp(min_step, max_step).min(remaining);
                    let (mut next, err) = self.dopri(y, h, rel_tol, abs_tol).map_err(|e| at_time(e, *t))?;
                    // The usual controller: 0.9·err^(−1/5), within [0.2, 5].
                    let factor = if err > 0.0 { (0.9 * err.powf(-0.2)).clamp(0.2, 5.0) } else { 5.0 };
                    if err <= 1.0 {
                        let landed = h >= remaining;
                        *t = if landed { t_end } else { *t + h };
                        self.finish_step(&mut next, *t)?;
                        *y = next;
                        self.stats.steps += 1;
                        // Keep the step chosen before a shortened landing step.
                        if !landed || factor < 1.0 {
                            *h_next = h * factor;
                        }
                    } else {
                        self.stats.rejected += 1;
                        if h <= min_step * (1.0 + 1e-9) {
                            return Err(DynamicsError::StepTooSmall { time: *t });
                        }
                        *h_next = (h * factor).max(min_step);
                    }
                }
                *t = t_end;
            }
        }
        Ok(())
    }

    fn sample(&mut self, y: &[f64], t: f64) -> Result<Sample> {
        let acc = self.sys.accelerations(y).map_err(|e| at_time(e, t))?;
        let constraint_error = self.sys.constraint_error(y);
        self.stats.max_constraint_error = self.stats.max_constraint_error.max(constraint_error);
        self.stats.redundant_equations = self.stats.redundant_equations.max(acc.row_count().saturating_sub(acc.rank));
        Ok(Sample { time: t, bodies: self.sys.body_states(y), joints: self.sys.joint_results(y, &acc), energy: self.sys.energy(y), constraint_error })
    }
}

/// Simulates `model` for `settings.duration`, returning the states at every output interval
/// (the first at t = 0, after the start has been projected onto the joints).
pub fn simulate(model: &Model, settings: &Settings) -> Result<Trajectory> {
    settings.validate()?;
    let sys = System::new(model, settings.baumgarte)?;
    let mut y = System::initial_state(model)?;
    // An inconsistent start (frames a little apart, velocities the joints don't allow) is
    // brought onto the joints first, whether or not later steps project.
    sys.project_positions(&mut y, settings.projection_tol).map_err(|e| at_time(e, 0.0))?;
    sys.project_velocities(&mut y).map_err(|e| at_time(e, 0.0))?;
    let mut run = Run { sys: &sys, settings, stats: Stats::default() };
    let count = (settings.duration / settings.output_interval).ceil() as usize;
    let mut samples = Vec::with_capacity(count + 1);
    samples.push(run.sample(&y, 0.0)?);
    let mut t = 0.0;
    let mut h_next = match settings.integrator {
        Integrator::Rk4 { step } => step,
        Integrator::Adaptive { min_step, max_step, .. } => (settings.output_interval * 0.1).clamp(min_step, max_step),
    };
    for k in 1..=count {
        let t_end = (k as f64 * settings.output_interval).min(settings.duration);
        if t_end <= t {
            continue;
        }
        run.advance(&mut y, &mut t, t_end, &mut h_next)?;
        samples.push(run.sample(&y, t)?);
    }
    Ok(Trajectory { samples, stats: run.stats })
}
