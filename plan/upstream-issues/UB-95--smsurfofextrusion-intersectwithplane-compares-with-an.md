#### UB-95. `SmSurfOfExtrusion::IntersectWithPlane` compares with an uninitialised first point
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:2348` (`sFirstCrvPt`), `:2454` (`IntersectWithPlane`).
- **What happens:** `sFirstCrvPt` is only set when the first plane intersection solution makes a curve. When it does not (no directrix, or the directrix misses the plane domain), a later solution on a closed generator compares its point with the uninitialised `sFirstCrvPt`, and may then `RemoveAt(0)` from an empty curve array.
- **Trigger:** A plane parallel to the extrusion of a closed generator, bounded so that the directrix at the first hit misses the plane domain while a later one hits it.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** The comparison is false without a first point.
