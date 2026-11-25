# audit.recursion: source review of the fillet/heal/topology wave `2ca17d8e9..f1ac1f4c1` (PR #835, 2026-10-25)

SOURCE-ONLY follow-up to the whole-crate audit of PR #273. This is not a fresh
whole-crate audit and does not re-certify `audit.recursion`. No cargo, C++, wasm or
difftest command was run for it, and no worst-case wasm stack use is measured; the
evidence below is source accounting on the merged range. Range: base `2ca17d8e9`
(main before the wave) to `f1ac1f4c1` (authoritative `upstream/main` tip at review
time). `git diff --name-only` lists 44 files under `crates/solidcore/src`,
+8437/-1179.

Question asked of the delta: do the newly merged blend/fillet session,
classification and finalization paths, and the heal/topology callers they wire,
introduce an input-driven recursive or mutually reentrant traversal, and do the
existing explicit depth/work bounds still cover those callers without changing the
visitation order?

Method: enumerate the changed source files, read each new stage entry, and follow its
callees until traversal depth or work is controlled. Classify direct self-recursion,
mutual calls through traits/closures, finite `Vec`/arena iteration and algorithmic
work counts separately. A same-file call-graph pass over the changed files was used
only as a navigation aid; every self- or mutually-matched pair was then read. It
found no new recursive function. Apart from the known
`corner_vert_geom`/`mate_geom` pair (pre-existing and bounded, below), the matches
are one-way wrapper→body or base→override pairs:
`constant_radius_setup_offset_values` calls the inherent
`ConstantRadiusAssistedFs::setup_offset_values`
(`fillet_standard_solver/p1.rs:586`, whose signature — `self` plus the intersection
point, returning `f64` — differs from the trait method's), `swap_uv` → `swap_uv_notifying`
(`topology/face/p1.rs:1147`/`:1173`), `split_rail` → `split_rail_with_ops`
(`fillet_solver/p1/rails.rs:98`/`:107`), `rail_edgeuse_intersect` →
`_with_solver_ops` (`fillet_geom/p6.rs:1718`/`:1747`),
`find_and_set_infinite_region` → `_with_identity`
(`heal/stitch/p2.rs:1490`/`:1500`), and `TemporaryRadiusChange::new` → `set_up`
(`fillet_executive/p2.rs:1360`/`:1385`, one direction only).
`select_topology_to_keep`'s trait default forwards to the free function
(`heal/stitch/p1.rs:180`), not to itself.

### Traversal accounting for the new entry points

| Entry | Traversal | Progress / bound |
|-------|-----------|------------------|
| `FilletSession::make_nx1_corner_topology` (`fillet_executive/session.rs:660`) → `FilletCorner::make_nx1_corner_topology` (`blend/fillet_corner/topology.rs:185`) | one corner: `all_eus` index arithmetic, then `for rail in 0..2` (`topology.rs:229`); constant-time `edgeuse`/`mate`/`radial` lookups | Fixed two rails; no recursion or input-driven loop |
| `FilletSession` stage methods (`session.rs:685`, `:696`, `:708`, `:716`, `:726`, `:734`, `:745`, `:760`, `:769`, `:781`, `:821`, `:832`) → `FilletExecutive` stages | per-stage `while let Some(x) = vec.get(index)` loops over the executive's own `corners`/`solvers` (`fillet_executive/p3.rs:1593`, `:1656`, `:1681`, `:1704`, `:1741`, `:1782`, `:1827`, `:1838`, `:1849`, `:1934`, `:1998`, `:2006`, `:2052`) | Index strictly increases; the only pushes to those vectors are `create_fillet_corners` (`fillet_executive/p1.rs:971`) and `load_fillet_solver` (`fillet_executive/p1.rs:662`), neither reachable from a stage callback. See Q2 |
| `FilletSession::calc_nx1_corner_vert_geom` (`session.rs:1651`) | vertex dispatch limited to `OnVertex` and `RailXEdgeuse` (`session.rs:1674`–`:1717`); other vertex kinds return `Unported` | No call into `corner_vert_geom`, so the mate recursion is not entered from this session stage |
| `process_smooth_edge_sequences` (`p3.rs:452` → `fillet_executive/p2.rs:939`) | outer `while solvers.len() > 1` (`p2.rs:950`); collector walks tangent solvers backwards then forwards (`p2.rs:1035`, `:1090`) | Collector step cap `bound = solvers.len() + 1` (`p2.rs:1002`, checked at `:1037`/`:1092`); collector ends with `solvers.retain(Option::is_some)` (`p2.rs:1119`), so a pass strictly shrinks the list. The body `create_smooth_edge_sequence` is still always `Unported` (`p2.rs:295`), so the loop currently exits after one pass. See Q1 |
| `FilletSession::stitch_fillet_faces` (`p3.rs:1870`) → `session.rs:1032` → `topology/brep/p4.rs:1604` → heal `Stitch` | explicit `f_stack: Vec<FaceId>` with face marks (`heal/stitch/p1.rs:1589`) | Stack pop plus mark check (`:1592`–`:1605`); no call-stack growth |
| `FilletSession::shrink_fillet_faces` (`p3.rs:1928`) → `session.rs:961` → `topology/face/p1.rs` shrink/notification | per-solver index loop, then one face edit per solver | Notification delivery goes through the lent context (`session.rs:977`); a nested crate edit through it is refused (`NOTIFY_LENT`) |
| Heal stitch edit bridge (`heal/stitch/p1.rs:647`, `BorrowedStitchEdits` `:707`, `edit_glue_edges` `:1002`, `edit_delete_face` `:1057`) | `delete_face` → `topology/brep/p3.rs:1889`; `glue_edges_general` → `brep/p7.rs:1657`; `glue_vertices`/`remove_faces` → `brep/p5.rs` | `delete_face_nesting` (`brep/p3.rs:2180`) is called once (`:2139`); its shell loop (`:2235`) clears marked or shell-owned vertices from `verts` each pass. Glue paths loop over record lists; no self-call |
| `primitive::primitive_creation::model::Driver` (`model.rs:301`, `model/merge.rs:301`) | delegates to the Boolean engine; `collect_faces` uses an explicit stack with marks (`topology/topology_traverser.rs:186`); `clear_offset_maps` (`model/merge.rs:217`) removes handles per face | Traverser is mark-bounded; the attribute loop breaks on a non-live handle (`:221`–`:224`) |
| File readers | none changed by the wave | Read nesting bound `MAX_READ_NESTING` = 32 (`topology/database_io.rs:649`) is not exercised by this delta; `topology.brep_data` is #829's |

### Bounds that still cover the wired callers

- **Fillet mate nesting.** `FilletBrep::corner_vert_geom` dispatches `Mate` to
  `mate_geom` with the current `depth` (`blend/fillet_geom/p3.rs:301`), and
  `mate_geom` is the only place that increments it (`p3.rs:356`) and checks it
  (`p3.rs:343`, `MAX_MATE_DEPTH` = 64 at `:116`). Stage entries reset it to 0
  (`p3.rs:264`, `:331`), and the degenerate/tangent helpers that call
  `calc_corner_vert_geom_unchecked` (`blend/fillet_corner/p3.rs:1951`) are not
  reached from inside the mate chain, so the bound is not bypassed by a reset in the
  middle of a chain. The depth error is propagated rather than swallowed
  (`p3.rs:357`, using `is_depth_error` at `:1446`). Regression test by inspection:
  `mate_cycle_is_bounded` (`p3.rs:1634`), which builds a two-vertex mate cycle and
  asserts the depth error from both entry points.
- **Nested-edit refusal.** The edits this wave threads notifications through lend
  the kernel context: `with_lent_context` (`topology/brep/p3.rs:889`, used by the
  session's shrink/stitch/face paths and by heal `plain_edit` → `with_kernel_callbacks`,
  `brep/p5.rs:167`/`p3.rs:951`) and `Context::with_notification_context`
  (`geom/context.rs:455`, used by the heal `glue_edges_general` helper's no-edits
  fallback at `heal/stitch/p1.rs:2428`). A nested crate edit through the lent context is
  refused with `InvalidInput` (`NOTIFY_LENT`, `geom/context.rs:334`–`:339`), so a
  notification callback cannot re-enter the same edit pipeline without bound through
  library code.
- **Not reached by this delta.** `MAX_SPLIT_AT_SEAM_DEPTH` = 8
  (`topology/face/p1.rs:171`) belongs to the unchanged `SplitAtSeam`; the wave only
  threads face notifications through `shrink_geometry`/`remove_uv_trim_curves`.
  `MAX_LOOP_NESTING`/`MAX_LOOP_MARK_CALLS` (`brep/p1.rs:176`/`:1168`) are reached
  from `brep/p8.rs:670`, not from the heal `delete_face` bridge.

### Open questions (not certifications)

- **Q1 — collector contract in the new generic loop.**
  `FilletExecutive::process_smooth_edge_sequences_with` (`p2.rs:939`) is `pub` and
  takes the collector as a closure. Its termination now assumes the collector's
  postcondition ("removes at least one solver", `p2.rs:949`); the in-tree collector
  enforces it via `retain` (`p2.rs:1119`) and the session passes a closure that
  errors first (`session.rs:745`), so current paths are bounded. When
  `CreateSmoothEdgeSequence` is ported, that porter should keep the strict-decrease
  invariant explicit (or derive the loop bound from the initial solver count) rather
  than rely on the closure contract.
- **Q2 — index loops tolerate growth.** The stage loops use
  `while let Some(x) = vec.get(index)` rather than a snapshot `for`. With the sole
  production adapter (`SessionObjects`) each pass reads and mutates existing slots;
  only `create_fillet_corners`/`load_fillet_solver` push, and no stage callback calls
  them. That is the invariant to preserve: any future adapter that loads a solver or
  creates a corner from `vertices_calculate`, `edges_calculate`, `faces_make_face`,
  `rails_classify` or `shrink_actual_face` would extend the loop it is running
  inside.
- **Q3 — mate recursion reachability.** The session stage handles only
  `OnVertex`/`RailXEdgeuse` vertices, so the bounded mate chain is currently reached
  through the other corner adapters, not through `FilletSession`. Widening that
  dispatch later must keep the depth parameter threaded; the `Unported` results keep
  those cases deferred rather than silently unvisited.
- **Q4 — work, not stack.** `collect_faces` uses `stack.contains(&neighbor)`
  (`topology_traverser.rs:211`), which is quadratic in the stack; it is finite and
  unchanged by this wave, noted only so it is not mistaken for a recursion bound.
  Likewise, `MAX_MATE_DEPTH` bounds call depth, not the total number of mate
  evaluations per stage (each vertex entry may walk its own chain).

### Scope limits

- No runtime evidence: no fmt/clippy/test/wasm/difftest/C++ command was run for this
  review, so completeness of branch coverage, maximum recursion depth actually
  reached, and wasm stack headroom are not measured. Canonical CI on the documentation
  PR runs the usual gates; it does not measure worst-case wasm stack either.
- Optional dependency-graph scanning (Sentrux) was not run; it would not replace the
  call-chain reading above.
- Files owned by other lanes were read only where the changed call chains enter them;
  #818 (surface cache) and #829 (`brep_data` platform classification) are untouched.
  Unmerged contributor branches were not treated as landed.

### Changed source files reviewed

`blend.rs`, `blend/fillet_classification.rs`, `blend/fillet_corner.rs`,
`blend/fillet_corner/{p1,p2,p3,topology}.rs`, `blend/fillet_custom_solver.rs`,
`blend/fillet_executive.rs`, `blend/fillet_executive/{p1,p2,p3,session}.rs`,
`blend/fillet_executive/session/classification.rs`,
`blend/fillet_geom/{p1,p2,p3,p4,p5,p6}.rs`, `blend/fillet_intersector.rs`,
`blend/fillet_solver.rs`, `blend/fillet_solver/{p1,p2,p3,p4,p5}.rs`,
`blend/fillet_solver/p1/rails.rs`, `blend/fillet_solver/p5/{recovery,tests}.rs`,
`blend/fillet_solver/surfaces.rs`, `blend/fillet_standard_solver/{p1,p2}.rs`,
`heal/stitch/{p1,p2}.rs`, `offset/offset_executive/p1.rs`,
`primitive/primitive_creation/model.rs`, `primitive/primitive_creation/model/merge.rs`,
`sweep/api_brep.rs`, `topology.rs`, `topology/brep/{p4,p6,p7}.rs`,
`topology/face/p1.rs`.

### Post-review main advance (PR #835, 2026-10-25)

While this review was open, `upstream/main` moved `f1ac1f4c1` → `92454ca6` (#816:
Boolean naming and evaluation-copy wiring; 28 kernel files, +1517/−414) and this
branch merges it. Those files are outside the range audited above and are not
certified here. A screen of that advance found no new self- or mutually recursive
function and no new input-driven `while` loop: the added control flow is bounded
`for` loops plus the fixed `[1.0, 10.0, 300.0]` drop-retry ladder in
`topology/brep/p9.rs`; the recursive helpers the screen met
(`clone_for_evaluation_depth`, `transform_at_depth`) pre-date the audited range and
were not changed there. The `topology/brep/p3.rs` advance shifted that file's line
numbers by +2; the p3 citations above use the post-merge numbers. Recursion
conclusions remain pinned to `2ca17d8e9..f1ac1f4c1`; a claim about the #816 paths
needs its own review.
