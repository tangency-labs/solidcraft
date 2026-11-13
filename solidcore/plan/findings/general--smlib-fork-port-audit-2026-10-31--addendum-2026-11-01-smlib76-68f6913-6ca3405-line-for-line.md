# General: addendum (2026-11-01): smlib#76's 68f6913 and 6ca3405 checked line for line against their ports

Updates: plan/findings/general--smlib-fork-port-audit-2026-10-31.md (with its addendum `--addendum-2026-10-31-section-a-already-covered.md`, which matched these two commits to their rebased twins by subject only)

tangency-labs/smlib#76 (merged into the fork's `replay/1.0.1` as cd47efd; solidcore#2243) carries two commits that solidcore had already ported from earlier twins: 68f6913 from 0cc8cd4 (#2080) and 6ca3405 from ecf1963 (WR-668, #2086). This addendum checks them line for line (solidcore#2246).

## The twins are the same patch

GitHub's patches for 68f6913 and 0cc8cd4 (4 files), and for 6ca3405 and ecf1963 (1 file), are identical once the hunk headers are dropped; only the line numbers differ. The fork's `NL_CrvFit.cpp` at 6ca3405 is the pinned file (3fd5b95) plus 68f6913's hunks and nothing else (88 changed lines, the commit's +74 −14).

## 68f6913: interpolation at given parameters

