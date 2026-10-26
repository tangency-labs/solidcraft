Updates: UB-209

Confirmed natively (2026-10-26). `SmApiFilletEdges` on every edge of
`TestFiles/pt_TestFiles/Fillet/extrusion1002.smb` (3 edges) at radius 0.1, circular,
G1, thumbweight 1, crashes with SIGSEGV in a standalone program linked against the
pinned `c9c5979` libraries (`-O2 -ffp-contract=off`, the harness flags). Backtrace
(signal-handler `backtrace()`, with `SmFilletExecutive.cpp` rebuilt `-g`):
`SmFilletSolver::GetVertex` <- `SmFilletSolver::GetFilletCorner` <-
`SmFilletSolver::GetOtherFilletCorner` <- `SmFilletExecutive::CollectTangentEdgeSequence`
(`SmFilletExecutive.cpp:4640`, `pPrevFS->GetOtherFilletCorner( pCurrCorner )` on the
solver `GetTangentFilletSolver` returned) <- `ProcessSmoothEdgeSequences` <-
`CreateFilletBrep` <- `DoFilleting` <- `SmApiFilletEdges`. This is the tangent walk of
this entry: the solver pointer comes from a mistyped corner. The root cause (which
vertex keeps its edge-use array) is not re-derived for this file.

`blend.fillet_executive_p3` runs file-source cases in a fork crash trap; the case
`sweep4-native-crash-extrusion1002-all-r0.1` is committed as rejected.
