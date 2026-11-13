Updates: WR-13

#### WR-13 (addendum 2026-11-01) — Opt-in preservation of removed bucket-item records in `SmHashTable::IncMaxItemCount` (#2200)

`Kernel::fixes.hash_table_grow_preserves_removed_records` (#2200, default `false`) opts into
preserving existing `SmBucketItem` records across item-array growth even when all items in the
table have been removed (`GetUsedBucketItemCount() == 0` while `GetTotalBucketItemCount() > 0`),
exposed via `HashTable::inc_max_item_count_with_fixes`, `HashTable::add_with_fixes`, and
`ObjsInVoxels::add_with_fixes` in `crates/solidcore/src/geom/hash_table.rs`.

- **Root cause (`source/SMLib/inc/SmHashTable.h:804-879`):** `SmHashTable<TYPE>::IncMaxItemCount`
  allocates a zero-filled `pNewBucketItemArray` (`smos_Calloc` + `smos_MemSet`) and copies the
  `GetTotalBucketItemCount()` existing records from `pOldBucketItemArray` only when
  `GetUsedBucketItemCount() > 0` (`:823`), yet unconditionally installs `m_sBucketItems.GetSize()`
  and leaves `m_lRemovedBucketItemCount` unchanged (`:871-877`). When items were added and all
  subsequently removed (`GetTotalBucketItemCount() == m_lRemovedBucketItemCount > 0`,
  `GetUsedBucketItemCount() == 0`), every removed record (`m_lBucketIndex == SM_UNDEF_ULONG`) is
  replaced by a zeroed record (`m_lBucketIndex == 0`, `m_sItem == NULL`, `m_lItemId == 0`,
  `IsRemovedBucketItem() == FALSE`). Any later `Remove` on a previously-removed slot index (for
  example via `SmObjsInVoxels::Remove`, whose `FindPointItems` returns all historical point-item
  indices for `sTgtItem`) treats the slot as active in bucket `0`, linearly walks bucket `0`, and
  increments `m_lRemovedBucketItemCount` again, corrupting `GetUsedBucketItemCount()` (and
  causing a subsequent growth to zero live items if `GetUsedBucketItemCount()` underflows to `0`).
- **Call-site audit:**
  - Upstream (`source/SMLib` at v1.0.1 / `3fd5b95`): `SmHashTable::IncMaxItemCount` (`:804`) is
    called by `SmHashTable::Add` (`:1049`) when `m_sBucketItems.GetSize() == m_sBucketItems.GetDataSize()`
    and by `SmObjsInVoxels::IncMaxItemCount` (`:1760`), which in turn is called by
    `SmObjsInVoxels::Add` (`:2183`) when `m_sPointItems.GetSize() + 7 > m_sPointItems.GetDataSize()`.
  - Rust port (`crates/solidcore/src`): `HashTable::inc_max_item_count` is called by
    `HashTable::add` and `ObjsInVoxels::inc_max_item_count` (from `ObjsInVoxels::add`). Outside
    `geom::hash_table`, `HealData` (`crates/solidcore/src/heal/heal_data/p2.rs`) uses
    `ObjsInVoxels` without a `Kernel` parameter on `remove_vertex_props_from_lists` /
    `cache_coin_vertices`, keeping pinned C++ parity by default.
- **Default (`false`, pinned C++ parity):** `HashTable::inc_max_item_count`, `HashTable::add`,
  and `ObjsInVoxels::add` (as well as the `*_with_fixes` methods when the flag is `false`)
  continue to overwrite `bucket_items` with zeroed `BucketItem` records (`bucket_index: Some(0)`,
  `item: None`, `item_id: 0`, links `None`) when growing while `used_bucket_item_count() == 0`.
- **Opt-in (`true`):** `HashTable::inc_max_item_count_with_fixes`, `HashTable::add_with_fixes`,
  and `ObjsInVoxels::add_with_fixes` grow capacity while keeping all existing `BucketItem`
  records intact (`bucket_index: None`, `item`, `item_id`), so subsequent `remove` and `add`
  operations skip already-removed indices without walking bucket `0` or corrupting counters.
- **Regression tests (`crates/solidcore/src/geom/hash_table.rs::tests`):**
  - `wr13_default_grow_after_removing_all_zeroes_records_and_corrupts_remove`
  - `wr13_opt_in_grow_after_removing_all_preserves_removed_records_and_invariants`
  - `wr13_auto_grow_via_add_with_fixes_and_objs_in_voxels`
  - `wr13_non_empty_and_fresh_empty_grow_parity`

`addendum_options: Kernel::fixes.hash_table_grow_preserves_removed_records`
