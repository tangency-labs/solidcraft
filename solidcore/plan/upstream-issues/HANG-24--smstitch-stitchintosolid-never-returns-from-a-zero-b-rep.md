#### HANG-24. `SmStitch::StitchIntoSolid` never returns from a zero B-rep tolerance
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmStitch.cpp:2676`-`2711` (`StitchIntoSolid`).
- **What happens:** The loop starts at `GetTolerance() / 10` and doubles it while it is below `1/200` of the bounding box diagonal and lamina edges remain. A tolerance of `0` stays `0` (a negative one only grows more negative), so `DoStitching` runs forever. A positive tolerance always ends: against an infinite diagonal it doubles to infinity in about 2100 steps and `inf < inf` is false; a NaN tolerance or diagonal fails the loop test at once.
- **Trigger:** A B-rep tolerance `<= 0` with a positive box diagonal and a lamina edge that is never stitched.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection; the shim refuses a tolerance `<= 0` below the diagonal test, `-3`). The difftest block cannot set the B-rep tolerance, so no fixture reaches the guard.
- **Severity:** hang.
- **Rust port:** Stops after `MAX_TOL_DOUBLINGS` (2200) doublings with `SolidError::InvalidInput`.
