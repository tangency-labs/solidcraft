# mass.integrator: each edge component has an error scale of its own size, 36 edge cases move (2026-11-06)

- **Unit:** `mass.integrator` (36 edge cases move, every one already a known mismatch;
  no case that passes moves). No other unit reaches the change: elsewhere wire edges are
  integrated with a cross-section radius of 0, whose tube integrands are exactly zero.
- **Change (#2887):** `curve_properties` (`SmEdge::ComputePreciseProperties`) integrates
  27 components: the length, the area and volume of the tube of cross-section radius `r`
  about the edge, and their eight moment vectors. Upstream gives the length the error
  scale `L / 10` (`L` the approximate length) and the other 26 `BIG_DOUBLE`, so their step
  errors divide to about zero: the steps are the length's and their accuracy is whatever
  that gives. Now each has a tenth of its own size: the tube's area `2 pi r L` and volume
  `pi r^2 L`, and a moment of power `n` that measure times `D^n`, with `D` the start's
  distance from the origin of computation plus `L` (every point is within its arc length of
  the start).
- **Not moved:** cases with `r = 0` (bit for bit: the tube integrands are zero), the
  zero-length `review_edge_constant_*` and `review_edge_scaled_exact_arc_1e-09_0` (values of
  order 1e-37, below the integrator's absolute floor, #2886).

## Who is right

Each moved case is graded against an independent reference: the edge's rational quadratic
(or line) evaluated in float64 and every component integrated by a composite Gauss-Legendre
rule (200 panels of 40 points; its own error is below 1e-13 of every size here), and for
`review_edge_exact_quarter_circle_*` also against the closed forms of
`crates/solidcore/tests/edge_properties_hold_their_accuracy.rs`. The figure is the largest
component error over the component's size: the length; the tube's area and volume; a moment
of power `n` its measure times `D^n`, `D` the largest distance of the edge from the origin
of computation, measured on 2001 samples. The C++ has upstream's scales, so its values are
those of the main column within the difftest's 1e-9 wherever main agreed with it.

| case | r | accuracy | main (f826c1613) | this change |
|---|---|---|---|---|
| `edge_0_0` | 0.1 | 1e-06 | 3.60e-07 (out[6]) | 2.17e-08 (out[23]) |
| `edge_0_2` | 0.1 | 1e-06 | the B-rep wire edge of `edge_0_0`: its tube values are `edge_0_0`'s, bit for bit, on both sides | |
| `review_edge_exact_quarter_circle_1e-06_[0, 1]_0` | 0.1 | 1e-06 | 4.53e-07 (out[23]) | 5.28e-08 (out[19]) |
| `review_edge_exact_quarter_circle_1e-09_[0, 1]_0` | 0.1 | 1e-09 | 2.28e-09 (out[24]) | 1.51e-11 (out[4]) |
| `review_edge_rational_1e-06_[0, 1]_0` | 0.1 | 1e-06 | 1.74e-07 (out[13]) | 1.15e-07 (out[13]) |
| `review_edge_rational_1e-06_[0.1, 0.9]_0` | 0.1 | 1e-06 | 9.40e-08 (out[2]) | 1.73e-08 (out[2]) |
| `review_edge_rational_1e-09_[0, 1]_0` | 0.1 | 1e-09 | 2.38e-10 (out[1]) | 9.02e-11 (out[13]) |
| `gen0014` | 0.05 | 1e-06 | 7.30e-07 (out[25]) | 5.35e-08 (out[25]) |
| `gen0015` | 5000 | 1e-06 | 4.71e-08 (out[12]) | 1.03e-07 (out[24]) |
| `gen0053` | 5000 | 1e-06 | 7.46e-08 (out[2]) | 4.25e-08 (out[13]) |
| `gen0089` | 5e-07 | 1e-06 | 1.98e-07 (out[12]) | 1.98e-07 (out[12]) |
| `gen0170` | 0.05 | 1e-06 | 1.25e-07 (out[0]) | 2.51e-07 (out[0]) |
| `gen0306` | 5e-07 | 1e-06 | 3.01e-07 (out[24]) | 3.01e-07 (out[24]) |
| `gen0325` | 0.05 | 1e-06 | 4.10e-07 (out[25]) | 1.42e-07 (out[25]) |
| `gen0370` | 0.05 | 1e-06 | 7.12e-07 (out[12]) | 1.51e-07 (out[0]) |
| `gen0410` | 5e-07 | 1e-06 | 2.13e-05 (out[13]) | 2.13e-05 (out[13]) |
| `gen0415` | 5e-07 | 1e-06 | 1.69e-07 (out[0]) | 1.69e-07 (out[0]) |
| `gen0417` | 5e-07 | 1e-06 | 2.62e-07 (out[16]) | 2.62e-07 (out[16]) |
| `gen0599` | 0.05 | 1e-06 | 1.45e-07 (out[0]) | 1.72e-07 (out[13]) |
| `gen0622` | 5000 | 1e-06 | 9.66e-09 (out[2]) | 3.26e-08 (out[1]) |
| `gen0624` | 0.05 | 1e-06 | 4.03e-07 (out[25]) | 5.28e-08 (out[13]) |
| `gen0658` | 5000 | 1e-06 | 1.25e-08 (out[14]) | 1.14e-08 (out[9]) |
| `gen0662` | 5e-07 | 1e-06 | 1.87e-07 (out[14]) | 1.87e-07 (out[14]) |
| `gen0672` | 0.05 | 1e-06 | 2.46e-07 (out[25]) | 7.77e-08 (out[13]) |
| `gen0674` | 5e-07 | 1e-06 | 4.22e-07 (out[25]) | 4.22e-07 (out[25]) |
| `gen0700` | 5000 | 1e-06 | 2.96e-09 (out[25]) | 4.79e-09 (out[25]) |
| `gen0713` | 0.05 | 1e-06 | 2.57e-07 (out[15]) | 1.05e-07 (out[15]) |
| `gen0765` | 5e-07 | 1e-06 | 5.46e-07 (out[1]) | 5.46e-07 (out[1]) |
| `gen0794` | 0.05 | 1e-06 | 6.02e-07 (out[14]) | 1.55e-07 (out[19]) |
| `gen0803` | 5e-07 | 1e-06 | 1.36e-07 (out[26]) | 1.36e-07 (out[26]) |
| `gen0832` | 5000 | 1e-06 | 5.79e-08 (out[25]) | 1.42e-07 (out[8]) |
| `gen0936` | 5e-07 | 1e-06 | 3.33e-07 (out[25]) | 3.33e-07 (out[25]) |
| `gen0940` | 5e-07 | 1e-06 | 9.17e-08 (out[2]) | 9.17e-08 (out[0]) |
| `gen1072` | 5000 | 1e-06 | 6.23e-09 (out[14]) | 2.24e-08 (out[14]) |
| `gen1106` | 5e-07 | 1e-06 | 2.72e-07 (out[14]) | 2.72e-07 (out[26]) |
| `gen1191` | 5e-07 | 1e-06 | 3.70e-07 (out[6]) | 3.70e-07 (out[18]) |

`review_edge_exact_quarter_circle_1e-09_[0, 1]_0`, the issue's case: `out[18]` (the
volume's `x^2` moment) against the closed form `pi r^2 int_0^(pi/2) (cos t - 0.37)^2 dt`
(`r` 0.1) = 8.181969578704593e-3 was -2.34e-8 relative on main (C++ -1.88e-8) at a
requested 1e-9; it is 1.72e-11 now (8.181969578845394e-3).

Every case with `r` 0.05 or 0.1 holds its accuracy now. By this measure only the issue's
case missed on main; the others held it by chance of the length's steps, and nothing held
them to it (by its own value, `gen0370` missed 1e-6 by 1.3e-5 on `out[7]`, now 8.0e-7).
The `r = 5000` cases (`gen0015`, `gen0053`, `gen0622`, `gen0658`, `gen0700`,
`gen0832`, `gen1072`) held it already, as the length's steps were fine enough for a tube
far wider than the curve; they move within it. The `r = 5e-7` cases are micro-scale
curves (sizes near 1e-5 and moments below 1e-20): their components are below the
integrator's absolute floor `EFF_ZERO` and move by rounding only; `gen0410` and
`gen0662` miss 1e-6 of size by up to 2.1e-5 alike on both sides. That is #2886 (the
absolute floor of a tiny integral), not this change.

## What is left

- #2886: an integral below `EFF_ZERO` is held to an absolute bound (the `r = 5e-7` cases).

## Goldens (2026-11-07)

The difftest above is gone (#2816). The goldens of `mass.integrator` move in 43 cases, every
one blessed with this change. The goldens' generated cases are not the difftest's: same
names, other seeds, so the `gen` rows below are their own. Each is graded as above, on the
goldens' inputs, against the same independent reference (composite Gauss-Legendre, 200
panels of 40 points; 1000 panels give the same figures on the cusp and rational cases). The figure is the worst
component error over its size. "Largest move" is the largest change of a component, over
its size.

| case | r | accuracy | main: worst error / size | this PR | largest move / size |
|---|---|---|---|---|---|
| `edge_0_0` | 0.1 | 1e-06 | 3.60e-07 | 2.17e-08 | 3.46e-07 |
| `edge_0_2` | | | (query 2: the wire edge of `edge_0_0`; its area, volume and moments are `edge_0_0`'s, bit for bit, before and after) | | |
| `gen0014` | 0.05 | 1e-06 | 7.30e-07 | 5.35e-08 | 6.77e-07 |
| `gen0015` | 5000 | 1e-06 | 4.71e-08 | 1.03e-07 | 5.62e-08 |
| `gen0053` | 5000 | 1e-06 | 7.46e-08 | 4.25e-08 | 9.53e-08 |
| `gen0170` | 0.05 | 1e-06 | 1.25e-07 | 2.51e-07 | 1.27e-07 |
| `gen0225` | 5000 | 1e-06 | 5.31e-08 | 5.22e-08 | 5.62e-08 |
| `gen0311` | 5000 | 1e-06 | 8.06e-09 | 5.25e-09 | 2.81e-09 |
| `gen0325` | 0.05 | 1e-06 | 4.10e-07 | 1.42e-07 | 2.68e-07 |
| `gen0366` | 0.05 | 1e-06 | 2.14e-06 | 7.01e-08 | 2.07e-06 |
| `gen0367` | 5000 | 1e-06 | 2.87e-08 | 3.09e-08 | 2.64e-09 |
| `gen0370` | 0.05 | 1e-06 | 7.12e-07 | 1.51e-07 | 6.77e-07 |
| `gen0448` | 5000 | 1e-06 | 5.00e-08 | 7.12e-08 | 2.21e-08 |
| `gen0598` | 5000 | 1e-06 | 4.01e-08 | 1.52e-07 | 1.12e-07 |
| `gen0599` | 0.05 | 1e-06 | 1.45e-07 | 1.72e-07 | 2.24e-07 |
| `gen0622` | 5000 | 1e-06 | 9.66e-09 | 3.26e-08 | 2.70e-08 |
| `gen0624` | 0.05 | 1e-06 | 4.03e-07 | 5.28e-08 | 3.50e-07 |
| `gen0658` | 5000 | 1e-06 | 1.25e-08 | 1.14e-08 | 3.65e-09 |
| `gen0672` | 0.05 | 1e-06 | 2.46e-07 | 7.77e-08 | 1.68e-07 |
| `gen0700` | 5000 | 1e-06 | 2.96e-09 | 4.79e-09 | 4.39e-09 |
| `gen0713` | 0.05 | 1e-06 | 2.57e-07 | 1.05e-07 | 2.02e-07 |
| `gen0754` | 5000 | 1e-06 | 4.81e-09 | 7.93e-08 | 8.21e-08 |
| `gen0794` | 0.05 | 1e-06 | 6.02e-07 | 1.55e-07 | 4.53e-07 |
| `gen0807` | 5000 | 1e-06 | 4.51e-08 | 6.96e-08 | 2.44e-08 |
| `gen0831` | 5000 | 1e-06 | 4.16e-09 | 4.21e-08 | 4.38e-08 |
| `gen0832` | 5000 | 1e-06 | 5.79e-08 | 1.42e-07 | 8.41e-08 |
| `gen0914` | 5000 | 1e-06 | 1.97e-08 | 1.07e-07 | 1.03e-07 |
| `gen1011` | 0.05 | 1e-06 | 5.71e-06 | 6.32e-08 | 5.65e-06 |
| `gen1072` | 5000 | 1e-06 | 6.23e-09 | 2.24e-08 | 1.75e-08 |
| `gen1082` | 5000 | 1e-06 | 6.07e-09 | 1.82e-08 | 1.26e-08 |
| `gen1136` | 5000 | 1e-06 | 7.18e-09 | 7.15e-09 | 4.44e-10 |
| `gen1186` | 5000 | 1e-06 | 4.93e-08 | 3.47e-08 | 8.14e-08 |
| `review_edge_cusp_0.001_[0, 1]_0` | 0.1 | 0.001 | 1.70e-15 | 1.61e-04 | 1.61e-04 |
| `review_edge_cusp_0.001_[0.1, 0.9]_0` | 0.1 | 0.001 | 6.76e-15 | 7.90e-05 | 7.90e-05 |
| `review_edge_cusp_1e-06_[0, 1]_0` | 0.1 | 1e-06 | 1.70e-15 | 5.46e-07 | 5.46e-07 |
| `review_edge_cusp_1e-06_[0.1, 0.9]_0` | 0.1 | 1e-06 | 6.76e-15 | 2.56e-07 | 2.56e-07 |
| `review_edge_exact_quarter_circle_1e-06_[0, 1]_0` | 0.1 | 1e-06 | 4.53e-07 | 5.28e-08 | 4.33e-07 |
| `review_edge_exact_quarter_circle_1e-06_[0.1, 0.9]_0` | 0.1 | 1e-06 | 1.32e-06 | 6.87e-08 | 1.25e-06 |
| `review_edge_rational_0.001_[0, 1]_0` | 0.1 | 0.001 | 1.23e-03 | 6.83e-05 | 1.23e-03 |
| `review_edge_rational_0.001_[0.1, 0.9]_0` | 0.1 | 0.001 | 2.24e-03 | 1.08e-05 | 2.23e-03 |
| `review_edge_rational_1e-06_[0, 1]_0` | 0.1 | 1e-06 | 1.74e-07 | 1.15e-07 | 5.91e-08 |
| `review_edge_rational_1e-06_[0.1, 0.9]_0` | 0.1 | 1e-06 | 9.40e-08 | 1.73e-08 | 7.67e-08 |
| `review_edge_scaled_exact_arc_1000000000.0_0` | 1e+08 | 1e-06 | 2.60e-08 | 3.47e-08 | 1.97e-08 |

Every case holds its accuracy now. On main five did not: `gen0366` and `gen1011` (r 0.05,
1e-6: 2.1e-6 and 5.7e-6), `review_edge_exact_quarter_circle_1e-06_[0.1, 0.9]_0` (1.3e-6),
and both `review_edge_rational_0.001_*` (1.2e-3 and 2.2e-3). The `r = 5000` cases and the
scaled arc (r 1e8) held it already and move within it. The four `review_edge_cusp_*` cases
(a quadratic that runs out and back, its speed zero at its middle) were exact on main to
1e-14. The steps the length's error alone chose happened to be exact there, though nothing
held the tube to it. They now hold the accuracy asked (1.6e-4 at 1e-3, 5.5e-7 at 1e-6),
as error control promises and no more. The one other case that differs here,
`review_edge_constant_1e-09_[0, 1]_0` (zero length, values near 1e-37), differs on macOS
arm64 on main as well, and not on CI's Linux. It is not this change, and not blessed.

