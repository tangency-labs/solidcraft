#### WR-9. `SmPolarBox::AddVector3d` relabels a region as an arc without transposing
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmPolarBox.cpp:696`, `SmPolarBox::AddVector3d` (`SM_PS_REGION` case).
- **What happens:** When the x range collapses, the basis is rotated and the state becomes `SM_PS_ARC`, but the domain is re-set to its own bounds. The arc's span stays in y while arc code reads x.
- **Trigger:** A region box whose x range collapses below `SM_EFF_ZERO_SQRT`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
