# sweep.topology_sweep: whole-B-rep DoSweep in SweepStore — stitched and merged results not compared (2026-10-27)

**Unit:** `sweep.topology_sweep` (`SmTopologySweep::DoSweep` with all six target arrays
null, `SmTopologySweep.cpp:628`). The `whole_*` fixtures
(`fixtures/sweep.topology_sweep/whole_brep.json`) sweep an `SmApiCreateBox` solid in
the production `SweepStore` and compare the swept B-rep against C++
(`sc_sweep_topology_sweep_whole`). 13 cases with stitching and merging off pass with
`max_rel_err = 0` (linear z/oblique/x/-y, tiny and large boxes, rotations about y/z/x,
zero repetitions; a full turn and two repetitions compare their error status).

**Reachability.** Only a whole-B-rep sweep reaches `SweepAccess::hcr` (the store's
override), `SweepStore::object_ref`/`store::validate` (through `lock_mark`/`mark`/
`is_marked`) and the interior-face removal. Upstream callers: every
`SmPrimitiveCreation` `DoSweep` passes a non-null array; `SmApiNonManifoldSweep` /
`SmApiNonManifoldRotationalSweep` do it only with no selection and are out of scope
(`plan/closure.md`), and their SM_API tests always pass a selection; `DoAdvSweep`
has no caller. `sm_CreateEdgeForBase` (Rust `TopologySweep::create_edge_for_base`,
and through it `SweepAccess::make_edge_in_face`) runs only under `if (pStrutC)`,
which is always NULL: the only assignment is inside `#if 0`
(`SmTopologySweep.cpp:2083-2099`; the call is at `:2148` under `:2142`). It is dead upstream.

**Fixed here.** The store's `RemoveFaces`, `RemoveAllTopology` and stitch used the
trait defaults, which lend the kernel's notification context inside `with_brep_mut`,
which had already lent it: every whole-B-rep sweep in a `SweepStore` failed with
`InvalidInput("... lent to an edit in progress (nested edit)")`. The store now edits
through the lent context (`remove_faces_notified`, `remove_all_topology_notified`,
`BorrowedStitchEdits`).

**Not compared (open).** Inputs are `[length, width, height, kind, vx, vy, vz,
angle, stitch, merge, repetitions]`.

1. *Stitching on.* Upstream takes the silhouette edges from a pointer hash
   (`SmHCR::GetSilhouetteEdges`, `SmHCR.cpp:312`, ND-2), so the swept topology's
   creation order and the stitch's pairing order follow heap layout. Observed: C++
   `[3,7,2,1,5,0,0,0,1,0,1]` returned `SM_ERR` (1001) inside a `cargo test` process and
   success (1 region, 1 shell, 14 faces, 28 edges, 16 vertices) in the difftest worker.
   Rust (silhouettes in ascending id order) fails in `SmBrep::GlueVertices`'
   `NER(pSurvVShell)` (`topology::brep::p5`, 1006) for `[10,10,10,1,0,0,5,0,1,0,1]`,
   `[3,7,2,1,0,-4,0,0,1,0,1]`, `[1e4,1e4,1e4,1,0,0,5e3,0,1,0,1]`, where C++ succeeds
   (14 faces, 28 edges, 16 vertices): both glued vertices (ids 74, 75 at the origin)
   are on the vertex list but have no vertex uses, so an earlier stitch edit left them
   orphaned. `[1e-3,2e-3,1e-3,1,0,0,1e-3,0,1,0,1]` gives 4 regions/shells (C++ 3);
   rotations `[10,10,10,2,0,1,0,90,1,0,1]` and `[10,10,10,2,1,0,0,30,1,0,1]` differ in
   vertex positions or shell count. `[10,10,10,1,1,2,5,0,1,0,1]` agreed. Next step:
   find which `BorrowedStitchEdits` edit orphans vertices (compare against the plain
   kernel-callback stitch on the same pre-stitch B-rep), then decide which stitched
   cases are order-independent enough to gate.
2. *Merging on* (`[10,10,10,1,0,0,5,0,0,1,1]`): Rust returns `InvalidInput("a retained
   HCR scene cannot be consumed by a Boolean")` from `with_merge`, which registers the
   HCR scene as a merge operand for `CopyFaces`; C++ succeeds (14 faces, 32 edges, 16
   vertices). The store needs a non-consuming merge registration for the scene.

PR: tangency-labs/solidcore#1271.
