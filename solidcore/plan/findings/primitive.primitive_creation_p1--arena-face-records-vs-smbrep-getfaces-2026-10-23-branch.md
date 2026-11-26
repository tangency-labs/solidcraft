# primitive.primitive_creation_p1: arena face records vs SmBrep::GetFaces (2026-10-23, branch claude/edge-handles)

- Once the curve-solver `IdenticalSolutions` reads curve natural intervals (ParameterDomains,
  same branch), `linear_circle_cap1_distance0` and `linear_analytic_circle_cap1_distance0`
  (CreateLinearSweep of a full circle, distance 0, capped) run to completion. The runner
  counted faces as `brep.faces().len()` (every live face record): Rust 4, C++
  `SmBrep::GetFaces` 2. Two of the four Rust face records have no face use (`faceuse =
  None`, shells 0 and 6), so a traversal does not reach them. The runner now counts with
  `topology::brep::p1::faces/edges/vertices` (the `GetFaces` / `GetEdges` / `GetVertices`
  ports), as the shim and the shape <= 1 branch already do; both cases pass (2 faces, 1
  edge, 1 vertex) and every former pass is unchanged.
- Open question for the primitive porters: the two unreachable face records are made by
  the capped degenerate sweep before `Stitch` (they exist before it). Upstream probably
  deletes such a face on its failure path; Rust leaves the record in the arena. Not
  observable through any compared output today.
