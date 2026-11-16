#### UB-282. The NURB of a surface of extrusion with an empty V interval faults when evaluated by SmSurface::IsClosed
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:**
  - `SmSurface::IsClosed` (`source/SMLib/src/SmSurface.cpp:22157`), evaluating through `SmBSplineSurface::EvaluatePoint` (`source/SMLib/src/SmBSplineSurfaceNLib.cpp:8684`) into `N_SrfEvalPt` (`source/SMLib/src/NL_SrfNurb.cpp:1533`), where it faults.
  - The NURB comes from `SmSurfOfExtrusion::MakeNurb` (`source/SMLib/src/SmSurfOfExtrusion.cpp:3668`).
- **What happens:** with the V interval empty, or empty to 1e-9 with the parameters swapped, `MakeNurb` leaves a NURB that `N_SrfEvalPt` cannot evaluate. The base checks of `SmSurface::AssertValid` (any level but `SM_LEVEL_GIVEN`) call `IsClosed` on it, which faults.
- **Trigger:** an `SmSurfOfExtrusion` whose V interval is empty, or empty to 1e-9 with the swap flag set. Then `MakeNurb`, then `IsClosed` in either direction, as the base surface checks do.
- **How found:** `surface.surface_assert_valid` harness. The FFI guard refused "MakeNurb of a collapsed extrusion" for 86 generated inputs. An isolated run of each (one process per input) sorted them: 23 faulted, 13 hit UB-273's unwritten-continuity read (see its addendum of 2026-10-31), and 50 ran to completion.
- **Confirmed:** yes, in an isolated run, with frame names only. All 23 inputs fault with SIGSEGV (none hang), with the same stack: `N_SrfEvalPt`, then `SmBSplineSurface::EvaluatePoint`, then `SmSurface::IsClosed`.
