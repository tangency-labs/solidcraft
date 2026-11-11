Updates: UB-289

#### UB-289 (update, 2026-11-01). A second trigger: STEP Tools corpus `ap203/rear.stp`, body 5, on the stitch recipe
- **Trigger:** `ap203/rear.stp`, body 5, built with the plain `--step-corpus` recipe (`CreateFacesWithCurves`, `SmApiStitchIntoSolid`, `SmApiHealBrep`). The first trigger (`ap203/gehaeuse.stp`, body 0) was on the `SmBrepConstructor` recipe.
- **Where:** a heal-step bisection of the plain-corpus HEAL-DIFF bodies (batch 54k5) found the first difference at `Fix_DegenFaces` (step 9).
  - Faces/edges/vertices after step 9: C++ 40/98/59, port 40/99/60.
  - Before the heal, and after step 7, both kernels have 43/106/64.
- **Traces:**
  - C++ (debugger, release build, symbol breakpoints): the top-level `Fix_CoinVertices` merges nothing. `Fix_DegenFaces`'s `UpdateTargetLists` frees four vertices' props and makes new props for one. The catch-up `Fix_CoinVertices` then merges two pairs that share a survivor. At the first merge the survivor's new props get the survivor's own freed address, so the second pair is live and merged.
  - Port (`PROBE` trace, probe batch 54k8): the same three squeezes, the same single target update (four vertices removed, one re-added), and one catch-up merge with the same survivor and deleted vertex. The second pair names a retired props id and is skipped.
  - That one skipped merge is the whole difference: one vertex and one edge.
- **Trace note:** the shim's output-buffer retry runs the C++ build twice in one process. The trace shows the same sequence twice, with the same address reuse both times.
- **Rust port:** unchanged. The default stays deterministic, and no opt-in is planned (see the 2026-11-01 addendum on the allocator's cache fill).
- **Confirmed:** yes (observed on the C++ reference build).
