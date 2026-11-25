#### WR-13. `SmHashTable::IncMaxItemCount` zeroes bucket items after all were removed
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `source/SMLib/inc/SmHashTable.h:804`, `SmHashTable<TYPE>::IncMaxItemCount`.
- **What happens:** Records are copied only while `GetUsedBucketItemCount() > 0`. Otherwise they come back calloc-zeroed (bucket 0, NULL item, id 0), and a later `Remove` of such an item walks bucket 0 while the counters still increment.
- **Trigger:** Add items, remove all, grow, then remove one.
- **How found:** generated difftest case.
- **Confirmed:** yes ("reproduced").
- **Severity:** wrong result.
- **Rust port:** Reproduced.
