#### HANG-41. `SmLine::Tessellate` runs `lMinimumNumberOfSegments` iterations and ignores failed `Add`s
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmLine.cpp:1955` `SmLine::Tessellate` (loops at `:1973`, `:2001`, `:2038`); `SmTArray<TYPE>::Add` (`source/SMLib/inc/SmTArray.h:1049`).
- **What happens:** Each requested output array gets `lMinimumNumberOfSegments + 1` entries through `Add` with no up-front size check. For a huge count the arrays grow until `SetAtGrow` fails; `Add` then returns `SM_BIG_ULONG`, which is ignored, and the loop runs on for the remaining count (up to ~1.8e19 iterations), finally returning `SM_SUCCESS` with short arrays.
- **Trigger:** `Tessellate` on a bounded line with `lMinimumNumberOfSegments = 2^63 - 1` and parameters or points requested (`curve.curve_p1` fuzz `fuzz01447`: `/minseg = 9223372036854775807`).
- **How found:** generated difftest case (fuzz sweep; the Rust port aborted on the allocation).
- **Confirmed:** no. The FFI caps the count (`MAX_SEGMENTS = 64`, `crates/solidcore-ffi/src/units/curve_curve_p1.rs`).
- **Severity:** hang (also memory exhaustion and a short result reported as success).
- **Rust port:** `Line::tessellate` reserves the `min_segments + 1` entries per requested array first and returns `InvalidInput` when that fails.
