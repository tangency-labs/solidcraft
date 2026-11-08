#### UB-78. `SmApiCreateSkinFromFaces` with no faces crashes
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:1335` `SmApiCreateSkinFromFaces` -> `source/SMLib/src/SmPrimitiveCreation.cpp:6711` `SmPrimitiveCreation::CreateSkinFromFaces` -> `SmBSplineSurfaceNLib.cpp` `SmBSplineSurface::CreateSkinnedSurface`.
- **What happens:** With an empty face array (and no curves, which the API always passes), `CreateSkinFromFaces` still runs one layer and calls `CreateSkinnedSurface` with zero cross sections, which has no check for that and segfaults.
- **Trigger:** `SmApiCreateSkinFromFaces` with an empty `rFaces`.
- **How found:** crash in the C++ reference run (`sweep.api_surfaces` probe case).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** `primitive.primitive_creation_p3` (PR #233) implements `CreateSkinFromFaces`; its `BSplineSurface::create_skinned_surface` call returns `InvalidInput` for no cross sections. The p3 native shim and checked FFI reject that exact case before C++; a direct probe of the linked reference reconfirmed SIGSEGV. The legacy API remains blocked earlier at B-rep construction.
