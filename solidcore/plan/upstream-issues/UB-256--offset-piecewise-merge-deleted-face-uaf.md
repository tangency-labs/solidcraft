#### UB-256. `SmOffsetExecutive::PiecewiseMerge` reads `GetSurface()` off offset faces already freed by `RemoveExcessFaces`
- **Severity:** Crash / undefined behaviour
- **Area:** Offsets and shells
- **Location:** `source/SMLib/src/SmOffsetExecutive.cpp:2313-2409`, `SmOffsetExecutive::PiecewiseMerge` (after `RemoveExcessFaces` at `:1580` / `:2100`).
- **What happens:** `DoExtendedOffset` calls `RemoveExcessFaces(m_pOffsetBrep)` (`:1580`), which deletes excess offset faces via `m_pOffsetBrep->RemoveFaces(sDeleteFaces)` (`:2100`) without removing those `SmFace*` pointers from `m_vFUToF`, `m_vEToF`, `m_vVToF`, or `m_vBlendToFaces` (`SmRelation` / `SmMapPtrToPtrs` do not listen to `SM_NO_DESTRUCTION`). `PiecewiseMerge` (`:2303-2412`) then iterates those relations and calls `GetSurface()` on the freed `SmFace` objects before clearing the relations at `:2419-2423`. When the freed slot has not been reused, `SmFace::~SmFace` left `m_pSurface == NULL` and `sNoSSI.RelatePair` (`SmRelation.h:203-206`, return value ignored) skips null surface pointers.
- **Trigger:** Extended offset or static shell with `m_bMergeResults = TRUE` where `RemoveExcessFaces` deletes an offset face recorded in `m_vFUToF` (such as a sheet offset near the 2×tolerance boundary at `1e-15` model tolerance).
- **How found:** difftest regression triage on PR #962 once `topology::brep::p3::destroy_face` dropped surfaces owned under caller/Notify identities
- **Confirmed:** yes, case `tiny_tolerance_sheet_above_boundary` of `offset.offset_executive_p2`
