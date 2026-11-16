#### Updates: WR-259 (2026-10-26). The midpoint test becomes an `UpstreamFixes` field, off in the Boolean too

- **Change (decision on tangency-labs/solidcore#959, following #1077):**
  `Kernel::cone_parallel_midpoint_test`, which `boolean::merge::p4::manifold_boolean` set
  for the duration of every Boolean, is replaced by the opt-in
  `Kernel::fixes.cone_parallel_line_midpoint` (`UpstreamFixes`, off by default). The
  Boolean no longer sets it, so by default a Boolean tests the parallel-cylinder line at
  its bottom end, as upstream does, and the WR-259 / #711 cuts fail with `SM_ERR` as in
  the C++.
- **Kind:** a result-changing fix (upstream returns a wrong result or `SM_ERR`; no crash,
  undefined behaviour, hang or nondeterminism), so it follows the opt-in rule.
- **Test:** `crates/solidcore/tests/issue_711_overlapping_holes.rs`,
  `issue_711_second_overlapping_hole_cuts`, now sets the field.
- **Callers** that relied on the Boolean's previous behaviour (e.g. SolidCraft's
  FAIL-3665 cases) must set the field.
