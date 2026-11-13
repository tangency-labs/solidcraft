#### WR-542. Two rounds whose bands overlap on a face they share, away from any corner, build a body that is not closed
- **Severity:** Wrong result (an invalid body returned: not a closed solid)
- **Area:** Fillets
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp`, `SmFilletExecutive::CreateFilletBrep`: no check that the blends on a shared face stay clear of each other when no corner trims them.
- **What happens:** A blend takes off each face it rolls on the band between its filleted edge and its rail. Take two blends rolling on one face from edges that share no vertex, so no corner trims them. If one rail runs inside the other's band, the bands overlap and the blends are larger than the face allows. SMLib still builds the result, and it is not closed.
- **Trigger:** both vertical edges at one end of a box rounded past half the box's width (each rail beyond the other).
- **How found:** SolidCraft; the fork's fix is tangency-labs/smlib@c0d74ee ("blends whose bands overlap on a face they share, away from a corner, are too large for it", `SmFilletExecutive::BandsClearOtherRails`), which is not merged upstream.
- **Confirmed:** no C++ run was made here.
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.fillet_refuse_overlapping_bands`, the executive runs `refuse_overlapping_bands` right after `refuse_face_consuming_blends`. That calls the session's `geometry_bands_clear_other_rails`, the fork's `BandsClearOtherRails`:
  - For every pair of solvers on edges with no common vertex, and every pair of their rails on one face, 15 points along the first rail are tested. A point is tested only if it classifies on the face and its nearest point of the second blend's edge is strictly inside the edge.
  - Such a point lies inside the band if it is more than ten tolerances from both ends along the chord from the second blend's rail to its edge.
  - If any point lies inside, the fillet fails with `NoRoom`.

  `tests/wr_542_fillet_overlapping_bands.rs` rounds both vertical edges at the end of a 40 x 10 x 20 box. It checks that r 6 is refused and r 4 builds.
