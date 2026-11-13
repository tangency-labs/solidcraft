# General: a slit PolyFace listed twice by SmPolyBrep::CombineCoincidentVertices (#810, UB-276) has no Rust or difftest counterpart (2026-10-31)

- **Issue:** tangency-labs/solidcore#810, upstream record `plan/upstream-issues/UB-276--polybrep-combinecoincidentvertices-lists-a-slit-polyface-twice.md`.
- **Unit:** none. `SmPolyBrep::CombineCoincidentVertices`, `SmPolyBrep::SqueezeFaceIfDegenerate` and `SmPolyIntersector::CheckForSqueezeVertices` belong to no unit in `harness/callgraph/units.json` (the polygon Boolean is in `plan/closure.md`'s Provably unused table); `query.poly` ports only the PolyBrep queries.
- **Rust behaviour on the trigger:** the operation does not exist in the port. The B-rep `SmBrep::CombineCoincidentVertices` / `SmTopologyIntersector::CheckForSqueezeVertices` (`crates/solidcore/src/boolean/topology_intersector/p2.rs`) return no list of updated faces and squeeze no faces.
- **C++ behaviour on the trigger:** SIGILL from `SmPolyBrep::SqueezeFaceIfDegenerate(this=0x0)` on Linux GCC aarch64 in the fork's prog_test with refined tessellations; passes by chance on macOS; stock 1.0.1 tessellations do not produce the slit.
- **Difftest classification:** no case. There is no shim, FFI binding or Rust function for these symbols, so no case can run either side; no `cpp_defect` marker is needed because nothing is parked. No unit status changes.
- **If the polygon Boolean is ported:** take the fork's `AddUnique` at the three `rFacesUpdated` sites as an unconditional fix (crash class under the #1018 interim policy).
