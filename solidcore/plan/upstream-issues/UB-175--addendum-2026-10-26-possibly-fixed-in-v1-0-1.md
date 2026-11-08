Updates: UB-175

#### UB-175 (addendum). Possibly fixed upstream in v1.0.1
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Change:** in usd-brep v1.0.1 (3fd5b95), `SmProtoTopologyManager::~SmProtoTopologyManager` (`source/SMLib/src/SmProtoTopology.cpp`) first sets every proto edge's start and end proto vertex to NULL and empties every proto vertex's proto-edge list. Only then does it run the `SmObjsDelete` teardown of the proto vertices and edges. The new comment reads: "a ProtoVertex can still list a ProtoEdge that has moved to another ProtoVertex, and its destructor would then write into a deleted ProtoEdge". Also in 1.0.1, `SmProtoEdge::CombineProtoEdge` moves every edge definition and hands its coincident proto faces to the surviving proto edge, so a deleted proto edge is no longer left listed.
- **Status:** read from the v1.0.1 source only. No run has verified it: the UB-175 inputs (`gen0047`, `gen0059`, `gen0066`, `gen0071`, `gen0150`, `gen0201`, `gen0235`, `gen0239` and `ub175_after_editing_imprint_failure`) have not been run against the 1.0.1 reference under AddressSanitizer. The difftest cannot run them as they are, because the `boolean.api_brep` runner's UB-175 pre-check keeps those inputs away from the C++ side.
- **Rust port:** follows v1.0.1 since the reference pin moved there. The port has no teardown use-after-free either way.
