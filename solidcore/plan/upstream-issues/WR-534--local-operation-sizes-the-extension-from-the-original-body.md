#### WR-534. `SmBrep::LocalOperation` sizes the neighbours' extension from the original body, so a face moved further than the body's size is not reached
- **Severity:** Wrong result (an open result; the operation fails)
- **Area:** Offsets and shells
- **Location:** `source/SMLib/src/SmBrep.cpp`, `SmBrep::LocalOperation` (def. `:11945`): `CalculateBoundingBox(sBBox)` and `ComputeSphereBound` size the offset executive from the original body only.
- **What happens:** The local operation finds the edges of a moved face by extending the neighbouring surfaces until they meet the new surface. The extension distance comes from the original body's bounding sphere. A face moved further than about the body's size lies beyond the extended neighbours, `MateEdges` finds no intersection, and the result is open.
- **Trigger:** a 20 mm cube with its top face moved up by 60 (the capi's `smlib_offset_faces`); the expected result is a 20 x 20 x 80 box.
- **How found:** reported by SolidCraft. The fork's fix is tangency-labs/smlib@a1ee273 ("offset faces: neighbours are extended to reach a face moved further than the body's size"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port's `local_operation` follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.local_operation_box_holds_new_surfaces`, `local_operation` adds to the body's box each given surface's box over its natural domain (`CalculateBoundingBox`), in input order, before taking the bounding sphere. `moves_the_top_of_a_box_further_than_its_size` in `topology/brep/local_operation.rs` moves a 20 mm cube's top to z = 80 and checks for 6 faces and a box from 0 to 80.
