Updates: WR-634

#### WR-634 addendum (2026-10-31). The overflow-first stop also notes the blends mitred at a face's concave corner
- **Severity:** Missing feature (wrong result for its callers), as WR-634
- **Area:** Blend (fillet executive)
- **Location:** the fork's `source/SMLib/inc/SmFilletCorner.h` (`SmFilletNx2Corner::m_bMitre`, `IsMitre`, `GetFilletedEdgeuses`), `source/SMLib/src/SmFilletCorner.cpp` (`SmFilletNx2Corner::MakeCornerTopology`, the bevel branch), `source/SMLib/inc/SmFilletExecutive.h` / `.cpp` (`m_vMitredEdges`, `GetMitredEdges`, the overflow-first stop of `CreateFilletBrep`), tangency-labs/smlib@9ba2b12.
- **What happens:** a sloped basin's rim rounded R4 (solidcraft#1080) was refused. The front round overflows across a cliff; the others fit, and each meets its neighbours in a mitre at the pocket's concave corners (WR-642). Overflow first built the front round alone, then rounded the others on what it left, where the right round's rail met the front round's cut beyond its wall (`NoRoom`, "a round's rail leaves the face it rolls on into the body's material"). Two rounds mitred at a face's concave corner are one corner and must be built together.
- **How found:** SolidCraft (#960). 9ba2b12 marks the Nx2 bevel at a corner of mixed convexity as a mitre and, when the executive stops for overflow first, lists the edges of each mitred pair, two by two (`GetMitredEdges`). The policy that takes a round mitred to one that goes first with it is in the fork's C API, outside SMLib, and is not ported.
- **Confirmed:** no C++ run of the fork was made; not reachable in the pinned C++ (`3fd5b95`).
- **Rust port:** `FilletCorner::mitre` / `is_mitre` / `filleted_edgeuses` (`blend/fillet_corner/p3.rs`, set in `blend/fillet_corner/topology.rs`), `FilletExecutive::mitred_edges` and the stop's loop over the corners (`SolverGeometryAccess::geometry_mitred_corner_edges`, `blend/fillet_executive/p3.rs`), `FilletSession::mitred_edges` (`blend/fillet_executive/session/overflow_first.rs`). Per session, as WR-634's hooks; no `UpstreamFixes` field (the list is filled only at the overflow-first stop, which only callers that set it reach).

  Tests: `crates/solidcore/tests/wr634_mitred_edges.rs`: WR-642's pocket rim chamfered 3 with classify only lists its four corners' pairs of rim edges with `fillet_mitre_concave_face_corners`, none without it.
