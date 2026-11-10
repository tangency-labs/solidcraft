#### HANG-27. `SmCrvOnSurf::ApproximateCurve` of an offset-surface rail grows with the offset distance and stops returning
- **Severity:** Hang
- **Area:**
- **Location:** `SmOffsetGeometryCreation::EdgeuseOffset` (the offset-surface rail when no offset edge use is given) -> `SmCrvOnSurf::ApproximateCurve` -> `SmCrvOnSurf::GetOrCreateBSApproxPointer` -> `SmCurve::ApproximateCurve` -> `SmCurve::LocalPointSolve` -> `SmLocalSolve1d::SolveIt` -> `SmFindPCExtremaEFO::Evaluate` (gdb after 40 s).
- **What happens:** On the `Mouse.smb` shell fixture (`mouse_edgeuse_offset_shell`) the op takes 1 s at offset distance 1e4, 7 s at 1e5, 10 s at 1e6, 15 s at 1e7, 28 s at 1e8, 30 s at 1e10, 39-75 s at 1e50 to 1e200, 66 s at 1e280, and does not return within 90 s at 1e250 or 1e300 (either sign).
- **Trigger:** `EdgeuseOffset` with a huge offset distance; the `difftest --fuzz` mutation `dist = -1e300` (fuzz01796).
- **How found:** `difftest --fuzz 2000` of PR #156; gdb backtrace; replays with the distance substituted.
- **Confirmed:** yes (reference build). Not proven infinite.
- **Severity:** hang in practice.
- **Rust port:** the port builds the rail's `SmOffsetSurface` and defers at the curve on it (`UNPORTED(curve.crv_on_surf)`: the ported `CrvOnSurf` holds a B-spline surface only). The difftest shim and runner treat an `EdgeuseOffset` with `|distance| > 1e6` as a hang guard (the runner rejects a script whose Rust run reaches it, naming HANG-27; the shim keeps its own stop there, status `-6`, as a safety net). The bound is chosen for the fuzz time budget (20 s per side), not the termination boundary (about 1e250); the generator's distances (0.001 to 100) are far below it.
