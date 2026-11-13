# Issue #489: retain intersection UV curves at face/face edge insertion

`boolean.topology_intersector_p1`, `SmTopologyIntersector::FaceFaceIntersect`:
the pinned C++ passes `NULL` as the UV curve to `MakeEdgeInFace` for both faces
(`source/SMLib/src/SmTopologyIntersector.cpp:8246,8294`), even though the face
classifications retain the intersection UV curves. The Rust counterpart was
`TopologyIntersector::make_edge_for_interval`, which likewise passed `None`.
It now copies the classification's current `uv_curve1()` alongside its 3D curve
and supplies that independent copy with the same parameter interval. Missing or
stale UVs retain the projection fallback. This deliberately differs from the
pinned C++ handoff; it removes an additional projection fit, not the intersection
algorithm's own approximation error. No tolerance is changed.

The other stages already retain supplied UVs: `topology::face::p3::face_intersect`
attaches the trace UV handles to both classifications; proto-edge creation copies
and trims both UVs; `topology::brep::p9::make_edge_in_face_with_vu` projects only
when the supplied trim is absent and otherwise attaches it (with seam copies).
Pinned C++ `SmEdgeDefinition::Imprint` already forwards both UVs.

Validation limits: this is a latent Rust loss, not a reproduction of the reported
2–4e-6 threaded-shaft gaps. The handle insertion adapter still defers at
`SmFace::FindVertexSector`, and proto-edge imprint still defers at its
`MakeEdgeInFace` adapter. The new tests cover independent curved UV copies for
two faces, non-unit domains and subintervals, analytic kind preservation,
missing/stale UV fallback, and forwarding to both vertex sectors and insertion
in both orientations. They do not certify successful boolean insertion or
periodic seam behavior. Issue #489 remains open for end-to-end reproduction
after those adapters are wired. Existing parity cases must retain their original
comparison rules; a future completed explicit-null C++ path may differ by design.
