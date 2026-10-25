#### WR-96. `SmCurveLoops::GetLoopStartEnd` starts a later loop at the previous loop's last curve
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:4127` (`rlStart = rlEnd;`).
- **What happens:** `rlEnd` is the index of the last curve of the loop just passed, so `rlStart` of the next loop should be `rlEnd + 1`; it is one less. For loops of 3 and 4 curves, curve 4 gets the range `[2, 6]`. `FindAdjacentLoopCurve` then steps from the first curve of a later loop to its predecessor in the previous loop instead of wrapping to the loop's last curve (and a first loop of zero curves wraps `rlEnd` below zero).
- **Trigger:** `GetLoopStartEnd` / `FindAdjacentLoopCurve` / `SampleNearbyPoints` on any loop after the first.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** yes (fixture `loops_start_end_second_loop`).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code (`CurveLoops::loop_start_end`).
