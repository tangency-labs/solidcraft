# offset.offset_executive_p2 gen0235: imprint edge definition without its source topology (2026-10-25)

`gen0235` (solid offset, `offset_distance = -2e-5`, merge results, extend convex) was
deferred on main in `RemoveExcessFaces`' `pOriginal->BrepPointSolve` (the face solve's
`smsurf_GetSurfaceCache`). That solve now reaches the original's face caches (built
on the original between attempts, `RayPreparation`), and the case runs further: C++
returns `SM_SUCCESS` with a result B-rep, Rust fails with `SM_ERR_NULL_POINTER` from
`ProtoTopologyManager::imprint_edge_definition` (one of the edge definition's source
faces/edges is missing) during the merge Boolean, so `ints.status` is 1000 vs 1006 and
the result projections are missing on the Rust side. The excess-face solves themselves
succeed (one solution each). Suspected cause: a divergence in the Boolean's
intersection classification upstream of the imprint, not the face-cache wiring; not
analysed further here.