- `N_FitCrvHighDerivsParams` → `nurbs::crv_fit::p3::interp_high_derivs_params`. Same checks (`p < 1`, `p > nn + kk + ll`, `kk >= p`, `ll >= p`), the same knot vector (`N_FitCalcKnotsHighEndDerivs`), the same banded matrix rows in the same order (start derivatives from `N_BasisDerivs` at `u[0]` from the left, interior points from `N_BasisEval`, end derivatives reversed from `u[nn]`), the same LU solve and right-hand side order.
- `N_FitCrvHighDerivs` → `interp_high_derivs`. The fork computes the parameters before the checks; the port keeps the pinned order, checks first. `N_FitCalcCrvParamValues` fails only for an unknown parametrization flag (`NL_CAL_ERR`) or out of memory, so for the three flags no input sees a different result or error.
- `CreateInterpolatingCurve`'s `cpOptParams` → `BSplineCurve::create_interpolating_curve_with_params`: the degree-1 two-point line only without parameters; the dimension check, then the parameter checks (`SM_ERR_INVALID_INPUT` for a count other than the point count, for one vector per point with more than two points, and for `!(u[i] > u[i-1])`, so a NaN is refused); the three end-derivative branches through `N_FitCrvHighDerivsParams` with the parameters, `N_FitCrvHighDerivs` without. Same branch order, degree raises and vector counts.
- **Probe (local, not committed), against the fork's own code.** The fork's `NL_CrvFit.cpp` at 6ca3405 compiled against the pinned NLib headers (plus the fork's `NL_CrvFit.h`, which only adds the declaration) and linked with the pinned `libSMLib.a`, and `interp_high_derivs_params`, on 12 inputs at parameters no parametrization gives: the fork's tangent-end call (degree 3, a zero first and a given second start derivative, stations 1, 1.5, ..., 3), uneven stations on [0, 1], negative stations, degree 5 with two derivatives at each end, spacing 1e-9, coordinates 1e6, a degree-1 pair at (0.5, 4), and the three input errors. Every control point, weight and knot is identical bit for bit, and the errors are `NL_INP_ERR` (14) on both sides (macOS arm64, Apple clang, `-O2 -ffp-contract=off`).
- **Without parameters nothing changes, in the C++ too.** The same probe built twice, once with the fork's `NL_CrvFit.cpp` and once with the pinned one, ran `N_FitCrvHighDerivs` on 513 inputs: the 51 `high_derivs` and 62 `high_derivs_params` fixtures of `nlib.crv_fit_p3` (with their parametrizations) and 400 random ones (1 to 14 points, degree 1 to 7, up to 6 derivatives at each end, a third of them with a zero first start derivative). The two outputs are identical byte for byte: 449 curves and 64 `NL_INP_ERR`s. So the fork's split leaves `N_FitCrvHighDerivs` bit-identical, and the port's `interp_high_derivs` is held to it by the existing `high_derivs` difftest cases against the pinned C++.

## 6ca3405: a zero-speed end at a vertex where the edge's faces touch

- `sm_IsZeroSpeedTangentEnd` → `is_zero_speed_tangent_end`, `sm_ZeroSpeedRadiusStretch` → `zero_speed_radius_stretch` (`nurbs/b_spline_curve/p3.rs`), the face loop → `faces_tangent_at` (`topology/brep/validation/assert_valid/edge.rs`), all under `Kernel::fixes.curve_check_zero_speed_tangent_end` (WR-668).
- Same: every test, tolerance and evaluation side; `SmBSplineCurve`'s `Evaluate` default `bNonZeroTangents = FALSE`; the stretch formula and its `<=`/`<`/`>` tests in the same sense; the stretch measured on the mapped trim (`*pCopyCurve`) and the end test on the curve itself (`*this`); `SM_DEG2RAD(SM_CONTINUITY_ANGLE)` as `a * (SM_PI / 180)`; `EvaluateNormal(uv, TRUE, TRUE)` from the left in both directions. Not checked: that the port's edge-use order is `GetEdgeuses`' list order. The first face's normal is the reference, so the order matters only when three or more faces have normals within the tolerance of one face but not of each other.
- **Different, fixed here:** `sPV[2].Length() <= dScaledZero` refuses the end, so a NaN second derivative does not; the port refused unless the length was `>`. Only a NaN can tell the two apart.
- **Different, kept (recorded before):** the fork takes the least radius solution not passed over (its loop is from cc4289d), the port the first one not passed over (`bspline_check_min_radius`, WR-382); see `--addendum-2026-10-31-three-partial-ports-no-result-difference.md`. The fork computes the stretches when its analysis succeeded (`bRadii`); the port computes them whenever the radius is sized, and a failed analysis leaves it no solution to pass over, so nothing differs.
- **Not compared with the fork's C++.** The fork's `SmBSplineCurve.cpp` carries many other fork changes, and its check reads the curve's edge through back-pointers, so it cannot be compiled alone against the pinned library as the probe above was. The opt-in's behaviour at a tangent vertex is held by unit tests (`nurbs::b_spline_curve::p3` `wr_668_*`, `topology::brep::validation::assert_valid::edge` `wr_668_edge_ends_read_the_faces_tangency`).

## Difftest (against the pinned C++)

The pinned C++ has neither of the fork's entry points, so the new cases compare them where the fork says they are the pinned routine. The C++ reference was built locally at 3fd5b95 (macOS arm64, Apple clang).

- `nlib.crv_fit_p3`, op `high_derivs_params` (62 fixtures): the Rust side computes the parameters of `par` itself (`param_values`) and fits at them with `interp_high_derivs_params`. The C++ side runs `N_FitCrvHighDerivs` with `par`.
- `nurbs.b_spline_curve_n_lib`, `interp_crv` with `given_params` (94 fixtures): those parameters go to `create_interpolating_curve_with_params` as `cpOptParams`, against the pinned `CreateInterpolatingCurve` with `par`.
- `nurbs.b_spline_curve_p3`, `zero_speed_end.json` (16 fixtures): `AssertValid` on free curves with a zero-speed start or end, and on three coincident start control points.

Results, against the base commit (`main` 0d83f753f) on the same machine:

| Unit | Base | Branch | New cases |
| --- | --- | --- | --- |
| `nlib.crv_fit_p3` | 2242 pass | 2304 pass | 62, all pass |
| `nurbs.b_spline_curve_n_lib` | 2335 pass, 7 `skipped(platform)` | 2429 pass, 7 `skipped(platform)` | 94, all pass |
| `nurbs.b_spline_curve_p3` | 2444 pass | 2460 pass | 16, all pass |

Every case has max_rel_err 0. Every base case is in the branch's report with the same outcome, compared value count and source. `--compare-reports --baseline` retains all base cases of the two units without platform skips; it refuses `skipped(platform)` reports, as documented.
