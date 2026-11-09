#### WR-167. `SmTArray::FindElementInMaskedArray` tests `1<ii` instead of `1<<ii`
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/inc/SmTArray.h:1543`, `SmTArray<TYPE>::FindElementInMaskedArray` (def. `:1531`). Called by `sms_FindAndClassifyLoopBisector` (`SmFace.cpp:2588`) on `SmFaceProps::m_sPolePoints` with the `m_lPoles` side mask.
- **What happens:** The mask test is `lBitMask & (1<ii)`, a comparison, not a shift: elements 0 and 1 are never searched, and elements 2 and up are searched exactly when bit 0 of the mask (`SM_SS_UMIN`) is set, whatever their own bits. The bisector therefore never recognises an edge mid point on the u-min or v-min pole, and tests the u-max and v-max poles only when the u-min side is singular.
- **Trigger:** A face whose surface is singular at its v-min side (e.g. a sphere pole) with an edge mid point on that pole: the edge is not skipped.
- **How found:** read during porting.
- **Confirmed:** no (the bisector cases of the `query.face_p3` difftest defer before comparing a pole skip).
- **Severity:** wrong result.
- **Rust port:** Ported as is (`query::face::p3`, private `find_element_in_masked_array`).
