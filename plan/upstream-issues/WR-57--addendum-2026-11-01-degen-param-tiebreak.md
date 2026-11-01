Updates: WR-57

#### WR-57 (addendum 2026-11-01) — Opt-in mapped singular parameter comparison in `SmSurface::FindDegenParamForDirection` (#2209)

- **Location:** `source/SMLib/src/SmSurface.cpp:6948`, `SmSurface::FindDegenParamForDirection` (def. `:6757`) at pinned v1.0.1 (`3fd5b95`).
- **What happens (default, pinned):** `dS` (`ds`, set from `rUV.x` for `SM_SP_U` or `rUV.y` for `SM_SP_V` at `:6785`-`:6798`) is the caller's singular parameter in the surface's natural singular interval `sSIvl` (`s_ivl`). Inside the 10-guess loop (`ii = 0..9`), `dThisParam = ii / (lNumGuesses - 1)` (`this_param`) and `dNearBestParam` (`near_best_param`, initialized to `0.5` at `:6840`) are normalized `[0, 1]` fractions that are later mapped into `sSIvl` via `sSIvl.Evaluate(dNearBestParam)` as the starting guess for `SmLocalSolve1d::SolveIt` (`:6996`) and on non-converged fallback (`:7014`-`:7015`). In the near-equal starting-guess tie-break (`|dThisFVal - dNearBestFVal| <= dFTol`, `dFTol = 2.0 / 50.0` at `:6946`-`:6949`), pinned v1.0.1 tests `smos_Fabs(dThisParam - dS) < smos_Fabs(dNearBestParam - dS)` — comparing `[0, 1]` fractions directly against the unnormalized parameter `dS`. On any degenerate boundary whose singular interval `sSIvl` is not `[0, 1]` (e.g. `[0, 360]`, `[0, 2 * PI]`, `[10, 20]`, or `[-20, -10]`), `dS > 1` biases the tie-break toward `dThisParam = 1.0` (`ii = 9`) and `dS < 0` biases it toward `dThisParam = 0.0` (`ii = 0`), selecting the wrong Newton-Raphson basin when two samples are nearly equally aligned with `dir`.
- **Opt-in:** `Kernel::fixes.degen_param_tiebreak_uses_actual_param`, default `false` (`#2209`):
  - When `false` (default), `Surface::find_degen_param_for_direction` (`crates/solidcore/src/surface/surface/p7.rs`) compares `(this_param - ds).abs() < (near_best_param - ds).abs()`, preserving pinned v1.0.1 C++ parity.
  - When `true`, it maps both candidate fractions into `s_ivl` (`s_ivl.evaluate(this_param)` and `s_ivl.evaluate(near_best_param)`) before comparing distances to `ds`. Mapping through `s_ivl` compares the actual Newton-Raphson starting guesses (`sSIvl.Evaluate(...)`) against `ds` and is algebraically bit-identical to `(this_param - ds).abs() < (near_best_param - ds).abs()` whenever `s_ivl` is `[0.0, 1.0]` (unlike `sample_domain`, which contracts `[0, 1]` by `2.5%` to `[0.025, 0.975]`).
- **Caller trace:** Every caller of `Surface::find_degen_param_for_direction` across `crates/solidcore` already passes `k: &Kernel` (`intersect/curve_class/p4.rs:1984,1993`, `intersect/surface_drop_lib/p2.rs:334`, `intersect/surface_drop_lib/p3.rs:904`, `surface/surface/p8.rs:2926`, and `topology/vertexuse.rs:1452,1462`), so enabling `k.fixes.degen_param_tiebreak_uses_actual_param` takes effect through all callers without signature changes.
- **Tests:** `crates/solidcore/src/surface/surface/p7.rs`:
  - `wr57_flag_default_off_and_lookup_by_name`
  - `wr57_sphere_pole_seam_tiebreak_on_non_unit_and_unit_domains`
  - `wr57_nurbs_degenerate_patch_shifted_domains_and_both_singular_directions`
- **Difftest:** no change on the default path.

`addendum_options: Kernel::fixes.degen_param_tiebreak_uses_actual_param`
