#### UB-279. A second `SmFeatureExecutive::DoDefeature` reads loop edges the first one's stitch deleted
- **Severity:** Crash / undefined behaviour
- **Area:** Blends and defeaturing
- **Location:** `source/SMLib/src/SmFeatureExecutive.cpp:892`, `SmFeature::BuildPatches` (and
  `SmFeature::BuildPatch`, `:989`), after `SmFeature::ApplyPatches`.
- **What happens:** `ApplyPatches` copies a patch into the base or feature B-rep and calls
  `StitchAndOrient`, which merges and deletes the old loop edges; the `SmFeatureLoop` keeps its
  `m_sBaseEdges` / `m_sFeatEdges` pointers. A later `DoDefeature` (nothing resets the feature)
  runs `BuildPatches` again: with joined patches it calls `ValidatePatch(*m_pFBrep, *m_pBasePatch,
  sEdges)` on those edges, and `BuildPatch` reads them through `GetEdgeNumberInBrep`.
- **Trigger:** `DoDefeature` twice on one executive when the first applied a patch, e.g.
  `Defeature/defillet.smb`, `AddFeature(face 8, edges 14 16, SM_RB_EXTEND_SURFACE,
  SM_RB_FROM_OTHER_BREP)`, `DoDefeature`, `DoDefeature`: segmentation fault in
  `SmFeatureExecutive::ValidatePatch`.
- **How found:** porting (`blend.feature_executive`), generated case `defeature_twice_defillet`.
  The shim refuses a `DoDefeature` that would read a loop edge no longer in its B-rep (`-996`);
  `my_test_multi_rebuilds`' second `DoDefeature` keeps stale edges too but never reads them.
- **Confirmed:** yes, native build (crash with the backtrace above, `DoDefeature` ->
  `SmFeature::BuildPatches` -> `ValidatePatch`).
