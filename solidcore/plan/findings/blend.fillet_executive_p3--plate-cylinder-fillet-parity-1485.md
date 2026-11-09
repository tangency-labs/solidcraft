# blend.fillet_executive_p3: the plate-and-cylinder fillet of #1485 fails the same way in pinned C++ (2026-10-31)

**Unit:** `blend.fillet_executive_p3` (corner vertices: `SmFilletVertex::CalcRailIntRail` and `SmFilletSolver::RailRailIntersect`).

**Input.** The plate 100 x 100 x 10 at the origin joined with the cylinder r 20, h 20 whose base centre is (100, 50, 10). This is `SmApiBooleanUnion(box, cylinder)`, 12 V / 19 E / 9 F on both sides. Every edge is then filleted r 2 (tangency-labs/solidcore#1485). It was run on the pinned C++ (v1.0.1, `3fd5b95`) and on the port, through the probe `probe_1485_plate_cylinder` and harness source kind 5 (box and cylinder union) in `blend.fillet_executive_p3`'s end-to-end shim.

**Both values.**
- **`SmApiCircularFillet(2)` on each side's own in-memory union** (the issue's path): both return **1001** at `CreateCornerVertices`.
  - C++: 31 of its 32 rail/rail solves succeed. The 32nd, at the vertex (100, 70, 10) on the cylinder (guess (31.415926536973156, 0)), finds no intersection, and `CalcRailIntRail` returns SM_ERR.
  - The port: 31 of its 32 rail/rail vertices are processed. The 32nd, `VertexId(32)` of corner 9 at the same vertex (guess (31.415926536973153, 0)), ends `SolverNotConverge`.
  - The two guesses are 1 ULP apart (`0x403f6a7a2959d696` against `…d697`). Both solves fail regardless.
- **`SmApiFilletEdges(every edge, 2, 1, 1, 1.0)` on the same body read from an ASCII `.smb`:** both return **1007** before any corner is made. This is a different call from the one in the artifact below. The vertex-6 traces on the file body came from `circular_fillet`, which does reach the corner vertices. Pinned C++'s `circular_fillet` was not run on the file body, because the issue's path is in-memory.

**What the issue reported.** The issue said the pinned C++ gets through `CreateCornerVertices` and fails later, in the corner-edge loop, while the port fails earlier. That doesn't hold for the pinned build on this body. The openscad-occt reference that the issue compared with must have run a different body (its union) or a different call.

**An artifact on the way.** Comparing a Rust body that had been written to and read back from an ASCII `.smb` with the C++ in-memory union showed a divergence at the 6th vertex, (100, 30, 10):
- the port failed it and the C++ solved it;
- one UV trim curve point differed by 1 ULP (89.967899326421957 against 89.96789932642197), which flipped the first Newton step of a near-singular rail/rail system;
- on the same in-memory body the guesses there are bit-identical (`0x40578fdb9f0111d5`) and both sides solve that vertex.

So an ASCII `.smb` round trip of this union is not bit-exact for at least one UV trim value. That is worth checking separately (the writer's float format).

**Status.** The default path is at parity, and there is no port change. A difftest case for the in-memory union (kind 5, `SmApiCircularFillet(2)`, comparing status and topology) should lock this in.
