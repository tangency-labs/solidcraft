#### UB-167. `SmApiStitch(SmBrep*, SmFace*)` always crashes: `SmMerge` dereferences its null other B-rep
- **Severity:** Wrong result
- **Area:** Healing
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1157`-`1188` (`SmApiStitch(pBrep, pFaceToStitch)`: `SmMerge sMerge( *pContext, pBrep, NULL )`), then `source/SMLib/src/SmMerge.cpp:98`-`125` (`SmMerge::SmMerge`: `if( dApproxTol3d < SM_EFF_ZERO )` ... `pOtherBrep->GetMinEdgeBBoxSize(dEdge2Size, pE2)`).
- **What happens:** The constructor comment says `pOtherBrep` "can now be Null, for the Stitch methods", but `SmApiStitch` passes the default `dApproxTol3d = 0`, so the automatic-tolerance branch runs and calls `GetMinEdgeBBoxSize` on the null other B-rep (SIGSEGV in `SmBrep::GetEdges` with `this == NULL`), before the face is looked at.
- **Trigger:** any call with a non-null B-rep and a non-null face, e.g. a `SmApiCreateBox` box and one of its faces.
- **How found:** difftest fuzzing of `heal.stitch_p1` (PR #196): 4 crashes in 2000 programs; gdb backtrace `SmBrep::GetEdges <- SmBrep::GetMinEdgeBBoxSize <- SmMerge::SmMerge <- SmApiStitch`.
- **Confirmed:** yes (C++ reference build, standalone reproducer under gdb).
- **Severity:** crash/UB.
- **Rust port:** `heal::stitch::p1::api_stitch_face` builds the merge with `MergeState::with_breps(.., None, 0.0, 0.0, None)` (PR #229), which returns `InvalidInput` where upstream dereferences the null other B-rep (after the primary's `GetMinEdgeBBoxSize`); the difftest FFIs refuse `SmApiStitch` with a B-rep and a face (`heal.stitch_p1`) and a computed tolerance without other B-rep (`boolean.merge_p2`).
