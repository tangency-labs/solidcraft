Updates: WR-591

#### WR-591 (update, 2026-11-15). `loops_go_on_round_the_vertex` is removed; `OrderCurvesIntoLoops` always goes on round a shared vertex

`order_curves_into_loops` (`topology/trimming_tools.rs`) always computes the call's turn normal (`loops_turn_normal`) and goes on from the end next round the vertex (`next_round_the_vertex`). Removed: the field, SolidCraft's preset line, the goldens `enable_fix` arm, the rig's `fixes.json` entry, the flag-list test entry, and the tests' flag-off halves (`wr_591_off_the_input_order_decides`, `wr591_without_the_flag_the_capped_sweep_returns_an_open_skin`).

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes: loops touching at a vertex close in every input order.
- **Goldens:** only `primitive.primitive_creation_p2` `ub215_degenerate_face_top_line_*` (6 rows) move: a partial cone whose top line (`1e-9` to `9.9e-7` long) is shorter than the matching tolerance. Upstream's order and the new one both close the profile within the tolerance, but the new one runs the top line backwards (orients `[Same, Opposite, Opposite, Same]` against `[Same, Same, Opposite, Same]`), and the sweep then refuses the cone as axis inside face (1025) instead of upstream's invalid input (1007, the degenerate swept face). Both refuse the same degenerate input, but the new reason is wrong, so the rows are not blessed: they are listed in `known_failing.json` against tangency-labs/solidcraft#17033, which tracks the sub-tolerance curve.
- **Tests:** `wr_591_loops_touching_at_a_vertex.rs` and `wr591_touching_loops_sweep_any_order.rs` run on the default kernel; `wr_591_loops_apart_close_apart` checks two loops touching nowhere against their closed-form areas 60 and 2.
