#### WR-638. A cliff overflow that cannot be built is reported as having no room
- **Severity:** Wrong result (error report)
- **Area:** Blend (fillet executive)
- **Location:** the fork's `source/SMLib/src/SmFilletExecutive.cpp`, `SmFilletExecutive::DoFilleting` and `SmFilletCliffOverflow.cpp`, `NoteUnbuiltOverflow` (tangency-labs/smlib@8ba4667).
- **What happens:** when the construction of a cliff overflow (WR-631) fails, 8ba4667 rewrote whatever the failing step noted into `SM_FILERR_NO_ROOM` with the generic "larger than the faces" comment (`NoteUnbuiltOverflow`), unless a continuation was not exact. A caller fitting the radius could not tell a removal region that did not close (`SM_FILERR_BAD_MERGE`), or a step that found no room for its own reason, from a blend too large for its faces.
- **Trigger:** a round on a box's top edge past its 10 high side whose end face is a concave cylinder: the open end cannot be cut by a plane.
- **How found:** SolidCraft (#1071, #960). The fork removes `NoteUnbuiltOverflow` in tangency-labs/smlib@ae2a6de ("a cliff overflow that fails reports its own error"): `CreateFilletBrep` and `SubtractRemovalRegion` failures keep the error their step noted.
- **Confirmed:** no C++ run of the fork was made; not reachable in the pinned C++ (`3fd5b95`), which has no cliff overflow.
- **Rust port:** follows ae2a6de with `Kernel::fixes.fillet_cliff_overflow` (the session's `DoFilleting` passes the subtraction's error on, `blend/fillet_executive/session/finish.rs`): `NoRoom` with the step's own comment and `NoRoomReason`, `NotExact`, or `BadMerge`.

  Tests: `crates/solidcore/tests/wr631_cliff_overflow_construction.rs` (`an_overflow_the_construction_cannot_cut_reports_its_own_error`).
