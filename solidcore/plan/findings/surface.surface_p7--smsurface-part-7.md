# surface.surface_p7 (`SmSurface`, part 7)

- **Co-ported callees.** `SmSurface::Evaluate1stDerivatives`, `Evaluate2ndDerivatives` and
  `EvaluateDirectionalDerivs` (unit `surface.surface_p1`) are non-virtual one-call wrappers of
  the virtual `Evaluate` that nearly every function here calls first; stubbing them would
  have deferred the whole unit. They live in `surface::surface::p7`, generic over
  `SurfaceEvaluate`, and are recorded as `CO-PORTED` in `surface.surface_p1`'s notes. Their
  stubs elsewhere (`b_spline_surface::p2`, `b_spline_surface_n_lib::p2`, `trim_srf_cache`,
  `crv_on_surf`) were wired too.
- **`SurfaceEvaluate::evaluate_normal`.** Code generic over the surface type
  (`TangentField<S>`) needs `EvaluateNormal`'s virtual dispatch; the trait method defaults to
  the `SmSurface` body (what `SmBSplineSurface` runs) and `Surface` dispatches to the plane and
  curve-bounded overrides. `surface_cache::p1`'s stub assumed no ported kind overrides
  `EvaluateNormal`; planes and curve-bounded surfaces do, so it now dispatches.
- **`IsClosed` and a failed `smgu_FrenetFromDerivatives`.** Upstream writes the position,
  the tangent and a zero curvature vector before returning `SM_ERR` for a zero tangent;
  `base_is_closed` now leaves exactly that (found by comparing with the copy co-ported in
  `intersect.surface_intersector_p1`, PR #33, which this unit's functions replace).
- **Mislabelled stub.** `b_spline_surface::p1::is_closed` was marked
  `UNPORTED(surface.surface_p5)` for `SmSurface::IsClosed`, which is this unit's; wired.
  `IsPlanar` of a B-spline defers on `SmSurface::GetFace` (`surface.surface_p1`), reached
  through `b_spline_surface::p1::owner_zone_tolerance`.
- **Upstream quirks kept.** `GetFlatCorners` masks its return with `&=` while filling the
  points; `SmSSGlobalSolver::BranchMayContainAnswers` returns `SM_SUCCESS` (`FALSE`) for a
  node outside its face and `NER`'s status (`TRUE`) for a missing patch; `sm_GoodDropResult`
  swaps the guess/result distance names; `FindDegenParamForDirection` compares a sample
  fraction with the parameter `dS`, and on Newton failure returns `SM_ERR` with `rUV` moved
  (the port returns `DegenParam { converged: false }`).
- **Not difftested.** `SmSPGlobalSolver` / `SmSSGlobalSolver::BranchMayContainAnswers` need
  surface cache trees with Bezier patch data (`surface.surface_cache_p3`); their non-normalize
  path is the green base `SmGlobalSolver::BranchMayContainAnswers`. The shim's
  `sm_LoadGuesses` copy takes the owner surface directly (two lines marked `ADAPTED`):
  reaching it through a tree node needs a real `SmSurfaceCache`.
- **Slow upstream solves.** `GlobalSurfaceSolve` (minimize) between two random surfaces
  that intersect runs for minutes in the C++ (a continuum of zero-distance answers along
  the intersection curve), and the global solves on 1e-4-scale surfaces with a 1e-6
  tolerance take tens of seconds. These ops always defer on the Rust side, so the
  generator keeps them at unit scale, drops `SM_SO_NORMALIZE` for `GlobalCurveSolve`, and
  covers `GlobalSurfaceSolve` with fixtures on separated surfaces only.
- **Curve-bounded surfaces** (without boundary curves) are generated too, so the
  `EvaluateNormal` / `IsClosed` dispatch of that kind is compared.
- **Generator note.** Ordinary random nets almost never reach `EvaluateNormal`'s fall-backs
  (zero-corrected first derivatives keep the cross product alive); the runner adds poles,
  exactly folded nets and pinched corners and samples boundaries, and the fixtures hit each
  branch (pole: second-derivative substitution; fold: zero result; pinch: step-off
  extrapolation).
