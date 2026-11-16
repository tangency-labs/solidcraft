#### UB-122. `SmCurveClassification::MergeInterval` dereferences an unresolved start vertex
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmCurveClass.cpp:17300-17347` (UNKNOWN start recovery), `:17650-17679` (unchecked `pStartV`).
- **What happens:** An UNKNOWN start remains without an object when the interior is EDGE or VERTEX, or when projection onto a FACE interior fails. Full nondegenerate merging still casts that object to `SmVertex*` and dereferences it while checking its point or obtaining its B-rep.
- **Trigger:** Unit line, interval `[0,1]`, classifications UNKNOWN/EDGE/UNKNOWN, full merge and either fillet flag. A stored start point `(10,10,10)` with UNKNOWN/FACE/UNKNOWN on the fixture's unit box also fails to project and leaves the start unresolved.
- **How found:** PR #138 port audit: 6,860 isolated merge probes found 64 native SIGSEGVs; follow-up stored-point probes exposed failed FACE projection. Regression tests cover both causes, successful FACE projection, vertices-only merging and constant-curve early returns.
- **Confirmed:** yes, native subprocesses terminated with SIGSEGV before the oracle preflight was added.
- **Severity:** crash.
- **Rust port:** Topology mutation remains explicitly deferred at p2. The native oracle rejects unresolved-start full merges, preserving degenerate and vertices-only paths; rejected fixtures are not differential passes.
