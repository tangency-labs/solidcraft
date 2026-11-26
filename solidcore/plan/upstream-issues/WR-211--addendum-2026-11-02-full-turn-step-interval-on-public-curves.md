Updates: WR-211

#### WR-211 (update, 2026-11-02). The turn-preserving opt-in decides a public ellipse step

`Kernel::fixes.ellipse_adjust_step_interval_keeps_turns` (the 2026-10-30 addendum) is
observable through the public curve factories alone, with every other fix at its
default. Measured at `2de7e8f34` with Rust 1.98.0: `create_analytic_circle(k, origin,
1.0)` and `create_ellipse(k, origin, 3.0, 2.0)` each start on `[0, 360]` with the NURB
interval `[0, 1]`; `Ellipse::adjust_step_interval(k, [360, 540], None)` then differs
before any other operation.

| after the edit, circle r = 1 | flag off (pinned C++) | flag on (the fork) |
|---|---|---|
| STEP domain | `[360, 540]` | `[360, 540]` |
| NURB interval | `[1, 1]`, length 0 | `[1, 1.5]`, length 0.5 |
| knots | `1 1 1 1 1 1 1 1` | `1 1 1 1.25 1.25 1.5 1.5 1.5` |
| point at NURB start / middle / end | `(1, 0, 0)` three times: the curve is that one point | `(1, 0, 0)`, `(0, 1, 0)`, `(-1, 0, 0)`: angles 0, 90, 180 |
| after `edit_parameterization(k, [360, 540])` knots | `360 360 360 NaN NaN 540 540 540` | `360 360 360 450 450 540 540 540` |
| point at STEP 450 after that edit | `(NaN, NaN, NaN)` | `(0, 1, 0)` |

The 3 x 2 ellipse gives the same intervals and knots, with `(3, 0, 0)`, `(0, 2, 0)`,
`(-3, 0, 0)` on the flag-on arc (implicit residual `(x/3)^2 + (y/2)^2 - 1` zero within
`1e-15`) and `(3, 0, 0)` alone with the flag off. Radially, every flag-on sample is on
the circle to `1e-15`; every flag-off sample equals the arc's start.

So the guard decides at the first edit (a zero-length NURB interval against a half-turn
one); the collapse no longer needs the second operation to be visible. The NaN knots are
the #428 record reproduced above through public calls.

Test `crates/solidcore/tests/wr211_ellipse_step_interval_full_turn.rs` pins both halves:
`a_full_turn_step_move_collapses_the_nurb_at_default_fixes` (intervals, exact knots,
sample points, then the NaN knots and NaN interior point) and
`a_full_turn_step_move_keeps_its_arc_with_the_turn_preserving_fix` (intervals, exact
knots, on-circle samples at 0/45/90/135/180 degrees and on-ellipse samples at 0/90/180
before the reparameterisation, then the finite knots and the ellipse sample after
the same reparameterisation). Mutation: forcing the production read at
`crates/solidcore/src/curve/ellipse/p1.rs:2036` to upstream's branch fails the flag-on
half with `[1, 1]` where `[1, 1.5]` is expected; forcing it on fails the default half
with `[1, 1.5]` where `[1, 1]` is expected. Restored, both pass; no library change.

Limits: only the full-turn arc (`span == 360`) and the `inside_out = false` public
factories are covered; a partial arc's proportional branch, an inside-out ellipse and
the surface-of-revolution caller (`SmSurfOfRevolution::AdjustSTEPUVDomain`, the
SolidCraft flat-round-trip trigger) are not exercised here. The C++ reference was not
rerun for this case; the default half is the pinned behaviour already recorded in the
2026-10-25 addendum and by `step_interval_past_a_turn_is_rejected_without_changing_the_curve`
(the collapsed interval and untouched curve that test pins), and the flag stays off by default.
