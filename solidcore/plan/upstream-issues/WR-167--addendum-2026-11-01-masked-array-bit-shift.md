#### WR-167 (addendum 2026-11-01) — Opt-in per-index bit shift in `SmTArray::FindElementInMaskedArray` (#2172)

Updates: WR-167

`Kernel::fixes.masked_array_uses_bit_shift` (#2172, default `false`) opts into testing each
element `ii`'s own bit `(mask & (1 << ii)) != 0` (using shift-safe `1u64.checked_shl`, `0`
for `ii >= 64`) in `find_element_in_masked_array` (`crates/solidcore/src/query/face/p3.rs`),
the Rust port of `SmTArray<TYPE>::FindElementInMaskedArray` (`source/SMLib/inc/SmTArray.h:1531-1555`).

- **Call-site audit:** Across `source/SMLib` and `source/SM_API` at v1.0.1 (`3fd5b95`),
  `SmTArray::FindElementInMaskedArray` has a single live caller: `sms_FindAndClassifyLoopBisector`
  (`source/SMLib/src/SmFace.cpp:2624`; a second occurrence at `SmFace.cpp:2250` is commented
  out). Likewise in `crates/solidcore/src`, `find_element_in_masked_array` is called only by
  `find_and_classify_loop_bisector_cached` (`query/face/p3.rs:1554`) on
  `FaceProps::pole_points` (`[UMinPole, VMinPole, UMaxPole, VMaxPole]`) with the `FaceProps::poles`
  side mask (`SS_UMIN = 1`, `SS_VMIN = 2`, `SS_UMAX = 4`, `SS_VMAX = 8`).
- **Default (`false`, pinned C++ parity):** Preserves `(mask & u64::from(1 < ii)) != 0`, which
  never matches indices `0` (`SS_UMIN`) or `1` (`SS_VMIN`) and tests indices `2..` (`SS_UMAX`,
  `SS_VMAX`) against bit `0` (`SS_UMIN`).
- **Opt-in (`true`):** Evaluates `let bit = u32::try_from(ii).ok().and_then(|s| 1u64.checked_shl(s)).unwrap_or(0); (mask & bit) != 0`,
  so each singular side index `0..4` matches its own mask bit in `props.poles` without
  false positives when `SS_UMIN` is set and without overflow when `ii >= 64`.
- **Regression tests:** `crates/solidcore/src/query/face/p3.rs::tests`:
  - `wr167_find_element_in_masked_array_comparison_vs_bit_shift` checks all four side bits,
    default false negatives/false positives, exhaustive 4-bit masks under opt-in, and
    `ii >= 64` shift safety.
  - `wr167_bisector_skips_pole_edge_on_vmin_or_vmax_under_opt_in` checks
    `UpstreamFixes::flag_mut("masked_array_uses_bit_shift")` and runs end-to-end through
    `find_and_classify_loop_bisector`, verifying singular-edge skipping on `SS_UMIN`,
    `SS_VMIN`, `SS_UMAX`, and `SS_VMAX` as well as elimination of the `SS_UMIN` false
    positive on slots `2` and `3`.
