#### WR-210. `MakeFacesWithCurves` counts each outer-loop group's curves with the patch's first loop counts
- **Severity:** Wrong result
- **Area:**
- **Location:** `source/SMLib/src/SmBrep.cpp:17590-17607` (`SmBrep::MakeFacesWithCurves`, split branch: the loop over `ll < lNumLoopsInGroup` reads `sThisPatchLoops.m_sLoopCounts[ll]`, but the curves and orientations from `sThisLoopGroup.m_sCurves[mm]` / `m_sOrients[mm]`).
- **What happens:** `sThisLoopGroup` holds only the current outer-loop group's loops, while the counts come from the patch's loop list from its start. For the first group of a sub-patch the two agree; for a later group the counts are those of the first group's loops, so the face gets the wrong number of curves per loop, and reading past the group's curves is an out-of-bounds read.
- **Trigger:** a surface with a `C0` crease and two disjoint outer loops (each with its own holes) on one side of the crease, so that `FindOuterLoops` returns two groups for one sub-patch.
- **How found:** read during porting (`topology.brep_p8`, PR #72).
- **Confirmed:** no (no native reproduction built).
- **Severity:** wrong topology, or UB (out-of-bounds read) when the first group has more curves.
- **Rust port:** keeps the counts; a read past the group's curves is `SolidError::InvalidInput`.
