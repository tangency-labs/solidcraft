Updates: WR-697

#### WR-697 (update, 2026-11-02). The Turn at a vertex of four edges re-tests its pair for G1, reads an unset angle where the edges' tangents can't be measured, and runs a tangent adjustment that a Turn can't need
- **Severity:** Wrong result
- **Area:** Fillets (corners)
- **Location:**
  - `source/SMLib/src/SmFilletCorner.cpp`, `SmFilletNx2Corner::MakeCornerTopology`: the four-edge branch's `sm_TestG1Continuity` and `sm_CheckFaceComplimentaryCornerConcavity`, `double dAngleDeg;` read uninitialised when the tangents can't be measured.
  - `SmFilletNx2Corner::AdjustTangentCorner`.
- **What happens:**
  - With the blend corner (step 4b), `SmBlendCorner::Classify` has already made a pair a Join where these tests hold. The two-blend topology it inherited from the Nx2 corner tests the pair again at a Turn of four edges. Where the edges' tangents can't be measured, `Classify` takes 90 degrees and calls the pair a Turn, but the topology's mixed-convexity test reads the unset angle.
  - The inherited tangent adjustment runs at that Turn. It acts only where the two edges' tangents are parallel within 2 degrees, which a Turn can be only at the rounding of an angle of exactly 2 degrees under a 2-degree tolerance.
- **How found:** the SolidCraft fork's smlib#113 (tangency-labs/smlib e4083c8 and 34288e5, merged at 2ac7a17). The fork's `MakeTwoBlendTopology` keeps only the mitre at a Turn of four edges (bevel, not blending, one `SM_FE_FILLET_X_FILLET` edge). The blend corner's `AdjustTangentCorner` is the base class's (success). Every capi log was the same before and after.
- **Rust port:** under the same opt-in `Kernel::fixes.fillet_blend_corner`, off by default.
  - `make_nx2_corner_topology` doesn't run the G1 and mixed-convexity tests at a blend corner's Turn of four edges. It mitres the two blends and leaves `g1_case` false. Where the tangents can't be measured, the port returned `InvalidInput` ("edge angle at the corner is unset"), and now it mitres, as the fork does.
  - `adjust_tangent_corner_with` makes no tangent adjustment at a blend corner's Turn.
  - The Join stays as it was. In solidcore it is still built and adjusted by the Nx2 port until step 4d (68e039c, 543d25a; #2307) is ported.
  - The pinned default is unchanged.
- **Confirmed:** in the port (`crates/solidcore/tests/wr697_four_edge_turn.rs`, and the unit test `a_blend_corner_turn_has_no_tangent_adjustment` in `blend::fillet_corner::p3`). The test rounds two opposite slant edges at a square pyramid's apex and at a pyramid-shaped pocket's floor, at r 0.5, 1 and 2. Every volume, face count and check line is the same to the bit before and after the change, with and without the flag. The record of what else smlib#113 deletes, and why it stays in solidcore, is `plan/findings/blend.fillet_corner_p3--fork-smlib-113-old-corner-code-still-reached.md`.
