#### WR-191. `SmMerge::ManifoldBoolean` leaves both contexts in boolean mode when it fails
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmMerge.cpp:2551`-`2552` (`SmMerge::ManifoldBoolean` sets `SetDoingBoolean(TRUE)` on the B-rep's and the other B-rep's contexts); they are cleared only at the three success returns (`:3114`-`3115`, `:4148`-`4149`, `:5095`-`5096`). Every error return in between (`SER` at `:2616`, `:2628`, `:2661`, `:3023` and later, `SER(SM_ERR)` at `:3394`, `:3621`) leaves them set.
- **What happens:** After a failed boolean, every later operation on those contexts runs in boolean mode (which delays surface cache construction, among other effects), until something resets the flag.
- **Trigger:** the static `SmPrimitiveCreation::CreateSweepAlongPlanarPath` (tolerance `1e-7`, both caps, profile centred on the path) of a circle of radius `1e-3` at the origin along the lines `(0,0,0)-(0,0,0.01)` and `(0,0,0.01)-(0.01,0,0.01)`: it returns `SM_ERR` with the context's doing-boolean flag set, and the next primitive on that context reports it still set (generated case `gen0092` of `primitive.primitive_creation_p2`, which made the three cases after it differ).
- **How found:** generated difftest case (`primitive.primitive_creation_p2`), then the same inputs in sequence in one process.
- **Confirmed:** yes (the flag is set after the failing sweep and after the following, unrelated primitive). That the flag comes from `ManifoldBoolean` is from source reading: it is the only unscoped set on that path without a restore (the `SmBrep` and `SmEdge` destructors save and restore it, the primitive entry points use `SmTemporaryChangeValue`).
- **Severity:** wrong result (state leaks into later calls).
- **Rust port:** The `ManifoldBoolean` port (`boolean.merge_p4`) keeps the same exits. The `primitive.primitive_creation_p2` shim clears the flag before each case.
