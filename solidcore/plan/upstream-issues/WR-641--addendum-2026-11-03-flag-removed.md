Updates: WR-641

#### WR-641 (update, 2026-11-03). `fillet_through_extended_rail_end` is removed

The flag that carried the fork's d33890f hunk (WR-468) and the port's skip of an unsolved end section (#1952, the 2026-11-01 addendum) is removed (tangency-labs/solidcore#2500). It patched the old construction's extended rail end; the construction kept is the blend corner (`fillet_blend_corner`) and its End.

- **Removed:** the field `UpstreamFixes::fillet_through_extended_rail_end`, `c1_extended_rail_end_knots` and the through-knot branch of `c1_collect_points` (`crates/solidcore/src/blend/fillet_solver/p4.rs`), and every test's setting of the flag. `c1_collect_points` again traces the G1 knots only, and a section not found at one fails the trace, as upstream.
- **Evidence for removal** (MarkusReichart on #2500, main `72dc5f364`): SolidCraft's 308 rig cases give the same verdict and values with and without the flag. Among solidcore's tests only `issue_1945_rail_uv_trim`, `issue_2089_tangent_tracer_crease_round` and `issue_2175_chamfer_rail_past_tangent_rim_end` changed without it.
- **Tests:** `issue_1952_extended_rail_end_unsolved_section.rs` and `wr_468_overhanging_boss_fork_parity.rs` keep their cases without the flag (they asserted the same outcome both ways). `issue_1945_rail_uv_trim.rs`, `issue_2089_tangent_tracer_crease_round.rs` and #2175's chamfer run under the blend corner.
