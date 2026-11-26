# surface.offset_surface (`SmOffsetSurface`, PR #62, 2026-10-14)

- **Status (2026-10-17, after merging main `ccb8ab93`; C++ in process): green.** Default
  difftest 3077 cases (77 fixtures + 3000 generated), 3077 passed, 0 failed, 0 deferred;
  100,000-seed run 100,076 cases, 100,076 passed, no crash or hang; compared values
  bit-identical. `intersect.surface_drop_lib_p2` (1531/1531) and
  `nurbs.b_spline_surface_p1` (2120/2120) reach 0 deferred and are promoted to green.
  (Earlier: 3066 cases, 2853 passed, 213 deferred on `surface.surface_point_solve_p3`.)
- **Found by the in-process 30,000 / 100,000-seed sweeps (second review).** Three issues
  beyond the default 3000 seeds, all fixed:
  - *Nested offset extension (port).* `SmOffsetSurface::STEPInversion` calls the base's
    `STEPInversion`; for an offset base that allows extension, the base's
    `EvaluatePoint` stores an extension in the base offset (a `const` cast), which then
    widens the outer offset's natural domain and what `CreateOffsetSurface` copies. The
    port ran the base's inversion through `&self` and dropped it (2 of 30,000 cases:
    natural domain `u` max `192.8` vs `1`). `step_inversion_extending` now runs an offset
    base's storing inversion. Fixture `nested_step_inversion_extends_inner`.
  - *`IsPlanar` reaching UB-173.* After such an inversion, `IsPlanar` (the `SmSurface`
    body) boxes the widened natural domain through the virtual and reaches the inner
    offset's `SmSurface` box body, which recursed until the stack overflowed (1 of
    100,000; in process this aborted the difftest). The FFI's static rule only knew the
    outer offset's extension, so the shim now runs its chain check for `IsPlanar` too
    (over `GetNaturalUVDomain()`), and the generator preflights `IsPlanar` steps. FFI
    test `is_planar_after_an_inner_extension_is_refused`.
  - *Uninitialised `dMaxGap3d` (upstream UB, `curve.curve_p1` entry).* A `BaseOffset`
    of `-3e149` on a unit-size surface of revolution offsets its generator into a
    degenerate curve; `SmBSplineCurve::CreateSimpleOffset` then copies an unwritten
    `dMaxGap3d` and C++ succeeded on garbage where the port returns `InvalidInput`
    (1 of 100,000). `BaseOffset` distances are now generated relative to the base's
    size, as in `surface.surf_of_revolution_p2`, instead of the magnitude scale.
