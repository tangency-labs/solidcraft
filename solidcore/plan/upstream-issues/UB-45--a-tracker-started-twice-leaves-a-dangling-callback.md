#### UB-45. A tracker started twice leaves a dangling callback
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmTrackTopologyChanges.cpp:434` `StartTracking`, `:512` `StopTracking`.
- **What happens:** The second `StartTracking` stores the tracker's own callback as "previous". `StopTracking` (and the destructor) then does nothing, and the context keeps a dangling `SmNotifyCallback*`. The next `Notify` calls into a destroyed object.
- **Trigger:** `t.StartTracking(); t.StartTracking();` destroy `t`, then any notify.
- **How found:** read during porting.
- **Confirmed:** no. The shim skips destroying a tracker in that state.
- **Severity:** crash/UB (use after free).
- **Rust port:** Identifies callbacks by key, so the case cannot arise.
