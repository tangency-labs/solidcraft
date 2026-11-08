# surface.volume_p2 (`SmVolume`, part 2)

Difftest: 2265 cases (265 fixtures: `TestSmTransform`-derived boxes, points and inverses,
edge cases, and 210 review cases in `review_adversarial.json` over scaled, permuted,
skew and degenerate bases, partially unbounded, huge and tiny boxes and oriented chains;
2000 generated), 2263 pass, 2 deferred (`surface.unbend_volume`),
max_rel_err 0 (after merging `surface.b_spline_volume_n_lib`, whose B-spline
`IsCurveInParamDomainSimple` / `HasDiscontinuitiesSimple` the 90 formerly deferred cases
reach). A 60,000-case run was also clean. The shim
builds plain, transform, twist (component constructor), B-spline and default unbend
maps, each but the twist optionally oriented.

After `surface.unbend_volume` merged (PR #20): its co-ports of `EvaluatePoint` and
`IsPointInParamDomain` were replaced by this unit's versions, the unbend overrides
(`EvaluateBoundingBoxSimple`, `GlobalPointSolveSimple`, `LocalPointSolveSimple`) and
`SmUnbendVolume::ReadFromDB` are dispatched, and `SmUnbendVolume::ReadFromDB` reads the
base data. The shim also builds component-constructed unbends (kind `5`), the generator
puts unbends in any op, and `fixtures/surface.volume_p2/unbend.json` adds 2290 unbend
cases (frames tilted, huge, repaired, neutral distance below the tolerance, negative,
default; chained with transforms, twists and other unbends; every op; unbend database
texts). Difftest: 4555 cases, 4442 pass, 113 deferred (`surface.surface_p8`: the
unbend's `IsCurveInParamDomainSimple` needs `SmCurve::HasPlaneCrossings`), max_rel_err
0; a 62,555-case run is clean.
- **Unbend partial outputs.** `SmUnbendVolume::Evaluate` writes its own image into the
  caller's array before its next map's `Map`, like the twist (found by the 30k-seed
  run: `EvaluatePseudoBoxSimple` of an unbend followed by a B-spline whose domain the
  sample leaves).
- **Unbend box guard.** `SmUnbendVolume::EvaluateBoundingBoxSimple` uses a null or
  deleted line when a bounded box has a zero-length edge (unit `surface.unbend_volume`).
  The FFI accepts an unbend box mapping only on the caller's own box (`EvaluateBoundingBox`
  and `EvaluateBoundingBoxSimple` on map 0, the domain box of `InvEvaluatePoint` /
  `GlobalPointSolve`) with every side at least `1e-9` or unbounded; a box computed by an
  earlier map or by `MapPseudoBox` is rejected whatever its size.

After `surface.bend_volume` merged (PR #22): the bend overrides
(`EvaluateBoundingBoxSimple`, `GlobalPointSolveSimple`, `LocalPointSolveSimple`) and
`SmBendVolume::ReadFromDB` are dispatched, and `SmBendVolume::ReadFromDB` reads the base
data (the bend unit's deferred `read_db` case now passes). `SmBendVolume::Evaluate`
also writes its own image before its next map's `Map`, so `evaluate_point_written` keeps
it like the twist's and unbend's. The shim builds default and component-constructed
bends (kinds `6`, `7`), the generator puts bends in any op and bend texts in `ReadDb`,
and `fixtures/surface.volume_p2/bend.json` adds 5014 cases (frames standard, tilted,
huge, tiny and negative neutral distance, below the tolerance, repaired zero/parallel
axes, zero tolerance; chained with transforms, twists, unbends, bends and a plain
volume; every op; bend database texts; the call paths the FFI guard rejects are left
out). Difftest: 11220 cases, 10922 pass, 298 deferred (162 `curve.curve_p9`: the bend's
`IsCurveInParamDomainSimple` needs `SmCurve::IsOnPositiveSideOfPlanes`; 136
`surface.surface_p8`), max_rel_err 0; a 109,220-case run is clean. The `ReadDb` guards of
`surface.bend_volume` and `surface.unbend_volume` now apply this unit's strict text
grammar (`surface_volume_p2::check_text`), since their readers end in the base read.

Notes for later agents:
- **Partial outputs of ignored statuses.** `SmTwistVolume::Evaluate` writes the twist's
  own image into the caller's array before its next map's `Map` fails. Where upstream
  ignores `EvaluatePoint`'s status (box samplers, `IsPointInParamDomain`, gap loops) the
  point is therefore the twist's first out-space image, not the previous value
  (`Volume::evaluate_point_written`). The base `Evaluate` writes nothing on failure.
- **Undefined call paths.** `SmTransform::EvaluateBoundingBoxSimple` with a NULL
  parameter pseudo box (UB-42) is reachable from `EvaluateBoundingBox`, `MapPseudoBox`,
  `InvMapPoint`, `InvEvaluatePoint` and `GlobalPointSolve` (UB-90; it crashed the C++
  run). The port returns `SolidError::Unsupported` at the dispatch (also for a twist's
  unbounded box without a box output). The FFI guard models the call paths over the
  chain's kinds and orient flags; whether the box/line domain checks that precede them
  pass is structural (a plain `SmVolume` later in the chain fails them), except for a
  B-spline map, where the guard rejects as if the check passed.
- **`ReadFromDB`** (UB-89): a next map of `SmVolume_TYPE`, an orient map that is not a
  transform or fails to read, and a map type missing after a "has map" flag are
  undefined upstream and `SolidError::Unsupported` here. The FFI only accepts ASCII text
  in a strict token grammar (one well-formed token per value) so it
  can predict those cases. `SmTransform::ReadFromDB` and `SmTwistVolume::ReadFromDB` now
  return the whole volume (`Transform::read_from_db(db, version) -> Volume`).
- **Wiring.** `UNPORTED(surface.volume_p2)` stubs replaced in `geom::transform`
  (`EvaluatePoint` in the unaligned box branch now a function argument; `ReadFromDB`),
  `surface::twist_volume` (`ReadFromDB`), `curve::crv_in_volume` (`EvaluatePoint`,
  `ReadFromDB`) and `geom::tol` (`GetScaledZero(SmVolume)`). `geom.transform` and
  `surface.twist_volume` have no deferred cases left; `curve.crv_in_volume` keeps only
  `intersect.curve_class_p4`.
