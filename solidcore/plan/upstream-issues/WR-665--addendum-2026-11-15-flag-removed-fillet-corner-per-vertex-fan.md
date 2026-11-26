Updates: WR-665

#### WR-665 (update, 2026-11-15). `fillet_corner_per_vertex_fan` is removed

`SetFilletParameters` (and `prepare_per_edge_two_offsets`) now always split each filleted edge's vertex where the body touches itself into one vertex per fan, `DoFilleting` always joins the vertices still at a split point afterwards, and `SmFilletCorner::Create` always builds no two-edge corner where an edge not blended there is a knife edge, as tangency-labs/smlib@4ecfbab and @e418750. The fix is unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::fillet_corner_per_vertex_fan`, SolidCraft's preset line, its entries in `fixes.json`, `issue_2260`'s flag list and the goldens name map, the flag-off branches, and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main add215f56: every goldens unit with this flag and `fillet_mitre_gap_patch` on moves no row; leave-one-out and flag-alone runs over the units the batch's other flags moved also move none; the rig runs with SolidCraft's preset, where the flag was already on).
- **Tests:** `tests/wr665_corner_per_vertex_fan.rs`, `tests/issue_2088_chamfer_second_offsets.rs`, `tests/issue_2015_helpers.rs` (`two_offset_chamfers_honor_the_vertex_fan_fix_and_cleanup`). Removed with the flag: their halves that pinned upstream's behaviour (status 1001 / code 3 at the self-touching corner, the Nx2 corner at a knife edge's end, and the unsplit vertex).
