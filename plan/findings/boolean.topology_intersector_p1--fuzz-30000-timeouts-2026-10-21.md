# boolean.topology_intersector_p1/_p2: `--fuzz 30000` (2026-10-21)

- Hangs on surface control points of `1e154` or more: new triggers of HANG-29 (`plan/upstream-issues/HANG-29--addendum-2026-10-26-fuzz-30000-triggers.md`; also in `heal.gap`, `heal.heal_data_p2` and `blend.fillet_geom_p2`); the FFIs refuse them.
- Slow, not hung: `fuzz09748` (p1) and `fuzz15421` (p2) set `/approx = 1e300`. Replayed with `approx` from `1e3` to `1e300`, the C++ returns in 22-25 s every time (`1`: 0.5 s), just over the fuzz harness's fixed 20 s budget (`CASE_TIMEOUT`). These are valid upstream inputs, so the FFI accepts them; they stay `timeout` verdicts in the fuzz until the harness has a per-unit budget.
- Also slow, not hung: p2 `fuzz07961` and `fuzz29932` (`/approx = -1e+300`, accepted as upstream accepts negative tolerances): the C++ returns in 12.5 s alone, past the fuzz budget only under load.
- 5 `differs` in p2 (`fuzz06628`, `fuzz08367`, `fuzz19033`, `fuzz19955`, `fuzz23628`): a squeeze whose vertex was related to one of its own B-rep; fixed in the shim by #360.
- Found by raykholo's PR #358, stacked onto main.
