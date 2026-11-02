//! Fairing the control points of a B-spline curve or surface: moving the interior control
//! vertices to the least-bending shape while the ends, or the boundary rows, stay exactly where
//! the design put them.
//!
//! The fairness energy is the thin-plate (curvature-variation) energy of the *geometry* the
//! control net describes, in the net's own parameter directions:
//!
//! - a curve: `E = ∫‖C″(t)‖² dt`,
//! - a surface: `E = ∫∫ ‖S_uu‖² + 2‖S_uv‖² + ‖S_vv‖² du dv`.
//!
//! A plane has energy zero and anything else pays for how much it bends, which is what a
//! designer means by "fair". Both energies are quadratic in the control points, `E(P) = Pᵀ·G·P`,
//! with `G` the Gram matrix of the basis functions' derivatives (`Gᵢⱼ = ∫Nᵢ″Nⱼ″ dt`, and the
//! tensor products of the one-direction Gram matrices for a surface). Each one-direction Gram is
//! integrated span by span with four-point Gauss–Legendre, exact to degree 7 — a cubic's second
//! derivative products are quadratic, so the cubic curves and bicubic faces fairing meets in
//! practice are integrated exactly.
//!
//! One fairing step is then a constrained least-squares solve: hold the boundary control points,
//! take the exact minimiser of `E` over the interior (for a quadratic that is one Newton step,
//! `G_ff·d = −(G·P)_f`), and apply a fraction `strength` of it, so `strength = 1` is the full
//! solution and a smaller value walks towards it. With the exact step the energy falls by
//! `q·s·(1 − s)` for `q ≥ 0`, so a run at any strength decreases it strictly until it converges.
//! `G_ff` does not change as the control points move, so it is factorised once, by banded
//! Cholesky, and every further step is a matrix–vector product and a triangular solve.
//!
//! Two guarantees are built in rather than checked afterwards:
//!
//! - **The boundary never moves.** The result starts as a copy of the input and only interior
//!   entries are written, so a held control point is bit-identical to the one the design gave.
//! - **The geometry cannot jump.** A B-spline lies in the convex hull of its control points
//!   (NURBS Book §3.1; with the weights unchanged the rational basis functions are still
//!   non-negative and sum to one), so the geometry moves at most as far as the farthest control
//!   point. Every step is clamped so that no control point leaves the ball of radius
//!   `tolerance` about where it started: the faired curve or surface is then within `tolerance`
//!   of the input at the same parameters, by a bound rather than by measurement. A step that
//!   would not lower the energy is halved and, if it still would not, the run stops — it never
//!   loops for ever and never moves the design further than the caller allowed.
//!
//! Everything is checked before it is used: degrees, knot vectors and counts must be consistent,
//! the control points finite, `strength ∈ (0, 1]`, `iterations` in `1…`[`MAX_FAIR_ITERATIONS`] and
//! `tolerance > 0`. A singular system — fewer than two interior control points, or an interior
//! control point that does not change the geometry at all — is reported as an error and the
//! geometry is left alone.

use solidcraft_geom::brep as b;
use solidcraft_geom::bspline::{HPoint, MAX_SPLINE_DEGREE, NurbsSurface, Side, SplineError, check_knots, local_basis};
use solidcraft_geom::{Grid, Vec3};

use crate::body::Body;
use crate::history::Change;
use crate::{KernelError, Result};

/// Most steps one fairing run takes (see [`FairOptions::iterations`]).
pub const MAX_FAIR_ITERATIONS: usize = 1000;
/// Most control points one fairing run accepts (a 128 × 128 net). `MAX_FAIR_BAND` is what
/// bounds the solver's memory and its factorisation; this bounds the rest of a step, the energy
/// sweep over every control point (`(2p_u + 1)(2p_v + 1)` neighbours each, three terms) and the
/// copies of the net: for a bicubic net a few million multiply-adds and under a megabyte. It is
/// set above the band's own limit for square bicubic nets (about 90 × 90), so a face's net is
/// refused for the system it needs rather than for its size: SMLib's thickenings make faces of
/// 72 × 78 control points, which the band takes at about 1.1 M entries.
pub const MAX_FAIR_POINTS: usize = 1 << 14;
/// Most entries of the banded system (2²¹ doubles, 16 MB). The factorisation's work is at most
/// `n · min(n, bw)²` for `n` unknowns `bw` apart with `n · (bw + 1)` within this, a few 10⁹
/// multiply-adds at the very worst (`n ≈ bw ≈ 1450`), and each solve's at most twice the band.
const MAX_FAIR_BAND: usize = 1 << 21;
/// The fairness energy of a curve, as a one-direction net: the `(order in u, order in v, weight)`
/// term list of `∫∫ (∂S)²`.
const CURVE_ENERGY: [(usize, usize, f64); 1] = [(2, 0, 1.0)];
/// The thin-plate energy of a surface: `‖S_uu‖² + 2‖S_uv‖² + ‖S_vv‖²` — the classic
/// curvature-variation energy, zero on a plane and the same if u and v are swapped.
const SURFACE_ENERGY: [(usize, usize, f64); 3] = [(2, 0, 1.0), (1, 1, 2.0), (0, 2, 1.0)];
/// Four-point Gauss–Legendre nodes and weights on `[−1, 1]` (exact to degree 7).
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];
/// The second direction of a curve seen as a control net: one basis function of degree 0 over
/// `[0, 1]`, whose Gram entry is exactly `∫1 dt = 1`, so the net's energy is the curve's own
/// energy unchanged.
const TRIVIAL_KNOTS: [f64; 2] = [0.0, 1.0];
/// How often a step is halved before the run calls the point converged.
const MAX_HALVINGS: u32 = 40;
/// A step smaller than this fraction of the full Newton step is not worth taking.
const MIN_STEP: f64 = 1e-12;
/// Steps are clamped this far inside the tolerance, so rounding cannot push a control point
/// past it.
const TOLERANCE_SLACK: f64 = 1.0 - 1e-12;

/// How a fairing run is asked to behave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FairOptions {
    /// The fraction of each energy step to apply, `0 < strength ≤ 1`: `1` is the full least
    /// squares solution (the exact minimiser of the energy at the held boundary), a smaller
    /// value takes that fraction of the way there. The energy falls at every step either way.
    pub strength: f64,
    /// How many steps at most, one to [`MAX_FAIR_ITERATIONS`].
    pub iterations: usize,
    /// Hold the layer of control points next to the boundary as well as the boundary itself (two
    /// layers held at each end). For a clamped knot vector the boundary row fixes the position
    /// along the edge and the row next to it fixes the derivative across the edge, so with this
    /// set the boundary keeps the position and the tangent the design gave it.
    pub keep_edges: bool,
    /// The farthest a control point may move from where the design put it, in mm. The geometry
    /// then stays within this distance of the input at the same parameters (see the module
    /// docs), so a fairing run can never make a design jump.
    pub tolerance: f64,
}

impl FairOptions {
    /// Checked options for one fairing run: an error, never a panic, for a strength outside
    /// `(0, 1]`, zero iterations, more than [`MAX_FAIR_ITERATIONS`] of them, or a tolerance that
    /// is not a positive finite number.
    pub fn new(strength: f64, iterations: usize, keep_edges: bool, tolerance: f64) -> Result<FairOptions> {
        FairOptions { strength, iterations, keep_edges, tolerance }.checked()
    }

    /// The same checks as [`FairOptions::new`], for options built field by field.
    pub fn checked(self) -> Result<FairOptions> {
        if !(self.strength.is_finite() && self.strength > 0.0 && self.strength <= 1.0) {
            return Err(KernelError::Invalid(format!("the fairing strength {} must be greater than 0 and at most 1", self.strength)));
        }
        if self.iterations == 0 || self.iterations > MAX_FAIR_ITERATIONS {
            return Err(KernelError::Invalid(format!("{} fairing iterations (1 to {MAX_FAIR_ITERATIONS})", self.iterations)));
        }
        if !(self.tolerance.is_finite() && self.tolerance > 0.0) {
            return Err(KernelError::Invalid(format!("the fairing tolerance {} must be a positive number of mm", self.tolerance)));
        }
        Ok(self)
    }
}

impl Default for FairOptions {
    /// One full step with the boundary held, at most 1 mm of movement: the defaults a command can
    /// start from before a user picks their own.
    fn default() -> Self {
        FairOptions { strength: 1.0, iterations: 1, keep_edges: false, tolerance: 1.0 }
    }
}

/// What a fairing run did: the energy before and after, how many steps were applied, how far the
/// control points moved and the energy after every applied step. The energy is a sum of squares,
/// so where rounding leaves it a hair below zero the true value is zero: it is reported as zero.
#[derive(Clone, Debug, PartialEq)]
pub struct FairReport {
    /// The fairness energy of the input, in the parameter units of the net's own knots.
    pub energy_before: f64,
    /// The fairness energy of the result: never more than `energy_before`, and equal to it only
    /// when no step could lower it.
    pub energy_after: f64,
    /// How many steps were applied (at most the requested `iterations`).
    pub steps: usize,
    /// The farthest any control point moved from where the design put it, in mm. It bounds how
    /// far the geometry moved from the input (see the module docs) and never exceeds the
    /// requested tolerance.
    pub movement: f64,
    /// The energy after each applied step, the last equal to `energy_after`: it never rises, and
    /// falls strictly at every step until the run reaches the rounding of its own arithmetic.
    pub energy_trace: Vec<f64>,
}

/// A faired B-spline curve: the input's degree and knots, the moved control points and the run's
/// report.
#[derive(Clone, Debug, PartialEq)]
pub struct FairCurve<const N: usize> {
    pub degree: usize,
    pub knots: Vec<f64>,
    /// The faired control points; the ends (and, with `keep_edges`, the points next to them) are
    /// bit-identical to the input.
    pub ctrl: Vec<[f64; N]>,
    pub report: FairReport,
}

/// A faired B-spline control net: the input's degrees and knots, the moved control points and the
/// run's report.
#[derive(Clone, Debug, PartialEq)]
pub struct FairNet<const N: usize> {
    pub degree: [usize; 2],
    pub knots: [Vec<f64>; 2],
    /// The faired control points, as many as the input's; the held boundary layers are
    /// bit-identical to the input.
    pub ctrl: Grid<[f64; N]>,
    pub report: FairReport,
}

/// A geom spline error as the kernel's.
fn spline_error(what: &str, e: SplineError) -> KernelError {
    match e {
        SplineError::TooLarge(m) => KernelError::TooLarge(format!("{what}: {m}")),
        other => KernelError::Invalid(format!("{what}: {other}")),
    }
}

