#### UB-223. `SmIsoCurve` / `SmProjectedCurve` / `SmBSplineSurface` readers cast unchecked file values to enums [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `source/SMLib/src/SmIsoCurve.cpp:421` (`(SmSurfParamType) lSurfParam`, `SmIsoCurve::ReadFromDB`, def. `:369`); `SmProjectedCurve.cpp:785` (`(SmProjectionType) lProjType`, `SmProjectedCurve::ReadFromDB`, def. `:690`); `SmBSplineSurface.cpp:6515` (`(SmBSplineSurfaceForm) lBSplineSurfaceForm`, reached from the iso curve's nested surface).
- **What happens:** A `ULONG` read from the file is cast to an enum without a range check. Values outside the enum's range of values (`SmSurfParamType`, `SmBSplineSurfaceForm`: 0..=15; `SmProjectionType`: 0..=3) are undefined behaviour; in the reference build the raw value is stored and later read back (`m_eProjType` comes back as `-2147483648` for `2147483648`). In the ASCII branches an extraction from an already-failed stream leaves `lSurfParam`, `bFromLeft` (`SmIsoCurve.cpp:398`, `:400`) and `lProjType` (`SmProjectedCurve.cpp:729`, `:754`) uninitialised before the same cast.
- **Trigger:** `curve.iso_curve` fuzz `fuzz00479` (`/db/sp = 2147483648`), `fuzz01374` (`/db/form = 9223372036854775807`); `curve.projected_curve` fuzz `fuzz00412` (`/ptype = 2147483648`), `fuzz00488` (`/ptype = 4`).
- **How found:** generated difftest case (fuzz sweep, `--fuzz 2000`).
- **Confirmed:** no (no crash; the C++ run kept and returned the out-of-range value).
- **Severity:** crash/UB.
- **Rust port:** Maps unknown values to `SM_SP_UNKNOWN` / `SM_PT_UNKNOWN`; the difftest runners keep out-of-range values away from the C++ side (`curve_iso_curve.rs`, `curve_projected_curve.rs`).
