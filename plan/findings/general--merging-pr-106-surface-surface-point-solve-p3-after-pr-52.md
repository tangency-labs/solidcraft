# Merging PR #106 (surface.surface_point_solve_p3) after PR #52 (2026-10-15)

- **Newly compared cases exposed two port bugs.** With `GetSingularities`/`SmFindPSNormalENFO`
  (PR #52) and the point solves (PR #106) both wired, cases that used to defer on one or the
  other now run end to end:
  - `SmSurfPtGlobalSolver::Solve` (`surface.surface_point_solve_p2`) set
    `m_apObjects[0] = m_pSrf` on every `LocalPointSolve` result. Upstream only gets that from
    `LocalPointSolve_1`; the `LocalPointSolve_0` path (MAXIMIZE) leaves the default solution's
    objects untouched. 52 `surface.surface_point_solve_p3` and 2 `surface_point_solve_p2`
    MAXIMIZE cases on planes disagreed on the object slot; the port now sets it only when the
    local solve reports an object (`m_lNumObjects == 1`).
  - `CoverCoincidentSurface` (`surface.surface_p3`) propagated the status of
    `AdjustSTEPUVDomain`; upstream ignores it (`SmSurface.cpp:8073,8266`). Generated case
    gen1979 (a rational B-spline that covers itself) failed with `SM_ERR` from
    `expand_boundary_span` in Rust, while C++ succeeded. The status is now ignored.
- `topology.loopuse`'s `DropPoint` stub (`UNPORTED(surface.surface_p5)`) is wired: with no
  guess and no solution array, `DropPoint` writes `rResultUV` only when it ends with
  `rbSuccess` set (`SE(...)` ignores the status, `SmLoop.cpp:441-442`).
- **`intersect.surface_drop_curve` planes now carry their NURB in the difftest.** With the
  point solves wired, 15 generated plane cases that used to defer on `LocalPointSolve_1`
  compared ULP-level differences (a converged UV one ulp apart, so `m_dSurfNormLineDist`
  near zero disagreed at relative error 1). Cause: C++ `SmPlane`'s const queries make and keep
  the NURB (`GetOrCreateGwNurbPointer` through a `const_cast`), and an `SmPlane` that has it
  takes a different number of `LocalPointSolve_1` steps than a fresh one (same inputs: fresh
  `u = 0.0023719945901309069` after 1 step, after `ComputePointValues` `0.0023719945901309065`
  after 0). The Rust plane never keeps a NURB made by a read-only query (deviation documented in
  `surface::plane::p1`). The shim now calls `GetOrCreateGwNurbPointer()` and the runner
  `Plane::make_nurb()` right after construction, so both sides start from the same state:
  527 pass, 20 deferred, 0 failed.
- **`intersect.surface_drop_lib_p2` (#112) had the same plane NURB state issue.** Its four
  stubs were wired here (`IsSingularity` both overloads and `LocalPointSolve` with `SER` as
  `?` and the caller's kernel, which `sm_AddToCurve` now takes in place of its unused
  `crContext`; `EvaluateGeometric` with its status ignored, curvatures `0.0` on failure).
  Eight generated plane `GetNextPoint` cases then disagreed at ULP level in the same way
  (`GetNextPoint`'s start evaluation makes the C++ plane's NURB). Its shim and runner now
  make the NURB up front too: 1304 pass, 227 deferred (surface.offset_surface), 0 failed.
