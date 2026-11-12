//! The equations a lattice analysis solves, and how they are solved.
//!
//! A conduction or pressure operator on a lattice touches a cell and its six neighbours and
//! nothing else, and the coupling between two cells is the same read from either end. Both
//! facts are built into [`Stencil7`]: only the forward coupling along each axis is stored, so
//! the operator *cannot* be asymmetric, and conjugate gradients -- which needs symmetry and
//! positive definiteness and is wrong without them -- is always applied to a matrix that has
//! them.
//!
//! The method is the standard preconditioned conjugate gradient iteration (Hestenes and
//! Stiefel, 1952; Saad, *Iterative Methods for Sparse Linear Systems*, §6.7), with Jacobi
//! preconditioning. It is exact in the absence of rounding after as many steps as there are
//! equations, and in practice reaches a relative residual of 1e-10 in far fewer.

use crate::grid::{NO_EQ, Occupancy};
use crate::{Result, SimError, setup};

/// A residual the iteration has stopped improving is taken as the answer when it is at least
/// this good, and refused otherwise.
///
/// What is achievable is not the tolerance asked for but machine precision times how badly
/// conditioned the operator is, and on a real model those are not the same number: a lattice
/// over a heatsink settles at four parts in a million million and will not go further however
/// many times the iteration is restarted. Refusing that would be refusing a perfectly good
/// answer over its last two digits. A system with no single answer stagnates too -- but it
/// stagnates at a residual of about one, nowhere near this, so the two cases stay apart and the
/// refusal that matters still happens.
///
/// A solve that stops here reports the residual it reached, which a caller has beside the
/// tolerance it asked for.
const STAGNATION_LIMIT: f64 = 1e-6;

/// How much better each restart has to do to count as still making progress.
const PROGRESS: f64 = 0.9;

/// How many times a solve may go back and work the residual out afresh before giving up.
///
/// One restart is the ordinary case: the carried residual reaches the target, the true one
/// confirms it, and the loop returns. More than a couple means the carried residual is
/// collapsing without the answer improving, which is what a system with no single answer does.
const MAX_RESTARTS: u32 = 4;

/// How a solve went.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CgReport {
    /// Iterations taken.
    pub iterations: u32,
    /// The residual the last iteration left, relative to the right-hand side's norm.
    pub residual: f64,
    /// The relative residual asked for.
    pub tolerance: f64,
}

/// Something conjugate gradients can be applied to: a symmetric, positive definite operator
/// that can multiply a vector and name its own diagonal.
///
/// Both solvers here are such an operator. Conduction's is a seven-point stencil, held
/// outright; elasticity's is never assembled at all, being applied one element at a time. The
/// iteration does not care which, and only has to be got right once -- and getting it right
/// turned out to mean more than the textbook loop, which is reason enough not to write it
/// twice.
pub trait Symmetric {
    fn equations(&self) -> usize;
    /// `out = A·x`.
    fn mul(&self, x: &[f64], out: &mut [f64]) -> Result<()>;
    /// The diagonal, for Jacobi preconditioning. Every entry must be positive.
    fn diagonal(&self) -> &[f64];
}

/// A symmetric seven-point operator over the solid cells of a lattice.
///
/// Equations are numbered by [`Occupancy`], one per solid cell. The couplings an equation
/// holds are those to the next solid cell along x, y and z; the backward couplings are the
/// forward couplings of the cells behind, which is why they are not stored twice.
#[derive(Clone, Debug)]
pub struct Stencil7 {
    equations: usize,
    diag: Vec<f64>,
    along: [Vec<f64>; 3],
    forward: [Vec<u32>; 3],
}

impl Stencil7 {
    /// An all-zero operator over the solid cells of `occ`, with the forward neighbours already
    /// worked out so assembly only has to put numbers in.
    pub fn over(occ: &Occupancy) -> Stencil7 {
        let equations = occ.equations();
        let grid = occ.grid();
        let mut forward = [vec![NO_EQ; equations], vec![NO_EQ; equations], vec![NO_EQ; equations]];
        for e in 0..equations {
            let Some(cell) = occ.cell_of(e) else { continue };
            for (axis, links) in forward.iter_mut().enumerate() {
                let Some(next) = grid.next(cell, axis) else { continue };
                let Some(to) = occ.eq_of(next) else { continue };
                if let Some(slot) = links.get_mut(e) {
                    *slot = to as u32;
                }
            }
        }
        Stencil7 { equations, diag: vec![0.0; equations], along: [vec![0.0; equations], vec![0.0; equations], vec![0.0; equations]], forward }
    }

    pub fn equations(&self) -> usize {
        self.equations
    }

    /// The equation the forward coupling along `axis` reaches, or `None` at a wall or the
    /// lattice's edge.
    pub fn forward_of(&self, eq: usize, axis: usize) -> Option<usize> {
        match self.forward.get(axis)?.get(eq).copied() {
            Some(NO_EQ) | None => None,
            Some(e) => Some(e as usize),
        }
    }

