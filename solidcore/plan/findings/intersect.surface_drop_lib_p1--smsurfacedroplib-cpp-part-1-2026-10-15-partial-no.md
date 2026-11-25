# intersect.surface_drop_lib_p1 (`SmSurfaceDropLib.cpp` part 1, 2026-10-15; partial, no disagreements)

- **Ignored evaluation statuses.** `SmDropPt::IsMovingAlongBoundary` ignores the statuses
  of its `Evaluate1stDerivatives` / `EvaluateNormal` calls, and `TestSpanAccuracy` those of
  the two `EvaluatePoint`s of its domain clamp check. This is not undefined behaviour:
  `SmVector3d` / `SmPoint3d` members default to `SM_UNDEF_DOUBLE`, so upstream goes on with
  those defaults or whatever the evaluator wrote before it failed. The port does not
  model that path (a failed evaluation is an error), and the shim reports such inputs as
  `REJECTED` on both sides. The guard has never fired: 0 of the generated moving/span
  cases and 0 of the review's ~3600 pole, cone-apex and collapsed-edge cases, so no valid
  input is hidden today. If an evaluator failure is ever reached, port it with
  `SM_UNDEF_DOUBLE` vectors instead of rejecting.
- **`sm_FixBoundaryCurves`** on `3k` points (`k >= 2`) reads one point past the array
  (`SM_ASSERT` compiled out): undefined, rejected by the shim, `InvalidInput` in Rust.
- **Coverage limit.** `DropIsoCurve` grows its `dKnots[100]` `SmTArray` buffer past 100
  knots; the FFI caps B-spline curves at 64 control points, so that growth is not
  difftested (the Rust side has no limit).
