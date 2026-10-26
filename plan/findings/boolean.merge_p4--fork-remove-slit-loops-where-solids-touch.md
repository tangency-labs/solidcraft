# boolean.merge_p4: fork feature, slit loops where solids touch are removed after a Boolean (2026-10-26)

Reclassified from `plan/upstream-issues/WR-360--smmerge-manifoldboolean-leaves-a-slit-loop-where-solids-touch.md` (merged in #1077) by the 2026-10-26 audit: not a defect in pinned upstream c9c5979. See `plan/upstream-issues/WR-360--addendum-2026-10-26-reclassified-fork-only.md`.

- **Why fork-only:**
  - The slit loop only fails SMLib's check after the fork's own trim fix (WR-361, issue #488, tangency-labs/smlib@ad24a8efb). Pinned upstream gives the new edge uses no trims, so `SmTrimmingTools::CheckFace` skips the face and the result passes its check.
  - The fix is the fork's `SmBrep::RemoveSlitLoopsWhereSolidsTouch` and `SmBrep::SplitDisconnectedShells`, which pinned upstream doesn't have.
  - The record's location, `SmMerge.cpp:5051`-`5054`, does not match c9c5979. There, `SmMerge::ManifoldBoolean` starts at `:2510` and sets `rpResult = m_vTI.m_pBrep` at `:3118`.
- **Confirmed:** no. It was reported from the fork (issue #746, capi `tangent_cylinders` seed 36) and not reproduced on the pinned C++ build.
- **Rust port:** unchanged. It is an opt-in fork feature, `Kernel::fixes.remove_slit_loops_where_solids_touch` (default `false`). The code is `remove_slit_loops_where_solids_touch` and `split_disconnected_shells` in `crates/solidcore/src/topology/brep/p3.rs`, and the test is `crates/solidcore/tests/issue_746_slit_loops.rs`.