- **Guards loosened to what C++ needs (first review, nice-to-have):** each static FFI
  limit was probed against unchanged C++ (sweeps of coordinates to `1e300`, scales
  `0`/`-1e-6`/`1e-100` and orders to 64 left no hang or crash). Removed the `1e6`
  coordinate cap, the `1e-3` transform-scale floor and the order cap of 6 (now 64, a bound
  on the caller's array: upstream refuses offset orders above 3 before writing). UB-171
  (writes past the array) moved from a static order table into the shim, checked on the
  actual compound distance (a zero offset delegates and is compared). The box guard
  follows `CalculateBoundingBox` level by level and checks each offset's own
  `m_pSurface`, not only the innermost base. Generated cases include magnitudes `1e-9`
  to `1e150`, zero/negative scales and orders up to 8.
- **C++ reference in process (second review).** The isolated reference worker (a
  per-case executable linked by `solidcore-ffi/build.rs`, with GNU ld `--wrap` guards
  around NLib's extension routines) and `cc`'s `parallel` feature were moved out of this
  PR for a separate, maintainer-reviewed one: a port PR does not change build scripts.
  The shim now runs in the difftest process. UB-107 and the HANG-6/HANG-22 searches lie
  inside `CreateExtendedSurface` and cannot be detected from outside NLib, so the FFI
  refuses every `Extend` / `ExtendDir` step and the shim refuses one sent past it (what
  the worker already did where it had no `--wrap`). Generated scripts with an extension
  step are dropped by `ffi::check`; the upstream-derived fixture
  `api_offset_m12_copy_split_extend` lost its two extension steps (our additions:
  `TestSmCreateOffsetSurface` never extends) and is now `api_offset_m12_copy_split`.
  A surface of revolution base keeps that unit's in-process `1e-6` minimum line and
  circle sizes, which the worker's deadline had made unnecessary. The extension
  comparison comes back with the worker PR.
- **Dispatch compared against C++:** a `BaseOffset` op calls
  `GetBaseSurface()->CreateOffsetSurface(d, tol)` through `SmSurface*` (fixtures over
  B-spline, plane, revolution and nested bases; 81 generated cases over every kind).
- **`nurb_of`:** not given a lazy `MakeNurb`. Upstream's `SmBSplineSurface::Evaluate`
  passes `m_pNurb` to NLib without creating it, and the offset's constructor and
  `ReadFromDB` evaluate the base first, so a NURB-less revolution base is undefined
  before `GetOrCreateGwNurbPointer` could run.
  (Before `surface.surface_p3`..`p5` landed: 3060 cases, 1582 passed, 1478 deferred.)
- **Maintainer review fixes:** `Surface::create_offset_surface` dispatches as the virtual
  (plane: moved copy; surface of revolution: offset generator with the caller's
  `approx_tol_3d`, now a parameter; offset: compound distance; others: the base body).
  A face-owned offset's expanded box returns `Unported(topology.face_p2)`
  (`SmFace::GetTolerance`) for a B-spline-family base instead of the default zone
  tolerance: upstream's base finds the face through `GetFace()`'s owner chain, which the
  base now follows (see **Owners**). `SmSurface::IsPeriodic`
  ignores its `Evaluate` statuses as upstream (`Surface::evaluate_ignoring_status`): a
  B-spline keeps the derivatives it wrote when its zero-tangent repair fails, so a
  bicubic collapsed to one point is periodic both ways and its offset constructs
  (fixtures `collapsed_bspline_d1`, `..._unknown_sing`; was `values.out.len` 72 vs 1).
  Second review: the wrapper kinds no longer propagate the status either. A curve bounded
  surface passes the caller's array on to its basis surface and a STEP surface to its
  base's `EvaluateSTEP` (`Surface::evaluate_step_ignoring_status`), as upstream; a
  surface in a volume fails before writing (`None`); a plane never fails upstream (its
  port errors are array guards and still propagate). An offset's failure is `None`:
  upstream writes part of the array first only for a zero distance and a pole's
  extrapolated tangent, which `Surface::is_periodic` reaches only through a wrapper over
  an offset (an offset base's flags are copied), and the shim builds no such wrapper.
  Fixtures `step_allpt`, `bounded_allpt` (each also `..._unknown_sing`; was
  `values.out.len` 42 vs 1).
- **Generated-case exclusions from main's upstream issues** (inputs only, no tolerance
  change): extension steps are refused in process (UB-107, `CreateExtendedSurface`'s
  halving retry reading a surface a failed in-place extension half-wrote; see above).
  As `nlib.srf_adv_p1` does, the generator still makes reflection extensions (`G1`,
  `G1G2`) of a base whose extended NURB has a degree-1 direction C-max (UB-32:
  `ST_surxsdr`'s reflection-plane fallback reads `Rw[ii][kby + 2]` past its row; the port
  returns `InvalidInput`). Under the worker, two generated cases each disagreed without
  these exclusions (C++ succeeding on undefined data).
- **Exact replay:** `serde_json/float_roundtrip` preserves the eight doubles that
  previously changed by one ULP on fixture reload. A regression checks input bits.
- **Reference safety:** `gen5909` is HANG-22, a successful NaN arc length entering
  a nonterminating extension search, reproduced without earlier cases or a shared
  context. Its exact input remains an FFI regression: in process it stops at its
  extension step. Runtime guards inspect actual edited base domains and effective
  offset distance: the revolution polar-box reproducer is rejected, while nonzero plane
  offset `(hu=2,hv=0,upper)` stays covered. Shim roots/intermediates/borrowed bases use
  scoped ownership.
- **Rust nesting:** construction/restoration checks a maximum of `MAX_SURFACE_NESTING`
  (32) surface wrappers, including STEP/bounded wrappers and extended bases, before
  recursive queries.

- **Extension cache.** Upstream's `const` evaluators (`EvaluatePoint`, `Evaluate`,
  `EvaluateSimple`, and `EvaluateNormal` through the `SmSurface` body) build an extended
  B-spline base for points outside the domain when extension is allowed (only the fillet
  standard solver allows it) and store it in `m_pExtendedSurface` through a `const` cast;
  later calls (and `GetNaturalUVDomain`, `GetKnots`) see it. The `&self` evaluators carry
  the extension through the call (its recursion and repeated base-body evaluations see it)
  and drop it; the `*_extending` evaluators store it. `TrimWithDomain` uses the storing one.
  The fillet solvers (`blend.fillet_custom_solver`) hold their rail offsets mutably and
  evaluate through the `*_extending` methods and `OffsetSurface::with_evaluator`, which runs
  an inherited `SmSurface` body (`Evaluate1st/2ndDerivatives`, `ComputeInwardVector`) on the
  offset and stores the extension it makes. The difftest runs every script
  op through the storing methods; `via` ops (the `Surface` interface) only run while
  extension is off.
- **Kernel arguments.** An offset's constructor asks its base for `GetSingularities`, which
  needs the kernel, so `Surface::copy`, `copy_and_add_analytics`, `split_at`,
  `create_extended_surface(_dir)`, `create_offset_surface`, `trim_with_domain` (main) and
  `is_planar` take `&Kernel`; `BSplineSurface::create_offset_surface` takes it and now
  returns a `Surface` (the offset).
- **Owners (found when `surface.surface_point_solve_p3` landed).** The 213 cases that had
  deferred on `LocalPointSolve_1` ran, and 5 disagreed: Rust returned a solution at a pole
  corner that C++ rejects. Upstream's owning offset calls `m_pSurface->SetOwner(this)`, and
  `LocalPointSolve_1` on the base then takes `SmTol::GetZoneTol3d(GetOwner())` (the system
  zone tolerance for an offset without a face) as its loose tolerance instead of `0.01`.
  The port now does the same: an owning offset gives its base the surface owner
  `BASE_OWNER` wherever upstream calls `SetOwner(this)` (constructor, `Copy`, `ReadFromDB`;
  the extended surface gets none) and hands down the face its own owner chain reaches
  (`Surface::set_owner` keeps nested bases in step), so the base's `Surface::face`
  follows the chain as `GetFace()` does. A non-owning offset's base keeps its caller's
  owner (`Surface::clone_keeping_owner`, as upstream shares the object) in `Copy`,
  `topology.face_p2`'s thick-face offset and `DropOffset`. Main's two "any owner" tests
  that stand for `SmTol::GetZoneTol3d(const SmObject*)` (the approximation tolerance in
  `surface.surface_p3` and the zone tolerance in `surface.surface_cache_p2`) now ask
  `Surface::face`, as that function asks `GetFace()`. The three generated cases are kept
  as fixtures with the base owned and shared (`owned_base_loose_tol_*`,
  `shared_base_loose_tol_*`).
- **Fixes in other units found by this difftest** (their difftests re-run, still green):
  `nlib.srf_nurb` / `nlib.srf_adv_p1`: `N_SrfIsClosedSmooth` stores its flag before
  returning a failed-angle error (WR-166); `Surface::is_closed_smooth_flag` returns both and
  `eval_pt_derivs_unbounded` keeps the copy and flag, so a retry extends it as upstream
  (this also changes `EvaluatePointOnExtendedSurface`'s retry). `surface.surface_p1`: the
  base `CalculateBoundingBox` maps a failed surface cache to `SM_ERR_NULL_POINTER`
  (upstream's `NER` on `smsurf_GetSurfaceCache`), and refuses a polar box (UB-84) only
  where upstream recurses (curve-bounded and STEP surfaces, offsets whose extension reaches
  past the base domain). `surface.surface_point_solve_p1`: the solver's B-spline is an
  offset's base surface's, as upstream.
- **Upstream bugs kept:** WR-160 (`Cuuv` / `Cuvv` coefficients), WR-161 (`Duvv` / `Dvvv` add
  `Buuv`), WR-162 (`Duv` over `Duu`), WR-163 (`SplitAt` extension), WR-164 (`SwapUV` double
  negation), WR-165 (`STEPInversion` retry tolerance). UB-171 to UB-173, HANG-21 and,
  with every `CreateExtendedSurface` step, UB-107, HANG-6 and HANG-22 are kept away from
  the C++ side by the harness.
- **Harness.** The shim and runner compare, for `bOnlyUpperHalf`, only the entries upstream
  writes (`i + j` up to the larger order); untouched entries are caller memory upstream and
  zero or `UNDEF` in the Rust kinds. Fixture placeholders (`"NAT"`, `"SUB"`, `"MID"`,
  `"VMIN"`, `"VMAX"`, `"ON"`) are filled from the base built on the Rust side. The FFI
  rejects a split once the base domain is unknown, boxes outside a base domain it can
  still track and every extension step; the shim re-checks every box (and `IsPlanar`'s
  box over the natural domain) against the actual base domain at each level after the
  preceding steps (UB-173, HANG-21), the orders of UB-171 on the actual compound distance
  and zero-distance delegation to a plane's evaluator (UB-172).
- **Deferred:** none since `surface.surface_point_solve_p3` landed (it was the last
  deferral, `LocalPointSolve_1` reached through the `SmSurface` solver bodies run on an
  offset). `GetSingularities` runs
  the `SmSurface` body for every base, an offset base too: the constructor calls it
  through `SmSurface&`, and `SmOffsetSurface::GetSingularities()` hides, not overrides,
  the virtual.
- **Dispatch gaps:** `surface_cache::p1::calculate_bounding_box` of an offset (the cache
  callback has no kernel and does not forward the basis guesses to the base as upstream;
  no difftest reaches it; error names `surface.surface_cache_p1`); `SetOffsetDistance` through the surface
  intersector's shared `&Surface` (`intersect.surface_intersector_p1`, reached only after
  `SmFilletLaw::Evaluate`, `blend.fillet_executive_p1`).
- **Back-edges wired:** `blend.fillet_custom_solver` (PR #78) had co-ported the offset's data
  with `UNPORTED(surface.offset_surface)` evaluations. Its `OffsetSurface` now wraps this
  port (plus the base-pointer `SurfaceId`), and `LoadInitialValues` takes `crSurface1` as
  `&mut` because upstream's `const` evaluation stores the extension in it. Upstream builds
  the rail offsets with the default `SM_SS_UNKNOWN`, which waits on `surface.surface_p3`; the
  fillet harness now passes `lSingularities` (`sing`, default `16`), and its generated
  `SM_SS_NONE` cases compare `LoadJacobian` through the real evaluations. With
  `GetSingularities` landed, `SM_SS_UNKNOWN` rails construct, and `rpSurface2->LocalPointSolve`
  runs `SmSurface::LocalPointSolve` on a copy of the other rail's offset (the solver is
  borrowed shared, so an extension it makes is dropped): 448 cases, 401 pass, 0 failed,
  47 deferred (`blend.fillet_standard_solver_p2`; `LocalPointSolve_1` runs since
  `surface.surface_point_solve_p3` landed). `topology.face_p2` (#91): a thick
  face's iso-curve integrand builds its temporary `SmOffsetSurface(+-t/2)` over a copy of
  the face surface and calls `EvaluateSimple`; its 112 offset-deferred cases now pass
  (2692 cases, 2322 pass, 370 deferred) and `mass.integrator`'s one (1521 pass, 7 deferred).
  `intersect.surface_drop_lib_p2` (#112, merged after this PR started): `DropOffset`'s
  `DropVectors` / `EvaluateDirectionalDerivs` build the ported offset (not owning, the
  stored singularities) over a copy of the drop surface and run the `SmSurface` bodies,
  which `SmOffsetSurface` does not override; all its offset deferrals pass (1531 cases,
  1531 pass, 0 deferred: green).
- **Issue numbers:** renumbered past main (`ccb8ab93`) and every open PR (2026-10-17):
  UB-171..174 (first UB-99..102, then UB-116..119, then UB-162..165, then UB-165..168),
  WR-160..166 (first WR-81..87, then WR-97..103, then WR-159..165), HANG-21 and HANG-22
  (first HANG-13/14, then HANG-14/15, then HANG-17/18, then HANG-19/20).