/// The dot product of two control points' coordinate vectors.
fn dot<const N: usize>(a: &[f64; N], b: &[f64; N]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The distance between two control points.
fn dist<const N: usize>(a: &[f64; N], b: &[f64; N]) -> f64 {
    let d: [f64; N] = std::array::from_fn(|c| a[c] - b[c]);
    dot(&d, &d).sqrt()
}

/// The Gram matrices of one parameter direction, stored banded: entry `r` is
/// `∫ Nᵢ^{(r)}(t)·Nₖ^{(r)}(t) dt` over the direction's domain, and since a basis function
/// overlaps only its `degree` neighbours an entry is non-zero only for `|i − k| ≤ degree`, so
/// the whole thing is `count·(degree + 1)` numbers per order.
struct Gram {
    count: usize,
    bw: usize,
    g: [Vec<f64>; 3],
}

impl Gram {
    /// The Gram matrices of `count` basis functions of `degree` over `knots`, for orders 0, 1 and
    /// 2 — the derivatives a thin-plate energy is built from.
    fn new(degree: usize, knots: &[f64], count: usize) -> Result<Gram> {
        let bw = degree;
        let mut g: [Vec<f64>; 3] = std::array::from_fn(|_| vec![0.0; count * (bw + 1)]);
        // On the span `[knots[s], knots[s + 1])` only the basis functions `i ∈ [s − degree, s]`
        // are non-zero. The quadrature nodes lie strictly inside the span, so either one-sided
        // limit gives the same value there.
        let mut ders = [[0.0f64; 3]; MAX_SPLINE_DEGREE + 1];
        for s in degree..count {
            let (Some(&a), Some(&b)) = (knots.get(s), knots.get(s + 1)) else { continue };
            if !(b > a) {
                continue;
            }
            for &(x, w) in GAUSS4.iter() {
                let t = 0.5 * (a + b) + 0.5 * (b - a) * x;
                let first = s - degree;
                let mut live = 0usize;
                for i in first..=s {
                    let Some(local) = knots.get(i..=i + degree + 1) else { break };
                    let d = local_basis(local, t, Side::Right, 2);
                    if let Some(slot) = ders.get_mut(live) {
                        for (o, v) in slot.iter_mut().zip(d.iter()) {
                            *o = *v;
                        }
                    }
                    live += 1;
                }
                let weight = 0.5 * (b - a) * w;
                let value = |rel: usize, r: usize| ders.get(rel).and_then(|d| d.get(r)).copied().unwrap_or(0.0);
                // The upper triangle only — the band holds one copy of a symmetric entry — and at
                // the basis functions' own indices, not their places in this span.
                for i in 0..live {
                    for k in i..live {
                        let (row, off) = (first + i, k - i);
                        for (r, gg) in g.iter_mut().enumerate() {
                            if let Some(slot) = gg.get_mut(row * (bw + 1) + off) {
                                *slot += weight * value(i, r) * value(k, r);
                            }
                        }
                    }
                }
            }
        }
        if g.iter().flatten().any(|v| !v.is_finite()) {
            return Err(KernelError::Invalid("the fairness energy of this net is not a finite number".into()));
        }
        Ok(Gram { count, bw, g })
    }

    /// `∫ Nᵢ^{(r)} Nₖ^{(r)} dt`, zero outside the band or off the end of the basis.
    fn at(&self, r: usize, i: usize, k: usize) -> f64 {
        let Some(m) = self.g.get(r) else { return 0.0 };
        let (row, off) = if i <= k { (i, k - i) } else { (k, i - k) };
        if off > self.bw || row >= self.count {
            return 0.0;
        }
        m.get(row * (self.bw + 1) + off).copied().unwrap_or(0.0)
    }
}

/// A control net to fair: a tensor-product B-spline with the terms of its fairness energy. A
/// direction of one control point (degree 0, knots `[0, 1]`) makes the net a curve, whose energy
/// is the one-direction [`CURVE_ENERGY`].
struct Net<'a, const N: usize> {
    degree: [usize; 2],
    /// The control points as the design has them; the run moves copies.
    ctrl: &'a Grid<[f64; N]>,
    gram: [Gram; 2],
    /// The energy's terms that the net's degrees can carry (see [`Net::new`]).
    terms: Vec<(usize, usize, f64)>,
}

impl<'a, const N: usize> Net<'a, N> {
    /// Check the degrees, knots and counts and build the Gram matrices: an error, never a panic,
    /// for a degree above [`MAX_SPLINE_DEGREE`], too few control points for the degree, a knot
    /// vector of the wrong length, unusable knots or an empty parameter range.
    ///
    /// A term that differentiates a direction more often than its degree is dropped: on every
    /// span the basis functions are polynomials of that degree, so the derivative is zero and so
    /// is the term, exactly. A face of degree 1 one way (an extruded curve) has no `S_vv` that
    /// way: its energy is the bending along the curve and the twist, and the straight direction
    /// pays nothing.
    fn new(degree: [usize; 2], knots: [Vec<f64>; 2], ctrl: &'a Grid<[f64; N]>, terms: &'static [(usize, usize, f64)]) -> Result<Net<'a, N>> {
        let count = ctrl.count();
        if ctrl.cells().len() > MAX_FAIR_POINTS {
            return Err(KernelError::TooLarge(format!("fairing {} × {} control points (at most {MAX_FAIR_POINTS} in all)", count[0], count[1])));
        }
        for d in 0..2 {
            let (p, c, kn) = (degree[d], count[d], &knots[d]);
            if p > MAX_SPLINE_DEGREE {
                return Err(KernelError::TooLarge(format!("fairing a spline of degree {p} (at most {MAX_SPLINE_DEGREE})")));
            }
            if c <= p {
                return Err(KernelError::Invalid(format!("{c} control points is too few across one direction of a spline of degree {p}")));
            }
            if kn.len() != c + p + 1 {
                return Err(KernelError::Invalid(format!(
                    "{} knots across one direction, expected {} for {c} control points of degree {p}",
                    kn.len(),
                    c + p + 1
                )));
            }
            check_knots(kn).map_err(|e| spline_error("fairing", e))?;
            let (lo, hi) = (kn.get(p).copied().unwrap_or(f64::NAN), kn.get(c).copied().unwrap_or(f64::NAN));
            if !(lo < hi) {
                return Err(KernelError::Invalid("the fairing's parameter range is empty".into()));
            }
        }
        if terms.iter().any(|&(_, _, w)| !(w.is_finite() && w >= 0.0)) {
            return Err(KernelError::Invalid("a fairness term's weight is not a non-negative number".into()));
        }
        let terms: Vec<(usize, usize, f64)> = terms.iter().copied().filter(|&(ru, rv, _)| ru <= degree[0] && rv <= degree[1]).collect();
        if terms.is_empty() {
            return Err(KernelError::Invalid(format!("a net of degree {} × {} does not bend: there is nothing to fair", degree[0], degree[1])));
        }
        let gram = [Gram::new(degree[0], &knots[0], count[0])?, Gram::new(degree[1], &knots[1], count[1])?];
        Ok(Net { degree, ctrl, gram, terms })
    }

    /// Control points along u and along v.
    fn count(&self) -> [usize; 2] {
        self.ctrl.count()
    }

    /// The number of control points, `count[0] · count[1]`.
    fn total(&self) -> usize {
        self.ctrl.cells().len()
    }

    /// The index of control point `(i, j)` in the flattened net (u fastest).
    fn index(&self, i: usize, j: usize) -> usize {
        j * self.count()[0] + i
    }

    /// The fairness energy `Pᵀ·G·P` of the net `ctrl` (u fastest) and, in `grad`, `G·P`. One
    /// banded sweep gives both, since `E = Σ Pₚ·(G·P)ₚ`.
    fn energy_gradient(&self, ctrl: &[[f64; N]], grad: &mut [[f64; N]]) -> f64 {
        let [cu, cv] = self.count();
        let (pu, pv) = (self.degree[0], self.degree[1]);
        let mut energy = 0.0;
        for j in 0..cv {
            for i in 0..cu {
                let mut acc = [0.0f64; N];
                for &(ru, rv, w) in &self.terms {
                    for l in j.saturating_sub(pv)..=(j + pv).min(cv - 1) {
                        for k in i.saturating_sub(pu)..=(i + pu).min(cu - 1) {
                            let a = w * self.gram[0].at(ru, i, k) * self.gram[1].at(rv, j, l);
                            if a == 0.0 {
                                continue;
                            }
                            if let Some(p) = ctrl.get(self.index(k, l)) {
                                for (x, v) in acc.iter_mut().zip(p) {
                                    *x += a * v;
                                }
                            }
                        }
                    }
                }
                let at = self.index(i, j);
                if let Some(c) = ctrl.get(at) {
                    energy += dot(c, &acc);
                }
                if let Some(g) = grad.get_mut(at) {
                    *g = acc;
                }
            }
        }
        energy
    }

    /// The energy matrix `G` over the interior control points, banded and already factorised (the
    /// matrix does not change as the control points move, so one factorisation serves every
    /// step). In the u-fastest order two unknowns couple within `degree_u + degree_v · cols`:
    /// `degree_u` along a row, `degree_v` rows apart.
    fn system(&self, interior: &Interior) -> Result<Band> {
        let [cu, cv] = self.count();
        let (pu, pv) = (self.degree[0], self.degree[1]);
        let rows = if cv > 1 { interior.rows.saturating_sub(1).min(pv) } else { 0 };
        let mut band = Band::new(interior.points.len(), pu + rows * interior.cols)?;
        for (r, &at) in interior.points.iter().enumerate() {
            let (i, j) = (at % cu, at / cu);
            for &(ru, rv, w) in &self.terms {
                for l in j.saturating_sub(pv)..=(j + pv).min(cv - 1) {
                    for k in i.saturating_sub(pu)..=(i + pu).min(cu - 1) {
                        let Some(s) = interior.slot.get(self.index(k, l)).copied().flatten() else { continue };
                        band.add(r, s, w * self.gram[0].at(ru, i, k) * self.gram[1].at(rv, j, l));
                    }
                }
            }
        }
        band.factorised()
    }

    /// Whether holding `keep` layers at each end of each direction holds every control point: a
    /// direction of more than one control point and no more than `2 · keep` of them.
    fn all_held(&self, keep: usize) -> bool {
        self.count().iter().any(|&c| c > 1 && c <= 2 * keep)
    }

    /// The report of a run that moved nothing: the net's energy before and after, no steps.
    fn unmoved(&self) -> Result<FairReport> {
        let mut grad = vec![[0.0; N]; self.total()];
        let energy = self.energy_gradient(self.ctrl.cells(), &mut grad);
        if !energy.is_finite() {
            return Err(KernelError::Invalid("the fairness energy of this net is not a finite number".into()));
        }
        let energy = energy.max(0.0);
        Ok(FairReport { energy_before: energy, energy_after: energy, steps: 0, movement: 0.0, energy_trace: Vec::new() })
    }

    /// Fair the net with checked `opts`: see the module docs. The result is the input with the
    /// interior control points moved; the held ones are bit-identical.
    fn fair(&self, opts: &FairOptions) -> Result<(Grid<[f64; N]>, FairReport)> {
        let ctrl = self.ctrl.cells();
        if ctrl.iter().flatten().any(|x| !x.is_finite()) {
            return Err(KernelError::Invalid("a control point is not a finite number".into()));
        }
        let interior = Interior::new(self, if opts.keep_edges { 2 } else { 1 })?;
        let system = self.system(&interior)?;
        let mut current = self.ctrl.clone();
        let mut grad = vec![[0.0; N]; self.total()];
        let energy_before = self.energy_gradient(ctrl, &mut grad);
        if !energy_before.is_finite() {
            return Err(KernelError::Invalid("the fairness energy of this net is not a finite number".into()));
        }
        // The energy is a sum of squares, so where rounding leaves it a hair below zero the true
        // value is zero: report it that way rather than as a negative number.
        let energy_before = energy_before.max(0.0);
        // How big the net itself is, for telling a fairing step from rounding (see below).
        let size = interior.size(ctrl);
        let mut step = vec![[0.0; N]; interior.points.len()];
        let mut b = vec![0.0; interior.points.len()];
        let mut report = FairReport { energy_before, energy_after: energy_before, steps: 0, movement: 0.0, energy_trace: Vec::new() };
        for _ in 0..opts.iterations {
            let e_now = self.energy_gradient(current.cells(), &mut grad);
            if !e_now.is_finite() {
                break;
            }
            // The exact minimiser over the interior is the Newton step `G_ff·d = −(G·P)_f`; the
            // system is per coordinate, all three sharing the one factorisation.
            for c in 0..N {
                for (slot, &at) in interior.points.iter().enumerate() {
                    b[slot] = -grad.get(at).and_then(|g| g.get(c)).copied().unwrap_or(0.0);
                }
                system.solve(&mut b)?;
                for (slot, v) in b.iter().enumerate() {
                    if let Some(d) = step.get_mut(slot).and_then(|d| d.get_mut(c)) {
                        *d = *v;
                    }
                }
            }
            if step.iter().flatten().all(|&x| x == 0.0) {
                break;
            }
            // The largest fraction of the step that keeps every control point within the
            // tolerance of where the design put it, and never more than the whole step.
            let mut s = opts.strength.min(interior.limit(ctrl, current.cells(), &step, opts.tolerance));
            if !(s > 0.0) {
                break;
            }
            // A step this much shorter than the net's own size cannot be told from the rounding
            // of the arithmetic that found it: the net is as fair as it gets.
            let longest = step.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
            if s * longest <= MIN_STEP * size {
                break;
            }
            let mut taken = false;
            for _ in 0..MAX_HALVINGS {
                let candidate = interior.apply(&current, &step, s);
                let e = self.energy_gradient(candidate.cells(), &mut grad);
                if e < e_now {
                    current = candidate;
                    report.steps += 1;
                    report.energy_trace.push(e.max(0.0));
                    taken = true;
                    break;
                }
                // The step did not lower the energy: halve it and try again, and when even a hair
                // of a step will not, this is as fair as the net gets.
                s *= 0.5;
                if s <= MIN_STEP {
                    break;
                }
            }
            if !taken {
                break;
            }
        }
        report.energy_after = report.energy_trace.last().copied().unwrap_or(energy_before);
        report.movement = interior.movement(ctrl, current.cells());
        Ok((current, report))
    }
}

