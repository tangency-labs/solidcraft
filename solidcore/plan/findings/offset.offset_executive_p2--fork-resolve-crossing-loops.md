# offset.offset_executive_p2: fork feature, an offset face whose loops cross is rebuilt from the stretches that bound it (2026-10-26)

Reclassified from `plan/upstream-issues/WR-345--offset-face-loops-cross.md` (merged in #1141) by the 2026-10-26 audit: not a defect in pinned upstream c9c5979. See `plan/upstream-issues/WR-345--addendum-2026-10-26-reclassified-fork-only.md`.

- **Why fork-only:**
  - The record names no defective line. At c9c5979, `SmOffsetExecutive::DoExtendedOffset` (`SmOffsetExecutive.cpp:1062`) builds each loop from its edges' offsets and goes on to `PiecewiseMerge` (`:2236`) with no step that trims one loop against another.
  - The fix is the fork function `SmOffsetExecutive::ResolveCrossingLoops`, which pinned upstream doesn't have.
  - This is the same shape as `plan/findings/blend.fillet_solver_p5--fork-rails-fit-their-faces.md`: a missing check, added as a fork feature.
- **Confirmed:** no. It was reproduced on the SolidCraft fork build only (issue #854, gdb, and the fork's closed-form volumes listed in the record), not on the pinned C++ build.
- **Rust port:** unchanged. It is an opt-in fork feature, `Kernel::fixes.offset_resolve_crossing_loops` (default `false`). It is read in `OffsetExecutive::do_extended_offset` (`crates/solidcore/src/offset/offset_executive/p2.rs`), and the routine is in `crates/solidcore/src/offset/offset_executive/crossing_loops.rs`.
