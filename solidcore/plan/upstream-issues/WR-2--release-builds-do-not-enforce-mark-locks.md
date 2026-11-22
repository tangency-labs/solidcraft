#### WR-2. Release builds do not enforce mark locks
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmContext.cpp:331` `SmContext::NewMark` (and `LockMark` / `UnlockMark`).
- **What happens:** The lock checks are `SM_ASSERT*` only, so in release builds locked marks can be advanced and double locks are silent.
- **Trigger:** `NewMark` on a locked mark type.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (release-only).
- **Rust port:** Drops the assertions (matches release).
