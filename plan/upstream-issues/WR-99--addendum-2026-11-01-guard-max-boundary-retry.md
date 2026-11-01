Updates: WR-99

#### WR-99 (addendum 2026-11-01) — Opt-in `!bFoundSolution` guard on maximum-boundary retries in `SmSurface::LocalSurfaceIntersect`

- **Severity:** Wrong result
- **Area:** Healing
- **Status in the port:** `Kernel::fixes.local_surface_intersect_guards_max_boundary_retry`, off by default. Default `Surface::local_surface_intersect` (`!k.fixes.local_surface_intersect_guards_max_boundary_retry`) preserves pinned v1.0.1 C++ parity so `surface.surface_p11` and downstream intersection difftests remain unchanged.
- **Location:** `source/SMLib/src/SmSurface.cpp:27782`, `:27804`, `:27826` (`SmSurface::LocalSurfaceIntersect`, step 4 out-of-bounds boundary curve retries; def. `:27556`) at v1.0.1 (`3fd5b95`); in Rust, `crates/solidcore/src/surface/surface/p11.rs` (`Surface::local_surface_intersect_at`).
- **What changes:**
  1. When the 4D `SmSSPIntersectENFO` Newton solve in step 2 of `SmSurface::LocalSurfaceIntersect` stops with `SM_TR_OUT_OF_BOUNDS` and `bDoBoundaryTesting` is `TRUE`, step 4 (`SmSurface.cpp:27757-27846`) tests up to four constant-parameter boundary curves sequentially: Surface 1 `U`, Surface 1 `V`, Surface 2 `U`, and Surface 2 `V`. Unlike step 1 (`:27603`, `:27632`, `:27661`), which parenthesises `!bFoundSolution && (min || max)`, the three subsequent tests in step 4 (`:27782`, `:27804`, `:27826`) omit the parentheses around the `||`: `(!bFoundSolution && SM_ARE_SAME(x, min)) || SM_ARE_SAME(x, max)`. Because `&&` binds tighter than `||`, any later parameter sitting at its domain maximum (either from the Newton stopping vector or written into `sSolutionVector` by an earlier boundary hit at a corner) re-runs `sm_IntersectSrfBndryCrv` even when `bFoundSolution` is already `TRUE`, overwriting `bFoundSolution` (and losing the found intersection whenever that later iso-curve solve fails, e.g. when the iso-curve lies in the other surface's tangent plane).
  2. When `Kernel::fixes.local_surface_intersect_guards_max_boundary_retry` is `true`, `Surface::local_surface_intersect_at` evaluates `!found_solution && (are_same(sv[i], min) || are_same(sv[i], max))` for all three subsequent step-4 boundary checks (`sv[1]` on `uv_domain.y`, `sv[2]` on `other_uv_domain.x`, and `sv[3]` on `other_uv_domain.y`), matching step 1 so the first successful boundary intersection is preserved.
  3. When no earlier step-4 boundary check finds a solution (`found_solution == false`), maximum-boundary retries still run identically in both modes.
- **Regression evidence:** Unit tests in `crates/solidcore/src/surface/surface/p11.rs`:
  - `wr99_flag_metadata_and_toggle`
  - `wr99_surface1_u_solution_not_lost_on_v_max_retry`
  - `wr99_other_surface_max_retries_do_not_clobber_earlier_boundary_hits`
  - `wr99_true_max_boundary_solution_still_found_when_prior_checks_miss`
- **Tracking:** tangency-labs/solidcore#2210.
