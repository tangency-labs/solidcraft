Updates: WR-716

#### WR-716 (update, 2026-11-17). `fillet_chamfer_corner` is removed

The blend corner's chamfer event is the default and its flag is gone (tangency-labs/solidcraft#14246, zero-row batch). Chamfers meeting at a vertex where every edge is chamfered, each a plane between two planes, are always closed by the chamfer planes themselves (`crate::blend::blend_corner::chamfer`): the Mitre, the Facet or the corner patch, of the session's corner type (`FilletSession::set_chamfer_corner`). A corner the event declines is still the pinned 3x3 corner. Removed: the field `UpstreamFixes::fillet_chamfer_corner`, SolidCraft's preset line, its `fixes.json` entry and its entry in solidcore-ops' `issue_13911` fix list, solidcore-ops' `Error::Unsupported` refusal of the Facet without the flag, and the flag-off tests: `without_the_flag_the_chamfer_corner_is_the_pinned_patch`, the pinned half of `per_edge_linear_chamfers_at_a_corner_are_the_mitre` (`tests/wr697_ball_corner.rs`), and the without-fix half of solidcore-ops' `the_facet_without_its_fix_or_on_a_round_is_refused` (now `the_facet_on_a_round_is_refused`).

- **Results:** no goldens row moves. All 283 units were run with the event forced on against main's expectations: 283 passed, 0 rows changed. SolidCraft's preset and the kernel rig (`fixes.json`) already ran it.
- **Tests:** the chamfer-corner cases of `tests/wr697_ball_corner.rs` (Mitre, Facet and corner patch, each against its closed form) and solidcore-ops' `blend_chamfer_corner.rs` run on the default path.
