#### WR-154. Discontinuity classifiers select boundary solution slots with V+V offsets
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmCurveClass.cpp:9241`, `:9654`, `:10121-10129`.
- **What happens:** The UV and 3D surface classifiers select the last boundary's solution array using `2 * V_count - 1`, while the later consumer selects its curve using `U_count + V_count - 1`. The volume classifier repeats V in the offsets for its last three boundary slots too. Unequal counts can associate an internal boundary's solutions with a different outer boundary, or omit an outer solution array.
- **Trigger:** different U/V knot or discontinuity counts, with intersections on the affected boundaries.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no numerical reproduction isolated; native unequal-count fixtures are dependency-deferred in Rust before downstream classification.
- **Rust port:** preserves these exact slot expressions; crossing classifications are uncertified until solution insertion is available.
