#### WR-81. `SmCPolyFace::RemovePolyFace` of a non-member removes the first member
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmPoly.cpp:13195` `SmCPolyFace::RemovePolyFace`.
- **What happens:** The status of `FindElement` is ignored and the index starts at 0, so a face that is not a member removes member 0. `~SmPolyFace` calls it for any face whose `m_pCPolyFace` still points at the composite, for example after an earlier `RemovePolyFace` of that face.
- **Trigger:** `RemovePolyFace(f)` twice, or deleting a face after removing it from its composite.
- **How found:** generated difftest case (`query.poly` mutate ops).
- **Confirmed:** yes (`query.poly` generated `cpoly_remove` cases agree with the port, which keeps the behaviour).
- **Severity:** wrong result.
- **Rust port:** Faithful.
