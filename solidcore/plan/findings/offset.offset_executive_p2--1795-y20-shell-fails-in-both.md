# offset.offset_executive_p2: the #1795 plate shelled open at y = 20 fails in the pinned C++ and in the port alike (2026-10-30)

- **Unit:** `offset.offset_executive_p2` (`SmApiShellBrepFull`, `SmOffsetExecutive::ShellBrep`). tangency-labs/solidcore#1795. Upstream record: WR-617.
- **Inputs:** the #1528 drilled plate (`fixtures/offset.offset_executive_p2/data/issue1528_plate.smb`, built by `crates/solidcore/tests/issue_1528_write_operand.rs`), shelled 2 inward with extended convex edges and merged results, default fixes, open at its face y = 20 (`GetFaces` index 2 of 8), `create_offset_solid` false and true.
- **Both values:**
  - The pinned C++ 1.0.1 returns `SM_ERR` (1001) with no result, through `SmApiShellBrepFull` and through a bare `ShellBrep` on the read body. The port returns the same; its error comes from `boolean::merge::p2::make_faceuse` (`SmMerge::MakeFaceuse`).
  - The difftest cases `issue1795_plate_{api,static}_shell_2_y20` and `_y20_offset_solid` pass on the equal status. The digest shows `status [1001]` and `result.present [0]` on both sides.
  - Open at the top (z = 27.5), both sides shell the plate, and every value agrees. In the port the result has 16 faces, 37 edges and 24 vertices, and volume 9655.150582612916.
- **Every operand of the plate fails alike in the port:**
  - the body as built, in its building kernel and in a fresh one;
  - written and read back (the bytes equal the committed file);
  - the committed file;
  - an `api_brep_copy`;
  - each of these through the API or a bare `ShellBrep`.
  The operand's construction, file round trip and copy are not involved (`issue_1795_y20_read_back` in `crates/solidcore/tests/issue_1795_shell_hole_through_arc.rs`).
- **Correction:** an earlier reading of these cases took "pass" for both sides shelling the plate. A Y20 case's smaller key count (41 exact, 14 values, against 101 to 109 and 21 for the top) was the missing result projection.
- **Pins:** `shell_open_at_y20_fails_as_upstream` asserts the 1001 through the API. `shell_open_at_the_top_shells_as_upstream` pins the top-open result.
- **Suspected cause:** not traced. Which of `MakeFaceuse`'s `SM_ERR` exits (no containing region, a vertex orientation that cannot be matched, no radial sector) the shell reaches is open. Inset by 2, the hole's tube (r 7) passes out through the arc's offset (r 8), and the open y = 20 face meets the arc tangentially at (0, 20).
