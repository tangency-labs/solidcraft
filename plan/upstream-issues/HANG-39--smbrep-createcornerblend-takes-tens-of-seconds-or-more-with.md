#### HANG-39. `SmBrep::CreateCornerBlend` takes tens of seconds or more with a tangency tolerance below what doubles resolve
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmBrep.cpp:2488`-`2775` -> `SmBSplineSurface::CreateCornerBlend` -> `SmBSplineSurface::CreateCoonsPatch` -> `N_MakeSrfsCompatible` -> `N_SrfInsertKnots` (where a hung run sits).
- **What happens:** The cross-tangent curves are approximated to `dTangencyTolRadians` (`SmTangentField::ApproximateCurve`), which refines towards a tolerance no double computation meets; the over-refined curves then make the Coons patch's surface compatibility insert knots for tens of seconds. Measured on `plane_circ.smb` (three edge uses): `1e-23` returns in 0.4 s, `1e-24` in 1 s, `1e-25` in 22 s, `1e-26` in 47 s; `1e-27` and below, `0` and negative tolerances do not return within 60 s.
- **Trigger:** a corner blend with a tangency tolerance of `0` (difftest fuzz program of `topology.brep_p1`), negative, or below about `1e-24`.
- **How found:** difftest fuzzing of `topology.brep_p1` (the follow-up to #145); located with gdb on the running C++ reference.
- **Confirmed:** yes (C++ reference runs, timed).
- **Severity:** hang (unbounded slowness). A caller argument: no [PSIRT].
- **Rust port:** unchanged. The `topology.brep_p1` shim rejects a tangency tolerance below `1e-20` (four decades of margin above the measured knee) only where its replay of the call reaches the cross-tangent approximation; earlier statuses stay compared.
