Updates: WR-12

#### WR-12 (update, 2026-10-31). Fixed in v1.0.1

Fixed in `usd-brep` `v1.0.1` (`3fd5b95`): `SmObjsInVoxels<TYPE>::FindCoincidentPairs` (`source/SMLib/inc/SmHashTable.h:2398-2465`) advances `pBegBucketItem = pBegBucketItem->GetNextBucketItem()` across every entry in each bucket, compares every unordered pair via `pEndBucketItem = pBegBucketItem->GetNextBucketItem()`, skips a pair under `pOptLimitIndx` only when both `pBegBucketItem->GetItemId()` and `pEndBucketItem->GetItemId()` are below `*pOptLimitIndx`, and binds `SmPointItem<TYPE> * pEndPointItem = pEndBucketItem->GetItem()` as a pointer without aliasing or overwriting stored `m_sTgtPoint3d` coordinates.

- **Rust port:** `ObjsInVoxels::find_coincident_pairs` (`crates/solidcore/src/geom/hash_table.rs`) already faithfully ports the `v1.0.1` behaviour, so no `UpstreamFixes` opt-in flag is needed (`#2142`).
- **Verification:** Covered by unit tests `wr12_single_voxel_clusters_report_all_unique_pairs_without_mutation` and `wr12_voxel_boundary_deduplication_and_limit_index_and_histogram` in `crates/solidcore/src/geom/hash_table.rs` (`#2142`).
