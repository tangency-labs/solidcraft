Updates: WR-4

#### WR-4 (update, 2026-10-31). Withdrawn as observationally equivalent on IEEE-754 binary64

- **Finding:** `WR-4` was filed from source reading during porting (`Confirmed: no`) after
  observing that `SmExtent3d::ApproximateUnbounded` (`source/SMLib/src/SmExtent3d.cpp:1789-1848`,
  and likewise `SmExtent1d::ApproximateUnbounded` and `SmExtent2d::ApproximateUnbounded`)
  mutates `sMin` in place before reading `sMin` to compute `sMax` in the
  `SM_BT_UNBOUNDED_MAX` branch. On IEEE-754 binary64 (`-ffp-contract=off`), reading the
  mutated `sMin` is bit-for-bit identical to reading the original `min` for every `f64`
  triple `(min, pc, h)`:
  1. When `pUnboundedCenter == NULL || min > pc - h`, `sMin` is unchanged
     (`new_min = min`), so `new_min > pc - h` is `true` and `new_min + 2 * h` equals
     `min + 2 * h`.
  2. When `pUnboundedCenter != NULL && !(min > pc - h)` (including `min <= pc - h` and any
     `NaN` operand), `sMin` becomes `v = pc - h`. The subsequent `sMax` guard evaluates
     `v > v`, which is irreflexive (`false`) for **every** binary64 value (finite numbers,
     `±0.0`, `±inf`, subnormals, and `NaN`). Both the mutated-`sMin` expression and an
     unmutated `min > pc - h` check therefore take the `pc + h` branch without reading
     `sMin` (even when `(pc - h) + 2 * h != pc + h`, e.g. `pc = 1.0, h = 0.3`).
- **Focus-window semantics:** Replacing finite `sMin` with `pc - h` when `min <= pc - h` in
  `SM_BT_UNBOUNDED_MAX` is symmetric with `SM_BT_UNBOUNDED_MIN` replacing finite `sMax`
  with `pc + h` when `max >= pc + h` across `SmExtent1d`, `SmExtent2d`, and `SmExtent3d`: a
  `2 * h` viewing window centered on `pUnboundedCenter` is clamped into the half-space while
  `pOptExpandedApprox` pushes out only the originally infinite end for `SmExtent3d::Draw`.
- **Rust port:** Because an `UpstreamFixes` flag would have zero behavioural effect on any
  input, no flag is added (`#2132`). `approx_axis` in `crates/solidcore/src/geom/extent3d.rs`
  now reads `min_near_center` and `min` directly, and unit tests
  `wr4_unbounded_max_center_regimes_and_expanded`,
  `wr4_mixed_boundary_types_and_negative_half_size`, and
  `wr4_approx_axis_bit_identical_to_mutated_min_across_ieee754` verify bit-for-bit
  equivalence against upstream's mutated-`sMin` expression across all boundary kinds,
  center regimes, negative half-sizes, `NaN`/`±inf`, and expanded outputs.
