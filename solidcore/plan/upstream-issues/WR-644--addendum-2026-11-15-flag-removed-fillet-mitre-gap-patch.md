Updates: WR-644

#### WR-644 (update, 2026-11-15). `fillet_mitre_gap_patch` is removed

`SmFilletNx2Corner::AdjustTopology`, at a bevel of two blends at a face's concave corner whose rails meet the side edge at two places, now always builds the concave corner's topology and fills the gap with a patch on the side face's extended surface (`FilletCorner::bevel_gap_patch`), as tangency-labs/smlib@2223108. The fix is unconditional and the flag is gone; SolidCraft already ran with it on. Mitres at such corners still come from `fillet_mitre_concave_face_corners` (still a flag) or a corner set to bevel. Removed: the field `UpstreamFixes::fillet_mitre_gap_patch`, SolidCraft's preset line, its entries in `fixes.json`, `issue_2260`'s flag list and the goldens name map, the flag-off branch, and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main add215f56: every goldens unit with this flag and `fillet_corner_per_vertex_fan` on moves no row; leave-one-out and flag-alone runs over the units the batch's other flags moved also move none; the rig runs with SolidCraft's preset, where the flag was already on).
- **Tests:** `tests/wr644_mitre_gap_patch.rs`, `tests/wr650_tabbed_plate_mitred_rounds.rs`, `tests/issue_2020_offset_mitred_below_depth.rs`. Removed with the flag: `wr644`'s checks that the two sloped-basin chamfers fail without it, and the equal-slope basin's run without it.
