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
  kernel, the end sections of the box 20 x 30 x 20's edge rounded by the linear law 1 to 5 (the
  touch circles of its balls, WR-685) were 3.26e-4 off a circle and are now 1.36e-11 off, as
  the flag gave (SolidCraft's 52ea418). A lone constant round between planes
  does not show the difference: the default kernel recognises it as an analytic surface.
- **Values pinned to the approximated arcs** move with the material the exact circles keep
  (about 0.0024 on the linear round of `wr685_variable_round_touch_circle`):
  - `issue_2704_gn_side_curve`'s round removes 22.594877 (was 22.596105; the union of its balls,
    integrated numerically, 22.5948765);
  - `wr631_cliff_overflow_construction`'s variable cliff overflows were the fork d01d7b0's (the
    approximated arcs) and are graded against the box less the balls' cone in closed form
    instead, at the same 1e-7 (the builds are within 3.3e-8);
  - `wr609_trace_reaches_rails`' two traces are within 1.6e-5 of the removed volume and 2.1e-8
    apart (were 5.5e-5 and 1.5e-4).
- **Tests:** those that set the flag drop the setting. `without_it_the_approximated_section_is_off`
  and the fork d01d7b0 halves of `wr685_variable_round_touch_circle` (the approximated arcs'
  volumes) are removed with the behaviour they pinned.
- **Difftest:** the pinned C++ builds approximated arcs (this record), so cases whose round is a
  B-spline circular round on the default fixes now differ from it. They are listed as known
  mismatches with the finding
  `plan/findings/blend--exact-arc-sections-default-2026-11-06.md`.
