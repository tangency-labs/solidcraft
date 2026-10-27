# Join a flush contact with a small face overlap when the volume guard refuses

## Summary

Combine Join of two solids that only touch was refused after the kernel separated coincident faces by a shift scaled to the whole body (about 8.4 mm on a 367 mm frame). The guard in `boolean_guard::check_retry` rejects that solid: its volume is heavier than the two volumes added by more than `2e-4 * (|target| + |tool|)`. The error text names that separation. It is the attempt that already failed, so running it again does not help.

Join now retries that one refusal in the document layer. Only the tool faces that lie back-to-back on a target face are moved, outward, by 0.05 mm, then 0.2 mm, then 0.5 mm (`kernel::offset_faces`). The union is kept when it is valid and its volume is the flush sum (original target plus original tool) within that same slack. The overlap sits inside the target, so the outer shape is the flush join. A Move of the whole tool is not required, and it is a different result: moving this rail 0.5 mm drops its far face from y = 9.8 to y = 9.3 and loses about 851 mm³.

`crates/kernel` is unchanged (#3992). The body-scaled separation still produces the heavy solid; this path does not call it a second time with the same depth.

## Call sites

- `crates/doc/src/flush_join.rs` — `join_bodies`.
- `crates/doc/src/eval.rs` — `Operation::Join` in `apply_traced` (Combine, and every other traced join that used this closure).
- The same helper joins a mirrored copy when the mirror feature combines it.

Other booleans stay on `kernel::boolean`. Cut, intersect, and the union that builds an intersection tool are unchanged. A join the kernel already accepts is returned as it is; the overlap runs only when the error text contains both `could not be computed reliably` and `coincident-face separation`.

## Tests

`cargo test -p solidcraft-doc --lib flush_join`

- `a_flush_rail_joins_a_drilled_wall_without_moving_the_rail` — the wall-and-rail pair above. Direct `boolean` errors. `join_bodies` returns one valid lump whose volume is within the guard slack of the sum, and whose tessellation still reaches y = 9.8 (a whole-body move of 0.5 mm would not).
- `an_ordinary_overlap_is_joined_as_it_is` — two boxes that already overlap join to 1500 mm³. The retry is not used.

## Out of scope

Making coincident-face separation itself volume-exact is kernel work (solidcore, #3992), shared with #5936, #5353 and #5413. This change does not edit `boolean_guard`, `coplanar`, or `ops`.

## Handoff

`UPSTREAM_ISSUE_DRAFT.md` is the issue text. These two files are the handoff, not product documentation. Remove them from the branch before opening an upstream pull request so the diff is only the join retry. Root `*.md` files do not by themselves select the full CI suite (`scripts/ci/affected.py` skips a markdown file in the repository root).
