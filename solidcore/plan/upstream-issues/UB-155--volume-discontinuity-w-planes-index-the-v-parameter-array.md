#### UB-155. Volume discontinuity W planes index the V parameter array
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10071-10072`, `SmCurveClassification::Intersect3DDiscontinuities` (volume).
- **What happens:** The W-plane loop takes its coordinates from `m_sParamsV[kk]` while iterating `m_sParamsW`. Different V/W values put the planes at the wrong W positions; more W than V parameters reads past V's array.
- **Trigger:** unequal V/W discontinuity parameter arrays.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no; native fixtures use equal V/W lists and never execute the out-of-bounds case.
- **Severity:** crash/UB; wrong result for in-bounds unequal values.
- **Rust port:** retains V coordinates, with checked indexing returning an error. The plane-intersection path remains dependency-deferred.
