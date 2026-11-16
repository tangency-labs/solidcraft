Updates: WR-400

#### WR-400 addendum (2026-10-26): a latent status-handling defect, low severity

- **What is in pinned upstream:** at c9c5979, `SweepProfilesAlongPath` is a `void` helper (`source/SMLib/src/SmPrimitiveCreation.cpp:5640`) that sets `sOptions.bCapEndsArg = TRUE` for every profile (`:5665`). It drops the `SM_ERR` that `CreateCurveSweep` returns when an open profile cannot be capped.
- **Why the severity is low:** unmodified upstream still produces the right result here. The planar sweep's final `StitchFaces` stitches the walls, so the open-profile sheet sweep works, by accident. The defect matters only once the error is passed on, as the SolidCraft fork does. Then the documented open-profile sweep fails with `SM_ERR` (1001).
- **Confirmed:** no (read at c9c5979 `source/SMLib/src/SmPrimitiveCreation.cpp:5640` and `:5665`; reproduced on the SolidCraft fork's 1.0.1 replay, gdb trace of issue #837). Unmodified upstream gives no wrong result.
- **Rust port:** unchanged. With the default, the port ignores the status as upstream does. The opt-in `Kernel::fixes.planar_path_uncapped_open_profiles` asks for caps only when the profile closes.
