#### UB-229. `SmSurfOfRevolution::CreateOffsetSurface` dereferences a failed axis segment
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmSurfOfRevolution.cpp:1413` (`SmLine::CreateLineSegment(..., origin, origin + dHeight * zAxis, pAxisLine)`, status ignored) and `:1417` (`*pAxisLine` passed to `GlobalCurveIntersect`) in `SmSurfOfRevolution::CreateOffsetSurface` (def. `:1244`).
- **What happens:** `dHeight` is 1.5 times the offset generator's box size. With an axis point near `2^63`, `origin + dHeight * zAxis == origin`, the segment is degenerate, `CreateLineSegment` fails and the NULL line is dereferenced.
- **Trigger:** `CreateOffsetSurface` of a revolution whose axis point has a coordinate `9223372036854775807` or `-9223372036854775808` (generator of unit size).
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.surf_of_revolution_p2` `fuzz01805`, `fuzz01926`).
- **Confirmed:** yes.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns the segment error. The `surface.surf_of_revolution_p2` FFI rejects `Offset` with an axis point coordinate above `MAX_AXIS_POINT = 1e12` (a heuristic bound; the exact threshold depends on the generator size).
