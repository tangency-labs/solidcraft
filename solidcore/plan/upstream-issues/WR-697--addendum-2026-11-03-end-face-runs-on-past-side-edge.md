Updates: WR-697

#### WR-697 (update, 2026-11-03). An End whose end face runs on tangentially past its side edge is refused, but the global merge builds it
- **Severity:** Wrong result (a refusal of a body that builds)
- **Area:** Fillets (corners)
- **Location:** the fork's `source/SMLib/src/SmBlendCornerEnd.cpp`: `SmBlendCorner::CalcEndEdgeGeom` (`RefuseEndPastSideEdge`) and `SmBlendCorner::FitOfEndPastSideEdges` (tangency-labs/smlib `41a39ec`, smlib#124, still so at `v1.1.0-alpha.1`, `8eac63c`).
- **What happens:**
  - An End on its end face G, whose rail on its face F runs past the far end P of its side edge (between F and G), is refused with `SM_FILERR_NO_ROOM`. The refusal names the radius whose rail reaches P.
  - That refusal also fires where G runs on tangentially past P into a face H, which F's next edge from P bounds. There the rail ends on H. G and H are one smooth surface, and cutting the round where it meets them (the global merge, WR-303) is exact.
  - Before smlib#124, the fork and this port built such an End through WR-303's dropped end rib and global merge.
- **Trigger:** SolidCraft #11360's `a_round_ending_in_an_earlier_round_matches_both_together`. The part is the arc-wall plate: the plate x 0..41 1/6, y -21..0, plus the corner x 41 1/6..62 1/6 less the cylinder r 21 about (62 1/6, -21), 10 high.
  - The arc's top edge (or the back top edge) is rounded r 2. Then, in a second call, the upright at the knife-edge tip (62 1/6, 0) is rounded r 2.
  - The second round's End is on the first round. Its side edge is the first round's boundary: on the back face for the arc case, on the arc wall for the back-edge case. Its far end P lies on the first round's top rail, where that round runs on tangentially into the top face.
  - Refused with 1.04762 and 1.05 named. Rounded together at r 2, both build.
- **How found:** bherbruck's question on #2364 (from #11360's known-failing line 2).
  - The fork, built at 6843885 (the same `SMLib` and `SM_API` source as `v1.1.0-alpha.1`), refuses both, also when `AddEarlierPassFace` is given the first call's faces (5 for the arc case, 3 for the back case). The named radius builds.
  - main before #2364 (fb937dd8) built both, through `drop_end_rib`'s global merge (WR-303).
- **Rust fix:** under the existing `Kernel::fixes.fillet_blend_corner`, with no new flag: a refusal becomes a correct build.
  - The narrow rule is `blend::blend_corner::end::end_face_runs_on_past_side_edge`. It holds when P has exactly three edges: the side edge, an edge that bounds F and a face H, and an edge between G and H whose faces meet tangentially at P.
  - Where it holds, `calc_end_edge_geom` deletes the end rib and returns `EndEdges::GlobalMerge`, and the session sets the executive's global merge. `fit_of_end_past_side_edges` does not size that side edge.
  - Elsewhere the refusal is unchanged. That includes the boss's wall split by a real edge, where past P the end face meets a crease; it is still refused naming 0.583942734. The mixed End's overflow refusal (54a4ee7) is untouched.
- **Values:** solidcore, accuracy 1e-8, `check_brep` level 2 clean, 8 faces each.
  - Arc top, then the tip: 9452.927851913100, against 9452.927826170264 rounded together (2.7e-9 relative, two constructions of one body).
  - Back top, then the tip: 9428.557343079310, against 9428.557343079308.
  - Both equal main before #2364 to the printed digits. Test: `crates/solidcore/tests/issue_2364_round_ending_in_an_earlier_round.rs`.
