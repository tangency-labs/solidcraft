#### WR-777. LocalOperation turns a full cylinder given to a round inside out
- **Severity:** Wrong result
- **Area:** Offset
- **Location:** `source/SMLib/src/SmBrep.cpp:11982`, `SmBrep::LocalOperation` (def. `:11945`).
- **What happens:** each new surface is turned to face the way the face's old surface did by
  comparing normals (`sOldNorm.Dot(sNewNorm) < 0.0` reverses it, `:11991`). The new normal is
  read at the middle of the new surface's own UV domain, the old one at the middle of the
  face's UV domain. For a full cylinder given to a round, the middle of the cylinder's domain is
  across the axis from the round, where the outward normal points the other way: the normals
  "oppose" though both surfaces face out, the cylinder is reversed, and the new round faces
  into the body. Its outer loop runs against its surface (`SmFace` check "Loop Orientation"
  where the face keeps UV trims), and with exact mate curves the body's volume comes out
  negative.
- **Trigger:** a 10 mm cube whose vertical edge at x = y = 10 is rounded r 2, the round given
  the coaxial cylinder r 2.3 (`create_cylinder_surface` about (8, 8)): the cylinder's domain
  middle is at (5.7, 8), normal (−1, 0, 0); the round's middle normal is (0.71, 0.71, 0).
- **How found:** read during porting (tangency-labs/solidcraft#13718)
- **Confirmed:** no (no C++ run of the case; solidcore reads the new normal where the new
  surface passes nearest the old face's middle, `topology/brep/local_operation.rs`)
