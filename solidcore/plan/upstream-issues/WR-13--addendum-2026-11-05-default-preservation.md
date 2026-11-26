Updates: WR-13

#### WR-13 (addendum 2026-11-05) — Preserve removed records by default

The prior opt-in behavior is now the only behavior: capacity growth retains all
records, links, bucket heads and counts even when every item is removed. A later
removal of an old index therefore cannot corrupt the count or a live bucket.
The `hash_table_grow_preserves_removed_records` flag and three `*_with_fixes`
methods are removed; callers use the corresponding default methods. This is a
public API break. The earlier addendum describes the historical behavior.

The prior record reports a reproduced C++ defect; this change was not checked
against a locally built native reference. Its correctness is tested by state
invariants rather than reference parity.
