# surface.surf_of_extrusion_p2 (`SmSurfOfExtrusion`, part 2, 2026-10-14)

- **Not a `SurfaceKind` yet.** `SurfOfExtrusion` does not join
  `surface::surface::SurfaceKind`: every `SmSurfOfExtrusion` override of a `SmSurface`
  virtual (`AdjustSTEPUVDomain`, `Reparameterize`, `Transform`, `TrimWithDomain`,
  `STEPInversion`, `DropPointFast`, the UV conversions, `MakeNurb`) needs a `&Kernel`
  (generator edits and solves), and the `Surface` dispatchers in `surface::surface::p2`
  take none. Adding the variant means threading a `Kernel` through those dispatchers and
  their callers (a cross-unit refactor), so this unit ports `GlobalSurfaceIntersect` with
  an explicit `IntersectTarget` (plane, cone, surface of extrusion, or any `Surface`)
  and `Surface::read_from_db` reads the whole `SmSurfOfExtrusion` record
  (`SurfOfExtrusion::read_from_db`, stream consumed and errors as upstream) and then
  returns `Unported { upstream: "SurfaceKind::SurfOfExtrusion", unit:
  "surface.surf_of_extrusion_p2" }` (a `DISPATCH GAP`, no `UNPORTED` marker); the
  2 `topology.brep_data` cases reading `Fillet/eg1_srf1.smb` and
  `Fillet/blade_fillet_fails.smb` stay deferred on it. Whoever threads the `Kernel`
  through the `Surface` dispatchers should add the variant (B-spline queries go to
  `SurfOfExtrusion::base`, the overrides above to the p1/p2 methods) and wire that stub.
- **Pass a curve handle to `GlobalPropertyAnalysis`.** The global solver's duplicate test
  (`IdenticalSolutions`) widens its parameter tolerance by `1e-4` of the objects' natural
  intervals. With `PropertyAnalysisOptions::handle = None` the solutions name no object,
  the interval term is 0, and near-duplicate hits upstream merges (a few `1e-9` apart in
  the parameter) all survive: `IntersectWithPlane` then made extra directrices. Callers
  must pass a handle whose `SolutionObject::natural_interval` is the curve's interval
  (here `GenHandle`).
- **`IntersectWithCylinder` and unbounded cylinders.** Upstream makes the other generator
  with `CreateIsoCircleFromPoint` at the bottom of the cylinder's heights; for an
  unbounded cylinder (or a zero tolerance) the cone's STEP inversion leaves its outputs
  unset and upstream reads them (`plan/upstream-issues.md` UB-93). The shim replays the
  inversion (as the `surface.cone_p1` shim does) and refuses those inputs; the generator
  only makes bounded `SmCone`s with tolerances it can meet. `GlobalSurfaceIntersect` only
  special-cases `SmCone_TYPE`, so an `SmCylinder` always goes to the general intersector.
- **`GlobalSurfaceIntersect` difftest.** When the special case needs more (or the other
  surface has none) upstream runs `SmAdvSurfaceIntersector`; the shim does not run it
  (the Rust side is deferred on `intersect.adv_surface_intersector`) and answers `-3`.
- **State of a found `IsNurbSurfaceSurfOfExtrusion` surface.** Its `Reparameterize`
  returns early (no NURB made) when the domains already agree; upstream's state queries
  (`GetGenDirParamExtent`) would make the NURB lazily, so the runner records whether the
  NURB exists, makes it on both sides, then compares the state.
- **Review (PR #57): `sm_IntersectGenerators` status.** Upstream `IntersectWithCylinder`
  and `IntersectWithSurfOfExtrusion` ignore the status `sm_IntersectGenerators` returns
  (keeping whatever outputs and needs-more flag it set); Rust propagates it with `?`. Not
  reachable today: every failure past the parallel test goes through the unported
  `GlobalCurveSolve` (`curve.curve_p9`). When `curve.curve_p9` lands, swallow non-`Unported`
  errors there (keep the partial curves/types and the needs-more flag), which needs
  `intersect_generators` (`surface.surf_of_extrusion_p1`) to hand back its partial result
  on failure.
- **Review (PR #57): generator record coverage.** The FFI `check_record` accepts only
  generator records of type `SmBSplineCurve` (14033). Upstream `WriteToDB` writes the
  generator's own type, and `SmLine`, `SmEllipse`, `SmCircle`, `SmHyperbola` and
  `SmParabola` records are valid too. `ReadFromDB` does not branch on the generator kind and
  built surfaces cover every kind, so this is a coverage gap only; extending the
  `topology.brep_data` Sim to line and conic records would close it.
- **Review (PR #57): self-tagged stub.** The `UNPORTED(surface.surf_of_extrusion_p2)` stub
  in `Surface::read_from_db` named this (now `partial`) unit, so a back-edge grep would
  never pick it up again. It is now a `DISPATCH GAP` (as for `SmCone` / `SmTorus`, the
  reader is ported and the error still names this unit, so the 2 `topology.brep_data`
  cases stay deferred). Its real owner is whoever threads a `Kernel` through the
  `Surface` dispatchers; raised in `plan/STATUS.md` Open issues for the coordinator.

- **Re-wiring (PR #177, 2026-10-16).** `SmPlane::IntersectWithPlane` is wired into
  `IntersectWithPlane` (linear/coplanar generator): its status is ignored as upstream; every
  failure of the plane/plane intersection comes before its first output (which it resets
  with the needs-more flag), so a failure is read as no curves and no needs-more. The
  extrusion passes the converted `SmApproxTol3d` (not the `min(.., SM_EFF_ZERO_SQRT)` it
  uses itself). A line result then needs `SmSurface::CurveOnClassify`, whose
  `SmCurveClassification::ReSet` / `IntersectNaturalBounds` are `intersect.curve_class_p2`
  / `intersect.curve_class_p8` (todo); the stub is `UNPORTED(intersect.curve_class_p2)`.
  Even once those land, the classification takes a `Surface`, which a surface of
  extrusion is not yet (the `SurfaceKind` gap above), as does the
  `SmAdvSurfaceIntersector` fall-back of `GlobalSurfaceIntersect`, whose stub therefore
  stays tagged `intersect.adv_surface_intersector` although that unit is `partial`.
  *Update (PR #85, 2026-10-18):* `ReSet` (part 2) and `IntersectNaturalBounds` (part 8) are
  now both ported and `SmSurface::CurveOnClassify` runs them for a `Surface`; the stub here
  is retagged `UNPORTED(intersect.curve_class_p8)` `IntersectNaturalBounds`, which needs a
  `Surface` the surface of extrusion is not yet (the `SurfaceKind` gap above), so it stays
  although part 8 is `partial`.
  The `sm_IntersectGenerators` status TODO above is done: `IntersectWithCylinder` and
  `IntersectWithSurfOfExtrusion` keep the partial curves/types and a `false` needs-more
  flag on failure (upstream clears it before the first failure); `Unported` and
  `InvalidInput` (guards for inputs upstream would not survive) still propagate. No
  fixture reaches such a failure today.
