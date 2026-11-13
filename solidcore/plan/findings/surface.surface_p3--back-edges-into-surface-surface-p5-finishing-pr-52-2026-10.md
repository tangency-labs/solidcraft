# surface.surface_p3 back-edges into surface.surface_p5 (finishing PR #52, 2026-10-15)

- **Wired in `surface::surface::p5`:** `GetSingularities` in `IsSingularity` (status-free
  upstream, so `ignored()` with its initial outputs `SM_SS_NONE` and uninitialised poles),
  the `SmFindPSNormalENFO` Newton solve of `LocalPointSolve_0` (`SER(sLS.SolveIt(...))`
  is `?`), the UV-guess `LocalSurfaceSurfaceIntersect` of the image-space overload (status
  ignored upstream: no answer), `CoverCoincidentSurface` in `CoverOtherCoincidentSurface`'s
  `Cover` lambda (a failing call is no cover and leaves `rdMaxDist = SM_BIG_DOUBLE`, its
  first statement), and `EvaluateNormalSection(uv, TRUE, TRUE, ..)` in `sm_FilterSolution`
  (status ignored; curvature `0.0` keeps the line branches upstream takes when the section
  fails).
- **Kernel threading.** `IsSingularity`, `EvaluateNormalAtSingularity` and
  `CoverOtherCoincidentSurface` take `&Kernel` (`GetSingularities` needs the approximation
  tolerance). Callers without a kernel use `Kernel::default()`: the `IdenticalSolutions`
  overrides in p5, `LocalCurveIntersect` (p6), `sm_ComputeBezierApproximation`
  (surface_cache p1) and `CreateExtendedSurface` (b_spline_surface_n_lib p1). The
  `SmSSGlobalSolver` override uses the kernel it holds.
- **`CoverOtherCoincidentSurface` changes the covering surface.** It now takes both
  surfaces `&mut`. `topology::edge::is_topological` therefore takes `&mut Brep` and a
  `&Kernel`; it moves the two face surfaces out of their faces for the call (keeping their
  owners) and puts them back, so an expansion stays on the face, as upstream.
- **`surface.surface_p6` shim runs `sm_SpLocalSolve`.** It used to stop with `-2` where
  `sm_SCLocalSolve` calls it, and the Rust side deferred there. With `GetSingularities` and
  the `SmFindPSNormalENFO` solve wired, three generated cases (gen0073, gen0882, gen1429)
  ran the whole Rust point solve, so the shim now holds the same verbatim
  `sm_SpLocalSolve`/`sm_LoadGuesses`/`sm_PassSPConvergenceTest` copies as
  `surface_surface_p5.cpp`. All three agree. (Merging PR #106 replaced the copies with
  the shared `harness/cpp/shim/surface_sp_local_solve.hpp` that both shims include.)
- **`surface.surface_p2` extension cases with a failed first attempt are not generated.**
  Upstream retries an in-place `N_SrfExtendByDist` on half-written arrays (UB-107 in
  `plan/upstream-issues.md`); generated case gen0016 returned `SM_ERR` in C++ in a fresh
  process and `SM_SUCCESS` after other extensions. The generator emulates the side
  sequence with the Rust port and skips a case when any side fails at the full distance.
