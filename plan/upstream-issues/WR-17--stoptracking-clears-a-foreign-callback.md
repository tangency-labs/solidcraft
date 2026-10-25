#### WR-17. `StopTracking` clears a foreign callback
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `SmTrackTopologyChanges.cpp:512`, `SmTrackTopologyChanges::StopTracking`.
- **What happens:** If the slot was empty at start, `StopTracking` clears whatever callback is installed now, including one installed by someone else since.
- **Trigger:** Start tracker A, install callback B, stop A: B is removed.
- **How found:** adversarial / edge-case fixture `track_stop_clears_foreign`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
