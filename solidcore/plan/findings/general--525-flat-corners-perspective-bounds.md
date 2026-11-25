# General: opt-in flat corners and perspective bounds (2026-10-26, PR #684)

Units: `surface.surface_p7`, `geom.global_solver`, `curve.curve_p9`;
WR-55 / WR-384 (flat corners), WR-608 (perspective bounds); #525.

Main already fixes flat-corner point requests behind default-off
`Kernel::fixes.flat_corners_test_bits`. This PR retains that algorithm and adds
two opt-in regressions: a bilinear net whose only flat corner moves through each
bit when its parameter directions reverse, and a collinear net with all four
bits. Optional points must preserve the mask and evaluate only classified corners;
regular plane corners stay undefined. Ten saved fixtures (five masks, with and without points) compare default behavior
against C++, which returns mask zero when points are requested.

The new default-off `Kernel::fixes.perspective_sphere_offsets` makes each sphere
use its own radius and center, and passes radius vectors to the vector projection
API, which already adds its base point. The four sampling directions, tolerance,
iteration and comparison order are unchanged. This does not make the inherited
four-sample approximation a general exact projected-sphere bound. The policy is
copied into curve global solvers; standalone solvers expose their own `fixes`.

Small reproduction: eye (0,0,0), plane z=1, point (1.75,0,2), and box segment
[(0,0,2),(2,0,2)]. The point lies inside the box and the projections overlap at
x=.875. Pinned C++ culls the pair; the opt-in keeps it. The direct regression
asserts both policies and checks swapped operands, coincident points and separated
points. Eight saved perspective fixtures retain default-policy comparisons, for eighteen
new fixtures in total.

The production `curve.curve_p9/gen0415` case (seed 7577090626449490212) gives no
C++ solutions. The opt-in regression reconstructs it, verifies that a solution
exists, checks both parameter domains, and perspective-projects its evaluated
curve points to verify the caller's unchanged 0.01 tolerance.

Earlier native runs confirmed the reference defects on these exact inputs:

| Case | Pinned C++ | Corrected opt-in behavior |
|---|---|---|
| `flat_mask_1_points_1` | mask 0 | mask 1 |
| `flat_mask_all_points_1` | mask 0 | mask 15 |
| `perspective_unequal_containing_swap_0` | no leaf solution | one leaf solution |
| `curve.curve_p9/gen0415` | no curve solution | one curve intersection |

No new known mismatch or status downgrade is needed: ordinary differential runners
use default fixes. No comparison is dropped or tolerance widened. The new issue
record uses WR-608: WR-260, WR-445 and WR-454 were taken on main, and the
intermediate WR-589 collides with open PR #1828. WR-608 is the next number after
the highest actual record on main and across all open PR files checked on
2026-10-31 (including the reserved WR-604 maintainer follow-up).
