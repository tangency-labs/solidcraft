#### ND-259. `SmOffsetExecutive::DoSolidOffset`: degenerate-wall vertex positions depend on heap addresses
- **Severity:** Nondeterminism
- **Area:** Surface
- **Location:** `SmRelation` / `SmMapPtrToPtrs` (`SmMapPtrToPtr.h:115`, `std::unordered_map` keyed by object address) iterated in `SmOffsetExecutive::DoSolidOffset` (`SmOffsetExecutive.cpp:1419`, `m_vEUToEU.GetAllFirsts`).
- **Trigger:** `SmOffsetExecutive::ShellBrep` of a plane sheet (2 x 3 at z = 1, tolerance 1e-6) by exactly -2*tol with extend, merge and create-solid set. The side walls are degenerate and their vertices are merged by midpoint averaging in an order that follows the map's bucket order, so the result's vertex positions depend on the process heap layout: max z is 0.999999 or 0.9999985 in different processes of the same binary inputs.
- **How found:** the `offset.offset_executive_p2` difftest case `shell_boundary_negative_exactly_two_tol` failed in some CI runs only; reproduced locally by pre-fragmenting the heap with an `LD_PRELOAD` constructor (see plan/findings.md, "offset.offset_executive_p2: exact-2tol shell vertex bounds follow native heap layout").
- **Confirmed:** yes, on the C++ reference (Ubuntu 24.04 x86_64 prebuilt, glibc 2.39 on CI and 2.43 locally): the same native call gives either value depending only on prior heap allocations.
- **Severity:** run-to-run nondeterministic geometry (within the merge tolerance) for degenerate offsets; results are not reproducible across builds or processes.
- **Fix:** iterate the relations in a deterministic order (insertion order, or sorted by a stable object id) wherever the order reaches geometry.
- **Rust port:** `geom::tol::Relation` iterates sorted handles, so the port is deterministic; the difftest omits only that case's result vertex bounds.
