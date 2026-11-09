# FaceFaceIntersect with faces of two B-reps: GlueEdgesGeneral across B-reps

Unit: boolean.topology_intersector_p1 (PR #1165 runner work).

Inputs: plane_square.smb x sphere.smb (and torus.smb), `SetEditing` on both B-reps, one `EdgeFace` over all edges and both faces, then `FaceFace` with faces f(0,i), f(1,j) (8 cases, temporary names `cand_*`, not committed).

Both sides reach `MakeEdgeInFace` for the interval (Vertex/Vertex ends) in both faces. Upstream then calls `m_pBrep->GlueEdgesGeneral(pNewEdge1, ..., pNewEdge2)` with edges of two different B-reps. C++ continues (state length 36); the Rust runner returns "not an edge of this B-rep" because `brep_glue_edges_general` takes one arena (state length 25).

Suspected cause: upstream only calls this with both faces in one (merged) B-rep; the two-B-rep call is outside the contract and `topology::brep::p7::glue_edges_general` cannot model it. A fixture needs both faces in one B-rep, or the FFI should reject the cross-B-rep case as undefined.