- **Review fixes (PR #98).** Clippy with `--features difftest/cpp` failed in the runner
  (collapsible `if`, unchecked indexing in two generators); fixed without changing the rng
  draw order, so the generated cases are byte-identical. 328 review fixtures added
  (`fixtures/intersect.surface_drop_lib_p1/review_poles.json`, subsampled from 3596; none
  undefined in C++): all pass bit-identical or defer on `surface.surface_p3`.
- **Review fixes (PR #140): UV curve kind.** `DropAndTrimCurve` used to turn each traced
  UV curve into a plain `BSplineCurve`. Upstream `SM_CAST_PTR(SmBSplineCurve, p)` is only
  an `IsKindOf` check on the same object, so `rUVCurves` keeps the `SmLine` that
  `SmPlane::DropCurve` makes for a line on a plane, and callers use the `SmLine` overrides
  (`EvaluatePoint` is `P + t*scale*V`, not de Boor). `DropAndTrim::uv_curves` is now
  `Vec<Curve>`; the runner writes a line's data through `base_with_nurb`, as the shim's
  `GetOrCreateGwNurbPointer` does. This was latent: every plane `DropAndTrimCurve` case
  still defers on `surface.surface_cache_p1` (`SmPlane::CreateIsoParametricCurve`), so
  plane cases will start comparing (and would have diverged) once that stub is wired.
- **Coverage (PR #140 review).** Before the review fixtures, the only compared trim
  cases were iso lines on bilinear and cylinder patches: all plane lines defer on
  `surface.surface_cache_p1`, and B-spline cases with `on_surface = false` defer on
  `intersect.curve_class_p4`. 632 review fixtures (`review_trim_adversarial.json`) add 360
  passing cases. A zero-length line (`a == b`) is still rejected by the FFI
  ("zero-length segment"), so that degenerate case is not difftested.
- **Surface cache.** `drop_curve_trace` builds a fresh surface cache per call, where
  upstream checks out the context-cached `smsurf_GetSurfaceCache`; state left in that
  cache between upstream calls is not modelled (documented in the module Deviations).
  Callers that hold the surface's cache now pass it (`Surface::drop_and_trim_curve_in`):
  the coincident branches of `GlobalCurveIntersect` / `GlobalLineIntersect`
  (`surface.surface_p8`) and the boundary curve drops of `FindBoundaryStartPoints`
  (`intersect.surface_intersector_p3`, on a handle-mapped copy as its
  `GlobalCurveIntersect` already does).
- **ComputeSingularityPoint dispatch (merge with main, 2026-10-16).** Wiring the trace
  let 6 generated `intersect.surface_intersector_p5` cases (gen0127, gen0428, gen0439,
  gen0512, gen1092, gen1264: bilinear patches sharing an edge) reach past
  `DropAndTrimCurve`, and they failed: C++ returned two more curves (`rec.len` 437 vs
  297), the touching points at the shared edge's ends. Cause: `FindBoundaryStartPoints`
  calls `ComputePointValues`, which calls the virtual `ComputeSingularityPoint`; on an
  `SmAdvSurfaceIntersector` that is the override, but the port always ran the basic
  `SM_ERR` body, so the start points were dropped (the "DISPATCH GAP" the p5 header
  noted as unreachable). Fix: the override's body reads only base members, so it moved to
  `SurfaceIntersector::adv_compute_singularity_point`, and `ComputePointValues` calls it
  when the base's `advanced` flag (set by `AdvSurfaceIntersector::new`) is on. All 6
  cases now pass bit-identical; p2/p3/p4/adv/p5 have 0 failures.
- **Second review (PR #140): iso curves of the surface cache wired.** The two
  `UNPORTED(surface.surface_cache_p1)` stubs (`SmPlane::CreateIsoParametricCurve`,
  `SmSurfOfRevolution::CreateIsoParametricCurve`) were tagged with a green unit while the
  callees were ported (`surface.plane_p1`, `surface.surf_of_revolution_p1`). They are now
  wired the way PR #270 does it (its `surface_cache/p1.rs`, the `Curve` storage of the iso
  curves, `copy_as_bspline` for upstream's `new SmBSplineCurve(*p)` copies, and the
  `&Curve` boundary signatures of the tracer, silhouette and surface intersector p2/p3,
  taken over unchanged so the two PRs merge cleanly), **including the plane branch of
  `GetIsoBoundaryCurves`** that #270 keeps stubbed because of
  `intersect.surface_intersector_p5` `gen0072`. On this branch `gen0072` passes
  bit-identical (157 values): its missing touching-point curves are the
  `ComputeSingularityPoint` dispatch gap fixed above. `intersect.surface_intersector_p5`
  goes from 1009 to 1471 passes with 0 failures (its runner now writes the B-spline part
  of a `Line` / `Ellipse` output curve, as #270's does, after its type code).
  The plane and revolution `DropAndTrimCurve` cases now trace past the iso curves and
  stop at a real gap: `SmCrvOnSurf` over a non-B-spline surface
  (`surface_drop_curve.rs`, the `curve.crv_on_surf` port holds only a `BSplineSurface`;
  an `SmPlane`'s virtual evaluators differ from its B-spline part, so wrapping the
  plane's B-spline would not be faithful). These defer on `curve.crv_on_surf`.
- **Second review (PR #140): curve_class_p2 stubs not wired.** The `ReSet` and
  `ConnectIntervals` stubs in `intersect/curve_class/p4.rs` are deliberate: wiring them
  breaks `topology.brep_p2`'s `merge_plane10_line` and 4 squeeze cases of
  `intersect.curve_class_p4` (see that unit's section). The deferrals behind them are
  counted as such in the unit's notes.
- **Second review (PR #140): coverage.** The generator is back to main's buckets
  (`88..=98` iso, `99` plane line), so every generated case keeps its inputs; the wider
  trim generator runs as 400 fixed-seed cases `trim0000..trim0399` appended after the
  1500. The `Trim` record now carries each UV curve's `GetType()` (line vs B-spline is
  compared), and the shim writes the sentinel `NULL_CURVE` for a `NULL` entry instead of
  dereferencing it.
