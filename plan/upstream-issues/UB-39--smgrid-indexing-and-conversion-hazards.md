#### UB-39. `SmGrid` indexing and conversion hazards
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmGrid.cpp:881` `Step3DDDA` (stepping after an exit indexes voxel `m_lOut*`); `:1143` `GetVoxelAddress` (NaN to `ULONG` for a zero-size axis); `:369` `~SmGrid` (reads `m_aVoxels[0][0]` with a zero voxel count); `:525` `AddElement(..., SM_GE_HIERARCHY, ...)` (dereferences a NULL element).
- **What happens:** Out-of-bounds reads, UB conversions, NULL dereference.
- **Trigger:** Call `Step3DDDA` again after `rbExitedGrid = TRUE`; build a grid with a zero extent on one axis.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`, or a saturating cast / constructor check.
