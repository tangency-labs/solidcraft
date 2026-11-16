# intersect.curve_class_p5 to green: B-rep cases (PR #610, 2026-10-23)

- **C++ raises a vertex tolerance while computing a vertex/face gap; the port does not
  (`topology.vertexuse`, not a part-5 disagreement).** Found by the part-5 B-rep runner:
  a box whose vertices are moved off their faces by `1e-4` (raw `m_vPoint` edit), vertex
  tolerances `1e-5`, face tolerances default, `IntersectVertices` with `bHasBigGaps`.
  `SmVertexuse::GetVertexFaceGap` -> `CalcVertexFaceGap` -> `ComputeUVPoint(FALSE)`: the
  local projection lands farther than vertex + face tolerance, so upstream falls back to
  `smsurf_GetSurfaceCache(face surface)` + `GlobalPointSolve`. After the call C++ has the
  moved vertex's tolerance at `2e-4` (twice its distance to the edge ends); Rust keeps
  `1e-5`. Same input with `bHasBigGaps = FALSE` (no gap computed) agrees. Suspected cause:
  building the face's trimmed surface cache validates its edges, and
  `SmEdge::ValidateGeometry(TRUE)` (SmEdge.cpp:6188, 6213) sets an end vertex's tolerance
  to twice its gap; the port has no object caches. Not confirmed by stepping the C++.
  The part-5 generator keeps such projections local (face tolerances `1e-3` whenever it
  moves a vertex); the gap value itself still agrees.
- **Large-gap vertex insertion of `IntersectVertices` is not reached.** Its curve point
  solve passes `dZero` as the distance limit and `XSectTol3d` as the tolerance; in every
  fixture and generated case (vertices up to `1e-4` off their faces, curves `6e-5` from
  the vertex) it returns no drop point beyond `XSectTol3d`, so the projections onto the
  (owner-cleared) face surface run in no case. The large-gap tolerance itself
  (`sBigGapXSectTol3d` from real vertex uses) is computed and compared.
- **The SSS cull's removal branch is not reached**: two drop points of one vertex that
  snap to the same knot. A cubic loop near a vertex gives two drop points, but a Bezier
  loop's two parameters straddle its span, so they snap to different knots.
