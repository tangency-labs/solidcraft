# topology.brep_validation: B-rep-wide coincident topology is not a validity test (2026-11-13)

`SmBrep::ContainsCoincidentTopology` searched every pair of vertices, edges and faces of a
body for two that lie within tolerance of each other. `AssertValid` ran it on every check and
never used the result (WR-785). The search is removed, with the certified span-box covers
that only pruned it (`CurveCover`, `SurfaceCover`, `span_boxes`, `cover`).

**Making it fire fails valid bodies.** A trial change made the search report whether it found a
pair and failed `AssertValid` at level 2 with the test-15 report (tangency-labs/solidcraft#15273).
Its CI run failed only on that report, on these results:

- solidcore tests (5): the touching-loops sweep (`wr591_touching_loops_sweep_any_order`, both
  tests), `issue_892_only_join_touching_loops_changes_the_sweep`,
  `a_gap_as_large_as_the_volume_leaves_the_sign_undecided`, and solidcore-convert's
  `lumps_and_solids_touching_along_an_edge_go_round`.
- Kernel cases (7): ladder `02-booleans/018-hole-tangent-to-side-face`,
  `02-booleans/021-holes-touching-at-a-point`, `05-holes-and-taps/010-hole-tangent-to-end-face`;
  booleans 307 and 3665; fillets 1390; sheet metal 1095.
- SolidCraft tests (29): the `pinch_tests`, tangent and touching holes, solids touching along an
  edge or round a pocket, a stepped cut leaving pieces meeting along a line, a blend dialog case.
- Goldens: one case each of `boolean.api_brep` and `corpus.parity`.

Each of those bodies is right. A hole tangent to a face, or two holes touching at a point, leaves
a pinch, and solidcore keeps a pinch as edges or vertices lying on each other (often in separate
shells). The sweep's stitch glues four lamina edges on one line. Geometrically the search is
correct; as a validity rule it is wrong for the bodies solidcore builds, and it cannot tell a
pinch from a boolean that left an edge on a face without imprinting it.

**What still checks coincidence.** A face's own coincident vertices and edges are `SmFace` tests 32,
33 and 34 (`assert_valid/face.rs`). They caught WR-580's duplicated tube-joint edge
(`wr580_tube_joint_no_coincident_edges`), and every body listed above passes them.

**Tolerance growth.** `docs/validity-and-tolerances.md` §5 asks that a grown tolerance not reach
topology it does not bound. That needs a check local to the entity whose tolerance grows, written
for it; the body-wide search would also flag every pinch.
