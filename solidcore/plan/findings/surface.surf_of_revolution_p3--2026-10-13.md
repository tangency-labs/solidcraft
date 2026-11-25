# surface.surf_of_revolution_p3 (2026-10-13)

- **`SurfaceKind::SurfOfRevolution`.** The surface of revolution joined the `SmSurface`
  dispatch (`surface::surface::p2`) to wire `SmSurfOfRevolution::ReadFromDB` into
  `SmSurface::ReadFromDB`. `Surface::reverse`, `swap_uv`, `reparameterize`, `transform`,
  `drop_point_fast` and `convert_uv_from_*` now take the `Kernel` (the surface's
  overrides, and the virtual `RebuildSTEPFromNURBParameters` that the inherited
  `SmBSplineSurface::Reverse` / `SwapUV` reach, need it); `intersect_with_plane` takes a
  `Plane`. `STEPInversion`, `AdjustSTEPUVDomain` and `TrimWithDomain` of the kind are
  stubbed to `surface.surf_of_revolution_p2`.
- **Surface cache iso curves.** `SmSurfaceCache` reaches the virtual
  `CreateIsoParametricCurve`, which makes an `SmCircle` in the sweep direction of a
  surface of revolution; the cache stores its iso curves as `BSplineCurve`, so that call
  stays an `UNPORTED(surface.surface_cache_p1)` stub (as the plane's `SmLine` iso
  curves do, retagged from `surface.plane_p1` in the review fixes) until the cache stores
  `Curve`s. Both are tagged with the unit that owns that storage
  (`SmSurfaceCache::GetIsoBoundaryCurves`), not with the already-partial units that
  ported the overrides, so whoever changes the storage finds them by grep.
- **Polar converter records.** A surface of revolution's `SmPolarConversion` owns its
  sweep circle, so `WriteToDB` stores it (`bHasCurve`); it is read by
  `PolarConversion::read_from_db_with_curve` (curve.ellipse_p1).
- **Cones and spheres.** `IntersectWithCone` / `IntersectWithSphere` take the other
  surface through the `Revolved` trait (its `SmSurfOfRevolution` part and its virtual
  `ConvertDomainFromNURBSToSTEP`); `SmCone` / `SmSphere` should implement it when ported,
  which turns the 44 deferred cases into compared ones.
- **Inexact polar conversions.** `ConvertUVFromSTEPToNURBS` without polar conversion and
  `DropPointFast` with an inexact angle need `SmSurface::LocalPointSolve`
  (`surface.surface_p5`). They are unreachable in both C++ and Rust, not just
  unreached: `SmPolarConversion::ConvertToNURBSParameter`
  (`PolarConversion::convert_to_nurbs_parameter`) reports an exact conversion whenever
  it succeeds, so the inexact iso-curve inversion never runs.
- **Review fixes (PR #29).** The FFI wrappers rejected valid inputs: zero-width drop,
  intersection and update domains, and zero drop / intersection tolerances. Upstream
  handles both without undefined behaviour (198 such cases: C++ does not crash, Rust is
  bit-identical), so those domains now need only be ordered and tolerances only `>= 0`;
  `Reparameterize` keeps strictly positive extents (a zero-width reparameterisation
  divides by zero in NLib). Fixtures `review_adversarial_zero_width.json` (144) and
  `review_adversarial_zero_tol.json` (54). `review_adversarial.json` (3497 cases) is a
  subset of the reviewer's 6125-case adversarial set (every non-drop case, every third
  drop case; all 6125 were bit-identical or deferred): torus / cylinder / cone / sphere /
  partial-sweep / Bezier generators, swapped and at 1e-4 and 1e5 scale, drops on the
  axis and seams, coaxial / tangent / coincident / disjoint / wrapping intersections,
  non-uniform and negative-scale transforms, rebuilds, reverse / swap / update /
  reparameterize. Its deferred cases are the `tol = None` path (`surface.surface_p1`).
- **Not reachable today.** (1) `Surface::reverse` / `swap_uv` of a surface of revolution
  use the stored NURB; upstream calls the virtual `GetOrCreateGwNurbPointer`, which would
  run `SmSurfOfRevolution::MakeNurb` for a surface without one (none exists: every
  constructed surface has its NURB). (2) `convert_uv_from_*_or_written` (callers that
  ignore the status, e.g. `UpdateAnalyticalDomain`) model `rSTEPUV` as written only after
  `Point2dMapToDomain` / `ConvertToPolarParameter` succeed; if either C++ call wrote its
  out-parameter partway before failing, such a caller would see a different value. No
  input found reaches a failure there.
