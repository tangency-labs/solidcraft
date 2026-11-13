# topology.brep_local_operation: a caller marks the picked face where upstream marks the replacement surface, and a merge survivor can differ (2026-10-30)

- **Unit:** `topology.brep_local_operation` (`SmBrep::LocalOperation`); the attribute handlers of `offset.offset_executive_p2` and `boolean.merge_p2`. tangency-labs/solidcore#1875.
- **Upstream caller:** the SMLib capi's draft (tangency-labs/smlib `capi/smlib_ops.cpp`, `draft_faces` and `moved_faces`).
  - It puts an `SM_AB_COPY` attribute on each `SmSurface` it hands `LocalOperation`.
  - On success it answers `SMLIB_ERR_NO_EFFECT` when no result face's surface carries one.
  - `LocalOperation` itself reports only a status (`SmBrep.cpp:11945`). The mark reaches the result through SMLib's attribute propagation: `SmLocalOperationGeometryCreation::FaceuseOffset` uses the caller's surface or its analytic copy, and the offset, merge and `CopyFaces` hand its attributes on.
- **The port:** there is no surface identity apart from the owning face. `Surface::owner()` is the face's reference, and the merge keys surface attributes by it (`boolean::merge::p2::copy_surface_attributes`). So a surface attribute and a face attribute are one record in every operation's store, and a free replacement surface has no identity to carry one.
  - The equivalent is a mark on the picked face (`Brep::add_attribute`). `faces[i]`'s offset face takes `surfaces[i]`, and the face's attributes travel with it through the offset and the copy back, as upstream's surface attributes do.
  - Test: `crates/solidcore/tests/issue_1875_local_operation_marks.rs`.
- **The difference:** a merge (or coincidence) that keeps an unpicked survivor face over a picked one.
  - Upstream: the survivor keeps its own `SmSurface`, so the mark on the picked face's surface is gone.
  - The port: the merge hands the survivor the picked face's mark (a merge gives the survivor the attribute ids it lacks).
  - Effect: in that case the port's check reads "moved" where the capi answers `SMLIB_ERR_NO_EFFECT`.
  - No fixture reaches it.
- **Not fixed:** closing it needs surface identities distinct from faces through the merge, the offset executive's notifications (as curves have `original_curve_ref` / `result_curve_ref`), the restore and the copy-back. That is a design change to the arena model, not a default-path port bug: every attribute a caller can set today travels as upstream's does.
