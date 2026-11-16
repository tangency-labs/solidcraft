# sweep.topology_sweep: nm_test_rotation_neg_y_36 sweeps two near-coincident arcs into the final union (2026-10-27)

**Unit:** `sweep.topology_sweep`. Case `nm_test_rotation_neg_y_36` (fixture
`fixtures/sweep.topology_sweep/nonmanifold_body.json`, body input
`[1, 2, 0, -1, 0, 36, -10, 0, 0, 0.0001, 0, 0, 1, 0, 0, 0]`): the prog_test test body
(`my_create_test_brep`) swept whole by 36 degrees about the axis (0,-1,0) through (-10,0,0),
eps 1e-4, without stitching or merging. `DoSweep` copies the wire edge and shell vertex
into a separate B-rep, sweeps it, and unions it into the swept body
(`SmTopologySweep.cpp:1394-1425`).

**Values.** C++ returns SM_ERR (400/400 heap-varied processes). Before the HCR
silhouette-order fix (fix-hcr) Rust succeeded (1 region, 15 faces, 66 edges, 63 vertices);
with it Rust returns the same SM_ERR, so the case passes on status and leaves the
known-mismatch list.

**What the traces showed** (gdb, C++ v1.0.1 `-O2 -g` and Rust opt-level 0; the probe
scripts are not committed):
- The swept body that enters the union carries **two near-coincident start vertices**
  for the arc swept by the wire's start vertex: (10,0,0) and (9.9999999999999964,0,3.9e-31),
  about 4e-15 apart and never merged. Each has its own arc to the **same** end vertex
  (6.18034,0,11.7557).
- In the union's `SetRelationships`, the wire sheet's arc is related to the first Brep arc
  and then to the second. The second `Relate` hits its conflict exit
  (`SmTopologyIntersector.cpp:5328`, the Other edge already has a Brep mate) and returns
  SM_ERR. `SetRelationships` ignores the status. This is 1 conflict out of 90 C++ `Relate`
  calls.
- **Rust conflicts the same way:** `TopologyIntersector::relate` returns SM_ERR at its
  `pBrp` check (`boolean/topology_intersector/p1.rs`, Brep edge 30 against Other edge 3,
  which is already mated to Brep edge 26), and `set_relationships` ignores it too.
- C++ then fails in `SmMerge::MakeFaceuse`, after 27 `FindRadialSector` calls; the region
  lookup succeeds. Rust's `make_faceuse_attempt_with` succeeded before fix-hcr. The
  exit was not traced further, because fix-hcr made Rust fail the same way: the earlier
  Rust success came from the wrong silhouette order, not from a better union.
- Pre-fix Rust result, checked by a Rust validator test (not committed):
  `integrity::check` and `validate_pointers` (level 4, walk) pass, but the arc swept by
  (10,0,0) is **two** edges, `EdgeId(26)` used by 2 faces and `EdgeId(30)` by 1. That is
  the same near-duplicate shape, carried through instead of failing.

**Cause.** Both kernels sweep two vertices 4e-15 apart into two arcs with a shared end
vertex. With a single start vertex, both arcs would be one edge, and the union would
relate the wire arc once. The 1:1 port reproduces this faithfully. No C++ defect is
claimed: the near-duplicate comes from the body construction and sweep, and C++ handles
it consistently (SM_ERR).

**Candidate improvement (not a parity issue).** This is a place where Rust could do better
than C++ later: merge vertices that coincide within tolerance, together with arcs that
share both ends, before the union (or at sweep time). The union would then succeed with a
valid solid. It would be a deliberate divergence from C++, recorded as such. Where the
4e-15 vertex is created (the body's TSurf face corner versus the vertex the wire edge
starts at) was not traced.

**PR:** ships with the fix-hcr PR (fixer-3's silhouette-order fix).
