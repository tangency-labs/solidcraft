#### DOC-2. `N_GetCompatibleKnotVector` doc says multiplicities are not carried over
- **Severity:** Docs
- **Area:**
- **Location:** `NL_BasisBasic.cpp:2339` (`N_GetCompatibleKnotVector` doc comment); behaviour in `:2483` `N_GetCompatibleKnotVectorToTol`.
- **What happens:** Each distinct knot gets the highest multiplicity it has in any vector, unlike the doc says.
- **Trigger:** Vectors `[0,0,.5,1,1]` and `[0,0,.5,.5,1,1]`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** Faithful to the code.
