Updates: WR-708

#### WR-708 (update, 2026-11-06). `fillet_exact_arc_sections` is removed: every circular round takes the exact circle

A kernel built with the default fixes now builds a circular round's sections as the ball's
exact rational circle, as SolidCraft's set already did (tangency-labs/solidcore#2330, part of
#2273). `UpstreamFixes::fillet_exact_arc_sections` is removed. Both read sites take the fix's
side unconditionally: the executive's circular generator (`SetFilletParameters`,
`blend/fillet_executive/p2.rs`) and `MakeConstantRadiusFillet`'s (`blend/fillet_corner/p1.rs`).
An explicit request for approximated arcs (cross-section type 1 of the API options, which builds
`FilletSurfaceGenerator::circular(true, ..)`) still gets them. That is the caller's choice, not a
compatibility flag.

- **Results that change:** every circular round built without the flag that is not recognised
  as an analytic surface. Its sections were upstream's polynomial arcs within 0.05% of the
  radius; they are now the circle. Regression test:
  `wr708_exact_arc_sections::the_default_kernel_rounds_on_the_exact_circle`. On the default
  kernel, the end sections of the box 20 x 30 x 20's edge rounded by the linear law 1 to 5 were
  2.34e-4 off the ball's circle and are now 8.1e-15 off. A lone constant round between planes
  does not show the difference: the default kernel recognises it as an analytic surface.
- **Values pinned to the approximated arcs** move with the material the exact circles keep
  (about 0.0024 on the linear round of `wr685_variable_round_touch_circle`):
  - fork ea59e9c's great-circle mark in `fork_ea59e9c_section_orders` is 0.0168 off the cone
    and 11955.6489 (was 0.0166 and 11955.6466);
  - the great circles' removed volume in `wr685_variable_round_touch_circle` is 44.351 (was
    44.353).
- **The folded law without the G^n section** (`[2, 2, 40, 3, 2]`, every fix but
  `fillet_gn_section_generator`) is no longer built folded (726.5 removed against the balls'
  332.2). Its exact circles' sheet reaches every face of the body, and the crease trim refuses
  it without a reason. With the G^n section it is refused as a fold, naming the radius, as
  before.
- **Tests:** those that set the flag drop the setting. `without_it_the_approximated_section_is_off`
  and the fork d01d7b0 halves of `wr685_variable_round_touch_circle` (the approximated arcs'
  volumes) are removed with the behaviour they pinned.
- **Difftest:** the pinned C++ builds approximated arcs (this record), so cases whose round is a
  B-spline circular round on the default fixes now differ from it. They are listed as known
  mismatches with the finding
  `plan/findings/blend--exact-arc-sections-default-2026-11-06.md`.
