#### UB-80. `SmSurfOfExtrusion::EvaluateSTEP` lets order 5 overrun `sm_CrvDerivs`
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmSurfOfExtrusion.cpp:920`-`922` (`EvaluateSTEP`, def. `:897`: `lMaxDeriv = 5; // 5: MAXDER in NLib`), `:959` (`m_pGenCurve->EvaluateSTEP(dU, lHighestUDeriv, ..)`); `SmNurbsCrv.cpp:1281` `sm_CrvDerivs` (see UB-33).
- **What happens:** The guard allows order 5, but `NL_MAXDER` is 4. A B-spline generator evaluates through `SmBSplineCurve::Evaluate` -> `sm_CrvDerivs`, which writes past its fixed derivative rows: `*** stack smashing detected ***`. Analytic generators (`SmLine`, `SmCircle`, `SmEllipse`) evaluate order 5 fine.
- **Trigger:** `EvaluateSTEP(uv, 5, 5, ..)` on an extrusion of any `SmBSplineCurve` (degrees 1 to 3 tried).
- **How found:** crash in the C++ reference run (`surface.surf_of_extrusion_p1` generated cases).
- **Confirmed:** yes.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** The B-spline evaluation rejects order 5 and `evaluate_step` returns the error. The harness keeps order 5 of a B-spline generator away from the C++.
