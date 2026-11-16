#### UB-199. `SmApiCreatePlane` (plane overload) with a NaN length or width indexes outside the NURB knot arrays
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:181` `SmApiCreatePlane` (`SmPlane*&`): `SmExtent2d::SetMinMax(0, 0, dLength, dWidth)` keeps NaN (its `min > max` test is false), then `SmPlane::AdjustSTEPUVDomain` -> `SmPlane::MakeNurb` (`SmPlane.cpp:2993`) -> `SmBSplineSurface::CreateCanonical` -> `N_Iges128Srf` clamps to NaN bounds.
- **What happens:** The clamp's span search finds no span for NaN and reads past the knot arrays: a NaN length segfaults; a NaN width returns `SM_SUCCESS` with a plane built from out-of-bounds data.
- **Trigger:** `SmApiCreatePlane((0,0,0), NaN, 3, SmPlane*&)` (segfault); `SmApiCreatePlane((0,0,0), 3, NaN, SmPlane*&)` (success).
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures `plane_surface_len_nan`, `plane_surface_wid_nan`, removed since).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** the bounded clamp returns `InvalidInput` (`nurbs::iges::clamp`), so `create_plane_surface` fails with `SM_ERR`; the difftest guard keeps NaN sizes from the C++ side.
