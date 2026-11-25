Updates: WR-73

#### WR-73 (update, 2026-11-02). The miss guard decides a public union with every other fix default (#1954)

- **Why measured now.** The #1954 flag audit left `proto_vertex_miss_is_false` pinned only by the unit test `issue_848_no_hit_is_false` (`crates/solidcore/src/topology/proto_topology/p1.rs`). The guard's only production caller is the proto-vertex cleanup of `IntersectTopology` (`crates/solidcore/src/boolean/topology_intersector/p2.rs:1669`), so a public `boolean` is the place its return value can show. `imprint_*` and the stitch APIs do not call `remove_coincident_no_edge_proto_vertex` (grep over `crates/solidcore/src`), so they cannot show it.
- **What shows it.** `wr73_proto_vertex_miss_is_false_decides_the_lip_union` in `crates/solidcore/tests/wr73_proto_vertex_miss_is_false.rs` builds the fork's capi case `lip_swept_flush_round_a_corner` through public operations only: a 50 x 40 x 10 block with r 4.5 corners (a capped `create_linear_sweep` of its rim), and a 1 x 1 lip (four `create_line_segment`s) swept along half the top rim by `create_curve_sweep` with the path joined into one B-spline (`BSplineCurve::create_by_joining`), the square kept at the path start. It then calls `boolean_union` on a `Kernel::default()` each time; only `fixes.proto_vertex_miss_is_false` differs.
- **The outcomes differ, measured:**

  | outcome | flag off (pinned upstream) | flag on (the opt-in) |
  |---|---|---|
  | union | refused: `Upstream { operation: "boolean::merge::p2::make_faceuse", code: 1001, entity: None }` | `Ok` |
  | faces, edges, vertices | none | 22, 50, 30 |
  | volume (`brep_volume`, 1e-7) | none | 19951.73887787208 (block and lip closed form 19951.738882966; 2.55e-10 relative) |
  | `check_brep(Level2)` | none | valid (`assert_valid` `Some(true)`), no failure lines |

- **The miss itself, instrumented.** With a temporary print at the two `return Ok(missed)` sites and then the source restored, the cleanup calls the guard 32 times with the flag off, once per proto vertex 0..31, each against one proto edge only (30 against `ProtoEdgeId(0)`, 2 against `ProtoEdgeId(1)`), then breaks on the first `true`; with the flag on it calls the guard 330 times over the same 32 vertices and all 16 proto edges `ProtoEdgeId(0..15)`, every miss returning `false`, so the cleanup reaches the edges that do contain a vertex. This is the record's defect: a miss ends the loop, so a vertex inside a later proto edge is never removed.
- **Isolation.** Every other `Kernel::fixes` flag is default, including `boolean_splits_definition_across_split` (WR-533), so the union above succeeds with this flag alone. `WR-533--addendum-2026-11-02-public-boolean-topology.md` recorded the interaction from a preset probe; this run isolates this flag.
- **Mutation.** With the production read replaced by `let missed = true;` (guard dead), the flag-on run returns the same `make_faceuse` 1001 and the test fails at the `unwrap` on line 179; with `let missed = false;` (guard always on), the flag-off run succeeds with `Ok(22)` and the test fails at the refusal assertion on line 161. Restored, the test passes and `git diff -- crates/solidcore/src` is empty.
- **Commands.** `env -u RUSTUP_TOOLCHAIN -u RUSTC_WRAPPER CARGO_BUILD_JOBS=2 nice -n 10 cargo +1.98.0 test -p solidcore --test wr73_proto_vertex_miss_is_false` exits 0 (two consecutive restored runs identical); the two mutations exit 101 at the lines above. Measured on this branch's base, not against the pinned C++ build.
- No library code, tolerance, unit status or known mismatch changes.
