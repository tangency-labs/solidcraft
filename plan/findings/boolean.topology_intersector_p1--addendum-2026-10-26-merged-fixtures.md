Updates: plan/findings/boolean.topology_intersector_p1--cross-brep-glue-edges.md

Whether C++ is defined for the cross-B-rep `GlueEdgesGeneral` was not established (no ASan run), so no FFI guard was added for it. The committed fixtures take the route `SmMerge` takes: a `MergeBreps` runner op (shim op 35, `SmBrep::MergeBrep`) puts both operands into the primary B-rep, then `SetEditing`, `EdgeFace`, `FaceFace` on faces of that one B-rep. `merged_*` cases (plane_square or disk x sphere, cone, torus) reach `MakeEdgeInFace` in both faces and agree with C++ at 1e-9.
