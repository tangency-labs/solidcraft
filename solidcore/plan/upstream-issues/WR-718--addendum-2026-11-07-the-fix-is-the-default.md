Updates: WR-718

#### WR-718 (update, 2026-11-07). The fix is the default; its flag is retired

`Kernel::fixes.shell_one_construction` is gone and S1's path is the only one:
- `OffsetExecutive::replace_implicit_offsets` (`crates/solidcore/src/offset/offset_executive/p1.rs`)
  replaces every implicit offset by `certified_offset`, a bicubic B-spline within the shell's
  tolerance of the exact offset at every parameter. Its `N_SrfOffset` branch is deleted.
  `turn_to_nurbs` (`topology/brep/p3.rs`) still fits `N_SrfOffset`; that is not this record's path.
- `offset_face_prefix` (`offset_executive/p2.rs`) refuses a face whose offset is not certified
  regular on its parameter box for every offset but the local operation, after the collapse test.
  The refusal is `OffsetFailure::Folds { face, distance, largest }` (a `SolidError::Failed`, status
  `SM_ERR`; `Error::WallFolds` in ops). The record's `SolidError::ShellRefused` /
  `ShellRefusal::Fold` are no longer in the code.

The record's trigger, the parabolic sheet thickened 11 towards its centre, is refused naming 10 on
the default kernel, one span and two; thickened 2 both ways and 5 towards the centre it builds
within the shell's tolerance of the tube formula
(`crates/solidcore/tests/issue_2350_shell_offset_construction.rs`, now on `Kernel::default()`).
The test of the flag's off path is deleted with the path.
