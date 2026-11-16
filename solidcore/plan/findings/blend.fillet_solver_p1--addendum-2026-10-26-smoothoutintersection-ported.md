Updates: plan/findings/blend.fillet_solver_p1--smoothoutintersection-back-edge-stub-of-179-pr-192-round-4.md

# SmoothOutIntersection: ported (2026-10-26)

`SmMakeCurveBlendSIH` and its `SmoothOutIntersection` (with `sm_ComputeFilletRadius`) are
ported in `crates/solidcore/src/blend/fillet_solver/curve_blend_sih.rs`; the live solver's
`FlushCurve` (`IntersectorSolver` in `blend/fillet_solver/p4.rs`) runs it when the
executive holds that handler (`FilletSession::set_curve_blend_self_intersection_handler`).
The intersector's constant-radius assisted solver has no executive, so it reports
`InvalidInput` there instead of the former `Unported` stub.

Reach: no SM_API entry installs the handler, and prog_test's three declarations
(`smFilletTest.cpp:4739/4841/4926`) install the surface-blend handler `sSIH` instead of
the declared `pSIH`. The difftest shim installs it on `SmApiFilletEdges`' executive
(`fixtures/blend.fillet_executive_p3/curve_blend_sih.json`). Every traced flush then
either fails `NER(pFace1)` (compared: `Fillet/box_diff_cyl.smb` all edges, 9 calls) or
dereferences the NULL centre line and crashes (UB-260), so the trimming, blending and
joining body is ported but never compared against C++.

Fillet-corner temporary solvers (`SetFilletExecutive(m_pExecutive)`) now read the
executive's handler too (`inherit_executive_handler`), and `CreateFilletCorners` syncs it
to solvers loaded by `SetFilletParameters`.
