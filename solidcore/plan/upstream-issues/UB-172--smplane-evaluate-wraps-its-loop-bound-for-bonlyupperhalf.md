#### UB-172. `SmPlane::Evaluate` wraps its loop bound for `bOnlyUpperHalf` with more `u` than `v` derivatives
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmPlane.cpp:1536`, `SmPlane::Evaluate`: `ULONG lHighestVIndex = bOnlyUpperHalf ? lHighestVDeriv-k : lHighestVDeriv`.
- **What happens:** For `k > lHighestVDeriv` the unsigned subtraction wraps, and the zeroing loop runs far past the caller's array. A zero-distance `SmOffsetSurface` over a plane passes such requests straight to it.
- **Trigger:** `SmPlane::Evaluate(uv, 2, 0, .., TRUE /*bOnlyUpperHalf*/, a /*3 entries*/)`.
- **How found:** read during porting (`surface.offset_surface`).
- **Confirmed:** no. The harness rejects these orders over a plane.
- **Severity:** crash/UB.
- **Rust port:** The plane evaluation returns `InvalidInput` for an upper half with more `u` than `v` derivatives.
