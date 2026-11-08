# General: `fillet_through_extended_rail_end` (WR-468) fails a round that the fork's other fixes complete; the fork's main does the same (2026-10-31)

- **Flags:** `fillet_through_extended_rail_end` (WR-468, fork d33890f), with `fillet_nx1_opposite_continuation_side_edge` (WR-499, 420e47d) and `fillet_nx1_tangent_side_edge` (WR-457). All three are in the SolidCraft preset (`reported`, `fork`, `fork`).
- **Inputs:** `create_box(0, 0, 0; 30, 60, 10)` united with `create_cylinder((4, 33, 10), r 5, h 10)`. The base arc over the box (the two edges either side of the cylinder's seam) is rounded R 2 (`FilletSession`, circular, G1).
- **Values:** with WR-499 and WR-457 the round succeeds at 18807.824471783373. Adding WR-468 gives 1001, "Fillet Surface Computation Failure".
- **Where:** with temporary tracing, `c1_collect_points` adds the edge's end parameter (1.9999999999999976) as a through knot. Its guess point (-0.8, 36.6, 12) lies past the wall, `find_fillet_tsect_pnt` fails with 1001, and `CalcFilletGeom` returns `SM_ERR`.
- **Port against fork, hunk by hunk** (`blend/fillet_solver/p4.rs` `c1_extended_rail_end_knots` / `c1_collect_points` against fork main `SmFilletSolver.cpp:1864`ff):
  - The rails are taken in the same order, ends from `GetVertex` / `GetOtherVertex`.
  - The same filter: `SM_FV_RAIL_X_EXTENDED_EDGEUSE` with a corner.
  - The closed-edge skip, the start/end parameter from `sIvl`, and the not-already-a-knot test are the same.
  - The extra knots are traced after the G1 knots through the same guess construction, and a failed section is `SER(SM_ERR)` in both.
  - The fork's `GetRadiusCornerParameters` adds nothing for a constant radius.
  - No gate was lost.
- **Fork:** its main 83748ba, built from source, fails the same boss with 1001. With d33890f's hunk reverted it rounds the boss at 18807.824471782, matching the port without the flag to 7e-14. The fork's own 4369 test passes on its main without the hunk.
- **Conclusion:** a fork defect, recorded as `plan/upstream-issues/WR-641--extended-rail-end-through-knot-fails-an-oblique-wall-boss.md`. The port is faithful. `tests/wr_468_overhanging_boss_fork_parity.rs` pins the parity (the fork's 4369 measurable to 1e-7 with and without the flag; the wall boss's volume without it and its 1001 with it).
- **Recommendation:** SolidCraft should hold `fillet_through_extended_rail_end` back (it is in the preset's `reported` list), since its own trigger no longer needs it and it breaks this boss. The fork should either drop the hunk or skip a through knot whose section cannot be found instead of `SER(SM_ERR)`. Changing the preset is a code change, left to a separate PR.
