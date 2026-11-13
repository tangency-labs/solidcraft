#### UB-233. `SmBSplineCurve::CreateByJoining` reads `cpOptSenses` past a shorter array
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `source/SMLib/src/SmBSplineCurve.cpp:1188` `SmBSplineCurve::CreateByJoining`: the size match is only `SM_ASSERT`ed (`:1199`); `(*cpOptSenses)[ii]` is read for every segment at `:1271`.
- **What happens:** With fewer senses than segments, `SmTArray::operator[]` reads outside the array. The release build returned a joined curve.
- **Trigger:** `CreateByJoining` with two segments and an empty senses array.
- **How found:** generated difftest case (`difftest --fuzz 2000`, `nurbs.b_spline_curve_p2` `fuzz00360`, `fuzz01002`: C++ returned a curve, Rust an error).
- **Confirmed:** no (no crash; the out-of-range read is from source inspection).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** Returns `InvalidInput`. The `nurbs.b_spline_curve_p2` runner keeps fewer senses than segments from the C++ side.
