#### UB-284. SmBrep::MakeFaceWithCurves reads past its curve and orientation arrays when the loop counts sum past the curve count
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmBrep.cpp`, in `SmBrep::MakeFaceWithCurves` (def. `:15972`). It reads `p3DCurves->GetAt(lCurveTotal+jj)` at `:16393` and `crCurveOrientations[lCurveTotal+jj]` at `:16433`. `lCurveTotal` grows by each loop's count at `:16761`. Reached from `SmApiTrimSurfaceWith3dCurves` (`source/SM_API/src/SmApiTrimmedSurfaces.cpp:43`) through `SmTrimmingTools::TrimSurfaceWithModelSpaceCurves` (`source/SMLib/src/SmTrimmingTools.cpp:87`).
- **What happens:** the only input checks are null curve arrays, no loops, and a degenerate surface (`:15995-16014`). Nothing compares the sum of the loop counts with the size of the curve array, or of the orientation array, which has the same size. A sum above the curve count reads past both arrays.
- **Trigger:** `SmApiTrimSurfaceWith3dCurves(surface, loops, curves, result)` with `sum(loops) > curves.GetSize()`, e.g. one rectangle (four lines) with loops `{4, 1}`.
- **How found:** pinned-source audit of the `primitive.api_trim_ops` FFI guard, which refuses exactly these inputs (`crates/solidcore-ffi/src/units/primitive_api_trim_ops.rs`, "unsupported loop-count input"; test `ffi_refuses_overrun`).
- **Confirmed:** by reading; not reproduced. The guard keeps the input from the C++ reference.
