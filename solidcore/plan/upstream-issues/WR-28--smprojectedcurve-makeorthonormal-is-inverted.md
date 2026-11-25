#### WR-28. `SmProjectedCurve::MakeOrthoNormal` is inverted
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmProjectedCurve.cpp:219`-`221` (`if(bProjInit && bProjZero) { m_sProjVec.Unitize() ; } else { return( SM_ERR ) ; }`), and the same for `m_sAuxData`; `SmProjectedCurve::MakeOrthoNormal` (def. `:215`).
- **What happens:** It unitizes only a zero vector (giving `(0,0,1)`) and returns `SM_ERR` for every non-zero vector, leaving it un-normalised. A non-unit view normal gives a skewed projection.
- **Trigger:** A projected curve with view vector `(0, 0, 2)`.
- **How found:** generated difftest case (both sides agree on the skew).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
