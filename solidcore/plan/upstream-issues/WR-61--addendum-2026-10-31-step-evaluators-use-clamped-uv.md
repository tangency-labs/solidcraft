Updates: WR-61

#### WR-61 (update, 2026-10-31). The clamped copy is now an opt-in fix

At the pinned commit `3fd5b95` (usd-brep v1.0.1),
`SmSurfOfExtrusion::EvaluateSTEP` (`source/SMLib/src/SmSurfOfExtrusion.cpp:897`)
clamps `sUV` to `m_vAnalUVDomain` at `:940`-`:943` and then evaluates the caller's
`crUV` at `:954`-`:955`; `EvaluateSTEPPoint` (`:1009`) clamps the same copy at
`:1014`-`:1017` and uses `crUV.x` / `crUV.y` at `:1020` and `:1024`. That native behaviour
was confirmed while porting (the record above) and is unchanged. The Rust port keeps it
by default (pinned C++ parity) and adds
`Kernel::fixes.extrusion_step_uses_clamped_uv` (default `false`), which evaluates the
clamped copy in the analytic UV domain instead.

- **Rust measurement (opt-in path).** The `surface.surf_of_extrusion.p1` tests
  `step_clamped_uv_fix_is_off_by_default`,
  `step_clamped_uv_fix_clamps_when_enabled_and_out_of_bounds_off` and
  `step_clamped_uv_fix_keeps_the_generator_flag_policy` build a real `SmLine`
  generator (a unit X line swept along Z) and a real quadratic B-spline generator
  through the public `SurfOfExtrusion::new` / `Line::create_line_segment`
  construction; no stand-in geometry is used.
  - With the fix off, `(0.5, 4)` on a `[0, 0, 1, 3]` domain evaluates to
    `(0.5, 0, 4)` (extrapolation along the extrusion), and `(0.9, 1)` on a surface
    whose analytic U is `[0.2, 0.8]` over a generator whose own STEP interval is
    `[0, 1]` evaluates to `(0.9, 0, 1)`.
  - With the fix on, those inputs evaluate at the clamped parameters,
    `(0.5, 0, 3)` and `(0.8, 0, 1)`, and `(0.9, 4)` at `(0.8, 0, 3)`. In-domain and
    exact-boundary parameters are bit-identical to the default path, and a
    `SwapUV` surface clamps in the same (analytic) domain.
  - `set_out_of_bounds_enabled(true)` keeps the old extrapolation with the fix either
    way.
  - The derivative matrix is checked at clamped parameters: position, `Sv = E`, the
    pure u derivative, the zeroed partial-v entries and an entry the caller's array
    keeps. A B-spline generator whose own out-of-bounds flag is on still extrapolates
    in `EvaluateSTEPPoint` while `EvaluateSTEP` evaluates its temporary copy with the
    surface's flag, so that policy difference is unchanged.
  - A missing generator, a short derivative array and orders above `MAXDER` keep their
    exact errors.
- **Limits.** This is a Rust-only measurement of the opt-in path: no new native (C++)
  case runs it and no new native count is claimed here; the original native
  confirmation above stands at the pin. The `SwapUV`-related difference between the
  two bodies is out of scope (both clamp in `m_vAnalUVDomain` and neither reads
  `SwapUV`, which the port reproduces). The regression tests do not gate difftest: the
  default path remains the pinned C++ behaviour, and the unit's status is unchanged.
