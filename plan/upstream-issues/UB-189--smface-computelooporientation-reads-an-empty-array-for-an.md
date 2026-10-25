#### UB-189. `SmFace::ComputeLoopOrientation` reads an empty array for an edge loop use without edge uses
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFace.cpp:1851` `SmFace::ComputeLoopOrientation` (`sEUs.GetLast()`), `source/SMLib/inc/SmTArray.h:1035`-`1037` (`GetLast` returns `m_pData[m_lSize-1]`).
- **What happens:** for a loop use of edge type whose edge use array is empty, `GetLast()` reads `m_pData[-1]` (an out-of-bounds read of the array header's neighbour, or of a null array).
- **Trigger:** an edge-type loop use with no edge uses. Looks unreachable from files: `SmBrepData` sets `SmEdgeuse_TYPE` for a loop use only together with its edge use array (`SmBrepData.cpp:706`-`712`), so a loaded loop use of edge type has edge uses; it needs a B-rep built or edited in memory by calling code.
- **How found:** read during porting (unit `topology.face_p3`).
- **Confirmed:** no (the difftest guard rejects such a B-rep on both sides).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** `SolidError::InvalidInput` for a loop use without edge uses. (The other `ComputeLoopOrientation` reads that looked undefined, `sVeryFirstUV`, are defined: `SmVector2d` has default member initialisers `SM_UNDEF_DOUBLE`, `SmVector2d.h:56`-`58`; the port starts from `Point2::UNDEF` and computes the closing span with it.)