/// Which control points a fairing run may move, and where each sits in the banded system.
struct Interior {
    /// The flattened index (u fastest) of interior point number `k`.
    points: Vec<usize>,
    /// The interior number of each flattened index, `None` where the point is held.
    slot: Vec<Option<usize>>,
    /// How many control points across u (`cols`) and across v (`rows`) are interior.
    cols: usize,
    rows: usize,
}

impl Interior {
    /// The interior of `net` with `keep` layers held at each end of each direction (1 without
    /// [`FairOptions::keep_edges`], 2 with it).
    fn new<const N: usize>(net: &Net<N>, keep: usize) -> Result<Interior> {
        let [cu, cv] = net.count();
        let (i0, i1) = free_span(cu, keep)?;
        let (j0, j1) = free_span(cv, keep)?;
        let (cols, rows) = (i1 - i0, j1 - j0);
        let mut points = Vec::with_capacity(cols * rows);
        let mut slot = vec![None; net.total()];
        for j in j0..j1 {
            for i in i0..i1 {
                if let Some(s) = slot.get_mut(net.index(i, j)) {
                    *s = Some(points.len());
                }
                points.push(net.index(i, j));
            }
        }
        if points.len() < 2 {
            return Err(KernelError::Invalid(format!(
                "fairing needs at least two interior control points; this {cu} × {cv} net has {}",
                points.len()
            )));
        }
        Ok(Interior { points, slot, cols, rows })
    }

    /// `current` with `s` times the step `d` added to the interior control points; the held ones
    /// are copied as they are, bit for bit.
    fn apply<const N: usize>(&self, current: &Grid<[f64; N]>, d: &[[f64; N]], s: f64) -> Grid<[f64; N]> {
        let mut out = current.clone();
        for (slot, &at) in self.points.iter().enumerate() {
            let (Some(p), Some(v)) = (out.cells_mut().get_mut(at), d.get(slot)) else { continue };
            for (x, dx) in p.iter_mut().zip(v) {
                *x += s * dx;
            }
        }
        out
    }

    /// How big the interior is: the farthest interior control point from the first one, in mm.
    /// A fairing step far below this belongs to the rounding of the arithmetic, not the design.
    fn size<const N: usize>(&self, ctrl: &[[f64; N]]) -> f64 {
        let Some(first) = self.points.first().and_then(|&at| ctrl.get(at)) else { return 0.0 };
        self.points.iter().fold(0.0f64, |m, &at| ctrl.get(at).map_or(m, |p| m.max(dist(first, p))))
    }

    /// The largest fraction `s ≤ 1` of the step `d` that keeps every interior control point
    /// within `tolerance` of where the design put it (`input`). The movement is `‖Δ + s·d‖ ≤ tol`
    /// per point — a quadratic in `s` opening upwards, with the accumulated movement
    /// `Δ = current − input` already inside the ball, so the bound is its positive root.
    fn limit<const N: usize>(&self, input: &[[f64; N]], current: &[[f64; N]], d: &[[f64; N]], tolerance: f64) -> f64 {
        let mut s = 1.0f64;
        for (slot, &at) in self.points.iter().enumerate() {
            let (Some(dv), Some(cv), Some(iv)) = (d.get(slot), current.get(at), input.get(at)) else { continue };
            let a = dot(dv, dv);
            if a <= 0.0 {
                continue;
            }
            let delta: [f64; N] = std::array::from_fn(|c| cv[c] - iv[c]);
            let b = 2.0 * dot(&delta, dv);
            let c = dot(&delta, &delta) - tolerance * tolerance;
            let disc = b * b - 4.0 * a * c;
            let root = if disc >= 0.0 { (-b + disc.sqrt()) / (2.0 * a) } else { 0.0 };
            // `f64::max` ignores a NaN, so a step whose arithmetic overflows stops the run
            // instead of leaking a not-a-number into a control point.
            s = s.min(root.max(0.0));
        }
        (s * TOLERANCE_SLACK).clamp(0.0, 1.0)
    }

    /// The farthest any interior control point moved from the design's own position, in mm.
    fn movement<const N: usize>(&self, input: &[[f64; N]], current: &[[f64; N]]) -> f64 {
        self.points.iter().fold(0.0f64, |m, &at| match (input.get(at), current.get(at)) {
            (Some(a), Some(b)) => m.max(dist(a, b)),
            _ => m,
        })
    }
}

/// The control points of one direction a fairing run may move, as `[lo, hi)`. A direction of one
/// control point — a curve seen as a net one row deep — is all interior; otherwise `keep` layers
/// are held at each end: the boundary, and with `keep = 2` the layer next to it.
fn free_span(count: usize, keep: usize) -> Result<(usize, usize)> {
    if count == 1 {
        return Ok((0, 1));
    }
    if count <= 2 * keep {
        return Err(KernelError::Invalid(format!(
            "fairing a net {count} control points across one direction and holding {keep} at each edge leaves nothing to move"
        )));
    }
    Ok((keep, count - keep))
}

/// A symmetric banded matrix in upper-triangular form: row `r` holds columns `r ..= r + bw`.
struct Band {
    n: usize,
    bw: usize,
    /// The upper band, `n · (bw + 1)` numbers; the scaling by the largest diagonal is folded in
    /// when the matrix is factorised.
    a: Vec<f64>,
    scale: f64,
}

impl Band {
    fn new(n: usize, bw: usize) -> Result<Band> {
        let Some(len) = (bw + 1).checked_mul(n).filter(|&l| l <= MAX_FAIR_BAND) else {
            return Err(KernelError::TooLarge(format!("a fairing system of {n} unknowns {bw} apart")));
        };
        Ok(Band { n, bw, a: vec![0.0; len], scale: 1.0 })
    }

    fn index(&self, r: usize, s: usize) -> Option<usize> {
        let (r, s) = if r <= s { (r, s) } else { (s, r) };
        (s - r <= self.bw && s < self.n).then_some(r * (self.bw + 1) + (s - r))
    }

    fn get(&self, r: usize, s: usize) -> f64 {
        self.index(r, s).and_then(|i| self.a.get(i)).copied().unwrap_or(0.0)
    }

    fn set(&mut self, r: usize, s: usize, v: f64) {
        if let Some(x) = self.index(r, s).and_then(|i| self.a.get_mut(i)) {
            *x = v;
        }
    }

    /// Add `v` at `(r, s)`. Only `r ≤ s` is stored (the matrix is symmetric, so callers adding a
    /// whole symmetric neighbourhood reach each stored entry once).
    fn add(&mut self, r: usize, s: usize, v: f64) {
        if r <= s
            && let Some(x) = self.index(r, s).and_then(|i| self.a.get_mut(i))
        {
            *x += v;
        }
    }

    /// `A = Uᵀ·U` in place, the entries scaled by the largest diagonal first (the scaling does
    /// not change the solution, only the size of the pivots, so a net whose knots span microns or
    /// kilometres factorises just as well). An error, never a panic, when a pivot is not
    /// positive: the system is singular there and the geometry must not move.
    fn factorised(mut self) -> Result<Band> {
        let (n, bw) = (self.n, self.bw);
        let scale = (0..n).fold(0.0f64, |m, r| m.max(self.get(r, r).abs()));
        if !(scale > 0.0) || !scale.is_finite() {
            return Err(KernelError::Invalid("the fairness energy does not depend on any interior control point: nothing to fair".into()));
        }
        self.scale = scale;
        for v in self.a.iter_mut() {
            *v /= scale;
        }
        for r in 0..n {
            let mut d = self.get(r, r);
            for k in r.saturating_sub(bw)..r {
                let u = self.get(k, r);
                d -= u * u;
            }
            if !(d > 0.0) {
                return Err(KernelError::Invalid(
                    "the fairing equations are singular (an interior control point that does not move the geometry)".into(),
                ));
            }
            let d = d.sqrt();
            self.set(r, r, d);
            for s in r + 1..(r + bw + 1).min(n) {
                let mut v = self.get(r, s);
                for k in s.saturating_sub(bw)..r {
                    v -= self.get(k, r) * self.get(k, s);
                }
                self.set(r, s, v / d);
            }
        }
        Ok(self)
    }

