Updates: WR-533

#### WR-533 (update, 2026-11-02). The public union's exact topology with the fix, and the guard's isolation (#1954)

The #1954 flag audit classed `boolean_splits_definition_across_split` as B-unit because
the only test that failed with the flag forced off, `update_edges_splits_a_definition_across_the_split_point`,
calls `update_edges` directly. `WR-533--addendum-2026-11-02-boolean-gate.md` records the
public-API gate built afterwards: the fork's `lip_swept_flush_round_a_corner` case run
through `boolean_union`, refused with 1001 off and valid with 22 faces and volume
19951.738837 on. This addendum pins the same run at the topology level and records the
isolation of the guard.

`wr_533_lip_swept_flush_round_a_corner_has_the_block_and_lip_topology` in
`crates/solidcore/tests/wr_533_lip_swept_flush_round_a_corner.rs` builds the same block
and lip, sets only this flag, and asserts the united body's exact face, edge and vertex
counts. The test next to it, `wr_533_lip_swept_flush_round_a_corner_joins_only_with_the_fix`,
pins the flag-off refusal, so the two tests are the off/on comparison.

| | flag off (pinned upstream) | flag on (the fork) |
|---|---|---|
| union | refused: `boolean::merge::p2::make_faceuse`, code 1001 (the port refuses later than the fork's 1006, in `make_faceuse`) | succeeds |
| faces, edges, vertices | none | 22, 50, 30 |
| volume | none | 19951.738836965 (the block and the lip 19951.738882966) |
| check `Level2` | none | valid, no failures, `assert_valid` true |

Isolation: with every other `Kernel::fixes` flag default and only the production read in
`update_edges` (`crates/solidcore/src/topology/proto_topology/p2.rs`) forced dead
(`if false && k.fixes.boolean_splits_definition_across_split`), the flag-on run returns
the same 1001 in `make_faceuse`; the guard's body, not the flag plumbing, decides the
outcome. Restored, the counts above are reproduced (two consecutive runs identical).

The `WR-73` interaction already recorded in the previous addendum is unchanged: with
`proto_vertex_miss_is_false` on, the union succeeds with this flag off as well, so this
test keeps every other flag off.

No library code, tolerance, unit status or known mismatch changes.
