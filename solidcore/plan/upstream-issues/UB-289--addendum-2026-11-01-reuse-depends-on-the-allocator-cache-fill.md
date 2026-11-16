Updates: UB-289

#### UB-289 (update, 2026-11-01). Address reuse observed: which merges follow the survivor depends on the allocator's cache state, not on the healer
- **Now observed:** the C++ reference build's heal of the trigger body (STEP Tools corpus `ap203/gehaeuse.stp`, body 0) was traced under a debugger, with breakpoints on symbols in the release build (probe batch 54k-round1 follow-up). The trace logged each `Fix_CoinVertices` merge, the vertex props `UpdateTargetLists` frees, the 0x68-byte allocation of the survivor's new `SmVertexProps`, and any allocation in between.
- **What the trace shows:**
  - `UpdateTargetLists` frees the survivor's props first, then the deleted vertex's, and then allocates the survivor's new props. Nothing else is allocated in between.
  - **Top-level `Fix_CoinVertices` (step 7), 4 merges:** each time, the new props get the deleted vertex's freed address, the last one freed. The loop then rewrites the later pairs from that address to the survivor's old address, which is freed, so later pairs that named either vertex are skipped.
  - **Catch-up `Fix_CoinVertices` inside `Fix_DegenFaces` (step 9), 4 merges:** each time, the new props get the survivor's own freed address, the first one freed. The later pair that named the deleted vertex is then live and is merged. Two survivors each absorb a second vertex this way, which is the 2-vertex difference in this record.
- **Why the two differ:** the healer's own sequence is the same in both places. A 104-byte block comes back from glibc's per-thread cache, which hands out its most recently freed block, until that cache's bin is full; once full, a free goes to a different list. Which freed address the next allocation gets therefore depends on how many blocks of that size class the whole process had freed before. That count is not visible to the healer. This mechanism is read from glibc's documented behaviour; it was not traced.
- **Consequence for the port:** a deterministic rule cannot reproduce both results.
  - Always taking the survivor's current props (the opt-in proposed for this record) also merges the step-7 pairs C++ skips. On this body it gives 87/220/140 after step 9, against C++'s 87/226/141.
  - Never following it (the port's default) skips the step-9 pairs.
  - Reproducing C++ would mean modelling glibc's allocator. The opt-in is withdrawn, and the default stays.
- **Confirmed:** yes (observed on the C++ reference build).
