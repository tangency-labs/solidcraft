# General: addendum (2026-10-31): eleven section A fork commits are already covered

Updates: plan/findings/general--smlib-fork-port-audit-2026-10-31.md

The audit counted a fork commit as covered only when its own sha was cited, in solidcore or in a PR's title, body or diff. A re-check by commit subject, against solidcore main de85a3e006 and the open PRs, finds 11 of section A's 31 commits already covered. Nine are cited on main under a rebased twin in the fork, a later commit with the same subject. Two are in open PRs that cite issue numbers and a fork PR instead of the sha.

| Section A commit | Covered by | Where |
| --- | --- | --- |
| 6ca3405 (a zero-speed curve end at a vertex where its faces touch is no kink) | fork ecf1963 | WR-668, cited in `kernel.rs` and `nurbs/b_spline_curve/p3.rs` |
| 68f6913 (`CreateInterpolatingCurve` at given parameters) | fork 0cc8cd4 | WR-600, `nurbs/b_spline_curve_n_lib.rs` and `nurbs/crv_fit/p3.rs` |
| b34d61b (a G2 blend's end rib at a tangent end) | fork cd47efd | WR-600 |
| 6d73e4f (the tangent end is an event of the fillet's trace) | fork 856b54e | WR-600, `blend/fillet_corner/topology/nx1_end.rs` |
| c290e61 (one classification of a constant round's spine) | fork b5f2867 | WR-669, #2081 |
| 344b305 (the analytic try-chain deleted) | fork 7ac037e | WR-669, #2081 |
| 808c7f6 (a plane-conic spine is an exact rational surface) | fork 0061c41 | WR-669, #2102 |
| 1866b03 (two cones touching a common ball) | fork 1b5f1e7 | WR-669, #2102 |
| f473917 (faces touching at one point, the nodal quartic) | fork dda3b5d | WR-669, open #2106 |
| 55bffe0 (a seam that collapses to a point is not classified) | open #1565 | "HasSeamProblem skips a seam that collapses to a point (opt-in, #1516)", citing the fork's PR #42 |
| 922f333 (a value just past the period's top is a period down) | open #1572 | "ContainsPeriodicValue moves a value just past the start down a period (opt-in, #1518)" |

Whether each port matches its rebased twin, or the twin matches the section A commit line for line, was not checked here.

Section A therefore has 20 commits that no port or PR covers. The record's other sections are unchanged. Future checks should match fork commits by subject as well as by sha, because the fork rebases its branches.
