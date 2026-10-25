#### WR-100. `SmSurface::DropCurve` tests the domain's u length for the v degeneracy
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfaceDropLib.cpp:4250` `SmSurface::DropCurve`: `bDegenSurfV = sUVDomain.XLength() < SM_EFF_ZERO` (the line above sets `bDegenSurfU` the same way).
- **What happens:** A drop domain degenerate in v is not reported (`SM_ERR_DEGENERATE_SURFACE`, `SM_CTC_DEGENSURFACE_FAIL`) and the drop goes on; `m_bDegenSurfV` of the failure record copies the u test.
- **Trigger:** Only a natural domain degenerate in v reaches it (a given domain shorter than `SM_EFF_ZERO` in either direction is replaced by the natural one), e.g. a plane built over a domain of zero height.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`Surface::drop_curve_base`).
