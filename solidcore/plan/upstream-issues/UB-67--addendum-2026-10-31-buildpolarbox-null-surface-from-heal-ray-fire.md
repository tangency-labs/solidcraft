Updates: plan/upstream-issues/UB-67--surface-helpers-read-empty-arrays-or-unset-counts.md

#### UB-67 (update, 2026-10-31). The `BuildPolarBox` NULL surface also crashes the C++ reference from `HealBrep`'s infinite-region ray fire

The NULL `mBA_pSurface` dereference in `SmBezierAux2d::BuildPolarBox` (`source/SMLib/src/SmSurfaceCache.cpp:1184`) is reached on a third path. The update of 2026-10-30 found it from an offset's stitch; this one comes from the healer, on a STEP-imported body.

- **Location:** `SmBezierAux2d::BuildPolarBox` + 981, a virtual call's vtable load (`mov (%rdi),%rax` with `rdi` = 0). The call's last stack argument is a constant NULL, which in `BuildPolarBox` only the root branch passes (`pLastAux->mBA_pSurface->CalculateBoundingBox(..., NULL, NULL)`, `SmSurfaceCache.cpp:1281-1282`). This is the same site as the 2026-10-30 update.
- **Signal:** SIGSEGV, `si_code` 1 (`SEGV_MAPERR`), fault address 0x0: a NULL dereference.
- **Call path:** `SmBezierAux2d::BuildPolarBox` <- `GetPolarBox` <- `SmLSIGlobalSolver::LocalSolve` <- `SmGlobalSolver::SolveBranch` (5 levels) <- `SmGlobalSolver::SolveIt` <- `SmSurface::GlobalLineIntersect` <- `SmTLSPointFace::SolveIt` <- `SmTopologySolver::LocalSolve` <- `SmGlobalSolver::SolveBranch` (5 levels) <- `SmTopologySolver::SolveTrees` <- `SmTopologySolver::BrepPointSolve` <- `SmBrep::RayFire` <- `SmBrep::FindInfiniteRegion` <- `SmBrep::FindAndSetInfiniteRegion` <- `SmHealData::Fix_InfiniteRegion` <- `SmHealData::HealTgts` <- `SmHealData::HealBrep` <- `SmBrep::HealBrep`.
- **Trigger:** one solid of a private STEP assembly. The file is not in the repository and none of its geometry is recorded. Built by the difftest STEP corpus path (`difftest --step-case`: `CreateFacesWithCurves` per face, `StitchIntoSolid`, then `HealBrep`).
  - The body has 75 faces, 170 edges and 97 vertices, and it stitches into a manifold solid that is inside out.
  - The healer's infinite-region fix fires a ray, and the line/face solve of that ray reaches the NULL surface.
- **How found:** the C++ child process of a STEP corpus run exited with SIGSEGV (rc 139). It was rerun alone under gdb, launched by gdb with address randomization off, and the crash reproduced on the first run. The prebuilt reference libraries have no line info, so frames are function + offset; the site is read from the disassembly, as in the 2026-10-30 update.
- **Confirmed:** yes, the crash on this path, reproduced alone. That the NULL is the root aux's `mBA_pSurface` is read from the disassembly.
- **Rust port:** no crash. The Rust side builds, stitches and heals the body. Its volume is negative before the heal and positive after, with the same face, edge and vertex counts, and the healed solid is manifold. UB-67's port note is unchanged.
