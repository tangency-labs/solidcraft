#### WR-546. `CreateFilletBrep`'s stitch can keep a corner patch's approximate edge instead of the fillet's exact section
- **Severity:** Inexact result (a section half 1.4e-5 to 1.7e-5 off its round)
- **Area:** Blends (fillet executive)
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp`, `SmFilletExecutive::CreateFilletBrep` (def. `:1931`), the stitch of `m_pFilletBrep` (`:2627`) with `SmStitchCallback`'s default choice.
- **What happens:** an N-sided corner patch (a set-back corner's six quadrilaterals) is fitted to non-rational approximations of the fillets' sections and rails (`CreateCornerBlend`, as `N_FillNSidedHole` needs). `MakeFaceBrep` bounds its faces with the surfaces' isoparametric curves, so along each section the patch's edge is the approximation. Of a section half and the patch's edge, the stitch keeps the one nearer the other's faces. The two distances are near-equal (1.4969e-5 against 1.4988e-5), so one half of a section came out as the exact arc and the other as the polynomial approximation, off the round.
- **Trigger:** a box 40 x 40 x 40 with every edge rounded r 4 and a setback of 7 at the corner (0, 0, 0).
- **How found:** tangency-labs/solidcore#1377 (SolidCraft). The fork's fix is tangency-labs/smlib@1072d1b ("a corner's patch meets a fillet on the fillet's own edge"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.fillet_patch_keeps_fillet_edges`, the stitch of the fillet body (`FaceStitchAccess::stitch_body_keeping_fillet_edges`) uses `FilletPatchStitchCallback`, the fork's `SmFilletPatchStitchCallback`:
  - It takes the default choice first.
  - Where an edge whose faces are all corner patches meets an edge with no patch faces, it keeps the latter.
  - The max distance it reports is the larger of the default's and the kept curve's `SurfaceMaxDistanceBetween` to each of the dropped edge's faces.

  `SmBrep::StitchFaces` takes the callback as the fork's optional last argument (`stitch_faces_with_callback`).

  Test: `tests/wr_546_setback_patch_keeps_fillet_edges.rs`.
