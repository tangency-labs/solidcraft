# sweep.topology_sweep: the non-manifold whole sweeps follow upstream's silhouette order (2026-10-27)

Updates: plan/findings/sweep.topology_sweep--nonmanifold-body-whole-sweep.md

Items 4 and 5 of the record (the two known mismatches) are resolved by one change to the
order `SmHCR::GetSilhouetteEdges` hands back. Both cases now pass, and the unit is green
with only its seven `cpp_defect` entries parked.

**Item 4, `nm_box_linear_z_stitch` (Rust `SM_ERR` in the box stitch, C++ succeeds 400/400).**
It is not the heap-layout effect the record suspected. It is a deterministic order difference.
The cause was traced with gdb on both sides (probes `gsw1`-`gsw5`, opt-0 build of main
against cpp-101):

1. Both sides find the same eight silhouette edges (the box's horizontal edges) in the same
   order.
2. C++ hands them back from its `std::unordered_map` (`m_pSilEdges`, ND-2) newest first, with
   one bucket collision swapping two edges that share no vertex. The port handed them back in
   ascending id order, which here is oldest first.
3. The whole-B-rep sweep sweeps the silhouette edges in that order. Each box corner then gets
   two coincident vertex-swept edges, one from each silhouette edge through it.
   `SmTopologySweep::SetHigher` keeps a vertex's first image only (`SmTopologySweep.cpp:75`),
   so only the edge from the first-swept silhouette edge gets the `from_lower` entry.
4. The stitch splits each of these vertical edges at the box's top corner
   (`MakeVertexSplitEdge`). The original edge keeps the half its primary edgeuse's
   orientation selects. Here the two sides agree: the same uses, the same parameters.
5. With the opposite first image, the port's `from_lower` edge kept the other half. That half
   later met a second swept edge that also has `from_lower` (keep#65, the edge (0,3,2)-(0,3,10)).
   `SelectTopologyToKeep` then returns `SM_ERR` ("both are swept entities"). In C++ that
   half is the split-off new edge, with no map entry.

**Fix.** `blend::hcr::Hcr` records its silhouette edges in the order `ComputeSilhouettes`
finds them (first insertion of each, as `(*m_pMap)[key] = value` keeps an existing key's
place). [`Hcr::silhouette_edges`] returns them newest first, which is libstdc++'s
`unordered_map` order when no buckets collide. The rule is deterministic and uses no address
or hash. It is recorded in the module's Deviations. The `blend.hcr` runner sorts the ids
before recording them, as the C++ shim's `put_sorted` does, so that unit compares the same
set as before.

**Item 5, `nm_test_rotation_neg_y_36` (C++ `SM_ERR` in the final union, Rust succeeded).**
With the same change the port now returns the same `SM_ERR` as C++. Presumably the swept
body and wire B-rep now enter `ManifoldBoolean` as they do upstream. This was observed in
the gate, not traced separately.

**Gate.**
- `sweep.topology_sweep`: `nm_box_linear_z_stitch` passes (264 values compared). No other
  `whole_*` or `nm_*` case moved. The ND-491 cases stay parked, since upstream's order still
  varies with bucket collisions there.
- `blend.hcr`: 434/434.
- The two `known_mismatches` entries are removed.

**ND-491** is unchanged. Upstream's order still depends on addresses wherever buckets
collide, and the port now reproduces the collision-free order.
