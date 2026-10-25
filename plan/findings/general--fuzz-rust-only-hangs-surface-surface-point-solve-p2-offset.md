# Fuzz "Rust-only hangs" (surface.surface_point_solve_p2, offset.offset_executive_p1; 2026-10-21)

The fuzz sweep (#321-#325) reported two Rust-only timeouts as possible port divergences.
Neither is one.

- **offset.offset_executive_p1, `/offset_distance = -1.0` or `-1e300` (`op = extend`).**
  The C++ side never ran: the FFI rejects a negative internal extension magnitude
  (HANG-16), but `--fuzz` still runs Rust after a C++ harness error, and the port looped as
  upstream would. `CreateExtendedSurface(5 * dist)` extends once, then `while (dExtDist <
  dDist/2.0) dExtDist *= 2.0` doubles a negative distance down to `-inf`, where the G1R
  extension of a plane still succeeds, so the loop never exits. Fix: the doubling loop of
  `nurbs::b_spline_surface_n_lib::p1::extend_side` is bounded by `MAX_EXTEND_DOUBLINGS`
  (`MAX_EXTEND_HALVINGS + 1`, never reached by a positive distance) and returns
  `NotConverged`. Positive distances are unchanged.
- **surface.surface_point_solve_p2, `/wrap = true` (fuzz00474 `push`, fuzz00961 `line`).**
  Not a hang: both sides finish and agree (PASS, `max_rel_err = 0`). The Rust side just
  needs more than the 20 s fuzz timeout. The STEP-wrapped surface takes the generic
  `SmSurfaceCache` path (`CalculateBoundingBox` of the directed operation, solver 10), and
  these near-degenerate nets (control points around `1e-4`) make the tree split 133,249
  times. Each `sm_ComputeBezierApproximation` corner calls `IsSingularity` ->
  `GetSingularities` (no cache upstream either), so that is 1,066,002 `GetSingularities`
  calls, 23.5 M `N_SrfDerivs` and 21.9 M `N_SrfEvalPt`. The C++ builds the same tree in
  about 3 s. Rust takes 25 s / 56 s at `opt-level = 3` and 42 s / 94 s with the
  `difftest` profile (`opt-level = 1`, debug assertions). Samply shows the same call
  structure on both sides. The gap is per-evaluation cost (surface derivatives about 10x
  slower than C++: zeroed `[[_; 18]; 18]` scratch arrays, nested `Vec` results, checked
  indexing), not more work. Nothing to fix for parity. A faster `srf_nurb::derivs` would
  help every surface unit.
