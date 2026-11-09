# blend.blend_corner: a planar G2 Setback fill's setback patches have control nets that double back (2026-11-03)

- **Unit:** `blend.blend_corner` (the Setback's planar G2 fill, `blend_setback::fill_hole` with order 2: the fork's 870c970 fill, ported at parity; `Kernel::fixes.fillet_blend_corner`). Found while extending the net checks of #2243 (PR #2366) to every Setback fill; not changed there.
- **How found:** `check_brep`'s net test #5 (`grid_has_non_monotonic_points`) and the curved fill's net containment, evaluated on every net the planar fills accept in the tests that reach a Setback fill (36 test files and the library's tests, release).
  - Planar G1 (`graph_g1`): 678 accepted nets, all monotone and all contained.
  - Planar G2: 336 nets, all contained; 6 fail test #5.
- **Inputs:** `crates/solidcore/tests/fork_870c970_blend_setback.rs`, `box_corner_g2_rounds` (order-2 rounds r 2 at a box corner, no ease) and `box_corner_g2_eased` (circular rounds r 2 eased into the order-2 section over 2), both filled to G2 at degree 5 (n = 3).
- **Values:** in both, each of the three setback patches (5 x 5) turns back along columns 1 and 4, the columns that lie on the faces, at index 4. The turn is 174.02 degrees, between a step of 3.263e-2 and one of 5.286e-1. On patch 0, column 1 runs (2.1835, 0, 1.0), (2.2150, 0, 0.99151), (1.7217, 0, 1.18137). The doubling back is real, not rounding (the points are of size 1 to 2, the steps far above an ulp) and not #2560 (no collapsed row).
- **Elsewhere:** the box corners' G2 fills in `issue_2369_g2_setback_corner` (bar, pocket, residues bar, eased asked setback) and in the definition tests pass test #5.
- **Suspected cause:** the G2 fill's least squares (`least_squares` over the free control points, then the fold checks) bounds the conditions and the fold, not the net. Near the round's end section, the second row's point is pulled past the third.
- **Expected (#2243's rule, jyviko 13:23Z: a certified patch keeps a non-reversing control net):** the G2 fill's nets never turn back. A fix changes G2 Setback bodies that build today, so it needs its own `UpstreamFixes` flag.
- **State:** open. Not in #2366's scope (the curved G1 fill). The planar G1 certificate is left to #2361's per-patch certificate.
