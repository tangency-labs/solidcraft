# general: addendum (2026-10-31): 62762b8 documented, not ported

Updates: plan/findings/general--smlib-fork-port-audit-2026-10-31.md (section D partials)

- **62762b8:** curve-level difference on tangent parallel cylinders (the fork's line extends past the other face's trim by half its offset), with no difference in the fork's own disc test or its tangent variant. Not ported. Fork rule re-implemented, not a fork run.
  - **Where:** `Cone::intersect_with_cone` (`crates/solidcore/src/surface/cone/p3.rs`) with `cone_parallel_line_domains` (WR-259, #711). The fork's hunk writes the other face's trimmed bottom, projected, into `dParam`, which then sets the first circle's centre (`sStart`) and the line shift (`dMin - dParam`). Ours keeps the other cylinder's own bottom for both.
  - **Evidence (probe batch 54g5, SolidCraft's preset, ours against ours with the fork's `dParam`):**
    - L3, the fork's `capi/tests/ops_kernel.cpp` disc (r 20 x 10 minus a hole r 5 at (6, 0), then one at (3, 3 sqrt 3)): crossing 7 faces / 15 edges / 10 vertices, 0 Level-2 failures, volume rel 3.7e-12; tangent variant (the second hole at (-4, 0)) 6 / 11 / 6, 0 failures, rel 7.9e-12. Identical in both runs.
    - L1, two parallel r 5 cylinders of height 12, axes 10 apart (tangent), face v ranges cut: the other face from v 1, ours z 1..12, the fork's 0.5..11.5 (the reverse order, the other face whole: identical); both cut (2..10 and 1..11), ours 2..10 in both orders, the fork's 1.5..9.5 and 1..9; the other face cut at the top only: identical. Crossing (axes 6 apart): identical bits.
