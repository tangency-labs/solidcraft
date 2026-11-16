Updates: WR-49

#### WR-49 (update, 2026-10-31). Representational end continuities and interior minimum are now an opt-in fix

At the pinned commit `3fd5b95` (usd-brep v1.0.1),
`SmBSplineVolume::CalculateContinuitiesSimple` (`source/SMLib/src/SmBSplineVolumeNLib.cpp:501`-`:521`)
documents in `source/SMLib/inc/SmBSplineVolume.h:689`-`:692` that `reMinContinuity` is
the "minimum continuity over all interior knots" and `rConts` sets
"end param continuities = `SM_CT_DISCONTINUOUS`" (matching `SmSurface::CalculateContinuities`
and the fallback of the `bCalcGeometric == TRUE` branch at `:607`-`:611`), yet its
`bCalcGeometric == FALSE` branch sizes `rConts` to `lKnotCnt` (zero-filling new entries
with `SM_CT_UNDEFINED == 0`) after an empty `// set end continuties` comment and loops
`ii = 0 .. lKnotCnt - 1`. That native behaviour was confirmed while porting (the record
above) and is unchanged. The Rust port keeps it by default (pinned C++ parity) and adds
`Kernel::fixes.volume_representational_end_continuities` (default `false`, #2151), which
sets both end knots (`0` and `knot_cnt - 1`) to `ContinuityType::Discontinuous` and
evaluates `knot_continuity(degree, mult)` and `min_cont` over interior knots
`1..knot_cnt - 1` only.

- **Rust measurement (opt-in path).** The `surface.b_spline_volume_n_lib` tests
  `wr49_single_span_clamped_volume_ends_and_has_discontinuities`,
  `wr49_multi_span_cubic_interior_continuities` and `wr49_volume_wrapper_and_flag_lookup`
  construct real clamped trilinear (`[0, 0, 1, 1]`), clamped tricubic
  (`[0, 0, 0, 0, 1, 1, 1, 1]`), and multi-span clamped cubic `BSplineVolume` and
  `Volume` instances directly (no stand-in geometry).
  - With the fix off (default), single-span clamped trilinear and tricubic volumes
    produce `conts == [C0, Undefined]` and `min_cont == C0` in `U`, `V` and `W`, and
    `has_discontinuities_simple(..., false)` returns `true`; a smooth clamped cubic
    volume with interior multiplicities `[1, 2]` (`[0,0,0,0, 1, 2,2, 3,3,3,3]`) produces
    `conts == [C0, C1C2, C1, Undefined]`, `min_cont == C0`, and
    `has_discontinuities_simple(..., false) == true`.
  - With the fix on, single-span clamped trilinear and tricubic volumes produce
    `conts == [Discontinuous, Discontinuous]` and `min_cont == CInfinity` across `U`,
    `V` and `W`, and `has_discontinuities_simple(..., false)` returns `false`.
  - On the smooth multi-span cubic volume, the opt-in path produces
    `conts == [Discontinuous, C1C2, C1, Discontinuous]`, `min_cont == C1`, and
    `has_discontinuities_simple(..., false) == false` (including through
    `Volume::calculate_continuities_simple`, `Volume::has_discontinuities_simple` and
    `Volume::has_discontinuities`), while adding a triple interior knot (`mult == 3` in
    degree `3`) produces `conts == [Discontinuous, C1C2, C1, C0, Discontinuous]`,
    `min_cont == C0`, and `has_discontinuities_simple(..., false) == true`.
  - A degenerate knot vector with a single unique knot (`knot_cnt <= 1`) still writes
    `params`, leaves `conts` empty and `min_cont == CInfinity`, and fails with `SM_ERR`.
- **Limits.** This is a Rust-only measurement of the opt-in path: no new native (C++)
  case runs it and no new native count is claimed here; the original native confirmation
  above stands at the pin. The `bCalcGeometric == TRUE` branch (which fails at the first
  iso-curve sample because upstream builds without `USE_VOLUMES`) is unchanged. The
  regression tests do not gate difftest: the default path remains the pinned C++
  behaviour, and the unit's status is unchanged.
