Updates: WR-96

#### WR-96 (update). Opt-in correction of later loop starts

- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:4134`, `SmCurveLoops::GetLoopStartEnd`: `rlStart = rlEnd` copies an inclusive end instead of advancing past it. The pinned source retains this defect; the original record's `loops_start_end_second_loop` fixture confirms the C++ result.
- **Rust port:** `Kernel::fixes.curve_loop_start_after_previous_end` (off by default) advances the start to the previous end plus one. `CurveLoops::loop_start_end` and `find_adjacent_loop_curve` now take `&Kernel`; `sample_nearby_points` forwards its kernel so adjacency and sampling use the same opt-in.
- **Regression:** `crates/solidcore/tests/wr96_curve_loop_start.rs` checks first, second and singleton third loops, out-of-range indices, default C++ ranges, and both adjacency directions and endpoint sampling on the second loop. Before the formula change, the enabled range regression failed with `(2, 6)` instead of `(3, 6)` for counts `[3, 4, 1]`.
- **Scope:** only the later-loop start assignment changes when enabled. Empty-loop count underflow and upstream's ignored-status behavior remain as before. Default difftest inputs and tolerances are unchanged.
