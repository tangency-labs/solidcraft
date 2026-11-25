# blend.fillet_solver_p4 `gen0512`: the calc_geom shim's split flag was never read (fixed, PR #584, 2026-10-23)

Follow-up to "PR #463: 27 Rust-vs-C++ mismatches newly reachable once
`SmSurface::DropCurve` is wired" (blend.fillet_solver_p4 group). `gen0512` (calc_geom,
edge 3, `split`, orientations `[true, false]`): C++ `SM_ERR_NULL_POINTER` with no G1 knot
left, Rust `SM_ERR` with the split knot still in `m_vG1Knots`. An instrumented C++ build
showed `CalcFilletGeom` reading `GetSplitFlag() == FALSE`: the `SmFilletSolver`
constructor already adds a default `SmFilletGeom` (`m_vFilletGeoms.Add(new
SmFilletGeom(this))`), and the shim appended a second geometry carrying the split flag,
so `m_vFilletGeoms[0]` never split and the C++ went straight to `NER(pThisRail)`. The
Rust runner models the intended single geometry with the flag, so it split, and
`sm_FindFilletTsectPnt` failed (the two offset planes, +260 and -260 of the same plane,
do not meet), returning `SM_ERR` before the split knot is removed. The shim now sets the
split flag on the constructor's geometry; both sides then agree (the C++ now returns
`SM_ERR` with the knot left, as the port). The other `split` cases passed only because
their tsect point was found and both sides then stopped at the missing rail with the
knot removed. No kernel change; difftest 650 pass / 1 known -> 651 pass, unit green.
