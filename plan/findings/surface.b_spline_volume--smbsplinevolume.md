# surface.b_spline_volume (SmBSplineVolume)

- **`USE_VOLUMES` is not defined in the SMLib build.** `SmBSplineVolume.cpp` compiles only
  the `#else` branches of `AreEqual_gw_VOLUME` (`FALSE`), `sm_LoadNLibTransform`,
  `EvaluateSimple`, `EvaluateIsoParametricCurveSimple`, `EvaluateIsoParametricSurfaceSimple`,
  `CreateCanonical`, `WriteToDB`, `ReadFromDB` and `Dump` (all `SM_ERR` / no-ops). So a
  B-spline volume cannot be evaluated (`Evaluate`, `Map`, `MapPoint` return `SM_ERR`), two
  distinct B-spline volumes never compare equal (a volume equals only itself, not its own
  copy), and `TransformSimple` builds its transform then returns `SM_ERR` with the control
  points untouched (`Transform` of a volume with next maps still succeeds: it goes to the
  last map). The port follows the build (difftest: identical statuses).
- **Copies re-run the knot fixup.** Every constructor (including the copy constructor) runs
  `sm_FixupKnotVector(.., SM_EFF_ZERO_PARAM)` on all three knot vectors, so knots closer
  than `1.2e-8` are spread / merged on construction and again on every `Copy`.
- **`InvEvaluateGuessPointSimple`** indexes knots by the closest control point's index
  without a check (only `SM_ASSERT`); a record whose knot vectors are shorter than control
  points plus degree reads out of bounds. The port returns `InvalidInput`; the ffi only
  builds records with `cp + degree + 1` knots.
