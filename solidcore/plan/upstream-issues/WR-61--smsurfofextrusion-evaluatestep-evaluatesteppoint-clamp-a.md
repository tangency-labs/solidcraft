#### WR-61. `SmSurfOfExtrusion::EvaluateSTEP` / `EvaluateSTEPPoint` clamp a copy they never use
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmSurfOfExtrusion.cpp:941`-`955` (`EvaluateSTEP`: clamps `sUV`, evaluates `crUV.x`, `crUV.y`); `:1015`-`1024` (`EvaluateSTEPPoint`, same).
- **What happens:** "Per SMLib practice clamp input point to surface domain" clamps a local copy, but both evaluate the caller's unclamped parameters. With out-of-bounds evaluation disabled, a v outside the STEP domain still extrapolates along the extrusion, and an analytic generator is evaluated outside its domain.
- **Trigger:** `EvaluateSTEPPoint((u, vmax + 1))` with out-of-bounds evaluation off.
- **How found:** read during porting; generated difftest cases evaluate outside the domain on both sides.
- **Confirmed:** yes (`surface.surf_of_extrusion_p1` generated `eval_step` cases outside the domain agree with the unclamped port).
- **Severity:** wrong result.
- **Rust port:** Faithful (evaluates the unclamped parameters).
