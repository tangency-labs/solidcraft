# surface.cone_p1 (`SmCone` / `SmCylinder`, part 1) (2026-10-13)

- **Virtual calls back into the cone.** The inherited `SmSurfOfRevolution::MakeNurb`
  rebuilds the polar converter through the virtual `CreateIsoParametricCurve`, i.e.
  `SmCone::CreateIsoParametricCurve` (a circle averaged from three iso-curve points), not
  the surface-of-revolution one. `SurfOfRevolution` now takes the iso curve as a hook
  (`make_nurb_with_sweep_params_using`, `rebuild_polar_converter(k, iso)`, shared with
  `surface.torus`, which added the same hook on `main`); `Cone::make_nurb`
  passes the cone's. A unit porting `SmSphere` needs the same.
- **Owner re-entrancy in the unbounded constructor.** `pNewLine->AdjustSTEPInterval(apex,
  INF)` runs after `SetOwner(this)`: the line's `MakeNurb` asks the cone for
  `GetGenDirParamExtent`, whose `GetNaturalUVDomain` makes the cone's NURB (virtual
  `MakeNurb`, inside the constructor) and the line is reparameterised to it.
  `Cone::new_unbounded` replays these steps explicitly; `CreateCanonical` then makes the
  NURB again. Bit-identical in difftest.
- **Faithful upstream quirks** (also in `plan/upstream-issues.md`): the
  `CreateCanonical(semi-angle, pOptHeight)` bounded branch passes `FALSE` as
  `bInsideOut` and the caller's `bInsideOut` as `bMakeNurbGenCurve`; the
  `CreateCanonical(bot, top, height)` branch for `height <= 0` passes the top radius as
  the semi-angle; `sm_TotalCoincidenceChecker` compares the wrong end points for reversed
  lines and then returns `FALSE` (status `0`, not `SM_SUCCESS`); `GetApex` returns
  `SM_ERR` for a cylinder (callers use its outputs).
- **Harness guards.** `SmPolarConversion::ConvertToNURBSParameter` dereferences the polar
  curve unchecked: a cone whose converter could not be set up (e.g. a negative radius)
  would crash in `ConvertUVFromSTEPToNURBS` / `CreateIsoCircleFromPoint`; the shim answers
  `SC_ERR_ARG` for those ops and leaves `GetHeight` out of the state (it reads
  uninitialised points when `EvaluatePoint` fails). `CreateIsoCircleFromPoint` ignores
  the `STEPInversion` status and reads its height uninitialised when the generator drop
  fails; the shim replays that inversion first and answers `SC_ERR_ARG`, the runner does
  the same test. `AdjustSTEPUVDomain` with a reversed sweep interval (`[90, 10]`) never
  returns in the C++ reference (`SmSurfOfRevolution::AdjustSTEPUVDomain`, unported in
  Rust): the FFI rejects a reversed sweep there. Everything else (parallel or zero
  placement axes, zero or negative heights, semi-angles beyond 90 degrees, reversed or
  over-long bounded sweeps) is accepted and compared: upstream returns statuses or builds
  degenerate surfaces.
- **Not compared yet.** `SmSurfOfRevolution::STEPInversion` is co-ported for the planar
  generator every cone has (its non-planar branch is `Unported`, unit
  `surface.surf_of_revolution_p2`). `AdjustSTEPUVDomain` past its apex checks, `Transform`
  past its argument checks and `ReadFromDB` past the cone's own data wait on
  `surface.surf_of_revolution_p2` / `_p3` (deferred). `ReadFromDB` is unit-tested only:
  the `surface.surface_p2` shim dumps B-spline surfaces only. The `SmSurface::ReadFromDB`
  dispatch now reads the cone data and defers on `surface.surf_of_revolution_p3`.
- **Merge renumbering.** On merging with `main` the cone's UB-78, WR-55 and WR-56 became
  UB-81, WR-61 and WR-62 (units merged first had taken those ids). Those still collided
  on `main` (with `intersect.topology_solver_p1` and `surface.surf_of_extrusion_p1`), so
  PR #61 renumbered the cone's UB-80, UB-81, WR-61 and WR-62 to UB-87, UB-88, WR-63 and
  WR-64; HANG-11 is unchanged. The id lists below are historical.
- **Review fixes.** Upstream-issue ids renumbered to UB-78, UB-80, WR-55, WR-56 (UB-73 to
  UB-77, UB-79 and WR-48 to WR-54 were taken by units merged first). Deviations now also note:
  `Cone::new_unbounded` propagates an error from `GetApex` (upstream ignores its status
  and reads an uninitialised `dApexParam`); the inversion enables out-of-bounds on a clone
  of the generator where upstream's `const` `STEPInversion` sets it on the generator for
  good (same results, the flag already propagates). **Revisit with
  `surface.surf_of_revolution_p3`:** ASCII `Cone::read_from_db` leaves the radius and
  semi-angle at `UNDEF_DOUBLE` after a failed numeric extraction, where C++11 `>>` stores
  `0`; not observable while `ReadFromDB` ends in `Unported`. The reviewer's adversarial
  cases are in `fixtures/surface.cone_p1/review.json` (3167, pruned from 3607 by keeping
  one status-only off-surface inversion per point and cone instead of one per tolerance
  and guess); all pass bit-identically.
