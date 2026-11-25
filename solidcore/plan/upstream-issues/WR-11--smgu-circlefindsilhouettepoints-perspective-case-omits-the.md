#### WR-11. `smgu_CircleFindSilhouettePoints` perspective case omits the centre
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmGeomUtility.cpp:2185`, `smgu_CircleFindSilhouettePoints`.
- **What happens:** The perspective case returns rotated unit directions times the radius, without adding the circle centre. The parallel case adds it.
- **Trigger:** A circle centred at `(10,0,0)` viewed in perspective.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
