#### WR-159. `SmVector3d::Unitize` turns a vector with an overflowing squared length into a zero vector
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `source/SMLib/inc/SmVector3d.h:537-566`, `SmVector3d::Unitize`; reached from `SmAxis2Placement::SetCanonical` (`SmAxis2Placement.cpp:70-97`).
- **What happens:** The degenerate test is only `dLenSq < SM_EFF_ZERO_SQ`. A component of about 1.34e154 or more makes `LengthSquared()` infinite, so the vector passes the test and is divided by an infinite length. Finite components become 0 and the result is a silent zero vector (with a negative-zero component), not the documented `(0,0,1)` default, and the status is success.
- **Trigger:** An axis placement whose Y axis is `(0, -1e300, 1)`, unitized by `SetCanonical`, used in a cone/cone intersection (`SmCone::IntersectWithCone`). A component of 1e150 still unitizes correctly; 1e160 does not.
- **How found:** differential fuzzing of `surface.cone_p3` (`fuzz01093`).
- **Confirmed:** yes (C++ returns two lines with a zero Y axis: the polar conversion of that cone then fails, `SmEllipse.cpp:596`, and `IntersectWithCone` ignores that status, `SmCone.cpp:2966-2967`).
- **Severity:** wrong result.
- **Rust port:** `Vec3::unitize` is faithful and has the same overflow; such inputs agree with C++ (`surface.cone_p3` fixtures `fuzz_axis_overflow_zero_y*`).
