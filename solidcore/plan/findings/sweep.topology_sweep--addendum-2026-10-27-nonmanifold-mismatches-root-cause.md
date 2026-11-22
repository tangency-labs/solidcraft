Updates: plan/findings/sweep.topology_sweep--nonmanifold-body-whole-sweep.md

# sweep.topology_sweep: root causes of the two non-manifold whole-sweep mismatches (2026-10-27)

**Unit:** `sweep.topology_sweep`. Both known mismatches of the non-manifold body fixtures
(items 4 and 5 of the updated record) are the C++ silhouette visiting order, not a Rust
divergence. The Rust port visits the HCR silhouettes in ascending edge id order; a probe in
a private copy of `SmHCR.cpp` shows that this is exactly the order in which the C++ inserts
them into `m_pSilEdges`. The C++ visits them in `std::unordered_map` iteration order (ND-2).
Evidence came from a standalone driver that calls `sc_sweep_topology_sweep_body` in fresh
forked children after varied parent allocations, and from private copies of the upstream
sources linked ahead of the reference library (silhouette reordering, `SER` trace,
`Relate`/`AddEdgeDefinition` prints). None of it is committed.

1. **`nm_box_linear_z_stitch`: parked as ND-491.** The unmodified C++ also returns
   `SM_ERR` for this body: 7/1000 heap-varied processes, 44/1000 with shuffled free lists
   (previously 0/400). Each failure is in the box stitch. With the silhouettes forced into
   the Rust order, C++ fails the stitch for this body and for the box alone in every run,
   and the difftest record agrees with Rust in every value
   (`plan/upstream-issues/ND-491--addendum-2026-10-27-box-with-wire-stitch.md`).
2. **`nm_test_rotation_neg_y_36`: known mismatch, not parked (ND-492 is evidence only).** The C++ `SM_ERR` comes from
   `SmMerge::MakeFaceuse` (`SmMerge.cpp:7077`) in the final wire union. The swept body has
   two copies of the arc of face vertex (10,0,0), one per unstitched side face. Their
   edge definitions are added in face order, and the face order follows the silhouette
   order. `SetRelationships` relates the wire arc to whichever comes first. With the
   libstdc++ order, that is the copy-vertex arc, and `MakeFaceuse` then finds the vertex mate
   on neither end. libstdc++ visits the first-inserted silhouette last in all 5450 observed
   processes, so the C++ never succeeds on this build. With the insertion order forced, C++
   succeeds and agrees with Rust in every value (1 region, 11 shells, 15 faces, 66 edges, 63
   vertices). Random forced orders give both outcomes (27/60 succeed)
   (`plan/upstream-issues/ND-492--whole-brep-sweep-wire-union-follows-silhouette-hash-order.md`).

No Rust code changed. `nm_box_linear_z_stitch` is parked (ND-491); `nm_test_rotation_neg_y_36` stays a visible known mismatch (no `cpp_defect`: the cause is shown by forced-order probes only), so the unit is partial.
Open (unchanged): a Rust plumbing refusal swallowed by `ignore_status` (item 1 of the
updated record).
