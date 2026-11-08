Updates: WR-533

#### WR-533 (update, 2026-11-02). Gated at operation level through the fork's own case (#1954)

The #1954 flag audit (tdamsma, `flag-reachability-2026-10-31`) classed `boolean_splits_definition_across_split` as B-unit: the only test that failed with the flag forced off, `update_edges_splits_a_definition_across_the_split_point`, calls `update_edges` directly. The record said no C++ run was made.

`wr_533_lip_swept_flush_round_a_corner_joins_only_with_the_fix` in `crates/solidcore/tests/wr_533_lip_swept_flush_round_a_corner.rs` now runs the union. It builds the fork's capi test `lip_swept_flush_round_a_corner` (tangency-labs/smlib@f11d0ac, `capi/tests/ops_kernel.cpp`) with the calls the capi makes: the 50 x 40 x 10 block with r 4.5 corners is a capped linear sweep of its rim; the lip is the 1 x 1 square at (20.5, -20, 10) swept with `profile_at_path_start` along the top rim (three lines and two r 4.5 arcs joined into one B-spline by `create_by_joining`). It then runs `boolean_union` on a default kernel with only this flag toggled, and `check_brep(Level2)` on the result.

| | flag off (pinned upstream) | flag on (the fork) |
|---|---|---|
| union | refused: `boolean::merge::p2::make_faceuse`, code 1001 | 22 faces |
| volume | none | 19951.738837 (the block and the lip, 19951.738883) |
| check | none | valid, no failures |

The fork's C++ refused with 1006 (`SM_ERR_NULL_POINTER`); the port refuses later, in `make_faceuse`, with 1001.

Mutation: with the production read in `update_edges` (`crates/solidcore/src/topology/proto_topology/p2.rs`) forced to upstream's path, the test fails at the flag-on assertion with the same 1001. Restored, it passes.

Interaction (probe, not committed): with every `UpstreamFixes` flag on, the union succeeds with this flag off as well. Turning the flags on one at a time (with this one off) finds `proto_vertex_miss_is_false` (WR-73): with it on, the union succeeds with or without this flag, with the same 22 faces and volume (19951.738878) and a valid check. So SolidCraft, which sets WR-73, does not reach this flag on this case. Why WR-73's proto-vertex cleanup avoids the split across the definition was not traced.

No library code, tolerance, unit status or known mismatch changes.
