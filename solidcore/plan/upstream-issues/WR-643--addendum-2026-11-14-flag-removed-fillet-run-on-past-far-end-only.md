Updates: WR-643

#### WR-643 (update, 2026-11-14). `fillet_run_on_past_far_end_only` is removed

With `fillet_trace_reaches_rails`, `RailEdgeuseIntersect` runs a straight side edge on past its far end only, never behind the filleted edge's vertex (the fork's 7224896), except across a tangent seam (WR-600). The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_run_on_past_far_end_only`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** `blend::fillet_solver::p2::tests::wr643_side_edge_runs_on_past_its_far_end_only` and `tests/wr643_run_on_past_far_end_only.rs`.
