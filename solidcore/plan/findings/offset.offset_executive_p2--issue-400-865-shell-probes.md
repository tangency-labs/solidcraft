# offset.offset_executive_p2: shell probes for #400 and #865 pass in Rust (2026-10-26)

The SMLib fork lists tangency-labs/solidcore#400 and #865 as unfixed. Rust probes,
each checked against a closed form, show that the Rust port does not share either failure. Both
probes are kept as regression tests.

These numbers were measured on the port while it followed c9c5979 (1.0.0). Both tests
were rechecked on the 1.0.1 port (3fd5b95) and pass unchanged: `issue_400` 2 of 2,
`issue_865` 2 of 2 (default and with the fixes). That matches 1.0.1's two offset changes
as read from its source:
- `RemoveExcessFaces` forgets the faces and edge uses it deletes. The port already
  forgot the faces.
- `SmOffsetGeometryCreation` bridges a tangent edge between an offset face and an open
  shell face. Neither probe has an open shell face.

- **#400** (`crates/solidcore/tests/issue_400_shell_cylinder_seam_on_face.rs`). The
  fork's shell of a box joined with a cylinder whose seam lies on a box face fails
  SMLib's check (test #61; rig case shell/1494, not public). The probe reconstructs it:
  - a 40 x 40 x 20 box joined with an r10 cylinder about (30, 20), z -10..30;
  - the cylinder touches the face x = 40 along its +x line.

  Shelled closed 2 inward, Rust gives a manifold solid: 20 faces, 40 edges and 24
  vertices, volume 13525.990. The sharp-offset closed form is 13525.947, a relative gap
  of 3.2e-6. The input union is 38283.193 against 32000 + 2000π. The default path needs
  no opt-in.
- **#865** (`crates/solidcore/tests/issue_865_whole_offset_of_holed_plate.rs`). The fork
  traced the ladder part's failures (a closed shell of 1001, a whole offset of volume
  0) to #977. The ladder part is not public. The probe takes the whole inward offset
  (`api_offset_brep_full`, -1.5) of #977's plate instead: 30 x 20 x 6, with an r1.5 hole
  1 from the front side, so the hole's offset crosses the front's.
  - Default (upstream): non-manifold, volume 0. This is the fork's failure.
  - With `Kernel::fixes.offset_excess_faces_after_merge` (WR-341, #977) and
    `offset_resolve_crossing_loops` (WR-345, #854): a manifold solid of 1316.9265
    against the closed form 1316.9276 (8.8e-7).
  - The test asserts the opt-in result, so the default stays upstream's.
