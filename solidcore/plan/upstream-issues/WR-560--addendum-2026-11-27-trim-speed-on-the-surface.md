# WR-560: a trim's speed (test 11) is read on its surface where its image's speed is proved

Updates: WR-560

WR-560 maps a face's UV trim by the Cholesky factor of its surface's first fundamental
form at the trim's middle before tests 13 and 15. Test 11, the first-derivative test
("Bad SpeedPt", `SmBSplineCurve.cpp`, `AssertValid`), reads the trim in raw (u, v). A
speed is a length per unit of parameter, so test 11 has the defect WR-560 fixed for test
13: on a face whose two parameters have very different lengths on the surface, a trim
reads as nearly stopped where it runs along the parameter whose unit step is long,
although on the surface it moves at its usual speed. The fork maps tests 13 and 15 only.

Trigger (MEASURED, main 235991e554): the upper half of a torus (centre circle Ø1000, tube
Ø1), both edges of its flat face rounded with 0.05, less a Ø0.1 notch 0.25 deep on the flat
face's outer rim. The boolean's result failed test 11 on a cubic UV trim of the outer round
(a surface of revolution), over a parameter range of 0.218:

| | raw (u, v) | mapped (lengths on the surface) |
|---|---|---|
| box diagonal | 0.967 | 0.128 |
| tolerance (box / range / 10000) | 4.44e-4 | 5.86e-5 |
| slowest interior speed the search finds | 3.54e-4 | 0.969 |

Change (not upstream, no flag): test 11 searches, evaluates and reports the raw trim as
upstream does. A solution below the tolerance is passed over only where a lower bound on the
image's speed `|d/dt S(c(t))|` is at least the tolerance on the surface over the whole trim.
The tolerance on the surface is the mapped box's (`bounded_scale(mapped_size, size_t)`, as
test 13's). The bound is `speed_clears_on_surface`
(`crates/solidcore/src/nurbs/b_spline_curve/trim_image.rs`): the pieces, the composition
of each polynomial piece of the trim with each polynomial patch of the surface in rounded
interval arithmetic, and the halving budget of `radius_clears_on_surface` (WR-560's
addendum of 2026-11-13). On each piece `|g'| >= (N . n)_lo / (|n| W_hi^2)` in the piece's
parameter `s`, with `g' = N / W^2`, and over `h` (the piece's half-length rounded up) in the
trim's parameter `t = m + h s`. No sampling: the speed is proved, never read from the
constant metric, which only sizes the tolerance. A rational trim, a procedural blend, a
trim or surface without a NURBS form, a bound that is not finite and a trim the budget does
not prove are reported as before. A curve that is no face's trim, an unbounded one, or one
whose surface has no metric is tested as before. Test 12 (3D) is unchanged.

Why not test 11 on the mapped trim (the first version of this change, review on its pull
request): the constant metric of the trim's middle is exact on a plane only. On a regular
surface whose metric changes along the trim, the mapped trim can move fast where the image
nearly stops. On `S(u, v) = (u + 16 u^3 / (3 e^3), v, 0)`, `e = 1e-6`, the trim
`c(t) = (e (t - 1/4), (t - 1/4)^2)` mapped at its middle (`|S_u|` about 1e6) moves at about
1 or more, while its image moves at 1e-6 at `t = 1/4` (`|S_u| = 1`), against a tolerance of
about 1e-4. Raw test 11 reports it, and so does this change; test 11 on the mapped trim
passed it.

Tests (`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`):
`issue_23062_uv_trim_speed_is_measured_on_its_surface` (a cubic arch 0.4 x 0.4 on a plane
100000 x 1: test 11 fires in raw (u, v) on a raw speed of 6e-6 against 3e-5; the image moves
at least 0.6, proved, so it is passed over), and
`issue_23062_a_trim_whose_image_slows_off_its_middle_is_reported` (the example above,
reported). The trigger is SolidCraft's `a_notch_through_a_thin_round_on_a_large_half_torus_is_cut`
(`crates/engine/src/boolean_regression_tests.rs`) and the kernel case
`booleans/23062-notch-through-thin-round-on-large-half-torus`.

Code: `BSplineCurve::assert_valid_with` (`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`)
and `speed_clears_on_surface` (`crates/solidcore/src/nurbs/b_spline_curve/trim_image.rs`).
