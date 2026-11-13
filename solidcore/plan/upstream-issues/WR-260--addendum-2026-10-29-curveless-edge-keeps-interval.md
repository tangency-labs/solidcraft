Updates: WR-260

#### WR-260 (update, 2026-10-29). An edge without curve keeps its interval when reversed (opt-in)

The rest of tangency-labs/smlib 47507b6's `SmEdge::ReverseOrientation` change is ported under the existing opt-in `Kernel::fixes.edge_reversal_keeps_failed_edge`. The part a failed curve reversal needs (WR-260 itself) was already on main.

- **What it changes:** upstream installs `sNewIvl` whether or not anything reversed it, so an edge without curve gets the default (uninitialised, `[1e20,-1e20]`) interval and its edge uses are still flipped. The fork keeps the edge's interval (`sNewIvl = sOldIvl`), and with the opt-in so does the port (`topology::edge::reverse_orientation`).
- **Not ported:** the rest of 47507b6 is status hygiene (team-lead ruling of 2026-10-29). `OrientEdgeSequence` passing the status on is already the port's behaviour.
- **Tests:** `reversing_an_edge_without_curve_keeps_its_interval_with_the_opt_in` in `crates/solidcore/src/topology/edge.rs`: the default interval without the opt-in, the edge's own with it.
- **Difftest:** no change on the default path.
