#### UB-63. `SmCurveBoundedSurface::operator==` reads past the UV boundary array
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmCurveBoundedSurface.cpp:122`, `SmCurveBoundedSurface::operator==` (def. `:83`).
- **What happens:** The UV loop runs to the number of 3D boundaries, so `m_vUVBoundaries[i]` is read out of bounds when there are more 3D boundaries than UV boundaries.
- **Trigger:** Two equal surfaces with 2 3D boundaries and 1 UV boundary.
- **How found:** crash in the C++ reference run (difftest case `drop_curve_eq_other_same`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Stops at the end of the UV array.
