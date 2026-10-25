# geom.hash_table (green; no disagreements)

619 default cases (19 fixture + 600 generated) and a 10,019-case run (`--seeds 10000`)
agree bit for bit with upstream (`max_rel_err = 0`). Fixtures replay the
`SmHealData::Cache_CoinVertices` pattern (12x12x12 voxels over the vertex box, `7 * n`
estimate, all-pairs search with histogram, then an incremental batch with tracked buckets
and the old vertex count as limit) on `Points.pts`, `FitCrvPoints.pts` and
`PointSet/Section*.pts`, plus edge cases. Upstream behaviour later agents must know:

- **Use-after-free in `SmObjsInVoxels::IncMaxItemCount`.** It re-points every bucket item's
  `m_sItem` at `&m_sPointItems.GetAt(id)` *before* `SetArray` swaps in the new array and
  frees the old one, so once a model that still has items in buckets grows (`Add` with
  `size + 7 > GetDataSize()`), every bucket item dangles (`FindCoincidentPairs` then reads
  freed memory). `SmHealData` only avoids it while incremental adds stay within the
  `max(1.5 * 7n, 7n + 64)` slots reserved by the first `SetSize`. The Rust port stores
  indices (the intended contents); the shim refuses such programs (`SC_ERR_ARG`) and the
  generator skips them, so that path is not difftested.
- **`FindCoincidentPairs` only compares against the bucket's first item.** `pBegPointItem`
  / `rBegPoint` are bound once per bucket and never advanced, so two coincident items that
  are both behind the bucket head are never paired (only head-vs-other pairs are found).
  Also `rEndPoint` is a `SmPoint3d&`: `rEndPoint = pEndPointItem->m_sTgtPoint3d` copies each
  end item's point into the item after the current begin item, so the stored points of the
  model change during the search. Both kept faithfully (`geom::hash_table` module docs).
- **Points below the voxel box snap to the top voxel.** `(ULONG)((x - min) / len)` is UB for
  negative values; x86-64 g++ truncates signed (`-3.2` -> `2^64 - 3`), which the clamp then
  maps to `cnt - 1` (the comment says the low end). NaN maps high, `+inf` / `>= 2^64` map to
  `0`. The port emulates this conversion (`ulong_from_f64`).
- **Growth after every item was removed zeroes the bucket items.** `SmHashTable::
  IncMaxItemCount` copies the records only while `GetUsedBucketItemCount() > 0`; otherwise
  they come back calloc-zeroed (bucket `0`, NULL item, id `0`), and a later `Remove` of such
  an item "removes" it from bucket 0 (no-op walk, counters still incremented). Reproduced.
- `SetSize` does not reset `m_lRemovedPointItemCount`; `SmHealData` passes the old *vertex*
  count as `pOptLimitIndx`, which `FindCoincidentPairs` compares with point-item indices
  (one vertex can own up to 7 point items). An empty `pOptBucketIndices` reads `GetAt(0)` of
  an empty array (UB; Rust checks no bucket, the shim refuses it).
- No `UNPORTED(geom.hash_table)` stubs existed to wire.
- **The shim refuses tracked buckets past the table (2026-10-17).** The tracked-bucket
  list survives `SetSize`. After the model shrinks, op 4 mode 1 can therefore pass
  `FindCoincidentPairs` an index at or past `GetBucketCount()`, and upstream reads
  `m_sBuckets` past its end. The generator drops such programs only through its own
  Rust model. On x86-64 that model matches the C++, so the shim never saw one. On
  aarch64 the C++ voxel indices differ (see General), and `gen0270` reaches the read: an
  add on a box with zero height lands in bucket 42 there (bucket 12 in the port), then
  `SetSize` shrinks the model to 16 buckets. The full run then segfaulted in
  `FindCoincidentPairs` or returned garbage, depending on heap contents; single cases
  passed. The shim now returns `SC_ERR_ARG` for such a list, as the port returns
  `InvalidInput`. On aarch64 the unit now fails the same way in every run (184 passed,
  435 failed) instead of crashing, so `difftest --green` no longer aborts there.
