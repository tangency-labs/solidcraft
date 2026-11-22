# general: addendum (2026-10-31): three partial ports of fork fixes with no result difference shown (709c5be, cc4289d, 242ca97)

Updates: plan/findings/general--smlib-fork-port-audit-2026-10-31.md (section D partials; see also plan/findings/general--fork-fixes-done-differently-7a6b4ad-f72c79b-69d8857-7a8f4d3.md)

The same bit-for-bit check also found three fork commits that solidcore's flags port only in part. Unlike the four in that finding, these flags port the fork's own fix, with a difference in one condition or step. Probe batch 53i ran each of them. For all three the fork's rule is re-implemented in the probe (not a fork run), and no result of ours differs. They stay as they are, recorded here in case an input turns up that reaches the difference. The two follow-ups the same batch found, 5df5b27 (WR-385) and c4209ab (WR-471), are being made exact in their own PRs.

- **709c5be** (the perspective cull projects each sphere's own radius from its centre) vs `perspective_sphere_offsets` (WR-608, #525), `geom/global_solver.rs` (`max_projected_radius_sq`):
  - Same: the offsets, relative to each centre with each sphere's own radius, in the order ±minor then ±major.
  - Different: the axes. The fork builds them with `MakeUnitOrthoVectors` on the unit eye vector (an arbitrary perpendicular pair when `eye × normal` vanishes). Ours keeps upstream's `unit(eye × n)` and `unit(dir1 × eye)` on the unnormalised eye vector, and falls back to Z when degenerate.
  - 53i: at the root of the probe's solve, the cull decision differs (ours: projected distance 0.714286 − 0.001 − 0.143367 = 0.569919, culled; the fork's rule: 0.714286 − 1.999 − 0.143367 = −1.428081, not culled). Ours still returns the one real solution, so no result differs on that input.
- **cc4289d** (the B-spline curve check reads the minimum radius of curvature) vs `bspline_check_min_radius` (WR-382, #475), `nurbs/b_spline_curve/p3.rs`:
  - Different: the fork takes the least solution value over all the `MINIMIZE_RADIUS_OF_CURVATURE` solutions. Ours reads the first.
  - 53i: seven curves (`d_b` = 1 down to 1e-6). The solutions come back in ascending order to rounding: on `d_b = 1` the first is 0.36545866771307506 and the least 0.3654586677130741 (1e-15 apart), and on the other six the first is the least. Test 14 fires the same way under both rules on all seven (no for 1, 1e-1, 1e-2; yes for 1e-3 to 1e-6).
- **242ca97** (a slice keeps a face coincident with its sheet only on the side kept) vs `slice_coincident_faces_sheet_side` (WR-254, #528), `boolean/merge/p4.rs`:
  - Same: the keep rule, at both sites.
  - Different: the condition. The fork applies it to every coincident pair in a slice. Ours applies it only when the primary face bounds material on exactly one side, and otherwise falls back to the difference rule.
  - 53i: a lamina face sliced by a coincident sheet, both sheet orientations. Ours gives an empty result both ways (0 faces). The fork's rule would keep the lamina face for exactly one orientation. The probe did not show that our run reaches the condition, so this is a prediction, not a measured difference.
- **Status.** All three flags are in SolidCraft's preset (#8526 core.rs 0b627d63). No change. A case showing a result difference makes it a follow-up: the flag's body reworked to the fork's rule, keeping its name, with a WR addendum citing the sha.
