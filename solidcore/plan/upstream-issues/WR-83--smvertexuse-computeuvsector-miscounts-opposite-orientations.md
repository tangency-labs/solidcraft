#### WR-83. `SmVertexuse::ComputeUVSector` miscounts opposite orientations (operator precedence)
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmVertexuse.cpp:993`-`994`.
- **What happens:** `lOppositeCount = FU == OPP ? 1 : 0 + LU == OPP ? 1 : 0` parses as `FU == OPP ? 1 : (LU == OPP ? 1 : 0)`, so the count is never 2: a sector whose face use and loop use are both opposite gets the complementary angle (`EndTan` to `BegTan`) instead of `BegTan` to `EndTan`.
- **Trigger:** an edge vertex use on an opposite face use of an opposite (inner) loop use.
- **How found:** read during porting.
- **Confirmed:** no (the path is deferred in the difftest until `topology.face_p2` lands).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`topology::vertexuse::compute_uv_sector`).
