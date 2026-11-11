# General: WR-468 addendum: the overhanging boss's 1001 is the Nx1 corner of #1021 (WR-499), and with WR-499 on the WR-468 flag makes one round fail (2026-10-31)

Updates: plan/findings/general--flag-reachability-wr468-extended-rail-end-not-reached.md

- **Where the 1001 arises (origin/main 6a2e8c68f, instrumented build, `FilletSession` error info):**
  - Rounding only the boss's concave base arc on the block's top (two edges, split at the cylinder's seam) with the default flags fails in `SmFilletNx1Corner`: error code 3, corner `NByOne`, "Fillet Corner Error: Corner Edge Computation Failure", at the arc's end vertex on the wall.
  - That is tangency-labs/solidcore#1021: the overhang's rim continues the arc tangentially with the opposite convexity, and `MakeCornerTopology` takes it as the corner's side edge. #1021 names SolidCraft rig fillets/4174 and 4369, the latter being WR-468's own trigger.
  - Selecting all three arcs at z = 10 (the convex rim under the overhang as well) fails under every flag combination tried, so the finding's earlier runs measured that, not WR-468.
- **Pinned C++ (v1.0.1, the prebuilt `bin/cpp` libraries, a standalone `SM_API` probe):** `SmApiCreateBox(0,0,0; 30,60,10)`, `SmApiCreateCylinder` boss (height 10 on the top), `SmApiBooleanUnion`, then `SmApiFilletEdges` on the same arcs.
  - It returns 1001 for every overhanging variant ((4, 33) r 5, (3, 30) r 5, (2, 30) r 6, (1, 30) r 5), with either edge selection.
  - The control boss at (10, 30), fully on the block, gives 1000 and 18814.775309974. The port's default gives 18814.775309973586.
  - So the 1001 is upstream behaviour, not a port divergence.
- **Fork fix:** tangency-labs/smlib@420e47d (WR-499), in the port as `fillet_nx1_opposite_continuation_side_edge` with `fillet_nx1_tangent_side_edge` (WR-457). Open issue tangency-labs/solidcore#1410 reports the same topology (a plate-cylinder union overhanging the plate's side, 1001 on the pinned C++).
- **With WR-499 and WR-457 on** (top arc only), the rounds succeed and WR-468's guard is reached:

  | boss, round | WR-499 + WR-457 | + `fillet_through_extended_rail_end` |
  |---|---|---|
  | (4, 33) r 5, R 2 | Ok, 18807.824471783373 | **1001**, error code 4 "Fillet Surface Computation Failure" on the filleted edge |
  | (2, 30) r 6, R 1.5 | Ok, 19142.548432299573 | Ok, identical volume |
  | (0, 30) r 5, R 2 (axis on the wall) | Ok, 18800.086736685844 | Ok, identical volume |

- **Answer:** WR-468 does not wait on a missing fork fix: it needs WR-499 (and WR-457) on, as the fork had them when 4369 was fixed. A WR-468 test must turn those on. No geometry found yet shows WR-468's intended effect (the round completing where it otherwise fails). The one that diverges goes the other way: on the (4, 33) boss, with the extra through knot, a round that WR-499 alone completes fails with 1001. That is a candidate defect in the opt-in WR-468 path (the port's or the fork's; the fork's C++ is not available to compare), worth its own issue before WR-468 stays in the SolidCraft preset.