    /// Solve `A·x = b` with the stored factorisation, in place.
    fn solve(&self, b: &mut [f64]) -> Result<()> {
        let (n, bw) = (self.n, self.bw);
        if b.len() != n {
            return Err(KernelError::Invalid(format!("{} fairing unknowns for a system of {n}", b.len())));
        }
        for x in b.iter_mut() {
            *x /= self.scale;
        }
        // Uᵀ·y = b, then U·x = y.
        for r in 0..n {
            let mut v = b.get(r).copied().unwrap_or(0.0);
            for k in r.saturating_sub(bw)..r {
                v -= self.get(k, r) * b.get(k).copied().unwrap_or(0.0);
            }
            if let Some(x) = b.get_mut(r) {
                *x = v / self.get(r, r);
            }
        }
        for r in (0..n).rev() {
            let mut v = b.get(r).copied().unwrap_or(0.0);
            for s in r + 1..(r + bw + 1).min(n) {
                v -= self.get(r, s) * b.get(s).copied().unwrap_or(0.0);
            }
            if let Some(x) = b.get_mut(r) {
                *x = v / self.get(r, r);
            }
        }
        if b.iter().any(|x| !x.is_finite()) {
            return Err(KernelError::Invalid("the fairing step is not a finite number".into()));
        }
        Ok(())
    }
}

/// Fair a B-spline curve's control points of any dimension: hold control points `0` and `n − 1`
/// and (with [`FairOptions::keep_edges`]) those next to them, and move the rest to lower the
/// bending energy `∫‖C″‖² dt` in the curve's own parameter (see the module docs). `N` is 3 for a
/// wire in space, 2 for a sketch curve.
pub fn fair_curve<const N: usize>(degree: usize, knots: &[f64], ctrl: &[[f64; N]], opts: &FairOptions) -> Result<FairCurve<N>> {
    let opts = opts.checked()?;
    let row = Grid::row(ctrl.to_vec());
    let (faired, report) = Net::new([degree, 0], [knots.to_vec(), TRIVIAL_KNOTS.to_vec()], &row, &CURVE_ENERGY)?.fair(&opts)?;
    Ok(FairCurve { degree, knots: knots.to_vec(), ctrl: faired.into_cells(), report })
}

/// Fair the interior of a B-spline control net with the thin-plate energy
/// `∫∫ ‖S_uu‖² + 2‖S_uv‖² + ‖S_vv‖² du dv` in the net's own parameters (see the module docs).
/// The boundary layers (see [`FairOptions::keep_edges`]) are held. The generic core behind
/// [`fair_surface`], [`fair_bspline_surface`] and their rational counterparts, for a caller with
/// its own control net.
pub fn fair_net<const N: usize>(degree: [usize; 2], knots: [&[f64]; 2], ctrl: &Grid<[f64; N]>, opts: &FairOptions) -> Result<FairNet<N>> {
    let opts = opts.checked()?;
    let (faired, report) = Net::new(degree, [knots[0].to_vec(), knots[1].to_vec()], ctrl, &SURFACE_ENERGY)?.fair(&opts)?;
    Ok(FairNet { degree, knots: [knots[0].to_vec(), knots[1].to_vec()], ctrl: faired, report })
}

/// The degree of a spline record of `count` control points on `knots`.
fn degree_of(knots: &[f64], count: usize) -> Result<usize> {
    knots.len().checked_sub(count + 1).ok_or_else(|| KernelError::Invalid(format!("{} knots are too few for {count} control points", knots.len())))
}

/// Fair a B-spline curve record's control points (a `Curve::Spline`): [`fair_curve`] on its
/// points, the same knots.
pub fn fair_bspline_curve(curve: &b::Curve, opts: &FairOptions) -> Result<(b::Curve, FairReport)> {
    let b::Curve::Spline { knots, points } = curve else { return Err(KernelError::Invalid("not a B-spline curve".into())) };
    let faired = fair_curve(degree_of(knots, points.len())?, knots, points, opts)?;
    Ok((b::Curve::Spline { knots: faired.knots, points: faired.ctrl }, faired.report))
}

/// Fair a rational curve record's control points (a `Curve::RationalSpline`): its Cartesian
/// control points are faired like any other curve and its weights are kept, so the result is the
/// same rational curve with moved control points. The convex-hull bound that keeps the geometry
/// within the tolerance still holds, because with the weights unchanged the rational basis
/// functions are non-negative and sum to one.
pub fn fair_nurbs_curve(curve: &b::Curve, opts: &FairOptions) -> Result<(b::Curve, FairReport)> {
    let b::Curve::RationalSpline { knots, homogeneous } = curve else { return Err(KernelError::Invalid("not a rational B-spline curve".into())) };
    let hpoints: Vec<HPoint> = homogeneous.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect();
    let ctrl = positions(&Grid::row(hpoints.clone()))?;
    let faired = fair_curve(degree_of(knots, homogeneous.len())?, knots, ctrl.cells(), opts)?;
    let net = reweighted(Grid::row(hpoints), &Grid::row(faired.ctrl));
    let homogeneous = net.into_cells().into_iter().map(|h| [h.x, h.y, h.z, h.w]).collect();
    Ok((b::Curve::RationalSpline { knots: faired.knots, homogeneous }, faired.report))
}

/// Fair a B-spline surface record's control net (a `Surface::Spline`): [`fair_net`] on its
/// points, the same knots, the boundary rows held.
pub fn fair_bspline_surface(surface: &b::Surface, opts: &FairOptions) -> Result<(b::Surface, FairReport)> {
    let b::Surface::Spline { u_knots, v_knots, points } = surface else { return Err(KernelError::Invalid("not a B-spline surface".into())) };
    let net = record_net(points)?;
    let [cu, cv] = net.count();
    let degree = [degree_of(u_knots, cu)?, degree_of(v_knots, cv)?];
    let faired = fair_net(degree, [u_knots, v_knots], &net, opts)?;
    Ok((b::Surface::Spline { u_knots: u_knots.clone(), v_knots: v_knots.clone(), points: faired.ctrl.into_u_rows() }, faired.report))
}

/// Fair a rational surface record's control net (a `Surface::RationalSpline`), like
/// [`fair_surface`]: the Cartesian control points are faired, the weights kept.
pub fn fair_nurbs_surface(surface: &b::Surface, opts: &FairOptions) -> Result<(b::Surface, FairReport)> {
    let b::Surface::RationalSpline { u_knots, v_knots, homogeneous } = surface else {
        return Err(KernelError::Invalid("not a rational B-spline surface".into()));
    };
    let net = record_net(homogeneous)?.map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] });
    let [cu, cv] = net.count();
    let degree = [degree_of(u_knots, cu)?, degree_of(v_knots, cv)?];
    let faired = fair_net(degree, [u_knots, v_knots], &positions(&net)?, opts)?;
    let homogeneous = reweighted(net, &faired.ctrl).map(|h| [h.x, h.y, h.z, h.w]).into_u_rows();
    Ok((b::Surface::RationalSpline { u_knots: u_knots.clone(), v_knots: v_knots.clone(), homogeneous }, faired.report))
}

/// A surface record's control net (`rows[i][j]`, i along u) as a grid: refused when it is empty
/// or not rectangular.
fn record_net<P: Clone>(rows: &[Vec<P>]) -> Result<Grid<P>> {
    let net = Grid::from_u_rows(rows).ok_or_else(|| KernelError::Invalid("the surface's control net is not rectangular".into()))?;
    if net.is_empty() {
        return Err(KernelError::Invalid("the surface has no control points".into()));
    }
    Ok(net)
}

/// A homogeneous control net's Cartesian points; refused when a weight is zero (the point has no
/// position).
fn positions(net: &Grid<HPoint>) -> Result<Grid<[f64; 3]>> {
    net.clone().try_map(|h| {
        let p = h.project().ok_or_else(|| KernelError::Invalid("a control point has no position: its weight is zero".into()))?;
        Ok([p.x, p.y, p.z])
    })
}

/// `net` moved to the faired `points`, each keeping its weight.
fn reweighted(mut net: Grid<HPoint>, points: &Grid<[f64; 3]>) -> Grid<HPoint> {
    for (h, &[x, y, z]) in net.cells_mut().iter_mut().zip(points.cells()) {
        *h = HPoint::weighted(Vec3::new(x, y, z), h.w);
    }
    net
}

/// Fair a geom NURBS surface's control net: [`fair_net`] on its Cartesian control points, with
/// the weights kept as they are (a fairing moves points, not weights) and the boundary rows held.
/// A rational surface's geometry then stays within the tolerance too, by the same convex-hull
/// bound as the non-rational case.
pub fn fair_surface(surface: &NurbsSurface, opts: &FairOptions) -> Result<(NurbsSurface, FairReport)> {
    let net = surface.net();
    let [ku, kv] = surface.knots();
    let degree = surface.degree();
    let faired = fair_net(degree, [ku, kv], &positions(&net)?, opts)?;
    let net = reweighted(net, &faired.ctrl);
    let out =
        NurbsSurface::from_homogeneous(degree, surface.knots().clone(), net.into_cells()).map_err(|e| spline_error("fairing the surface", e))?;
    Ok((out, faired.report))
}

