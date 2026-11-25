# topology.vertex: vertex/face gap of a sphere or torus vertex moved 1e-5 off its faces (2026-10-29)

- **Unit:** found by the `Corner` op of `topology.vertex` (PR #1478, #1419 WP4a:
  `sm_AssertTestVertex1`). The differing value is the one the existing gap code
  computes (`vertexuse::vertex_face_gap`, via `SmVertexuse::GetVertexFaceGap` and
  `ComputeUVPoint`), not one of the new corner functions.
- **Cases (9):** `corner_{sphere,torus,toruswvy}_v{0,1,2}_off1em5` in
  `fixtures/topology.vertex/corner.json`. Upstream `sphere.smb`, `torus.smb` and
  `toruswvy.smb` vertices 0-2 (pole and seam vertices) are moved by `(1e-5, -1e-5, 2e-6)`
  with `SmVertex::SetPoint`, and then `IsWithinXSectTol3dOfFaces` runs.
- **Values (`state[2]`, the max vertex/face gap; `state[23]` repeats it):**
  - sphere: C++ `1.1007145606666338e-5`, Rust `1.0198037066258565e-5`;
  - torus: C++ `1.4142135907336496e-5`, Rust `1.4282856856820645e-5`;
  - toruswvy: C++ `1.4104227772077376e-5`, Rust `1.4282856860970198e-5`.

  The pass/fail flags and every other value agree. The 1e-3 and 1e-7 moves of the same
  vertices also agree.
- **Observation:** on the tori the Rust gap is exactly the length of the move
  (`|d| = 1.42829e-5`). The C++ gap is close to its tangential part only (`sqrt(2) *
  1e-5`). The Rust side seems to measure from the UV point found before the move, where
  the C++ re-solves it.
- **Suspected cause:** the UV point or lazily created UV trim used for a seam or pole
  vertex use after `SetPoint`. This may be the same family as
  `plan/findings/topology.assert_valid--sphere-moved-pole-lazy-trim.md`. Not resolved.
- **Status:** the 9 cases are listed in
  `crates/difftest/known_mismatches/topology.vertex.toml` (a Rust divergence, no
  `cpp_defect`).
