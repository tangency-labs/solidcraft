#### WR-717. A cliff overflow's crease that runs along an edge of the face it crosses is dropped, and the trim refuses at the sewing
- **Severity:** Wrong result (a refusal where the blend builds)
- **Area:** Blend (fillet executive, the overflow trim at creases)
- **Location:** the fork's `source/SMLib/src/SmFilletOverflow.cpp`, `SmFilletExecutive::TrimAtCreases` (tangency-labs/smlib#79 step 6, merged at 5e8c9c1), around `sm_PiecesInFace`. The Rust is `crates/solidcore/src/blend/fillet_executive/session/trim_at_creases.rs` and `crates/solidcore/src/blend/fillet_overflow.rs` (`pieces_in_face`).
- **What happens:** a disc 13 in radius and 2 high has its top edge rounded r a little past its height. The round crosses the bottom face's plane a gap g = r - sqrt(4r - 4) (about (r - 2)^2 / 4) inside the bottom edge.
  - The trim finds that crease (`sm_Creases`), but `sm_PiecesInFace` keeps only the runs it classifies inside the face. Within the trim's tolerance of the bottom edge, the whole crease classifies as on the edge, so it's dropped.
  - The sheet then isn't split, so its piece past the bottom stays, and nothing is imprinted on the bottom face.
  - The flood from the blended edge crosses the bottom edge and takes the bottom face off with the wall and the top's outer ring. The stitch leaves the sheet's edge open (one lamina edge), and the trim refuses: "a blend past its faces does not sew into the body along its creases and rails" (`SM_FILERR_BAD_MERGE`, status 1001).
  - It fails for r 2.0001 to 2.013 and builds at r 2 and r 2.02 and up. At r 2.00005 to 2.00007 a second limit also applies: the crease lies within the tolerance of the sheet's own rail too, so the sheet isn't split even once the crease is kept, and the rail sits 5e-5 to 7e-5 from the edge it should sew to.
- **Trigger:** a convex blend past its faces (WR-631's cliff overflow, trimmed per WR-695) whose crease on a face beyond the cliff runs within tolerance of one of that face's edges. SolidCraft's #190 disc rounded 2.001 is the case found.
- **How found:** porting SolidCraft's refusal cases to the difftest runner (#2273, #2325). SolidCraft's own "refused, fit 2" comes from its earlier kernel, which had no overflow.
- **Confirmed:**
  - The pinned C++ (3fd5b95) has no overflow trim, so it can't reach this.
  - A local build of the fork at 5e8c9c1 refuses r 2.001 with the same message.
  - In Rust (every `UpstreamFixes` field on, this one off), a scratch probe traced each step: the crease found, 0 pieces kept in the face at r 2.001 and 2.01 (1 at 2.05), the bottom face taken off, and `lamina_edges = 1` at the stitch.
- **Rust fix (opt-in):** `UpstreamFixes::fillet_overflow_crease_along_edge`, with `fillet_overflow_trim_at_creases`. Not the fork's.
  - A crease that runs along an edge of the face it crosses, within the trim's tolerance, is taken as that edge (`pieces_along_edges`). The sheet is split along it, and the edge stands as the imprint, as an extension's edges along the body do: the material taken off stops there, and the sheet is sewn to it.
  - A face that holds one of the sheet's edges only to the looser `on_tol` (a rail that runs past the face by less than ten tolerances) is still looked at for creases.
  - A sheet edge that lies along such an edge is sewn across the gap measured between them, at twice that gap (the stitch glues within half its tolerance) and never past `on_tol`.
  - Limit: the face keeps its own edge, within g of the crease, so the body carries a ring of width g (at most the trim's tolerance) within that edge's tolerance. On the disc the volume is V(r) plus about 2 pi 13 g: 2.0e-5 at r 2.001 and 5.1e-4 at r 2.005, against below 1e-7 where the crease lies inside the face. Moving the face's edge onto the crease would remove it.

  Tests: `crates/solidcore/tests/issue_2385_crease_along_edge.rs`:
  - r 2.00005 to 2.01 build one closed solid of three faces, at the Pappus closed form within the ring's bound;
  - the band is refused with the flag off;
  - r 2, 2.02 and 2.5 are bit for bit unchanged.

  Also `fixtures/solidcraft/190-round-as-tall-as-its-wall.json`'s `disc_top_rounded_just_past_its_height`.
