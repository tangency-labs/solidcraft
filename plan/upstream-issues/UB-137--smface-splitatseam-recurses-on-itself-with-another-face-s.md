#### UB-137. `SmFace::SplitAtSeam` recurses on itself with another face's seam classifications
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFace.cpp:18098-18149` `SmFace::SplitAtSeam` (the `bMergeU && bMergeV` branch); the crash is in `SmCurveClassification::MergeInterval` (`MergeIntervals`).
- **What happens:** After merging the U seam, the loop over the faces the merge made computes `pTgtFace->HasSeamProblem(sThisCrvClassU, sThisCrvClassV, ..)` but then calls `SplitAtSeam(sThisCrvClassU, ..)` on `this`, not on `pTgtFace`. `MergeIntervals` then splits `this` along a classification of another face and dereferences invalid topology.
- **Trigger:** `MoveSeam` with `bOkToSplit` (or `SplitAtSeam`) on a face that is treated as closed (by the face properties' closed flags) or is periodic in either direction, with seam problems to merge in both directions, e.g. `vase_rev.smb` (UV trim curves rebuilt), face 5 of the traversal order, with an `SmFaceProps` giving `m_eBadCrossedSeam = m_eBadMissingSeam = SM_SP_BOTH`, `m_bClosedU = m_bClosedV = TRUE` (difftest case `vase_rev.smb_1_f5_op9` of `topology.face_p1`).
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes (SIGSEGV in `SmCurveClassification::MergeInterval` called from the recursive `SmFace::SplitAtSeam`, gdb backtrace).
- **Severity:** crash.
- **Rust port:** Faithful (recurses on the same face, bounded by `MAX_SPLIT_AT_SEAM_DEPTH`). `MergeIntervals` is wired (`intersect.curve_class_p5`), but its topology edits (`MergeInterval`, `intersect.curve_class_p2`) still defer, so the U merge makes no new face and the branch is not reached yet. The difftest shim keeps the C++ side away from every input that can reach it.
