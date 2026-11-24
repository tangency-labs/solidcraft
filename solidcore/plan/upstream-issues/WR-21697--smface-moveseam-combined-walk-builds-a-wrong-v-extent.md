#### WR-21697. `SmFace::MoveSeam`'s combined walk builds a wrong V extent: U period and minimum, no start at the first sample

- **Severity:** Wrong result
- **Area:** Topology, seam repair (`MoveSeam`)
- **Location:** `source/SMLib/src/SmFace.cpp`, `SmFace::MoveSeam`, the edge walk with the block commented "small performance savings for tori - build both Bounding Boxes on first pass" (about `:17200-17376`).
- **What happens:** when both seams are moved (`bFixSeamBoth`), the U walk fills the V extent `sPeriodicV` too, and the V walk is skipped. The V extent it builds is not the one a V walk builds:
  - Each V span is added the short way round only if it is shorter than the **U** period minus itself (`pPeriodic->GetPeriod()`), and is offset by the **U** minimum (`pIvl->GetMin()`). That is right only when U and V share period and minimum (a torus on `[0, 360]` in both). With a V step past half the U period every V span is added as its complement; with different minimums every span is shifted.
  - `pPeriodic->SetMinMax(...)` at the first sample starts only the U extent. The V extent stays the point at the V seam, and `AddInterval` refuses a span disjoint from it (status ignored), so every span before the walk reaches the seam is lost, and all of them when the face does not touch the seam.
  - The V seam then goes to the middle of what the V extent leaves unused, which can be on the face.
- **Trigger:** a face on a surface closed in both directions whose seam problems are reported in both directions. `crates/goldens` `topology_face_p1::tests`:
  - `move_seam_both_on_the_torus_keeps_its_edges`: the six `MoveSeam` rows on `torus.smb` (U `[0, 120]`, V `[0, 48.284]`) whose face properties report both seams crossed. The V seam was moved onto the face, and every row broke `check_brep`'s gap checks.
  - `move_seam_both_walks_v_as_the_v_walk_does`: a bilinear square torus with U period 1 and V period 12, a face over U `[0.1, 0.6]` and V `[1, 7]` that does not touch the V seam. The V seam was moved into the face; walked in V it moves to knot 9, outside it.
- **How found:** CodeRabbit's review of #21622 (the period and minimum, #21697); the start while writing the test for it.
- **Confirmed:** by reading the C++, and by the tests above on the Rust port, which did the same before the fix.
- **Rust port:** fixed. `move_seam_notified`'s combined walk starts the V extent at the first sample and measures each V span against `periodic_v`'s period from the V interval's minimum.
- **Not fixed, read only:** the walk also stops once the U extent is full (`pPeriodic->IsFullPeriod()` in both loop conditions), whatever is left of V. No case found shows it: on the square torus above, faces over the whole U period with V `[1, 7]`, `[1, 11]` and `[0.5, 2.5]` give the same V result with and without walking on (measured on the port).
