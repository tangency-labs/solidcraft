# offset.offset_executive_p2 shell_boundary_negative_exactly_two_tol after PR #849 (2026-10-25)

Deferred on main in `RemoveExcessFaces`' `BrepPointSolve` (the face solve's
`smsurf_GetSurfaceCache`). With that solve reaching the original's face caches, the
case passes on the offset B-rep copy/merge stack (#856) alone; after merging main at
`6ad48dd70` (PR #849: `intersect/curve_class/p5.rs` merges paired intervals through
their actual peers) it disagrees in one value: `values.result.geometry[6]` C++
`9.999985000000001e-1`, Rust `9.99999e-1` (rel err 5.0e-7). The case puts the shell
boundary at exactly twice the tolerance, so a classification-interval merge at that
boundary picks a different end point. Suspected cause: #849's peer forwarding in the
interval merge differs from upstream at an exact-tolerance boundary; not analysed here.

Update (2026-10-25): with main merged at the state after #851, #856 and #861, `offset.offset_executive_p2` `shell_boundary_negative_exactly_two_tol` passes again; its known-mismatch entry is removed.
