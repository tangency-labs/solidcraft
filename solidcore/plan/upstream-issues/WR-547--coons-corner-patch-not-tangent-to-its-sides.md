#### WR-547. `CreateCornerBlend`'s Coons corner patch is not tangent to its sides where rounds of different curvature meet
- **Severity:** Wrong result (a corner patch up to 15.6 degrees off the rounds it should be tangent to)
- **Area:** NURBS (B-spline surface corner blends)
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp`, `SmBSplineSurface::CreateCornerBlend`, the four-sided branch: the patch is `sm_CreateCoonsSrf`'s bicubic Coons patch.
- **What happens:** `CreateCornerBlend` promises a G1 patch. A bicubic Coons patch meets its cross-boundary derivatives only where their twists agree at the corners. Each derivative lies in its own surface's tangent planes, so the normal part of its twist is fixed by that surface's curvature on the two boundary tangents. Where two rounds of different curvature meet, the twists differ along the normal, and `sm_CreateCoonsSrf` takes their mean. The patch then left both rounds at an angle along whole sides:
  - 15.6 degrees off a bar's round at r 2;
  - 14.7 degrees in an L-shaped box's notch;
  - up to 38 degrees off a side face near a collapsed side.
- **Trigger:** SolidCraft rig case fillets/1207 (a bar's top-edge round meeting a boss's base round), and an L-shaped box's notch corner (a concave and a convex round), at r 2 and 5.
- **How found:** SolidCraft. The fork's fix is tangency-labs/smlib@cae8618 ("a Coons corner patch is tangent to its sides' surfaces"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.corner_blend_coons_patch_tangent_to_sides`, the four-sided `CreateCornerBlend` (not bilinear) calls `make_coons_patch_tangent_to_sides` (the fork's `sm_MakeCoonsPatchTangentToSides`):
  - A non-rational patch whose sides read more than `SM_CONTINUITY_ANGLE` off their surfaces is copied.
  - The copy is refined to 8, 16, 32 and 64 spans each way.
  - The control points next to each side are moved into the side's tangent plane at their Greville stations, onto the mean plane next to two sides.
  - This repeats until every side is within tolerance. The first copy within it, or else the nearest copy if it is nearer than the patch, replaces the patch.

  Test: `tests/wr_547_coons_patch_tangent_to_sides.rs`.
