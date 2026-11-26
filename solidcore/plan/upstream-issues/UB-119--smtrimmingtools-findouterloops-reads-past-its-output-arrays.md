#### UB-119. `SmTrimmingTools::FindOuterLoops` reads past its output arrays for a zero-area loop
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:2275` (`if(sLoopAreas[ii] <= 0.0) continue;`), `:2426`-`2427` (`rsInLoopCounts[0] = sOutLoops[0]`, `sOutLoops[ii] - sOutLoops[ii-1]`).
- **What happens:** A loop whose tessellation encloses zero area has `sLoopAreas[ii] == 0` and is skipped as if already processed when the next outer loop is picked (`<= 0.0`). The inner-loop pass skips only `< 0`, so a zero-area loop inside an outer loop is still classified as its inner loop (no UB); one that no outer loop contains is never classified, so `sOutLoops` gets fewer entries than there are loops (none at all when every loop has zero area). The translation back to counts still reads `sOutLoops[0 .. lNumLoops-1]`, past its size (inside its reserved capacity: uninitialised values), and writes them to `rsInLoopCounts`; the curves of the dropped loop vanish from `rsInCurves`.
- **Trigger:** `FindOuterLoops` on a loop of collinear curves (e.g. `(0,0)-(2,0)`, `(2,0)-(1,0)`, `(1,0)-(0,0)`), alone or beside (not inside) a real loop.
- **How found:** adversarial / edge-case fixture `outer_zero_area_loop` (unit `topology.trimming_tools`).
- **Confirmed:** no (the shim sees the dropped curves and reports the call instead of the garbage counts).
- **Severity:** UB (uninitialised read).
- **Rust port:** `topology::trimming_tools::find_outer_loops` returns `SolidError::InvalidInput`.
