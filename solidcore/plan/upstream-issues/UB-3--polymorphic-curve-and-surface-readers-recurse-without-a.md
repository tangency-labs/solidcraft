#### UB-3. Polymorphic curve and surface readers recurse without a depth limit [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `SmCurve.cpp:23709` `SmCurve::ReadFromDB` dispatching to `SmProjectedCurve.cpp:690` `SmProjectedCurve::ReadFromDB` (and to the offset and iso curve readers, e.g. `SmOffsetCurve.cpp:567` `SmOffsetCurve::ReadFromDB`), which read their base curve through `SmCurve::ReadFromDB` again; likewise `SmSurface.cpp:34641` `SmSurface::ReadFromDB` and `SmOffsetSurface.cpp:2197` `SmOffsetSurface::ReadFromDB` for surfaces.
- **What happens:** Each nesting level costs a few bytes of input and one stack frame chain, so a crafted file overflows the stack.
- **Trigger:** A file of N nested projected-curve records with N large (thousands). Two concrete triggers (reader fuzz, 2026-10-20): `Fillet/eg1_srf2.smb` with its first 3D curve header preceded by 200000 copies of `Object Type 14036` / `Curve Dim 3` / `0 0 1` / `0.1` / `0 1` / `0` / `//base` (offset curves), or its surface header preceded by 200000 copies of `Object Type 15400` / `0.1` / `0` / `0` / `0` / `0` / `//base` (offset surfaces).
- **How found:** read during porting (commit security review of `curve.projected_curve`); confirmed by the `.smb` reader fuzz (`crates/difftest/examples/reader_fuzz.rs`).
- **Confirmed:** yes. `SmBrepData::ReadFromFile` (ASCII) on both 200000-level files segfaults (SIGSEGV, stack overflow) in the C++ reference build, with an 8 MiB stack; 200 levels read fine. The Rust port returns `InvalidInput` for both.
- **Severity:** crash/UB (stack exhaustion).
- **Rust port:** Limits nesting to 32 levels (`DatabaseReader::enter_nested`; 64 before the wasm32 stack review) and returns `InvalidInput` for deeper input.