    pub fn diagonal(&self, eq: usize) -> f64 {
        self.diag.get(eq).copied().unwrap_or(0.0)
    }

    pub fn add_diagonal(&mut self, eq: usize, by: f64) {
        if let Some(d) = self.diag.get_mut(eq) {
            *d += by;
        }
    }

    /// The coupling between `eq` and the next solid cell along `axis`.
    pub fn coupling(&self, eq: usize, axis: usize) -> f64 {
        self.along.get(axis).and_then(|a| a.get(eq)).copied().unwrap_or(0.0)
    }

    pub fn add_coupling(&mut self, eq: usize, axis: usize, by: f64) {
        if let Some(a) = self.along.get_mut(axis).and_then(|a| a.get_mut(eq)) {
            *a += by;
        }
    }

    /// A conduction operator couples a cell to a neighbour with `-c` off the diagonal and `+c`
    /// on both ends' diagonals, which is what makes the row sums vanish and the operator
    /// positive semi-definite. This does all three in one go, so the three cannot drift apart.
    pub fn couple(&mut self, eq: usize, axis: usize, conductance: f64) {
        let Some(to) = self.forward_of(eq, axis) else { return };
        self.add_coupling(eq, axis, -conductance);
        self.add_diagonal(eq, conductance);
        self.add_diagonal(to, conductance);
    }

    /// `out = A·x`.
    pub fn mul(&self, x: &[f64], out: &mut [f64]) -> Result<()> {
        if x.len() != self.equations || out.len() != self.equations {
            return setup("the solver was handed a vector of the wrong length");
        }
        for (o, (d, xe)) in out.iter_mut().zip(self.diag.iter().zip(x)) {
            *o = d * xe;
        }
        for (coefficients, links) in self.along.iter().zip(&self.forward) {
            for (e, (coefficient, to)) in coefficients.iter().zip(links).enumerate() {
                if *to == NO_EQ || *coefficient == 0.0 {
                    continue;
                }
                let to = *to as usize;
                let (Some(here), Some(there)) = (x.get(e).copied(), x.get(to).copied()) else { continue };
                if let Some(o) = out.get_mut(e) {
                    *o += coefficient * there;
                }
                if let Some(o) = out.get_mut(to) {
                    *o += coefficient * here;
                }
            }
        }
        Ok(())
    }

    /// What each row of the operator adds up to.
    ///
    /// Pure conduction with no boundary condition sums to zero in every row -- adding a
    /// constant to every temperature changes no flux -- so this is how a test tells an
    /// assembly bug from a boundary condition.
    pub fn row_sums(&self) -> Vec<f64> {
        let mut sums = self.diag.clone();
        for (coefficients, links) in self.along.iter().zip(&self.forward) {
            for (e, (coefficient, to)) in coefficients.iter().zip(links).enumerate() {
                if *to == NO_EQ {
                    continue;
                }
                if let Some(s) = sums.get_mut(e) {
                    *s += coefficient;
                }
                if let Some(s) = sums.get_mut(*to as usize) {
                    *s += coefficient;
                }
            }
        }
        sums
    }

    /// Solve `A·x = b` by Jacobi-preconditioned conjugate gradients, starting from whatever
    /// `x` already holds.
    ///
    /// A good starting guess is worth having: a transient step starts from the step before and
    /// settles in a handful of iterations. `tolerance` is on the residual relative to `‖b‖`.
    pub fn solve(&self, b: &[f64], x: &mut [f64], tolerance: f64, max_iterations: u32) -> Result<CgReport> {
        solve(self, b, x, tolerance, max_iterations)
    }
}

impl Symmetric for Stencil7 {
    fn equations(&self) -> usize {
        self.equations
    }

    fn mul(&self, x: &[f64], out: &mut [f64]) -> Result<()> {
        Stencil7::mul(self, x, out)
    }

    fn diagonal(&self) -> &[f64] {
        &self.diag
    }
}

