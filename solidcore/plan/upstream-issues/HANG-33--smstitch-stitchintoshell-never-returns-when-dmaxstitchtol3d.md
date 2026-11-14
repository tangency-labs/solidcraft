#### HANG-33. `SmStitch::StitchIntoShell` never returns when `dMaxStitchTol3d / 100` underflows
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmStitch.cpp:2756`-`2791` (`StitchIntoShell`).
- **What happens:** The loop starts at `dStitchTol3d = dMaxStitchTol3d / 100` and doubles it until it reaches `dMaxStitchTol3d` or no lamina edge is left. When the division underflows to `0`, doubling keeps it `0`, so `0 < dMaxStitchTol3d` holds forever and `DoStitching` runs again and again.
- **Trigger:** `0 < dMaxStitchTol3d` below about `5e-322` (e.g. `1e-323`) on a B-rep that keeps a lamina edge.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** yes (C++ difftest run with the shim guard disabled: `1e-323` on two sheets 0.05 apart was still running at 120 s; `1e-300` returns at once).
- **Severity:** hang.
- **Rust port:** Stops after `MAX_TOL_DOUBLINGS` (2200) doublings with `SolidError::InvalidInput`; the shim refuses the input (`-3`) and the runner applies the same test.
