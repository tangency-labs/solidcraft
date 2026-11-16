Updates: plan/findings/sweep.topology_sweep--whole-brep-sweep-store-stitch-and-merge.md

# sweep.topology_sweep: whole-B-rep DoSweep with stitching and merging now compared (2026-10-27)

**Unit:** `sweep.topology_sweep`. Twelve `whole_stitch_*` / `whole_merge_*` cases join
the `whole_*` fixtures (`fixtures/sweep.topology_sweep/whole_brep.json`); the C++ side
of every `whole_*` case now runs in a fresh forked child (`solidcore_ffi::isolate::fresh_process`).

1. *Stitching on: the orphaned vertices.* `GlueVertices` failed (1006) on vertices
   without vertex uses because every `MakeVertexSplitEdge` the stitch made
   (`Stitch::edit_split_edge` through `BorrowedStitchEdits::split_edge`) failed part-way:
   the runner's `SweepIdentities` had no `new_curve`, so the store refused the split
   curve's identity after `MakeVertexSplitEdge` had already allocated the new vertex,
   and `sm_TestAndSplitEdge` treats any split failure as "close to boundary" and goes on,
   leaving that vertex in the B-rep without uses. The stitch edits themselves were
   right. The runner now names a split curve by its edge, as every curve there
   (`WholeIdentities::new_curve`), and reserves the merge's source lists
   (`WholeIdentities::source_lists`). Open (Rust-only hazard, not reached by any
   production caller with identities): a caller identity refusal inside a split is
   still swallowed by `test_and_split_edge_with` like a native split failure, leaving
   the partially built vertex; upstream has no such failure.
2. *Merging on.* The refusal was not about the HCR scene: `PiecewiseMerge`'s
   `ManifoldBoolean` (`SmTopologySweep.cpp:1346`, `SmMerge(pBrepToSweepInto, pNewBrep)`)
   gets the caller's own B-rep as its primary, which the store holds borrowed, and the
   store's Boolean driver accepted only owned operands. Upstream edits
   `pBrepToSweepInto` in place; the driver now moves a borrowed operand's content out for
   the Boolean and back into the caller's B-rep afterwards (`take_operand` /
   `restore_body` in `store/merge.rs`), still refusing a retained HCR scene. A Boolean
   that deleted a borrowed body is reported as a cleanup error.
3. *Canonical order.* `whole_output` sorted vertex points exactly, so a coordinate of 10
   computed as 10 + 2e-15 on one side reordered the record
   (`[10,10,10,2,0,1,0,90,1,0,1]`, same geometry). The order now uses coordinates
   quantized to 1e-7 of the largest coordinate (then exact); values are compared
   unchanged at 1e-9.
4. *C++ nondeterminism.* With the above, the C++ results of six inputs still change
   between processes with heap layout alone (ND-491: the silhouette edges come from a
   pointer-keyed map). For each of them the Rust result equals one of the observed C++
   results; they are parked with `cpp_defect = "ND-491"`. The other six (translations
   along z and oblique with stitch, merge, or both; the 1e4 box with stitch) gave one
   C++ result in 200 heap-varied runs each and pass with values compared.
