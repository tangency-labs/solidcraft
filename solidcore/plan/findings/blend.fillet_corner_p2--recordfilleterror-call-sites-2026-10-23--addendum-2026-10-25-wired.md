# blend.fillet_geom_p3: RecordFilletError wired into ErrorInfo (2026-10-25)

Updates: plan/findings/blend.fillet_corner_p2--recordfilleterror-call-sites-2026-10-23.md

`SmFilletEdge::CalcFilletIntFillet` (`crates/solidcore/src/blend/fillet_geom/p3.rs`) and
`SmFilletEdge::CalcFilletIntSideFace` (`crates/solidcore/src/blend/fillet_geom/p5.rs`)
now resolve `pCorner` via `CornerLookup::corner` and record `SM_FILERR_BAD_INT` through
`FilletCorner::record_fillet_error_in` into `ErrorInfo<ExecutiveErrorObject>` before
returning `SM_ERR` (when a caller omits `ErrorInfo`, the helper returns
`UNPORTED(blend.fillet_executive_p1)` for `SmFilletExecutive::NoteFilletError`).
`FilletCorner::calc_corner_edge_geom` forwards `FilletExecutiveData::error`, and the
`blend.fillet_geom_p3` difftest runner supplies an `ErrorInfo` matching the shim's
`shim_executive()` (`SmFilletErrorInfo`).

Both remaining `blend.fillet_corner_p2` deferrals in `blend.fillet_geom_p3` pass
bit-for-bit (`318/325` pass, `7` rejected, `0` deferred, `0` failed; `--perturb`
`SENSITIVE` `318/318`), promoting `blend.fillet_geom_p3` to `green` (PR #972).
