# topology.proto_topology_p3: the UB-241 stop in GlueCoincidentBrepVertices was broader than the defect (2026-10-29)

**Unit:** `topology.proto_topology_p3` (`ProtoTopologyManager::glue_coincident_brep_vertices`),
found through `boolean.api_brep` case `crossing_creases_union_d1e-05`. Until the v1.0.1 pin the
case was kept from the C++ side by the `boolean.api_brep` UB-175 guard.

**Values.** Both sides succeed; C++ returns 12 faces, Rust 13 (`values.measures.len`: cpp=15
rust=16).

**Cause.** The UB-241 stop (SolidCraft fix, tangency-labs/smlib da8d05e) was broader than the
defect: the defect is on the `GlueVertices` path; the delete branch is safe, so the port now
stops only before `GlueVertices`. With the broader stop the port skipped edge deletes that C++
makes, and one extra edge was related and kept, splitting a face. An ASan run of this case on
v1.0.1 is clean.

**Fix.** Unit test `glue_deletes_a_second_short_common_edge_after_the_squeeze`;
`glue_stops_once_the_second_vertex_is_glued_away` is unchanged.
