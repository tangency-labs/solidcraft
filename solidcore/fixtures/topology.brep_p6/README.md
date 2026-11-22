# Closure traversal fixture

`open_box.smb` is ASCII SMLib topology generated from the Apache-2.0 input
`upstream/TestFiles/pt_TestFiles/Solids/box.smb` (upstream `c9c5979`). It is an
actual five-face sheet, not an operation-result snapshot. The native file reader
constructs it afresh on both sides; Rust imports only its raw input graph.

Generation used the reference kernel and the shared fixture builder:

```cpp
const double input[] = {0, 0, 0, 0}; // file, no rebuild, no validation, no gaps
Rd rd{input, 0};
Built b;
SmStatus status;
b.brep = build(rd, "upstream/TestFiles/pt_TestFiles/Solids/box.smb", status);
// Check b.brep and status == SM_SUCCESS.
b.brep->m_bEditingEnabled = TRUE;
Graph g;
walk(b.brep, g, true);
status = b.brep->DeleteFace(g.faces.at(0)); // Check SM_SUCCESS.
status = b.brep->WriteToFile("open_box.smb"); // Check SM_SUCCESS.
```

The exported graph has one region, one shell, five faces and ten faceuses. Seeds
0 and 5 are the two sides of the face opposite the hole. Both mates share a shell
and neither seed face has a lamina edge. Traversal reaches the hole through the
neighboring faces and returns open, before allocation or missing dependencies.
For one call with no caller lock, C++ advances MARK by four and releases its lock;
two calls advance it by eight. The 56 cases compare both seeds, seven caller-lock
masks, one/two calls and both infinite-region flags, including all faceuse marks
and lock state. The native FFI regression checks that these inputs still reach
traversal rather than an earlier no-op return.

# Face construction fixture (`make_face.json`, op 3)

Each case builds a kind-5 fixture block: shell vertices at the listed points
(`MakeShellVertex`), joined by `MakeEdgeTopology` edges carrying straight
`CreateLineSegment` curves, exactly as the wire network of `my_test_topology`.
It then runs `SmBrep::MakeFaceTopology` once per `faces` entry, each on the B-rep
the previous call left. Loops are given as network point pairs. The runner turns
each pair into the edge use running from the first point to the second in the
imported pre-operation graph, so both sides pass the same objects.

The 89 cases cover:

- Single loops on planes and B-spline surfaces (flat and saddle bilinear
  patches), in both orientations, with and without `bCalculateTrims`,
  `bVerifyRadialSectors`, an explicit region and caller UV curves.
- Holes, whose inner wire shell is merged into the keeper shell by
  `RemoveShell`, region `Remove` and delete.
- Loop vertices that are a shell vertex (its shell is deleted), on a wire of
  another shell (`TransferChildrenTo`), or in the keeper shell.
- Shared edges, an open box built from one to five faces and the closed cube.
- A three-page non-manifold book and a slit face followed by a fin, with and
  without radial-sector verification, and an edge used three times in one loop.
- Degenerate inputs: no edges, a NULL surface (also after a successful face),
  zero-length loops, misordered or open chains, and a one-edge loop.
- Scale and tolerance limits: squares from 1e-6 to 1e3, a corner off the plane by
  1e-7 to 1e-4, and squares away from or tilted against the plane.

The comparison covers every call's status and outputs, the context mark counters
and locks, and the whole post-operation graph. The graph walk is the
`topology.brep_p10` one; its export carries no `SmTopology` flags and no edge
marks, see `plan/findings.md`.
