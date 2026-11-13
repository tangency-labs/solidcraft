#### UB-14. `NL_PascalTri[18][18]` is indexed past its end
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `source/SMLib/inc/NL_Globals.h:67` (the table); users include `NL_Bezier.cpp:105`, `:118` (elevation / product matrices) and `:5417` `N_BezCrvExtend` (hence `N_BezSrfExtend4D`).
- **What happens:** Degree sums above 17 index past the table.
- **Trigger:** Elevating a degree-10 Bezier by 10.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
