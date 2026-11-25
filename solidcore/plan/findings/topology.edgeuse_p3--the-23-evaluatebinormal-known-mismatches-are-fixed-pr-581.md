# topology.edgeuse_p3: the 23 `EvaluateBinormal` known mismatches are fixed (PR #581, 2026-10-23)

Follow-up to "PR #463: 27 Rust-vs-C++ mismatches newly reachable once
`SmSurface::DropCurve` is wired" (topology.edgeuse_p3 group). Cause, as the km-binormal
probe found: `SmBrep::ReadFromFile` builds the B-rep while doing a Boolean operation, so
its healing (`SmVertexuse::ComputeUVPoint` -> `smsurf_GetSurfaceCache`) leaves an
`SmTrimSrfCache` without a tree on each face surface; `EvaluateBinormal`'s
`smsurf_GetSurfaceCache` reuses it, so `SmFace::CreateUVTrimCurves` never runs. The port
models that slot already (`Surface::cache_tree_state`); the `topology.edgeuse_p1` graph
export did not carry it, so the Rust import started with no cache and built the face's
trimmed cache (attaching UV trim curves: state length 44 vs 33). The `topology.edgeuse_p1`
shim now has a `CacheStates` op and the `edgeuse_p3` runner restores the states before
the Rust op. No kernel change. difftest: 4686 pass / 80 deferred / 23 known ->
4709 pass / 80 deferred / 0 known. Other runners that import file B-reps through the
same graph export still start with empty cache slots (`seed_cache_tree_states` is
available to them).
