#### UB-222. Size constructors and setters write through a failed `smos_Calloc`
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `source/SMLib/src/SmMatrix.cpp:20` `SmMatrix(ULONG, ULONG)` and `:143` `SmMatrix::SetSize` (`smos_Calloc(1, sizeof(double)*rows*cols)` unchecked, product can wrap, then the row-pointer loop writes `m_ppdM[ii]`); `SmSolutionArray.cpp:1076` `SmSolutionArray::SetSize` (calloc result unchecked, then `smos_MemCpy`/writes; the `nNewMaxSize *= 2` loop never ends for a size above `2^63`, and `nNewMaxSize * sizeof(SmSolution)` can wrap) and `:1448` `InsertAt` (`nIndex + nCount` unchecked, then `smos_MemMove` and slot writes); `source/SMLib/inc/SmLocalSolve1d.h:159` `SmIterQueue1d(ULONG)` (`SetSize` refuses or fails softly, `Add` then writes `m_vArray[0]` of an empty array).
- **What happens:** For a caller-chosen size that cannot be allocated the object keeps a NULL buffer (or a too-small one after a wrapped product) and the next statement writes through it.
- **Trigger:** `SmMatrix(2^31, 5)`, `SmSolutionArray::SetSize(2^31)`, `InsertAt(2^32, s, 2^32)`, `SmIterQueue1d(2^31)` then `Add`. Upstream's own callers use small sizes (`SmIterQueue1d(8)`, matrix sizes from curve/surface degrees).
- **How found:** generated difftest case (fuzz sweep: the Rust ports aborted on these sizes in `geom.matrix`, `geom.solution_array`, `geom.local_solve1d`, `blend.fillet_custom_solver`; upstream source then read).
- **Confirmed:** no. The existing FFI limits (`geom.matrix` `MAX_DIM` 64, `geom.solution_array` `MAX_SIZE` 4096, `geom.local_solve1d` queue size 1..=1024) kept these sizes from the C++ side.
- **Severity:** crash/UB (NULL write, size wrap) and hang (`SetSize` above `2^63`). Not file-reachable.
- **Rust port:** `Matrix::try_new` / `Matrix::set_size`, `SolutionArray::try_set_size` / `try_insert_at` / `try_with_capacity`, `IterQueue1d::try_new` return `InvalidInput` with the object unchanged.
