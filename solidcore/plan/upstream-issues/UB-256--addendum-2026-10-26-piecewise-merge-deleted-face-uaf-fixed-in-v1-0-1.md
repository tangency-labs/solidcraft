Updates: plan/upstream-issues/UB-256--offset-piecewise-merge-deleted-face-uaf.md

#### UB-256 (`PiecewiseMerge` deleted-face UAF) (update, 2026-10-26). Fixed in v1.0.1

- **Fixed upstream in v1.0.1 (3fd5b95):** `SmOffsetExecutive::RemoveExcessFaces` now removes the deleted faces from `m_vFUToF`, `m_vEToF`, `m_vVToF` and `m_vBlendToFaces` (both directions for the blend relation) right after `RemoveFaces`. It also prunes `m_vEUToEU` of the deleted and dead edge uses. `PiecewiseMerge` therefore no longer iterates freed `SmFace` pointers (UB-258 addendum).
- **Consistent with our earlier C++ measurement:** shelling a 30x20x10 box with all twelve edges filleted at r3 crashed in 6 of 25 heap layouts at 1.0.0 and succeeds in all 25 at 1.0.1. This forgetting is one of the 1.0.1 changes on that path; the 25-layout result did not isolate which.
- **Found by:** reading the c9c5979..3fd5b95 diff during the reference pin move.
- **Confirmed:** yes, pin101 gate 2026-10-26 (HEAD 69f8b7f9d): the blend.fillet_executive_p3 shell cases (`--filter shell`) pass 11/11 against the C++ 1.0.1 reference (max_rel_err 0), and issue_460_closed_shell passes. Case tiny_tolerance_sheet_above_boundary of offset.offset_executive_p2 is not cited here; no ASan run.
- **Rust port:** follows v1.0.1 (`OffsetExecutive::remove_excess_faces`).
