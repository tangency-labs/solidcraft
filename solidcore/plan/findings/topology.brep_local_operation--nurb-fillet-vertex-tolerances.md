# topology.brep_local_operation: vertex, edge and face tolerances of Solids/nurb_fillet.smb after the merge copy (2026-10-28)

Unit: `topology.brep_local_operation` (`SmBrep::LocalOperation`, PR #1427).

Inputs: `Solids/nurb_fillet.smb` with the surface of any one face (`face` 0..7 of `GetFaces`)
replaced by the plane through the surface point at the middle of the face's UV domain with
that surface's normal (offset 0, no tilt): cases `nurb_fillet_face0_same` ..
`nurb_fillet_face7_same` in `fixtures/topology.brep_local_operation/nurb_fillet.json`.

Result: status, face and edge lists, topology, vertex points, edge intervals, UV domains
and the samples of every edge curve and face surface agree within `1e-9` relative
(`max_rel_err` over those is 0). Only tolerances differ, always larger on the Rust side:
`nurb_fillet_face0_same` vertex tolerances (C++ -> Rust) `5.000000003972055e-6 ->
5.759574129151012e-4`, `5.000000003140185e-6 -> 9.550966541931186e-6`,
`9.855454333597543e-6 -> 1.1692206296377632e-5`, `2.9097423742456926e-4 ->
7.136825317424238e-3`, and the edge and face tolerances derived from them.

Measured on arm64 (Apple container, GCC 13.3):
- Rust's offset result (`do_extended_offset`, before the copy) has the C++ final values
  for every one of those vertices, so the offset agrees with the native one.
- Rust's final `CopyFaces` (with merge) of that result into the emptied B-rep (`RemoveFaces`
  of every face, as upstream) raises them. Both B-reps' tolerance is 5e-6 after the copy
  (`SetTolerance(GetTolerance())` of the offset result), as in C++.
- A standalone `CopyFaces` of the unmodified `nurb_fillet.smb` into an empty B-rep keeps
  every vertex tolerance on both sides (31 vertices, equal to 6 digits), so the copy
  itself is not at fault for that input; its status is `SM_ERR` (1000) in C++ and success
  in Rust (not investigated; the case compares equal otherwise).

Suspected cause: the merge grows a vertex's tolerance in the Rust copy where the native
merge does not, for vertices whose edge curve ends lie further from the vertex point than
the vertex tolerance (as offset results have). Not isolated further.

Other solids (box, pyramid, extrusion, revolution, l_shape_with_boss, box_with_large_fillet,
and the curved ones) agree fully, so `nurb_fillet` is excluded from the generated cases and
kept as the named fixtures, listed in `crates/difftest/known_mismatches/`.
