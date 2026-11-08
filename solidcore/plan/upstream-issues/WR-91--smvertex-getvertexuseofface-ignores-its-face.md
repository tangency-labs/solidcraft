#### WR-91. `SmVertex::GetVertexuseOfFace` ignores its face
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmVertex.cpp:225` `SmVertex::GetVertexuseOfFace` (the face test inside the `// Remove Composite` comment, `:258`).
- **What happens:** The test of the vertex use's face against `pFace` was commented out with the composite face code, leaving its block unconditional: for any non-null `pFace` the function returns the first vertex use (of any face) on its face's upward face use, else the last vertex use with a face use, whichever face it belongs to.
- **Trigger:** a vertex on two or more faces.
- **How found:** read during porting (unit `topology.vertex`).
- **Confirmed:** no (the Rust side is deferred on `SmFace::GetUpwardFaceuse`, `topology.face_p2`).
- **Severity:** wrong result.
- **Rust port:** Faithful (`topology::vertex::vertexuse_of_face`).
