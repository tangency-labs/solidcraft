Updates: UB-154

#### UB-154 (addendum). Still present in v1.0.1 (3fd5b95)
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmCone.cpp:5434`-`5439` and `:5505`-`5521` (v1.0.1),
  `sm_GetEllipseTrimIntervals` (def. `:5315`, file static).
- **Class:** uninitialised read: the statuses of the two seam-line `smgu_LinePlaneIntersect`
  calls are still ignored, and `dParam[0]`, `dParam[1]` are still compared and used when a call
  fails.
- **Change in v1.0.1:** the function now takes its trim circles from the trim domain's v limits
  and accepts seam-line crossings within a tolerance scaled by each seam's length
  (`dStartParamTol`, `dEndParamTol`) instead of `dScaledZero`; `GlobalSurfaceIntersect` also
  sends cylinders to `IntersectWithCone`. None of this checks the two statuses.
- **How checked:** read from the v1.0.1 source. The `surface.cone_p3` shim's replay of the
  function's head (STEP domain, corners, seam vectors, the two intersection statuses) was compared
  line by line with v1.0.1 and is unchanged, so its guard still refuses exactly the inputs that
  reach the read.
- **Status:** present in v1.0.1; the difftest guard stays.
