Updates: WR-641

#### WR-641 (update, 2026-11-01). Under `fillet_through_extended_rail_end`, an end knot whose section is not found is skipped

WR-641 is from the flag audit that #1902 merged. This addendum only records the port's change under the same flag (tangency-labs/solidcore#1952).

- **Port change:** `c1_collect_points` (`crates/solidcore/src/blend/fillet_solver/p4.rs`) traces the extra through knots that `c1_extended_rail_end_knots` adds (d33890f's hunk). If `sm_FindFilletTsectPnt` finds no section at one of them, that knot is now skipped, and the trace runs on without it, as it does with the flag off. Before, the loop's `SER(SM_ERR)` aborted `CalcFilletGeom`. A failure at a G1 knot still aborts, as upstream's does. The flag stays off by default, so the pinned behaviour is unchanged.
- **Second trigger:** a box 40 long, 10 wide and 15 high joined with `create_cylinder` r 10, h 30 at the origin (bases coplanar), with the arc where the box's top meets the cylinder rounded r 2.
  - At the arc's ends, the section's rail on the top face lies past the box's side face (y 6 against 5). That is outside the top plane's domain, so the section's plane solve finds nothing.
  - With the flag alone, every orientation of the box (+X, -X, +Y, -Y) failed with 1001 "Fillet Surface Computation Failure". Each now rounds as it does without the flag.
  - With `fillet_torus_span_from_start_section` (WR-568) on as well, all four round to the closed form 13998.81358767 (to 1e-5).
  - Without either flag, the -X and -Y boxes fail at their corner (1001), as the pinned C++ does (`blend.fillet_executive_p3`, `attributed-box-cylinder-arc-round-*`, with the cylinder's base moved below the box to avoid #2195).
- **WR-641's own boss** (30 x 60 x 10 block, boss r 5 at (4, 33)) now rounds with WR-468 on, to 18807.824471782 (1e-9 relative). That is the fork's main without d33890f's hunk. The fork's main as it is still fails with 1001: the port deliberately differs from the fork here, under the flag.
- **The fork's own d33890f trigger** (rig fillets/4369) is unchanged: 2.49e-8 from its closed form, with the flag on and off.
- **Tests:** `crates/solidcore/tests/issue_1952_extended_rail_end_unsolved_section.rs` and `crates/solidcore/tests/wr_468_overhanging_boss_fork_parity.rs` (`wr_641_wr468_rounds_the_wall_boss_as_without_it`).
