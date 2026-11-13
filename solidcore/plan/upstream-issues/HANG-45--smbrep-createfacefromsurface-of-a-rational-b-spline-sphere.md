#### HANG-45. `SmBrep::CreateFaceFromSurface` of a rational B-spline sphere of radius about `1e-5` does not return
- **Severity:** Hang
- **Area:**
- **Location:** `SmBrep::CreateFaceFromSurface` -> `SmSurface::LocalCurveIntersect` -> `SmLocalSolveNd::SolveIt` inside `SmGlobalSolver::SolveBranch` (location as recorded by the sweep worker in the FFI guard comment; not re-checked with gdb).
- **What happens:** Building the face of a full rational biquadratic sphere (4 x 8 spans, weights `sqrt(0.5)`) of radius `1e-5` (and in the `1e-5`..`1e-4` range) never returns; `1e-6` and `1e-3` return. Shape-dependent, like UB-201 (small-radius `SmApiCreateSphereNoPole` hangs); may share its cause.
- **Trigger:** `query.api_queries` shim kind 9, radius `1e-5`, domain `[0,1]^2`, any B-rep op (fuzz mutated `op 3 -> 0`).
- **How found:** generated difftest case (`difftest --unit query.api_queries --fuzz 2000`: `fuzz-out/n2000/query.api_queries/fuzz00500.json`, base `rational_kind9_1e-05_north`, mutation `/op = 0`).
- **Confirmed:** yes (no result after 20 s and the retry).
- **Severity:** hang.
- **Rust port:** not reached; the FFI wrapper requires radius `>= 1e-3` for kind-9 B-rep ops (`crates/solidcore-ffi/src/units/query_api_queries.rs:83`).