- **Review fixes, round 2 (PR #29).** The FFI frame guard rejected scale vectors with any
  component below `1e-6` in magnitude, for both `Transform` and the rebuild edit. Upstream
  handles all of them without undefined behaviour: `Transform` has its own
  `cpOptScale->x < SM_EFF_ZERO` -> `SM_ERR` branch, and `SmBSplineSurface::Transform`
  accepts zero and negative components. Because of the guard, the boundary of that branch
  (`0 < s < 1e-12` against `1e-12 <= s < 1e-6`) was never compared. The guard now only
  requires finite values. `review2_scale.json` has 1092 cases: scales 1e-7, 1e-9, 1e-11,
  1e-13, -1e-7, 2e-7, zero, mixed zero and tiny components and +-1e-300, on every test
  surface at model scales 1, 1e-4 and 1e5. All are bit-identical and none crash.
  `review2_extra_adversarial.json` (4654 cases) is a subset of the reviewer's 11010-case
  second adversarial set: every case other than drops and `ToNurbs` conversions, plus every
  third drop and every third conversion. All 11010 were bit-identical or deferred. It
  covers coincident generators with an apex end, flipped axes, wrapped and partial sweeps,
  swapped surfaces, a tangent torus and cylinder, drops at seams, poles and generator
  seams with pointing vectors, out-of-range `ToNurbs` angles, tiny and huge
  reparameterisations, and near-identity transforms and rebuilds. A missing bounding box
  in `IntersectWithSurfOfRevolution` now reports the intersection's operation path.
- **`SmSurface` dispatch after `surface.surface_p7`.** `EvaluateNormal`, `IsClosed` and
  `IsPlanar` are not overridden by `SmSurfOfRevolution`, so a surface of revolution
  runs the base bodies like a B-spline surface. `IsPlanar` reaches
  `SmBSplineSurface::CalculateBoundingBox` on the surface's NURB.
  Likewise `EvaluateGeometric` / `EvaluateNormalSection` in `intersect.surface_silhouette`
  (stubbed to `surface.surface_p3`, as for a B-spline surface), and `BuildTreeBase` in
  `surface.surface_cache_p3` decomposes the surface's NURB (`GetOrCreateGwNurbPointer`:
  `MakeNurb` on a copy when it has none).
- **Review fixes, round 3 (PR #29).** The runner reused p1's `status()`, which reports
  every Rust error other than `Upstream` as `-1`. `UpdateAnalyticalDomain` on a swapped
  surface with a B-spline generator and a zero-width U domain (`[0.5, 0, 0.5, 1]`)
  reaches `SmBSplineCurve::Trim` through `AdjustSTEPInterval`, which returns
  `SM_ERR_INVALID_INPUT` (1007, "trim points too close together"); Rust returns
  `SolidError::InvalidInput` (faithful), but the case was reported as a failure. The p3
  runner now maps `InvalidInput` to 1007, as the `nurbs.b_spline_curve_p2` and
  `intersect.surface_tracer` runners do. Fixtures: `review3_update_bspline_generator.json`
  (240 update cases), `review3_adversarial.json` (5829: coaxial cone / cylinder / sphere /
  torus / disk / axis-line intersections at tolerances None, 0, 1e-15, 1e-9, 1e-3 and 10,
  off-axis and tilt sweeps up to 1e-5, 1e8 scale, transform scales at the `SM_EFF_ZERO`
  boundary, seam / axis / pole drops) and `review3b_adversarial.json` (10240: B-spline and
  arc generators on five sweeps, swapped and unswapped, every op including `read_db`
  ascii / binary v30 / v33; the 240 cases duplicated in the update fixture were dropped).
  All are bit-identical or deferred.
- **Self-tagged iso-curve stubs.** The `UNPORTED(surface.surface_cache_p1)` stubs for
  `SmPlane` / `SmSurfOfRevolution::CreateIsoParametricCurve` name a unit that is already
  `partial`, against AGENTS 4a. This is deliberate (see "Surface cache iso curves"
  above): the cache stores iso curves as `BSplineCurve`, so the `SmLine` / `SmCircle`
  results cannot be stored until it stores `Curve`s. Whoever changes that storage must
  wire both stubs.
- **Not reachable today (3).** `SwapUV` of a surface of revolution toggles the swap bit
  after the inherited base swap; upstream toggles it between `GetCanonical` and
  `SetCanonical`. These differ only if `SetCanonical` fails, which is not modelled.
- **Tiny-scale coverage (follow-up for `surface.surf_of_revolution_p1`).** p1's FFI guard
  rejects generator lines shorter than `1e-6` and circles with radius `<= 1e-6`
  (absolute thresholds), so no surface below about `1e-6` model scale can be compared
  (the reviewer's `1e-8` scale cases were rejected). If upstream handles such inputs
  without undefined behaviour, the thresholds should become relative.