- **Second review fixes.** The cone's uninitialised-read issue is UB-80 (main took UB-79
  for `intersect.point_class`). Deviations now also note that `CreateIsoCircleFromPoint`
  propagates an error from `ConvertUVFromSTEPToNURBS`, where upstream ignores the status
  and, with `swap_uv`, uses the `x` value the polar converter already wrote; it needs a
  polar-inversion failure on a valid polar curve, which no case reaches. The second
  adversarial set is `fixtures/surface.cone_p1/review2.json` (3875 cases, pruned from
  82,910 run against the C++ with 0 failures): semi-angles -30, 89, 89.9, 90 and 120
  degrees, bounded cones with a zero radius (inside-out, swap), `make_nurb_gen`
  generators, near-cylinders, negative and 1e-6 heights, far/tiny/rotated/skewed/non-unit
  placements, seam/apex/past-apex inversions, iso circles 1e-7 off the surface,
  derivative orders up to 4, cross-class equality and reversed/partial/ellipse
  coincidence pairs. Difftest: 8794 cases, 8446 pass bit-identically, 0 failed, 348
  deferred (`surface.surf_of_revolution_p2` 283, `_p3` 65); perturbation 8446/8446.
- **Third review fixes.** `Cone::type_string` (`GetTypeString`) returned the class name;
  upstream's `SM_COMMON(cl, sc, ty)` returns `_T(#ty)`, i.e. `"SmCone_TYPE"` /
  `"SmCylinder_TYPE"`, and `GetClassString` (`#cl`) returns `"SmCone"` / `"SmCylinder"`.
  Fixed, and both strings are now part of the compared state (shim and runner). The same
  bug is still in `SurfOfRevolution::type_string` (`surface::surf_of_revolution::p1`,
  returns `"SmSurfOfRevolution"` instead of `"SmSurfOfRevolution_TYPE"`; unit
  `surface.surf_of_revolution_p1`, already merged, not changed here). Pinned-toolchain
  clippy lints in the runner's generator fixed without changing its RNG draw order. The
  third adversarial set is `fixtures/surface.cone_p1/review3.json` (4100 cases: every
  derivative order pair incl. out of range, NURB eval at knot boundaries with every flag
  combination, `to_nurbs` at u = 720 / -725 / 1e6, iso curves with reversed, zero-size and
  over-range trims, inversion and iso circles with tolerances 0..10*scale and seam
  guesses, radius-boundary heights in `adjust`, near-identity and negative-scale
  transforms, cross-mode equality, partial/reversed circle coincidence at 1e-5..1e5 radii,
  near-coincident lines). Difftest: 12894 cases, 12426 pass bit-identically, 0 failed,
  468 deferred (`surface.surf_of_revolution_p2` 403, `_p3` 65); perturbation 12426/12426.
- **Fourth review fixes.** `sm_TotalCoincidenceChecker` takes its tolerance as a
  `const SmApproxTol3d` (callers pass a `double`): the conversion takes `fabs` and maps
  `SM_USE_DEFAULT` to half the zone tolerance; `total_coincidence_checker` now applies
  `approx_tol3d`. `CreateIsoCircleFromPoint`'s ignored `ConvertUVFromSTEPToNURBS` status
  is reachable: unbounded cones with a semi-angle just above the cylinder test (`5e-11`
  degrees) have their polar circle ~5e11 below the origin and the polar conversion
  fails. With `swap_uv` upstream then uses the sweep value the converter wrote (now
  ported); without it (or with nothing written) it reads an unset value (UB-87): the
  shim and runner refuse those inputs. The fourth adversarial set is
  `fixtures/surface.cone_p1/review4.json` (7800 cases, pruned from 35,138 run against
  the C++: tiny/negative semi-angles with every swap/inside-out, zero/over-long/negative
  sweeps, zero heights, apex-crossing cones, negative and `SM_USE_DEFAULT` coincidence
  tolerances, sub-domain and closed-vs-arc circle and line coincidences). `Cone::apex`
  propagates a failed generator inversion where upstream's `GetPositivePoint` and
  `STEPInversion` ignore `GetApex`'s status (deviation noted; no case reaches it).
  Difftest: 20694 cases, 20173 pass bit-identically, 0 failed, 521 deferred
  (`surface.surf_of_revolution_p2` 456, `_p3` 65); perturbation 20173/20173.
