# heal.heal_data_p2 (`SmHealData` p2 steps, `SmEdgeProps`, `SmVertexProps`, problem arrays; 2026-10-16 review, PR #128; partial, no disagreements)

- **Review cases.** `fixtures/heal.heal_data_p2/review_adv.json` (25 cases): edge-length
  histogram buckets 0, 1 (the x86-64 wrapping `(ULONG)floor(log10(a))` path; the C++
  reference build is the oracle for that conversion, as for every existing sub-unit
  edge length) and 10, tiny wires that give real coincident-vertex pairs followed by
  `RemoveVertexProps` / `RemoveEdgeProps` on a live `SmPairProbArray`, incremental
  coincident-vertex re-runs, duplicate targets, 1e4 and mixed-scale boxes, a 1-voxel
  flat box, `ResetEdgeProps(FALSE)` / `ResetVertexProps` re-runs. 21 pass bit-identical,
  4 deferred (`topology.edgeuse_p3`, box vertex/face gaps), 0 failed.
- **Coverage gaps (not compared yet).** No case sets `m_bBadUncontainedEdge`, so the
  `SetMin`/`SetMax` clamping of `Fix_UncontainedEdges` is never compared: the
  `topology.vertexuse` wire builder rejects an edge interval outside the curve domain.
  It needs an `.smb` fixture or a shim kind that builds such an edge. Vertex
  `m_bBadSmallZoneTol3d`, vertex `m_bWarnLargeGap3d` and face Small/LargeZoneTol3d are
  never true: they need vertex/face gaps, deferred on `topology.edgeuse_p3`; re-run when
  it lands. Line-by-line reading finds the Rust matches upstream on these branches.
- **Analytic geometry.** The shim (like `heal.gap` and the shared topology import)
  copies circles, ellipses, spheres, etc. as B-splines, keeping only `SmLine` and
  `SmPlane`. This unit does not branch on geometry kind itself, but its callees
  (`ApproximateLength`, `IsDegenerate`, `GetNaturalInterval`, the gap computations) do,
  so their analytic paths are not exercised through this unit.
- **`Cache_CoinVertices` no-work path** leaves `*pbNewProbs` unwritten upstream; the
  port returns `false` (documented in the file's Deviations; `HealTgts` initialises the
  flag to `FALSE`, so no observable difference).
- **Test-only merge.** The branch carries PR #122's (`heal.face_props`) head 845627d3,
  whose three unit-test fixes are needed for `cargo test -p solidcore`. Merge #122 first
  (or with this PR) so the CO-PORTED types are not duplicated.
- **aarch64 hosts: 41 expected failures (2026-10-17).** Both causes are the integer casts
  described under General:
  - 26 cases (`rev_wire_len_1e-5_*`, `rev_wire_len_wrap_*` and 22 generated) have edges
    of length 1e-5 to 1. aarch64 saturates `(ULONG)floor(log10(a))` to 0, so these edges
    land in histogram bucket 6 instead of 1 to 5 (UB-194).
  - 15 cases (`wire_edge_degenerate`, `rev_wire_tiny*`, `rev_wire_subtol_*`,
    `rev_tiny_*`, `gen0069`, `gen0403`, `gen0502`) have a vertex box with a zero-length
    axis. On aarch64, `MapPointToVoxel` then maps `0/0` to the first voxel instead of the
    last one (UB-35), the neighbour-voxel entries change, and the coincident-vertex
    results differ (the port's `values.out` is 15 to 60 values longer).

  Not `qsort`, `rand()` or libm: `SmHealData.cpp`, `SmEdgeProps.cpp`, `SmVertexProps.cpp`
  and `SmHealProbArray.h` call neither `qsort` nor `rand`, and with aarch64 casts in the
  port every case matches bit for bit.