/// Fair the B-spline faces at `faces` (indices in `body`'s face order) of `body`: each face's
/// interior control vertices move to flatten the thin-plate energy of the face while its boundary
/// rows stay put, so the face keeps meeting its neighbours as it did.
///
/// Every picked face keeps the very loops, edges and vertices it had and the way it pointed: only
/// its surface changes in the body's record, so the body is as closed (or as open) as it was,
/// holes and all, and SMLib builds it from that record. A face that is not a B-spline (a plane,
/// cylinder, cone, sphere, torus or revolution) has no control points to fair and is refused by
/// name; a mesh body has no faces to fair at all. A face whose held layers are its whole net (see
/// [`held_face`]) has no interior control vertex to move and is left exactly as it is, its energy
/// reported unchanged.
///
/// The report is over the picked faces together: the energies summed, the largest movement of any
/// control point, and the energy trace as the sum at each step (a face that stops early keeps its
/// last energy for the steps the others still take).
pub fn fair_faces(body: &Body, faces: &[usize], opts: &FairOptions) -> Result<(Body, FairReport)> {
    let opts = opts.checked()?;
    body.require_brep("fair")?;
    let mut record = crate::native::brep_record(body)?;
    let count = record.faces.len();
    if count == 0 {
        return Err(KernelError::Invalid("the body has no faces".into()));
    }
    let mut picked: Vec<usize> = Vec::new();
    for &i in faces {
        if i >= count {
            return Err(KernelError::Invalid(format!("no face {i}: the body has {count}")));
        }
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    if picked.is_empty() {
        return Err(KernelError::Invalid("pick the faces to fair".into()));
    }
    let mut reports: Vec<FairReport> = Vec::with_capacity(picked.len());
    let mut changed: Vec<usize> = Vec::with_capacity(picked.len());
    for &i in &picked {
        let Some(face) = record.faces.get_mut(i) else { continue };
        if let Some(report) = held_face(&face.surface, &opts)? {
            reports.push(report);
            continue;
        }
        let (surface, report) = match &face.surface {
            b::Surface::Spline { .. } => fair_bspline_surface(&face.surface, &opts)?,
            b::Surface::RationalSpline { .. } => fair_nurbs_surface(&face.surface, &opts)?,
            _ => {
                return Err(KernelError::Invalid(format!(
                    "face {i} is not a B-spline face: a plane, cylinder, cone, sphere, torus or revolution has no control points to fair"
                )));
            }
        };
        face.surface = surface;
        reports.push(report);
        changed.push(i);
    }
    let report = FairReport {
        energy_before: reports.iter().map(|r| r.energy_before).sum(),
        energy_after: reports.iter().map(|r| r.energy_after).sum(),
        steps: reports.iter().map(|r| r.steps).max().unwrap_or(0),
        movement: reports.iter().fold(0.0f64, |m, r| m.max(r.movement)),
        energy_trace: {
            let longest = reports.iter().map(|r| r.energy_trace.len()).max().unwrap_or(0);
            (0..longest).map(|k| reports.iter().map(|r| r.energy_trace.get(k).copied().unwrap_or(r.energy_after)).sum()).collect()
        },
    };
    if changed.is_empty() {
        return Ok((body.clone(), report));
    }
    let smlib = solidcraft_smlib::Brep::from_record(&record, true).map_err(|e| KernelError::Failed(format!("fair: SMLib: {e}")))?;
    let (result, order) = crate::native::body_from_smlib_ordered(&smlib)?;
    // The topology is the body's own, so SMLib's order is the body's: each face kept or
    // modified, each edge kept. Where SMLib's order is not the record's the history is not known.
    let kept = order.is_some_and(|o| o.faces.iter().enumerate().all(|(i, f)| i == *f) && o.edges.iter().enumerate().all(|(i, e)| i == *e));
    if !kept || result.face_count() != body.face_count() || result.edge_count() != body.edge_count() {
        return Ok((result, report));
    }
    let mut history = <crate::history::History as crate::history::BodyHistory>::identity(body);
    for &i in &changed {
        if let Some(f) = history.faces.get_mut(i) {
            f.change = Change::Modified;
        }
    }
    Ok((result.with_history(history, &[body])?, report))
}

/// The report for a B-spline face whose every control point `opts` holds, which the run leaves
/// exactly as it is: a direction of no more control points than the held layers (a single-span
/// cubic with its edges kept, an extruded curve's two rows) holds the whole net, since every row
/// is a boundary row or one next to it. `None` when some control point is free to move, and for a
/// face that is not a B-spline.
fn held_face(surface: &b::Surface, opts: &FairOptions) -> Result<Option<FairReport>> {
    let (u_knots, v_knots, ctrl) = match surface {
        b::Surface::Spline { u_knots, v_knots, points } => (u_knots, v_knots, record_net(points)?),
        b::Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            (u_knots, v_knots, positions(&record_net(homogeneous)?.map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }))?)
        }
        _ => return Ok(None),
    };
    let [cu, cv] = ctrl.count();
    let degree = [degree_of(u_knots, cu)?, degree_of(v_knots, cv)?];
    let net = Net::new(degree, [u_knots.clone(), v_knots.clone()], &ctrl, &SURFACE_ENERGY)?;
    if !net.all_held(if opts.keep_edges { 2 } else { 1 }) {
        return Ok(None);
    }
    net.unmoved().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::brep::eval::CurveEval;

    /// A clamped uniform knot vector over `[0, spans]` of degree `degree`: `spans + degree`
    /// control points.
    fn clamped(spans: usize, degree: usize) -> Vec<f64> {
        let mut k = vec![0.0; degree + 1];
        k.extend((1..spans).map(|i| i as f64));
        k.extend(std::iter::repeat_n(spans as f64, degree + 1));
        k
    }

    /// The knot average `(kᵢ₊₁ + … + kᵢ₊ₚ)/p` of control point `i`: `Σ Nᵢ(t)·ξᵢ = t` (NURBS Book
    /// eq. 3.4), so control points at `a + b·ξᵢ` describe the affine curve `a + b·t` exactly.
    fn average(knots: &[f64], degree: usize, i: usize) -> f64 {
        (1..=degree).map(|r| knots.get(i + r).copied().unwrap_or(0.0)).sum::<f64>() / degree as f64
    }

    /// The control points of the affine curve `ξ ↦ a + b·ξ` at the knot averages of `knots`: a
    /// curve of exactly zero bending energy.
    fn affine_curve(knots: &[f64], degree: usize, a: [f64; 3], b: [f64; 3]) -> Vec<[f64; 3]> {
        (0..knots.len() - degree - 1).map(|i| std::array::from_fn(|c| a[c] + b[c] * average(knots, degree, i))).collect()
    }

    /// The control net (u fastest) of the plane `(ξ, η) ↦ a + b·ξ + c·η` at the knot averages of
    /// `ku` and `kv`: a net of exactly zero thin-plate energy.
    fn planar_net(ku: &[f64], kv: &[f64], degree: [usize; 2], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Vec<[f64; 3]> {
        let (cu, cv) = (ku.len() - degree[0] - 1, kv.len() - degree[1] - 1);
        let mut net = Vec::with_capacity(cu * cv);
        for j in 0..cv {
            for i in 0..cu {
                let (xi, eta) = (average(ku, degree[0], i), average(kv, degree[1], j));
                net.push(std::array::from_fn(|d| a[d] + b[d] * xi + c[d] * eta));
            }
        }
        net
    }

    /// The point of the curve `(degree, knots, ctrl)` at `t`, through the same basis geom uses: a
    /// basis function `i` owns the `degree + 2` knots `knots[i..=i + degree + 1]`.
    fn curve_at<const N: usize>(degree: usize, knots: &[f64], ctrl: &[[f64; N]], t: f64) -> [f64; N] {
        let n = ctrl.len();
        let side = if t >= knots.get(n).copied().unwrap_or(f64::INFINITY) { Side::Left } else { Side::Right };
        let mut out = [0.0; N];
        for (i, c) in ctrl.iter().enumerate() {
            let b = knots.get(i..=i + degree + 1).and_then(|l| local_basis(l, t, side, 0).first().copied()).unwrap_or(0.0);
            for (o, x) in out.iter_mut().zip(c) {
                *o += b * x;
            }
        }
        out
    }

    /// The greatest distance between two points of space, in mm.
    fn apart(a: Vec3, b: Vec3) -> f64 {
        a.dist(b)
    }

    /// A geom point as a coordinate triple.
    fn triple(p: Vec3) -> [f64; 3] {
        [p.x, p.y, p.z]
    }

    #[test]
    fn a_perturbed_curve_is_faired_back_to_its_smooth_shape_and_its_ends_stay_put() {
        // A cubic curve whose control points sit at the knot averages of an affine curve: zero
        // bending energy, so a fairing run must return to the same control polygon. The knots are
        // deliberately uneven, so a fairing that ignored the curve's own parameter would not.
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 9.0, 9.0, 9.0, 9.0];
        let (a, b) = ([-20.0, 5.0, 1.5], [4.0, -2.5, 3.0]);
        let smooth = affine_curve(&knots, 3, a, b);
        let mut rough = smooth.clone();
        rough[2] = [rough[2][0] + 1.5, rough[2][1] - 2.0, rough[2][2] + 3.0];
        rough[3][2] += 2.5;
        rough[4][0] -= 1.25;
        rough[5][1] += 1.75;
        let far = (0..smooth.len()).map(|i| dist(&rough[i], &smooth[i])).fold(0.0, f64::max);

        let opts = FairOptions::new(1.0, 8, false, 100.0).unwrap();
        let faired = fair_curve(3, &knots, &rough, &opts).unwrap();
        // The ends did not move at all, bit for bit.
        assert_eq!(faired.ctrl[0], rough[0]);
        assert_eq!(faired.ctrl[6], rough[6]);
        // Every interior control point came back to the smooth shape.
        let near = (1..6).map(|i| dist(&faired.ctrl[i], &smooth[i])).fold(0.0, f64::max);
        eprintln!(
            "curve: perturbation {far} mm, faired to {near:e} mm, energy {:e} → {:e} in {} steps, movement {:e} mm",
            faired.report.energy_before, faired.report.energy_after, faired.report.steps, faired.report.movement
        );
        assert!(near < 1e-6 && near < far * 1e-6, "faired to {near:e}, was {far} mm out");
        // The energy is a real number before, fell to nothing, and never rose on the way.
        assert!(faired.report.energy_before > 1e-3, "{:e}", faired.report.energy_before);
        assert!(faired.report.energy_after >= 0.0 && faired.report.energy_after < 1e-9 * faired.report.energy_before);
        assert!(faired.report.steps >= 1 && faired.report.energy_trace.len() == faired.report.steps);
        assert_eq!(faired.report.energy_trace.last().copied(), Some(faired.report.energy_after));
        assert!(faired.report.energy_trace.windows(2).all(|w| w[1] < w[0]), "{:?}", faired.report.energy_trace);
        // A step never moves a control point further than the tolerance, and here it moved the
        // whole way back to the smooth shape.
        assert!(faired.report.movement <= 100.0 && (faired.report.movement - far).abs() < 1e-9, "{:e}", faired.report.movement);
        // The faired curve is the affine line it was meant to be.
        let mut off = 0.0f64;
        for k in 0..=32 {
            let t = 9.0 * f64::from(k) / 32.0;
            let want: [f64; 3] = std::array::from_fn(|c| a[c] + b[c] * t);
            off = off.max(dist(&curve_at(3, &knots, &faired.ctrl, t), &want));
        }
        eprintln!("curve: the faired curve is {off:e} mm off the affine line it describes");
        assert!(off < 1e-6, "{off:e}");
    }

    #[test]
    fn a_half_strength_run_walks_to_the_same_shape_over_its_iterations() {
        // The same perturbation, walked a half step at a time: the energy must fall at every step
        // (monotone, strictly) and still arrive at the smooth shape.
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 9.0, 9.0, 9.0, 9.0];
        let smooth = affine_curve(&knots, 3, [-20.0, 5.0, 1.5], [4.0, -2.5, 3.0]);
        let mut rough = smooth.clone();
        rough[2][2] += 4.0;
        rough[3][0] += 2.0;
        let opts = FairOptions::new(0.5, 40, false, 100.0).unwrap();
        let faired = fair_curve(3, &knots, &rough, &opts).unwrap();
        eprintln!("half strength: energy {:e} → {:e} in {} steps", faired.report.energy_before, faired.report.energy_after, faired.report.steps);
        assert!(faired.report.energy_trace.len() > 4, "one step is not a walk");
        assert!(faired.report.energy_trace.windows(2).all(|w| w[1] < w[0]), "the energy must fall at every step");
        assert!(faired.report.energy_after < 1e-9 * faired.report.energy_before);
        assert_eq!(faired.ctrl[0], rough[0]);
        assert_eq!(faired.ctrl[6], rough[6]);
        assert!(dist(&faired.ctrl[3], &smooth[3]) < 1e-6);
    }

    #[test]
    fn the_faired_curve_stays_within_the_tolerance_of_the_design() {
        // A heavily perturbed curve with a 0.75 mm tolerance: the design may be smoothed, but
        // never by more than the caller allowed, and the bound is on the geometry itself.
        let knots = clamped(3, 3);
        let smooth = affine_curve(&knots, 3, [0.0, 0.0, 0.0], [30.0, 8.0, 0.0]);
        let mut rough = smooth.clone();
        for (i, d) in [(1, [1.0, -2.0, 4.0]), (2, [-3.0, 2.0, -6.0]), (3, [2.0, -1.0, 5.0]), (4, [-2.0, 3.0, -4.0])] {
            for c in 0..3 {
                rough[i][c] += d[c];
            }
        }
        let tolerance = 0.75;
        let opts = FairOptions::new(1.0, 12, false, tolerance).unwrap();
        let faired = fair_curve(3, &knots, &rough, &opts).unwrap();
        let mut off = 0.0f64;
        for k in 0..=64 {
            let t = 3.0 * f64::from(k) / 64.0;
            off = off.max(dist(&curve_at(3, &knots, &faired.ctrl, t), &curve_at(3, &knots, &rough, t)));
        }
        eprintln!(
            "tolerance {tolerance}: the curve moved {off:e} mm at worst ({:e} mm of control points), energy {:e} → {:e}",
            faired.report.movement, faired.report.energy_before, faired.report.energy_after
        );
        assert!(faired.report.movement <= tolerance, "{:e}", faired.report.movement);
        assert!(off <= tolerance * (1.0 + 1e-9), "{off:e} over {tolerance}");
        assert!(faired.report.energy_after < faired.report.energy_before);
        // The tolerance did not stop the run from heading for the smooth shape.
        let before = (0..6).map(|i| dist(&rough[i], &smooth[i])).fold(0.0, f64::max);
        let after = (0..6).map(|i| dist(&faired.ctrl[i], &smooth[i])).fold(0.0, f64::max);
        assert!(after < before, "{after} vs {before}");
        assert_eq!(faired.ctrl[0], rough[0]);
        assert_eq!(faired.ctrl[5], rough[5]);
    }

    #[test]
    fn a_sketch_curve_of_two_dimensions_fairs_the_same_way() {
        // The generic core takes `[f64; 2]` just as well: a quadratic sketch curve, its middle
        // pulled 2 mm out of the smooth polyline.
        let knots = clamped(4, 2);
        let smooth: Vec<[f64; 2]> = (0..knots.len() - 3).map(|i| [average(&knots, 2, i) * 10.0, average(&knots, 2, i) * 2.0]).collect();
        let mut rough = smooth.clone();
        rough[2] = [rough[2][0] + 1.0, rough[2][1] - 2.0];
        rough[3][1] += 1.5;
        let opts = FairOptions::new(1.0, 10, false, 50.0).unwrap();
        let faired = fair_curve(2, &knots, &rough, &opts).unwrap();
        eprintln!("2D sketch curve: energy {:e} → {:e} in {} steps", faired.report.energy_before, faired.report.energy_after, faired.report.steps);
        assert_eq!(faired.ctrl[0], rough[0]);
        assert_eq!(faired.ctrl[5], rough[5]);
        assert!(faired.report.energy_after >= 0.0 && faired.report.energy_after < 1e-9 * faired.report.energy_before);
        for (i, want) in smooth.iter().enumerate().take(5).skip(1) {
            assert!(dist(&faired.ctrl[i], want) < 1e-6, "point {i}");
        }
    }

    #[test]
    fn a_perturbed_net_is_faired_back_to_its_plane_with_its_boundary_rows_held() {
        // A bicubic net of a plane (zero thin-plate energy), its middle pushed out; the boundary
        // rows must not move at all and the interior must come back to the plane.
        let (ku, kv) = (clamped(3, 3), clamped(4, 3));
        let degree = [3, 3];
        let (cu, cv) = (ku.len() - 4, kv.len() - 4);
        let (a, b, c) = ([1.0, -2.0, 4.0], [2.0, 0.5, -0.25], [-0.5, 1.5, 0.75]);
        let smooth = planar_net(&ku, &kv, degree, a, b, c);
        let mut rough = smooth.clone();
        for &(i, j, d) in &[(2usize, 2usize, 3.0), (3, 3, -2.5), (1, 4, 1.5), (4, 5, -1.0)] {
            rough[j * cu + i][2] += d;
        }
        let opts = FairOptions::new(1.0, 6, false, 100.0).unwrap();
        let faired = fair_net(degree, [&ku, &kv], &Grid::from_fn([cu, cv], |i, j| rough[j * cu + i]).unwrap(), &opts).unwrap();
        for j in 0..cv {
            for i in [0, cu - 1] {
                assert_eq!(faired.ctrl.get(i, j), Some(&rough[j * cu + i]), "column {i}, row {j} moved");
            }
        }
        for i in 0..cu {
            for j in [0, cv - 1] {
                assert_eq!(faired.ctrl.get(i, j), Some(&rough[j * cu + i]), "row {j}, column {i} moved");
            }
        }
        let near = faired.ctrl.cells().iter().zip(&smooth).map(|(a, b)| dist(a, b)).fold(0.0, f64::max);
        let far = (0..cu * cv).map(|k| dist(&rough[k], &smooth[k])).fold(0.0, f64::max);
        eprintln!("net: perturbation {far} mm, faired to {near:e} mm, energy {:e} → {:e}", faired.report.energy_before, faired.report.energy_after);
        assert!(near < 1e-6, "{near:e}");
        assert!(faired.report.energy_after < 1e-9 * faired.report.energy_before);
        assert!(faired.report.energy_trace.windows(2).all(|w| w[1] < w[0]));
        // With keep_edges the two outer layers at each side stay exactly as designed — so the
        // bump goes into the middle of the net — and the rest still comes back to the plane.
        let mut inner = smooth.clone();
        for &(i, j, d) in &[(2usize, 2usize, 3.0), (3, 3, -2.5), (2, 3, 1.5), (3, 2, -1.0)] {
            inner[j * cu + i][2] += d;
        }
        let kept = FairOptions::new(1.0, 6, true, 100.0).unwrap();
        let faired = fair_net(degree, [&ku, &kv], &Grid::from_fn([cu, cv], |i, j| inner[j * cu + i]).unwrap(), &kept).unwrap();
        let shell = |i: usize, j: usize| i < 2 || i + 2 >= cu || j < 2 || j + 2 >= cv;
        for j in 0..cv {
            for i in 0..cu {
                if shell(i, j) {
                    assert_eq!(faired.ctrl.get(i, j), Some(&inner[j * cu + i]), "({i}, {j}) is a boundary layer and moved");
                }
            }
        }
        let near = (0..cu * cv).filter(|&k| !shell(k % cu, k / cu)).map(|k| dist(&faired.ctrl.cells()[k], &smooth[k])).fold(0.0, f64::max);
        eprintln!("net with keep_edges: interior faired to {near:e} mm, energy {:e} → {:e}", faired.report.energy_before, faired.report.energy_after);
        assert!(near < 1e-6 && faired.report.energy_after < faired.report.energy_before);
    }

    #[test]
    fn a_faired_geom_surface_stays_within_the_tolerance_of_the_design_and_keeps_its_weights() {
        // A rational surface (weights off one) whose middle is pushed out: fair it with a 0.5 mm
        // tolerance. The weights must be untouched, the boundary rows must not move, and the
        // surface must stay within the tolerance of the one the design asked for.
        let (ku, kv) = (clamped(3, 3), clamped(3, 3));
        let degree = [3, 3];
        let (cu, cv) = (ku.len() - 4, kv.len() - 4);
        let smooth = planar_net(&ku, &kv, degree, [0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0, 15.0, 0.0]);
        let weights: Vec<f64> = (0..cu * cv).map(|k| 0.5 + (k % 5) as f64 * 0.25).collect();
        let mut rough = smooth.clone();
        for &(i, j, d) in &[(2usize, 2usize, 4.0), (3, 3, -3.0), (1, 2, 2.0)] {
            rough[j * cu + i][2] += d;
        }
        let surface =
            NurbsSurface::new(degree, [ku.clone(), kv.clone()], &rough.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect::<Vec<_>>(), &weights)
                .unwrap();
        let tolerance = 0.5;
        let opts = FairOptions::new(1.0, 8, false, tolerance).unwrap();
        let (faired, report) = fair_surface(&surface, &opts).unwrap();
        for k in 0..cu * cv {
            let (i, j) = (k % cu, k / cu);
            assert_eq!(faired.point(i, j).w, surface.point(i, j).w, "the weight of ({i}, {j}) changed");
        }
        for j in 0..cv {
            for i in [0, cu - 1] {
                assert_eq!(faired.point(i, j), surface.point(i, j), "column {i} moved");
            }
        }
        let dom = surface.domain();
        let mut off = 0.0f64;
        for k in 0..=24 {
            for l in 0..=24 {
                let (u, v) = (dom[0][0] + (dom[0][1] - dom[0][0]) * f64::from(k) / 24.0, dom[1][0] + (dom[1][1] - dom[1][0]) * f64::from(l) / 24.0);
                off = off.max(surface.eval(u, v).unwrap().dist(faired.eval(u, v).unwrap()));
            }
        }
        eprintln!(
            "surface: moved {off:e} mm at worst ({:e} mm of control points), energy {:e} → {:e}",
            report.movement, report.energy_before, report.energy_after
        );
        assert!(report.movement <= tolerance, "{:e}", report.movement);
        assert!(off <= tolerance * (1.0 + 1e-9), "{off:e} over {tolerance}");
        assert!(report.energy_after < report.energy_before);
        // Every interior control point is nearer the plane than it was.
        let before = (0..cu * cv).map(|k| dist(&rough[k], &smooth[k])).fold(0.0, f64::max);
        let after = (0..cu * cv).map(|k| dist(&triple(faired.point(k % cu, k / cu).project().unwrap()), &smooth[k])).fold(0.0, f64::max);
        assert!(after < before, "{after} vs {before}");
    }

    #[test]
    fn the_record_wrappers_fair_a_spline_curve_and_surface_with_their_ends_held() {
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 9.0, 9.0, 9.0, 9.0];
        let smooth = affine_curve(&knots, 3, [-20.0, 5.0, 1.5], [4.0, -2.5, 3.0]);
        let mut rough = smooth.clone();
        rough[2][2] += 3.0;
        rough[4][1] -= 2.0;
        let curve = b::Curve::Spline { knots: knots.to_vec(), points: rough.clone() };
        let tolerance = 5.0;
        let opts = FairOptions::new(1.0, 8, false, tolerance).unwrap();
        let (faired, report) = fair_bspline_curve(&curve, &opts).unwrap();
        let (fe, ce) = (CurveEval::new(&faired).unwrap(), CurveEval::new(&curve).unwrap());
        let mut off = 0.0f64;
        for k in 0..=64 {
            let t = 9.0 * f64::from(k) / 64.0;
            off = off.max(apart(fe.point(t).unwrap(), ce.point(t).unwrap()));
        }
        eprintln!(
            "record curve: energy {:e} → {:e} in {} steps, moved {off:e} mm ({:e} mm of control points)",
            report.energy_before, report.energy_after, report.steps, report.movement
        );
        let b::Curve::Spline { points: moved, .. } = &faired else { panic!("a B-spline curve") };
        assert_eq!(moved.first(), rough.first());
        assert_eq!(moved.last(), rough.last());
        assert!(off <= tolerance * (1.0 + 1e-9), "{off:e}");
        assert!(report.energy_after < 1e-9 * report.energy_before);

        // The same on a B-spline surface: boundary rows bit-identical, the bump in the middle
        // comes down, the energy falls.
        let (ku, kv) = (clamped(3, 3), clamped(3, 3));
        let degree = [3, 3];
        let (cu, cv) = (ku.len() - 4, kv.len() - 4);
        let smooth = planar_net(&ku, &kv, degree, [0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0, 15.0, 0.0]);
        let mut rough = smooth.clone();
        rough[2 * cu + 2][2] += 5.0;
        let rows: Vec<Vec<[f64; 3]>> = (0..cu).map(|i| (0..cv).map(|j| rough[j * cu + i]).collect()).collect();
        let surface = b::Surface::Spline { u_knots: ku.clone(), v_knots: kv.clone(), points: rows.clone() };
        let (faired, report) = fair_bspline_surface(&surface, &FairOptions::new(1.0, 6, true, 8.0).unwrap()).unwrap();
        let b::Surface::Spline { points: moved, .. } = &faired else { panic!("a B-spline surface") };
        eprintln!("record surface: energy {:e} → {:e}, {} steps", report.energy_before, report.energy_after, report.steps);
        for i in 0..cu {
            for j in 0..cv {
                if i < 2 || i + 2 >= cu || j < 2 || j + 2 >= cv {
                    assert_eq!(moved[i][j], rows[i][j], "({i}, {j}) is a boundary layer and moved");
                }
            }
        }
        assert!(report.energy_after < report.energy_before);
        assert!(moved[2][2][2] < rows[2][2][2], "the bump should come down");
    }

    #[test]
    fn a_rational_surface_fairs_with_its_boundary_and_tolerance_intact() {
        // The rational record wrapper, on weights away from one: fairing moves the Cartesian control
        // points and keeps the weights, so the surface still cannot move more than the tolerance.
        let (ku, kv) = (clamped(3, 3), clamped(3, 3));
        let degree = [3, 3];
        let (cu, cv) = (ku.len() - 4, kv.len() - 4);
        let smooth = planar_net(&ku, &kv, degree, [0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0, 15.0, 0.0]);
        let mut rough = smooth.clone();
        rough[2 * cu + 3][2] += 6.0;
        let weights: Vec<f64> = (0..cu * cv).map(|k| 0.75 + (k % 3) as f64 * 0.5).collect();
        let rows: Vec<Vec<[f64; 4]>> = (0..cu)
            .map(|i| {
                (0..cv)
                    .map(|j| {
                        let (c, w) = (rough[j * cu + i], weights[j * cu + i]);
                        [c[0] * w, c[1] * w, c[2] * w, w]
                    })
                    .collect()
            })
            .collect();
        let surface = b::Surface::RationalSpline { u_knots: ku.clone(), v_knots: kv.clone(), homogeneous: rows.clone() };
        let (faired, report) = fair_nurbs_surface(&surface, &FairOptions::new(1.0, 6, false, 0.5).unwrap()).unwrap();
        eprintln!("rational surface: energy {:e} → {:e}, movement {:e} mm", report.energy_before, report.energy_after, report.movement);
        assert!(report.movement <= 0.5 && report.energy_after < report.energy_before);
        let b::Surface::RationalSpline { homogeneous: moved, .. } = &faired else { panic!("a rational surface") };
        for i in 0..cu {
            for j in 0..cv {
                let (before, after) = (rows[i][j], moved[i][j]);
                assert_eq!(after[3], before[3], "the weight of ({i}, {j}) changed");
                if i == 0 || i + 1 == cu || j == 0 || j + 1 == cv {
                    assert_eq!(after, before, "({i}, {j}) is on the boundary and moved");
                }
            }
        }
    }

    #[test]
    fn hostile_fairing_inputs_are_refused() {
        let knots = clamped(3, 3);
        let ctrl = affine_curve(&knots, 3, [0.0, 0.0, 0.0], [10.0, 4.0, 2.0]);
        let ok = FairOptions::new(1.0, 4, false, 10.0).unwrap();
        // Out-of-range parameters: strength, iterations and tolerance are all input-derived.
        for strength in [0.0, -0.25, 1.000_001, f64::NAN, f64::INFINITY] {
            assert!(FairOptions::new(strength, 4, false, 10.0).is_err(), "strength {strength}");
        }
        for iterations in [0, MAX_FAIR_ITERATIONS + 1, usize::MAX] {
            assert!(FairOptions::new(1.0, iterations, false, 10.0).is_err(), "iterations {iterations}");
        }
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(FairOptions::new(1.0, 4, false, tolerance).is_err(), "tolerance {tolerance}");
        }
        // A control point that is not a finite number.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut nan = ctrl.clone();
            nan[2][1] = bad;
            assert!(fair_curve(3, &knots, &nan, &ok).is_err(), "a control point of {bad} was faired");
        }
        // Knot vectors that do not fit the control points or the degree.
        assert!(fair_curve(3, &knots[..9], &ctrl, &ok).is_err(), "nine knots for six control points of degree three");
        assert!(fair_curve(3, &[0.0, 0.0, 0.0, 0.0, 2.0, 1.0, 3.0, 4.0, 4.0, 4.0], &ctrl, &ok).is_err(), "knots that decrease");
        assert!(fair_curve(3, &vec![f64::NAN; knots.len()], &ctrl, &ok).is_err(), "a knot that is not a number");
        assert!(fair_curve(9, &knots, &ctrl, &ok).is_err(), "a degree above the supported maximum");
        // Fewer than two interior control points: one free point (a triangle of a quadratic), and
        // a 3 × 3 net whose two edge layers leave nothing at all.
        let triangle = [[0.0, 0.0, 0.0], [5.0, 2.0, 0.0], [10.0, 0.0, 1.0]];
        let one = fair_curve(2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &triangle, &ok).unwrap_err();
        assert!(one.to_string().contains("at least two interior"), "{one}");
        let flat = [0.0, 0.0, 1.0, 2.0, 2.0];
        let grid = [[0.0, 0.0, 0.0]; 9];
        let kept = FairOptions::new(1.0, 4, true, 1.0).unwrap();
        assert!(
            fair_net([1, 1], [&flat, &flat], &Grid::from_fn([3, 3], |i, j| grid[j * 3 + i]).unwrap(), &kept).is_err(),
            "keep_edges on a 3 × 3 net"
        );
        // A net too large to fair, refused before anything is allocated for it.
        let line = [0.0, 0.0, 1.0, 1.0];
        let huge = fair_net([1, 1], [&line, &line], &Grid::from_fn([129, 129], |_, _| [0.0; 3]).unwrap(), &ok).unwrap_err();
        assert!(matches!(huge, KernelError::TooLarge(_)), "16641 control points: {huge}");
        // A singular system: a cubic whose last control points lie past the end of its domain
        // (0.5), where they do not move the curve at all, so the interior equations have no
        // unique solution and the run must refuse rather than move anything.
        let odd = [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0];
        let six: Vec<[f64; 3]> = (0..6).map(|i| [i as f64, 0.0, 1.0]).collect();
        let singular = fair_curve(3, &odd, &six, &ok).unwrap_err();
        eprintln!("hostile inputs: refused with {singular}");
        assert!(singular.to_string().contains("singular"), "{singular}");
    }

    #[test]
    fn a_small_quadratic_net_and_a_quintic_curve_fair_the_same_way() {
        // Degrees and band shapes away from the bicubic case. A 5 × 5 quadratic net has fewer
        // rows than the degree, so the solver's band is narrower than `degree_u + degree_v · cols`
        // (the coupling cannot reach that far in v); a quintic curve carries six basis functions
        // per span instead of four. Both must reproduce a zero-energy shape exactly.
        let (ku, kv) = (clamped(3, 2), clamped(3, 2));
        let degree = [2, 2];
        let (cu, cv) = (ku.len() - 3, kv.len() - 3);
        assert_eq!((cu, cv), (5, 5));
        let smooth = planar_net(&ku, &kv, degree, [0.0, 0.0, 0.0], [12.0, 1.0, 0.0], [-2.0, 9.0, 0.0]);
        let mut rough = smooth.clone();
        for &(i, j, d) in &[(1usize, 2usize, 2.5), (2, 2, -1.5), (3, 3, 3.0), (2, 1, -2.0)] {
            rough[j * cu + i][2] += d;
        }
        let faired = fair_net(
            degree,
            [&ku, &kv],
            &Grid::from_fn([cu, cv], |i, j| rough[j * cu + i]).unwrap(),
            &FairOptions::new(1.0, 8, false, 100.0).unwrap(),
        )
        .unwrap();
        let near = faired.ctrl.cells().iter().zip(&smooth).map(|(a, b)| dist(a, b)).fold(0.0, f64::max);
        eprintln!("5 × 5 quadratic net: faired to {near:e} mm, energy {:e} → {:e}", faired.report.energy_before, faired.report.energy_after);
        assert!(near < 1e-6 && faired.report.energy_after < 1e-9 * faired.report.energy_before);
        for j in 0..cv {
            for i in 0..cu {
                if i == 0 || i + 1 == cu || j == 0 || j + 1 == cv {
                    assert_eq!(faired.ctrl.get(i, j), Some(&rough[j * cu + i]), "({i}, {j}) is on the boundary and moved");
                }
            }
        }
        // A quintic curve, its middle pulled out.
        let knots = clamped(3, 5);
        let smooth = affine_curve(&knots, 5, [-10.0, 0.0, 2.0], [6.0, 4.0, -1.0]);
        let mut rough = smooth.clone();
        rough[3][1] += 3.0;
        rough[4][2] -= 2.0;
        let faired = fair_curve(5, &knots, &rough, &FairOptions::new(1.0, 8, false, 100.0).unwrap()).unwrap();
        let near = (1..smooth.len() - 1).map(|i| dist(&faired.ctrl[i], &smooth[i])).fold(0.0, f64::max);
        eprintln!("quintic curve: faired to {near:e} mm, energy {:e} → {:e}", faired.report.energy_before, faired.report.energy_after);
        assert!(near < 1e-6 && faired.report.energy_after < 1e-9 * faired.report.energy_before);
        assert_eq!(faired.ctrl[0], rough[0]);
        assert_eq!(faired.ctrl[rough.len() - 1], rough[rough.len() - 1]);
    }

    #[test]
    fn a_net_of_degree_one_across_fairs_along_its_curved_direction_only() {
        // Cubic along u, linear across v over two spans (three rows): `S_vv` is zero on every
        // span, so only the bending along u and the twist count. A plane's net bumped at its
        // middle row fairs back to the plane, the boundary rows bit-identical.
        let (ku, kv) = (clamped(3, 3), vec![0.0, 0.0, 1.0, 2.0, 2.0]);
        let degree = [3, 1];
        let (cu, cv) = (ku.len() - 4, kv.len() - 2);
        assert_eq!((cu, cv), (6, 3));
        let smooth = planar_net(&ku, &kv, degree, [0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [0.0, 20.0, 4.0]);
        let mut rough = smooth.clone();
        rough[cu + 2][2] += 2.0;
        rough[cu + 3][2] -= 1.0;
        let faired = fair_net(
            degree,
            [&ku, &kv],
            &Grid::from_fn([cu, cv], |i, j| rough[j * cu + i]).unwrap(),
            &FairOptions::new(1.0, 8, false, 100.0).unwrap(),
        )
        .unwrap();
        let near = faired.ctrl.cells().iter().zip(&smooth).map(|(a, b)| dist(a, b)).fold(0.0, f64::max);
        eprintln!("degree 3 × 1 net: faired to {near:e} mm, energy {:e} → {:e}", faired.report.energy_before, faired.report.energy_after);
        assert!(near < 1e-6 && faired.report.energy_after < 1e-9 * faired.report.energy_before);
        for i in 0..cu {
            for j in [0, cv - 1] {
                assert_eq!(faired.ctrl.get(i, j), Some(&rough[j * cu + i]), "({i}, {j}) is on the boundary and moved");
            }
        }
    }

    #[test]
    fn a_face_whose_edges_hold_its_whole_net_is_left_as_it_is() {
        // A single-span bicubic with its edges kept: every row is a boundary row or the one next
        // to it, so there is nothing to move. The run succeeds, the body is the one it was and
        // the report says no step was taken.
        let (ku, kv) = (clamped(1, 3), clamped(1, 3));
        let mut net = planar_net(&ku, &kv, [3, 3], [0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0, 15.0, 0.0]);
        net[5][2] += 2.0;
        let body = sheet(&ku, &kv, &net);
        let (faired, report) = fair_faces(&body, &[0], &FairOptions::new(1.0, 10, true, 1.0).unwrap()).unwrap();
        eprintln!("held face: energy {:e} → {:e}, {} steps", report.energy_before, report.energy_after, report.steps);
        assert_eq!(face_surface(&faired, 0), face_surface(&body, 0));
        assert_eq!(edge_records(&faired), edge_records(&body));
        assert_eq!((report.steps, report.movement), (0, 0.0));
        assert!(report.energy_before > 0.0 && report.energy_after == report.energy_before);
    }

    /// A one-face sheet body of the bicubic surface `net` (u fastest) over the clamped knots `ku`
    /// and `kv`, its sides the straight lines between the surface's corners — true while the
    /// net's boundary rows describe lines, which is how a plane's control net is built here.
    fn sheet(ku: &[f64], kv: &[f64], net: &[[f64; 3]]) -> Body {
        use crate::sew::{PartEdge, PartFace, PartUse, Parts, assemble};
        let (cu, cv) = (ku.len() - 4, kv.len() - 4);
        assert_eq!(net.len(), cu * cv, "the net does not fit the knots");
        let points: Vec<Vec<[f64; 3]>> = (0..cu).map(|i| (0..cv).map(|j| net[j * cu + i]).collect()).collect();
        let surface = b::Surface::Spline { u_knots: ku.to_vec(), v_knots: kv.to_vec(), points };
        let corners = [(0, 0), (cu - 1, 0), (cu - 1, cv - 1), (0, cv - 1)];
        let (u, v) = ([ku[3], ku[cu]], [kv[3], kv[cv]]);
        let uv = [[u[0], v[0]], [u[1], v[0]], [u[1], v[1]], [u[0], v[1]]];
        let vertices: Vec<[f64; 3]> = corners.iter().map(|&(i, j)| net[j * cu + i]).collect();
        let edges = (0..4)
            .map(|k| PartEdge {
                nurbs: None,
                curve: b::Curve::Line { start: vertices[k], end: vertices[(k + 1) % 4] },
                interval: [0.0, 1.0],
                vertices: [k as u32, ((k + 1) % 4) as u32],
            })
            .collect();
        let loops = vec![
            (0..4)
                .map(|k| PartUse {
                    edge: k as u32,
                    forward: true,
                    given: Some(b::Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![uv[k], uv[(k + 1) % 4]] }),
                })
                .collect(),
        ];
        let made = assemble(Parts { vertices, edges, faces: vec![PartFace { surface, forward: true, loops }], tolerance: 1e-6 }, false).unwrap();
        crate::native::body_from_smlib(&solidcraft_smlib::Brep::from_record(&made.brep, true).unwrap()).unwrap()
    }

    /// Each edge of the body's record: its curve, interval and vertices.
    fn edge_records(body: &Body) -> Vec<(b::Curve, [f64; 2], [u32; 2])> {
        crate::native::brep_record(body).unwrap().edges.into_iter().map(|e| (e.curve, e.interval, e.vertices)).collect()
    }

    /// The surface of face `i` of the body's record.
    fn face_surface(body: &Body, i: usize) -> b::Surface {
        crate::native::brep_record(body).unwrap().faces[i].surface.clone()
    }

    #[test]
    fn a_body_face_is_faired_with_its_edges_untouched_and_its_history_kept() {
        // A sheet whose middle is bumped: fairing the face must smooth it without moving a single
        // edge, vertex or neighbouring face of the body, and must leave a history saying so.
        let (ku, kv) = (clamped(3, 3), clamped(3, 3));
        let (cu, cv) = (ku.len() - 4, kv.len() - 4);
        let smooth = planar_net(&ku, &kv, [3, 3], [0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0, 15.0, 0.0]);
        let mut rough = smooth.clone();
        for &(i, j, d) in &[(2usize, 2usize, 3.0), (3, 3, -2.0), (2, 3, 1.5)] {
            rough[j * cu + i][2] += d;
        }
        let body = sheet(&ku, &kv, &rough);
        let before = edge_records(&body);
        assert!(!before.is_empty());
        let (faired, report) = fair_faces(&body, &[0], &FairOptions::new(1.0, 6, false, 50.0).unwrap()).unwrap();
        let after = edge_records(&faired);
        eprintln!(
            "face: energy {:e} → {:e} in {} steps, movement {:e} mm, {} faces, history {}",
            report.energy_before,
            report.energy_after,
            report.steps,
            report.movement,
            faired.face_count(),
            faired.history().is_some()
        );
        assert_eq!(before, after, "the body's edges must be the very ones it had");
        assert_eq!(faired.face_count(), 1);
        assert!(faired.history().is_some(), "the face came from the one it was faired from");
        assert!(report.energy_after < 1e-9 * report.energy_before);
        // The face's own surface is back on the plane, boundary rows bit for bit as they were.
        let b::Surface::Spline { points, .. } = face_surface(&faired, 0) else { panic!("a B-spline face") };
        for j in 0..cv {
            for i in 0..cu {
                let p = points[i][j];
                let want = if i == 0 || i + 1 == cu || j == 0 || j + 1 == cv { rough[j * cu + i] } else { smooth[j * cu + i] };
                assert!(dist(&p, &want) < 1e-6, "({i}, {j})");
            }
        }
        // A solid keeps its shells, its faces and its edges: thickening the sheet gives a closed
        // body whose top face is still the B-spline one, and fairing that face leaves the body a
        // solid of the same shape.
        let solid = crate::thicken(&body, 2.0, false).unwrap();
        assert!(!solid.is_surface());
        let top = (0..solid.face_count()).find(|&i| matches!(face_surface(&solid, i), b::Surface::Spline { .. }));
        let Some(top) = top else { panic!("the thickened sheet keeps its B-spline face") };
        let edges = edge_records(&solid);
        let (faired, report) = fair_faces(&solid, &[top], &FairOptions::new(1.0, 6, false, 0.5).unwrap()).unwrap();
        eprintln!("thickened solid: energy {:e} → {:e}, movement {:e} mm", report.energy_before, report.energy_after, report.movement);
        assert!(!faired.is_surface() && faired.face_count() == solid.face_count() && faired.shell_count() == solid.shell_count());
        assert_eq!(edge_records(&faired), edges);
        assert!(report.energy_after < report.energy_before);
        // A face with no control points to fair is refused, by name.
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let refused = fair_faces(&cube, &[0], &FairOptions::new(1.0, 1, false, 1.0).unwrap()).unwrap_err();
        eprintln!("cube face: {refused}");
        assert!(refused.to_string().contains("not a B-spline face"), "{refused}");
        assert!(fair_faces(&body, &[7], &FairOptions::new(1.0, 1, false, 1.0).unwrap()).is_err(), "no such face");
        assert!(fair_faces(&body, &[], &FairOptions::new(1.0, 1, false, 1.0).unwrap()).is_err(), "no face picked");
    }

    #[test]
    fn an_already_fair_curve_is_left_where_it_is() {
        // Zero bending energy, no reason to move: nothing churns, and an input at the minimum
        // cannot be faired into a worse shape.
        let knots = clamped(4, 3);
        let smooth = affine_curve(&knots, 3, [1.0, 2.0, 3.0], [5.0, -1.0, 0.5]);
        let opts = FairOptions::new(1.0, 20, false, 10.0).unwrap();
        let faired = fair_curve(3, &knots, &smooth, &opts).unwrap();
        eprintln!(
            "already fair: energy {:e} → {:e}, {} steps, {:e} mm",
            faired.report.energy_before, faired.report.energy_after, faired.report.steps, faired.report.movement
        );
        assert!(faired.report.energy_after <= faired.report.energy_before);
        assert!(faired.report.energy_after >= 0.0);
        assert!(faired.report.movement < 1e-9, "{:e}", faired.report.movement);
        for (i, want) in smooth.iter().enumerate() {
            assert!(dist(&faired.ctrl[i], want) < 1e-9, "control point {i}");
        }
    }
}
