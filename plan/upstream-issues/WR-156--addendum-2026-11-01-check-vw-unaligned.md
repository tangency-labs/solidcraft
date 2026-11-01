#### WR-156 (addendum 2026-11-01) — Opt-in V and W unaligned discontinuity rejection in `SmCurveClassification::Intersect3DDiscontinuities`
- **Updates:** WR-156
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10095-10100` at v1.0.1 `3fd5b95` (`:10025-10027` at `c9c5979`), `SmCurveClassification::Intersect3DDiscontinuities(const SmVolume&, ...)` (def. `:10040`).

`Kernel::fixes.volume_discontinuity_checks_vw_unaligned` (#2171) opts into testing
`d.u.unaligned || d.v.unaligned || d.w.unaligned` instead of `d.u.unaligned` alone when
rejecting unaligned volume discontinuities with `SM_ERR` in
`CurveClassification::intersect_volume_discontinuities` (factored through
`CurveClassification::intersect_volume_discontinuity_report`). It defaults to `false`,
preserving pinned C++ v1.0.1 parity across all callers and difftests.

1. **Root cause.**
   After calling `SmVolume::HasDiscontinuities(&sDisconts, FALSE)` and reading the natural
   parameter domain, `SmCurveClassification::Intersect3DDiscontinuities` guards against
   unaligned discontinuities with
   `if (sDisconts.m_bUnalignedU == TRUE || sDisconts.m_bUnalignedU == TRUE || sDisconts.m_bUnalignedU == TRUE)`,
   testing `m_bUnalignedU` three times and never reading `m_bUnalignedV` or
   `m_bUnalignedW`. If a `Discontinuities3d` report marks `v.unaligned` or `w.unaligned`
   while `u.unaligned` is `false`, upstream proceeds to build constant-parameter planes
   from `m_sParamsU`/`m_sParamsV`/`m_sParamsW` instead of returning `SM_ERR`.
2. **Opt-in behaviour and scope.**
   - Default (`volume_discontinuity_checks_vw_unaligned == false`): checks `d.u.unaligned`
     only, matching pinned C++ v1.0.1.
   - Opt-in (`volume_discontinuity_checks_vw_unaligned == true`): checks
     `d.u.unaligned || d.v.unaligned || d.w.unaligned` after `volume.natural_param_domain()?`
     (preserving upstream check order) and returns `SM_ERR` without mutating classification
     intervals.
   - Note on reachability through `Volume::has_discontinuities`: in pinned v1.0.1,
     `SmDiscontinuities3d::SetUnaligned` sets all three flags together (`u`, `v`, `w` all
     `true` on compounded discontinuous volumes), while `BSplineVolume` leaves all three
     `false`; per-direction `v.unaligned` / `w.unaligned` reports are exercised directly
     through `CurveClassification::intersect_volume_discontinuity_report`, while
     end-to-end `intersect_volume_discontinuities` and `volume_discontinuity_intervals`
     verify compounded unaligned rejection and aligned `C0` `BSplineVolume` parity.

- **Code:** `crates/solidcore/src/kernel.rs`
  (`UpstreamFixes::volume_discontinuity_checks_vw_unaligned`),
  `crates/solidcore/src/intersect/curve_class/p8.rs`
  (`CurveClassification::intersect_volume_discontinuities`,
  `CurveClassification::intersect_volume_discontinuity_report`).
- **Tests:** `crates/solidcore/src/intersect/curve_class/p8.rs::tests::wr156_*`
  (`wr156_flag_metadata_and_toggle`,
  `wr156_v_and_w_unaligned_reports_default_vs_opt_in`,
  `wr156_u_unaligned_compounded_and_aligned_volumes_match_across_modes`).
