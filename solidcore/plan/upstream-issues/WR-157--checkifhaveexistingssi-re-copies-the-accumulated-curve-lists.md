#### WR-157. `CheckIfHaveExistingSSI` re-copies the accumulated curve lists on every match
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:184-249`, `SmTopologyIntersector::CheckIfHaveExistingSSI`.
- **What happens:** For each table entry matching the surface pair the function appends the entry's arrays to the outputs and then runs `for (jj=0; jj<r3DCurves.GetSize(); jj++)` over the *whole* accumulated output, replacing every 3D and UV B-spline curve by a fresh copy. With two matching entries the first entry's copies are copied once more and those first copies leak; with one entry the behaviour is as intended. A 3D curve that is not an `SmBSplineCurve` is `SER(SM_ERR)` after the output arrays were already partly filled with copies (also leaked by a caller that treats the status as fatal).
- **Trigger:** `RegisterExistingSSI` twice for the same `(pBrepSurface, pOtherSurface)`, then `CheckIfHaveExistingSSI` for that pair (fixture `analytic_tsurf_pair_table`, `fixtures/boolean.topology_intersector_p1/upstream.json`).
- **How found:** read during porting (`boolean.topology_intersector_p1`, PR #146); the difftest runs it in the release build.
- **Confirmed:** no. The fixture runs the path in the C++ build and the doubly copied curves compare equal to the originals, so neither the extra copy nor the leak is observable in a result; both follow from source inspection.
- **Severity:** memory leak / wasted work; results unaffected.
- **Rust port:** Faithful: the re-copies go to the caller's curve store and the earlier handles stay unreferenced; the non-B-spline case reports the `SM_ERR` in `ExistingSsi::status` with the outputs filled so far.