/// Solve `A·x = b` by Jacobi-preconditioned conjugate gradients, starting from whatever `x`
/// already holds.
///
/// A good starting guess is worth having: a transient step starts from the step before and
/// settles in a handful of iterations. `tolerance` is on the residual relative to `‖b‖`.
pub fn solve(a: &impl Symmetric, b: &[f64], x: &mut [f64], tolerance: f64, max_iterations: u32) -> Result<CgReport> {
    let n = a.equations();
    if b.len() != n || x.len() != n {
        return setup("the solver was handed a vector of the wrong length");
    }
    if !(tolerance > 0.0 && tolerance < 1.0) {
        return setup("the solver's tolerance must be between 0 and 1");
    }
    // Jacobi needs the diagonal, and a conduction operator has no business having a
    // non-positive one. Checking here turns an assembly bug into a refusal rather than a
    // plausible-looking answer.
    let mut inverse = vec![0.0; n];
    for (m, d) in inverse.iter_mut().zip(a.diagonal()) {
        if !(*d > 0.0) || !d.is_finite() {
            return Err(SimError::NotDefinite("a diagonal that is not positive"));
        }
        *m = 1.0 / d;
    }
    if !b.iter().all(|v| v.is_finite()) {
        return Err(SimError::NotFinite("the right-hand side"));
    }
    if !x.iter().all(|v| v.is_finite()) {
        return Err(SimError::NotFinite("the starting guess"));
    }

    let scale = norm(b);
    // A right-hand side of nothing has the answer nothing, and a relative tolerance has no
    // meaning against it; fall back to an absolute one.
    let target = if scale > 0.0 { tolerance * scale } else { tolerance };
    let (mut r, mut z, mut p, mut ap) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    let mut taken = 0u32;
    let mut restarts = 0u32;
    let mut before = f64::INFINITY;

    loop {
        // Start, or start again, from the residual as the operator actually gives it.
        a.mul(x, &mut ap)?;
        for (ri, (bi, ai)) in r.iter_mut().zip(b.iter().zip(&ap)) {
            *ri = bi - ai;
        }
        let mut residual = norm(&r);
        if !residual.is_finite() {
            return Err(SimError::NotFinite("the residual"));
        }
        if residual <= target {
            return Ok(CgReport { iterations: taken, residual: relative(residual, scale), tolerance });
        }
        // This is the residual the operator actually gives, worked out afresh. If another round
        // of the iteration did not improve it, no further round will either, and what happens
        // next turns on how good it already is.
        let now = relative(residual, scale);
        if now > before * PROGRESS {
            return concede(taken, now, tolerance);
        }
        before = now;
        for (zi, (ri, mi)) in z.iter_mut().zip(r.iter().zip(&inverse)) {
            *zi = ri * mi;
        }
        p.copy_from_slice(&z);
        let mut rz = dot(&r, &z);
        let mut settled = false;

        while taken < max_iterations {
            taken += 1;
            a.mul(&p, &mut ap)?;
            let pap = dot(&p, &ap);
            if !pap.is_finite() {
                return Err(SimError::NotFinite("the search direction"));
            }
            if pap <= 0.0 {
                return Err(SimError::NotDefinite("a search direction with no curvature"));
            }
            let alpha = rz / pap;
            if !alpha.is_finite() {
                return Err(SimError::NotFinite("the step length"));
            }
            for (xi, pi) in x.iter_mut().zip(&p) {
                *xi += alpha * pi;
            }
            for (ri, ai) in r.iter_mut().zip(&ap) {
                *ri -= alpha * ai;
            }
            residual = norm(&r);
            if !residual.is_finite() {
                return Err(SimError::NotFinite("the residual"));
            }
            if residual <= target {
                // The residual carried along from step to step drifts from the one the
                // operator would give, and on a system with no single answer it collapses
                // altogether while the answer itself runs away. So the claim is checked:
                // go back to the top, work the residual out from x, and only stop if it
                // really is small. This is what keeps a singular study from coming back
                // as a confident answer.
                settled = true;
                break;
            }
            for (zi, (ri, mi)) in z.iter_mut().zip(r.iter().zip(&inverse)) {
                *zi = ri * mi;
            }
            let rz_next = dot(&r, &z);
            if !rz_next.is_finite() {
                return Err(SimError::NotFinite("the preconditioned residual"));
            }
            let beta = if rz != 0.0 { rz_next / rz } else { 0.0 };
            for (pi, zi) in p.iter_mut().zip(&z) {
                *pi = zi + beta * *pi;
            }
            rz = rz_next;
        }

        // Out of iterations, or out of restarts. Either way the residual the operator gives is
        // the number that matters, not the carried one, so it is worked out afresh before the
        // decision.
        restarts += 1;
        if !settled || restarts > MAX_RESTARTS {
            a.mul(x, &mut ap)?;
            for (ri, (bi, ai)) in r.iter_mut().zip(b.iter().zip(&ap)) {
                *ri = bi - ai;
            }
            return concede(taken, relative(norm(&r), scale), tolerance);
        }
    }
}

/// What to do when the iteration can get no further: take the answer if the residual it
/// reached is good in its own right, and refuse if it is not. See [`STAGNATION_LIMIT`].
fn concede(iterations: u32, residual: f64, tolerance: f64) -> Result<CgReport> {
    if residual <= STAGNATION_LIMIT {
        Ok(CgReport { iterations, residual, tolerance })
    } else {
        Err(SimError::NoConvergence { iterations, residual })
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

fn relative(residual: f64, scale: f64) -> f64 {
    if scale > 0.0 { residual / scale } else { residual }
}

#[cfg(test)]
#[path = "linalg_tests.rs"]
mod tests;
