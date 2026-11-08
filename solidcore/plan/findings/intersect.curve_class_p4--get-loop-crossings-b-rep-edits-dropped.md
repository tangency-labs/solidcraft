# intersect.curve_class_p4: GetLoopCrossings B-rep edits were dropped (2026-10-25)

Unit `intersect.curve_class_p4`, cases gen1570, gen1622, gen1841, gen1876, gen1968 and gen2061 (the six
former known mismatches). gen1570: `values.out[965..968]`, the second `GetLoopCrossings`' face props
`m_eBadMissingSeam` / `m_eBadNearMissSeam` and their directions, C++ `FALSE`/4, port `TRUE`/0.

- **Where it splits:** the script runs `GetLoopCrossings` (props computed by `SetProps_Stage2`), then
  `construct` and a second `GetLoopCrossings` on the same face. Dropping the first call makes the second
  agree (both sides `TRUE`), so C++ carries state from the first call. gdb on the C++ reference: in the
  first call `HasSeamProblem` -> `CurveOnClassify` -> `FindVertexSector` -> `SectorClassifyCurveOn` ->
  `SmEdgeuse::EvaluateBinormal` -> `smsurf_GetSurfaceCache` builds the face's trimmed cache
  (`BuildTree` -> `SmFace::CreateUVTrimCurves`), which calls `SetUVTrimCurve` on the three edge uses and
  raises the edge tolerances to 1.3988431936168952, 0.46800933301965275, 0.4680093330196321 (and the vertex
  tolerances; they were 1e-5). The second call's seam classification sees those tolerances, classifies
  the seam against an edge (no `FindVertexSector`, no `IsSectorMissingSeam`) and reports no missing seam.
- **Cause in the port:** `get_loop_crossings` reads the B-rep through `&G`; its `SetProps_Stage2` ran the
  seam check on a private snapshot that was dropped, so the trim curves and tolerance growth were lost
  (the shared surface cache slot kept the trimmed cache, so the second call never rebuilt them).
- **Fix:** the runner calls `CurveClassification::get_loop_crossings_mut` (PR #958), which runs
  `SetProps_Stage2` on the real B-rep (`FaceProps::set_props_stage2_mut`), so the edits stay there in
  upstream order, as on the shim's B-rep. No copy-then-apply.
- **Second defect exposed by the fix:** with the larger tolerances, the second call's seam
  `IntersectVertices` -> `InsertPointClass` takes the "input deviation zero" branch, whose
  `EditIntervalMax(lIndex, .., TRUE)` collapses interval `lIndex` (the last one). Upstream then writes
  `rPtClass_Interval.m_vEnd` through the reference to slot `lIndex` it took earlier, a spare slot past
  `m_lSize` that `SetSize` resets before reuse; the port's `at_mut(index)` failed with InvalidInput.
  The port now writes to whatever sits in that slot and drops the write past the end
  (`plan/upstream-issues/UB-256--insert-point-class-stale-interval-reference.md`).
- **Result:** all six cases pass (3201 cases, 0 failed, 0 deferred, no known mismatch).
