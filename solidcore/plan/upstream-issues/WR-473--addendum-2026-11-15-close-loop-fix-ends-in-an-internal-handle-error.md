Updates: WR-473

#### WR-473 (update, 2026-11-15). With WR-528 always on, the three Booleans end in an internal handle error

Since `close_loop_relates_only_coincident_edges` was retired (WR-528, PR #16964), `CloseIntersectionLoop` no longer relates the far edge between the mated vertices. The #1036 skins (`tests/issue_1036_coincident_skin_sides.rs`, goldens `boolean.api_brep` `issue1036_coincident_skins_*`) then go on to the coincidence intersection, and all three Booleans fail with

```
InvalidInput { operation: "boolean::topology_intersector::p2", reason: "topology handle is unknown" }
```

That error comes from `tol_of` in `boolean/topology_intersector/p2.rs`: a vertex, edge or face handle that the intersector holds names no live entity of either operand. Before, the difference was empty and the intersection was all of A (wrong bodies returned as success), and the union failed with 1001.

- **Status in the goldens:** 1007, the harness's code for both `InvalidInput` and `PortInvariant`.
- **What it is:** an internal breach, not a refusal of bad input. The operands are valid, and upstream would dereference a pointer here. So `InvalidInput` is the wrong class. It should be a `PortInvariant`, or better, no error at all once the coincidence path keeps its handles live.
- **Open:** which `tol_of` call sees the dead handle, and why. Read with a trace of `close_intersection_loop` and the coincidence intersection on the #1036 skins. WR-473 stays open: the correct results are about 14051.84 (A − B), 12084.49 (A ∩ B) and 36398.22 (A ∪ B).
