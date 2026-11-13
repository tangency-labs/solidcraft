# geom.periodic_extent1d (green; no disagreements)

Green, 4054 difftest cases, all bit-identical. Upstream quirks ported as is (not bugs in
the port):
- **`AddInterval` on an empty extent gives the full period.** The empty branch computes
  `m_bFullPeriod = (m_dPeriod - |m_dMax - m_dMin|) < ScaledZero` on the empty sentinels
  `[1e20, -1e20]`, which is true for any period below `2e20`, so the extent becomes
  `[0, period]` full instead of `[from, to]`. Case `add_empty_quirk`.
- **`SetMinMax` updates the extent even when it returns `SM_ERR_INVALID_INPUT`** (min or
  max outside `[0, period]`). `PeriodicExtent1d::set_min_max` does the same and documents it.
  When the bounds differ but span the period, `m_bFullPeriod` is set without snapping
  `min`/`max` to `[0, period]`.
- **`IsInit` compares the `int` `m_bFullPeriod` with `-SM_BIG_DOUBLE`**, so it is always
  true; it only feeds a compiled-out assert (and is unused), not ported.
- Only the 17 functions owned by the unit are ported; `Invert`, `AddValue`, `Union`,
  `Intersect`, `Subtract`, `Evaluate`, `GetLength`, `IsPoint`, the 3-argument constructor
  etc. are unreachable from the upstream entry points (`harness/callgraph/unused_functions.txt`).
