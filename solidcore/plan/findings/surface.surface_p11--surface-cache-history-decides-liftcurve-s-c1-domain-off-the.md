# surface.surface_p11 / surface.surface_p1: surface cache history decides `LiftCurve`'s C1 domain off the surface domain (upstream:cache-history)

Found by `offset.offset_geometry_creation` gen0266 (`Mouse.smb` edge use, `Endcap.smb`
offset edge use, exact analytic export from #555) once `intersect.surface_drop_lib_p3`
wired `SmSurface::DropCurve` (PR #463): op `ExtendTrimEdges` gave `SM_SUCCESS` in C++ and
`SM_ERR` (`Surface::lift_curve`) in Rust.

- Trace (C++ reference rebuilt with temporary `stderr` prints, not committed): both sides
  agree bit for bit on the UV curve choice (`useRad=0`), the extended domain
  `[-0.05,1.05]^2`, the extended interval `[-43.708, 49.763]`, the
  `FindUVCurveIvlInDomain` trim (the whole interval) and `LiftCurve`'s first segment start
  `(18.834, 34.638)`, tangent `(0.6726, -0.7357)`. They part at
  `GetDomainWithContinuity(SM_CT_C1, ...)`: C++ returns the natural domain, Rust the
  point `[1.05,1.05]` (empty, so `LiftCurve` errors).
- Cause: upstream `GetDomainWithContinuity` (`SmSurface.cpp:12048`) reads
  `pSurfaceCache->GetUContinuitiesArray()` without building the cache's tree; the arrays
  are filled only by `BuildTreeBase`, which `GetTree()` runs lazily. Here
  `smsurf_GetSurfaceCache` returned an `SmTrimSrfCache` (the surface has an owner) whose
  tree had not been built, so `uconts`/`vconts` were empty and the loop never ran. The
  port builds every cache with its tree (`get_surface_cache`, "built fresh"), so the knot
  continuities (`[1, 10, 1]`: discontinuous end knots) always apply. For points inside
  the domain both give the same answer; for a point outside it the end-knot
  discontinuities can collapse the domain, which upstream does only if an earlier call
  happened to build that surface's tree.
- Handling (PR #463): `lift_curve` reports that `SM_ERR` (empty C1 domain around a point
  outside the natural domain) under `surface::surface::p11::OP_LIFT_OFF_DOMAIN`; the
  `offset.offset_geometry_creation` runner defers it as `upstream:cache-history`.
  4 cases there: gen0266 (was the only failure) and 3 that agreed only because the C++
  cache had been built (their C++ also errors). The other 3 Rust `LiftCurve` errors in the
  unit (no segment lifted) are still compared.
- Follow-up: model lazy cache trees (whether a surface's cache tree was already built in
  the operation) in `surface.surface_p1` / `surface.trim_srf_cache`, so
  `domain_with_continuity` can reproduce upstream's history-dependent answer. The
  `surface.surface_p1` `dom_cont` difftests do not see this: their shim builds the cache
  first (UB-83).
