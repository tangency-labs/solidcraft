# topology.proto_topology_p1 (`SmOwningTopology`, `SmAssembly`, `SmCopyBrepMap`, `SmProtoTopology` part 1, API transforms, 2026-10-14; partial, no disagreements)

- **Harness.** One shim entry runs scripts of three kinds (list, proto, transform); the
  runner interprets the same script against the port and applies the shim's run-time
  refusals to its own state, so an op the shim refuses as undefined upstream (null or
  deleted pointer dereference, use after free in a destructor, a list walk that would
  not end) ends both records with `-2`, and a state divergence shows up as a mismatch.
  Proto ops are `[code, args...]` arrays exactly as the shim reads them.
- **Upstream defects found:** UB-98 (`SmCopyBrepMap` release asserts call `GetType()`
  on null objects; crashed the reference run), HANG-12 (`DropPoint` never returns after
  `Reparameterize` to a zero-length domain; the Rust curve point solve loops the same
  way, which belongs to the curve units), WR-73 (`RemoveCoincidentNoEdgeProtoVertex`
  returns `SM_SUCCESS` as TRUE; ported for parity). The shim refuses the UB-98 calls and
  zero-length `Reparameterize`.
- **Deferred** (`Unported`): the B-rep walks of `sm_AreConnected` and point
  classifications on bare topology objects (topology.vertex, topology.edge,
  topology.face_p2), `SmProtoVertex::ContainsPoint` (query.face_p2), surface point
  solves (surface.surface_p4), `SmBrep::Transform` (topology.brep_p5) and
  `SmPolyBrep::Transform` (query.poly).
- **Co-port wired:** `topology::edgeuse::p1::owning_topology_is_kind_of` (CO-PORTED in
  topology.edgeuse_p1, PR #55) now calls `owning_topology::is_kind_of`; edgeuse_p1
  difftest unchanged (2449 pass, 273 deferred).
- **PR #144 review (2026-10-16).** The B-rep walks of `sm_AreConnected`,
  `SmProtoVertex::GetFinalVertex` and `sm_FindEDOnDegenFace` over a real `topology::Brep`
  (`impl ProtoTopology<EntityRef> for Brep`) are not compared against the C++: the
  runner's pool and the C++ stand-ins have no vertex uses or edge uses, so across kinds
  `are_connected` is only ever false (true for the same object), vertex-edge pairs are
  refused (`connect_crashes`) and `GetFinalVertex` only finds a vertex whose edge
  definition source is that vertex. Their connected branches are checked only by the
  unit test `brep_walks` and were read line by line against `SmProtoTopology.cpp`
  336-458 and 2133-2223 (they match). Follow-up: compare them on a real B-rep imported
  as topology.edgeuse_p1 does from `.smb` files. `sm_FindEDOnDegenFace` is unreachable
  from `CanCombineEdgeDefinitions` (two different edges are never connected), so
  `find_ed_on_degen_face` / `degen_face_choice` are not compared either; as a global C++
  function it could get a direct op on bare edges. Review fixtures added:
  `review_poly_transform.json` (216 poly B-rep transforms: 0-4 vertices, zero / negative
  / 1e300 / 1e-310 tolerances, 1e200 and 1e-300 coordinates, negative translates,
  near-identity, EFF_ZERO, 1e300 and non-uniform scales, scale by point, 0/360/-45/1e10
  degree rotations, a zero axis, flipped and non-orthonormal placements) and
  `review_final_vertex.json` (4 scripts at scales 1, 1e8, 1e-8, 1e3: GetFinalVertex,
  every non-crashing AreConnected pair and every CanCombine pair); all bit-identical.
