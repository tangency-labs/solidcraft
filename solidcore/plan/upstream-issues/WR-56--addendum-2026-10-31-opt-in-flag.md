Updates: WR-56

#### WR-56 (update, 2026-10-31). Confirmed, fixed in the fork, ported as an opt-in flag
- **Confirmed:** yes. SolidCraft issue #1852 (jyviko) reports the same swap, and the port's unit test below shows both directions: a local result nearer than its guess is refused and one farther is taken.
- **Fix:** tangency-labs/smlib@0b4de8c (fork PR tangency-labs/smlib#70, on `replay/1.0.1`, not yet merged in the fork's main) measures each distance to its own point. Its capi test `corner_patch_drop_from_a_guess_is_its_foot` drops 201 points along a corner patch edge, each from the previous foot, onto the patch to 9.82e-13. The same commit also tightens the fork's own convergence test (737c392), which is not upstream and not ported.
- **Rust port:** still reproduces it by default. `Kernel::fixes.good_drop_result_distances_in_order` measures the distances the right way round in `surface::surface::p7::good_drop_result`.
- **Tests:** `surface::surface::p7::tests::wr_56_good_drop_result_compares_the_right_distances`. The flat patch (10 u, 10 v, 0) and the point (5, 5, 1), with a local result 5e-4 along the patch from the foot:
  - Default: refused with a farther guess, taken with the foot itself as the guess.
  - With the opt-in: the other way round.
- **Difftest:** no change on the default path.
