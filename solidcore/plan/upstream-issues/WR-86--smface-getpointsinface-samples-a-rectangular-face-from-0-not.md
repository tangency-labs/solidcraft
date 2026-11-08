#### WR-86. `SmFace::GetPointsInFace` samples a rectangular face from `0`, not from its domain
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmFace.cpp:21206`-`21230`, `SmFace::GetPointsInFace` (def. `:21167`).
- **What happens:** On a face with `GetRectangularTrim() == TRUE` the sample points are `(k * dUInc, j * dVInc + dither)` with `dUInc` / `dVInc` fractions of the UV domain's lengths: the domain minimum is never added, so a face whose domain does not start at `(0, 0)` gets points outside it (and the positions and normals of those points). `dUInc` is also divided by `min(2 * count, 30) + 1` before the rectangular branch resets the column count to `min(count, 30)`, so the columns do not span the domain.
- **Trigger:** `GetPointsInFace(1, ..)` on a rectangularly trimmed face with UV domain `[3, 4] x [-7, -6.5]` returns `uv = (1/3, 0.25)`.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes, by the C++ build (fixture `points_dom_offset_1` of `topology.face_p2`).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`topology::face::p2::points_in_face`).
