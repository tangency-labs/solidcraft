Updates: WR-97

#### WR-97 (update, 2026-11-10). `is_iso_curve_at_propagates_canonical_failure` is removed

`SmTrimmingTools::IsIsoCurve(curve, side, low, tol)` propagates a failing `GetCanonical` as `Err` instead of counting the curve as an iso curve. That is now the only path: `is_iso_curve_at` propagates it too, and `is_iso_curve_at_with_fixes` is removed. Its callers, `is_iso_curve` and `fix_backwards_outer_loop_on_seam`, call `is_iso_curve_at`. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. Every goldens unit (283) gives the same output at this change as at main `87400c80e`, run on Windows; `topology.trimming_tools`, which calls `is_iso_curve_at` directly, has no case whose `GetCanonical` fails. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on.
- **Tests:** `wr97_nurb_less_bspline_curve_is_an_error`, `wr97_valid_iso_non_iso_and_non_bspline_curves_unchanged` and `wr97_callers_reject_nurb_less_uv_curves` in `topology::trimming_tools`. The flag's registration test and the flag-off halves are deleted.
