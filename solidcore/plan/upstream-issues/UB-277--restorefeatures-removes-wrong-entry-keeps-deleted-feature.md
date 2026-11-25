#### UB-277. `SmFeatureExecutive::RestoreFeatures` removes the wrong entry and keeps a deleted feature
- **Severity:** Crash / undefined behaviour
- **Area:** Blends and defeaturing
- **Location:** `source/SMLib/src/SmFeatureExecutive.cpp:1629`, `SmFeatureExecutive::RestoreFeatures`.
- **What happens:** the loop walks a copy `sFeature` of `m_sFeatures` by index `ii`, and for each restored
  feature calls `m_sFeatures.RemoveAt(ii)` and deletes `sFeature[ii]`. After the first removal the
  indices of `m_sFeatures` are shifted, so a second restored feature removes the entry after it (or
  nothing, at the end) while the feature itself is deleted and stays in `m_sFeatures`. Its later use
  (`GetFeatures`, `DoDefeature`) and the executive's destructor (`SmObjsDelete`) touch freed memory.
- **Trigger:** `RestoreFeatures` when two or more features are restorable (`GetCanRestore()`: partitioned
  by `DoDefeature` with neither patch applied).
- **How found:** porting (`blend.feature_executive`); the shim refuses such scripts (`-997`) and the port
  drops each restored feature itself (identical to upstream for at most one).
- **Confirmed:** by reading; not run (the shim never calls it in that state).
