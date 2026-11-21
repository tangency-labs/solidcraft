Updates: WR-473

#### WR-473 (update, 2026-11-15). With WR-528 always on, the three Booleans end in an internal handle error

Since `close_loop_relates_only_coincident_edges` was retired (WR-528, PR #16964), `CloseIntersectionLoop` no longer relates the far edge between the mated vertices. The #1036 skins (`tests/issue_1036_coincident_skin_sides.rs`, goldens `boolean.api_brep` `issue1036_coincident_skins_*`) then go on to the coincidence intersection, and all three Booleans fail with

```
InvalidInput { operation: "boolean::topology_intersector::p2", reason: "topology handle is unknown" }
```

That error comes from `tol_of` in `boolean/topology_intersector/p2.rs`: a vertex, edge or face handle that the intersector holds names no live entity of either operand. Before, the difference was empty and the intersection was all of A (wrong bodies returned as success), and the union failed with 1001.

- **Status in the goldens:** 1007, the harness's code for both `InvalidInput` and `PortInvariant`.
- **What it is:** an internal breach, not a refusal of bad input. The operands are valid, and upstream would dereference a pointer here. So `InvalidInput` is the wrong class. It should be a `PortInvariant`, or better, no error at all once the coincidence path keeps its handles live.
- **Where (traced):**
  1. `mark_common` (`boolean/merge/p4.rs`) calls `close_intersection_loop` on a common vertex. The far edge is no longer related, so the call goes on to IntersectInsertRelate of the target edge (`iir_edge_brep`).
  2. That reaches `merge_curve_classes` and `check_for_squeeze_vertices_into`, whose `brep_combine_coincident_vertices` squeezes an edge. The squeeze deletes primary vertex 19 (and 33), and edges 35 and 36.
  3. The deletion reaches neither `close_intersection_loop`'s deleted lists nor `topology_deleted()`, so vertex 19 stays in `mark_common`'s `single` list.
  4. `fix_gaps` then calls `tol_of` on it, in its pair loop (`topology_intersector/p2.rs`). Upstream would read a freed `SmVertex*` there.
- **Open:** the fix (report the squeeze, or drop dead entries before `FixGaps`) and the correct results, about 14051.84 (A − B), 12084.49 (A ∩ B) and 36398.22 (A ∪ B). WR-473 stays open.
