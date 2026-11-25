Updates: WR-87

#### WR-87 (update, 2026-11-14). `check_seam_crossing_unclosed` is removed; no seam is crossed on an unclosed surface

`check_seam_crossing` (`topology/face/p2.rs`) always returns `false` for a surface closed in neither direction:
nothing can be crossed, and the function leaves `uv1` and the shift unchanged there in both versions. The opt-in
described in the 2026-10-31 addendum no longer exists. Removed: the field, SolidCraft's preset line, its entries in
`kernel_cases/fixes.json` and the `issue_2260_gn_pipe_refined_range_end.rs` flag list, and the flag-off branch. The
function's kernel argument, which only read the flag, is dropped.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Effect on results:** the one library caller (`LoopOrientation`'s sampler in `topology/face/p3.rs`) ignores the
  return value, so no operation's result changes. Only the function's own return value does.
- **Goldens:** 6 `topology.face_p2` `SeamCross` rows move (seam_neither, extra00310, extra00470, extra00769, extra01197, extra01295): each is a surface closed in neither direction, and only the returned flag moves, 1 to 0; `uv1` and the shift are the same in every row.
- **Tests:** `seam_crossing` asserts `false` for the unclosed case; `seam_crossing_unclosed_returns_false` runs on the
  default kernel.
