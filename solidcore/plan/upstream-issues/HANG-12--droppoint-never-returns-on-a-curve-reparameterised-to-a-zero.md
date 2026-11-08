#### HANG-12. `DropPoint` never returns on a curve reparameterised to a zero-length interval
- **Severity:** Hang
- **Area:**
- **Location:** `SmProtoTopology.cpp:1206` `SmEdgeDefinition::Reparameterize` (calls `SmCurve::EditParameterization` with the new domain, unchecked); the loop is in the curve's local point solve (`SmCurve::LocalPointSolve` -> `SmLocalSolve1d::SolveIt` with the `FindPCExtrema` evaluation), reached from `SmCurve::DropPoint`.
- **What happens:** `Reparameterize(SmExtent1d(a, a))` collapses a line to a point (every evaluation returns the same point). A later `DropPoint` on it, e.g. from `SmEdgeDefinition::IsInteriorPoint`, does not return.
- **Trigger:** An edge definition on a bounded `SmLine`, `Reparameterize(SmExtent1d(-2.4753, -2.4753), FALSE)`, then `IsInteriorPoint` on a definition using the same line (any tolerance).
- **How found:** generated difftest case (`topology.proto_topology_p1`).
- **Confirmed:** yes (the C++ reference run did not finish).
- **Severity:** hang.
- **Rust port:** Same loop (the Rust run of the same script did not finish either; the loop belongs to the curve point-solve units). The shim refuses a zero-length `Reparameterize` domain.
