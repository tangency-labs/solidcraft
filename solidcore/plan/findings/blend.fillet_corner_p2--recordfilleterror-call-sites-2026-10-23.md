# blend.fillet_corner_p2: RecordFilletError call sites (2026-10-23)

`SmFilletCorner::RecordFilletError` is ported (`FilletCorner::record_fillet_error`,
PR #641): it takes the executive (`FilletExecutive::note_fillet_corner_error`) and the
caller's identities of the filleted vertex, fillet surfaces and fillet vertices. Its two
callers, `SmFilletEdge::CalcFilletIntFillet` (`blend/fillet_geom/p3.rs`) and the
fillet-vertex calculator in `blend/fillet_geom/p5.rs`, hold the corner only as a
`FilletCornerId` (`GetFilletCorner()`), with no store that resolves it to the corner record
or to the executive; the other corner calls there (`GetCornerType`, `GetFilletedVertex`,
... of `blend.fillet_corner_p1`) wait for the same store. Both stubs stay
`UNPORTED(blend.fillet_corner_p2)` with this justification; wire them together with the
corner_p1 ones when fillet geometry code reaches the corners. No disagreement.
