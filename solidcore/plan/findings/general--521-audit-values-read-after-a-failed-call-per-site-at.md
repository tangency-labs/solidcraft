# #521 audit: values read after a failed call, per site, at `c9c5979` (2026-10-24)

Issue #521 (filed from the fork) lists sites where SMLib reads a value a failed call never
set. At `c9c5979` none of them reads uninitialised memory: `SmVector3d`/`SmPoint3d`/
`SmPoint2d` default to `SM_UNDEF_DOUBLE` (`SmVector3d.h:56-58`), `SmExtent1d` to
`[SM_BIG_DOUBLE, -SM_BIG_DOUBLE]`, and `AngleBetween` writes `0` before it can fail. What each
site reads, and the port:

- **`SmVertex::AreEdgesTangent` / `IsTopologicalVertex`:** the UNDEF tangent (or `IsLine`'s
  line vector), angle `0`, answer `TRUE` (WR-256, correcting UB-106). The port returned
  `InvalidInput` and the `topology.vertex` shim refused the input; both now follow upstream.
  No harness input makes `GetEndTangent` fail (it builds line and B-spline edges only; WR-256
  *Trigger*), so no case changes value: `topology.vertex` 5578/5578 with the 70 new
  `end_tangents.json` cases. A composite-curve edge, whose evaluation always fails, is
  covered by the unit test `failed_end_tangent_reads_the_default_vector`.
- **`SmLoop::GetUVCornerBoundaries`:** the issue's "keeps the previous edgeuse's curve" does
  not hold here: `GetOrCreateUVTrimCurve` sets its out-pointer to the cached curve
  (`SmEdgeuse.cpp:4275`) before building, so a failed build leaves NULL and the drop-point
  branch runs. A failed `EvaluatePoint`/`DropPoint` there leaves the previous edge use's UV end
  (the points are declared outside the loop). `topology::loopuse::uv_corner_boundaries`
  is faithful on both.
- **`SmEdge::ReverseOrientation`:** a failed `ReverseParameterization` leaves the default
  interval, and the edge uses are still flipped. Faithful (`topology::edge`, Deviations).
- **`SmLoop::ContainsUVPoint`:** the four ignored evaluations (`SmLoop.cpp:546`, `743`, `815`,
  `1011`) leave UNDEF points or, at `1011`, the previous step's derivatives; they fail only for
  a failing surface evaluation, a NaN parameter or a degenerate knot span. The port returns the
  error there (`query::loop`, Deviations); the other ignored calls (`DropPoint`,
  `LocalPointSolve`, `smgu_LinePointDistance`, the zero-length-edge loop state) are faithful.
- **Edge-use loop-gap / dog-leg checks:** `GetStartPointUVLoopGap` reads UNDEF points or the
  drop guess after failed calls; `topology::edgeuse::p1` is faithful. `GetEndPointUVLoopGap`
  and `HasDogLegNearPole` are unreachable (`harness/callgraph/unused_functions.txt`) and not
  ported.
- **`sm_CalcDistAndDev`:** a failed `Evaluate1stDerivatives` leaves UNDEF point and tangents
  (`Vec3::UNDEF` in `calc_dist_and_dev`, faithful). Open: the port propagates an error from
  `evaluate_normal` (`edgeuse/p1.rs`) where upstream ignores it; that differs only for a
  zero-radius sphere (upstream's failed `Unitize` has stored `(0, 0, 1)`) or a `SrfInVolume`
  whose evaluation fails.
- **`SmFaceProps::SetProps_Stage3`:** no read of an unset value. A failed Stage 3 leaves
  `m_ePropStage` at `3a`, so a later call returns success with the reset values; the port
  copies that.
