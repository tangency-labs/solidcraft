# surface.plane_p1 (`SmPlane`, part 1)

All non-deferred cases agree bit for bit with upstream (`max_rel_err = 0`). Upstream
behaviour worth knowing:

- **`SmPlane::Reparameterize` keeps the origin.** The new origin (`sOriginMax`) is only
  computed inside `SM_DEBUG_CODE`. The release build changes the analytic domain and the
  scales but keeps `m_vPosition`, so the analytic plane no longer matches its NURB unless
  the new domain starts where the old one did. The port keeps this (fixture
  `edge_reparameterize_keeps_origin`).
- **Lazy NURB through `const_cast`.** An `SmPlane` built by its constructors has no NURB.
  `const` methods that need it (`GetNaturalUVDomain`, so `CoincidenceCheck`,
  `CreateIsoParametricCurve`, `DropPointFast(SM_SO_NORMALIZE)` via `SmTol::MapTo2d`) make
  it on `this` through `GetOrCreateGwNurbPointer`. So does the `SmBSplineSurface` copy
  constructor, on the *source*. The port makes it on a copy (`Plane::base_with_nurb`), so
  the source keeps none. Only `ToggleSwapUVBit` (which leaves the NURB alone) and
  `operator==` (which compares NURBs) can see the difference. The runner generates those
  two only while both sides agree (since PR #136 also after `DropCurve`, whose fallback
  to the general `SmSurface::DropCurve` asks `GetNaturalUVDomain`: two generated cases
  with a `ToggleSwapUVBit` after an uncontained-domain drop disagreed once the general
  drop was wired), and the adversarial fixtures (`zz_adv.json`) leave
  out the sequences that observe it (`==` after `Copy`, `ToggleSwapUVBit` after
  `CreateIsoParametricCurve` or `CoincidenceCheck`). The shim copies planes without a NURB by rebuilding
  them from their analytic data, because its own copies would otherwise change plane A.
- **`SmPlane::Evaluate` with `bOnlyUpperHalf` and more u than v derivatives** writes
  outside its array (the `ULONG` index `lHighestVDeriv - k` wraps). The port returns
  `InvalidInput`, and the ffi wrapper rejects that input.
- **Deferred paths:** `CoincidenceCheck` with a surface that is not an `SmPlane` needs
  `SmSurface::IsPlanar` (`surface.surface_p7`), then `CreateNaturalUVTrimCurves`
  (`surface.surface_p6`) and `DropPoint` (`surface.surface_p5`). `DropCurve` with a curve
  whose control points must be clamped to the domain, or with a curve that is neither a
  line nor a B-spline kind, needs the general `SmSurface::DropCurve`
  (`intersect.surface_drop_lib_p3`). `GlobalLineIntersect` of a line lying in the plane
  that meets the domain at a single line parameter needs the general
  `SmSurface::GlobalLineIntersect` through the surface cache (`surface.surface_p8`).
- **`SmSurfaceCache` and planes.** Upstream the `SM_CAST_PTR(SmBSplineSurface, plane)`
  in the cache succeeds and the virtual calls reach the plane. The port's cast gives the
  plane's B-spline part, and the two virtual paths return `SolidError::Unported`
  (marker `UNPORTED(surface.plane_p1)`): `SmPlane::CreateIsoParametricCurve` (ported, but
  it makes an `SmLine` and the cache stores its iso curves as `BSplineCurve`, which
  would drop the line's overrides; wiring needs `Curve` storage in the cache) and the
  inherited `SmBSplineSurface::CalculateBoundingBox` (which dispatches back to the plane
  through `IsKindOf` and `SmPlane::EvaluateNormal`; the plane does not re-dispatch it yet).
- **Undefined behaviour the harness keeps away from upstream** (found in review):
  `SmPlane(Origin, Normal)` with `|n|^2 < SM_EFF_ZERO_SQ`: `MakeUnitOrthoVectors` fails,
  the status is ignored and the axes stay uninitialised (the ffi encoder rejects a normal
  with `|n|^2 < 1e-24`). `Reverse` and `RebuildSTEPFromNURBParameters` on a plane whose
  NURB cannot be made (degenerate or inverted domain) pass a `NULL` NURB to
  `N_SrfReverse` / `SmBSplineSurface::EvaluatePoint` and segfault; the shim checks
  `GetOrCreateGwNurbPointer()` first and reports `SM_ERR`, which is what the port
  returns. `CreateExtendedSurface` with a zero distance loops forever (the halving never
  falls below `dDist / 100`); the ffi encoder accepts only positive distances.
- **`CreateExtendedSurface` on a plane** runs the `SmBSplineSurface` body with the
  `SmPlane` overrides of `IsSingularity` (none) and `IsClosed` (never), so a plane
  extends where a plain B-spline would still be waiting on `surface.surface_p5` /
  `surface.surface_p7`.

- **Port deviation: `natural_uv_domain` does not cache the NURB (found by difftest
  `--ssi-corpus`, 2026-10-14).** Upstream `SmBSplineSurface::GetNaturalUVDomain` makes the
  plane's NURB in place (`GetOrCreateGwNurbPointer` through a const cast) and keeps it.
  `Plane::natural_uv_domain`, like the port's other read-only queries, makes it on a copy
  and caches nothing. `IntersectWithPlane`'s parallel branch (`SmPlane.cpp:2183`) returns
  "no intersection, done" when either plane has no NURB, and otherwise tests for
  coincidence and sends coincident planes to the general intersector. So after the same
  call sequence, coincident planes (corpus cases `c6_planes_identical`,
  `c6_planes_overlap`) gave no curves in Rust and four `COINCIDENT` boundary curves in C++.
  The deviation is noted in `surface::plane::p1` (and `p2`); the follow-up to model the
  cache is in this unit's `notes` in `plan/modules.toml`.
