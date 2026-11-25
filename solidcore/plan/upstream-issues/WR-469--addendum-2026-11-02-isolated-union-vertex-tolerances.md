Updates: WR-469

#### WR-469 (update, 2026-11-02). Isolated union vertex tolerances
The opt-in `Kernel::fixes.new_edge_vertex_tolerance_covers_gap` is reachable and changes
values on a public operation even with every other flag at its default. On the #1480
pleated-loft union (`crates/solidcore/tests/issue_1480_pleated_loft_union_edge_ends.rs`)
the flag alone changes no topology, volume or validity outcome, and grows exactly eight
vertex tolerances.

- **Same outcome, both ways:** the union is 388 faces, 1096 edges, 712 vertices and
  `is_manifold_solid` in both runs; volume `44443.49369572108`, worst edge-end to vertex
  `7.4750471556652e-6`, and the same `check_brep` (Level 2) report: 12 tolerance reports
  at `offset=1.7255363, tolerance=0.01` (the #1058/WR-306 symptom, not WR-469) plus the
  synthetic `AssertValid skipped: ValidatePointers failed` line. `AssertValid` is skipped
  in both runs, so what the test compares is that segmented-pointer report (13 lines),
  against the same lines in the other run.
- **Changed values:** the eight vertices at
  `(+-99.5929150054111, +-57.49996907908349, z)` and
  `(+-57.49996907908349, +-99.5929150054111, z)`, with `z` in
  `-0.3000124142638605`..`-0.3000124142638637`, go from an off tolerance in
  `1.70348595203878130e-5`..`1.70348595232676946e-5` (kept by
  `RefreshTolerance(FALSE)`) to an on tolerance in
  `2.85897936177208906e-5`..`2.85897936891094169e-5` (the branch's `2 * gap`-sized
  values stored in `MakeEdgeInFaceWithVU`). The store-time gap is not observable from
  the final body: later booleans move those edges' ends, so the final end gaps are
  `7.4750471556652e-6` at most; the p9 routine test bounds the branch's growth at
  `>= 2 * gap`, and the test pins these stored values (which no later pass grows
  further on this rig).
- **Test:** `wr469_union_flag_alone_grows_eight_vertex_tolerances` in the #1480 rig file
  runs the pipeline (the two loft differences, then the union) with only this flag off
  and on, asserts the identical counts, volume, worst end gap and check reports, and
  pins the eight off/on tolerance values. No kernel code changed; the flag stays opt-in
  and default-off for pinned C++ parity.
