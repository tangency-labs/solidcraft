Updates: UB-170

#### UB-170 (update, 2026-10-29). Native bottle sweep reaches the out-of-bounds edge query

- **Trigger:** Pinned v1.0.1 C++ on Linux x86_64 reads `TestFiles/pt_TestFiles/Fillet/bottle.smb` and sweeps the whole B-rep by translation `(0, 0, 4)`, with stitching and merge disabled and one repetition. The loaded model returned `SM_SUCCESS` from level-2 `ValidatePointers` and true from `AssertValid` before the sweep.
- **Confirmed:** The unmodified reference crashed in 3/3 fresh child processes. A debugger placed the crash in `SmTol::GetZoneTol3d`, called from `SmHCR::IntersectCurveWithSilhouettes`: the vertex query held 89 objects, the edge query 49, and the vertex count was used to index the edge array. These checks establish the cited pointer/object validity, not every geometric invariant.
- **Diagnostic limitation:** A private native build that changes the edge-copy bound to the edge-query size returned success in 10/10 runs, but the resulting B-rep failed `AssertValid` in 3/3 additional runs. Correcting the bound alone is not a validated fix for the complete bottle sweep; the native probe is not the difftest reference.
- **Rust port:** When there are more queried vertices than edges, process the available edges instead of returning `InvalidInput` or reading out of bounds. When the vertex count is no greater than the edge count, retain the existing vertex-count limit and order, including upstream's skipped-edge behavior. A focused open-chain regression covers the formerly rejected path; native guard coverage remains unchanged.
