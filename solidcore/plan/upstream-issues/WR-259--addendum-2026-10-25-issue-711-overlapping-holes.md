#### Updates: WR-259 (2026-10-25). Issue #711, a second hole overlapping a first

- **Same defect.** tangency-labs/solidcore#711 reports `SmCone::IntersectWithCone`'s
  parallel-cylinder branch dropping both crossing lines when a second r 5 hole is cut 6 mm
  from a first one in an r 20 × 10 disc. The first hole's wall is trimmed to the disc
  (v 1..11 of the 0..12 tool), and each line is tested at the tool's bottom (v = 0). This is
  WR-259's trigger with a different part.
- **Rust port:** already covered by WR-259's opt-in. `Kernel::cone_parallel_midpoint_test`
  tests the line at its middle (v = 6, inside both domains), and
  `boolean::merge::p4::manifold_boolean` sets it for the Boolean's duration. Off by
  default, so `surface.cone_p3` still matches the C++ bit for bit.
- **Difference from the fork's fix** (tangency-labs/smlib@7edbf5a): the fork clips the
  line span to both faces' axial domains (converted to STEP v), and drops faces that are
  disjoint along the axis. The midpoint test keeps the line's full span and still drops a
  line that overlaps both domains only away from its middle (WR-259's remaining gap). Clipping
  would close that and is not ported.
- **Test:** `crates/solidcore/tests/issue_711_overlapping_holes.rs`,
  `issue_711_second_overlapping_hole_cuts`. It runs the issue's two cuts and checks the
  volume against the analytic value (disc less the union of the two r 5 discs).
- Fixes tangency-labs/solidcore#711.
