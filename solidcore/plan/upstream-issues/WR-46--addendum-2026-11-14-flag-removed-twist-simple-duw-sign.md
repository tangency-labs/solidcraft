Updates: WR-46

#### WR-46 (update, 2026-11-14). `twist_simple_duw_sign` is removed; the simple twist's `Duw` is the documented one

`TwistVolume::evaluate_simple` (`surface/twist_volume.rs`) always writes `Duw = (-per sin a, per cos a, 0)`, the `w`
derivative of its first partial `Du = (cos a, sin a, 0)` with `a = per w`, as the header documents and as the twist's
own `Evaluate` already did. The flag is gone, so the 2026-10-31 addendum's opt-in no longer exists. Removed: the field,
SolidCraft's preset line, its entries in the kernel rig's `fixes.json` and in `issue_2260_gn_pipe_refined_range_end.rs`,
the flag-off branch and the crate-private `evaluate_simple_impl`; volume dispatch calls `evaluate_simple` directly.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` and
  kernel-free calls of `TwistVolume::evaluate_simple` change: `Duw.y` flips sign, and so does every value computed
  from it (second and third partials of curves and surfaces mapped through a twist's `Map`).
- **Goldens:** `surface.twist_volume` eval_simple (76 rows; only `Duw.y` moves, every partial matches the twist's
  closed form), `surface.srf_in_volume` eval and eval_simple (46 rows; second and third surface partials through
  chains of twist, bend, unbend and transform maps, certified by interval forward-mode AD). Third-order surface
  partials still go through `smvol_LiftThirdDerivative` as pinned (WR-45, `volume_lift_third_derivative_terms`, off
  in the goldens), so they are certified against that lift applied to exact jets, not against the true partials.
  `surface.volume_p2` gen0106 (an unbend pseudo box mapped through a twist) moves too; that box is not an enclosure
  before or after this change, so the row is listed in `known_failing.json` for a maintainer decision instead of
  being blessed.
- **Tests:** `tests/wr46_twist_simple_duw.rs` runs on the default kernel. The flag-off halves that pinned upstream's
  sign are removed; the simple evaluation is checked against the twist's closed form for every partial up to third
  order, and the mapped curve and surface against central differences, as before.
