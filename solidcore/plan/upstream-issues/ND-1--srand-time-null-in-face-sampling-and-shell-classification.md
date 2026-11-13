#### ND-1. `srand(time(NULL))` in face sampling and shell classification
- **Severity:** Nondeterminism
- **Area:**
- **Location:** `SmFace.cpp:21183` `SmFace::GetPointsInFace` (def. `:21167`); `SmShell.cpp:888`, `:905` (in `SmShell::IsInnerShellOf`, def. `:827`).
- **What happens:** The C RNG is reseeded from the clock and used to dither sample parameters / ray directions. Results differ between runs, and the global `rand()` state is clobbered for the whole process.
- **Trigger:** `GetPointsInFace` with `lPointCount > 1`; inner-shell tests on wire shells.
- **How found:** read during porting (static scan of the call-graph closure, `plan/closure.md`).
- **Confirmed:** no.
- **Severity:** nondeterminism.
- **Rust port:** Draws from the seeded `kernel.rng`.
