#### Updates: WR-259 (2026-10-27). Opt-in clipping to both requested axial domains

- **Reference recheck:** NVIDIA SMLib v1.0.1 (`3fd5b95`) still uses whole cylinders
  in `source/SMLib/src/SmCone.cpp:2796-2811`, `SmCone::IntersectWithCone`. The
  midpoint fix already in Rust closes #711's original Boolean reproduction but
  retains the gap identified in the 2026-10-25 addendum: untrimmed lines and missing
  overlaps away from their middle.
- **Rust port:** independently opt-in `Kernel::fixes.cone_parallel_line_domains`
  converts both requested NURBS domains to STEP and clips the line span, preserving
  the existing tolerance branches and reversed-axis projection. Off by default,
  so difftest still compares the pinned C++ behavior. The separate midpoint option
  keeps its existing behavior. This follows the domain-clipping patch supplied in
  https://github.com/tangency-labs/solidcore/issues/711#issuecomment-6383839493.
- **Evidence:** five `issue711_*` analytic regressions fail on current main even
  with its midpoint option, and exercise both trims, touching/disjoint domains,
  tangency, swapped UV, inside-out surfaces, antiparallel axes and a nonzero STEP
  origin. The `issue_711_axial_domain_fix_cuts_below_the_tools_midpoint` Boolean
  regression exercises a plate below the tool's midpoint and checks its analytic
  volume. Default-reference fixtures remain in
  `fixtures/surface.cone_p3/issue711.json`; no known-mismatch waiver is introduced.
- **Details:** `plan/findings/surface.cone_p3--requested-axial-domains-735.md`, PR #735.
  The earlier unmerged WR-258 label in that PR is superseded by this existing ID.
