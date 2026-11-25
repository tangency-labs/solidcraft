# surface.surf_of_extrusion_p1 (`SmSurfOfExtrusion`, part 1, 2026-10-13)

PARTIAL: 973 difftest cases (173 fixture, 800 generated), 880 bit-identical, 93 deferred
(52 `STEPInversion` of non-perpendicular generators, `SmBSplineSurface::STEPInversion` ->
`surface.surface_p1` solves; 41 `sm_IntersectGenerators` of parallel sweeps,
`SmCurve::GlobalCurveSolve` -> `curve.curve_p9`). The shim builds B-spline, `SmLine`,
`SmCircle`, `SmEllipse`, `SmHyperbola` and `SmParabola` generators and every orientation (`SM_CO_LINEAR`,
`_PERPENDICULAR`, `_INDEPENDENT`, `_COPLANAR`, `_NOT_PLANAR`) is reached.

- **Generator owner step.** The C++ constructors make the surface its generator's owner.
  `SmLine::MakeNurb` (run by `AdjustSTEPInterval` when the interval changes and the line has
  a NURB) then reparameterises the line's NURB to the owner's `GetGenDirParamExtent`.
  `AdjustSTEPUVDomain` with an unswapped surface keeps that (its `Reparameterize` call
  returns early because it compares the new STEP domain with the equal NURB domain), so the
  rebuilt surface's generator knots are the old interval's. The Rust curves only hold an
  `ObjectRef` owner, so the `UNPORTED(surface.surf_of_extrusion_p1)` stubs in the line,
  ellipse, hyperbola and parabola `MakeNurb` stay (as for `surface.surf_of_revolution_p1`);
  `SurfOfExtrusion` leaves its generator unowned and applies the line step itself
  (`adjust_gen_step_interval`; disabling it fails `linear_adjust_owner_step` and 5 generated
  cases). `SmHyperbola` and `SmParabola` take the same step at the end of every `MakeNurb`
  (nothing overwrites it), so the surface applies it for them too (without it, `adjust` of
  a hyperbola or parabola extrusion disagrees: 31 of 6000 generated cases, rel. err ~1). The ellipse's owner step is overwritten upstream by `AdjustSTEPInterval`'s own
  `EditParameterization`, so it is not repeated (one affine reparameterisation instead of
  two: the knots then agree within rounding).
- **Curve solve handles.** The generator solves (`GlobalPointSolve`, `GlobalPointSolveSTEP`,
  `GlobalCurveIntersect`) name the generator with a handle that answers
  `GetNaturalInterval`; without it the solution merge keeps near-duplicate minima (a tiny
  scale `DropPointFast` returned 2 solutions against upstream's 1).
- **Upstream defects** (in `plan/upstream-issues.md`): `EvaluateSTEP` order 5 of a B-spline
  generator smashes the stack (UB-80, confirmed; the FFI rejects it), `sm_IntersectGenerators`
  reads unset offsets (UB-81), constructor / copy hazards (UB-82), the unused clamp in
  `EvaluateSTEP` / `EvaluateSTEPPoint` (WR-61, ported as is).
- With a full-circle `SmCircle` generator a seam point gives one solution, so `STEPInversion`
  reports `SM_LT_INTERIOR`; the U-seam branch is reached with a closed tangent-continuous
  B-spline (fixtures `closed_seam_*`), as are `DropPointFast`'s in-pointing seam skips.
- **Review fixes (PR #28).** The hyperbola / parabola owner step above was missing (only
  `SmLine` had it): the shim then built no hyperbola or parabola generator, so the
  `CurveKind::Hyperbola` / `Parabola` branches were never compared. The shim, FFI and runner
  now build both (88 adversarial fixtures in `review_extra_cases.json`). The runner mapped
  `SolidError::InvalidInput` to `-1` instead of `SM_ERR_INVALID_INPUT` (1007): 14 of 6000
  seeds failed falsely on directrices of a zero extrusion vector. After the fixes 6000
  seeds give 0 failed (max rel. err 2.1e-13, ellipse rounding).
- **Lazy NURB.** Upstream's `const` `ConvertUVFromSTEPToNURBS`, `ConvertUVFromNURBSToSTEP`,
  `DropPointFast` and `GetGenDirParamExtent` make a missing NURB; the Rust `&self` methods
  cannot. The conversions and `DropPointFast` now return `SM_ERR` without a NURB (they used
  to read a `(0, 0, 0, 0)` domain silently); `gen_dir_param_extent` still reads it as is.
  The shim always calls `MakeNurb`, so this is not compared.
- **Not yet compared.** `sm_IntersectGenerators` (all deferred on `curve.curve_p9`) is
  checked only by reading; it needs a real difftest once that unit lands. The FFI's guards
  on `v0 == v1` constructor domains hides inputs the C++ handles (both sides then fail the
  build; not relaxed).
- **Second review (PR #28).** `SmHyperbola` / `SmParabola::GlobalPointSolveSTEP` start with
  `AdjustSTEPInterval(crInterval)`; with out-of-bounds evaluation on, `STEPInversion` passes
  the generator's maximum analytic domain, so the owned generator is rebuilt and its
  `MakeNurb` owner step reparameterises the NURB to `GetGenDirParamExtent`. The port solved
  on an unowned copy without that step: the solve converged elsewhere within tolerance
  (STEP u rel. err up to 9.4e-3 near `u = 0`, 3.8e-10 in the generated cases). The copy now
  takes the owner step before the solve (bit-identical after). Upstream leaves the owned
  generator adjusted to the solve interval after this `const` call; the port does not.
  The FFI's circle / ellipse radius and line length guards (`<= 1e-6`) hid valid tiny
  generators (radii down to `5e-324` agree); they now reject only non-positive sizes.
  Tiny hyperbolas and parabolas (`a`, `b`, `f` of `1e-7` and below) crash the C++ build
  (segfault), so their `1e-6` guards stay. 1082 cases in `review2_extra_cases.json`.
