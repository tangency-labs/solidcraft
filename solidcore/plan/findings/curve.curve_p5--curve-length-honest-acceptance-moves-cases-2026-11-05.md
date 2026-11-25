# A curve's length: span ends from their own knot span, steps on the quadrature estimate (2026-11-05)

`Curve::length_base` integrates `|C'|` over each span of the curve's cache (each within one
knot span) with `integrate_opt_in`. Two changes, both the only path:

1. **Span end from its own knot span.** Every sample was evaluated with `bFromLeft = TRUE`,
   which at a knot takes the span above. A span's end is a knot, so its last sample was the
   next span's value. Where the speed jumps at a C0 knot (a double knot between unequal
   rational arcs), the steps shrank toward the knot and underflowed. The span's length was
   then taken as zero: -0.19 relative at 1e-10 on a circle of six unequal arcs. The end is now
   evaluated with `FALSE` (the span below, its own), as the area integrals' iso-u pieces
   (WR-670).
2. **Quadrature step estimate.** `integrate_opt_in` accepts each step on the larger of the
   embedded estimate and the fifth divided difference of its samples (`OdeOptions::quadrature`,
   the precise-property integrals' code, b3c30ed80). The embedded estimate alone vanishes where
   `g''''` changes sign.

Test: `crates/solidcore/tests/curve_length_holds_its_accuracy.rs`. A 120 degree arc, the
parabola `(t, t^2)` on [0, 10], and the six-arc circle each hold every accuracy from 1e-3 to
1e-10 against their closed forms.

## Moved cases (difftest, 13 units)

The run covered curve.curve_p3, curve.curve_p5, mass.integrator, boolean.api_brep,
blend.fillet_executive_p1/p2/p3, blend.fillet_geom_p2/p4/p5, blend.fillet_standard_solver_p2,
primitive.primitive_creation_p3 and topology.brep_p5. Cases moved in curve.curve_p5 (6),
curve.curve_p3 (1) and mass.integrator (14). The others are unchanged; fillet_p3's one failure
is the earlier source-file-corner_box-edge9.

Each case's error is against a reference. Curves and edges: composite 20-point Gauss–Legendre
of the kernel's `|C'|`, with 256 subintervals per piece, split at every knot, over the curve's
natural interval when it is clamped. Polynomial-sine integrals: closed form. The kind-3 patch:
a tensor Gauss–Legendre of its `|Su x Sv|`. The table gives each side's relative error, C++
first, then Rust.

| unit / case | acc | C++ | Rust |
|---|---|---|---|
| p5 length_raw_gap1.5e-12 | 1e-4 | 1.27e-6 | 9.38e-7 |
| p5 length_raw_gap1.9e-12 | 1e-4 | 1.27e-6 | 9.38e-7 |
| p5 gen0561 | 1e-6 | 9.73e-9 | 8.24e-9 |
| p5 gen2395 (Hermite) | 1e-6 | -6.32e-8 | -6.15e-8 |
| p5 gen2629 (rational) | 1e-6 | -9.37e-9 | -1.06e-8 |
| p5 gen2638 | 1e-3 | -3.21e-6 | -2.95e-6 |
| p3 gen1005 (parameter at arc length) | - | 5.95e-9 | -1.33e-8 |
| mass review_surface_area_kind3_eps0.001 | 1e-3 | 1.66e-5 | 1.66e-5 |
| mass review_edge_rational_1e-06 | 1e-6 | 4.04e-9 | 5.16e-9 |
| mass gen0054 / 0237 / 0509 / 1177 (edges) | 1e-6 | 6.3e-9 / 3.0e-9 / 9.3e-9 / -7.3e-8 | 8.5e-9 / 4.6e-9 / 1.1e-8 / -6.9e-8 |
| mass gen0339 / 0426 (integrate) | 1e-6 | -2.0e-8 / -2.4e-7 | -1.9e-8 / -2.5e-7 |
| mass gen0994 (amplitude 4.7e-16) | 1e-9 | -4.4e-6 | -4.7e-6 |
| mass review_edge_constant x6 (point curve) | - | noise ~3e-17 | exactly 0 (after the rounding floor below) |

## Resolved: a point curve's length at tight accuracy

The `review_edge_constant` curves are points (three equal control points). `|C'|` is pure
rounding noise there, about 1e-17, so its error relative to itself is O(1). The embedded
estimate passed it. The fifth divided difference does not: at 1e-9 over [0, 1] the steps
underflow. With the default kernel the failure is hidden as 0, which is exact. With
`integrator_propagates_failure`, which SolidCraft sets, the length errors with
`mass::integrator::step_control` 1001. The root is that the error scale has no floor at the
integrand's own rounding (about ε·max|P|·p/Δknot, not ε·|C'|). Choosing that floor, or making
a span that is a point to rounding exact zero, is left for a decision.

Resolved (same branch, follow-on commits). The fix does both:
Superseded later the same day: the length is integrated with
`integrate_rounded_with_kernel`; `speed_rounding_bound` and `is_point_on` below were replaced by
`BSplineCurve::span_speed` (the speed in difference form and its own bound, `None` for a point
span), and a floor-accepted step owes its error to the integral (`E`, now
`(1 + gamma_16) |h| sum |w_j| (delta + gamma_11 |g_j|)`). See
plan/findings/curve.curve_p5--speed-in-difference-form-moves-cases-2026-11-05.md.

- **A floor at the integrand's rounding.** A step's error scale is floored at the rounding
  of its error estimates over the accuracy. That rounding is
  `E = |h| sum |w_j| (delta + gamma_8 |g_j|)`, where `delta` bounds each sample's absolute
  rounding (`OdeOptions::rounding`, `mass/integrator.rs` `estimate_rounding`). For a
  B-spline's speed on a knot span of length D, `delta = gamma_(7p+10) S max|P|`, with
  `S = 2p/D`, or `4 p (w_max/w_min)/D` for a rational curve
  (`BSplineCurve::speed_rounding_bound`). An accuracy below that cannot be resolved; the
  tolerance is not loosened. Where the rounding is not known (the area/volume integrands,
  a Hermite curve, the raw ODE) there is no floor, and the step control is upstream's.
- **Exact zero for a point span.** A span whose control points are exactly one point has
  length 0 (`BSplineCurve::is_point_on`, exact homogeneous products). The floor alone
  leaves noise.

The six `review_edge_constant_*_1` cases give exactly 0 under both kernel settings: 5
re-worded, 1 new known mismatch (`0.001_[0, 1]_1`, C++ 7.1e-17). Nothing else moves in
curve.curve_p3, curve.curve_p5, mass.integrator or boolean.api_brep.
