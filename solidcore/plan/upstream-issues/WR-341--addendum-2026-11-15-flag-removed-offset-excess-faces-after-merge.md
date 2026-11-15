Updates: WR-341

#### WR-341 (update, 2026-11-15). `offset_excess_faces_after_merge` is removed; excess faces are found after the merge only

`DoExtendedOffset` (`offset/offset_executive/p2.rs`) no longer runs `RemoveExcessFaces` on the untrimmed offset
faces before the merge; it runs it on the merged result, for any offset without `m_bCreateOffsetSolid` and for a
solid input whatever that option says. The opt-in described in the original record no longer exists. Removed: the
field, SolidCraft's preset line, its `fixes.json` entry, the before-merge call, and the flag settings in the tests.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** 4 `offset.offset_executive_p2` rows move, each only in `input.pure` (`[0, 1]` to `[1, 1]`):
  `extended_sheet_direct_independent_flags`, `gen0015`, `gen0139`, `gen0194`. Measured on main fb5344be5 against
  this change: the before-merge pass changed the caller's source body (its `Debug` state after the call differs from
  before), and without it the source is left exactly as it came in; the result's vertex points, edge curve samples,
  tolerances and face kinds are identical. An offset must not change its input, so `[1, 1]` is the right value.
- **Tests:** `issue_977_closed_shell_excess_faces_after_merge` asserts the closed shell is a manifold solid on the
  default kernel; its flag-off half is removed. `issue_865_whole_offset_of_holed_plate.rs` runs on the default
  kernel, with and without the crossing-loops opt-in.
