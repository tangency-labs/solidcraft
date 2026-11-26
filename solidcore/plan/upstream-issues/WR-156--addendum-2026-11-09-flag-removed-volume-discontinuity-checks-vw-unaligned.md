Updates: WR-156

#### WR-156 (update, 2026-11-09). `volume_discontinuity_checks_vw_unaligned` is removed

`CurveClassification::intersect_volume_discontinuities` rejects `d.u.unaligned`, `d.v.unaligned` and `d.w.unaligned` with `SM_ERR`; upstream reads `m_bUnalignedU` three times and so only rejects the U direction. The fix is now unconditional and the flag is gone; SolidCraft already ran with it on. Removed: the field `UpstreamFixes::volume_discontinuity_checks_vw_unaligned`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (the survey found no golden or rig case that reaches a different result with the flag on).
- **Tests:** `intersect::curve_class::p8::tests::wr156_v_and_w_unaligned_reports_are_rejected` (V only, W only, both, in parameter and input space; intervals unchanged on the error) and `wr156_u_unaligned_compounded_and_aligned_volumes`.
