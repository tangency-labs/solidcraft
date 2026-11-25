Updates: UB-33

#### UB-33 (update, 2026-11-01). `der == 5` is a valid request; the port now evaluates it

`SmBSplineCurve::Evaluate` with 5 derivatives reaches `sm_CrvDerivs(der = 5)`, which `der > GW_MAX_DERIV` accepts, but `BD_STACK`/`BD` (and `ND_STACK`, `ND`, `tri_STACK`, `tri` in `sm_CrvRationalBasisDerivs`) hold `GW_MAX_DERIV` (5) rows for rows 0 to 5: row 5 is written past them. Fork fix: tangency-labs/smlib#79 commit 2391302 (arrays of `GW_MAX_DERIV+1` rows). Confirmed in the pinned source by reading (SmNurbsCrv.cpp:1193-1197, 1281-1290); no ASan run, because the C++ result at `der == 5` is undefined and cannot be compared.

solidcore: the port was already memory safe (it returned `NlibCode::MaxDerivative` for `der >= 5`); it now holds `MAX_DERIV + 1` rows and evaluates `der == 5` (and `BSplineCurve::evaluate` with 5), unconditionally, since the upstream result is undefined. `der > 5` still errors, as upstream. The FFI guard (`der == 5` rejected in `nurbs_nurbs_crv` and `nurbs_b_spline_curve_p1`) stays: it rejects exactly the overrunning input. Regression test: `nurbs::nurbs_crv::tests::derivs_up_to_max_deriv_fit_their_rows`.

UB-80 (`SmSurfOfExtrusion::EvaluateSTEP` order 5 on a B-spline generator) is the same overrun through `SmBSplineCurve::Evaluate`; the port now evaluates it too (`surface::surf_of_extrusion::p1::tests::canonical_perpendicular_extrusion`). The harness guard for that case stays.
