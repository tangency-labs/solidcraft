Updates: WR-81

#### WR-81 (update, 2026-10-31). Opt-in check for face membership before removal in SmCPolyFace::RemovePolyFace

- **Severity:** Wrong result
- **Area:** Intersection / query.poly
- **Location:** `source/SMLib/src/SmPoly.cpp:13195` `SmCPolyFace::RemovePolyFace`.
- **What happens (default, pinned):** Upstream initializes `ULONG lFound = 0;` and ignores the status of `m_vPolyFaces.FindElement(pPolyFace, lFound)`. When `pPolyFace` is not found, `lFound` stays `0`, and `m_vPolyFaces.RemoveAt(lFound, 1)` removes the first element of `m_vPolyFaces`. The port faithfully replicates this default behavior in `PolyBrep::cpoly_remove_poly_face` (`unwrap_or(0)`).
- **Trigger:** Removing a non-member face from a composite polyface, removing the same face twice, or deleting a face after removing it from composite (`~SmPolyFace` / `delete_face` calls `RemovePolyFace` for any face pointing to the composite).
- **Confirmed:** Read at the pin (`3fd5b95`, `source/SMLib/src/SmPoly.cpp:13195-13201`); verified in unit tests (`upstream_quirks`) and difftest generated cases.
- **Opt-in fix:** `Kernel::fixes.cpoly_remove_poly_face_check_member` (or `PolyBrep::fixes.cpoly_remove_poly_face_check_member`), default `false` (off for pinned C++ parity). When enabled, `cpoly_remove_poly_face` removes the face only when it is present in `members` (`if let Some(i) = pos { members.remove(i); }`). If the face is not found, no face is removed.
- **Rust port:** `query::poly` (`crates/solidcore/src/query/poly.rs` in `PolyBrep::cpoly_remove_poly_face`).
- **Tests:** `crates/solidcore/src/query/poly.rs`: `tests::wr81_remove_poly_face_non_member` tests both flag states (default removing member 0 vs opt-in retaining members, double removal, and `delete_face` after removal).
