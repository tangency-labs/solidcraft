# topology.brep_p7: the read leaves upstream's cache state; the harness copy dropped it (2026-10-28)

Updates: plan/findings/topology.brep_p7--read-from-file-leaves-no-face-surface-caches.md

**The premise of the finding was wrong.** The Rust `p7::read_from_file` leaves the same
face-surface cache states as upstream's `SmBrep::ReadFromFile`. Counted as (no cache,
cache with unbuilt tree, cache with built tree) straight after the read:
- `Mouse.smb`: C++ (1, 0, 3), Rust (1, 0, 3).
- `Endcap.smb`: C++ (0, 1, 0), Rust (0, 1, 0).
- `sphere.smb`: C++ (0, 0, 1), Rust (0, 0, 1).

C++ was measured with a probe that wraps `SmCacheMgr::GetOrCreateObjectCache` and reads
each face surface's `m_pTree` after the read. Rust was measured with a temporary test on
`own_reader::read_file` (`Surface::cache_tree_state`). Neither probe is committed.

**Where the caches come from (C++ backtraces).** Mouse's three built trees and the
sphere's tree are made by the read's heal:
`HealTgts -> Fix_InfiniteRegion -> FindAndSetInfiniteRegion -> RayFire ->
BrepPointSolve -> SmTLSPointFace::SolveIt -> GlobalLineIntersect`. Endcap's unbuilt
cache is made earlier, by `HealTgts -> Cache_VertexProps -> SmVertexProps::SetProps ->
GetVertexFaceGap -> CalcVertexFaceGap -> SmVertexuse::ComputeUVPoint`. The Rust read
runs both steps (`HealData::heal_targets`, with editing enabled) and records the same
states.

**What dropped them.** The difftest harness's `own_builder::renumber_with` copies the
read B-rep into the shim's traversal order. It cloned each face surface
(`f.surface.clone()`), and `Surface::clone` gives the copy an empty cache slot, so
every renumbered face read as "no cache". That gave the (4, 0, 0) and (1, 0, 0) of the
finding.

**Fix.** `Surface::copy_cache_slot_of` (unit `surface.surface_p2`) gives the copy the
source's tree state and its kept trimmed cache. The cache's handles are renamed into
the new ids and its face binding is keyed to the new owner. `renumber_with` calls it for
every face. No C++ state reaches the Rust side. `offset.offset_geometry_creation`
`gen0266` passes (926 cases, 910 passed, 16 rejected), so its known mismatch and the
ratchet waiver are removed. The reader itself needed no change.
