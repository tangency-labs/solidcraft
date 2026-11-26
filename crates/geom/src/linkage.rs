//! Geometric continuity of order `n` (G0 … G4) between two surfaces that share a curve, in the
//! linkage-curve formulation, as the fill imposes it and as a seam is measured.
//!
//! **The criterion.** Two surfaces meeting along a curve are G^n there exactly when the
//! *linkage curves* crossing it — a curve on one surface continued by a curve on the other — are
//! G^n curves (Pegna & Wolter, "Geometrical criteria to guarantee curvature continuity of blend
//! surfaces", J. Mech. Des. 114, 1992, for n = 2; Hermann, Lukács & Wolter, "Geometrical
//! criteria on the higher order smoothness of composite surfaces", CAGD 16, 1999, for any n).
//! In a form that needs no reparameterisation: write the neighbour, near a point `p` of the
//! curve, as a height `H` over its tangent plane along its unit normal `n`,
//! `q = p + x + H(x)·n` for `x` in the tangent plane, so that `H(0) = 0` and `∇H(0) = 0`. A
//! curve `c(σ)` from `p` lies on a surface meeting the neighbour with G^n exactly when its height
//! above the neighbour, `n·(c(σ) − p) − H(x(σ))` with `x(σ)` the part of `c(σ) − p` in the
//! tangent plane, vanishes to order `n` in σ. With `H(x) = Σ_{m ≥ 2} H_m(x) / m!` (`H_m` the
//! m-th derivative of `H` at `p`, a form of degree m) and `x(σ) = Σ_j x_j σʲ / j!`
//! (`x_j` the tangent-plane part of `c⁽ʲ⁾`), the condition of order `k` is
//!
//! `n·c⁽ᵏ⁾ = k! · [σᵏ] Σ_{m=2}^{k} H_m(x(σ)) / m!`,
//!
//! whose right-hand side reads `c′ … c⁽ᵏ⁻¹⁾` only: order 1 is `n·c′ = 0` (the tangent plane),
//! order 2 `n·c″ = H₂(x₁)` (the normal curvature in the curve's direction), order 3
//! `n·c‴ = 3·H₂(x₁, x₂) + H₃(x₁)` (the rate of change of that curvature), and order 4 its rate of
//! change in turn. For a unit `a`, `H₂(a)` is the curvature of the normal section along `a` and
//! `H₃(a)`, `H₄(a)` are its first and second derivatives by arc length.
//!
//! **The jet.** [`surface_jet`] reads `H₂ … H_n` off a surface record's exact partial
//! derivatives: along a tangent vector `a`, the curve on the surface whose foot in the tangent
//! plane runs straight along `a` at unit rate has `n·γ⁽ᵐ⁾(0) = H_m(a)`; its parameter series
//! `(u(τ), v(τ))` is found order by order (the part of each coefficient of `S(u(τ), v(τ))` in the
//! tangent plane is `a`, then zero), and `H_m` as a binary form from its values along
//! `t + λ·w` at `m + 1` values of λ (a polynomial of degree m in λ, so exactly).
//!
//! Input is hostile: orders are capped at [`MAX_CONTINUITY`], and every evaluation is checked.

use crate::brep::eval::SurfaceEval;
#[cfg(test)]
use crate::prelude::*;
use solidcore_record::Vec3;
pub use solidcraft_kernel_services::form_fill::linkage::{Jet, MAX_CONTINUITY, Target};

/// Read the exact source partials; the kernel constructs its directed height jet.
pub fn surface_jet(s: &SurfaceEval, uv: [f64; 2], normal: Vec3, leave: Vec3, order: usize) -> Option<Jet> {
    if !(2..=MAX_CONTINUITY).contains(&order) {
        return None;
    }
    solidcraft_kernel_services::form_fill::linkage::from_partials(&s.partials(uv[0], uv[1])?, normal, leave, order)
}

#[cfg(test)]
#[path = "linkage_tests.rs"]
mod tests;
