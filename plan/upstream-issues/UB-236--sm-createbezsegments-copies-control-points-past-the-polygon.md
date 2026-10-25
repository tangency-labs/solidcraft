#### UB-236. `sm_CreateBezSegments` copies control points past the polygon of a curve whose end knots are not clamped
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `source/SMLib/src/SmNurbsCrv.cpp:478` `sm_CreateBezSegments` (end-of-span loop `while( ie LT m AND UP[ie] EQ UP[ie+1] ) ie++` at `:551`, copy `InitCP(Pw[ie-p+i],&NQw[i])` at `:596`). Reached from `SmCurveCache.cpp:1291` (the curve cache `DropPoint` builds) during `SmBrepConstructor::EndLoop`.
- **What happens:** The span walk assumes `p + 1` equal end knots. With an unclamped end, `ie` stops short of the clamp and the next-segment copy reads `Pw[ie - p + i]` past the last control point.
- **Trigger:** An edge curve of a loop being built whose last `p + 1` knots are not all equal (`query.loop` fuzz: one tail knot of a 3D edge B-spline set to `0.0`, `1.0` or `-0.0`).
- **How found:** crash in the C++ reference run (`difftest --unit query.loop --fuzz 2000`: `fuzz-out/n2000/query.loop/fuzz00183.json`, `fuzz00293.json`, `fuzz00592.json`).
- **Confirmed:** yes (SIGSEGV, 3 cases).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** not reached; the `query.loop` FFI wrapper rejects an edge curve whose last `p + 1` knots differ (`crates/solidcore-ffi/src/units/query_loop.rs:400`).
