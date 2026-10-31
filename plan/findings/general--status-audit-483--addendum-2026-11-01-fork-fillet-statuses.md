Updates: plan/findings/general--status-audit-483.md

## `fillet_corner_statuses` and `fillet_edge_curve_domain_statuses` now follow the fork (#2009 group B)

Both flags returned the error of a call SMLib ignores. The SolidCraft fork of SMLib
(tangency-labs/smlib 66b02de) fails softly at these sites instead, so SolidCraft held
the flags off. Neither was turned on by a caller (no fix is shipped), so they change
meaning under the same names:

- `fillet_corner_statuses` (`crates/solidcore/src/blend/fillet_corner/p1.rs`, `p3.rs`):
  `sm_TestG1Continuity` finds no tangency when an end tangent cannot be evaluated
  (upstream tests the unset `SM_UNDEF_DOUBLE` vectors, which are shorter than
  `SM_EFF_ZERO`, so the outcome is the same; the fork states the reason).
  `CheckAndFixDegeneracy` refines the vertex's UV point only with a computed one
  (upstream stores the unset point); the corner is still degenerate. The binormal
  evaluations, the G1 angle and `CheckNx1Degeneracy` keep upstream's handling, as the
  fork keeps their `SE`/`SER`-through-`SmBoolean`; the flag no longer changes them.
- `fillet_edge_curve_domain_statuses` (`crates/solidcore/src/blend/fillet_solver/p1.rs`,
  `check_edge_curve_domains`): a copy that cannot be trimmed is no trimmed curve
  (`None`, the edge's own curve is used); upstream returns the untrimmed copy as the
  requested domain.

Flag off is unchanged. Tests: `blend::fillet_corner::p1::tests::audit_483_corner_tangency_is_none_without_a_tangent`
(coverage: same outcome off and on), `blend::fillet_corner::p1::tests::degeneracy_fix_ignores_a_failed_uv_projection`
(off stores the unset point, on keeps the original), `blend::fillet_corner::p3::tests::nx1_degeneracy_evaluator_status_selects_the_degenerate_corner`
(coverage: same outcome), `blend::fillet_solver::p1::tests::audit_483_edge_domain_is_no_trimmed_curve_after_a_failed_trim`
(off returns the untrimmed copy, on none).

`topology_failed_evaluations_propagate` (#521) covers `SmEdgeuse`/`SmLoopuse` sites that
no group B commit touches, so it is unchanged here.
