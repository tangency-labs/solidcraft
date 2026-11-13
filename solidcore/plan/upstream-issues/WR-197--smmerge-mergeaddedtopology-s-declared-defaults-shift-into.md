#### WR-197. `SmMerge::MergeAddedTopology`'s declared defaults shift into the wrong flags
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/inc/SmMerge.h:357` (declaration) against `source/SMLib/src/SmMerge.cpp:5304` (definition), `SmMerge::MergeAddedTopology`.
- **What happens:** The declaration puts `bDoGluing = TRUE` ninth, followed by `bMakingManifoldSolid = FALSE`, `bFastEdgeCompare = TRUE`, `bDoRegionNesting = TRUE`, `bIgnoreProblems = TRUE`. The definition takes the four flags ninth to twelfth and the unused gluing flag last. A call relying on the defaults therefore runs with making-manifold-solid `TRUE` and fast edge compare `FALSE`, the opposite of what the declaration documents.
- **Trigger:** Any call that omits the trailing arguments. Both upstream callers (`MergeAddedFace`, `SmFeatureExecutive`) pass all four flags explicitly, so they are not affected.
- **How found:** read during porting (`boolean.merge_p2`).
- **Confirmed:** no (no upstream caller uses the defaults).
- **Severity:** wrong result (latent).
- **Rust port:** `MergeAddedOptions::default()` is what the declared defaults actually pass, `(TRUE, FALSE, TRUE, TRUE)`; `MERGE_ADDED_FACE_OPTIONS` is `MergeAddedFace`'s explicit `(FALSE, TRUE, TRUE, TRUE)`.
