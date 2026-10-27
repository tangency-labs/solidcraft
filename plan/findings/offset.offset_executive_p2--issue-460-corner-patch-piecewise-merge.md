# offset.offset_executive_p2: #460, the shell of a fully rounded box failed in the offset's piecewise merge; fixed with the v1.0.1 pin (2026-10-26)

tangency-labs/solidcore#460 (SMLib fork, not fixed there) is a 30 x 20 x 10 box with all
twelve edges rounded r3, shelled 2 inward. In the fork it returns success but fails the
level-2 check when open at the top. Shelled closed, it is valid.

At the c9c5979 pin:
- The Rust port failed both shells, closed and open at the top, in every run, with
  `boolean::merge::p2::make_faceuse` `SM_ERR` (1001).
- The C++ usually failed too, but its outcome depended on heap layout and some layouts
  crashed (ND-261, UB-264).

With the reference pin moved to usd-brep v1.0.1 (3fd5b95) and the port following 1.0.1,
both shells succeed, on both sides. In the pin101 gate run:
- `issue_460_closed_shell` passes.
- `blend.fillet_executive_p3 --filter shell` passes 11 of 11 cases with max_rel_err 0.

## Probe

`crates/solidcore/tests/issue_460_open_shell_rounds_thicker_than_wall.rs` builds the rounded
box with `fillet_edges` (r3, circular, G1) and shells it with
`OffsetExecutive::shell_brep` (extended convex edges, merged results) through
`tests/support/offset_fixture.rs`.

- **Input:** 26 faces, volume 5572.6195 against the closed form 5572.6194 (the core
  24 x 14 x 4 grown by 3).
- **`issue_460_closed_shell`:** asserts a manifold solid whose volume is within 1e-5 of the
  closed form 3116.484 (the core grown by 3, less the core grown by 1).
- **`issue_460_open_top_shell` (ignored probe):** the C++ also returns success here, but
  its body reported `SmApiBrepIsManifoldSolid` FALSE at c9c5979, so the oracle (a manifold
  solid) is not settled.
- **`issue_460_closed_shell_by_round_set` (ignored probe):** at c9c5979 the closed shell
  with fewer rounds was a manifold solid in every case, so the failure needed the corner
  blend patches where three rounds meet:

  | Edges rounded r3 | Volume   |
  |------------------|----------|
  | 1 vertical edge  | 3485.97  |
  | 4 vertical edges | 3431.89  |
  | 4 top edges      | 3338.86  |

## Where it failed at c9c5979

gdb on the closed shell, in the port:

```
OffsetExecutive::shell_brep -> do_solid_offset -> do_extended_offset
  -> piecewise_merge (offset_executive/p1.rs)
  -> merge::merge_offset -> SweepStore::piecewise_merge_skipping
  -> boolean::merge::p3::piecewise_merge -> store::merge::manifold_boolean
  -> boolean::merge::p4::manifold_boolean -> editing_stages -> make_other_faces
  -> make_faceuse -> make_faceuse_attempt_with
  -> orientation_to_find (boolean/merge/p2.rs, the final `SM_ERR`)
```

- **The failing check:** the other operand's edge runs from vertex 0 to vertex 1. The merge
  mated vertex 0 to B-rep vertex 8, but the B-rep edge mated to the other edge runs from
  vertex 14 to vertex 15.
- **Where the mismatch came from:** every relation write was logged up to the failure.
  - Both mates were written by that pairwise merge's own `SetRelationships` (the edge
    14↔0 and the vertices 8↔0 and 15↔1), from its proto topology.
  - No vertex was glued, squeezed or combined in that merge.
  - So the intersection edge definition had been trimmed to a point off its vertex
    definition, and imprinting it made a new vertex (14) there.
- **The C++:** traced in a failing layout, it fails at the same check (`SmMerge.cpp:7071`)
  in the 16th pairwise `ManifoldBoolean` of the offset's `PiecewiseMerge`. A succeeding
  layout runs the same sequence, and that call succeeds.

1.0.1's new distance check in `SmEdgeDefinition::IsInteriorPoint` addresses exactly that
trimming: a closest point farther than the tolerance is not interior, so `UpdateEdges`
no longer trims there. The port took all of 1.0.1 at once, so whether that change alone is
sufficient was not isolated.

## Difftest

The C++/Rust comparison cases are in `fixtures/blend.fillet_executive_p3/issue_460_shell.json`:
- `real-box-fillet12-r3-shell-closed-issue-460`
- `real-box-fillet12-r3-shell-open-top-issue-460`
- `real-box-fillet4v-r3-shell-closed`
- `real-box-fillet1v-r3-shell-closed`

They are the runner's `shell` and `select` inputs of `RealSolid`, on the shim
`sc_blend_fillet_executive_p3_fillet_then_shell`.

At c9c5979 they could not run in the gate, because the native outcome changed with heap
layout and some layouts crashed the whole process. Against 1.0.1 all four pass in the
gate run's default layout. Layouts were not re-swept against the 1.0.1 port (ND-261).

## Reproduce

```sh
cargo test -p solidcore --test issue_460_open_shell_rounds_thicker_than_wall -- --nocapture
cargo run --profile difftest -p difftest --features cpp -- --unit blend.fillet_executive_p3 --filter shell
```
